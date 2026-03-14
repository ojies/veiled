use crate::core::registry::Registry;
use crate::core::types::Commitment;
use crate::core::Merchant;
use crate::registry::db;
use bdk_bitcoind_rpc::bitcoincore_rpc::{Client, RpcApi};
use bitcoin::hashes::Hash;
use bitcoin::key::TapTweak;
use bitcoin::secp256k1::{Keypair, Message, Secp256k1, SecretKey, XOnlyPublicKey};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::{Address, Amount, Network, OutPoint, ScriptBuf, Sequence, TxIn, TxOut, Witness};
use rusqlite::Connection as SqlConnection;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::info;

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
    /// Beneficiary payment UTXOs: (outpoint, value_sats).
    pub funding_utxos: Vec<(OutPoint, u64)>,
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
            min_sats_per_user: 2_000,
            merchant_registration_fee: 3_000,
        }
    }
}

/// Fixed miner fee estimate (sats) for the commitment transaction on regtest.
const MINER_FEE: u64 = 500;

pub struct RegistryStore {
    pub merchant_pool: HashMap<String, MerchantInfo>,
    pub active_sets: HashMap<u64, ActiveSet>,
    pub rpc_client: Option<Arc<Client>>,
    pub fee_config: FeeConfig,
    pub wallet_keypair: Keypair,
    pub wallet_xonly: XOnlyPublicKey,
    pub wallet_address: Address,
    db: Option<SqlConnection>,
}

fn generate_keypair() -> Keypair {
    use rand_core::{OsRng, RngCore};
    let mut sk_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut sk_bytes);
    let secp = Secp256k1::new();
    let secret_key =
        SecretKey::from_slice(&sk_bytes).expect("32 random bytes should be a valid secret key");
    Keypair::from_secret_key(&secp, &secret_key)
}

fn keypair_from_secret(sk_bytes: &[u8; 32]) -> Keypair {
    let secp = Secp256k1::new();
    let secret_key = SecretKey::from_slice(sk_bytes).expect("invalid secret key bytes");
    Keypair::from_secret_key(&secp, &secret_key)
}

