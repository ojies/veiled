//! Wallet management binary for the Veiled web UI.
//!
//! Reads a JSON command from stdin, executes the wallet operation via bitcoind RPC,
//! writes JSON result to stdout. Wallet state (mnemonic, descriptors) persisted to
//! a JSON file per participant.
//!
//! Commands:
//!   create-wallet      Create a new BIP86 P2TR wallet
//!   get-balance        Get wallet balance (confirmed/unconfirmed)
//!   get-address        Get a new receive address
//!   send               Send BTC to an address
//!   faucet             Mine regtest blocks to fund an address
//!   get-tx             Get transaction details
//!   get-tx-history     List wallet transactions

use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Read;

// ── Command dispatch ──

#[derive(Deserialize)]
struct Command {
    command: String,
    #[serde(flatten)]
    params: serde_json::Value,
}

// ── Parameter structs ──

#[derive(Deserialize)]
struct CreateWalletParams {
    state_path: String,
    name: String,
    rpc_url: Option<String>,
    rpc_user: Option<String>,
    rpc_pass: Option<String>,
}

#[derive(Deserialize)]
struct GetBalanceParams {
    state_path: String,
    rpc_url: Option<String>,
    rpc_user: Option<String>,
    rpc_pass: Option<String>,
}

#[derive(Deserialize)]
struct GetAddressParams {
    state_path: String,
    rpc_url: Option<String>,
    rpc_user: Option<String>,
    rpc_pass: Option<String>,
}

#[derive(Deserialize)]
struct SendParams {
    state_path: String,
    to_address: String,
    amount_sats: u64,
    rpc_url: Option<String>,
    rpc_user: Option<String>,
    rpc_pass: Option<String>,
}

#[derive(Deserialize)]
struct FaucetParams {
    address: String,
    blocks: Option<u64>,
    rpc_url: Option<String>,
    rpc_user: Option<String>,
    rpc_pass: Option<String>,
}

#[derive(Deserialize)]
struct GetTxParams {
    txid: String,
    rpc_url: Option<String>,
    rpc_user: Option<String>,
    rpc_pass: Option<String>,
}

#[derive(Deserialize)]
struct GetTxHistoryParams {
    state_path: String,
    rpc_url: Option<String>,
    rpc_user: Option<String>,
    rpc_pass: Option<String>,
}

// ── Wallet state (persisted to JSON) ──

#[derive(Serialize, Deserialize)]
struct WalletState {
    mnemonic: String,
    wallet_name: String,
    descriptor: String,
    change_descriptor: String,
    address: String,
    network: String,
}

// ── Error response ──

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ── Helpers ──

fn default_url() -> String {
    "http://localhost:18443".into()
}
fn default_user() -> String {
    "veiled".into()
}
fn default_pass() -> String {
    "veiled".into()
}

fn rpc_client(url: &str, user: &str, pass: &str, wallet: Option<&str>) -> Result<Client, String> {
    let full_url = match wallet {
        Some(w) => format!("{}/wallet/{}", url.trim_end_matches('/'), w),
        None => url.to_string(),
    };
    Client::new(&full_url, Auth::UserPass(user.to_string(), pass.to_string()))
        .map_err(|e| format!("RPC connection failed: {e}"))
}

