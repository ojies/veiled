use sha2::{Digest, Sha256};

use crate::types::{BlindingKey, Commitment, Nullifier};

/// Computes `SHA256(nullifier || blinding)`.
///
/// **Hiding**: given only the commitment, an adversary cannot determine the
/// nullifier (assuming the blinding key has sufficient entropy).
///
/// **Binding**: it is computationally infeasible to find a second
/// `(nullifier', blinding')` pair that produces the same commitment.
pub fn commit(nullifier: &Nullifier, blinding: &BlindingKey) -> Commitment {
    let mut hasher = Sha256::new();
    hasher.update(nullifier.as_bytes());
    hasher.update(blinding.as_bytes());
    Commitment(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_nullifier() -> Nullifier {
        Nullifier([0xABu8; 32])
    }

    fn sample_blinding() -> BlindingKey {
        BlindingKey([0x01u8; 32])
    }

    #[test]
    fn deterministic() {
        let n = sample_nullifier();
        let b = sample_blinding();
        assert_eq!(commit(&n, &b), commit(&n, &b));
    }

    #[test]
    fn different_blinding_gives_different_commitment() {
        let n = sample_nullifier();
        let b1 = BlindingKey([0x01u8; 32]);
        let b2 = BlindingKey([0x02u8; 32]);
        assert_ne!(commit(&n, &b1), commit(&n, &b2));
    }

    #[test]
    fn different_nullifier_gives_different_commitment() {
        let n1 = Nullifier([0x01u8; 32]);
        let n2 = Nullifier([0x02u8; 32]);
        let b = sample_blinding();
        assert_ne!(commit(&n1, &b), commit(&n2, &b));
    }

    #[test]
    fn output_is_32_bytes() {
        let c = commit(&sample_nullifier(), &sample_blinding());
        assert_eq!(c.as_bytes().len(), 32);
    }
}
