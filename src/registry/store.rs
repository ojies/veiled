use crate::core::registry::Registry;
use crate::core::tx::{aggregate_secret_key, build_identity_tree, p2tr_script, sign_tx, IdentityTXO, IdentityTree};
use crate::core::types::Commitment;
use crate::core::Merchant;
use crate::registry::db;
use bdk_bitcoind_rpc::bitcoincore_rpc::{Client, RpcApi};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::{Address, Amount, Network, OutPoint, TxOut};
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
    pub tree: Option<IdentityTree>,
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
            min_sats_per_user: 2_000,
            merchant_registration_fee: 3_000,
        }
    }
}

pub struct RegistryStore {
    pub merchant_pool: HashMap<String, MerchantInfo>,
    pub active_sets: HashMap<u64, ActiveSet>,
    pub rpc_client: Option<Arc<Client>>,
    pub fee_config: FeeConfig,
    db: Option<SqlConnection>,
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
        Self {
            merchant_pool: HashMap::new(),
            active_sets: HashMap::new(),
            rpc_client,
            fee_config,
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

        let mut store = Self::new(rpc_client, fee_config, Some(conn));

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
                    finalized: false, // set after replaying commitments + tree
                    tree: None,
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

        // 4. Restore finalized sets with VTxO trees
        for t in state.vtxo_trees {
            let active_set = store.active_sets.get_mut(&t.set_id).ok_or_else(|| {
                format!(
                    "DB inconsistency: vtxo_tree references unknown set {}",
                    t.set_id
                )
            })?;
            // Reconstruct IdentityTXO list from anonymity set
            let users: Vec<IdentityTXO> = active_set
                .registry
                .anonymity_set()
                .iter()
                .map(|phi| {
                    let pk = bitcoin::secp256k1::PublicKey::from_slice(&phi.0)
                        .expect("phi should be a valid compressed public key");
                    IdentityTXO {
                        pubkey: pk,
                        amount: Amount::from_sat(active_set.sats_per_user),
                    }
                })
                .collect();

            active_set.tree = Some(IdentityTree {
                root_tx: t.root_tx,
                fanout_tx: t.fanout_tx,
                users,
            });
            active_set.finalized = true;
            let _ = active_set.finalization_tx.send(true);
        }

        // Mark finalized sets that might not have trees (edge case)
        for s in &state.sets {
            if s.finalized {
                if let Some(active_set) = store.active_sets.get_mut(&s.set_id) {
                    active_set.finalized = true;
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
    ) -> Result<(), String> {
        if self.merchant_pool.contains_key(name) {
            return Err(format!("Merchant '{}' already registered", name));
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
                tree: None,
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
        if active_set
            .registry
            .anonymity_set()
            .contains(&phi)
        {
            return Err("Beneficiary already registered in this set".to_string());
        }

        // Verify payment on-chain if RPC client is available
        if let Some(rpc) = &self.rpc_client {
            let registry_pk = active_set.registry.public_key();
            let (xonly, _) = registry_pk.x_only_public_key();
            let secp = Secp256k1::new();
            let registry_address = Address::p2tr(&secp, xonly, None, Network::Regtest);

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

            // Check the address matches the registry's P2TR address
            let script_address = output["scriptPubKey"]["address"]
                .as_str()
                .ok_or("Output has no address")?;
            let expected_address = registry_address.to_string();
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
            let value_sats = (value_btc * 100_000_000.0).round() as u64;
            if value_sats < active_set.sats_per_user {
                return Err(format!(
                    "Payment amount too low: expected {} sats, got {} sats",
                    active_set.sats_per_user, value_sats
                ));
            }
        }

        let index = active_set.registry.add_beneficiary(phi, outpoint);

        if let Some(ref conn) = self.db {
            db::save_commitment(conn, set_id, index, &phi, &outpoint)
                .map_err(|e| format!("DB error saving commitment: {}", e))?;
        }

        Ok(index)
    }

    /// Finalize the set: build the VTxO tree, sign, and broadcast.
    /// Returns (root_txid, fanout_txid) as hex strings.
    pub fn finalize_set(
        &mut self,
        set_id: u64,
        sats_per_user: u64,
        funding_outpoint: OutPoint,
    ) -> Result<(String, String), String> {
        let active_set = self
            .active_sets
            .get_mut(&set_id)
            .ok_or_else(|| format!("Set {} not found", set_id))?;

        if active_set.finalized {
            // Return txids from existing tree if already finalized
            if let Some(ref tree) = active_set.tree {
                return Ok((
                    tree.root_tx.compute_txid().to_string(),
                    tree.fanout_tx.compute_txid().to_string(),
                ));
            }
            return Ok((String::new(), String::new()));
        }

        if active_set.registry.beneficiary_count() < active_set.beneficiary_capacity {
            return Err(format!(
                "Need {} beneficiaries to finalize, have {}",
                active_set.beneficiary_capacity,
                active_set.registry.beneficiary_count()
            ));
        }

        // Build VTxO tree
        let identity_txos: Vec<IdentityTXO> = active_set
            .registry
            .anonymity_set()
            .iter()
            .map(|phi| {
                let pk = bitcoin::secp256k1::PublicKey::from_slice(&phi.0)
                    .expect("phi should be a valid compressed public key");
                IdentityTXO {
                    pubkey: pk,
                    amount: Amount::from_sat(sats_per_user),
                }
            })
            .collect();

        let mut tree = build_identity_tree(&identity_txos, funding_outpoint)
            .map_err(|e| format!("Failed to build VTxO tree: {}", e))?;

        // Sign the commitment transactions with the aggregate key
        let all_keys: Vec<_> = identity_txos.iter().map(|u| u.pubkey).collect();
        let agg_sk = aggregate_secret_key(&all_keys);
        let secp = Secp256k1::new();
        let agg_pk = agg_sk.public_key(&secp);
        let (agg_xonly, _) = agg_pk.x_only_public_key();

        // The funding UTXO is locked to the aggregate key's P2TR script
        let funding_prevout = TxOut {
            value: tree.value(),
            script_pubkey: p2tr_script(&agg_xonly),
        };

        // Sign root_tx (spends the funding UTXO)
        sign_tx(&mut tree.root_tx, &agg_sk, &funding_prevout);

        // Sign fanout_tx (spends root_tx output[0])
        let root_output = tree.root_tx.output[0].clone();
        sign_tx(&mut tree.fanout_tx, &agg_sk, &root_output);

        let root_txid = tree.root_tx.compute_txid().to_string();
        let fanout_txid = tree.fanout_tx.compute_txid().to_string();

        // Broadcast if RPC client is available
        if let Some(ref rpc) = self.rpc_client {
            rpc.send_raw_transaction(&tree.root_tx)
                .map_err(|e| format!("Failed to broadcast root_tx: {}", e))?;
            rpc.send_raw_transaction(&tree.fanout_tx)
                .map_err(|e| format!("Failed to broadcast fanout_tx: {}", e))?;
        }

        active_set.finalized = true;
        if let Some(ref conn) = self.db {
            db::save_vtxo_tree(conn, set_id, &tree.root_tx, &tree.fanout_tx)
                .map_err(|e| format!("DB error saving vtxo tree: {}", e))?;
            db::mark_set_finalized(conn, set_id)
                .map_err(|e| format!("DB error marking set finalized: {}", e))?;
        }
        active_set.tree = Some(tree);
        let _ = active_set.finalization_tx.send(true);

        Ok((root_txid, fanout_txid))
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

    pub fn get_vtxo_tree(&self, set_id: u64) -> Result<&IdentityTree, String> {
        let active_set = self
            .active_sets
            .get(&set_id)
            .ok_or_else(|| format!("Set {} not found", set_id))?;
        active_set
            .tree
            .as_ref()
            .ok_or_else(|| format!("Set {} not yet finalized", set_id))
    }

    pub fn get_aggregate_address(&self, set_id: u64) -> Result<(String, Vec<u8>), String> {
        let active_set = self
            .active_sets
            .get(&set_id)
            .ok_or_else(|| format!("Set {} not found", set_id))?;

        let pubkeys: Vec<bitcoin::secp256k1::PublicKey> = active_set
            .registry
            .anonymity_set()
            .iter()
            .map(|phi| {
                bitcoin::secp256k1::PublicKey::from_slice(&phi.0)
                    .expect("phi should be a valid compressed public key")
            })
            .collect();

        if pubkeys.len() < 2 {
            return Err("Need at least 2 beneficiaries to compute aggregate key".to_string());
        }

        let agg_sk = aggregate_secret_key(&pubkeys);
        let secp = Secp256k1::new();
        let agg_pk = agg_sk.public_key(&secp);
        let (agg_xonly, _) = agg_pk.x_only_public_key();

        // Use p2tr_script (which uses dangerous_assume_tweaked) to match
        // the script used in create_root_tx — NOT Address::p2tr() which
        // applies a BIP341 tweak.
        let script = p2tr_script(&agg_xonly);
        let address = Address::from_script(&script, Network::Regtest)
            .map_err(|e| format!("Failed to derive aggregate address: {}", e))?;

        Ok((address.to_string(), agg_xonly.serialize().to_vec()))
    }

    pub fn get_fees(&self) -> &FeeConfig {
        &self.fee_config
    }

    pub fn get_registry_address(&self, set_id: u64) -> Result<(String, Vec<u8>), String> {
        let active_set = self
            .active_sets
            .get(&set_id)
            .ok_or_else(|| format!("Set {} not found", set_id))?;

        let pk = active_set.registry.public_key();
        let (xonly, _) = pk.x_only_public_key();
        let secp = Secp256k1::new();
        let address = Address::p2tr(&secp, xonly, None, Network::Regtest);

        Ok((address.to_string(), xonly.serialize().to_vec()))
    }
}
