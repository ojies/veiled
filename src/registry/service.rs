use crate::core::types::Commitment;
use crate::registry::pb::registry_server::Registry;
use crate::registry::pb::{
    BeneficiaryRequest, BeneficiaryResponse, CreateSetRequest, CreateSetResponse,
    FinalizeSetRequest, FinalizeSetResponse, GetAnonymitySetRequest, GetAnonymitySetResponse,
    GetCrsRequest, GetCrsResponse, GetFeesRequest, GetFeesResponse, GetMerchantsRequest,
    GetMerchantsResponse, GetRegistryAddressRequest, GetRegistryAddressResponse,
    MerchantEntry, MerchantRequest, MerchantResponse,
};
use crate::registry::store::RegistryStore;
use bitcoin::hashes::Hash;
use bitcoin::{OutPoint, Txid};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

pub struct RegistryService {
    pub store: Arc<Mutex<RegistryStore>>,
}

impl RegistryService {
    pub fn new(store: Arc<Mutex<RegistryStore>>) -> Self {
        Self { store }
    }
}

#[tonic::async_trait]
impl Registry for RegistryService {
    type SubscribeSetFinalizationStream =
        ReceiverStream<Result<GetAnonymitySetResponse, Status>>;

    async fn register_merchant(
        &self,
        request: Request<MerchantRequest>,
    ) -> Result<Response<MerchantResponse>, Status> {
        let req = request.into_inner();
        info!("register_merchant: name='{}', origin='{}', funding={}:{}", req.name, req.origin, hex::encode(&req.funding_txid), req.funding_vout);

        let txid: Txid = {
            let mut txid_bytes: [u8; 32] = req
                .funding_txid
                .try_into()
                .map_err(|_| Status::invalid_argument("funding_txid must be 32 bytes"))?;
            txid_bytes.reverse();
            Txid::from_byte_array(txid_bytes)
        };
        let outpoint = OutPoint {
            txid,
            vout: req.funding_vout,
        };

        let mut store = self.store.lock().await;
        store
            .register_merchant(&req.name, &req.origin, req.email, req.phone, outpoint)
            .map_err(|e| {
                warn!("register_merchant FAILED for '{}': {}", req.name, e);
                Status::already_exists(e)
            })?;
        let merchant_names: Vec<&str> = store.merchant_pool.keys().map(|k| k.as_str()).collect();
        info!("register_merchant OK: '{}' (total merchants: {} [{}])", req.name, store.merchant_pool.len(), merchant_names.join(", "));
        Ok(Response::new(MerchantResponse {
            message: format!("Merchant '{}' registered", req.name),
        }))
    }

    async fn create_set(
        &self,
        request: Request<CreateSetRequest>,
    ) -> Result<Response<CreateSetResponse>, Status> {
        let req = request.into_inner();
        info!(
            "create_set: id={}, merchants=[{}], capacity={}, sats_per_user={}",
            req.set_id,
            req.merchant_names.join(", "),
            req.beneficiary_capacity,
            req.sats_per_user
        );
        let mut store = self.store.lock().await;
        store
            .create_set(
                req.set_id,
                &req.merchant_names,
                req.beneficiary_capacity as usize,
                req.sats_per_user,
            )
            .map_err(|e| {
                warn!("create_set FAILED for set {}: {}", req.set_id, e);
                Status::invalid_argument(e)
            })?;
        info!(
            "create_set OK: set {} created (merchants: {}, beneficiary capacity: {}, fee: {} sats/user, total sets: {})",
            req.set_id,
            req.merchant_names.len(),
            req.beneficiary_capacity,
            req.sats_per_user,
            store.active_sets.len()
        );
        Ok(Response::new(CreateSetResponse {
            message: format!("Set {} created", req.set_id),
        }))
    }

