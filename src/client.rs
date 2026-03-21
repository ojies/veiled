//! Client helpers for the registry gRPC protocol.
//!
//! Each function corresponds to one named step in the Veiled protocol,
//! so both binaries and integration tests can compose them systematically.

use crate::core::crs::Crs;
use crate::core::types::Commitment;
use crate::registry::pb::registry_client::RegistryClient;
use crate::registry::pb::{
    BeneficiaryRequest, FinalizeSetRequest, GetAnonymitySetRequest, GetCrsRequest,
    GetFeesRequest, GetMerchantsRequest, GetRegistryAddressRequest, MerchantRequest,
};
use tonic::transport::Channel;

pub type RegClient = RegistryClient<Channel>;

// ── Connection ──────────────────────────────────────────────────────────────

/// Open a gRPC connection to the registry.
pub async fn connect(addr: impl Into<String>) -> Result<RegClient, tonic::transport::Error> {
    RegistryClient::connect(addr.into()).await
}

// ── Phase 0 helpers (usable before a set exists) ────────────────────────────

/// Query the current fee schedule.
/// Returns `(beneficiary_fee_sats, merchant_fee_sats)`.
pub async fn get_fees(client: &mut RegClient) -> Result<(u64, u64), tonic::Status> {
    let res = client.get_fees(GetFeesRequest {}).await?.into_inner();
    Ok((res.beneficiary_fee, res.merchant_fee))
}

/// Query the registry's receiving address (and x-only internal key).
/// Use `set_id = [0u8; 32]` before any set has been created.
pub async fn get_registry_address(
    client: &mut RegClient,
    set_id: &[u8],
) -> Result<(String, Vec<u8>), tonic::Status> {
    let res = client
        .get_registry_address(GetRegistryAddressRequest {
            set_id: set_id.to_vec(),
        })
        .await?
        .into_inner();
    Ok((res.address, res.internal_key))
}

/// Register a merchant.  Payment verification is done on-chain by the registry.
pub async fn register_merchant(
    client: &mut RegClient,
    name: &str,
    origin: &str,
    email: &str,
    phone: &str,
    funding_txid: Vec<u8>,
    funding_vout: u32,
) -> Result<String, tonic::Status> {
    let res = client
        .register_merchant(MerchantRequest {
            name: name.to_string(),
            origin: origin.to_string(),
            email: email.to_string(),
            phone: phone.to_string(),
            funding_txid,
            funding_vout,
        })
        .await?
        .into_inner();
    Ok(res.message)
}

/// List all registered merchants (name, origin, credential_generator).
pub async fn get_merchants(
    client: &mut RegClient,
) -> Result<Vec<crate::registry::pb::MerchantEntry>, tonic::Status> {
    let res = client
        .get_merchants(GetMerchantsRequest {})
        .await?
        .into_inner();
    Ok(res.merchants)
}

// ── Phase 2 ─────────────────────────────────────────────────────────────────

/// Register a beneficiary's commitment (`phi`).
/// No `set_id` needed — beneficiaries accumulate in the pending registry until finalization.
/// Returns the index assigned within the pending set.
pub async fn register_beneficiary(
    client: &mut RegClient,
    phi: Vec<u8>,
    name: &str,
    email: &str,
    phone: &str,
    funding_txid: Vec<u8>,
    funding_vout: u32,
) -> Result<u32, tonic::Status> {
    let res = client
        .register_beneficiary(BeneficiaryRequest {
            phi,
            name: name.to_string(),
            email: email.to_string(),
            phone: phone.to_string(),
            funding_txid,
            funding_vout,
        })
        .await?
        .into_inner();
    Ok(res.index)
}

// ── Finalization ─────────────────────────────────────────────────────────────

/// Finalize the pending set: broadcasts the Taproot commitment transaction.
/// Returns the commitment txid as the canonical `set_id` (32 bytes).
pub async fn finalize_set(client: &mut RegClient) -> Result<Vec<u8>, tonic::Status> {
    let res = client
        .finalize_set(FinalizeSetRequest {
            set_id: vec![0u8; 32], // placeholder; actual set_id derived from commitment txid
        })
        .await?
        .into_inner();
    Ok(res.set_id)
}

// ── Post-finalization ────────────────────────────────────────────────────────

/// Fetch the CRS for a finalized set.
/// `set_id` must be the commitment txid returned by [`finalize_set`].
pub async fn get_crs(client: &mut RegClient, set_id: &[u8]) -> Result<Crs, tonic::Status> {
    let res = client
        .get_crs(GetCrsRequest {
            set_id: set_id.to_vec(),
        })
        .await?
        .into_inner();
    Crs::from_bytes(&res.crs_bytes).map_err(|e| tonic::Status::internal(e))
}

/// Subscribe to finalization and block until the full anonymity set is returned.
/// If the set is already finalized (post-`finalize_set`), returns immediately.
pub async fn wait_for_finalization(
    client: &mut RegClient,
    set_id: &[u8],
) -> Result<Vec<Commitment>, tonic::Status> {
    let response = client
        .subscribe_set_finalization(GetAnonymitySetRequest {
            set_id: set_id.to_vec(),
        })
        .await?;
    let mut stream = response.into_inner();
    let anon_res = stream
        .message()
        .await?
        .ok_or_else(|| tonic::Status::unavailable("finalization stream closed without data"))?;

    anon_res
        .commitments
        .into_iter()
        .enumerate()
        .map(|(i, bytes)| {
            let arr: [u8; 33] = bytes
                .try_into()
                .map_err(|_| tonic::Status::data_loss(format!("commitment[{i}] is not 33 bytes")))?;
            Ok(Commitment(arr))
        })
        .collect()
}
