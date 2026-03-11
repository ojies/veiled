//! Per-verifier nullifier derivation using HKDF (RFC 5869).
//!
//! In the ASC protocol, each master identity produces L different nullifiers
//! (one per service provider / name) via HKDF with the name as salt.
//! This ensures:
//! - Same master secret + different name → different unlinkable nullifiers
//! - Different master secrets + same name → different nullifiers
//! - Deterministic: same (master_secret, name) always gives the same nullifier

use hkdf::Hkdf;
use k256::{
    elliptic_curve::{group::GroupEncoding, ops::Reduce},
    ProjectivePoint, Scalar, U256,
};
use sha2::Sha256;

use crate::core::types::{MasterSecret, Name, Nullifier};

/// Derive a nullifier for a specific service provider (name).
///
/// Uses HKDF-SHA256:
/// - Extract: `PRK = HKDF-Extract(salt = name, IKM = master_secret)`
/// - Expand:  `nullifier = HKDF-Expand(PRK, info = "CRS-ASC-nullifier", len = 32)`
///
/// The name `v_l` acts as the HKDF salt, binding each nullifier
/// to a specific service provider. This is the per-verifier nullifier
/// derivation from the ASC specification.
pub fn derive_nullifier(master_secret: &MasterSecret, name: &Name) -> Nullifier {
    let hk = Hkdf::<Sha256>::new(
        Some(name.as_str().as_bytes()), // salt = v_l
        &master_secret.0,               // IKM = master secret
    );
    let mut output = [0u8; 32];
    hk.expand(b"CRS-ASC-nullifier", &mut output)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    Nullifier(output)
}

/// Derive ALL L nullifiers for a master secret given a list of names.
///
/// Returns a `Vec<Nullifier>` of length `names.len()`, where
/// `result[i]` is the nullifier for `names[i]`.
pub fn derive_all_nullifiers(master_secret: &MasterSecret, names: &[Name]) -> Vec<Nullifier> {
    names
        .iter()
        .map(|name| derive_nullifier(master_secret, name))
        .collect()
}

// ── scalar helper ────────────────────────────────────────────────────────────

fn bytes_to_scalar(b: &[u8; 32]) -> Scalar {
    Scalar::reduce(U256::from_be_slice(b))
}

// ── public nullifier (group element) ─────────────────────────────────────────

/// Compute the public nullifier `nul_l = s_l · g` (a group element).
///
/// Where `s_l = HKDF(sk, v_l)` is the raw scalar derived by [`derive_nullifier`].
/// The `g` parameter MUST be the CRS base generator — using the standard
/// secp256k1 generator would break the commitment scheme.
///
/// The public nullifier serves double duty:
/// - A **Sybil-resistance token** (unique per master identity per service)
/// - A **public authentication key** (the user can prove knowledge of s_l)
///
/// Returns a 33-byte compressed secp256k1 point.
pub fn derive_public_nullifier(
    master_secret: &MasterSecret,
    name: &Name,
    g: &ProjectivePoint,
) -> [u8; 33] {
    let nul = derive_nullifier(master_secret, name);
    let s = bytes_to_scalar(&nul.0);
    let point = *g * s;
    point.to_affine().to_bytes().into()
}

