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

use crate::core::types::{BlindingKey, Commitment, Nullifier};

use rand_core::{OsRng, RngCore};
use sha2::{Digest, };


// ── constants ─────────────────────────────────────────────────────────────────

pub const M: usize = 3; // bits
pub const N: usize = 1 << M; // 8

/// CRS domain separation tag — shared with crs.rs.
pub const CRS_DST: &[u8] = b"CRS-ASC-v1";



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

pub fn bytes_to_scalar(b: &[u8; 32]) -> Scalar {
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


// ── per-bit generators H_0 .. H_{M-1} ────────────────────────────────────────
//
// Independent NUMS generators for the compact aggregate bit commitment scheme.
// Derived via HashToCurve with CRS-consistent domain separation.

pub fn bit_generators() -> [ProjectivePoint; M] {
    std::array::from_fn(|k| {
        let tag = format!("CRS-ASC-bit-generator-{k}");
        Secp256k1::hash_from_bytes::<ExpandMsgXmd<Sha256>>(
            &[tag.as_bytes()],
            &[CRS_DST],
        )
        .expect("hash_to_curve never fails for secp256k1")
    })
}

// ── scalar / point helpers ───────────────────────────────────────────────────



pub fn random_scalar() -> Scalar {
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    bytes_to_scalar(&buf)
}

pub fn scalar_to_bytes(s: &Scalar) -> [u8; 32] {
    s.to_bytes().into()
}

pub fn point_to_bytes(p: &ProjectivePoint) -> [u8; 33] {
    p.to_affine().to_bytes().into()
}

pub fn point_from_bytes(b: &[u8; 33]) -> Option<ProjectivePoint> {
    AffinePoint::from_bytes(b.into()).map(ProjectivePoint::from).into()
}

pub fn scalar_from_bytes(b: &[u8; 32]) -> Scalar {
    bytes_to_scalar(b)
}

// ── Fiat-Shamir challenge ───────────────────────────────────────────────────
//
// x = Hash(crs, Λ, d̂, l, nul_l, ϕ, {commitments}, {polynomial points})
// Spec section 3.5: "The inclusion of ϕ (the pseudonym) in the hash is
// critical — it binds the proof to a specific pseudonym."

pub fn fiat_shamir_challenge(
    crs_g: &ProjectivePoint,
    pseudonym: &[u8; 33],
    public_nullifier: &[u8; 33],
    service_index: usize,
    set_id: &[u8; 32],
    name_scalar: &[u8; 32],
    d_set: &[ProjectivePoint],
    a: &ProjectivePoint,
    b: &ProjectivePoint,
    c: &ProjectivePoint,
    d: &ProjectivePoint,
    e_poly: &[ProjectivePoint; M],
) -> Scalar {
    let mut hasher = Sha256::new();
    hasher.update(b"CRS-ASC-service-registration-v1");
    hasher.update(point_to_bytes(crs_g)); // bind to CRS
    hasher.update(pseudonym);             // ϕ
    hasher.update(public_nullifier);      // nul_l
    hasher.update(&(service_index as u64).to_be_bytes()); // l
    hasher.update(set_id);                // d̂ (merkle root)
    hasher.update(name_scalar);           // bind revealed name
    for di in d_set {
        hasher.update(point_to_bytes(di));
    }
    hasher.update(point_to_bytes(a));
    hasher.update(point_to_bytes(b));
    hasher.update(point_to_bytes(c));
    hasher.update(point_to_bytes(d));
    for ei in e_poly {
        hasher.update(point_to_bytes(ei));
    }
    let hash: [u8; 32] = hasher.finalize().into();
    bytes_to_scalar(&hash)
}

// ── Schnorr challenge for π_value ───────────────────────────────────────────
//
// Spec section 3.7: "π_value = Schnorr proof that nul_l = s_l·g"
// e = Hash("CRS-ASC-schnorr-nullifier" || g || nul_l || R)

pub fn schnorr_challenge(
    g: &ProjectivePoint,
    public_nullifier: &[u8; 33],
    r_point: &ProjectivePoint,
) -> Scalar {
    let mut hasher = Sha256::new();
    hasher.update(b"CRS-ASC-schnorr-nullifier");
    hasher.update(point_to_bytes(g));
    hasher.update(public_nullifier);
    hasher.update(point_to_bytes(r_point));
    let hash: [u8; 32] = hasher.finalize().into();
    bytes_to_scalar(&hash)
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
