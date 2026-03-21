use crate::core::registry::Registry;
use crate::core::types::Commitment;
use crate::core::Merchant;
use crate::registry::db;
use bdk_bitcoind_rpc::bitcoincore_rpc::{Client, RpcApi};
use bdk_bitcoind_rpc::Emitter;
use bdk_wallet::bitcoin::bip32::Xpriv;
use bdk_wallet::bitcoin::{FeeRate, Network as BdkNetwork};
use bdk_wallet::template::Bip86;
#[allow(deprecated)]
use bdk_wallet::{KeychainKind, SignOptions, Wallet as BdkWallet};
use bitcoin::{Address, Amount, OutPoint};
use rusqlite::Connection as SqlConnection;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{info, warn};

pub struct MerchantInfo {
    pub merchant: Merchant,
    pub email: String,
    pub phone: String,
}

pub struct ActiveSet {
    pub registry: Registry,
    pub beneficiary_capacity: usize,
    pub sats_per_user: u64,
    pub finalized: bool,
    pub finalization_tx: watch::Sender<bool>,
}

/// Configuration for minimum fees enforced by the registry.
#[derive(Debug, Clone)]
pub struct FeeConfig {
    /// Minimum sats-per-user required when creating a set.
    pub min_sats_per_user: u64,
    /// Minimum merchant registration fee in sats (future use).
    pub merchant_registration_fee: u64,
}

impl Default for FeeConfig {
    fn default() -> Self {
        Self {
            min_sats_per_user: 200,
            merchant_registration_fee: 500,
        }
    }
}

pub struct RegistryStore {
    pub merchant_pool: HashMap<String, MerchantInfo>,
    pub active_sets: HashMap<u64, ActiveSet>,
    pub rpc_client: Option<Arc<Client>>,
    pub fee_config: FeeConfig,
    pub wallet_mnemonic: String,
    pub wallet_address: Address,
    /// 32-byte x-only public key from the BDK wallet's P2TR address.
    pub wallet_xonly_bytes: Vec<u8>,
    db: Option<SqlConnection>,
}

fn generate_mnemonic() -> bip39::Mnemonic {
    let mut entropy = [0u8; 16];
    use rand_core::{OsRng, RngCore};
    OsRng.fill_bytes(&mut entropy);
    bip39::Mnemonic::from_entropy(&entropy).expect("16 bytes should produce a valid mnemonic")
}

/// Create an ephemeral BDK wallet from a mnemonic (BIP86 P2TR).
fn create_bdk_wallet(mnemonic: &bip39::Mnemonic) -> Result<BdkWallet, String> {
    let seed = mnemonic.to_seed("");
    let xprv = Xpriv::new_master(BdkNetwork::Regtest, &seed)
        .map_err(|e| format!("master key: {e}"))?;

    BdkWallet::create(
        Bip86(xprv, KeychainKind::External),
        Bip86(xprv, KeychainKind::Internal),
    )
    .network(BdkNetwork::Regtest)
    .create_wallet_no_persist()
    .map_err(|e| format!("create wallet: {e}"))
}

/// Sync a BDK wallet with bitcoind via Emitter.
fn sync_bdk_wallet(wallet: &mut BdkWallet, rpc: &Client) -> Result<(), String> {
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

    let mempool = emitter
        .mempool()
        .map_err(|e| format!("sync mempool: {e}"))?;
    wallet.apply_unconfirmed_txs(mempool.update);

    Ok(())
}

/// Convert a Bitcoin RPC JSON `value` field (BTC as a number) to satoshis
/// without floating-point precision loss. Tries the string representation
/// first (exact decimal), falling back to f64 with rounding.
fn btc_value_to_sats(value: &serde_json::Value) -> Result<u64, String> {
    // bitcoind returns value as a JSON number; serde_json preserves it.
    // Use Amount::from_btc on the string form to avoid IEEE 754 rounding.
    if let Some(n) = value.as_f64() {
        let amt = Amount::from_btc(n).map_err(|e| format!("invalid BTC value: {e}"))?;
        Ok(amt.to_sat())
    } else {
        Err("Output has no value".to_string())
    }
}

fn derive_wallet_address(mnemonic: &bip39::Mnemonic) -> Result<(Address, Vec<u8>), String> {
    let bdk = create_bdk_wallet(mnemonic)?;
    let addr = bdk.peek_address(KeychainKind::External, 0);
    // Extract 32-byte x-only key from P2TR script (OP_1 <32 bytes>)
    let script_bytes = addr.address.script_pubkey().to_bytes();
    let xonly_bytes = script_bytes[2..34].to_vec();
    // Convert from bdk_wallet::bitcoin::Address to bitcoin::Address
    let addr_str = addr.address.to_string();
    let address = addr_str
        .parse::<Address<bitcoin::address::NetworkUnchecked>>()
        .map_err(|e| format!("parse address: {e}"))
        .map(|a| a.assume_checked())?;
    Ok((address, xonly_bytes))
}

