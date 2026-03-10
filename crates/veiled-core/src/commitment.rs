use k256::{
    AffinePoint, ProjectivePoint,
    elliptic_curve::{
        group::GroupEncoding,
        hash2curve::{ExpandMsgXmd, GroupDigest},
        ops::Reduce,
    },
    Scalar, Secp256k1, U256,
};
use sha2::Sha256;

use crate::types::{BlindingKey, Commitment, Nullifier};

// ── NUMS generator H ─────────────────────────────────────────────────────────

/// Returns the NUMS generator H = hash_to_curve("veiled-H").
pub fn h_generator() -> ProjectivePoint {
    Secp256k1::hash_from_bytes::<ExpandMsgXmd<Sha256>>(
        &[b"veiled-H"],
        &[b"veiled-commitment-v1"],
    )
    .expect("hash_to_curve never fails for secp256k1")
}

// ── scalar helper ─────────────────────────────────────────────────────────────

fn bytes_to_scalar(b: &[u8; 32]) -> Scalar {
    Scalar::reduce(U256::from_be_slice(b))
}

// ── Pedersen commitment ───────────────────────────────────────────────────────

/// Computes the Pedersen commitment `C = r·G + v·H`.
///
/// **Hiding**: an adversary who does not know `r` cannot determine `nullifier`
/// from `C` (computational Diffie-Hellman hardness).
///
/// **Binding**: it is computationally infeasible to find a second
/// `(nullifier', blinding')` pair that produces the same point `C`
/// (discrete-log hardness).
///
/// Output: 33-byte SEC1 compressed secp256k1 point.
pub fn commit(nullifier: &Nullifier, blinding: &BlindingKey) -> Commitment {
    let v = bytes_to_scalar(&nullifier.0);
    let r = bytes_to_scalar(&blinding.0);

    let g = ProjectivePoint::GENERATOR;
    let h = h_generator();

    let c: AffinePoint = (g * r + h * v).to_affine();
    Commitment(c.to_bytes().into())
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
    fn output_is_33_bytes() {
        let c = commit(&sample_nullifier(), &sample_blinding());
        assert_eq!(c.as_bytes().len(), 33);
    }

    #[test]
    fn first_byte_is_valid_sec1_prefix() {
        let c = commit(&sample_nullifier(), &sample_blinding());
        assert!(c.as_bytes()[0] == 0x02 || c.as_bytes()[0] == 0x03);
    }
}
