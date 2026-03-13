//! Full-flow integration test: Phases 0–5 with VTxO tree.
//!
//! Exercises the complete ASC protocol and bridges the credential system
//! with the vtxo tree for the first time:
//!   Phase 0: CRS setup (L=3 merchants) via Registry
//!   Phase 1: Credential creation (8 beneficiaries)
//!   Phase 2: Anonymity set via Registry + VTxO tree (Φ → bitcoin PublicKey bridge)
//!   Phase 3: Payment identity registration (Alice → Merchant 1)
//!   Phase 4: Merchant 1 verifies Alice's proof via receive_payment_registration
//!   Phase 5: Payment address derivation + name revelation

use bitcoin::secp256k1::PublicKey;
use bitcoin::{Amount, Network, OutPoint, Txid};

use crate::core::beneficiary::Beneficiary;
use crate::core::merchant::Merchant;
use crate::core::payment_identity::{verify_name_revelation, serialize_payment_identity_registration_proof, deserialize_payment_identity_registration_proof};
use crate::core::registry::Registry;
use crate::core::request::{create_payment_request, pseudonym_to_address, verify_payment_request};
use crate::core::tx::{IdentityTXO, build_identity_tree};
use crate::core::utils::M;

const L: usize = 3;
const SET_SIZE: usize = 8; // N = 2^3

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
    // ── Phase 0: Registry + CRS setup ───────────────────────────────────────
    //
    // L=3 merchants, SET_SIZE=8 beneficiaries

    let mut merchants = vec![
        Merchant::new("merchant_1", "https://merchant_1"),
        Merchant::new("merchant_2", "https://merchant_2"),
        Merchant::new("merchant_3", "https://merchant_3"),
    ];

    let mut registry = Registry::new(SET_SIZE);
    for m in &merchants {
        registry.add_merchant(m.clone());
    }
    registry.setup();
    assert_eq!(registry.crs.num_merchants(), L);

    // ── Phase 1: Credential creation ────────────────────────────────────────
    //
    // 8 beneficiaries, each created with Beneficiary::new().
    // Φ = k·g + s_1·h_1 + ... + s_L·h_L + name_scalar·h_name

    let beneficiary_names = ["alice", "bob", "carol", "dave", "eve", "frank", "grace", "heidi"];

    let mut beneficiaries: Vec<Beneficiary> = beneficiary_names
        .iter()
        .map(|name| Beneficiary::new(&registry.crs, name))
        .collect();
    assert_eq!(beneficiaries.len(), SET_SIZE);

    // All Φ values are valid compressed secp256k1 points.
    for (i, ben) in beneficiaries.iter().enumerate() {
        assert!(
            ben.credential.phi.0[0] == 0x02 || ben.credential.phi.0[0] == 0x03,
            "Φ[{i}] has invalid prefix: {:#04x}",
            ben.credential.phi.0[0]
        );
    }

    // Alice can recompute Φ from stored secrets.
    assert_eq!(
        beneficiaries[0].credential.recompute_phi(&registry.crs),
        beneficiaries[0].credential.phi
    );

    // ── Phase 2: Anonymity set via Registry + VTxO tree ─────────────────────
    //
    // Each beneficiary registers their Φ with the registry.

    for ben in &beneficiaries {
        registry.add_beneficiary(ben.credential.phi);
    }

    assert_eq!(registry.beneficiary_count(), SET_SIZE);

    let anonymity_set = registry.anonymity_set().to_vec();
    assert_eq!(anonymity_set.len(), SET_SIZE);
    assert_eq!(anonymity_set[0], beneficiaries[0].credential.phi);

    // Bridge: convert each Φ to bitcoin::secp256k1::PublicKey for the vtxo tree.
    let sats_per_user = 10_000u64;
    let beneficiary_identity_tx_out_list: Vec<IdentityTXO> = anonymity_set
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
    let tree = build_identity_tree(&beneficiary_identity_tx_out_list, funding_outpoint())
        .expect("vtxo tree construction should succeed");

    assert_eq!(tree.user_count(), SET_SIZE);
    assert_eq!(tree.tx_count(), 2); // root + fan-out

    let expected_total = Amount::from_sat(sats_per_user * SET_SIZE as u64);
    assert_eq!(tree.value(), expected_total);

    // All beneficiaries register with the anonymity set (Phase 2 complete).
    let set_id = registry.set_id;
    for ben in &mut beneficiaries {
        ben.register(set_id, anonymity_set.clone())
            .expect("beneficiary's Φ must be in the anonymity set");
    }
    assert_eq!(beneficiaries[0].set_id, Some(set_id));
    assert_eq!(beneficiaries[0].index, Some(0));

    // ── Phase 3: Payment identity registration (Alice → Merchant 1) ─────────
    //
    // Alice registers her payment identity against Merchant 1
    // (merchant_id=1, 1-indexed). Produces (ϕ, nul_l, π, d̂, "alice").

    let merchant_1_id = 1;
    let payment_reg = beneficiaries[0]
        .create_payment_registration(&registry.crs, merchant_1_id)
        .expect("proof generation should succeed");

    assert_eq!(payment_reg.service_index, merchant_1_id);
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

    // Registration is stored on the beneficiary.
    assert!(beneficiaries[0].registrations.contains_key(&merchant_1_id));

    // Proof serialization roundtrip.
    let proof_bytes = serialize_payment_identity_registration_proof(&payment_reg.proof);
    let proof_deser =
        deserialize_payment_identity_registration_proof(&proof_bytes).expect("deserialization should succeed");
    let proof_bytes_2 = serialize_payment_identity_registration_proof(&proof_deser);
    assert_eq!(
        proof_bytes, proof_bytes_2,
        "serialize roundtrip must be lossless"
    );

    // Proof size: PROOF_BASE_SIZE + (L+1)×32 bytes.
    // With M=3: 4*33 (ABCD) + 3*33 (e_poly) + 3*32 (f) + 2*32 (z_a,z_c)
    //   + 33 (schnorr_r) + 3*32 (schnorr_s, nullifier_scalar, name_scalar) = 520
    // Plus z_responses = (L+1)*32 = 128. Total = 648.
    let expected_proof_size = 4 * 33 + M * 33 + M * 32 + 2 * 32 + 33 + 3 * 32 + (L + 1) * 32;
    assert_eq!(proof_bytes.len(), expected_proof_size);

    // ── Phase 4: Merchant receives and verifies registration ─────────────────
    //
    // Merchant 1 receives (ϕ, nul_l, π, d̂, "alice") and verifies the proof.

    let merchant_1 = &mut merchants[0];
    merchant_1.merchant_id = merchant_1_id;

    let pseudonym = merchant_1
        .receive_payment_registration(&registry.crs, &anonymity_set, &payment_reg)
        .expect("valid proof should be accepted");

    assert_eq!(pseudonym, payment_reg.pseudonym);
    assert_eq!(merchant_1.registered_identities.len(), 1);
    assert!(merchant_1.registered_identities.contains_key(&payment_reg.pseudonym));

    // Replay: same registration again → pseudonym already registered.
    let replay_err = merchant_1
        .receive_payment_registration(&registry.crs, &anonymity_set, &payment_reg)
        .unwrap_err();
    assert_eq!(replay_err, "pseudonym already registered");

    // Name revelation: Alice's proof embeds name_scalar = SHA256("alice").
    assert!(
        verify_name_revelation(&payment_reg.proof.name_scalar, "alice"),
        "correct name should verify"
    );
    assert!(
        !verify_name_revelation(&payment_reg.proof.name_scalar, "bob"),
        "wrong name should not verify"
    );

    // ── Phase 5: Payment request + identity lookup ──────────────────────────
    //
    // Alice creates a payment request with her pseudonym.
    // Merchant 1 verifies the Schnorr proof and looks up the pseudonym.

    let alice = &beneficiaries[0];
    let alice_payment_request = create_payment_request(
        &alice.credential.r,
        &merchants[0].name,
        &registry.crs.g,
        5000,
    );

    let mainnet_addr = pseudonym_to_address(&alice_payment_request.pseudonym, Network::Bitcoin)
        .expect("valid pseudonym should produce an address");
    assert!(
        mainnet_addr.to_string().starts_with("bc1p"),
        "mainnet P2TR address must start with bc1p, got: {}",
        mainnet_addr
    );

    let payment_request_verified = verify_payment_request(
        &registry.crs.g,
        &alice_payment_request.pseudonym,
        &alice_payment_request.proof,
    );
    assert!(payment_request_verified, "valid payment request proof should verify");

    // Merchant looks up the pseudonym in their registration table.
    let registered = merchants[0]
        .registered_identities
        .get(&alice_payment_request.pseudonym)
        .expect("pseudonym should be registered from Phase 3");

    assert_eq!(registered.pseudonym, alice_payment_request.pseudonym);
    assert_eq!(registered.friendly_name, "alice");
    assert_eq!(registered.public_nullifier, payment_reg.public_nullifier);
}
