//! Per-verifier nullifier derivation using HKDF (RFC 5869).
//!
//! In the ASC protocol, each master identity produces L different nullifiers
//! (one per service provider) via HKDF with the service name as salt.
//! This ensures:
//! - Same master secret + different service → different unlinkable nullifiers
//! - Different master secrets + same service → different nullifiers
//! - Deterministic: same (master_secret, service_name) always gives the same nullifier

use hkdf::Hkdf;
use k256::{
    ProjectivePoint,
    elliptic_curve::{group::GroupEncoding, ops::Reduce},
    Scalar, U256,
};
use sha2::Sha256;

use crate::types::{MasterSecret, Name, Nullifier};

/// Derive a nullifier for a specific service provider.
///
/// Uses HKDF-SHA256:
/// - Extract: `PRK = HKDF-Extract(salt = service_name, IKM = master_secret)`
/// - Expand:  `nullifier = HKDF-Expand(PRK, info = "CRS-ASC-nullifier", len = 32)`
///
/// The service name `v_l` acts as the HKDF salt, binding each nullifier
/// to a specific service provider. This is the per-verifier nullifier
/// derivation from the ASC specification.
pub fn derive_nullifier(master_secret: &MasterSecret, service_name: &Name) -> Nullifier {
    let hk = Hkdf::<Sha256>::new(
        Some(service_name.as_str().as_bytes()), // salt = v_l
        &master_secret.0,                        // IKM = master secret
    );
    let mut output = [0u8; 32];
    hk.expand(b"CRS-ASC-nullifier", &mut output)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    Nullifier(output)
}

/// Derive ALL L nullifiers for a master secret given a list of service names.
///
/// Returns a `Vec<Nullifier>` of length `service_names.len()`, where
/// `result[i]` is the nullifier for `service_names[i]`.
pub fn derive_all_nullifiers(
    master_secret: &MasterSecret,
    service_names: &[Name],
) -> Vec<Nullifier> {
    service_names
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
/// The public nullifier serves double duty:
/// - A **Sybil-resistance token** (unique per master identity per service)
/// - A **public authentication key** (the user can prove knowledge of s_l)
///
/// Returns a 33-byte compressed secp256k1 point.
pub fn derive_public_nullifier(master_secret: &MasterSecret, service_name: &Name) -> [u8; 33] {
    let nul = derive_nullifier(master_secret, service_name);
    let s = bytes_to_scalar(&nul.0);
    let point = ProjectivePoint::GENERATOR * s;
    point.to_affine().to_bytes().into()
}

/// Derive ALL L public nullifiers as compressed 33-byte points.
///
/// `result[i]` = `s_i · g` where `s_i = HKDF(sk, service_names[i])`.
pub fn derive_all_public_nullifiers(
    master_secret: &MasterSecret,
    service_names: &[Name],
) -> Vec<[u8; 33]> {
    service_names
        .iter()
        .map(|name| derive_public_nullifier(master_secret, name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_secret() -> MasterSecret {
        MasterSecret([0x42u8; 32])
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
        let names: Vec<Name> = vec![
            Name::new("a"),
            Name::new("b"),
            Name::new("c"),
        ];
        let nullifiers = derive_all_nullifiers(&s, &names);
        assert_eq!(nullifiers.len(), 3);
    }

    #[test]
    fn derive_all_matches_individual() {
        let s = sample_secret();
        let names: Vec<Name> = vec![
            Name::new("twitter.com"),
            Name::new("github.com"),
        ];
        let all = derive_all_nullifiers(&s, &names);
        assert_eq!(all[0], derive_nullifier(&s, &names[0]));
        assert_eq!(all[1], derive_nullifier(&s, &names[1]));
    }

    #[test]
    fn all_nullifiers_are_unique() {
        let s = sample_secret();
        let names: Vec<Name> = (0..10)
            .map(|i| Name::new(format!("service-{i}")))
            .collect();
        let nullifiers = derive_all_nullifiers(&s, &names);
        let unique: std::collections::HashSet<_> = nullifiers.iter().map(|n| n.0).collect();
        assert_eq!(unique.len(), 10);
    }

    // ── public nullifier tests ──────────────────────────────────────────────

    #[test]
    fn public_nullifier_is_33_bytes() {
        let pn = derive_public_nullifier(&sample_secret(), &Name::new("twitter.com"));
        assert_eq!(pn.len(), 33);
        assert!(pn[0] == 0x02 || pn[0] == 0x03, "must be valid SEC1 compressed");
    }

    #[test]
    fn public_nullifier_deterministic() {
        let s = sample_secret();
        let n = Name::new("twitter.com");
        assert_eq!(
            derive_public_nullifier(&s, &n),
            derive_public_nullifier(&s, &n)
        );
    }

    #[test]
    fn different_services_give_different_public_nullifiers() {
        let s = sample_secret();
        let pn1 = derive_public_nullifier(&s, &Name::new("twitter.com"));
        let pn2 = derive_public_nullifier(&s, &Name::new("github.com"));
        assert_ne!(pn1, pn2);
    }

    #[test]
    fn public_nullifiers_all_unique() {
        let s = sample_secret();
        let names: Vec<Name> = (0..10)
            .map(|i| Name::new(format!("service-{i}")))
            .collect();
        let pns = derive_all_public_nullifiers(&s, &names);
        assert_eq!(pns.len(), 10);
        let unique: std::collections::HashSet<_> = pns.iter().map(|p| *p).collect();
        assert_eq!(unique.len(), 10);
    }

    #[test]
    fn derive_all_public_matches_individual() {
        let s = sample_secret();
        let names = vec![Name::new("a"), Name::new("b")];
        let all = derive_all_public_nullifiers(&s, &names);
        assert_eq!(all[0], derive_public_nullifier(&s, &names[0]));
        assert_eq!(all[1], derive_public_nullifier(&s, &names[1]));
    }
}