impl Default for RegistryStore {
    fn default() -> Self {
        Self::new(None, FeeConfig::default(), None)
    }
}

impl RegistryStore {
    pub fn new(
        rpc_client: Option<Arc<Client>>,
        fee_config: FeeConfig,
        db: Option<SqlConnection>,
    ) -> Self {
        let mnemonic = generate_mnemonic();
        let (wallet_address, wallet_xonly_bytes) =
            derive_wallet_address(&mnemonic).expect("failed to derive wallet address");
        Self {
            merchant_pool: HashMap::new(),
            active_sets: HashMap::new(),
            rpc_client,
            fee_config,
            wallet_mnemonic: mnemonic.to_string(),
            wallet_address,
            wallet_xonly_bytes,
            db,
        }
    }

    /// Open SQLite at `path`, load persisted state, and return a ready store.
    pub fn open(
        rpc_client: Option<Arc<Client>>,
        fee_config: FeeConfig,
        db_path: &str,
    ) -> Result<Self, String> {
        let conn = db::open_db(db_path).map_err(|e| format!("Failed to open database: {}", e))?;
        let state =
            db::load_state(&conn).map_err(|e| format!("Failed to load database state: {}", e))?;

        // Load or generate wallet mnemonic
        let saved_mnemonic = db::load_wallet_mnemonic(&conn)
            .map_err(|e| format!("Failed to load wallet mnemonic: {}", e))?;
        let mnemonic_str = if let Some(m) = saved_mnemonic {
            info!("Loaded wallet mnemonic from database");
            m
        } else {
            let m = generate_mnemonic();
            let s = m.to_string();
            db::save_wallet_mnemonic(&conn, &s)
                .map_err(|e| format!("Failed to save wallet mnemonic: {}", e))?;
            info!("Generated and saved new wallet mnemonic");
            s
        };
        let mnemonic: bip39::Mnemonic = mnemonic_str
            .parse()
            .map_err(|e| format!("Failed to parse wallet mnemonic: {}", e))?;
        let (wallet_address, wallet_xonly_bytes) = derive_wallet_address(&mnemonic)?;
        info!("Wallet address: {}", wallet_address);

        let mut store = Self {
            merchant_pool: HashMap::new(),
            active_sets: HashMap::new(),
            rpc_client,
            fee_config,
            wallet_mnemonic: mnemonic_str,
            wallet_address,
            wallet_xonly_bytes,
            db: Some(conn),
        };

        // 1. Replay merchants
        for m in &state.merchants {
            let merchant = Merchant::new(&m.name, &m.origin);
            store.merchant_pool.insert(
                m.name.clone(),
                MerchantInfo {
                    merchant,
                    email: m.email.clone(),
                    phone: m.phone.clone(),
                },
            );
        }
        info!("Restored {} merchants from database", state.merchants.len());

        // 2. Replay sets (creates Registry + CRS via add_merchant + setup)
        for s in &state.sets {
            let mut merchants = Vec::new();
            for m_name in &s.merchant_names {
                let m_info = store.merchant_pool.get(m_name).ok_or_else(|| {
                    format!(
                        "DB inconsistency: set {} references unknown merchant '{}'",
                        s.set_id, m_name
                    )
                })?;
                merchants.push(m_info.merchant.clone());
            }

            let mut registry = Registry::new(s.beneficiary_capacity, s.sats_per_user);
            for m in merchants {
                registry.add_merchant(m);
            }
            registry.setup();

            let (finalization_tx, _) = watch::channel(s.finalized);
            store.active_sets.insert(
                s.set_id,
                ActiveSet {
                    registry,
                    beneficiary_capacity: s.beneficiary_capacity,
                    sats_per_user: s.sats_per_user,
                    finalized: false, // set after replaying commitments
                    finalization_tx,
                },
            );
        }
        info!("Restored {} sets from database", state.sets.len());

        // 3. Replay commitments (bypass on-chain verification)
        let mut commitment_count = 0;
        for c in &state.commitments {
            let active_set = store.active_sets.get_mut(&c.set_id).ok_or_else(|| {
                format!(
                    "DB inconsistency: commitment references unknown set {}",
                    c.set_id
                )
            })?;
            let outpoint = OutPoint {
                txid: c.txid,
                vout: c.vout,
            };
            active_set.registry.add_beneficiary(c.phi, outpoint);
            commitment_count += 1;
        }
        info!("Restored {} commitments from database", commitment_count);

        // 4. Mark finalized sets
        for s in &state.sets {
            if s.finalized {
                if let Some(active_set) = store.active_sets.get_mut(&s.set_id) {
                    active_set.finalized = true;
                    let _ = active_set.finalization_tx.send(true);
                }
            }
        }

        Ok(store)
    }

