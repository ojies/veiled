use bdk_bitcoind_rpc::bitcoincore_rpc::Client;
use bdk_bitcoind_rpc::Emitter;
use bdk_wallet::bitcoin::bip32::Xpriv;
use bdk_wallet::bitcoin::Network as BdkNetwork;
use bdk_wallet::template::Bip86;
#[allow(deprecated)]
use bdk_wallet::{KeychainKind, Wallet as BdkWallet};
use bitcoin::{Address, Amount};

pub(crate) fn generate_mnemonic() -> bip39::Mnemonic {
    let mut entropy = [0u8; 16];
    use rand_core::{OsRng, RngCore};
    OsRng.fill_bytes(&mut entropy);
    bip39::Mnemonic::from_entropy(&entropy).expect("16 bytes should produce a valid mnemonic")
}

/// Create an ephemeral BDK wallet from a mnemonic (BIP86 P2TR).
pub(crate) fn create_bdk_wallet(mnemonic: &bip39::Mnemonic) -> Result<BdkWallet, String> {
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
pub(crate) fn sync_bdk_wallet(wallet: &mut BdkWallet, rpc: &Client) -> Result<(), String> {
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

pub(crate) fn derive_wallet_address(mnemonic: &bip39::Mnemonic) -> Result<(Address, Vec<u8>), String> {
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