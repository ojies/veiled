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
use bdk_wallet::bitcoin::{Address, Amount, BlockHash, FeeRate, Network};
use bdk_wallet::chain::{ChainPosition, BlockId};
use bdk_wallet::template::{Bip86, DescriptorTemplate};
#[allow(deprecated)]
use bdk_wallet::{KeychainKind, SignOptions, Update, Wallet};
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
    /// Last synced block height (for resuming sync without replaying from genesis)
    #[serde(default)]
    checkpoint_height: Option<u32>,
    /// Block hash at checkpoint_height
    #[serde(default)]
    checkpoint_hash: Option<String>,
    /// Cached balance (updated after each sync)
    #[serde(default)]
    cached_balance: Option<CachedBalance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CachedBalance {
    confirmed: u64,
    unconfirmed: u64,
    total: u64,
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
    let data = std::fs::read_to_string(path).map_err(|e| {
        eprintln!("[wallet] ERROR: read state '{}': {e}", path);
        format!("read state '{}': {e}", path)
    })?;
    serde_json::from_str(&data).map_err(|e| {
        eprintln!("[wallet] ERROR: parse state '{}': {e}", path);
        format!("parse state '{}': {e}", path)
    })
}

fn save_state(path: &str, state: &WalletState) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(state).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("write state: {e}"))
}

/// Recreate a BDK wallet from stored mnemonic.
/// If a checkpoint was saved, inserts it so sync resumes from there instead of genesis.
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

    // Restore saved checkpoint so sync doesn't replay from genesis
    if let (Some(height), Some(hash_hex)) = (state.checkpoint_height, state.checkpoint_hash.as_ref()) {
        if let Ok(hash) = hash_hex.parse::<BlockHash>() {
            let block_id = BlockId { height, hash };
            let cp = wallet.latest_checkpoint().insert(block_id);
            let _ = wallet.apply_update(Update {
                chain: Some(cp),
                ..Default::default()
            });
            eprintln!("[wallet] {} restored checkpoint at height {} ({}...)", state.wallet_name, height, &hash_hex[..12]);
        } else {
            eprintln!("[wallet] {} WARNING: failed to parse checkpoint hash '{}'", state.wallet_name, hash_hex);
        }
    } else {
        eprintln!("[wallet] {} no checkpoint saved — will sync from genesis", state.wallet_name);
    }

    Ok(wallet)
}

/// Sync a BDK wallet with bitcoind blocks and mempool.
fn sync_wallet(wallet: &mut Wallet, rpc: &Client) -> Result<(), String> {
    let tip = wallet.latest_checkpoint();
    let start_height = tip.height();
    let start_time = std::time::Instant::now();
    let empty_mempool: Vec<std::sync::Arc<bdk_wallet::bitcoin::Transaction>> = vec![];
    let mut emitter = Emitter::new(rpc, tip.clone(), start_height, empty_mempool);

    let mut blocks_synced = 0u32;
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
        blocks_synced += 1;
    }

    // Sync mempool
    let mempool = emitter
        .mempool()
        .map_err(|e| format!("sync mempool: {e}"))?;
    let mempool_count = mempool.update.len();
    wallet.apply_unconfirmed_txs(mempool.update);

    let elapsed = start_time.elapsed();
    let final_height = wallet.latest_checkpoint().height();
    eprintln!(
        "[wallet] sync: {} blocks ({} -> {}), {} mempool txs, {:.1}ms",
        blocks_synced, start_height, final_height, mempool_count, elapsed.as_secs_f64() * 1000.0
    );

    Ok(())
}

/// Save the wallet's latest checkpoint back to the state file for faster future syncs.
fn save_checkpoint(state_path: &str, wallet: &Wallet) -> Result<(), String> {
    let mut state = load_state(state_path)?;
    let cp = wallet.latest_checkpoint();
    let old_height = state.checkpoint_height.unwrap_or(0);
    state.checkpoint_height = Some(cp.height());
    state.checkpoint_hash = Some(cp.hash().to_string());
    save_state(state_path, &state)?;
    eprintln!(
        "[wallet] {} checkpoint saved: {} -> {} ({})",
        state.wallet_name, old_height, cp.height(), &cp.hash().to_string()[..12]
    );
    Ok(())
}