    async fn register_beneficiary(
        &self,
        request: Request<BeneficiaryRequest>,
    ) -> Result<Response<BeneficiaryResponse>, Status> {
        let req = request.into_inner();
        info!(
            "register_beneficiary: set={}, name='{}', phi={}..., funding={}:{}",
            req.set_id,
            req.name,
            hex::encode(&req.phi[..8.min(req.phi.len())]),
            hex::encode(&req.funding_txid),
            req.funding_vout
        );
        let phi_bytes: [u8; 33] = req
            .phi
            .try_into()
            .map_err(|_| Status::invalid_argument("phi must be 33 bytes"))?;
        let phi = Commitment(phi_bytes);

        let txid: Txid = {
            let mut txid_bytes: [u8; 32] = req
                .funding_txid
                .try_into()
                .map_err(|_| Status::invalid_argument("funding_txid must be 32 bytes"))?;
            txid_bytes.reverse();
            Txid::from_byte_array(txid_bytes)
        };

        let outpoint = OutPoint {
            txid,
            vout: req.funding_vout,
        };

        let mut store = self.store.lock().await;
        let index = store
            .register_beneficiary(req.set_id, phi, outpoint)
            .map_err(|e| {
                warn!("register_beneficiary FAILED for set {}: {}", req.set_id, e);
                Status::invalid_argument(e)
            })?;

        let count = store.active_sets.get(&req.set_id).map(|s| s.registry.beneficiary_count()).unwrap_or(0);
        let capacity = store.active_sets.get(&req.set_id).map(|s| s.beneficiary_capacity).unwrap_or(0);
        info!("register_beneficiary OK: '{}' at index {} (set {}: {}/{})", req.name, index, req.set_id, count, capacity);

        Ok(Response::new(BeneficiaryResponse {
            message: "Beneficiary registered".to_string(),
            index: index as u32,
        }))
    }

    async fn finalize_set(
        &self,
        request: Request<FinalizeSetRequest>,
    ) -> Result<Response<FinalizeSetResponse>, Status> {
        let req = request.into_inner();
        info!("finalize_set: set {}", req.set_id);
        let mut store = self.store.lock().await;
        let message = store
            .finalize_set(req.set_id)
            .map_err(|e| {
                warn!("finalize_set FAILED for set {}: {}", req.set_id, e);
                Status::failed_precondition(e)
            })?;
        info!("finalize_set OK: set {} — {}", req.set_id, message);
        Ok(Response::new(FinalizeSetResponse { message }))
    }

    async fn get_merchants(
        &self,
        _request: Request<GetMerchantsRequest>,
    ) -> Result<Response<GetMerchantsResponse>, Status> {
        let store = self.store.lock().await;
        info!("get_merchants: returning {} merchants", store.merchant_pool.len());
        let merchants: Vec<MerchantEntry> = store
            .merchant_pool
            .values()
            .map(|info| MerchantEntry {
                name: info.merchant.name.as_str().to_string(),
                origin: info.merchant.origin.clone(),
                credential_generator: info.merchant.credential_generator.to_vec(),
            })
            .collect();
        Ok(Response::new(GetMerchantsResponse { merchants }))
    }

