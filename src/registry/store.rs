use crate::core::crs::{Crs, Merchant};
use crate::core::tx::{build_identity_tree, IdentityTXO, IdentityTree};
use crate::core::types::{Commitment, Name};
use bitcoin::{Amount, OutPoint};
use std::collections::HashMap;

pub struct BeneficiaryInfo {
    pub phi: Commitment,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub address: String,
}

pub struct MerchantInfo {
    pub merchant: Merchant,
    pub email: String,
    pub phone: String,
    pub address: String,
}

pub struct FinalizedSet {
    pub crs: Crs,
    pub tree: IdentityTree,
}

pub struct AnonymitySetState {
    pub beneficiaries: Vec<BeneficiaryInfo>,
    pub merchant_names: Vec<String>,
}

pub struct RegistryStore {
    pub merchant_pool: HashMap<String, MerchantInfo>,
    pub anonymity_sets: HashMap<u64, AnonymitySetState>,
    pub finalized_sets: HashMap<u64, FinalizedSet>,
    pub beneficiary_capacity: usize,
    pub merchant_capacity: usize,
}

impl RegistryStore {
    pub fn new(beneficiary_capacity: usize, merchant_capacity: usize) -> Self {
        Self {
            merchant_pool: HashMap::new(),
            anonymity_sets: HashMap::new(),
            finalized_sets: HashMap::new(),
            beneficiary_capacity,
            merchant_capacity,
        }
    }

    pub fn register_merchant(
        &mut self,
        name: Name,
        credential_generator: [u8; 33],
        origin: String,
        email: String,
        phone: String,
        address: String,
    ) {
        let name_str = name.0.clone();
        if !self.merchant_pool.contains_key(&name_str) {
            self.merchant_pool.insert(
                name_str,
                MerchantInfo {
                    merchant: Merchant {
                        name,
                        credential_generator,
                        origin,
                    },
                    email,
                    phone,
                    address,
                },
            );
        }
    }

    pub fn register_beneficiary(
        &mut self,
        set_id: u64,
        phi: Commitment,
        name: String,
        email: String,
        phone: String,
        address: String,
        merchant_names: Vec<String>,
    ) -> Result<usize, String> {
        if merchant_names.len() != self.merchant_capacity {
            return Err(format!(
                "Set requires exactly {} merchants, but {} were specified",
                self.merchant_capacity,
                merchant_names.len()
            ));
        }

        // Validate all merchants exist in the pool
        for m_name in &merchant_names {
            if !self.merchant_pool.contains_key(m_name) {
                return Err(format!(
                    "Merchant '{}' not found in registration pool",
                    m_name
                ));
            }
        }

        let state = self
            .anonymity_sets
            .entry(set_id)
            .or_insert_with(|| AnonymitySetState {
                beneficiaries: Vec::new(),
                merchant_names: merchant_names.clone(),
            });

        // Ensure merchant names match the initial configuration for this set
        if state.merchant_names != merchant_names {
            return Err(
                "Merchant selection does not match existing configuration for this set".to_string(),
            );
        }

        if state.beneficiaries.len() >= self.beneficiary_capacity {
            return Err("Anonymity set is full".to_string());
        }

        if state.beneficiaries.iter().any(|b| b.phi == phi) {
            return Err("Beneficiary already registered in this set".to_string());
        }

        state.beneficiaries.push(BeneficiaryInfo {
            phi,
            name,
            email,
            phone,
            address,
        });

        let index = state.beneficiaries.len() - 1;

        // Auto-finalize if capacity is reached
        if state.beneficiaries.len() == self.beneficiary_capacity {
            let _ = self.finalize_set(set_id);
        }

        Ok(index)
    }

    pub fn finalize_set(&mut self, set_id: u64) -> Result<(), String> {
        if self.finalized_sets.contains_key(&set_id) {
            return Ok(());
        }

        let state = self
            .anonymity_sets
            .get(&set_id)
            .ok_or_else(|| "Set not found".to_string())?;

        let beneficiaries = &state.beneficiaries;
        let merchant_names = &state.merchant_names;

        if beneficiaries.len() < self.beneficiary_capacity {
            return Err(format!(
                "Need at least {} beneficiaries to finalize",
                self.beneficiary_capacity
            ));
        }

        if merchant_names.len() != self.merchant_capacity {
            return Err(format!(
                "Set requires exactly {} merchants, but {} were specified",
                self.merchant_capacity,
                merchant_names.len()
            ));
        }

        // 1. Collect Merchants from pool
        let mut merchants = Vec::new();
        for m_name in merchant_names {
            let m_info = self
                .merchant_pool
                .get(m_name)
                .ok_or_else(|| format!("Merchant '{}' not found in registration pool", m_name))?;
            merchants.push(m_info.merchant.clone());
        }

        // 2. Setup CRS
        let crs = Crs::setup(merchants);

        // 3. Build VTxO tree
        let sats_per_user = 10_000;
        let identity_txos: Vec<IdentityTXO> = beneficiaries
            .iter()
            .map(|b| {
                let pk = bitcoin::secp256k1::PublicKey::from_slice(&b.phi.0).unwrap();
                IdentityTXO {
                    pubkey: pk,
                    amount: Amount::from_sat(sats_per_user),
                }
            })
            .collect();

        let tree = build_identity_tree(&identity_txos, OutPoint::null())
            .map_err(|e| format!("Failed to build VTxO tree: {}", e))?;

        self.finalized_sets
            .insert(set_id, FinalizedSet { crs, tree });

        Ok(())
    }

    pub fn get_set_beneficiaries(&self, set_id: u64) -> Option<&Vec<BeneficiaryInfo>> {
        self.anonymity_sets.get(&set_id).map(|s| &s.beneficiaries)
    }
}
