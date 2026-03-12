//! Full-flow integration test: Phases 0–5 with VTxO tree.
//!
//! Exercises the complete ASC protocol and bridges the credential system
//! with the vtxo tree for the first time:
//!   Phase 0: CRS setup (L=8 providers = group members)
//!   Phase 1: Credential creation (1024 members: 8 named + 1016 fillers)
//!   Phase 2: Anonymity set + VTxO tree (Φ → bitcoin PublicKey bridge)
//!   Phase 3: Payment identity registration (Alice → Bob's payment identifier)
//!   Phase 4: Verifier verification (Bob verifies Alice, steps 4.1–4.8)
//!   Phase 5: Payment address derivation + name revelation
//!
//! Architecture: In production L=N=1024 — all group members are CRS providers.
//! For testing we use L=8 with N=1024 (padded) since N is hardcoded in the
//! proof system. The vtxo structure has a root tx and a fan-out tx with 1024
//! outputs, each using Φ as its pubkey.

use bitcoin::consensus::serialize;
use bitcoin::secp256k1::PublicKey;
use bitcoin::{Amount, Network, OutPoint, Txid};

use crate::core::credential::{MasterCredential, Beneficiary};
use crate::core::crs::{Crs, Merchant};
use crate::core::payment_identity::{verify_name_revelation, serialize_payment_identity_registration_proof, deserialize_payment_identity_registration_proof};
use crate::core::request::{create_payment_request, pseudonym_to_address, verify_payment_request};
use crate::core::tx::{IdentityTXO, build_identity_tree};
use crate::core::types::{BlindingKey, ChildRandomness, Commitment, FriendlyName, MasterSecret, Name};
use crate::core::verifier::{VerificationError, VerifierState};

