use crate::core::registry::Registry;
use crate::core::Merchant;
use crate::registry::db;
use crate::registry::service::MerchantInfo;
use crate::registry::wallet::RegistryWallet;
use bdk_bitcoind_rpc::bitcoincore_rpc::Client;
use bitcoin::hashes::Hash;
use bitcoin::{OutPoint, Txid};
use rusqlite::Connection as SqlConnection;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::info;

pub struct ActiveSet {
    pub registry: Registry,
    pub beneficiary_capacity: usize,
    pub sats_per_user: u64,
    pub finalized: bool,
    pub finalization_tx: watch::Sender<bool>,
}

pub struct RegistryStore {
    pub merchant_pool: HashMap<String, MerchantInfo>,
    pub active_sets: HashMap<[u8; 32], ActiveSet>,
    pub wallet: RegistryWallet,
    /// Minimum merchants needed to auto-create a set.
    /// Beneficiary capacity per set.
    db: Option<SqlConnection>,
}



impl Default for RegistryStore {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl RegistryStore {
    pub fn new(
        rpc_client: Option<Arc<Client>>,
        db: Option<SqlConnection>,
    ) -> Self {
        Self {
            merchant_pool: HashMap::new(),
            active_sets: HashMap::new(),
            wallet: RegistryWallet::new(rpc_client),
            db,
        }
    }

    pub fn add_merchant(
        &mut self,
        name: &str,
        origin: &str,
        email: String,
        phone: String,
        outpoint: OutPoint,
        _merchant_id: usize,
        required_fee: u64,
    ) -> Result<(), String> {
        if self.merchant_pool.contains_key(name) {
            return Err(format!("Merchant '{}' already registered", name));
        }

        info!("Verifying merchant '{}' payment: tx {}:{}", name, outpoint.txid, outpoint.vout);
        self.wallet.verify_payment(&outpoint, required_fee)?;

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

    pub fn get_anonymity_set(&self, set_id: [u8; 32]) -> Result<&ActiveSet, String> {
        self.active_sets
            .get(&set_id)
            .ok_or_else(|| format!("Set {} not found", hex::encode(set_id)))
    }

    pub fn get_registry_address(&self, set_id: [u8; 32]) -> Result<(String, Vec<u8>), String> {
        // all-zero set_id returns the global wallet address (used by merchants before sets exist)
        if set_id != [0u8; 32] {
            self.active_sets
                .get(&set_id)
                .ok_or_else(|| format!("Set {} not found", hex::encode(set_id)))?;
        }
        Ok(self.wallet.get_address())
    }

    pub fn get_crs(&self, set_id: [u8; 32]) -> Result<&Registry, String> {
        self.active_sets
            .get(&set_id)
            .map(|s| &s.registry)
            .ok_or_else(|| format!("Set {} not found", hex::encode(set_id)))
    }

    pub fn create_tx(
        &mut self,
        registry: &mut Registry,
        beneficiary_capacity: usize,
        sats_per_user: u64,
    ) -> Result<Txid, String> {
        let dummy_outpoint = OutPoint::null();
        let commitment = registry
            .create_anonymity_set(dummy_outpoint)
            .map_err(|e| format!("Failed to create Taproot commitment: {}", e))?;

        // The commitment output amount is the total beneficiary fees for this set.
        let output_amount = beneficiary_capacity as u64 * sats_per_user;
        let output_script = commitment.tx.output[0].script_pubkey.clone();
        info!("Commitment output: {} sats, script {} bytes", output_amount, output_script.len());

        // Fund, sign, and broadcast using BDK wallet
        let btc_txid = self.wallet.fund_and_broadcast(&output_script, output_amount)?;

        let set_id: [u8; 32] = btc_txid.to_byte_array();

        let (finalization_tx, _) = watch::channel(true);
        self.active_sets.insert(
            set_id,
            ActiveSet {
                registry: registry.clone(),
                beneficiary_capacity,
                sats_per_user,
                finalized: true,
                finalization_tx,
            },
        );

        if let Some(ref conn) = self.db {
            db::mark_set_finalized(conn, set_id, Some(&btc_txid.to_string()))
                .map_err(|e| format!("DB error: {e}"))?;
        }

        info!("create_tx OK: commitment txid {}", btc_txid);
        Ok(btc_txid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::registry::Registry;

    fn store() -> RegistryStore {
        RegistryStore::new(None, None)
    }

    #[test]
    fn new_store_is_empty() {
        let s = store();
        assert!(s.merchant_pool.is_empty());
        assert!(s.active_sets.is_empty());
    }

    #[test]
    fn add_merchant_succeeds_without_rpc() {
        let mut s = store();
        let result = s.add_merchant(
            "acme", "https://acme.example", "a@acme.com".into(), "555-0100".into(),
            OutPoint::null(), 0, 1000,
        );
        assert!(result.is_ok());
        assert!(s.merchant_pool.contains_key("acme"));
    }

    #[test]
    fn add_merchant_rejects_duplicate() {
        let mut s = store();
        s.add_merchant(
            "acme", "https://acme.example", "a@acme.com".into(), "555-0100".into(),
            OutPoint::null(), 0, 1000,
        ).unwrap();
        let err = s.add_merchant(
            "acme", "https://acme.example", "a@acme.com".into(), "555-0100".into(),
            OutPoint::null(), 0, 1000,
        ).unwrap_err();
        assert!(err.contains("already registered"));
    }

    #[test]
    fn get_registry_address_zero_set_id_returns_wallet_address() {
        let s = store();
        let (addr, xonly) = s.get_registry_address([0u8; 32]).unwrap();
        assert_eq!(addr, s.wallet.address.to_string());
        assert_eq!(xonly, s.wallet.xonly_bytes);
    }

    #[test]
    fn get_registry_address_unknown_set_id_errors() {
        let s = store();
        let err = s.get_registry_address([1u8; 32]).unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn get_anonymity_set_unknown_errors() {
        let s = store();
        assert!(s.get_anonymity_set([0u8; 32]).is_err());
    }

    #[test]
    fn get_crs_unknown_errors() {
        let s = store();
        assert!(s.get_crs([0u8; 32]).is_err());
    }

    fn registry_with_beneficiary() -> Registry {
        use crate::core::types::Commitment;
        let mut r = Registry::new(4, 1000);
        r.add_beneficiary(Commitment([0u8; 33]), OutPoint::null());
        r
    }

    #[test]
    fn create_tx_inserts_active_set_without_rpc() {
        let mut s = store();
        let mut registry = registry_with_beneficiary();
        let txid = s.create_tx(&mut registry, 4, 1000).unwrap();
        let set_id: [u8; 32] = txid.to_byte_array();
        let active = s.get_anonymity_set(set_id).unwrap();
        assert_eq!(active.beneficiary_capacity, 4);
        assert_eq!(active.sats_per_user, 1000);
        assert!(active.finalized);
    }

    #[test]
    fn create_tx_makes_set_retrievable_by_txid() {
        let mut s = store();
        let mut registry = registry_with_beneficiary();
        let txid = s.create_tx(&mut registry, 4, 1000).unwrap();
        let set_id: [u8; 32] = txid.to_byte_array();
        // address lookup succeeds for the new set_id
        let (addr, xonly) = s.get_registry_address(set_id).unwrap();
        assert_eq!(addr, s.wallet.address.to_string());
        assert_eq!(xonly.len(), 32);
        // crs lookup succeeds
        assert!(s.get_crs(set_id).is_ok());
    }
}
