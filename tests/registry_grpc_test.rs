//! End-to-end integration test for the registry gRPC service.
//!
//! Mirrors the exact protocol steps performed by the `beneficiary` and `merchant`
//! binaries, using the same `veiled::client` functions so behaviour is identical.
//! Phases 3-5 (payment identity registration and payment request) are exercised
//! locally — no gRPC between merchant and beneficiary, by design.
//!
//! Requires bitcoind running on regtest:
//!   URL:      http://localhost:18443  (override via BITCOIND_RPC)
//!   User/pass: veiled / veiled        (override via BITCOIND_USER / BITCOIND_PASS)
//!
//! Run with:
//!   cargo test --test registry_grpc_test -- --nocapture

use bdk_bitcoind_rpc::bitcoincore_rpc::{Auth, Client, RpcApi};
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use veiled::client::{self, MerchantState};
use veiled::core::beneficiary::Beneficiary;
use veiled::core::request::{create_payment_request, pseudonym_to_address, verify_payment_request};
use veiled::registry::pb::registry_server::RegistryServer;
use veiled::registry::service::{Config, RegistryService};
use veiled::registry::store::RegistryStore;

// ── Bitcoind helpers ─────────────────────────────────────────────────────────

fn make_rpc() -> Client {
    let url = std::env::var("BITCOIND_RPC")
        .unwrap_or_else(|_| "http://localhost:18443".to_string());
    let user = std::env::var("BITCOIND_USER").unwrap_or_else(|_| "veiled".to_string());
    let pass = std::env::var("BITCOIND_PASS").unwrap_or_else(|_| "veiled".to_string());
    Client::new(&url, Auth::UserPass(user, pass)).expect("create rpc client")
}

/// Mine `count` blocks to `addr`; return block hashes.
fn mine_to(rpc: &Client, count: u64, addr: &str) -> Vec<String> {
    let result: serde_json::Value = rpc
        .call(
            "generatetoaddress",
            &[serde_json::json!(count), serde_json::json!(addr)],
        )
        .expect("generatetoaddress");
    result
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h.as_str().unwrap().to_string())
        .collect()
}

/// Return the coinbase txid (display-order hex) from a block hash.
fn coinbase_txid(rpc: &Client, block_hash: &str) -> String {
    let block: serde_json::Value = rpc
        .call(
            "getblock",
            &[serde_json::json!(block_hash), serde_json::json!(1)],
        )
        .expect("getblock");
    block["tx"][0].as_str().unwrap().to_string()
}

