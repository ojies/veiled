//! Wallet management binary for the Veiled web UI.
//!
//! Uses bdk_wallet for local BIP86 P2TR wallet management. No bitcoind
//! wallet needed — keys and state are managed entirely by BDK locally.
//! Syncs with bitcoind via bdk_bitcoind_rpc for balance and UTXO data.
//!
//! Reads a JSON command from stdin, writes JSON result to stdout.
//! Wallet state (mnemonic, descriptors) persisted to a JSON file per participant.
//!
//! Commands:
//!   create-wallet      Create a new BIP86 P2TR wallet
//!   get-balance        Get wallet balance (confirmed/unconfirmed)
//!   get-address        Get a new receive address
//!   send               Send BTC to an address
//!   faucet             Mine regtest blocks to fund an address
//!   get-tx             Get transaction details
//!   get-tx-history     List wallet transactions

use bdk_bitcoind_rpc::bitcoincore_rpc::{Auth, Client, RpcApi};
use bdk_bitcoind_rpc::Emitter;
use bdk_wallet::bitcoin::bip32::Xpriv;
use bdk_wallet::bitcoin::{Address, Amount, FeeRate, Network};
use bdk_wallet::chain::ChainPosition;
use bdk_wallet::template::{Bip86, DescriptorTemplate};
#[allow(deprecated)]
use bdk_wallet::{KeychainKind, SignOptions, Wallet};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Read;
use std::str::FromStr;

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

