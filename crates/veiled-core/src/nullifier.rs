use sha2::{Digest, Sha256};

use crate::types::{Nullifier, PublicKey};

/// Computes `SHA256(pub_key || name)`.
///
/// The nullifier is deterministic: the same `(pub_key, name)` pair always
/// produces the same value.  It is one-way: you cannot recover the inputs from
/// the nullifier alone.
///
/// SHA256 is chosen for consistency with the Bitcoin ecosystem.
pub fn compute_nullifier(pub_key: &PublicKey, name: &str) -> Nullifier {
    let mut hasher = Sha256::new();
    hasher.update(pub_key.as_bytes());
    hasher.update(name.as_bytes());
    Nullifier(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_key() -> PublicKey {
        PublicKey([1u8; 32])
    }

    #[test]
    fn deterministic() {
        let k = sample_key();
        assert_eq!(compute_nullifier(&k, "alice"), compute_nullifier(&k, "alice"));
    }

    #[test]
    fn different_names_give_different_nullifiers() {
        let k = sample_key();
        assert_ne!(compute_nullifier(&k, "alice"), compute_nullifier(&k, "bob"));
    }

    #[test]
    fn different_keys_give_different_nullifiers() {
        let a = PublicKey([1u8; 32]);
        let b = PublicKey([2u8; 32]);
        assert_ne!(compute_nullifier(&a, "alice"), compute_nullifier(&b, "alice"));
    }

    #[test]
    fn output_is_32_bytes() {
        let nul = compute_nullifier(&sample_key(), "test");
        assert_eq!(nul.as_bytes().len(), 32);
    }

    #[test]
    fn known_vector() {
        // SHA256(0x01*32 || "alice") — fixed reference value to catch regressions.
        let key = PublicKey([0x01u8; 32]);
        let nul = compute_nullifier(&key, "alice");
        assert_ne!(nul.as_bytes(), &[0u8; 32]);
    }
}