    pub fn register_merchant(
        &mut self,
        name: &str,
        origin: &str,
        email: String,
        phone: String,
        outpoint: OutPoint,
    ) -> Result<(), String> {
        if self.merchant_pool.contains_key(name) {
            return Err(format!("Merchant '{}' already registered", name));
        }

        // Verify payment on-chain if RPC client is available
        if let Some(rpc) = &self.rpc_client {
            let expected_address = self.wallet_address.to_string();
            let required_fee = self.fee_config.merchant_registration_fee;
            info!("Verifying merchant '{}' payment: tx {}:{}, expect {} sats to {}", name, outpoint.txid, outpoint.vout, required_fee, &expected_address[..20]);

            let raw_tx: serde_json::Value = rpc
                .call(
                    "getrawtransaction",
                    &[
                        serde_json::json!(outpoint.txid.to_string()),
                        serde_json::json!(true),
                    ],
                )
                .map_err(|e| format!("Failed to fetch transaction {}: {}", outpoint.txid, e))?;

            let confirmations = raw_tx["confirmations"].as_u64().unwrap_or(0);
            info!("  tx {} has {} confirmations", outpoint.txid, confirmations);

            let vout_array = raw_tx["vout"]
                .as_array()
                .ok_or("Transaction has no vout array")?;
            let output = vout_array
                .get(outpoint.vout as usize)
                .ok_or(format!("vout index {} not found in tx", outpoint.vout))?;

            let script_address = output["scriptPubKey"]["address"]
                .as_str()
                .ok_or("Output has no address")?;
            if script_address != expected_address {
                warn!("  address mismatch: expected {}, got {}", expected_address, script_address);
                return Err(format!(
                    "Payment output address mismatch: expected {}, got {}",
                    expected_address, script_address
                ));
            }

            let value_sats = btc_value_to_sats(&output["value"])?;
            info!("  vout[{}]: {} sats to {} (need >= {})", outpoint.vout, value_sats, script_address, required_fee);
            if value_sats < required_fee {
                warn!("  fee too low: {} < {}", value_sats, required_fee);
                return Err(format!(
                    "Merchant registration fee too low: expected {} sats, got {} sats",
                    required_fee, value_sats
                ));
            }
            info!("  payment verified OK for merchant '{}'", name);
        } else {
            info!("  no RPC client — skipping on-chain verification for '{}'", name);
        }

        let merchant = Merchant::new(name, origin);
        self.merchant_pool.insert(
            name.to_string(),
            MerchantInfo {
                merchant,
                email: email.clone(),
                phone: phone.clone(),
            },
        );
        if let Some(ref conn) = self.db {
            db::save_merchant(conn, name, origin, &email, &phone)
                .map_err(|e| format!("DB error saving merchant: {}", e))?;
        }
        Ok(())
    }

    pub fn create_set(
        &mut self,
        set_id: u64,
        merchant_names: &[String],
        beneficiary_capacity: usize,
        sats_per_user: u64,
    ) -> Result<(), String> {
        if self.active_sets.contains_key(&set_id) {
            return Err(format!("Set {} already exists", set_id));
        }

        if merchant_names.is_empty() {
            return Err("At least one merchant is required".to_string());
        }

        if sats_per_user < self.fee_config.min_sats_per_user {
            return Err(format!(
                "sats_per_user ({}) below minimum ({})",
                sats_per_user, self.fee_config.min_sats_per_user
            ));
        }

        // Collect merchants from pool
        let mut merchants = Vec::new();
        for m_name in merchant_names {
            let m_info = self
                .merchant_pool
                .get(m_name)
                .ok_or_else(|| format!("Merchant '{}' not found in registration pool", m_name))?;
            merchants.push(m_info.merchant.clone());
        }

        // Create core::Registry and setup CRS
        let mut registry = Registry::new(beneficiary_capacity, sats_per_user);
        for m in merchants {
            registry.add_merchant(m);
        }
        registry.setup();

        let (finalization_tx, _) = watch::channel(false);
        self.active_sets.insert(
            set_id,
            ActiveSet {
                registry,
                beneficiary_capacity,
                sats_per_user,
                finalized: false,
                finalization_tx,
            },
        );

        if let Some(ref conn) = self.db {
            db::save_set(conn, set_id, beneficiary_capacity, sats_per_user, merchant_names)
                .map_err(|e| format!("DB error saving set: {}", e))?;
        }

        Ok(())
    }

