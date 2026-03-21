//! Veiled Simulation: Full multi-party protocol simulation with real wallets
//!
//! Mirrors the UI demo flow with 2 merchants and 4 beneficiaries.
//! Runs an in-process registry + merchant servers, calls the veiled-wallet
//! binary for all wallet operations (same code path as the UI), and
//! simulates the complete Phases 0-5 flow.
//!
//! Requires: bitcoind running on regtest (default: localhost:18443, user: veiled, pass: veiled)
//!
//! Usage: cargo run --bin simulation --release

mod merchant_pb {
    tonic::include_proto!("merchant");
}

use merchant_pb::merchant_service_server::MerchantServiceServer;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use veiled::core::beneficiary::Beneficiary;
use veiled::core::crs::Crs;
use veiled::core::merchant::Merchant;
use veiled::core::payment_identity::serialize_payment_identity_registration_proof;
use veiled::core::request::create_payment_request;
use veiled::core::types::{Commitment, Name};
use veiled::registry::pb::registry_client::RegistryClient;
use veiled::registry::pb::registry_server::RegistryServer;
use veiled::registry::pb::{
    BeneficiaryRequest, CreateSetRequest, FinalizeSetRequest, GetAnonymitySetRequest,
    GetCrsRequest, GetFeesRequest, MerchantRequest,
};
use veiled::registry::service::RegistryService;
use veiled::registry::store::{FeeConfig, RegistryStore};

use merchant_pb::merchant_service_client::MerchantServiceClient;
use merchant_pb::{PaymentRegistrationRequest, PaymentRequestMsg};

use bdk_bitcoind_rpc::bitcoincore_rpc::{Auth, Client};

const REGISTRY_ADDR: &str = "[::1]:50070";
const REGISTRY_URL: &str = "http://[::1]:50070";

const RPC_URL: &str = "http://localhost:18443";
const RPC_USER: &str = "veiled";
const RPC_PASS: &str = "veiled";

struct MerchantConfig {
    name: &'static str,
    origin: &'static str,
    addr: &'static str,
    url: &'static str,
}

const MERCHANTS: [MerchantConfig; 2] = [
    MerchantConfig {
        name: "CoffeeCo",
        origin: "https://coffeeco.com",
        addr: "[::1]:50071",
        url: "http://[::1]:50071",
    },
    MerchantConfig {
        name: "BookStore",
        origin: "https://bookstore.com",
        addr: "[::1]:50072",
        url: "http://[::1]:50072",
    },
];

const BENEFICIARY_NAMES: [&str; 4] = ["alice", "bob", "carol", "dave"];