fn load_state(path: &str) -> Result<WalletState, String> {
    let data = std::fs::read_to_string(path).map_err(|e| format!("read state: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("parse state: {e}"))
}

// ── Handlers ──

fn handle_create_wallet(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: CreateWalletParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);

    // Idempotent: if wallet state already exists, return it
    if let Ok(state) = load_state(&p.state_path) {
        // Try to load wallet in bitcoind (no-op if already loaded)
        let base = rpc_client(&rpc_url, &rpc_user, &rpc_pass, None)?;
        let _ = base.call::<serde_json::Value>("loadwallet", &[json!(state.wallet_name)]);

        return Ok(json!({
            "address": state.address,
            "mnemonic": state.mnemonic,
            "wallet_name": state.wallet_name,
            "existing": true,
        }));
    }

    // Generate BIP39 mnemonic (12 words = 16 bytes entropy)
    let mut entropy = [0u8; 16];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut entropy);
    let mnemonic =
        bip39::Mnemonic::from_entropy(&entropy).map_err(|e| format!("mnemonic: {e}"))?;
    let seed = mnemonic.to_seed("");

    // BIP32 key derivation: m/86'/1'/0' (BIP86 P2TR, testnet/regtest)
    let secp = Secp256k1::new();
    let master =
        Xpriv::new_master(Network::Regtest, &seed).map_err(|e| format!("master key: {e}"))?;
    let path: DerivationPath = "m/86'/1'/0'".parse().unwrap();
    let account = master
        .derive_priv(&secp, &path)
        .map_err(|e| format!("derive: {e}"))?;
    let fingerprint = master.fingerprint(&secp);

    // BIP86 P2TR descriptors (xprv for signing capability)
    let recv_desc = format!("tr([{}/86'/1'/0']{}/0/*)", fingerprint, account);
    let change_desc = format!("tr([{}/86'/1'/0']{}/1/*)", fingerprint, account);

    // Create blank descriptor wallet in bitcoind (or load if it already exists)
    let base_client = rpc_client(&rpc_url, &rpc_user, &rpc_pass, None)?;
    let create_result = base_client.call::<serde_json::Value>(
        "createwallet",
        &[
            json!(p.name),
            json!(false), // disable_private_keys
            json!(true),  // blank
            json!(""),    // passphrase
            json!(false), // avoid_reuse
            json!(true),  // descriptors
        ],
    );
    if let Err(e) = &create_result {
        let msg = e.to_string();
        if msg.contains("already exists") || msg.contains("Database already") {
            // Wallet exists in bitcoind — try loading it
            let _ = base_client.call::<serde_json::Value>("loadwallet", &[json!(p.name)]);
        } else {
            return Err(format!("createwallet: {e}"));
        }
    }

    // Get descriptor checksums from bitcoind
    let wallet_client = rpc_client(&rpc_url, &rpc_user, &rpc_pass, Some(&p.name))?;

    let recv_info: serde_json::Value = wallet_client
        .call("getdescriptorinfo", &[json!(recv_desc)])
        .map_err(|e| format!("getdescriptorinfo: {e}"))?;
    let change_info: serde_json::Value = wallet_client
        .call("getdescriptorinfo", &[json!(change_desc)])
        .map_err(|e| format!("getdescriptorinfo: {e}"))?;

    let recv_desc_cs = recv_info["descriptor"]
        .as_str()
        .ok_or("missing recv descriptor checksum")?;
    let change_desc_cs = change_info["descriptor"]
        .as_str()
        .ok_or("missing change descriptor checksum")?;

    // Import descriptors with private keys
    wallet_client
        .call::<serde_json::Value>(
            "importdescriptors",
            &[json!([
                {
                    "desc": recv_desc_cs,
                    "active": true,
                    "range": [0, 100],
                    "timestamp": "now",
                    "internal": false,
                },
                {
                    "desc": change_desc_cs,
                    "active": true,
                    "range": [0, 100],
                    "timestamp": "now",
                    "internal": true,
                }
            ])],
        )
        .map_err(|e| format!("importdescriptors: {e}"))?;

    // Get first receive address (P2TR / bech32m)
    let addr: serde_json::Value = wallet_client
        .call("getnewaddress", &[json!(""), json!("bech32m")])
        .map_err(|e| format!("getnewaddress: {e}"))?;
    let address = addr
        .as_str()
        .ok_or("bad address response")?
        .to_string();

    // Persist wallet state
    let state = WalletState {
        mnemonic: mnemonic.to_string(),
        wallet_name: p.name.clone(),
        descriptor: recv_desc,
        change_descriptor: change_desc,
        address: address.clone(),
        network: "regtest".to_string(),
    };
    let state_json =
        serde_json::to_string_pretty(&state).map_err(|e| format!("serialize: {e}"))?;

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&p.state_path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    std::fs::write(&p.state_path, state_json).map_err(|e| format!("write state: {e}"))?;

    Ok(json!({
        "address": address,
        "mnemonic": mnemonic.to_string(),
        "wallet_name": p.name,
    }))
}

fn handle_get_balance(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: GetBalanceParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let state = load_state(&p.state_path)?;

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);

    let client = rpc_client(&rpc_url, &rpc_user, &rpc_pass, Some(&state.wallet_name))?;

    let balances: serde_json::Value = client
        .call("getbalances", &[])
        .map_err(|e| format!("getbalances: {e}"))?;

    let confirmed_btc = balances["mine"]["trusted"].as_f64().unwrap_or(0.0);
    let unconfirmed_btc = balances["mine"]["untrusted_pending"].as_f64().unwrap_or(0.0);

    let confirmed = (confirmed_btc * 100_000_000.0).round() as u64;
    let unconfirmed = (unconfirmed_btc * 100_000_000.0).round() as u64;

    Ok(json!({
        "confirmed": confirmed,
        "unconfirmed": unconfirmed,
        "total": confirmed + unconfirmed,
    }))
}

