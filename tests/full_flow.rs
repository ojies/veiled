//! Full-flow integration test: Phases 0–5 with VTxO tree.
//!
//! Exercises the complete ASC protocol and bridges the credential system
//! with the vtxo tree for the first time:
//!   Phase 0: CRS setup (L=8 providers = group members)
//!   Phase 1: Credential creation (1024 members: 8 named + 1016 fillers)
//!   Phase 2: Anonymity set + VTxO tree (Φ → bitcoin PublicKey bridge)
//!   Phase 3: Service registration (Alice → Bob proof)
//!   Phase 4: Verifier verification (Bob verifies Alice, steps 4.1–4.8)
//!   Phase 5: Payment address derivation + name revelation
//!
//! Architecture: In production L=N=1024 — all group members are CRS providers.
//! For testing we use L=8 with N=1024 (padded) since N is hardcoded in the
//! proof system. The vtxo tree has 1024 leaves, each using Φ as its pubkey.

use bitcoin::secp256k1::PublicKey;
use bitcoin::{Amount, Network, OutPoint, Txid};

use veiled::core::credential::{MasterCredential, RegisteredIdentity};
use veiled::core::crs::{Crs, User};
use veiled::core::payment::{nullifier_to_address, pseudonym_to_address, verify_name_revelation};
use veiled::core::service_proof::{deserialize_service_proof, serialize_service_proof};
use veiled::core::types::{BlindingKey, ChildRandomness, Commitment, FriendlyName, MasterSecret, Name};
use veiled::core::verifier::{VerificationError, VerifierState};
use veiled::vtxo_tree::tree::build_tree;
use veiled::vtxo_tree::types::User as VtxoUser;

const N: usize = 1024;
const L: usize = 8;

/// Named group member credentials (deterministic from seed + friendly name).
fn make_named_credential(crs: &Crs, seed: u8, friendly_name: &str) -> MasterCredential {
    let sk = MasterSecret([seed; 32]);
    let r = ChildRandomness([seed.wrapping_add(1); 32]);
    let k = BlindingKey([seed.wrapping_add(2); 32]);
    MasterCredential::create(crs, sk, r, k, FriendlyName::new(friendly_name))
}

/// Filler credential using index-encoded secret to avoid seed collisions.
fn make_filler_credential(crs: &Crs, index: usize) -> MasterCredential {
    let mut sk_bytes = [0u8; 32];
    sk_bytes[0] = (index >> 8) as u8;
    sk_bytes[1] = (index & 0xFF) as u8;
    sk_bytes[31] = 0xFF; // sentinel — avoids collision with named seeds
    let mut r_bytes = sk_bytes;
    r_bytes[31] = 0xFE;
    let mut k_bytes = sk_bytes;
    k_bytes[31] = 0xFD;
    MasterCredential::create(
        crs,
        MasterSecret(sk_bytes),
        ChildRandomness(r_bytes),
        BlindingKey(k_bytes),
        FriendlyName::new(format!("filler-{index}")),
    )
}

fn funding_outpoint() -> OutPoint {
    OutPoint {
        txid: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .parse::<Txid>()
            .unwrap(),
        vout: 0,
    }
}

