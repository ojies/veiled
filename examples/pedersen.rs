//! Example: Pedersen commitment properties on secp256k1.
//!
//! Shows the hiding and binding properties of the scheme
//! `C = r·G + v·H` and verifies the homomorphic addition identity.
//!
//! Run: cargo run --example pedersen

use k256::{
    AffinePoint, ProjectivePoint,
    elliptic_curve::{group::GroupEncoding, ops::Reduce},
    Scalar, U256,
};
use veiled::core::{BlindingKey, Nullifier, commit};
use veiled::core::commitment::h_generator;

fn scalar(bytes: &[u8; 32]) -> Scalar {
    Scalar::reduce(U256::from_be_slice(bytes))
}

fn decode(c: &veiled::core::Commitment) -> ProjectivePoint {
    AffinePoint::from_bytes(c.as_bytes().into())
        .map(ProjectivePoint::from)
        .expect("valid compressed point")
}

fn main() {
    // ── generators ────────────────────────────────────────────────────────────
    let g = ProjectivePoint::GENERATOR;
    let h = h_generator();
    println!("G = {}", hex::encode(g.to_affine().to_bytes()));
    println!("H = {}", hex::encode(h.to_affine().to_bytes()));
    println!();

    // ── sample inputs ─────────────────────────────────────────────────────────
    let n1 = Nullifier([0x11u8; 32]);
    let n2 = Nullifier([0x22u8; 32]);
    let b1 = BlindingKey([0xaau8; 32]);
    let b2 = BlindingKey([0xbbu8; 32]);

    let c1 = commit(&n1, &b1);
    let c2 = commit(&n2, &b2);

    println!("C1 = {}", hex::encode(c1.as_bytes()));
    println!("C2 = {}", hex::encode(c2.as_bytes()));
    println!();

    // ── property 1: determinism ───────────────────────────────────────────────
    assert_eq!(c1, commit(&n1, &b1));
    println!("✓ deterministic: same inputs → same commitment");

    // ── property 2: binding (different nullifiers → different commitments) ────
    let c_other = commit(&n2, &b1);
    assert_ne!(c1, c_other);
    println!("✓ binding: different nullifiers (same blinding) → different commitments");

    // ── property 3: hiding (different blinding → different commitments) ───────
    let c_other2 = commit(&n1, &b2);
    assert_ne!(c1, c_other2);
    println!("✓ hiding: different blinding (same nullifier) → different commitments");

    // ── property 4: 33-byte compressed SEC1 point ─────────────────────────────
    assert_eq!(c1.as_bytes().len(), 33);
    assert!(c1.as_bytes()[0] == 0x02 || c1.as_bytes()[0] == 0x03);
    println!("✓ output is a 33-byte compressed secp256k1 point");

    // ── property 5: homomorphic addition ─────────────────────────────────────
    // C1 + C2 == (r1+r2)·G + (v1+v2)·H
    let p1 = decode(&c1);
    let p2 = decode(&c2);
    let sum_point = p1 + p2;

    let v1 = scalar(&n1.0);
    let v2 = scalar(&n2.0);
    let r1 = scalar(&b1.as_bytes().try_into().unwrap());
    let r2 = scalar(&b2.as_bytes().try_into().unwrap());
    let expected = g * (r1 + r2) + h * (v1 + v2);

    assert_eq!(
        sum_point.to_affine().to_bytes(),
        expected.to_affine().to_bytes(),
    );
    println!("✓ homomorphic: C1 + C2 == (r1+r2)·G + (v1+v2)·H");

    println!();
    println!("All Pedersen commitment properties verified.");
}
