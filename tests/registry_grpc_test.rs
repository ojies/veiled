//! End-to-end integration test for the registry gRPC service.
//!
//! Mirrors the exact protocol steps performed by the `beneficiary` and `merchant`
//! binaries, using the same `veiled::client` functions so behaviour is identical.
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
use veiled::client;
use veiled::core::beneficiary::Beneficiary;
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

/// Mine `count` blocks to `addr`; return the block hashes.
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

/// Return the coinbase txid (display-order hex) of the first transaction in a block.
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

    // ── Setup: registry store with real RPC, config capacity = 2 ────────────
    let store = Arc::new(Mutex::new(RegistryStore::new(
        Some(rpc.clone()),
        None,
    )));
    let config = Config {
        beneficiary_capacity: 2,
        ..Config::default()
    };

    // Grab the wallet address before starting the server so we can mine to it.
    let registry_addr = store.lock().await.wallet_address.to_string();

    // ── Fund registry wallet ─────────────────────────────────────────────────
    // Mine 102 blocks directly to the registry address:
    //   - Coinbase outputs from blocks 0–2 serve as payment-proof txids.
    //   - After 102 confirmations the first coinbase is mature; BDK can spend
    //     it when building the commitment transaction in finalize_set.
    let blocks = mine_to(&rpc, 102, &registry_addr);
    let merchant_payment_txid  = hex::decode(coinbase_txid(&rpc, &blocks[0]))?;
    let beneficiary1_payment_txid = hex::decode(coinbase_txid(&rpc, &blocks[1]))?;
    let beneficiary2_payment_txid = hex::decode(coinbase_txid(&rpc, &blocks[2]))?;

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
    // (mirrors src/bin/merchant.rs steps 1-4)
    // ════════════════════════════════════════════════════════════════════════

    // Step 0.1: query fees
    let (beneficiary_fee, merchant_fee) = client::get_fees(&mut c).await?;
    assert!(merchant_fee > 0 && beneficiary_fee > 0);

    // Step 0.2: query registry address (zero set_id = before any set exists)
    let (addr_str, internal_key) = client::get_registry_address(&mut c, &[0u8; 32]).await?;
    assert!(addr_str.starts_with("bcrt1p"), "expected bcrt1p, got: {}", addr_str);
    assert_eq!(internal_key.len(), 32);

    // Step 0.3: register merchant — coinbase vout[0] proves payment
    client::register_merchant(
        &mut c,
        "Test Merchant",
        "http://test.com",
        "merchant@example.com",
        "+987654321",
        merchant_payment_txid.clone(),
        0,
    )
    .await?;

    // 0.3a duplicate merchant must fail
    assert!(client::register_merchant(
        &mut c,
        "Test Merchant",
        "http://test.com",
        "dup@example.com",
        "0",
        merchant_payment_txid,
        0,
    )
    .await
    .is_err());

    // 0.4: verify the merchant list
    let merchants = client::get_merchants(&mut c).await?;
    assert_eq!(merchants.len(), 1);
    assert_eq!(merchants[0].name, "Test Merchant");
    assert_eq!(merchants[0].credential_generator.len(), 33);

    // ════════════════════════════════════════════════════════════════════════
    // Phase 2 — Beneficiary registration
    // (mirrors src/bin/beneficiary.rs steps 4-5; credential creation in step 7
    //  happens post-finalization when the CRS is available)
    // ════════════════════════════════════════════════════════════════════════

    // Use plain secp256k1 public keys as phi — registration only requires a
    // valid 33-byte compressed point; ZK-valid phi is tested in full_flow_test.
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let phi1 = bitcoin::secp256k1::PublicKey::from_secret_key(
        &secp,
        &bitcoin::secp256k1::SecretKey::from_slice(&[0x01; 32])?,
    )
    .serialize()
    .to_vec();
    let phi2 = bitcoin::secp256k1::PublicKey::from_secret_key(
        &secp,
        &bitcoin::secp256k1::SecretKey::from_slice(&[0x02; 32])?,
    )
    .serialize()
    .to_vec();

    // Step 2.1: register Alice
    let alice_index = client::register_beneficiary(
        &mut c,
        phi1.clone(),
        "Alice",
        "alice@example.com",
        "+123456789",
        beneficiary1_payment_txid.clone(),
        0,
    )
    .await?;
    assert_eq!(alice_index, 0);

    // Step 2.1a: duplicate phi must fail
    assert!(client::register_beneficiary(
        &mut c,
        phi1,
        "AliceDup",
        "",
        "",
        beneficiary1_payment_txid,
        0,
    )
    .await
    .is_err());

    // Step 2.2: register Bob
    client::register_beneficiary(
        &mut c,
        phi2,
        "Bob",
        "bob@example.com",
        "+987654321",
        beneficiary2_payment_txid,
        0,
    )
    .await?;

    // Step 2.3: third beneficiary must fail (beneficiary_capacity = 2)
    let phi3 = bitcoin::secp256k1::PublicKey::from_secret_key(
        &secp,
        &bitcoin::secp256k1::SecretKey::from_slice(&[0x03; 32])?,
    )
    .serialize()
    .to_vec();
    // Need a fresh valid payment txid; mine one more block.
    let extra = mine_to(&rpc, 1, &registry_addr);
    let extra_txid = hex::decode(coinbase_txid(&rpc, &extra[0]))?;
    assert!(
        client::register_beneficiary(&mut c, phi3, "Carol", "", "", extra_txid, 0,)
            .await
            .is_err()
    );

    // ════════════════════════════════════════════════════════════════════════
    // Finalization — broadcasts real Taproot commitment tx
    // ════════════════════════════════════════════════════════════════════════

    let set_id = client::finalize_set(&mut c).await?;
    assert_eq!(set_id.len(), 32, "set_id must be a 32-byte commitment txid");

    // Mine 1 confirmation block for the commitment tx
    mine_to(&rpc, 1, &registry_addr);

    // ════════════════════════════════════════════════════════════════════════
    // Post-finalization — CRS, anonymity set, subscription
    // (mirrors src/bin/beneficiary.rs steps 6-7 and merchant steps 5-6)
    // ════════════════════════════════════════════════════════════════════════

    // Step: fetch CRS
    let crs = client::get_crs(&mut c, &set_id).await?;
    assert!(crs.num_merchants() > 0);

    // Verify CRS for an unknown set_id fails
    assert!(client::get_crs(&mut c, &[0xff; 32]).await.is_err());

    // Step: subscribe / wait for finalized set (already finalized → returns immediately)
    let anonymity_set = client::wait_for_finalization(&mut c, &set_id).await?;
    // Padded to N = 2^M (default feature m2 → M=2 → N=4)
    assert_eq!(anonymity_set.len(), 4);

    // Verify an unknown set_id hangs (don't test that here — would block forever).
    // Instead verify a bad set_id on get_crs, which fails fast.
    assert!(client::get_crs(&mut c, &[0xab; 32]).await.is_err());

    // Step: local beneficiary registration (find own index in the anonymity set)
    // Use a Beneficiary created with the real CRS to verify the crypto path.
    let mut beneficiary = Beneficiary::new(&crs, "Alice");
    // Register Alice's real phi (not in the set — just checking the error path).
    let result = beneficiary.register(set_id.clone().try_into().unwrap(), anonymity_set.clone());
    // Alice's phi was generated from the CRS we just fetched, not from [0x01;32],
    // so it won't be in the set — this verifies the error is returned correctly.
    assert!(result.is_err(), "phi not in set should return an error");

    server.abort();
    Ok(())
}
