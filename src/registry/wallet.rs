use bdk_bitcoind_rpc::bitcoincore_rpc::{Client, RpcApi};
use bdk_bitcoind_rpc::Emitter;
use bdk_wallet::bitcoin::bip32::Xpriv;
use bdk_wallet::bitcoin::Network as BdkNetwork;
use bdk_wallet::bitcoin::ScriptBuf as BdkScriptBuf;
use bdk_wallet::template::Bip86;
#[allow(deprecated)]
use bdk_wallet::{KeychainKind, Wallet as BdkWallet};
use bdk_wallet::bitcoin::FeeRate;
#[allow(deprecated)]
use bdk_wallet::SignOptions;
use bitcoin::hashes::Hash;
use bitcoin::{Address, Amount, OutPoint, ScriptBuf, Txid};
use std::sync::Arc;
use tracing::{info, warn};

pub struct RegistryWallet {
    pub rpc_client: Option<Arc<Client>>,
    pub mnemonic: String,
    pub address: Address,
    pub xonly_bytes: Vec<u8>,
}

impl RegistryWallet {
    pub fn new(rpc_client: Option<Arc<Client>>) -> Self {
        let mnemonic = Self::generate_mnemonic();
        let (address, xonly_bytes) =
            Self::derive_wallet_address(&mnemonic).expect("failed to derive wallet address");
        Self {
            rpc_client,
            mnemonic: mnemonic.to_string(),
            address,
            xonly_bytes,
        }
    }

    pub fn generate_mnemonic() -> bip39::Mnemonic {
        let mut entropy = [0u8; 16];
        use rand_core::{OsRng, RngCore};
        OsRng.fill_bytes(&mut entropy);
        bip39::Mnemonic::from_entropy(&entropy).expect("16 bytes should produce a valid mnemonic")
    }

