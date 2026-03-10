//! Example: Bootle/Groth one-out-of-many membership proof.
//!
//! Builds a full anonymity set of N = 1024 commitments, proves membership
//! at a chosen index in zero knowledge, then verifies the proof.
//!
//! Expected output (release build): ~2–5 s.  Debug build: ~90 s.
//!
//! Run (release, recommended):
//!   cargo run --example membership_proof -p veiled-core --release
//!
//! Run (debug — slow):
//!   cargo run --example membership_proof -p veiled-core

use std::time::Instant;
use veiled_core::{
    BlindingKey, Name, Nullifier, PublicKey,
    commit, compute_nullifier, prove_membership, verify_membership,
};

fn main() {
    // ── 1. Prover's identity ──────────────────────────────────────────────────
    let pub_key  = PublicKey([0x42u8; 32]);
    let name     = Name::new("alice");
    let blinding = BlindingKey([0x07u8; 32]);

    let nullifier  = compute_nullifier(&pub_key, &name);
    let my_commit  = commit(&nullifier, &blinding);

    println!("nullifier:  {}", hex::encode(nullifier.as_bytes()));
    println!("commitment: {}", hex::encode(my_commit.as_bytes()));
    println!();

    // ── 2. Build a 1024-element anonymity set ────────────────────────────────
    //   The prover's commitment sits at index 42.
    let my_index: usize = 42;

    println!("Building anonymity set of 1024 commitments …");
    let t0 = Instant::now();

    let set: Vec<_> = (0..1024)
        .map(|i| {
            if i == my_index {
                my_commit
            } else {
                // Deterministic dummy commitments for other slots.
                let dummy_n = Nullifier([i as u8; 32]);
                let dummy_b = BlindingKey([(i.wrapping_add(1)) as u8; 32]);
                commit(&dummy_n, &dummy_b)
            }
        })
        .collect();

    println!("  set built in {:.2?}", t0.elapsed());
    println!();

    // ── 3. Prove membership ───────────────────────────────────────────────────
    println!("Generating proof …");
    let t1 = Instant::now();

    let proof = prove_membership(&set, my_index, &nullifier, &blinding)
        .expect("prove_membership failed");

    println!("  proof generated in {:.2?}", t1.elapsed());
    println!();

    // The proof is 878 bytes serialised.
    let mut proof_bytes = Vec::with_capacity(878);
    proof_bytes.extend_from_slice(&proof.a);
    proof_bytes.extend_from_slice(&proof.b);
    proof_bytes.extend_from_slice(&proof.c);
    proof_bytes.extend_from_slice(&proof.d);
    for g in &proof.g { proof_bytes.extend_from_slice(g); }
    for f in &proof.f { proof_bytes.extend_from_slice(f); }
    proof_bytes.extend_from_slice(&proof.z_a);
    proof_bytes.extend_from_slice(&proof.z_c);
    proof_bytes.extend_from_slice(&proof.z);

    println!("Proof ({} bytes):", proof_bytes.len());
    // Print first 64 hex chars with ellipsis.
    println!("  {}…", &hex::encode(&proof_bytes)[..64]);
    println!();

    // ── 4. Verify membership ──────────────────────────────────────────────────
    println!("Verifying proof …");
    let t2 = Instant::now();

    let valid = verify_membership(&set, &nullifier, &proof);

    println!("  verification done in {:.2?}", t2.elapsed());
    println!();

    assert!(valid, "proof must verify");
    println!("✓ proof is valid — the prover knows an opening in the set");
    println!("  (the verifier learned nothing about which index was used)");

    // ── 5. Sanity: wrong nullifier fails ─────────────────────────────────────
    let wrong_nullifier = compute_nullifier(&PublicKey([0xffu8; 32]), &Name::new("eve"));
    assert!(
        !verify_membership(&set, &wrong_nullifier, &proof),
        "wrong nullifier must not verify"
    );
    println!("✓ presenting a wrong nullifier correctly fails verification");
}