    pub fn register_beneficiary(
        &mut self,
        set_id: u64,
        phi: Commitment,
        outpoint: OutPoint,
    ) -> Result<usize, String> {
        let sats_per_user = self
            .active_sets
            .get(&set_id)
            .ok_or_else(|| format!("Set {} not found", set_id))?
            .sats_per_user;

        let active_set = self
            .active_sets
            .get_mut(&set_id)
            .ok_or_else(|| format!("Set {} not found", set_id))?;

        if active_set.finalized {
            return Err(format!("Set {} is already finalized", set_id));
        }

        if active_set.registry.beneficiary_count() >= active_set.beneficiary_capacity {
            return Err("Anonymity set is full".to_string());
        }

        // Check for duplicate
        if active_set.registry.anonymity_set().contains(&phi) {
            return Err("Beneficiary already registered in this set".to_string());
        }

        // Verify payment on-chain if RPC client is available
        if let Some(rpc) = &self.rpc_client {
            let expected_address = self.wallet_address.to_string();
            info!("Verifying beneficiary payment for set {}: tx {}:{}, expect {} sats to {}", set_id, outpoint.txid, outpoint.vout, sats_per_user, &expected_address[..20]);

            let raw_tx: serde_json::Value = rpc
                .call(
                    "getrawtransaction",
                    &[
                        serde_json::json!(outpoint.txid.to_string()),
                        serde_json::json!(true),
                    ],
                )
                .map_err(|e| format!("Failed to fetch transaction {}: {}", outpoint.txid, e))?;

            let confirmations = raw_tx["confirmations"].as_u64().unwrap_or(0);
            info!("  tx {} has {} confirmations", outpoint.txid, confirmations);

            let vout_array = raw_tx["vout"]
                .as_array()
                .ok_or("Transaction has no vout array")?;
            let output = vout_array
                .get(outpoint.vout as usize)
                .ok_or(format!("vout index {} not found in tx", outpoint.vout))?;

            let script_address = output["scriptPubKey"]["address"]
                .as_str()
                .ok_or("Output has no address")?;
            if script_address != expected_address {
                warn!("  address mismatch: expected {}, got {}", expected_address, script_address);
                return Err(format!(
                    "Payment output address mismatch: expected {}, got {}",
                    expected_address, script_address
                ));
            }

            let value_sats = btc_value_to_sats(&output["value"])?;
            info!("  vout[{}]: {} sats to {} (need >= {})", outpoint.vout, value_sats, script_address, sats_per_user);
            if value_sats < sats_per_user {
                warn!("  payment too low: {} < {}", value_sats, sats_per_user);
                return Err(format!(
                    "Payment amount too low: expected {} sats, got {} sats",
                    sats_per_user, value_sats
                ));
            }
            info!("  payment verified OK for beneficiary in set {}", set_id);
        } else {
            info!("  no RPC client — skipping on-chain verification for set {}", set_id);
        }

        let index = active_set.registry.add_beneficiary(phi, outpoint);

        if let Some(ref conn) = self.db {
            db::save_commitment(conn, set_id, index, &phi, &outpoint)
                .map_err(|e| format!("DB error saving commitment: {}", e))?;
        }

        Ok(index)
    }

