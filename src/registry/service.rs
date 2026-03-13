use crate::core::types::Commitment;
use crate::registry::pb::registry_server::Registry;
use crate::registry::pb::{
    BeneficiaryRequest, BeneficiaryResponse, CreateSetRequest, CreateSetResponse,
    FinalizeSetRequest, FinalizeSetResponse, GetAggregateAddressRequest,
    GetAggregateAddressResponse, GetAnonymitySetRequest, GetAnonymitySetResponse,
    GetCrsRequest, GetCrsResponse, GetFeesRequest, GetFeesResponse, GetMerchantsRequest,
    GetMerchantsResponse, GetRegistryAddressRequest, GetRegistryAddressResponse,
    GetVtxoTreeRequest, GetVtxoTreeResponse, MerchantEntry, MerchantRequest, MerchantResponse,
};
use crate::registry::store::RegistryStore;
use bitcoin::consensus::Encodable;
use bitcoin::hashes::Hash;
use bitcoin::{OutPoint, Txid};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

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
        let mut store = self.store.lock().await;
        store
            .register_merchant(&req.name, &req.origin, req.email, req.phone)
            .map_err(Status::already_exists)?;
        Ok(Response::new(MerchantResponse {
            message: format!("Merchant '{}' registered", req.name),
        }))
    }

    async fn create_set(
        &self,
        request: Request<CreateSetRequest>,
    ) -> Result<Response<CreateSetResponse>, Status> {
        let req = request.into_inner();
        let mut store = self.store.lock().await;
        store
            .create_set(
                req.set_id,
                &req.merchant_names,
                req.beneficiary_capacity as usize,
                req.sats_per_user,
            )
            .map_err(Status::invalid_argument)?;
        Ok(Response::new(CreateSetResponse {
            message: format!("Set {} created", req.set_id),
        }))
    }

    async fn register_beneficiary(
        &self,
        request: Request<BeneficiaryRequest>,
    ) -> Result<Response<BeneficiaryResponse>, Status> {
        let req = request.into_inner();
        let phi_bytes: [u8; 33] = req
            .phi
            .try_into()
            .map_err(|_| Status::invalid_argument("phi must be 33 bytes"))?;
        let phi = Commitment(phi_bytes);

        let txid: Txid = {
            let txid_bytes: [u8; 32] = req
                .funding_txid
                .try_into()
                .map_err(|_| Status::invalid_argument("funding_txid must be 32 bytes"))?;
            Txid::from_byte_array(txid_bytes)
        };

        let outpoint = OutPoint {
            txid,
            vout: req.funding_vout,
        };

        let mut store = self.store.lock().await;
        let index = store
            .register_beneficiary(req.set_id, phi, outpoint)
            .map_err(Status::invalid_argument)?;

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

        let txid: Txid = if req.funding_txid.is_empty() {
            Txid::all_zeros()
        } else {
            let txid_bytes: [u8; 32] = req
                .funding_txid
                .try_into()
                .map_err(|_| Status::invalid_argument("funding_txid must be 32 bytes"))?;
            Txid::from_byte_array(txid_bytes)
        };

        let funding_outpoint = OutPoint {
            txid,
            vout: req.funding_vout,
        };

        let mut store = self.store.lock().await;
        let (root_txid, fanout_txid) = store
            .finalize_set(req.set_id, req.sats_per_user, funding_outpoint)
            .map_err(Status::failed_precondition)?;

        Ok(Response::new(FinalizeSetResponse {
            message: format!("Set {} finalized", req.set_id),
            root_txid,
            fanout_txid,
        }))
    }

    async fn get_merchants(
        &self,
        _request: Request<GetMerchantsRequest>,
    ) -> Result<Response<GetMerchantsResponse>, Status> {
        let store = self.store.lock().await;
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
            .map_err(Status::not_found)?;
        let crs_bytes = registry.crs.to_bytes();
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
            .map_err(Status::not_found)?;

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

    async fn get_vtxo_tree(
        &self,
        request: Request<GetVtxoTreeRequest>,
    ) -> Result<Response<GetVtxoTreeResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.lock().await;
        let tree = store
            .get_vtxo_tree(req.set_id)
            .map_err(Status::not_found)?;

        let mut root_tx_bytes = Vec::new();
        tree.root()
            .consensus_encode(&mut root_tx_bytes)
            .map_err(|e| Status::internal(format!("Failed to encode root_tx: {}", e)))?;

        let mut fanout_tx_bytes = Vec::new();
        tree.fanout()
            .consensus_encode(&mut fanout_tx_bytes)
            .map_err(|e| Status::internal(format!("Failed to encode fanout_tx: {}", e)))?;

        Ok(Response::new(GetVtxoTreeResponse {
            root_tx: root_tx_bytes,
            fanout_tx: fanout_tx_bytes,
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

    async fn get_aggregate_address(
        &self,
        request: Request<GetAggregateAddressRequest>,
    ) -> Result<Response<GetAggregateAddressResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.lock().await;
        let (address, aggregate_key) = store
            .get_aggregate_address(req.set_id)
            .map_err(Status::failed_precondition)?;
        Ok(Response::new(GetAggregateAddressResponse {
            address,
            aggregate_key,
        }))
    }

    async fn get_fees(
        &self,
        _request: Request<GetFeesRequest>,
    ) -> Result<Response<GetFeesResponse>, Status> {
        let store = self.store.lock().await;
        let fees = store.get_fees();
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