/// Derive ALL L public nullifiers as compressed 33-byte points.
///
/// `result[i]` = `s_i · g` where `s_i = HKDF(sk, names[i])`.
/// Uses the CRS base generator `g`.
pub fn derive_all_public_nullifiers(
    master_secret: &MasterSecret,
    names: &[Name],
    g: &ProjectivePoint,
) -> Vec<[u8; 33]> {
    names
        .iter()
        .map(|name| derive_public_nullifier(master_secret, name, g))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::{
        elliptic_curve::hash2curve::{ExpandMsgXmd, GroupDigest},
        Secp256k1,
    };

    fn sample_secret() -> MasterSecret {
        MasterSecret([0x42u8; 32])
    }

    /// CRS base generator g for tests.
    fn crs_g() -> ProjectivePoint {
        Secp256k1::hash_from_bytes::<ExpandMsgXmd<sha2::Sha256>>(
            &[b"CRS-ASC-generator-0"],
            &[b"CRS-ASC-v1"],
        )
        .expect("hash_to_curve never fails")
    }

    #[test]
    fn deterministic() {
        let s = sample_secret();
        let n = Name::new("twitter.com");
        assert_eq!(derive_nullifier(&s, &n), derive_nullifier(&s, &n));
    }

    #[test]
    fn different_services_give_different_nullifiers() {
        let s = sample_secret();
        let n1 = derive_nullifier(&s, &Name::new("twitter.com"));
        let n2 = derive_nullifier(&s, &Name::new("github.com"));
        assert_ne!(n1, n2);
    }

    #[test]
    fn different_secrets_give_different_nullifiers() {
        let s1 = MasterSecret([0x01u8; 32]);
        let s2 = MasterSecret([0x02u8; 32]);
        let name = Name::new("twitter.com");
        assert_ne!(derive_nullifier(&s1, &name), derive_nullifier(&s2, &name));
    }

    #[test]
    fn output_is_32_bytes() {
        let nul = derive_nullifier(&sample_secret(), &Name::new("test"));
        assert_eq!(nul.as_bytes().len(), 32);
    }

    #[test]
    fn derive_all_returns_correct_count() {
        let s = sample_secret();
        let names: Vec<Name> = vec![Name::new("a"), Name::new("b"), Name::new("c")];
        let nullifiers = derive_all_nullifiers(&s, &names);
        assert_eq!(nullifiers.len(), 3);
    }

    #[test]
    fn derive_all_matches_individual() {
        let s = sample_secret();
        let names: Vec<Name> = vec![Name::new("twitter.com"), Name::new("github.com")];
        let all = derive_all_nullifiers(&s, &names);
        assert_eq!(all[0], derive_nullifier(&s, &names[0]));
        assert_eq!(all[1], derive_nullifier(&s, &names[1]));
    }

    #[test]
    fn all_nullifiers_are_unique() {
        let s = sample_secret();
        let names: Vec<Name> = (0..10).map(|i| Name::new(format!("service-{i}"))).collect();
        let nullifiers = derive_all_nullifiers(&s, &names);
        let unique: std::collections::HashSet<_> = nullifiers.iter().map(|n| n.0).collect();
        assert_eq!(unique.len(), 10);
    }

    // ── public nullifier tests ──────────────────────────────────────────────

    #[test]
    fn public_nullifier_is_33_bytes() {
        let g = crs_g();
        let pn = derive_public_nullifier(&sample_secret(), &Name::new("twitter.com"), &g);
        assert_eq!(pn.len(), 33);
        assert!(
            pn[0] == 0x02 || pn[0] == 0x03,
            "must be valid SEC1 compressed"
        );
    }

    #[test]
    fn public_nullifier_deterministic() {
        let s = sample_secret();
        let n = Name::new("twitter.com");
        let g = crs_g();
        assert_eq!(
            derive_public_nullifier(&s, &n, &g),
            derive_public_nullifier(&s, &n, &g)
        );
    }

    #[test]
    fn different_services_give_different_public_nullifiers() {
        let s = sample_secret();
        let g = crs_g();
        let pn1 = derive_public_nullifier(&s, &Name::new("twitter.com"), &g);
        let pn2 = derive_public_nullifier(&s, &Name::new("github.com"), &g);
        assert_ne!(pn1, pn2);
    }

    #[test]
    fn public_nullifiers_all_unique() {
        let s = sample_secret();
        let g = crs_g();
        let names: Vec<Name> = (0..10).map(|i| Name::new(format!("service-{i}"))).collect();
        let pns = derive_all_public_nullifiers(&s, &names, &g);
        assert_eq!(pns.len(), 10);
        let unique: std::collections::HashSet<_> = pns.iter().map(|p| *p).collect();
        assert_eq!(unique.len(), 10);
    }

    #[test]
    fn derive_all_public_matches_individual() {
        let s = sample_secret();
        let g = crs_g();
        let names = vec![Name::new("a"), Name::new("b")];
        let all = derive_all_public_nullifiers(&s, &names, &g);
        assert_eq!(all[0], derive_public_nullifier(&s, &names[0], &g));
        assert_eq!(all[1], derive_public_nullifier(&s, &names[1], &g));
    }
}