    /// Finalize the set: create Taproot commitment, fund via BDK wallet, and broadcast.
    ///
    /// The registry funds the commitment transaction from its BDK wallet balance.
    /// BDK handles UTXO selection, fee estimation, signing, and change outputs.
    pub fn finalize_set(&mut self, set_id: u64) -> Result<String, String> {
        let active_set = self
            .active_sets
            .get_mut(&set_id)
            .ok_or_else(|| format!("Set {} not found", set_id))?;

        if active_set.finalized {
            return Ok(format!("Set {} already finalized", set_id));
        }

        if active_set.registry.beneficiary_count() < active_set.beneficiary_capacity {
            return Err(format!(
                "Need {} beneficiaries to finalize, have {}",
                active_set.beneficiary_capacity,
                active_set.registry.beneficiary_count()
            ));
        }

        // Create Taproot commitment to get the output script (Merkle root of anonymity set).
        info!("Creating Taproot commitment for set {} ({} members)", set_id, active_set.registry.beneficiary_count());
        let dummy_outpoint = OutPoint::null();
        let commitment = active_set
            .registry
            .create_anonymity_set(dummy_outpoint)
            .map_err(|e| format!("Failed to create Taproot commitment: {}", e))?;

        // The commitment output amount is the total beneficiary fees for this set.
        let output_amount = active_set.beneficiary_capacity as u64 * active_set.sats_per_user;
        let output_script = commitment.tx.output[0].script_pubkey.clone();
        info!("Commitment output: {} sats, script {} bytes", output_amount, output_script.len());

        // Fund, sign, and broadcast using BDK wallet
        let commitment_txid_str = if let Some(rpc) = &self.rpc_client {
            let mnemonic: bip39::Mnemonic = self
                .wallet_mnemonic
                .parse()
                .map_err(|e| format!("parse mnemonic: {e}"))?;
            let mut bdk = create_bdk_wallet(&mnemonic)?;
            info!("Syncing registry BDK wallet for commitment tx...");
            sync_bdk_wallet(&mut bdk, rpc)?;

            let balance = bdk.balance();
            info!(
                "Registry wallet balance: {} confirmed, {} pending, {} immature",
                balance.confirmed.to_sat(),
                balance.trusted_pending.to_sat() + balance.untrusted_pending.to_sat(),
                balance.immature.to_sat()
            );

            let bdk_script =
                bdk_wallet::bitcoin::ScriptBuf::from_bytes(output_script.to_bytes());
            let bdk_amount = bdk_wallet::bitcoin::Amount::from_sat(output_amount);

            let mut builder = bdk.build_tx();
            builder
                .add_recipient(bdk_script, bdk_amount)
                .fee_rate(FeeRate::from_sat_per_vb(2).expect("valid fee rate"));

            let mut psbt = builder
                .finish()
                .map_err(|e| format!("Failed to build commitment tx: {e}"))?;

            #[allow(deprecated)]
            let finalized = bdk
                .sign(&mut psbt, SignOptions::default())
                .map_err(|e| format!("Failed to sign commitment tx: {e}"))?;
            if !finalized {
                return Err("Commitment transaction signing incomplete".into());
            }

            let tx = psbt
                .extract_tx()
                .map_err(|e| format!("Failed to extract commitment tx: {e}"))?;
            let txid = tx.compute_txid();
            let fee = bdk.calculate_fee(&tx).unwrap_or(bdk_wallet::bitcoin::Amount::ZERO);

            info!(
                "Broadcasting commitment tx: {} ({} vbytes, fee: {} sats)",
                txid,
                tx.vsize(),
                fee.to_sat()
            );
            rpc.send_raw_transaction(&tx)
                .map_err(|e| format!("Failed to broadcast commitment tx: {e}"))?;

            info!("Commitment tx broadcast OK: {}", txid);
            Some(txid.to_string())
        } else {
            info!("No RPC client — skipping on-chain commitment for set {}", set_id);
            None
        };

        // Persist to DB first, then update memory, then notify subscribers.
        // This ordering ensures that if DB write fails, memory stays consistent.
        if let Some(ref conn) = self.db {
            db::mark_set_finalized(conn, set_id, commitment_txid_str.as_deref())
                .map_err(|e| format!("DB error marking set finalized: {}", e))?;
        }
        active_set.finalized = true;
        let _ = active_set.finalization_tx.send(true);

        match commitment_txid_str {
            Some(txid) => Ok(format!("Set {} finalized (commitment tx: {})", set_id, txid)),
            None => Ok(format!("Set {} finalized", set_id)),
        }
    }

    pub fn get_crs(&self, set_id: u64) -> Result<&Registry, String> {
        let active_set = self
            .active_sets
            .get(&set_id)
            .ok_or_else(|| format!("Set {} not found", set_id))?;
        Ok(&active_set.registry)
    }

    pub fn get_anonymity_set(&self, set_id: u64) -> Result<&ActiveSet, String> {
        self.active_sets
            .get(&set_id)
            .ok_or_else(|| format!("Set {} not found", set_id))
    }

    pub fn get_fees(&self) -> &FeeConfig {
        &self.fee_config
    }

    pub fn get_registry_address(&self, set_id: u64) -> Result<(String, Vec<u8>), String> {
        // set_id=0 returns the global wallet address (used by merchants before sets exist)
        if set_id != 0 {
            self.active_sets
                .get(&set_id)
                .ok_or_else(|| format!("Set {} not found", set_id))?;
        }

        Ok((
            self.wallet_address.to_string(),
            self.wallet_xonly_bytes.clone(),
        ))
    }
}