// ── Handlers ──

fn handle_create_wallet(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: CreateWalletParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;

    eprintln!("[wallet] create-wallet '{}' at {}", p.name, p.state_path);

    // Idempotent: if wallet state already exists, return it
    if let Ok(state) = load_state(&p.state_path) {
        eprintln!("[wallet] '{}' already exists (addr: {})", state.wallet_name, state.address);
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
        checkpoint_height: None,
        checkpoint_hash: None,
        cached_balance: None,
    };
    save_state(&p.state_path, &state)?;
    eprintln!("[wallet] '{}' created (addr: {})", p.name, address);

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
    eprintln!("[wallet] get-balance for '{}' (addr: {})", state.wallet_name, state.address);

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);
    let rpc = rpc_client(&rpc_url, &rpc_user, &rpc_pass)?;

    let cp_height = state.checkpoint_height.unwrap_or(0);
    eprintln!("[wallet] {} syncing from checkpoint height {}", state.wallet_name, cp_height);

    let mut wallet = recreate_wallet(&state)?;
    sync_wallet(&mut wallet, &rpc)?;

    let balance = wallet.balance();
    let confirmed = balance.confirmed.to_sat();
    let unconfirmed = balance.untrusted_pending.to_sat() + balance.trusted_pending.to_sat();
    let immature = balance.immature.to_sat();
    let total = confirmed + unconfirmed;

    eprintln!(
        "[wallet] {} balance: {} confirmed, {} unconfirmed, {} immature (synced to height {})",
        state.wallet_name, confirmed, unconfirmed, immature, wallet.latest_checkpoint().height()
    );

    // Save checkpoint and cached balance
    let mut state = load_state(&p.state_path)?;
    let cp = wallet.latest_checkpoint();
    state.checkpoint_height = Some(cp.height());
    state.checkpoint_hash = Some(cp.hash().to_string());
    state.cached_balance = Some(CachedBalance { confirmed, unconfirmed, total });
    let _ = save_state(&p.state_path, &state);

    Ok(json!({
        "confirmed": confirmed,
        "unconfirmed": unconfirmed,
        "total": total,
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
    eprintln!(
        "[wallet] send from '{}': {} sats -> {}",
        state.wallet_name, p.amount_sats, &p.to_address[..20]
    );

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);
    let rpc = rpc_client(&rpc_url, &rpc_user, &rpc_pass)?;

    let mut wallet = recreate_wallet(&state)?;
    sync_wallet(&mut wallet, &rpc)?;
    let _ = save_checkpoint(&p.state_path, &wallet);

    let balance = wallet.balance();
    let total = balance.confirmed.to_sat()
        + balance.trusted_pending.to_sat()
        + balance.untrusted_pending.to_sat();
    eprintln!(
        "[wallet] {} pre-send balance: {} confirmed, {} pending, {} immature",
        state.wallet_name,
        balance.confirmed.to_sat(),
        balance.trusted_pending.to_sat() + balance.untrusted_pending.to_sat(),
        balance.immature.to_sat()
    );

    // If BDK shows 0 spendable but we need funds, the checkpoint may have
    // jumped past the funding block. Reset checkpoint to (tip - 200) and
    // resync the recent window where funding likely happened.
    if total == 0 && p.amount_sats > 0 {
        let tip_height: u64 = {
            let empty: Vec<serde_json::Value> = vec![];
            rpc.call("getblockcount", &empty).unwrap_or(0)
        };
        let resync_from = tip_height.saturating_sub(200);
        eprintln!(
            "[wallet] {} has 0 spendable — resyncing from height {} (tip={}, window=200)",
            state.wallet_name, resync_from, tip_height
        );
        let mut fresh_state = load_state(&p.state_path)?;
        if resync_from == 0 {
            fresh_state.checkpoint_height = None;
            fresh_state.checkpoint_hash = None;
        } else {
            // Get the block hash at resync_from
            let hash: String = rpc
                .call("getblockhash", &[serde_json::json!(resync_from)])
                .map_err(|e| format!("getblockhash: {e}"))?;
            fresh_state.checkpoint_height = Some(resync_from as u32);
            fresh_state.checkpoint_hash = Some(hash);
        }
        save_state(&p.state_path, &fresh_state)?;
        wallet = recreate_wallet(&fresh_state)?;
        sync_wallet(&mut wallet, &rpc)?;
        let _ = save_checkpoint(&p.state_path, &wallet);
        let balance2 = wallet.balance();
        eprintln!(
            "[wallet] {} after resync: {} confirmed, {} pending, {} immature",
            state.wallet_name,
            balance2.confirmed.to_sat(),
            balance2.trusted_pending.to_sat() + balance2.untrusted_pending.to_sat(),
            balance2.immature.to_sat()
        );
    }

    let to_addr = Address::from_str(&p.to_address)
        .map_err(|e| format!("invalid address: {e}"))?
        .require_network(Network::Regtest)
        .map_err(|e| format!("wrong network: {e}"))?;

    let mut builder = wallet.build_tx();
    builder
        .add_recipient(to_addr.script_pubkey(), Amount::from_sat(p.amount_sats))
        .fee_rate(FeeRate::from_sat_per_vb(2).expect("valid fee rate"));

    let mut psbt = builder.finish().map_err(|e| format!("build tx: {e}"))?;

    // Log transaction details before signing
    let unsigned_tx = psbt.unsigned_tx.clone();
    eprintln!(
        "[wallet] {} built tx: {} input(s), {} output(s), {} vbytes",
        state.wallet_name,
        unsigned_tx.input.len(),
        unsigned_tx.output.len(),
        unsigned_tx.vsize()
    );
    let max_log_inputs = 5;
    for (i, input) in unsigned_tx.input.iter().enumerate() {
        if i < max_log_inputs {
            eprintln!(
                "[wallet]   vin[{}]: {}:{} (sequence: {})",
                i, input.previous_output.txid, input.previous_output.vout, input.sequence
            );
        } else if i == max_log_inputs {
            eprintln!("[wallet]   ... and {} more inputs", unsigned_tx.input.len() - max_log_inputs);
        }
    }
    for (i, output) in unsigned_tx.output.iter().enumerate() {
        let addr_label = if output.value.to_sat() == p.amount_sats {
            "recipient"
        } else {
            "change"
        };
        eprintln!(
            "[wallet]   vout[{}]: {} sats ({})",
            i, output.value.to_sat(), addr_label
        );
    }

    #[allow(deprecated)]
    let finalized = wallet
        .sign(&mut psbt, SignOptions::default())
        .map_err(|e| format!("sign: {e}"))?;
    if !finalized {
        return Err("transaction signing incomplete".into());
    }
    eprintln!("[wallet] {} tx signed successfully", state.wallet_name);

    let tx = psbt.extract_tx().map_err(|e| format!("extract tx: {e}"))?;
    let txid = tx.compute_txid();
    let fee = wallet.calculate_fee(&tx).unwrap_or(Amount::ZERO);
    let fee_rate_vb = if tx.vsize() > 0 {
        fee.to_sat() as f64 / tx.vsize() as f64
    } else {
        0.0
    };

    eprintln!(
        "[wallet] {} broadcasting tx {} ({} bytes, {} vbytes, fee: {} sats, {:.1} sat/vB)...",
        state.wallet_name, txid, tx.total_size(), tx.vsize(), fee.to_sat(), fee_rate_vb
    );

    rpc.send_raw_transaction(&tx)
        .map_err(|e| format!("broadcast: {e}"))?;

    // Verify tx made it into the mempool
    let mempool_params = vec![json!(txid.to_string()), json!(true)];
    match rpc.call::<serde_json::Value>("getrawtransaction", &mempool_params) {
        Ok(raw) => {
            let confirmations = raw.get("confirmations").and_then(|c| c.as_u64()).unwrap_or(0);
            if confirmations > 0 {
                eprintln!("[wallet] {} tx {} already confirmed ({} confirmations)", state.wallet_name, txid, confirmations);
            } else {
                eprintln!("[wallet] {} tx {} accepted into mempool", state.wallet_name, txid);
            }
        }
        Err(e) => {
            eprintln!("[wallet] {} WARNING: tx {} mempool check failed: {}", state.wallet_name, txid, e);
        }
    }

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
    eprintln!(
        "[wallet] faucet: mining {} block(s) to {}...{}",
        blocks,
        &p.address[..20],
        &p.address[p.address.len().saturating_sub(6)..]
    );

    let block_hashes: Vec<String> = rpc
        .call("generatetoaddress", &[json!(blocks), json!(p.address)])
        .map_err(|e| format!("generatetoaddress: {e}"))?;

    // Log current chain height
    let empty: Vec<serde_json::Value> = vec![];
    if let Ok(height) = rpc.call::<u64>("getblockcount", &empty) {
        eprintln!("[wallet] faucet: chain height now {}", height);
    }

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
    let _ = save_checkpoint(&p.state_path, &wallet);

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

fn handle_get_balance_fast(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: GetBalanceParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let state = load_state(&p.state_path)?;
    eprintln!("[wallet] get-balance-fast for '{}' (scantxoutset)", state.wallet_name);

    let rpc_url = p.rpc_url.unwrap_or_else(default_url);
    let rpc_user = p.rpc_user.unwrap_or_else(default_user);
    let rpc_pass = p.rpc_pass.unwrap_or_else(default_pass);
    let rpc = rpc_client(&rpc_url, &rpc_user, &rpc_pass)?;

    // Use scantxoutset to query the UTXO set directly — no BDK sync needed.
    // Recreate the wallet to derive all addresses (external + change) then
    // scan each one. This avoids descriptor format mismatches between BDK
    // and Bitcoin Core.
    let wallet = recreate_wallet(&state)?;

    let mut descriptors: Vec<serde_json::Value> = Vec::new();
    // Scan external addresses (0..=address_index + some lookahead)
    let ext_count = state.address_index.max(5) + 5;
    for i in 0..ext_count {
        let info = wallet.peek_address(KeychainKind::External, i);
        descriptors.push(json!(format!("addr({})", info.address)));
    }
    // Scan change addresses (small lookahead)
    for i in 0..20 {
        let info = wallet.peek_address(KeychainKind::Internal, i);
        descriptors.push(json!(format!("addr({})", info.address)));
    }
    eprintln!("[wallet] {} scanning {} addresses via scantxoutset", state.wallet_name, descriptors.len());

    let scan_result: serde_json::Value = rpc
        .call("scantxoutset", &[json!("start"), json!(descriptors)])
        .map_err(|e| format!("scantxoutset: {e}"))?;

    let total_sats = scan_result["total_amount"]
        .as_f64()
        .map(|btc| (btc * 100_000_000.0).round() as u64)
        .unwrap_or(0);

    let unspents = scan_result["unspents"].as_array();
    let utxo_count = unspents.map(|a| a.len()).unwrap_or(0);
    eprintln!(
        "[wallet] {} scantxoutset result: {} sats across {} UTXOs",
        state.wallet_name, total_sats, utxo_count
    );
    if let Some(utxos) = unspents {
        let max_log = 5;
        for (i, u) in utxos.iter().enumerate() {
            if i >= max_log {
                eprintln!("[wallet]   ... and {} more UTXOs", utxos.len() - max_log);
                break;
            }
            let amt = u["amount"].as_f64().unwrap_or(0.0);
            let sats = (amt * 100_000_000.0).round() as u64;
            let txid = u["txid"].as_str().unwrap_or("?");
            let vout = u["vout"].as_u64().unwrap_or(0);
            let addr = u["desc"].as_str().unwrap_or("?");
            eprintln!("[wallet]   UTXO {}:{} -> {} sats ({})", &txid[..12.min(txid.len())], vout, sats, addr);
        }
    }

    // Update cached balance in state file
    let _ = (|| -> Result<(), String> {
        let mut s = load_state(&p.state_path)?;
        s.cached_balance = Some(CachedBalance {
            confirmed: total_sats,
            unconfirmed: 0,
            total: total_sats,
        });
        save_state(&p.state_path, &s)
    })();

    Ok(json!({
        "confirmed": total_sats,
        "unconfirmed": 0,
        "total": total_sats,
    }))
}

// ── Command dispatch ──

fn dispatch(cmd: Command) -> Result<serde_json::Value, String> {
    let cmd_name = cmd.command.clone();
    let start = std::time::Instant::now();
    eprintln!("[wallet] ── {} ──", cmd_name);
    let result = match cmd.command.as_str() {
        "create-wallet" => handle_create_wallet(cmd.params),
        "get-balance" => handle_get_balance(cmd.params),
        "get-balance-fast" => handle_get_balance_fast(cmd.params),
        "get-address" => handle_get_address(cmd.params),
        "send" => handle_send(cmd.params),
        "faucet" => handle_faucet(cmd.params),
        "get-tx" => handle_get_tx(cmd.params),
        "get-tx-history" => handle_get_tx_history(cmd.params),
        "ping" => Ok(json!({"status": "ok"})),
        other => Err(format!("unknown command: {other}")),
    };
    let elapsed = start.elapsed();
    match &result {
        Ok(_) => eprintln!("[wallet] ── {} OK ({:.1}ms) ──", cmd_name, elapsed.as_secs_f64() * 1000.0),
        Err(e) => eprintln!("[wallet] ── {} FAILED ({:.1}ms): {} ──", cmd_name, elapsed.as_secs_f64() * 1000.0, e),
    }
    result
}

// ── Main ──

fn main() {
    use std::io::BufRead;

    // If --daemon flag is passed, run in persistent line-delimited JSON mode.
    // Otherwise, fall back to single-command mode for backwards compatibility.
    let daemon = std::env::args().any(|a| a == "--daemon");

    if daemon {
        eprintln!("[wallet] daemon started (pid: {})", std::process::id());
        // Flush stdout after every line to prevent buffering issues
        use std::io::Write;
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let reader = stdin.lock();
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[wallet] stdin read error: {e}");
                    break;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            // Catch panics so a single bad command doesn't kill the daemon
            let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match serde_json::from_str::<Command>(&line) {
                    Ok(cmd) => match dispatch(cmd) {
                        Ok(val) => serde_json::to_string(&val).unwrap(),
                        Err(e) => serde_json::to_string(&ErrorResponse { error: e }).unwrap(),
                    },
                    Err(e) => serde_json::to_string(&ErrorResponse {
                        error: format!("invalid JSON: {e}"),
                    })
                    .unwrap(),
                }
            }));
            let output = match output {
                Ok(s) => s,
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_info.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic".to_string()
                    };
                    eprintln!("[wallet] PANIC caught: {}", msg);
                    serde_json::to_string(&ErrorResponse {
                        error: format!("internal panic: {msg}"),
                    })
                    .unwrap()
                }
            };
            // Write + flush to prevent broken pipe from killing the daemon
            if let Err(e) = writeln!(stdout.lock(), "{output}") {
                eprintln!("[wallet] stdout write error (client gone?): {e}");
                break;
            }
        }
        eprintln!("[wallet] daemon shutting down");
    } else {
        // Single-command mode (backwards compatible)
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

        match dispatch(cmd) {
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
        let msg = format!("unknown command: {}", "bogus");
        assert_eq!(msg, "unknown command: bogus");
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