#[test]
fn full_protocol_flow_phases_0_through_5() {
    // ── Phase 0: CRS setup ──────────────────────────────────────────────────
    //
    // L=8 providers (group members). In production L=N=1024.
    // Alice = provider 0 (service_index=1), Bob = provider 1 (service_index=2).

    let provider_names = [
        "alice", "bob", "charlie", "diana", "eve", "frank", "grace", "heidi",
    ];
    let providers: Vec<User> = provider_names
        .iter()
        .map(|name| User {
            name: Name::new(*name),
            credential_generator: [0x02; 33],
            origin: format!("https://{name}"),
        })
        .collect();
    let crs = Crs::setup(providers);

    assert_eq!(crs.num_providers(), L);

    // ── Phase 1: Credential creation ────────────────────────────────────────
    //
    // All 1024 members create MasterCredentials.
    // First 8 are named group members, remaining 1016 are fillers.
    // Φ = k·g + s_1·h_1 + ... + s_8·h_8 + name_scalar·h_name

    let named_seeds: [u8; L] = [0xA0, 0xB0, 0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5];

    // Create all 1024 credentials.
    let mut all_credentials: Vec<MasterCredential> = Vec::with_capacity(N);
    for i in 0..N {
        if i < L {
            all_credentials.push(make_named_credential(
                &crs,
                named_seeds[i],
                provider_names[i],
            ));
        } else {
            all_credentials.push(make_filler_credential(&crs, i));
        }
    }

    let alice = &all_credentials[0];
    let bob = &all_credentials[1];

    // All Φ values are valid compressed secp256k1 points.
    for (i, cred) in all_credentials.iter().enumerate() {
        assert!(
            cred.phi.0[0] == 0x02 || cred.phi.0[0] == 0x03,
            "Φ[{i}] has invalid prefix: {:#04x}",
            cred.phi.0[0]
        );
    }

    // Alice can recompute Φ from stored secrets.
    assert_eq!(alice.recompute_phi(&crs), alice.phi);

    // ── Phase 2: Anonymity set + VTxO tree ──────────────────────────────────
    //
    // The anonymity set is all 1024 Φ values.
    // The vtxo tree leaf pubkey = Φ (the Pedersen commitment).

    let anonymity_set: Vec<Commitment> = all_credentials.iter().map(|c| c.phi).collect();
    assert_eq!(anonymity_set.len(), N);
    assert_eq!(anonymity_set[0], alice.phi);
    assert_eq!(anonymity_set[1], bob.phi);

    // Bridge: convert each Φ to bitcoin::secp256k1::PublicKey for the vtxo tree.
    // This is the key integration assertion — Φ is a valid secp256k1 point
    // produced by k256, and bitcoin::secp256k1 uses the same curve.
    let sats_per_user = 10_000u64;
    let vtxo_users: Vec<VtxoUser> = anonymity_set
        .iter()
        .enumerate()
        .map(|(i, phi)| {
            let pk = PublicKey::from_slice(&phi.0).unwrap_or_else(|e| {
                panic!("Φ[{i}] failed to convert to bitcoin PublicKey: {e}")
            });
            VtxoUser {
                pubkey: pk,
                amount: Amount::from_sat(sats_per_user),
            }
        })
        .collect();

    // Build the vtxo tree.
    let tree = build_tree(&vtxo_users, funding_outpoint())
        .expect("vtxo tree construction should succeed");

    assert_eq!(tree.user_count(), N);
    assert_eq!(tree.tx_count(), N - 1); // 1023 internal transactions
    assert_eq!(tree.depth(), 9); // log2(1024) - 1

    let expected_total = Amount::from_sat(sats_per_user * N as u64);
    assert_eq!(tree.value(), expected_total);

    // Alice and Bob have branches of length 10 (depth 9 + 1).
    let alice_branch = tree.branch(0).expect("alice branch");
    let bob_branch = tree.branch(1).expect("bob branch");
    assert_eq!(alice_branch.len(), 10);
    assert_eq!(bob_branch.len(), 10);

    // Alice and Bob share the root transaction (same leaf pair at positions 0,1).
    assert_eq!(
        alice_branch[0].compute_txid(),
        bob_branch[0].compute_txid(),
        "alice and bob share the root tx"
    );

    // Wrap Alice in RegisteredIdentity (Phase 2 complete).
    let set_id = 42u64;
    let reg_id = RegisteredIdentity::new(alice.clone(), set_id, anonymity_set.clone())
        .expect("alice's Φ must be in the anonymity set");
    assert_eq!(reg_id.set_id, set_id);
    assert_eq!(reg_id.index, 0); // Alice is at position 0

    // ── Phase 3: Service registration (Alice → Bob) ─────────────────────────
    //
    // Alice registers for Bob's service (service_index=2, 1-indexed).
    // Produces (ϕ, nul_bob, π, d̂, "alice").

    let bob_service_index = 2; // Bob is provider 1, so service_index = 2 (1-indexed)
    let service_reg = reg_id
        .register_for_service(&crs, bob_service_index)
        .expect("proof generation should succeed");

    assert_eq!(service_reg.service_index, bob_service_index);
    assert_eq!(service_reg.set_id, set_id);
    assert_eq!(service_reg.friendly_name, "alice");

    // Pseudonym and public nullifier are valid compressed points.
    assert!(
        service_reg.pseudonym[0] == 0x02 || service_reg.pseudonym[0] == 0x03,
        "pseudonym must be a valid compressed point"
    );
    assert!(
        service_reg.public_nullifier[0] == 0x02 || service_reg.public_nullifier[0] == 0x03,
        "public nullifier must be a valid compressed point"
    );

    // Proof serialization roundtrip.
    let proof_bytes = serialize_service_proof(&service_reg.proof);
    let proof_deser =
        deserialize_service_proof(&proof_bytes).expect("deserialization should succeed");
    let proof_bytes_2 = serialize_service_proof(&proof_deser);
    assert_eq!(
        proof_bytes, proof_bytes_2,
        "serialize roundtrip must be lossless"
    );

    // Proof size: 975 + (L+1)×32 = 975 + 288 = 1263 bytes.
    let expected_proof_size = 975 + (L + 1) * 32;
    assert_eq!(
        proof_bytes.len(),
        expected_proof_size,
        "proof size mismatch"
    );

    // ── Phase 4: Verifier verification ──────────────────────────────────────
    //
    // Bob (user_index=2) receives (ϕ, nul_bob, π, d̂, "alice") and runs 4.1–4.8.

    let mut bob_verifier = VerifierState::new(bob_service_index);

    // 4.1: Cache the frozen anonymity set.
    bob_verifier.cache_set(set_id, anonymity_set.clone());
    assert!(bob_verifier.get_cached_set(set_id).is_some());

    // 4.2–4.8: Verify proof and register.
    let result = bob_verifier
        .verify_and_register(
            &crs,
            &service_reg.pseudonym,
            &service_reg.public_nullifier,
            &service_reg.proof,
            set_id,
            &service_reg.friendly_name,
        )
        .expect("valid proof should register successfully");

    assert_eq!(result.pseudonym, service_reg.pseudonym);
    assert_eq!(result.public_nullifier, service_reg.public_nullifier);
    assert_eq!(result.friendly_name, "alice");
    assert_eq!(bob_verifier.registered_count(), 1);
    assert!(bob_verifier.has_nullifier(&service_reg.public_nullifier));
    assert!(bob_verifier.has_pseudonym(&service_reg.pseudonym));

    // 4.7 replay: same proof again → NullifierAlreadyUsed.
    let replay_err = bob_verifier
        .verify_and_register(
            &crs,
            &service_reg.pseudonym,
            &service_reg.public_nullifier,
            &service_reg.proof,
            set_id,
            &service_reg.friendly_name,
        )
        .unwrap_err();
    assert_eq!(replay_err, VerificationError::NullifierAlreadyUsed);

    // ── Phase 5: Payment + name revelation ──────────────────────────────────
    //
    // Alice sends BTC to Bob's public nullifier (converted to P2TR address).
    // nul_bob = s_bob · g is a valid secp256k1 point → bc1p... address.

    let mainnet_addr = nullifier_to_address(&service_reg.public_nullifier, Network::Bitcoin)
        .expect("valid nullifier should produce an address");
    assert!(
        mainnet_addr.to_string().starts_with("bc1p"),
        "mainnet P2TR address must start with bc1p, got: {}",
        mainnet_addr
    );

    let testnet_addr = nullifier_to_address(&service_reg.public_nullifier, Network::Testnet)
        .expect("valid nullifier should produce a testnet address");
    assert!(
        testnet_addr.to_string().starts_with("tb1p"),
        "testnet P2TR address must start with tb1p, got: {}",
        testnet_addr
    );

    // Pseudonym can also serve as a payment address.
    let pseudo_addr = pseudonym_to_address(&service_reg.pseudonym, Network::Bitcoin)
        .expect("valid pseudonym should produce an address");
    assert!(
        pseudo_addr.to_string().starts_with("bc1p"),
        "pseudonym P2TR address must start with bc1p, got: {}",
        pseudo_addr
    );

    // Name revelation: Alice's proof embeds name_scalar = SHA256("alice").
    // Bob verifies that the claimed friendly name matches.
    assert!(
        verify_name_revelation(&service_reg.proof.name_scalar, "alice"),
        "correct name should verify"
    );
    assert!(
        !verify_name_revelation(&service_reg.proof.name_scalar, "bob"),
        "wrong name should not verify"
    );
}