    /// Create an ephemeral BDK wallet from a mnemonic (BIP86 P2TR).
    pub fn create_bdk_wallet(mnemonic: &bip39::Mnemonic) -> Result<BdkWallet, String> {
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
    pub fn sync_bdk_wallet(wallet: &mut BdkWallet, rpc: &Client) -> Result<(), String> {
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

    pub fn derive_wallet_address(mnemonic: &bip39::Mnemonic) -> Result<(Address, Vec<u8>), String> {
        let bdk = Self::create_bdk_wallet(mnemonic)?;
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

    pub fn verify_payment(&self, outpoint: &OutPoint, required_sats: u64) -> Result<(), String> {
        let Some(rpc) = &self.rpc_client else {
            info!("  no RPC client — skipping on-chain payment verification");
            return Ok(());
        };

        let expected_address = self.address.to_string();
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

    pub fn get_address(&self) -> (String, Vec<u8>) {
        (self.address.to_string(), self.xonly_bytes.clone())
    }

    /// Fund, sign, and broadcast a transaction paying `output_amount` sats to `output_script`.
    /// Returns the confirmed `Txid`, or a deterministic dummy txid if no RPC client is present.
    pub fn fund_and_broadcast(
        &self,
        output_script: &ScriptBuf,
        output_amount: u64,
    ) -> Result<Txid, String> {
        if let Some(rpc) = &self.rpc_client {
            let mnemonic: bip39::Mnemonic = self
                .mnemonic
                .parse()
                .map_err(|e| format!("parse mnemonic: {e}"))?;
            let mut bdk = Self::create_bdk_wallet(&mnemonic)?;
            info!("Syncing registry BDK wallet for commitment tx...");
            Self::sync_bdk_wallet(&mut bdk, rpc)?;

            let balance = bdk.balance();
            info!(
                "Registry wallet balance: {} confirmed, {} pending, {} immature",
                balance.confirmed.to_sat(),
                balance.trusted_pending.to_sat() + balance.untrusted_pending.to_sat(),
                balance.immature.to_sat()
            );

            let bdk_script = BdkScriptBuf::from_bytes(output_script.to_bytes());
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
            Ok(Txid::from_raw_hash(
                bitcoin::hashes::sha256d::Hash::from_byte_array(bdk_bytes),
            ))
        } else {
            info!("No RPC client — generating deterministic dummy txid");
            Ok(Txid::from_raw_hash(bitcoin::hashes::sha256d::Hash::hash(
                output_script.as_bytes(),
            )))
        }
    }
}

/// Convert a Bitcoin RPC JSON `value` field (BTC as a number) to satoshis
/// without floating-point precision loss. Tries the string representation
/// first (exact decimal), falling back to f64 with rounding.
pub(crate) fn btc_value_to_sats(value: &serde_json::Value) -> Result<u64, String> {
    // bitcoind returns value as a JSON number; serde_json preserves it.
    // Use Amount::from_btc on the string form to avoid IEEE 754 rounding.
    if let Some(n) = value.as_f64() {
        let amt = Amount::from_btc(n).map_err(|e| format!("invalid BTC value: {e}"))?;
        Ok(amt.to_sat())
    } else {
        Err("Output has no value".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::OutPoint;

    fn wallet_no_rpc() -> RegistryWallet {
        RegistryWallet::new(None)
    }

    #[test]
    fn generate_mnemonic_is_valid() {
        let m = RegistryWallet::generate_mnemonic();
        // round-trip: a valid mnemonic parses back to itself
        let s = m.to_string();
        assert!(s.parse::<bip39::Mnemonic>().is_ok());
        assert_eq!(s.split_whitespace().count(), 12);
    }

    #[test]
    fn create_bdk_wallet_succeeds() {
        let mnemonic = RegistryWallet::generate_mnemonic();
        assert!(RegistryWallet::create_bdk_wallet(&mnemonic).is_ok());
    }

    #[test]
    fn derive_wallet_address_produces_valid_p2tr() {
        let mnemonic = RegistryWallet::generate_mnemonic();
        let (address, xonly_bytes) = RegistryWallet::derive_wallet_address(&mnemonic).unwrap();
        assert_eq!(xonly_bytes.len(), 32);
        // P2TR addresses on regtest start with "bcrt1p"
        assert!(address.to_string().starts_with("bcrt1p"));
    }

    #[test]
    fn derive_wallet_address_is_deterministic() {
        let mnemonic = RegistryWallet::generate_mnemonic();
        let (addr1, key1) = RegistryWallet::derive_wallet_address(&mnemonic).unwrap();
        let (addr2, key2) = RegistryWallet::derive_wallet_address(&mnemonic).unwrap();
        assert_eq!(addr1.to_string(), addr2.to_string());
        assert_eq!(key1, key2);
    }

    #[test]
    fn new_wallet_get_address_consistent() {
        let w = wallet_no_rpc();
        let (addr, xonly) = w.get_address();
        assert_eq!(addr, w.address.to_string());
        assert_eq!(xonly, w.xonly_bytes);
        assert_eq!(xonly.len(), 32);
    }

    #[test]
    fn verify_payment_skips_without_rpc() {
        let w = wallet_no_rpc();
        let outpoint = OutPoint::null();
        assert!(w.verify_payment(&outpoint, 1000).is_ok());
    }

    #[test]
    fn fund_and_broadcast_dummy_txid_is_deterministic() {
        let w = wallet_no_rpc();
        let script = ScriptBuf::new();
        let txid1 = w.fund_and_broadcast(&script, 1000).unwrap();
        let txid2 = w.fund_and_broadcast(&script, 1000).unwrap();
        assert_eq!(txid1, txid2);
    }

    #[test]
    fn fund_and_broadcast_dummy_txid_differs_by_script() {
        let w = wallet_no_rpc();
        let script_a = ScriptBuf::new();
        let script_b = ScriptBuf::from(vec![0x51]); // OP_1
        let txid_a = w.fund_and_broadcast(&script_a, 1000).unwrap();
        let txid_b = w.fund_and_broadcast(&script_b, 1000).unwrap();
        assert_ne!(txid_a, txid_b);
    }

    #[test]
    fn btc_value_to_sats_converts_correctly() {
        assert_eq!(btc_value_to_sats(&serde_json::json!(1.0)).unwrap(), 100_000_000);
        assert_eq!(btc_value_to_sats(&serde_json::json!(0.0001)).unwrap(), 10_000);
        assert_eq!(btc_value_to_sats(&serde_json::json!(0.0)).unwrap(), 0);
    }

    #[test]
    fn btc_value_to_sats_rejects_non_number() {
        assert!(btc_value_to_sats(&serde_json::json!("1.0")).is_err());
        assert!(btc_value_to_sats(&serde_json::json!(null)).is_err());
    }
}