#[derive(Debug, Serialize, Deserialize)]
struct WalletState {
    mnemonic: String,
    wallet_name: String,
    descriptor: String,
    change_descriptor: String,
    address: String,
    #[serde(default)]
    address_index: u32,
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

fn rpc_client(url: &str, user: &str, pass: &str) -> Result<Client, String> {
    Client::new(url, Auth::UserPass(user.to_string(), pass.to_string()))
        .map_err(|e| format!("RPC connection failed: {e}"))
}

fn load_state(path: &str) -> Result<WalletState, String> {
    let data = std::fs::read_to_string(path).map_err(|e| format!("read state: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("parse state: {e}"))
}

fn save_state(path: &str, state: &WalletState) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(state).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("write state: {e}"))
}

/// Recreate a BDK wallet from stored mnemonic.
fn recreate_wallet(state: &WalletState) -> Result<Wallet, String> {
    let mnemonic: bip39::Mnemonic = state
        .mnemonic
        .parse()
        .map_err(|e| format!("parse mnemonic: {e}"))?;
    let seed = mnemonic.to_seed("");
    let xprv = Xpriv::new_master(Network::Regtest, &seed[..])
        .map_err(|e| format!("master key: {e}"))?;

    let mut wallet = Wallet::create(
        Bip86(xprv, KeychainKind::External),
        Bip86(xprv, KeychainKind::Internal),
    )
    .network(Network::Regtest)
    .create_wallet_no_persist()
    .map_err(|e| format!("create wallet: {e}"))?;

    // Advance address index to match stored state
    let _ = wallet.reveal_addresses_to(KeychainKind::External, state.address_index);

    Ok(wallet)
}

/// Sync a BDK wallet with bitcoind blocks and mempool.
fn sync_wallet(wallet: &mut Wallet, rpc: &Client) -> Result<(), String> {
    let tip = wallet.latest_checkpoint();
    let empty_mempool: Vec<std::sync::Arc<bdk_wallet::bitcoin::Transaction>> = vec![];
    let mut emitter = Emitter::new(rpc, tip.clone(), tip.height(), empty_mempool);

    while let Some(block_event) = emitter
        .next_block()
        .map_err(|e| format!("sync block: {e}"))?
    {
        wallet
            .apply_block_connected_to(
                &block_event.block,
                block_event.block_height(),
                block_event.connected_to(),
            )
            .map_err(|e| format!("apply block: {e}"))?;
    }

    // Sync mempool
    let mempool = emitter
        .mempool()
        .map_err(|e| format!("sync mempool: {e}"))?;
    wallet.apply_unconfirmed_txs(mempool.update);

    Ok(())
}

// ── Handlers ──

fn handle_create_wallet(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: CreateWalletParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;

    // Idempotent: if wallet state already exists, return it
    if let Ok(state) = load_state(&p.state_path) {
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
    rand::thread_rng().fill_bytes(&mut entropy[..]);
    let mnemonic =
        bip39::Mnemonic::from_entropy(&entropy[..]).map_err(|e| format!("mnemonic: {e}"))?;
    let seed = mnemonic.to_seed("");

    // Derive BIP86 master key
    let xprv = Xpriv::new_master(Network::Regtest, &seed[..])
        .map_err(|e| format!("master key: {e}"))?;

    // Create BDK wallet with BIP86 P2TR descriptors
    let wallet = Wallet::create(
        Bip86(xprv, KeychainKind::External),
        Bip86(xprv, KeychainKind::Internal),
    )
    .network(Network::Regtest)
    .create_wallet_no_persist()
    .map_err(|e| format!("create wallet: {e}"))?;

    // Get first receive address (P2TR / bech32m)
    let addr_info = wallet.peek_address(KeychainKind::External, 0);
    let address = addr_info.address.to_string();

    // Build descriptor strings for state persistence
    let (desc, key_map, _) = Bip86(xprv, KeychainKind::External)
        .build(Network::Regtest)
        .map_err(|e| format!("build descriptor: {e}"))?;
    let (change_desc, change_key_map, _) = Bip86(xprv, KeychainKind::Internal)
        .build(Network::Regtest)
        .map_err(|e| format!("build descriptor: {e}"))?;

    let state = WalletState {
        mnemonic: mnemonic.to_string(),
        wallet_name: p.name.clone(),
        descriptor: desc.to_string_with_secret(&key_map),
        change_descriptor: change_desc.to_string_with_secret(&change_key_map),
        address: address.clone(),
        address_index: 0,
        network: "regtest".to_string(),
    };
    save_state(&p.state_path, &state)?;

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
    let rpc = rpc_client(&rpc_url, &rpc_user, &rpc_pass)?;

    let mut wallet = recreate_wallet(&state)?;
    sync_wallet(&mut wallet, &rpc)?;

    let balance = wallet.balance();
    let confirmed = balance.confirmed.to_sat();
    let unconfirmed = balance.untrusted_pending.to_sat() + balance.trusted_pending.to_sat();

    Ok(json!({
        "confirmed": confirmed,
        "unconfirmed": unconfirmed,
        "total": confirmed + unconfirmed,
    }))
}

fn handle_get_address(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: GetAddressParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let mut state = load_state(&p.state_path)?;

    let mut wallet = recreate_wallet(&state)?;
    let addr_info = wallet.reveal_next_address(KeychainKind::External);

    state.address_index = addr_info.index;
    state.address = addr_info.address.to_string();
    save_state(&p.state_path, &state)?;

    Ok(json!({
        "address": addr_info.address.to_string(),
    }))
}

fn handle_send(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: SendParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let state = load_state(&p.state_path)?;

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);
    let rpc = rpc_client(&rpc_url, &rpc_user, &rpc_pass)?;

    let mut wallet = recreate_wallet(&state)?;
    sync_wallet(&mut wallet, &rpc)?;

    let to_addr = Address::from_str(&p.to_address)
        .map_err(|e| format!("invalid address: {e}"))?
        .require_network(Network::Regtest)
        .map_err(|e| format!("wrong network: {e}"))?;

    let mut builder = wallet.build_tx();
    builder
        .add_recipient(to_addr.script_pubkey(), Amount::from_sat(p.amount_sats))
        .fee_rate(FeeRate::from_sat_per_vb(2).expect("valid fee rate"));

    let mut psbt = builder.finish().map_err(|e| format!("build tx: {e}"))?;
    #[allow(deprecated)]
    let finalized = wallet
        .sign(&mut psbt, SignOptions::default())
        .map_err(|e| format!("sign: {e}"))?;
    if !finalized {
        return Err("transaction signing incomplete".into());
    }

    let tx = psbt.extract_tx().map_err(|e| format!("extract tx: {e}"))?;
    let txid = tx.compute_txid();

    rpc.send_raw_transaction(&tx)
        .map_err(|e| format!("broadcast: {e}"))?;

    Ok(json!({
        "txid": txid.to_string(),
    }))
}

fn handle_faucet(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: FaucetParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);
    let rpc = rpc_client(&rpc_url, &rpc_user, &rpc_pass)?;