fn derive_wallet_address(keypair: &Keypair) -> (XOnlyPublicKey, Address) {
    let secp = Secp256k1::new();
    let (xonly, _) = keypair.x_only_public_key();
    let address = Address::p2tr(&secp, xonly, None, Network::Regtest);
    (xonly, address)
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
        let keypair = generate_keypair();
        let (wallet_xonly, wallet_address) = derive_wallet_address(&keypair);
        Self {
            merchant_pool: HashMap::new(),
            active_sets: HashMap::new(),
            rpc_client,
            fee_config,
            wallet_keypair: keypair,
            wallet_xonly,
            wallet_address,
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

        // Load or generate wallet keypair
        let wallet_key = db::load_wallet_key(&conn)
            .map_err(|e| format!("Failed to load wallet key: {}", e))?;
        let keypair = if let Some(sk_bytes) = wallet_key {
            info!("Loaded wallet keypair from database");
            keypair_from_secret(&sk_bytes)
        } else {
            let kp = generate_keypair();
            let sk = SecretKey::from_keypair(&kp);
            db::save_wallet_key(&conn, &sk.secret_bytes())
                .map_err(|e| format!("Failed to save wallet key: {}", e))?;
            info!("Generated and saved new wallet keypair");
            kp
        };
        let (wallet_xonly, wallet_address) = derive_wallet_address(&keypair);
        info!("Wallet address: {}", wallet_address);

        let mut store = Self {
            merchant_pool: HashMap::new(),
            active_sets: HashMap::new(),
            rpc_client,
            fee_config,
            wallet_keypair: keypair,
            wallet_xonly,
            wallet_address,
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
                    funding_utxos: Vec::new(),
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
            active_set.funding_utxos.push((outpoint, c.value));
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

            let raw_tx: serde_json::Value = rpc
                .call(
                    "getrawtransaction",
                    &[
                        serde_json::json!(outpoint.txid.to_string()),
                        serde_json::json!(true),
                    ],
                )
                .map_err(|e| format!("Failed to fetch transaction {}: {}", outpoint.txid, e))?;

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
                return Err(format!(
                    "Payment output address mismatch: expected {}, got {}",
                    expected_address, script_address
                ));
            }

            let value_btc = output["value"]
                .as_f64()
                .ok_or("Output has no value")?;
            let value_sats = (value_btc * 100_000_000.0).round() as u64;
            if value_sats < required_fee {
                return Err(format!(
                    "Merchant registration fee too low: expected {} sats, got {} sats",
                    required_fee, value_sats
                ));
            }
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
                funding_utxos: Vec::new(),
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
        let value_sats = if let Some(rpc) = &self.rpc_client {
            let expected_address = self.wallet_address.to_string();

            // Fetch the transaction from bitcoind
            let raw_tx: serde_json::Value = rpc
                .call(
                    "getrawtransaction",
                    &[
                        serde_json::json!(outpoint.txid.to_string()),
                        serde_json::json!(true),
                    ],
                )
                .map_err(|e| format!("Failed to fetch transaction {}: {}", outpoint.txid, e))?;

            // Verify the output at the specified vout
            let vout_array = raw_tx["vout"]
                .as_array()
                .ok_or("Transaction has no vout array")?;
            let output = vout_array
                .get(outpoint.vout as usize)
                .ok_or(format!("vout index {} not found in tx", outpoint.vout))?;

            // Check the address matches the registry wallet's P2TR address
            let script_address = output["scriptPubKey"]["address"]
                .as_str()
                .ok_or("Output has no address")?;
            if script_address != expected_address {
                return Err(format!(
                    "Payment output address mismatch: expected {}, got {}",
                    expected_address, script_address
                ));
            }

            // Check the amount (value is in BTC as f64, convert to sats)
            let value_btc = output["value"]
                .as_f64()
                .ok_or("Output has no value")?;
            let vs = (value_btc * 100_000_000.0).round() as u64;
            if vs < sats_per_user {
                return Err(format!(
                    "Payment amount too low: expected {} sats, got {} sats",
                    sats_per_user, vs
                ));
            }
            vs
        } else {
            // No RPC client (tests/demo) — assume sats_per_user
            sats_per_user
        };

        let index = active_set.registry.add_beneficiary(phi, outpoint);
        active_set.funding_utxos.push((outpoint, value_sats));

        if let Some(ref conn) = self.db {
            db::save_commitment(conn, set_id, index, &phi, &outpoint, value_sats)
                .map_err(|e| format!("DB error saving commitment: {}", e))?;
        }

        Ok(index)
    }

    /// Finalize the set: create Taproot commitment, sign, and broadcast.
    ///
    /// The registry self-funds the commitment transaction using the beneficiary
    /// payment UTXOs it has collected. Each beneficiary payment becomes an input
    /// to the commitment transaction.
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

        let funding_utxos = active_set.funding_utxos.clone();
        let total_input: u64 = funding_utxos.iter().map(|(_, v)| *v).sum();

        // Create Taproot commitment (Merkle root of anonymity set).
        // We pass a dummy outpoint; we'll replace inputs with actual beneficiary UTXOs.
        let dummy_outpoint = OutPoint::null();
        let mut commitment = active_set
            .registry
            .create_anonymity_set(dummy_outpoint)
            .map_err(|e| format!("Failed to create Taproot commitment: {}", e))?;

        // Replace the dummy input with actual beneficiary payment UTXOs
        commitment.tx.input = funding_utxos
            .iter()
            .map(|(op, _)| TxIn {
                previous_output: *op,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::default(),
            })
            .collect();

        // Adjust output value to account for miner fee
        let output_value = total_input.saturating_sub(MINER_FEE);
        commitment.tx.output[0].value = Amount::from_sat(output_value);

        // Sign all inputs using BIP341 Taproot key-spend
        if self.rpc_client.is_some() {
            let secp = Secp256k1::new();
            let tweaked_keypair = self.wallet_keypair.tap_tweak(&secp, None);
            let wallet_script = self.wallet_address.script_pubkey();

            let prevouts: Vec<TxOut> = funding_utxos
                .iter()
                .map(|(_, v)| TxOut {
                    value: Amount::from_sat(*v),
                    script_pubkey: wallet_script.clone(),
                })
                .collect();
            let prevouts_ref: Vec<&TxOut> = prevouts.iter().collect();

            for i in 0..commitment.tx.input.len() {
                let mut sighash_cache = SighashCache::new(&commitment.tx);
                let sighash = sighash_cache
                    .taproot_key_spend_signature_hash(
                        i,
                        &Prevouts::All(&prevouts_ref),
                        TapSighashType::Default,
                    )
                    .map_err(|e| format!("Sighash computation failed for input {}: {}", i, e))?;

                let msg = Message::from_digest(sighash.to_byte_array());
                let sig = secp.sign_schnorr(&msg, &tweaked_keypair.to_keypair());

                let mut witness = Witness::new();
                witness.push(sig.as_ref());
                commitment.tx.input[i].witness = witness;
            }

            // Broadcast the signed commitment transaction
            self.rpc_client
                .as_ref()
                .unwrap()
                .send_raw_transaction(&commitment.tx)
                .map_err(|e| format!("Failed to broadcast commitment tx: {}", e))?;

            info!(
                "Broadcast commitment tx: {}",
                commitment.tx.compute_txid()
            );
        }

        active_set.finalized = true;
        if let Some(ref conn) = self.db {
            db::mark_set_finalized(conn, set_id)
                .map_err(|e| format!("DB error marking set finalized: {}", e))?;
        }
        let _ = active_set.finalization_tx.send(true);

        Ok(format!("Set {} finalized", set_id))
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
            self.wallet_xonly.serialize().to_vec(),
        ))
    }
}