    async fn get_crs(
        &self,
        request: Request<GetCrsRequest>,
    ) -> Result<Response<GetCrsResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.lock().await;
        let registry = store
            .get_crs(req.set_id)
            .map_err(|e| {
                warn!("get_crs FAILED for set {}: {}", req.set_id, e);
                Status::not_found(e)
            })?;
        let crs_bytes = registry.crs.to_bytes();
        let set_info = store.active_sets.get(&req.set_id);
        let ben_count = set_info.map(|s| s.registry.beneficiary_count()).unwrap_or(0);
        let ben_cap = set_info.map(|s| s.beneficiary_capacity).unwrap_or(0);
        let merchant_count = registry.crs.num_merchants();
        info!(
            "get_crs: set {} -> {} bytes ({} merchants, {}/{} beneficiaries)",
            req.set_id, crs_bytes.len(), merchant_count, ben_count, ben_cap
        );
        Ok(Response::new(GetCrsResponse { crs_bytes }))
    }

    async fn get_anonymity_set(
        &self,
        request: Request<GetAnonymitySetRequest>,
    ) -> Result<Response<GetAnonymitySetResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.lock().await;
        let active_set = store
            .get_anonymity_set(req.set_id)
            .map_err(|e| {
                warn!("get_anonymity_set FAILED for set {}: {}", req.set_id, e);
                Status::not_found(e)
            })?;
        info!(
            "get_anonymity_set: set {} -> {}/{} members, finalized={}",
            req.set_id,
            active_set.registry.beneficiary_count(),
            active_set.beneficiary_capacity,
            active_set.finalized
        );

        let commitments: Vec<Vec<u8>> = active_set
            .registry
            .anonymity_set()
            .iter()
            .map(|c| c.0.to_vec())
            .collect();

        Ok(Response::new(GetAnonymitySetResponse {
            commitments,
            finalized: active_set.finalized,
            count: active_set.registry.beneficiary_count() as u32,
            capacity: active_set.beneficiary_capacity as u32,
        }))
    }

    async fn get_registry_address(
        &self,
        request: Request<GetRegistryAddressRequest>,
    ) -> Result<Response<GetRegistryAddressResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.lock().await;
        let (address, internal_key) = store
            .get_registry_address(req.set_id)
            .map_err(Status::not_found)?;
        Ok(Response::new(GetRegistryAddressResponse {
            address,
            internal_key,
        }))
    }

    async fn get_fees(
        &self,
        _request: Request<GetFeesRequest>,
    ) -> Result<Response<GetFeesResponse>, Status> {
        let store = self.store.lock().await;
        let fees = store.get_fees();
        info!("get_fees: merchant={} sats, beneficiary={} sats", fees.merchant_registration_fee, fees.min_sats_per_user);
        Ok(Response::new(GetFeesResponse {
            beneficiary_fee: fees.min_sats_per_user,
            merchant_fee: fees.merchant_registration_fee,
        }))
    }

    async fn subscribe_set_finalization(
        &self,
        request: Request<GetAnonymitySetRequest>,
    ) -> Result<Response<Self::SubscribeSetFinalizationStream>, Status> {
        let set_id = request.into_inner().set_id;
        let store = self.store.clone();

        // Get a watch receiver (briefly lock store)
        let (already_finalized, mut watch_rx) = {
            let store_guard = store.lock().await;
            let set_info = store_guard.active_sets.get(&set_id);
            let ben_count = set_info.map(|s| s.registry.beneficiary_count()).unwrap_or(0);
            let ben_cap = set_info.map(|s| s.beneficiary_capacity).unwrap_or(0);
            let finalized = set_info.map(|s| s.finalized).unwrap_or(false);
            info!(
                "subscribe_set_finalization: set {} ({}/{} beneficiaries, finalized={}, total merchants: {})",
                set_id, ben_count, ben_cap, finalized, store_guard.merchant_pool.len()
            );
            let active_set = store_guard
                .active_sets
                .get(&set_id)
                .ok_or_else(|| Status::not_found(format!("Set {} not found", set_id)))?;
            (active_set.finalized, active_set.finalization_tx.subscribe())
        };

        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            // Wait for finalization if not already done
            if !already_finalized {
                while !*watch_rx.borrow() {
                    if watch_rx.changed().await.is_err() {
                        return; // sender dropped
                    }
                }
            }

            // Fetch finalized data
            let store_guard = store.lock().await;
            if let Some(active_set) = store_guard.active_sets.get(&set_id) {
                let commitments = active_set
                    .registry
                    .anonymity_set()
                    .iter()
                    .map(|c| c.0.to_vec())
                    .collect();
                let _ = tx
                    .send(Ok(GetAnonymitySetResponse {
                        commitments,
                        finalized: true,
                        count: active_set.registry.beneficiary_count() as u32,
                        capacity: active_set.beneficiary_capacity as u32,
                    }))
                    .await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