    let blocks = p.blocks.unwrap_or(1);

    let block_hashes: Vec<String> = rpc
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
    let rpc = rpc_client(&rpc_url, &rpc_user, &rpc_pass)?;

    let raw: serde_json::Value = rpc
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
    let rpc = rpc_client(&rpc_url, &rpc_user, &rpc_pass)?;

    let mut wallet = recreate_wallet(&state)?;
    sync_wallet(&mut wallet, &rpc)?;

    let transactions: Vec<serde_json::Value> = wallet
        .transactions()
        .map(|canonical_tx| {
            let tx = &canonical_tx.tx_node.tx;
            let txid = canonical_tx.tx_node.txid;

            let (sent, received) = wallet.sent_and_received(tx);
            let sent_sats = sent.to_sat();
            let received_sats = received.to_sat();

            let net = received_sats as i64 - sent_sats as i64;
            let direction = if net >= 0 { "incoming" } else { "outgoing" };
            let amount_sats = net.unsigned_abs();

            let confirmations: u32 = match &canonical_tx.chain_position {
                ChainPosition::Confirmed { .. } => 1,
                ChainPosition::Unconfirmed { .. } => 0,
            };

            json!({
                "txid": txid.to_string(),
                "amount_sats": amount_sats,
                "direction": direction,
                "confirmations": confirmations,
                "category": if net >= 0 { "receive" } else { "send" },
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ── Command parsing ──

    #[test]
    fn parse_create_wallet_command() {
        let input = json!({"command":"create-wallet","state_path":"/tmp/t.json","name":"w"});
        let cmd: Command = serde_json::from_value(input).unwrap();
        assert_eq!(cmd.command, "create-wallet");
        let p: CreateWalletParams = serde_json::from_value(cmd.params).unwrap();
        assert_eq!(p.name, "w");
        assert!(p.rpc_url.is_none());
    }

    #[test]
    fn parse_send_command() {
        let input = json!({"command":"send","state_path":"/tmp/w.json","to_address":"bcrt1p","amount_sats":50000});
        let cmd: Command = serde_json::from_value(input).unwrap();
        let p: SendParams = serde_json::from_value(cmd.params).unwrap();
        assert_eq!(p.to_address, "bcrt1p");
        assert_eq!(p.amount_sats, 50000);
    }

    #[test]
    fn parse_faucet_default_blocks() {
        let input = json!({"command":"faucet","address":"bcrt1p"});
        let cmd: Command = serde_json::from_value(input).unwrap();
        let p: FaucetParams = serde_json::from_value(cmd.params).unwrap();
        assert!(p.blocks.is_none());
    }

    // ── Missing required params ──

    #[test]
    fn create_wallet_missing_name_fails() {
        let input = json!({"command":"create-wallet","state_path":"/tmp/t.json"});
        let cmd: Command = serde_json::from_value(input).unwrap();
        assert!(serde_json::from_value::<CreateWalletParams>(cmd.params).is_err());
    }

    #[test]
    fn send_missing_address_fails() {
        let input = json!({"command":"send","state_path":"/tmp/w.json","amount_sats":100});
        let cmd: Command = serde_json::from_value(input).unwrap();
        assert!(serde_json::from_value::<SendParams>(cmd.params).is_err());
    }

    // ── WalletState serialization ──

    fn sample_state() -> WalletState {
        WalletState {
            mnemonic: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".into(),
            wallet_name: "test-wallet".into(),
            descriptor: "tr([fp/86'/1'/0']xprv.../0/*)".into(),
            change_descriptor: "tr([fp/86'/1'/0']xprv.../1/*)".into(),
            address: "bcrt1ptest123".into(),
            address_index: 0,
            network: "regtest".into(),
        }
    }

    #[test]
    fn wallet_state_roundtrip() {
        let state = sample_state();
        let json_str = serde_json::to_string_pretty(&state).unwrap();
        let loaded: WalletState = serde_json::from_str(&json_str).unwrap();
        assert_eq!(loaded.wallet_name, state.wallet_name);
        assert_eq!(loaded.address, state.address);
        assert_eq!(loaded.address_index, 0);
    }

    #[test]
    fn wallet_state_backward_compat_no_address_index() {
        // Old state files without address_index should deserialize with default 0
        let json_str = r#"{"mnemonic":"m","wallet_name":"w","descriptor":"d","change_descriptor":"cd","address":"a","network":"regtest"}"#;
        let loaded: WalletState = serde_json::from_str(json_str).unwrap();
        assert_eq!(loaded.address_index, 0);
    }

    // ── load_state / save_state ──

    #[test]
    fn load_state_valid_file() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", serde_json::to_string_pretty(&sample_state()).unwrap()).unwrap();
        let loaded = load_state(f.path().to_str().unwrap()).unwrap();
        assert_eq!(loaded.wallet_name, "test-wallet");
    }

    #[test]
    fn load_state_missing_file() {
        let result = load_state("/nonexistent/wallet.json");
        assert!(result.unwrap_err().contains("read state"));
    }

    #[test]
    fn load_state_invalid_json() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "not json").unwrap();
        assert!(load_state(f.path().to_str().unwrap()).unwrap_err().contains("parse state"));
    }

    #[test]
    fn save_state_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/dir/wallet.json");
        save_state(path.to_str().unwrap(), &sample_state()).unwrap();
        let loaded = load_state(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.wallet_name, "test-wallet");
    }

