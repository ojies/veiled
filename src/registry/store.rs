use crate::core::registry::Registry;
use crate::core::tx::{build_identity_tree, IdentityTXO, IdentityTree};
use crate::core::types::Commitment;
use crate::core::Merchant;
use bitcoin::{Amount, OutPoint};
use std::collections::HashMap;
use tokio::sync::watch;

pub struct MerchantInfo {
    pub merchant: Merchant,
    pub email: String,
    pub phone: String,
}

pub struct ActiveSet {
    pub registry: Registry,
    pub beneficiary_capacity: usize,
    pub finalized: bool,
    pub tree: Option<IdentityTree>,
    pub finalization_tx: watch::Sender<bool>,
}

pub struct RegistryStore {
    pub merchant_pool: HashMap<String, MerchantInfo>,
    pub active_sets: HashMap<u64, ActiveSet>,
}

impl Default for RegistryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryStore {
    pub fn new() -> Self {
        Self {
            merchant_pool: HashMap::new(),
            active_sets: HashMap::new(),
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
    ) -> Result<(), String> {
        if self.active_sets.contains_key(&set_id) {
            return Err(format!("Set {} already exists", set_id));
        }

        if merchant_names.is_empty() {
            return Err("At least one merchant is required".to_string());
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
        let mut registry = Registry::new(beneficiary_capacity);
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

        let index = active_set.registry.add_beneficiary(phi);

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
}