const N: usize = 1024;
const L: usize = 3;

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
    // L=3 merchants . 
    // Beneficiaries = 1024

    let merchant_names = [
        "merchant_1", "merchant_2", "merchant_3",
    ];

    let  beneficiary_names = ["alice", "bob", "carol", "dave", "eve", "frank", "grace", "heidi"];

    let merchants: Vec<Merchant> = merchant_names
        .iter()
        .map(|name| Merchant {
            name: Name::new(*name),
            credential_generator: [0x02; 33],
            origin: format!("https://{name}"),
        })
        .collect();
    let crs = Crs::setup(merchants);

    assert_eq!(crs.num_merchants(), L);

    // ── Phase 1: Credential creation ────────────────────────────────────────
    //
    // All 1024 members create MasterCredentials.
    // First 8 are named group members, remaining 1016 are fillers.
    // Φ = k·g + s_1·h_1 + ... + s_8·h_8 + name_scalar·h_name

    let named_seeds: [u8; L] = [0xA0, 0xB0, 0xC0];

    // Create all 1024 credentials.
    let mut all_beneficiary_credentials: Vec<MasterCredential> = Vec::with_capacity(N);
    for i in 0..N {
        if i < L {
            all_beneficiary_credentials.push(make_named_credential(
                &crs,
                named_seeds[i],
                beneficiary_names[i],
            ));
        } else {
            all_beneficiary_credentials.push(make_filler_credential(&crs, i));
        }
    }

    let alice_credential = &all_beneficiary_credentials[0];

    // All Φ values are valid compressed secp256k1 points.
    for (i, cred) in all_beneficiary_credentials.iter().enumerate() {
        assert!(
            cred.phi.0[0] == 0x02 || cred.phi.0[0] == 0x03,
            "Φ[{i}] has invalid prefix: {:#04x}",
            cred.phi.0[0]
        );
    }

    // Alice can recompute Φ from stored secrets.
    assert_eq!(alice_credential.recompute_phi(&crs), alice_credential.phi);

    // ── Phase 2: Anonymity set + VTxO tree ──────────────────────────────────
    //
    // The anonymity set is all 1024 Φ values.
    // The vtxo tree leaf pubkey = Φ (the Pedersen commitment).

    let anonymity_set: Vec<Commitment> = all_beneficiary_credentials.iter().map(|c| c.phi).collect();
    assert_eq!(anonymity_set.len(), N);
    assert_eq!(anonymity_set[0], alice_credential.phi);

    // Bridge: convert each Φ to bitcoin::secp256k1::PublicKey for the vtxo tree.
    // This is the key integration assertion — Φ is a valid secp256k1 point
    // produced by k256, and bitcoin::secp256k1 uses the same curve.
    let sats_per_user = 10_000u64;
    let benefiary_identity_tx_out_list: Vec<IdentityTXO> = anonymity_set
        .iter()
        .enumerate()
        .map(|(i, phi)| {
            let pk = PublicKey::from_slice(&phi.0).unwrap_or_else(|e| {
                panic!("Φ[{i}] failed to convert to bitcoin PublicKey: {e}")
            });
            IdentityTXO {
                pubkey: pk,
                amount: Amount::from_sat(sats_per_user),
            }
        })
        .collect();

    // Build the vtxo tree.
    let tree = build_identity_tree(&benefiary_identity_tx_out_list, funding_outpoint())
        .expect("vtxo tree construction should succeed");

    assert_eq!(tree.user_count(), N);
    assert_eq!(tree.tx_count(), 2); // root + fan-out

    let expected_total = Amount::from_sat(sats_per_user * N as u64);
    assert_eq!(tree.value(), expected_total);


    // Wrap Alice in RegisteredIdentity (Phase 2 complete).
    let set_id = 42u64;
    let alice_as_beneficary = Beneficiary::new(alice_credential.clone(), set_id, anonymity_set.clone())
        .expect("alice's Φ must be in the anonymity set");
    assert_eq!(alice_as_beneficary.set_id, set_id);
    assert_eq!(alice_as_beneficary.index, 0); // Alice is at position 0

    // ── Phase 3: Payment identity registration (Alice → Merchant) ────────────────
    //
    // Alice registers her payment identity against Bob's payment identifier
    // (service_index=2, 1-indexed). Produces (ϕ, nul_bob, π, d̂, "alice").

    let merchant_1_index = 1; // 1
    let payment_reg = alice_as_beneficary
        .create_payment_registration(&crs, merchant_1_index)
        .expect("proof generation should succeed");

    assert_eq!(payment_reg.service_index, merchant_1_index);
    assert_eq!(payment_reg.set_id, set_id);
    assert_eq!(payment_reg.friendly_name, "alice");

    // Pseudonym and public nullifier are valid compressed points.
    assert!(
        payment_reg.pseudonym[0] == 0x02 || payment_reg.pseudonym[0] == 0x03,
        "pseudonym must be a valid compressed point"
    );
    assert!(
        payment_reg.public_nullifier[0] == 0x02 || payment_reg.public_nullifier[0] == 0x03,
        "public nullifier must be a valid compressed point"
    );

    // Proof serialization roundtrip.
    let proof_bytes = serialize_payment_identity_registration_proof(&payment_reg.proof);
    let proof_deser =
        deserialize_payment_identity_registration_proof(&proof_bytes).expect("deserialization should succeed");
    let proof_bytes_2 = serialize_payment_identity_registration_proof(&proof_deser);
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

    let mut merchant_1_verifier = VerifierState::new(merchant_1_index);

    // 4.1: Cache the frozen anonymity set.
    merchant_1_verifier.cache_set(set_id, anonymity_set.clone());
    assert!(merchant_1_verifier.get_cached_set(set_id).is_some());

    // 4.2–4.8: Verify proof and register.
    let result = merchant_1_verifier
        .verify_and_register(
            &crs,
            &payment_reg.pseudonym,
            &payment_reg.public_nullifier,
            &payment_reg.proof,
            set_id,
            &payment_reg.friendly_name,
        )
        .expect("valid proof should register successfully");

    assert_eq!(result.pseudonym, payment_reg.pseudonym);
    assert_eq!(result.public_nullifier, payment_reg.public_nullifier);
    assert_eq!(result.friendly_name, "alice");
    assert_eq!(merchant_1_verifier.registered_count(), 1);
    assert!(merchant_1_verifier.has_nullifier(&payment_reg.public_nullifier));
    assert!(merchant_1_verifier.has_pseudonym(&payment_reg.pseudonym));

    // 4.7 replay: same proof again → NullifierAlreadyUsed.
    let replay_err = merchant_1_verifier
        .verify_and_register(
            &crs,
            &payment_reg.pseudonym,
            &payment_reg.public_nullifier,
            &payment_reg.proof,
            set_id,
            &payment_reg.friendly_name,
        )
        .unwrap_err();
    assert_eq!(replay_err, VerificationError::NullifierAlreadyUsed);

    // name verification
    // Name revelation: Alice's proof embeds name_scalar = SHA256("alice").
    // Bob verifies that the claimed friendly name matches.
    assert!(
        verify_name_revelation(&payment_reg.proof.name_scalar, "alice"),
        "correct name should verify"
    );

     assert!(
        !verify_name_revelation(&payment_reg.proof.name_scalar, "bob"),
        "wrong name should not verify"
    );

    // ── Phase 5: Payment + revelation ──────────────────────────────────
    //
    // Alice sends BTC to Bob's public nullifier (converted to P2TR address).
    // nul_bob = s_bob · g is a valid secp256k1 point → bc1p... address.
    let alice_credential = &all_beneficiary_credentials[0];

    let alice_payment_request = create_payment_request(&alice_credential.r, &Name(merchant_names[0].to_string()), &crs.g, 5000);
       

    let mainnet_addr = pseudonym_to_address(&alice_payment_request.pseudonym, Network::Bitcoin)
        .expect("valid nullifier should produce an address");
    assert!(
        mainnet_addr.to_string().starts_with("bc1p"),
        "mainnet P2TR address must start with bc1p, got: {}",
        mainnet_addr
    );

    let payment_request_verified = verify_payment_request(&crs.g, &alice_payment_request.pseudonym, &alice_payment_request.proof);
    assert!(
        payment_request_verified,
        "valid payment request proof should verify"
    );

    // ???

    // Option A lookup: merchant looks up the pseudonym in their registration table
    // to find the associated nullifier and friendly name from Phase 3.
    let registered_identity = merchant_1_verifier
        .lookup_by_pseudonym(&alice_payment_request.pseudonym)
        .expect("pseudonym should be registered from Phase 3");

    assert_eq!(registered_identity.pseudonym, alice_payment_request.pseudonym);
    assert_eq!(registered_identity.friendly_name, "alice");
    assert_eq!(registered_identity.public_nullifier, payment_reg.public_nullifier);
}
