use crate::core::registry::Registry;
use crate::core::Merchant;
use crate::registry::db;
use crate::registry::service::MerchantInfo;
use crate::registry::wallet::{btc_value_to_sats, create_bdk_wallet, derive_wallet_address, generate_mnemonic, sync_bdk_wallet};
use bdk_bitcoind_rpc::bitcoincore_rpc::{Client, RpcApi};
use bdk_wallet::bitcoin::FeeRate;
#[allow(deprecated)]
use bdk_wallet::SignOptions;
use bitcoin::hashes::Hash;
use bitcoin::{Address, OutPoint, Txid};
use rusqlite::Connection as SqlConnection;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{info, warn};

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
    pub rpc_client: Option<Arc<Client>>,
    pub wallet_mnemonic: String,
    pub wallet_address: Address,
    /// 32-byte x-only public key from the BDK wallet's P2TR address.
    pub wallet_xonly_bytes: Vec<u8>,
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
        let mnemonic = generate_mnemonic();
        let (wallet_address, wallet_xonly_bytes) =
            derive_wallet_address(&mnemonic).expect("failed to derive wallet address");
        Self {
            merchant_pool: HashMap::new(),
            active_sets: HashMap::new(),
            rpc_client,
            wallet_mnemonic: mnemonic.to_string(),
            wallet_address,
            wallet_xonly_bytes,
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
        self.verify_payment(&outpoint, required_fee)?;

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

    pub fn verify_payment(&self, outpoint: &OutPoint, required_sats: u64) -> Result<(), String> {
        let Some(rpc) = &self.rpc_client else {
            info!("  no RPC client — skipping on-chain payment verification");
            return Ok(());
        };

        let expected_address = self.wallet_address.to_string();
        info!("Verifying payment: tx {}:{}, expect >= {} sats to {}", outpoint.txid, outpoint.vout, required_sats, &expected_address[..20]);

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
            warn!("  address mismatch: expected {}, got {}", expected_address, script_address);
            return Err(format!(
                "Payment output address mismatch: expected {}, got {}",
                expected_address, script_address
            ));
        }

        let value_sats = btc_value_to_sats(&output["value"])?;
        info!("  vout[{}]: {} sats to {} (need >= {})", outpoint.vout, value_sats, script_address, required_sats);
        if value_sats < required_sats {
            warn!("  payment too low: {} < {}", value_sats, required_sats);
            return Err(format!(
                "Payment amount too low: expected {} sats, got {} sats",
                required_sats, value_sats
            ));
        }

        info!("  payment verified OK");
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

        Ok((
            self.wallet_address.to_string(),
            self.wallet_xonly_bytes.clone(),
        ))
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
        let btc_txid: Txid = if let Some(rpc) = &self.rpc_client {
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
            let bdk_bytes: [u8; 32] = *txid.as_byte_array();
            Txid::from_raw_hash(bitcoin::hashes::sha256d::Hash::from_byte_array(bdk_bytes))
        } else {
            info!("No RPC client — generating deterministic dummy txid");
            Txid::from_raw_hash(bitcoin::hashes::sha256d::Hash::hash(output_script.as_bytes()))
        };

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