    // ── Helpers ──

    #[test]
    fn default_rpc_values() {
        assert_eq!(default_url(), "http://localhost:18443");
        assert_eq!(default_user(), "veiled");
        assert_eq!(default_pass(), "veiled");
    }

    #[test]
    fn error_response_serialization() {
        let err = ErrorResponse { error: "fail".into() };
        let v: serde_json::Value = serde_json::to_value(&err).unwrap();
        assert_eq!(v["error"], "fail");
    }

    #[test]
    fn unknown_command_returns_error() {
        let r: Result<serde_json::Value, String> = Err(format!("unknown command: {}", "bogus"));
        assert_eq!(r.unwrap_err(), "unknown command: bogus");
    }

    // ── BIP39 / BIP86 key derivation ──

    #[test]
    fn mnemonic_generates_12_words() {
        let mut entropy = [0u8; 16];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut entropy[..]);
        let m = bip39::Mnemonic::from_entropy(&entropy[..]).unwrap();
        assert_eq!(m.to_string().split_whitespace().count(), 12);
    }

    #[test]
    fn bip86_wallet_creates_p2tr_address() {
        let mnemonic: bip39::Mnemonic =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
                .parse().unwrap();
        let seed = mnemonic.to_seed("");
        let xprv = Xpriv::new_master(Network::Regtest, &seed[..]).unwrap();

        let wallet = Wallet::create(
            Bip86(xprv, KeychainKind::External),
            Bip86(xprv, KeychainKind::Internal),
        )
        .network(Network::Regtest)
        .create_wallet_no_persist()
        .unwrap();

        let addr = wallet.peek_address(KeychainKind::External, 0);
        // BIP86 P2TR addresses on regtest start with "bcrt1p"
        assert!(addr.address.to_string().starts_with("bcrt1p"));
    }

    #[test]
    fn deterministic_derivation() {
        let mnemonic: bip39::Mnemonic =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
                .parse().unwrap();

        let make_addr = || {
            let seed = mnemonic.to_seed("");
            let xprv = Xpriv::new_master(Network::Regtest, &seed[..]).unwrap();
            let w = Wallet::create(
                Bip86(xprv, KeychainKind::External),
                Bip86(xprv, KeychainKind::Internal),
            )
            .network(Network::Regtest)
            .create_wallet_no_persist()
            .unwrap();
            w.peek_address(KeychainKind::External, 0).address.to_string()
        };
        assert_eq!(make_addr(), make_addr());
    }

    #[test]
    fn different_mnemonics_give_different_addresses() {
        let addr_from = |ent: &[u8; 16]| {
            let m = bip39::Mnemonic::from_entropy(&ent[..]).unwrap();
            let seed = m.to_seed("");
            let xprv = Xpriv::new_master(Network::Regtest, &seed[..]).unwrap();
            let w = Wallet::create(
                Bip86(xprv, KeychainKind::External),
                Bip86(xprv, KeychainKind::Internal),
            )
            .network(Network::Regtest)
            .create_wallet_no_persist()
            .unwrap();
            w.peek_address(KeychainKind::External, 0).address.to_string()
        };
        assert_ne!(addr_from(&[1u8; 16]), addr_from(&[2u8; 16]));
    }

    // ── recreate_wallet ──

    #[test]
    fn recreate_wallet_from_state() {
        let state = WalletState {
            mnemonic: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".into(),
            wallet_name: "w".into(),
            descriptor: "".into(),
            change_descriptor: "".into(),
            address: "".into(),
            address_index: 0,
            network: "regtest".into(),
        };
        let wallet = recreate_wallet(&state).unwrap();
        let addr = wallet.peek_address(KeychainKind::External, 0);
        assert!(addr.address.to_string().starts_with("bcrt1p"));
    }

    #[test]
    fn recreate_wallet_advances_index() {
        let state = WalletState {
            mnemonic: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".into(),
            wallet_name: "w".into(),
            descriptor: "".into(),
            change_descriptor: "".into(),
            address: "".into(),
            address_index: 5,
            network: "regtest".into(),
        };
        let mut wallet = recreate_wallet(&state).unwrap();
        let next = wallet.reveal_next_address(KeychainKind::External);
        // After advancing to index 5, next should be index 6
        assert_eq!(next.index, 6);
    }

    #[test]
    fn recreate_wallet_bad_mnemonic() {
        let state = WalletState {
            mnemonic: "not a valid mnemonic".into(),
            wallet_name: "w".into(),
            descriptor: "".into(),
            change_descriptor: "".into(),
            address: "".into(),
            address_index: 0,
            network: "regtest".into(),
        };
        assert!(recreate_wallet(&state).is_err());
    }

    // ── handler bad-params errors ──

    #[test]
    fn handle_create_wallet_bad_params() {
        assert!(handle_create_wallet(json!({"garbage": true})).unwrap_err().contains("bad params"));
    }

    #[test]
    fn handle_get_balance_bad_params() {
        assert!(handle_get_balance(json!({})).unwrap_err().contains("bad params"));
    }

    #[test]
    fn handle_send_bad_params() {
        assert!(handle_send(json!({})).unwrap_err().contains("bad params"));
    }

    #[test]
    fn handle_faucet_bad_params() {
        assert!(handle_faucet(json!({})).unwrap_err().contains("bad params"));
    }

    #[test]
    fn handle_get_tx_bad_params() {
        assert!(handle_get_tx(json!({})).unwrap_err().contains("bad params"));
    }

    // ── Handlers that need state file ──

    #[test]
    fn get_balance_missing_state() {
        let r = handle_get_balance(json!({"state_path": "/no/file.json"}));
        assert!(r.unwrap_err().contains("read state"));
    }

    #[test]
    fn get_address_missing_state() {
        let r = handle_get_address(json!({"state_path": "/no/file.json"}));
        assert!(r.unwrap_err().contains("read state"));
    }

    #[test]
    fn send_missing_state() {
        let r = handle_send(json!({"state_path":"/no/file.json","to_address":"x","amount_sats":1}));
        assert!(r.unwrap_err().contains("read state"));
    }

    // ── create-wallet handler (no bitcoind needed) ──

    #[test]
    fn create_wallet_produces_p2tr_address() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        let result = handle_create_wallet(json!({
            "state_path": path.to_str().unwrap(),
            "name": "unit-test",
        }))
        .unwrap();

        assert!(result["address"].as_str().unwrap().starts_with("bcrt1p"));
        assert!(!result["mnemonic"].as_str().unwrap().is_empty());
        assert_eq!(result["wallet_name"], "unit-test");
        assert!(path.exists());
    }

    #[test]
    fn create_wallet_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("idem.json");
        let params = json!({"state_path": path.to_str().unwrap(), "name": "w"});

        let r1 = handle_create_wallet(params.clone()).unwrap();
        let r2 = handle_create_wallet(params).unwrap();

        assert_eq!(r1["address"], r2["address"]);
        assert_eq!(r2["existing"], true);
    }

    // ── get-address handler (no bitcoind needed) ──

    #[test]
    fn get_address_returns_address() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("addr.json");
        handle_create_wallet(json!({"state_path": path.to_str().unwrap(), "name": "w"})).unwrap();

        let r = handle_get_address(json!({"state_path": path.to_str().unwrap()})).unwrap();
        assert!(r["address"].as_str().unwrap().starts_with("bcrt1p"));
    }

    #[test]
    fn get_address_increments_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("idx.json");
        handle_create_wallet(json!({"state_path": path.to_str().unwrap(), "name": "w"})).unwrap();

        let a1 = handle_get_address(json!({"state_path": path.to_str().unwrap()})).unwrap();
        let a2 = handle_get_address(json!({"state_path": path.to_str().unwrap()})).unwrap();

        // Each call gives a new address
        assert_ne!(a1["address"], a2["address"]);

        // State file index was updated
        let state = load_state(path.to_str().unwrap()).unwrap();
        assert!(state.address_index >= 2);
    }

    // ── Descriptor template ──

    #[test]
    fn bip86_descriptor_builds() {
        let mnemonic: bip39::Mnemonic =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
                .parse().unwrap();
        let seed = mnemonic.to_seed("");
        let xprv = Xpriv::new_master(Network::Regtest, &seed[..]).unwrap();

        let (desc, key_map, _) = Bip86(xprv, KeychainKind::External)
            .build(Network::Regtest)
            .unwrap();
        let desc_str = desc.to_string_with_secret(&key_map);
        assert!(desc_str.starts_with("tr("));
    }

    // ── Integration tests (require running bitcoind) ──

    #[test]
    #[ignore]
    fn integration_create_and_get_balance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("int.json");
        handle_create_wallet(json!({
            "state_path": path.to_str().unwrap(),
            "name": "int-test",
        }))
        .unwrap();

        let balance = handle_get_balance(json!({
            "state_path": path.to_str().unwrap(),
        }))
        .unwrap();

        assert_eq!(balance["confirmed"], 0);
        assert_eq!(balance["total"], 0);
    }

    #[test]
    #[ignore]
    fn integration_faucet_and_balance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("faucet.json");
        let wallet = handle_create_wallet(json!({
            "state_path": path.to_str().unwrap(),
            "name": "faucet-test",
        }))
        .unwrap();

        let addr = wallet["address"].as_str().unwrap();
        handle_faucet(json!({"address": addr, "blocks": 101})).unwrap();

        let balance = handle_get_balance(json!({
            "state_path": path.to_str().unwrap(),
        }))
        .unwrap();

        assert!(balance["confirmed"].as_u64().unwrap() > 0);
    }
}
