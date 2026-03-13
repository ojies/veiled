use crate::core::registry::Registry;
use crate::core::tx::{build_identity_tree, IdentityTXO, IdentityTree};
use crate::core::types::Commitment;
use crate::core::Merchant;
use bdk_bitcoind_rpc::bitcoincore_rpc::{Client, RpcApi};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::{Address, Amount, Network, OutPoint};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::watch;

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
}

impl Default for RegistryStore {
    fn default() -> Self {
        Self::new(None, FeeConfig::default())
    }
}

impl RegistryStore {
    pub fn new(rpc_client: Option<Arc<Client>>, fee_config: FeeConfig) -> Self {
        Self {
            merchant_pool: HashMap::new(),
            active_sets: HashMap::new(),
            rpc_client,
            fee_config,
        }
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
                email,
                phone,
            },
        );
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

        Ok(index)
    }

    pub fn finalize_set(
        &mut self,
        set_id: u64,
        sats_per_user: u64,
        funding_outpoint: OutPoint,
    ) -> Result<(), String> {
        let active_set = self
            .active_sets
            .get_mut(&set_id)
            .ok_or_else(|| format!("Set {} not found", set_id))?;

        if active_set.finalized {
            return Ok(());
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

        let tree = build_identity_tree(&identity_txos, funding_outpoint)
            .map_err(|e| format!("Failed to build VTxO tree: {}", e))?;

        active_set.finalized = true;
        active_set.tree = Some(tree);
        let _ = active_set.finalization_tx.send(true);

        Ok(())
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
