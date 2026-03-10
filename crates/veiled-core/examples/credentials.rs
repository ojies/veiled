//! Example: generate keys and derive credentials locally.
//!
//! Demonstrates the full off-chain credential derivation:
//!   pub_key + name  →  nullifier  →  commitment
//!
//! Run: cargo run --example credentials -p veiled-core

use veiled_core::{BlindingKey, Name, PublicKey, commit, compute_nullifier};

fn main() {
    // ── 1. Identity key (in practice: generate randomly) ─────────────────────
    let pub_key = PublicKey([
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
        0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
    ]);
    let name = Name::new("alice");

    // ── 2. Blinding key (random per-registration; store securely) ────────────
    let blinding = BlindingKey([0xde, 0xad, 0xbe, 0xef,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01,
    ]);

    // ── 3. Derive credentials (entirely local — nothing hits the network) ─────
    let nullifier  = compute_nullifier(&pub_key, &name);
    let commitment = commit(&nullifier, &blinding);

    println!("pub_key:    {}", hex::encode(pub_key.as_bytes()));
    println!("name:       {name}");
    println!("blinding:   {}", hex::encode(blinding.as_bytes()));
    println!();
    println!("nullifier:  {}", hex::encode(nullifier.as_bytes()));
    println!("commitment: {}", hex::encode(commitment.as_bytes()));
    println!();

    // ── 4. Determinism check ──────────────────────────────────────────────────
    let nul2 = compute_nullifier(&pub_key, &name);
    let com2 = commit(&nul2, &blinding);
    assert_eq!(nullifier, nul2,  "nullifier must be deterministic");
    assert_eq!(commitment, com2, "commitment must be deterministic");
    println!("✓ same inputs always produce the same nullifier and commitment");

    // ── 5. Binding: different names → different nullifiers ────────────────────
    let nul_bob = compute_nullifier(&pub_key, &Name::new("bob"));
    assert_ne!(nullifier, nul_bob, "different names must give different nullifiers");
    println!("✓ alice's nullifier ≠ bob's nullifier");

    // ── 6. Hiding: different blinker → same nullifier, different commitment ───
    let blinding2   = BlindingKey([0xff; 32]);
    let commitment2 = commit(&nullifier, &blinding2);
    assert_ne!(commitment, commitment2, "different blinding keys must hide nullifier");
    println!("✓ changing the blinding key changes the commitment (hiding property)");

    // ── 7. Name length cap ────────────────────────────────────────────────────
    assert!(Name::try_new("a".repeat(255)).is_ok());
    assert!(Name::try_new("a".repeat(256)).is_err());
    println!("✓ name length is capped at {} bytes", Name::MAX_LEN);
}