fn handle_get_address(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: GetAddressParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let state = load_state(&p.state_path)?;

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);

    let client = rpc_client(&rpc_url, &rpc_user, &rpc_pass, Some(&state.wallet_name))?;

    let addr: serde_json::Value = client
        .call("getnewaddress", &[json!(""), json!("bech32m")])
        .map_err(|e| format!("getnewaddress: {e}"))?;

    Ok(json!({
        "address": addr.as_str().unwrap_or_default(),
    }))
}

fn handle_send(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: SendParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let state = load_state(&p.state_path)?;

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);

    let client = rpc_client(&rpc_url, &rpc_user, &rpc_pass, Some(&state.wallet_name))?;

    // Convert sats to BTC (as f64 for JSON RPC)
    let amount_btc = p.amount_sats as f64 / 100_000_000.0;

    let txid: serde_json::Value = client
        .call(
            "sendtoaddress",
            &[
                json!(p.to_address),
                json!(amount_btc),
            ],
        )
        .map_err(|e| format!("sendtoaddress: {e}"))?;

    Ok(json!({
        "txid": txid.as_str().unwrap_or_default(),
    }))
}

fn handle_faucet(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: FaucetParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);

    let client = rpc_client(&rpc_url, &rpc_user, &rpc_pass, None)?;

    let blocks = p.blocks.unwrap_or(1);

    let block_hashes: Vec<String> = client
        .call("generatetoaddress", &[json!(blocks), json!(p.address)])
        .map_err(|e| format!("generatetoaddress: {e}"))?;

    Ok(json!({
        "blocks_mined": blocks,
        "block_hashes": block_hashes,
    }))
}

fn handle_get_tx(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: GetTxParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);

    let client = rpc_client(&rpc_url, &rpc_user, &rpc_pass, None)?;

    let raw: serde_json::Value = client
        .call(
            "getrawtransaction",
            &[json!(p.txid), json!(true)],
        )
        .map_err(|e| format!("getrawtransaction: {e}"))?;

    Ok(json!({
        "txid": p.txid,
        "confirmations": raw["confirmations"],
        "blockhash": raw["blockhash"],
        "size": raw["size"],
        "vout": raw["vout"],
    }))
}

fn handle_get_tx_history(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: GetTxHistoryParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let state = load_state(&p.state_path)?;

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);

    let client = rpc_client(&rpc_url, &rpc_user, &rpc_pass, Some(&state.wallet_name))?;

    let txs: serde_json::Value = client
        .call("listtransactions", &[json!("*"), json!(50)])
        .map_err(|e| format!("listtransactions: {e}"))?;

    let transactions: Vec<serde_json::Value> = txs
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|tx| {
            let amount_btc = tx["amount"].as_f64().unwrap_or(0.0);
            let amount_sats = (amount_btc.abs() * 100_000_000.0).round() as u64;
            json!({
                "txid": tx["txid"],
                "amount_sats": amount_sats,
                "direction": if amount_btc >= 0.0 { "incoming" } else { "outgoing" },
                "confirmations": tx["confirmations"],
                "address": tx["address"],
                "category": tx["category"],
            })
        })
        .collect();

    Ok(json!({
        "transactions": transactions,
    }))
}

// ── Main ──

fn main() {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .expect("failed to read stdin");

    let cmd: Command = match serde_json::from_str(&input) {
        Ok(c) => c,
        Err(e) => {
            let err = ErrorResponse {
                error: format!("invalid JSON: {e}"),
            };
            println!("{}", serde_json::to_string(&err).unwrap());
            std::process::exit(1);
        }
    };

    let result = match cmd.command.as_str() {
        "create-wallet" => handle_create_wallet(cmd.params),
        "get-balance" => handle_get_balance(cmd.params),
        "get-address" => handle_get_address(cmd.params),
        "send" => handle_send(cmd.params),
        "faucet" => handle_faucet(cmd.params),
        "get-tx" => handle_get_tx(cmd.params),
        "get-tx-history" => handle_get_tx_history(cmd.params),
        other => Err(format!("unknown command: {other}")),
    };

    match result {
        Ok(val) => {
            println!("{}", serde_json::to_string(&val).unwrap());
        }
        Err(e) => {
            let err = ErrorResponse { error: e };
            println!("{}", serde_json::to_string(&err).unwrap());
            std::process::exit(1);
        }
    }
}