// ── Test ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_registry_e2e() -> Result<(), Box<dyn std::error::Error>> {
    let rpc = Arc::new(make_rpc());

    // ── Setup: registry store + config ───────────────────────────────────────
    let store = Arc::new(Mutex::new(RegistryStore::new(
        Some(rpc.clone()),
        None,
    )));
    // 2 beneficiary slots keeps the test fast; default fee schedule
    let config = Config {
        beneficiary_capacity: 2,
        ..Config::default()
    };
    let registry_addr = store.lock().await.wallet_address.to_string();

    // ── Fund registry wallet ─────────────────────────────────────────────────
    // Mine 102 blocks to the registry address:
    //   - Block 0 coinbase → merchant payment proof txid
    //   - Block 1 coinbase → Alice payment proof txid
    //   - Block 2 coinbase → Bob payment proof txid
    //   - After 102 confirmations block 0's coinbase is mature; BDK spends it
    //     for the commitment transaction inside finalize_set.
    let blocks = mine_to(&rpc, 102, &registry_addr);
    let merchant_txid  = hex::decode(coinbase_txid(&rpc, &blocks[0]))?;
    let alice_txid     = hex::decode(coinbase_txid(&rpc, &blocks[1]))?;
    let bob_txid       = hex::decode(coinbase_txid(&rpc, &blocks[2]))?;

    // ── Start gRPC server ────────────────────────────────────────────────────
    let listen: std::net::SocketAddr = "[::1]:50052".parse()?;
    let service = RegistryService::new(store, config);
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(RegistryServer::new(service))
            .serve(listen)
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut c = client::connect("http://[::1]:50052").await?;

    // ════════════════════════════════════════════════════════════════════════
    // Phase 0 — Merchant registration
    // ════════════════════════════════════════════════════════════════════════

    // Step 0.1: fee schedule
    let (beneficiary_fee, merchant_fee) = client::get_fees(&mut c).await?;
    assert!(merchant_fee > 0 && beneficiary_fee > 0);

    // Step 0.2: registry address (zero set_id = pre-finalization global address)
    let (addr_str, internal_key) = client::get_registry_address(&mut c, &[0u8; 32]).await?;
    assert!(addr_str.starts_with("bcrt1p"), "expected bcrt1p, got: {}", addr_str);
    assert_eq!(internal_key.len(), 32);

    // Step 0.3: register "Test Merchant"
    // Coinbase vout[0] pays to the registry address; value >> required fee.
    let mut merchant_state = client::register_merchant(
        &mut c,
        "Test Merchant",
        "http://test.com",
        "merchant@example.com",
        "+987654321",
        merchant_txid.clone(),
        0,
    )
    .await?;
    assert_eq!(merchant_state.merchant.merchant_id, 1);

    // duplicate registration must fail
    assert!(client::register_merchant(
        &mut c, "Test Merchant", "http://test.com", "", "0", merchant_txid, 0,
    )
    .await
    .is_err());

    // Step 0.4: verify merchant list
    let merchants = client::get_merchants(&mut c).await?;
    assert_eq!(merchants.len(), 1);
    assert_eq!(merchants[0].name, "Test Merchant");
    assert_eq!(merchants[0].credential_generator.len(), 33);

    // ════════════════════════════════════════════════════════════════════════
    // Phase 1 — Beneficiary credential creation (local, offline)
    // ════════════════════════════════════════════════════════════════════════

    // Build a local CRS from the registered merchant state — deterministic
    // generators, so this will match the remote CRS produced at finalization.
    // beneficiary_capacity = 2 is the set_size used by the registry service.
    let local_crs = MerchantState::build_crs(&[&merchant_state], 2);

    // Alice uses the real CRS so her phi is ZK-valid and will match post-finalization.
    let mut alice = Beneficiary::new(&local_crs, "alice");

    // Bob is a filler; any valid compressed point works for registration.
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let bob_phi = bitcoin::secp256k1::PublicKey::from_secret_key(
        &secp,
        &bitcoin::secp256k1::SecretKey::from_slice(&[0x02; 32])?,
    )
    .serialize()
    .to_vec();

    // ════════════════════════════════════════════════════════════════════════
    // Phase 2 — Beneficiary registration
    // ════════════════════════════════════════════════════════════════════════

    // Step 2.1: register Alice
    let alice_index = client::register_beneficiary(
        &mut c,
        alice.credential.phi.0.to_vec(),
        "alice",
        "alice@example.com",
        "",
        alice_txid.clone(),
        0,
    )
    .await?;
    assert_eq!(alice_index, 0);

    // duplicate phi must fail
    assert!(client::register_beneficiary(
        &mut c, alice.credential.phi.0.to_vec(), "alice-dup", "", "", alice_txid, 0,
    )
    .await
    .is_err());

    // Step 2.2: register Bob (fills the remaining slot)
    client::register_beneficiary(
        &mut c, bob_phi, "bob", "bob@example.com", "", bob_txid, 0,
    )
    .await?;

    // Step 2.3: third beneficiary must fail (beneficiary_capacity = 2)
    let extra = mine_to(&rpc, 1, &registry_addr);
    let extra_txid = hex::decode(coinbase_txid(&rpc, &extra[0]))?;
    let carol_phi = bitcoin::secp256k1::PublicKey::from_secret_key(
        &secp,
        &bitcoin::secp256k1::SecretKey::from_slice(&[0x03; 32])?,
    )
    .serialize()
    .to_vec();
    assert!(
        client::register_beneficiary(&mut c, carol_phi, "carol", "", "", extra_txid, 0)
            .await
            .is_err()
    );

    // ════════════════════════════════════════════════════════════════════════
    // Finalization — broadcasts real Taproot commitment tx;
    //                CRS is built from registered merchants inside finalize_set.
    // ════════════════════════════════════════════════════════════════════════

    let set_id = client::finalize_set(&mut c).await?;
    assert_eq!(set_id.len(), 32, "set_id must be a 32-byte commitment txid");

    // Mine 1 block to confirm the commitment tx
    mine_to(&rpc, 1, &registry_addr);

    // ── Fetch CRS from the finalized set ─────────────────────────────────────
    let crs = client::get_crs(&mut c, &set_id).await?;
    assert_eq!(crs.num_merchants(), 1);
    // The remote CRS must be identical to what we built locally — same
    // deterministic generators for the same merchant name.
    assert_eq!(crs.to_bytes(), local_crs.to_bytes());

    // ── Fetch finalized anonymity set ─────────────────────────────────────────
    // Also exercises subscribe_set_finalization (already finalized → returns immediately).
    let anonymity_set = client::wait_for_finalization(&mut c, &set_id).await?;
    // Padded to N = 2^M (default feature m2 → M=2 → N=4)
    assert_eq!(anonymity_set.len(), 4);

    // CRS for unknown set_id must fail
    assert!(client::get_crs(&mut c, &[0xff; 32]).await.is_err());

    // ── Attach CRS + anonymity set to the merchant state ─────────────────────
    merchant_state.attach_set(crs.clone(), anonymity_set.clone());

    // ════════════════════════════════════════════════════════════════════════
    // Phase 2 (local completion) — Alice attaches the anonymity set
    // ════════════════════════════════════════════════════════════════════════

    let set_id_arr: [u8; 32] = set_id.clone().try_into().unwrap();
    alice
        .register(set_id_arr, anonymity_set.clone())
        .expect("alice's phi must be in the anonymity set");
    assert_eq!(alice.index, Some(0));

    // ════════════════════════════════════════════════════════════════════════
    // Phase 3 — Alice registers her payment identity with merchant_state
    //           (local; no gRPC between beneficiary and merchant)
    // ════════════════════════════════════════════════════════════════════════

    let payment_reg = alice
        .create_payment_registration(&crs, merchant_state.merchant.merchant_id)
        .expect("ZK proof generation must succeed");

    assert_eq!(payment_reg.service_index, merchant_state.merchant.merchant_id);
    assert_eq!(payment_reg.set_id, set_id_arr);
    assert_eq!(payment_reg.friendly_name, "alice");

    // ════════════════════════════════════════════════════════════════════════
    // Phase 4 — merchant_state receives and verifies Alice's registration
    //           (local; no gRPC)
    // ════════════════════════════════════════════════════════════════════════

    let pseudonym = merchant_state
        .receive_payment_registration(&payment_reg)
        .expect("valid proof must be accepted");

    assert_eq!(pseudonym, payment_reg.pseudonym);
    assert_eq!(merchant_state.merchant.registered_identities.len(), 1);

    // Replay must be rejected
    assert_eq!(
        merchant_state.receive_payment_registration(&payment_reg),
        Err("pseudonym already registered")
    );

    // ════════════════════════════════════════════════════════════════════════
    // Phase 5 — Alice requests payment; merchant_state verifies and pays
    //           (local; no gRPC)
    // ════════════════════════════════════════════════════════════════════════

    let amount_sats: u64 = 50_000;
    let payment_req = create_payment_request(
        &alice.credential.r,
        &merchant_state.merchant.name,
        &crs.g,
        amount_sats,
    );

    // Merchant verifies the Schnorr proof
    let valid = verify_payment_request(&crs.g, &payment_req.pseudonym, &payment_req.proof);
    assert!(valid, "payment request Schnorr proof must verify");

    // Merchant looks up the registered identity by pseudonym
    let registered = merchant_state
        .merchant
        .registered_identities
        .get(&payment_req.pseudonym)
        .expect("pseudonym must be registered from Phase 3");
    assert_eq!(registered.friendly_name, "alice");

    // Merchant derives the payment address and pays
    let pay_addr =
        MerchantState::payment_address(&payment_req.pseudonym, bitcoin::Network::Regtest)
            .expect("valid pseudonym must produce a Bitcoin address");
    assert!(
        pay_addr.to_string().starts_with("bcrt1p"),
        "regtest P2TR address must start with bcrt1p, got: {}",
        pay_addr
    );

    // Sanity: pseudonym_to_address gives the same result
    let pay_addr2 = pseudonym_to_address(&payment_req.pseudonym, bitcoin::Network::Regtest)?;
    assert_eq!(pay_addr, pay_addr2);

    server.abort();
    Ok(())
}
