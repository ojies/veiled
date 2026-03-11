//! Child credential derivation for service registration (Phase 3).
//!
//! From the master credential's child randomness `r`, derives per-service
//! authentication keys and pseudonyms:
//!
//! ```text
//! csk_l = HKDF(r, salt=v_l, info="CRS-ASC-child-secret-key")   ← child auth secret
//! ϕ     = csk_l · g                                              ← pseudonym
//! ```
//!
//! The pseudonym `ϕ` is the user's public identity at service `l`. It is
//! unlinkable across services because `csk_l` is derived with a different
//! salt for each service.

use hkdf::Hkdf;
use k256::{
    elliptic_curve::{group::GroupEncoding, ops::Reduce},
    ProjectivePoint, Scalar, U256,
};
use sha2::Sha256;

use crate::core::types::{ChildRandomness, Name};

fn bytes_to_scalar(b: &[u8; 32]) -> Scalar {
    Scalar::reduce(U256::from_be_slice(b))
}

/// Derive the child secret key for a specific service provider.
///
/// ```text
/// csk_l = HKDF(IKM = r, salt = v_l, info = "CRS-ASC-child-secret-key")
/// ```
///
/// Returns a 32-byte scalar used as the authentication secret for service `l`.
pub fn derive_child_secret_key(child_randomness: &ChildRandomness, name: &Name) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(
        Some(name.as_str().as_bytes()), // salt = v_l
        &child_randomness.0,            // IKM = r
    );
    let mut output = [0u8; 32];
    hk.expand(b"CRS-ASC-child-secret-key", &mut output)
        .expect("32 bytes is valid for HKDF-SHA256");
    output
}

/// Derive the pseudonym `ϕ = csk_l · g` for a specific service provider.
///
/// The pseudonym is the user's public identity at service `l`. It is a
/// 33-byte compressed secp256k1 point using the CRS base generator `g`.
///
/// ```text
/// csk_l = HKDF(r, salt=v_l, info="CRS-ASC-child-secret-key")
/// ϕ     = csk_l · g
/// ```
pub fn derive_pseudonym(
    child_randomness: &ChildRandomness,
    name: &Name,
    g: &ProjectivePoint,
) -> [u8; 33] {
    let csk = derive_child_secret_key(child_randomness, name);
    let scalar = bytes_to_scalar(&csk);
    let point = scalar * *g;
    point.to_affine().to_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::credential::derive_child_randomness;
    use k256::{
        elliptic_curve::hash2curve::{ExpandMsgXmd, GroupDigest},
        Secp256k1,
    };

    fn crs_g() -> ProjectivePoint {
        Secp256k1::hash_from_bytes::<ExpandMsgXmd<sha2::Sha256>>(
            &[b"CRS-ASC-generator-0"],
            &[b"CRS-ASC-v1"],
        )
        .expect("hash_to_curve never fails")
    }

    fn sample_r() -> ChildRandomness {
        let real_rand = [0x42u8; 32];
        derive_child_randomness(&real_rand, &Name::new("alice"))
    }

    #[test]
    fn child_secret_key_deterministic() {
        let r = sample_r();
        let name = Name::new("twitter.com");
        assert_eq!(
            derive_child_secret_key(&r, &name),
            derive_child_secret_key(&r, &name)
        );
    }

    #[test]
    fn different_services_give_different_child_keys() {
        let r = sample_r();
        let csk1 = derive_child_secret_key(&r, &Name::new("twitter.com"));
        let csk2 = derive_child_secret_key(&r, &Name::new("github.com"));
        assert_ne!(csk1, csk2);
    }

    #[test]
    fn different_randomness_gives_different_child_keys() {
        let r1 = ChildRandomness([0x01; 32]);
        let r2 = ChildRandomness([0x02; 32]);
        let name = Name::new("twitter.com");
        assert_ne!(
            derive_child_secret_key(&r1, &name),
            derive_child_secret_key(&r2, &name)
        );
    }

    #[test]
    fn pseudonym_is_33_bytes() {
        let g = crs_g();
        let r = sample_r();
        let pnym = derive_pseudonym(&r, &Name::new("twitter.com"), &g);
        assert_eq!(pnym.len(), 33);
        assert!(pnym[0] == 0x02 || pnym[0] == 0x03);
    }

    #[test]
    fn pseudonym_deterministic() {
        let g = crs_g();
        let r = sample_r();
        let name = Name::new("twitter.com");
        assert_eq!(
            derive_pseudonym(&r, &name, &g),
            derive_pseudonym(&r, &name, &g)
        );
    }

    #[test]
    fn different_services_give_different_pseudonyms() {
        let g = crs_g();
        let r = sample_r();
        let p1 = derive_pseudonym(&r, &Name::new("twitter.com"), &g);
        let p2 = derive_pseudonym(&r, &Name::new("github.com"), &g);
        assert_ne!(p1, p2);
    }

    #[test]
    fn child_key_independent_from_nullifier() {
        // Child secret key (from r) and nullifier (from sk) must be independent
        use crate::core::nullifier_v2::derive_nullifier;
        use crate::core::types::MasterSecret;

        let r = sample_r();
        let sk = MasterSecret([0x42u8; 32]);
        let name = Name::new("twitter.com");

        let csk = derive_child_secret_key(&r, &name);
        let nul = derive_nullifier(&sk, &name);

        // They should never be equal (different HKDF domains)
        assert_ne!(csk, nul.0);
    }
}