fn separator(title: &str) {
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  {}", title);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

fn step(msg: &str) {
    println!("  -> {}", msg);
}

// ── veiled-wallet CLI helpers ───────────────────────────────

fn wallet_bin() -> String {
    // Prefer release build, fall back to debug
    let release = "target/release/veiled-wallet";
    if std::path::Path::new(release).exists() {
        return release.to_string();
    }
    "target/debug/veiled-wallet".to_string()
}

fn wallet_cmd(cmd: serde_json::Value) -> Result<serde_json::Value, String> {
    let input = serde_json::to_string(&cmd).map_err(|e| format!("json: {e}"))?;
    let output = std::process::Command::new(wallet_bin())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit()) // show wallet logs
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            child.wait_with_output()
        })
        .map_err(|e| format!("spawn veiled-wallet: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value =
        serde_json::from_str(stdout.trim()).map_err(|e| format!("parse wallet output: {e}: {stdout}"))?;

    if let Some(err) = result.get("error").and_then(|e| e.as_str()) {
        return Err(err.to_string());
    }
    Ok(result)
}

fn state_path(name: &str) -> String {
    format!("/tmp/veiled-sim/wallets/{}.json", name)
}

fn create_wallet(name: &str) -> Result<String, String> {
    let result = wallet_cmd(serde_json::json!({
        "command": "create-wallet",
        "state_path": state_path(name),
        "name": name,
        "rpc_url": RPC_URL,
        "rpc_user": RPC_USER,
        "rpc_pass": RPC_PASS,
    }))?;
    let address = result["address"]
        .as_str()
        .ok_or("no address in create-wallet response")?
        .to_string();
    Ok(address)
}

fn get_balance(name: &str) -> Result<serde_json::Value, String> {
    wallet_cmd(serde_json::json!({
        "command": "get-balance",
        "state_path": state_path(name),
        "rpc_url": RPC_URL,
        "rpc_user": RPC_USER,
        "rpc_pass": RPC_PASS,
    }))
}

fn log_balance(name: &str, label: &str) -> Result<(), String> {
    let bal = get_balance(name)?;
    let confirmed = bal["confirmed"].as_u64().unwrap_or(0);
    let unconfirmed = bal["unconfirmed"].as_u64().unwrap_or(0);
    let total = bal["total"].as_u64().unwrap_or(0);
    step(&format!(
        "{} balance: {} confirmed, {} unconfirmed ({} total) sats",
        label, confirmed, unconfirmed, total
    ));
    Ok(())
}

fn faucet(address: &str, blocks: u64) -> Result<(), String> {
    wallet_cmd(serde_json::json!({
        "command": "faucet",
        "address": address,
        "blocks": blocks,
        "rpc_url": RPC_URL,
        "rpc_user": RPC_USER,
        "rpc_pass": RPC_PASS,
    }))?;
    Ok(())
}

/// Send sats and return (txid_hex, vout)
fn send_to(from_name: &str, to_address: &str, amount_sats: u64) -> Result<(String, u32), String> {
    let result = wallet_cmd(serde_json::json!({
        "command": "send",
        "state_path": state_path(from_name),
        "to_address": to_address,
        "amount_sats": amount_sats,
        "rpc_url": RPC_URL,
        "rpc_user": RPC_USER,
        "rpc_pass": RPC_PASS,
    }))?;

    let txid = result["txid"]
        .as_str()
        .ok_or("no txid in send response")?
        .to_string();

    // Find the vout via getrawtransaction
    let rpc = Client::new(RPC_URL, Auth::UserPass(RPC_USER.into(), RPC_PASS.into()))
        .map_err(|e| format!("rpc: {e}"))?;
    let params = vec![serde_json::json!(txid), serde_json::json!(true)];
    let raw: serde_json::Value = bdk_bitcoind_rpc::bitcoincore_rpc::RpcApi::call(&rpc, "getrawtransaction", &params)
        .map_err(|e| format!("getrawtx: {e}"))?;

    let vout = raw["vout"]
        .as_array()
        .ok_or_else(|| "no vout".to_string())?
        .iter()
        .position(|o| o["scriptPubKey"]["address"].as_str() == Some(to_address))
        .ok_or_else(|| format!("vout not found for {} in tx {}", to_address, txid))? as u32;

    Ok((txid, vout))
}

fn rpc_client() -> Result<Client, String> {
    Client::new(RPC_URL, Auth::UserPass(RPC_USER.into(), RPC_PASS.into()))
        .map_err(|e| format!("RPC: {e}"))
}

fn get_block_count() -> Result<u64, String> {
    let rpc = rpc_client()?;
    let empty: Vec<serde_json::Value> = vec![];
    bdk_bitcoind_rpc::bitcoincore_rpc::RpcApi::call(&rpc, "getblockcount", &empty)
        .map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("  VEILED - Verified Payments, Veiled Identities");
    println!("  Full protocol simulation with real wallets (Phases 0-5)");
    println!("  2 merchants, 4 beneficiaries, on-chain fees via regtest");
    println!("  Using: {}", wallet_bin());
    println!();

    // Clean up any prior simulation state
    let _ = std::fs::remove_dir_all("/tmp/veiled-sim");
    std::fs::create_dir_all("/tmp/veiled-sim/wallets").map_err(|e| format!("mkdir: {e}"))?;

    // ── Connect to bitcoind ─────────────────────────────────────
    separator("Connecting to bitcoind (regtest)");

    let block_count = get_block_count()?;
    step(&format!("Connected to bitcoind at {} (block height: {})", RPC_URL, block_count));

    // ── Create miner wallet and pre-mine ────────────────────────
    separator("Pre-mining (miner wallet + maturity blocks)");

    let miner_addr = create_wallet("miner")?;
    step(&format!("Miner address: {}", miner_addr));

    // Mine 10 blocks to miner (coinbase rewards), then 101 maturity blocks to
    // a throwaway address. Keeps miner wallet with only 10 UTXOs for fast sends.
    faucet(&miner_addr, 10)?;
    step("Mined 10 blocks to miner");
    let throwaway_addr = create_wallet("throwaway")?;
    faucet(&throwaway_addr, 101)?;
    step("Mined 101 maturity blocks to throwaway");
    log_balance("miner", "Miner")?;

    // ── Start Registry with RPC ─────────────────────────────────
    separator("Starting Registry Server (with on-chain verification)");

    let rpc_arc = Arc::new(rpc_client()?);
    let fee_config = FeeConfig {
        min_sats_per_user: 1000,
        merchant_registration_fee: 3000,
    };
    let store = Arc::new(Mutex::new(RegistryStore::new(
        Some(rpc_arc.clone()),
        fee_config.clone(),
        None,
    )));

    let registry_address = {
        let s = store.lock().await;
        s.wallet_address.to_string()
    };
    step(&format!("Registry address: {}", registry_address));

    let registry_service = RegistryService::new(store);
    let registry_addr = REGISTRY_ADDR.parse()?;

    tokio::spawn(async move {
        Server::builder()
            .add_service(RegistryServer::new(registry_service))
            .serve(registry_addr)
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    step(&format!("Registry listening on {}", REGISTRY_ADDR));

    let mut client = RegistryClient::connect(REGISTRY_URL).await?;

    let fees_resp = client.get_fees(GetFeesRequest {}).await?.into_inner();
    step(&format!(
        "Fees: merchant={} sats, beneficiary={} sats",
        fees_resp.merchant_fee, fees_resp.beneficiary_fee
    ));

    // ── Create merchant wallets + pay fees + register ───────────
    separator("Phase 0 - Merchant Registration (with on-chain fee payment)");

    for m in &MERCHANTS {
        let m_name = format!("merchant-{}", m.name.to_lowercase());
        let m_addr = create_wallet(&m_name)?;
        step(&format!("{}: wallet {}...", m.name, &m_addr[..20]));

        // Fund merchant from miner
        log_balance("miner", &format!("Miner (before funding {})", m.name))?;
        let (fund_txid, _) = send_to("miner", &m_addr, 100_000)?;
        faucet(&miner_addr, 1)?; // confirm
        step(&format!("{}: funded with 100,000 sats (tx {}...)", m.name, &fund_txid[..12]));
        log_balance("miner", &format!("Miner (after funding {})", m.name))?;
        log_balance(&m_name, &format!("{} wallet", m.name))?;

        // Pay registration fee to registry
        let (txid_hex, vout) = send_to(&m_name, &registry_address, fees_resp.merchant_fee)?;
        faucet(&miner_addr, 1)?; // confirm
        step(&format!("{}: paid {} sats fee (tx {}...:{}) ", m.name, fees_resp.merchant_fee, &txid_hex[..12], vout));
        log_balance(&m_name, &format!("{} wallet (after fee)", m.name))?;

        // Register with the registry (send txid in display-order bytes)
        let txid_bytes = hex::decode(&txid_hex).map_err(|e| format!("hex: {e}"))?;
        client
            .register_merchant(MerchantRequest {
                name: m.name.into(),
                origin: m.origin.into(),
                email: format!("pay@{}", m.name.to_lowercase()),
                phone: "".into(),
                funding_txid: txid_bytes,
                funding_vout: vout,
            })
            .await?;
        step(&format!("{}: registered with registry (on-chain verified)", m.name));
    }

    // Create anonymity set
    let merchant_names: Vec<String> = MERCHANTS.iter().map(|m| m.name.to_string()).collect();
    client
        .create_set(CreateSetRequest {
            set_id: 1,
            merchant_names: merchant_names.clone(),
            beneficiary_capacity: BENEFICIARY_NAMES.len() as u32,
            sats_per_user: fees_resp.beneficiary_fee,
        })
        .await?;
    step(&format!(
        "Created anonymity set #1 (capacity: {}, merchants: {})",
        BENEFICIARY_NAMES.len(),
        merchant_names.join(", ")
    ));

    let crs_bytes = client
        .get_crs(GetCrsRequest { set_id: 1 })
        .await?
        .into_inner()
        .crs_bytes;
    let crs = Crs::from_bytes(&crs_bytes)?;
    step(&format!("CRS generated: {} merchants", crs.merchants.len()));

    // ── Start Merchant Servers ──────────────────────────────────
    separator("Starting Merchant Servers");

    let mut merchant_handles = Vec::new();
    for m in &MERCHANTS {
        let crs_clone = crs.clone();
        let name = m.name.to_string();
        let addr = m.addr.to_string();
        let handle = tokio::spawn(async move {
            start_merchant_server(&name, &addr, 1, crs_clone).await
        });
        merchant_handles.push(handle);
    }

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    for m in &MERCHANTS {
        step(&format!("{} listening on {}", m.name, m.addr));
    }

    // ── Phase 1: Beneficiaries Create Credentials ───────────────
    separator("Phase 1 - Create Identity (offline credential creation)");

    let mut beneficiaries: Vec<Beneficiary> = Vec::new();
    for name in &BENEFICIARY_NAMES {
        let b = Beneficiary::new(&crs, name);
        step(&format!(
            "{:<6} identity created, phi = {}...",
            name,
            hex::encode(&b.credential.phi.0[..8])
        ));
        beneficiaries.push(b);
    }

    // ── Phase 2: Pay fees + Register + Finalize ─────────────────
    separator("Phase 2 - Registration (on-chain fee payment + finalization)");

    for (i, name) in BENEFICIARY_NAMES.iter().enumerate() {
        let b_name = format!("beneficiary-{}", name);
        let b_addr = create_wallet(&b_name)?;

        // Fund beneficiary from miner
        log_balance("miner", &format!("Miner (before funding {})", name))?;
        let (fund_txid, _) = send_to("miner", &b_addr, 100_000)?;
        faucet(&miner_addr, 1)?;
        step(&format!("{:<6} funded with 100,000 sats (tx {}...)", name, &fund_txid[..12]));
        log_balance("miner", &format!("Miner (after funding {})", name))?;
        log_balance(&b_name, &format!("{} wallet", name))?;

        // Pay registration fee
        let (txid_hex, vout) = send_to(&b_name, &registry_address, fees_resp.beneficiary_fee)?;
        faucet(&miner_addr, 1)?;
        step(&format!(
            "{:<6} paid {} sats fee (tx {}...:{}) -> registering...",
            name, fees_resp.beneficiary_fee, &txid_hex[..12], vout
        ));
        log_balance(&b_name, &format!("{} wallet (after fee)", name))?;

        let txid_bytes = hex::decode(&txid_hex).map_err(|e| format!("hex: {e}"))?;
        let res = client
            .register_beneficiary(BeneficiaryRequest {
                set_id: 1,
                phi: beneficiaries[i].credential.phi.0.to_vec(),
                name: name.to_string(),
                email: format!("{}@example.com", name),
                phone: "".into(),
                funding_txid: txid_bytes,
                funding_vout: vout,
            })
            .await?
            .into_inner();
        step(&format!("{:<6} registered at index {} (on-chain verified)", name, res.index));
    }

    // Subscribe to finalization
    let mut sub_client = RegistryClient::connect(REGISTRY_URL).await?;
    let sub_handle = tokio::spawn(async move {
        let response = sub_client
            .subscribe_set_finalization(GetAnonymitySetRequest { set_id: 1 })
            .await
            .unwrap();
        let mut stream = response.into_inner();
        stream.message().await.unwrap().unwrap()
    });

    step("Finalizing set #1 (Taproot commitment)...");
    client
        .finalize_set(FinalizeSetRequest { set_id: 1 })
        .await?;

    let finalized =
        tokio::time::timeout(std::time::Duration::from_secs(5), sub_handle).await??;
    step(&format!(
        "Set #1 finalized: {} members sealed in anonymity set",
        finalized.count
    ));

    let anonymity_set: Vec<Commitment> = finalized
        .commitments
        .into_iter()
        .map(|b| {
            let arr: [u8; 33] = b.try_into().expect("33 bytes");
            Commitment(arr)
        })
        .collect();

    for (i, name) in BENEFICIARY_NAMES.iter().enumerate() {
        let mut set_id_bytes = [0u8; 32];
        set_id_bytes[..8].copy_from_slice(&1u64.to_le_bytes());
        beneficiaries[i].register(set_id_bytes, anonymity_set.clone())?;
        step(&format!(
            "{:<6} found own commitment at index {}",
            name,
            beneficiaries[i].index.unwrap()
        ));
    }

    // Wait for merchant servers to receive finalized set
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // ── Phase 3-4: Payment Identity Registration ────────────────
    separator("Phase 3-4 - Payment Identity Registration (ZK Proofs)");

    // Alice -> both, Bob -> CoffeeCo, Carol -> BookStore, Dave -> both
    let registrations: [(usize, usize); 6] = [
        (0, 1), (0, 2),
        (1, 1),
        (2, 2),
        (3, 1), (3, 2),
    ];

    let mut coffeeco_client = MerchantServiceClient::connect(MERCHANTS[0].url).await?;
    let mut bookstore_client = MerchantServiceClient::connect(MERCHANTS[1].url).await?;

    let mut alice_pseudonyms: Vec<(String, [u8; 33])> = Vec::new();
    let mut dave_pseudonyms: Vec<(String, [u8; 33])> = Vec::new();

    for &(ben_idx, merchant_id) in &registrations {
        let name = BENEFICIARY_NAMES[ben_idx];
        let merchant_name = MERCHANTS[merchant_id - 1].name;

        let reg = beneficiaries[ben_idx]
            .create_payment_registration(&crs, merchant_id)
            .map_err(|e| e.to_string())?;
        let proof_bytes = serialize_payment_identity_registration_proof(&reg.proof);

        let req = PaymentRegistrationRequest {
            pseudonym: reg.pseudonym.to_vec(),
            public_nullifier: reg.public_nullifier.to_vec(),
            set_id: u64::from_le_bytes(reg.set_id[..8].try_into().unwrap()),
            service_index: reg.service_index as u32,
            friendly_name: reg.friendly_name.clone(),
            proof: proof_bytes,
        };

        match merchant_id {
            1 => coffeeco_client.submit_payment_registration(req).await?,
            2 => bookstore_client.submit_payment_registration(req).await?,
            _ => unreachable!(),
        };

        step(&format!(
            "{:<6} -> {:<10} pseudonym = {}... (ZK proof verified)",
            name,
            merchant_name,
            hex::encode(&reg.pseudonym[..8])
        ));

        if ben_idx == 0 {
            alice_pseudonyms.push((merchant_name.to_string(), reg.pseudonym));
        }
        if ben_idx == 3 {
            dave_pseudonyms.push((merchant_name.to_string(), reg.pseudonym));
        }
    }

    // ── Cross-merchant unlinkability demonstration ───────────────
    separator("Privacy: Cross-Merchant Unlinkability");

    for (label, pseudonyms) in [("Alice", &alice_pseudonyms), ("Dave", &dave_pseudonyms)] {
        println!();
        println!("  {} registered with both merchants. Pseudonyms:", label);
        for (merchant, pseudo) in pseudonyms {
            println!("    {:<10}  {}", merchant, hex::encode(pseudo));
        }
        let differ = pseudonyms[0].1 != pseudonyms[1].1;
        println!(
            "  Pseudonyms differ: {} -> merchants CANNOT link these identities",
            if differ { "YES" } else { "NO" }
        );
    }

    // ── Phase 5: Payment Requests ───────────────────────────────
    separator("Phase 5 - Payments (Schnorr Authentication)");

    let payments: [(usize, usize, u64); 6] = [
        (0, 1, 50_000),
        (0, 2, 120_000),
        (1, 1, 30_000),
        (2, 2, 75_000),
        (3, 1, 25_000),
        (3, 2, 90_000),
    ];

    let mut total_sats = 0u64;
    for &(ben_idx, merchant_id, amount) in &payments {
        let name = BENEFICIARY_NAMES[ben_idx];
        let merchant_name = MERCHANTS[merchant_id - 1].name;
        let merchant_name_typed = Name(merchant_name.to_string());

        let pay = create_payment_request(
            &beneficiaries[ben_idx].credential.r,
            &merchant_name_typed,
            &crs.g,
            amount,
        );

        let req = PaymentRequestMsg {
            amount,
            pseudonym: pay.pseudonym.to_vec(),
            proof_r: pay.proof.r.to_vec(),
            proof_s: pay.proof.s.to_vec(),
        };

        let res = match merchant_id {
            1 => coffeeco_client.submit_payment_request(req).await?,
            2 => bookstore_client.submit_payment_request(req).await?,
            _ => unreachable!(),
        }
        .into_inner();

        step(&format!(
            "{:<6} -> {:<10} {:>7} sats  pay to {}",
            name, merchant_name, amount, res.address
        ));
        total_sats += amount;
    }

    // ── Summary ─────────────────────────────────────────────────
    separator("Simulation Complete");

    log_balance("miner", "Miner (final)")?;
    let final_height = get_block_count()?;
    println!();
    println!("  Merchants:       {} ({})", MERCHANTS.len(), merchant_names.join(", "));
    println!("  Beneficiaries:   {} ({})", BENEFICIARY_NAMES.len(), BENEFICIARY_NAMES.join(", "));
    println!("  Anonymity set:   {} commitments (2^{})",
        anonymity_set.len(),
        (anonymity_set.len() as f64).log2() as u32
    );
    println!("  Registrations:   {} payment identities across {} merchants",
        registrations.len(), MERCHANTS.len()
    );
    println!("  Payments:        {} requests totalling {} sats ({:.4} BTC)",
        payments.len(), total_sats, total_sats as f64 / 100_000_000.0
    );
    println!("  Chain:           {} blocks mined ({} new)", final_height, final_height - block_count);
    println!();
    println!("  On-chain activity:");
    println!("    [x] {} merchant fee payments verified by registry", MERCHANTS.len());
    println!("    [x] {} beneficiary fee payments verified by registry", BENEFICIARY_NAMES.len());
    println!("    [x] All registrations backed by real Bitcoin transactions");
    println!();
    println!("  Privacy guarantees demonstrated:");
    println!("    [x] ZK proofs reveal nothing about which commitment is whose");
    println!("    [x] Pseudonyms are cryptographically unlinkable across merchants");
    println!("    [x] Each merchant sees only their own view per beneficiary");
    println!("    [x] Schnorr authentication proves pseudonym ownership without revealing identity");
    println!("    [ ] friendly_name IS shared (design trade-off for payment coordination)");
    println!();

    for h in merchant_handles {
        h.abort();
    }

    Ok(())
}

/// Starts a merchant gRPC server that subscribes to set finalization
async fn start_merchant_server(
    name: &str,
    listen_addr: &str,
    set_id: u64,
    crs: Crs,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut registry_client = RegistryClient::connect(REGISTRY_URL).await?;
    let response = registry_client
        .subscribe_set_finalization(GetAnonymitySetRequest { set_id })
        .await?;
    let mut stream = response.into_inner();
    let anon_res = stream
        .message()
        .await?
        .ok_or("Finalization stream closed")?;

    let anonymity_set: Vec<Commitment> = anon_res
        .commitments
        .into_iter()
        .map(|b| {
            let arr: [u8; 33] = b.try_into().expect("33 bytes");
            Commitment(arr)
        })
        .collect();

    let merchant = Merchant::new(name, &format!("https://{}.com", name.to_lowercase()));

    let merchant_svc = DemoMerchantService {
        merchant: Arc::new(Mutex::new(merchant)),
        crs: Arc::new(crs),
        anonymity_set: Arc::new(anonymity_set),
    };

    let addr = listen_addr.parse()?;
    Server::builder()
        .add_service(MerchantServiceServer::new(merchant_svc))
        .serve(addr)
        .await?;

    Ok(())
}

/// Inline merchant service (mirrors src/bin/merchant/service.rs)
struct DemoMerchantService {
    merchant: Arc<Mutex<Merchant>>,
    crs: Arc<Crs>,
    anonymity_set: Arc<Vec<Commitment>>,
}

#[tonic::async_trait]
impl merchant_pb::merchant_service_server::MerchantService for DemoMerchantService {
    async fn submit_payment_registration(
        &self,
        request: tonic::Request<PaymentRegistrationRequest>,
    ) -> Result<tonic::Response<merchant_pb::PaymentRegistrationResponse>, tonic::Status> {
        let req = request.into_inner();

        let pseudonym: [u8; 33] = req
            .pseudonym
            .try_into()
            .map_err(|_| tonic::Status::invalid_argument("pseudonym must be 33 bytes"))?;

        let public_nullifier: [u8; 33] = req
            .public_nullifier
            .try_into()
            .map_err(|_| tonic::Status::invalid_argument("public_nullifier must be 33 bytes"))?;

        let proof =
            veiled::core::payment_identity::deserialize_payment_identity_registration_proof(
                &req.proof,
            )
            .map_err(|e| tonic::Status::invalid_argument(format!("invalid proof: {}", e)))?;

        let registration = veiled::core::payment_identity::PaymentIdentityRegistration {
            pseudonym,
            public_nullifier,
            set_id: {
                let mut bytes = [0u8; 32];
                bytes[..8].copy_from_slice(&req.set_id.to_le_bytes());
                bytes
            },
            service_index: req.service_index as usize,
            friendly_name: req.friendly_name.clone(),
            proof,
        };

        let mut merchant = self.merchant.lock().await;
        merchant
            .receive_payment_registration(&self.crs, &self.anonymity_set, &registration)
            .map_err(|e| tonic::Status::invalid_argument(e.to_string()))?;

        Ok(tonic::Response::new(
            merchant_pb::PaymentRegistrationResponse {
                message: format!("Registered '{}'", req.friendly_name),
            },
        ))
    }

    async fn submit_payment_request(
        &self,
        request: tonic::Request<PaymentRequestMsg>,
    ) -> Result<tonic::Response<merchant_pb::PaymentRequestResponse>, tonic::Status> {
        let req = request.into_inner();

        let pseudonym: [u8; 33] = req
            .pseudonym
            .try_into()
            .map_err(|_| tonic::Status::invalid_argument("pseudonym must be 33 bytes"))?;

        let proof_r: [u8; 33] = req
            .proof_r
            .try_into()
            .map_err(|_| tonic::Status::invalid_argument("proof_r must be 33 bytes"))?;

        let proof_s: [u8; 32] = req
            .proof_s
            .try_into()
            .map_err(|_| tonic::Status::invalid_argument("proof_s must be 32 bytes"))?;

        let proof = veiled::core::request::PaymentRequestProof {
            r: proof_r,
            s: proof_s,
        };

        if !veiled::core::request::verify_payment_request(&self.crs.g, &pseudonym, &proof) {
            return Err(tonic::Status::invalid_argument("invalid payment proof"));
        }

        let merchant = self.merchant.lock().await;
        let registered = merchant
            .registered_identities
            .get(&pseudonym)
            .ok_or_else(|| tonic::Status::not_found("pseudonym not registered"))?;

        let address =
            veiled::core::request::pseudonym_to_address(&pseudonym, bitcoin::Network::Bitcoin)
                .map_err(|e| tonic::Status::internal(format!("address error: {}", e)))?;

        Ok(tonic::Response::new(
            merchant_pb::PaymentRequestResponse {
                address: address.to_string(),
                friendly_name: registered.friendly_name.clone(),
            },
        ))
    }
}
