use crate::core::Merchant;
use crate::core::types::Commitment;
use crate::registry::pb::registry_server::Registry;
use crate::registry::pb::{
    BeneficiaryRequest, BeneficiaryResponse,
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
use crate::core::registry::Registry as VieledRegistry;

use tonic::{Request, Response, Status};
use tracing::{info, warn};

pub struct MerchantInfo {
    pub merchant: Merchant,
    pub email: String,
    pub phone: String,
}



/// Configuration for minimum fees enforced by the registry.
#[derive(Debug, Clone)]
pub struct Config {
    /// Minimum sats-per-user required when creating a set.
    pub min_sats_per_user: u64,
    /// Minimum merchant registration fee in sats (future use).
    pub merchant_registration_fee: u64,

    pub beneficiary_capacity: usize,

    pub merchant_capacity: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            min_sats_per_user: 1000,
            merchant_registration_fee: 3000,
            beneficiary_capacity: 4,
            merchant_capacity: 2,
        }
    }
}

pub struct RegistryService {
    pub store: Arc<Mutex<RegistryStore>>,
    pub registery_data: Arc<Mutex<VieledRegistry>>,
    pub config: Config,
    pub state: RegistryState,
}

pub enum RegistryState {
    Empty,
    Pending,
    Finalizing,
}

impl RegistryService {
    pub fn new(store: Arc<Mutex<RegistryStore>>, config: Config) -> Self {
        let new_registry = VieledRegistry::new(config.beneficiary_capacity, config.min_sats_per_user);
        let registery_data = Arc::new(Mutex::new(new_registry));
        let state = RegistryState::Empty;
        Self { store, registery_data, config, state }
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
        let merchant = Merchant::new(&req.name, &req.origin);
        let id = self.registery_data.lock().await.add_merchant(merchant);
        let mut store = self.store.lock().await;
        store
            .add_merchant(&req.name, &req.origin, req.email, req.phone, outpoint, id, self.config.merchant_registration_fee)
            .map_err(|e| {
                warn!("register_merchant FAILED for '{}': {}", req.name, e);
                Status::already_exists(e)
            })?;

        let merchant_names: Vec<String> = store.merchant_pool.keys().cloned().collect();
        info!("register_merchant OK: '{}' (total merchants: {} [{}])", req.name, store.merchant_pool.len(), merchant_names.join(", "));
        Ok(Response::new(MerchantResponse {
            message: format!("Merchant '{}' registered", req.name),
            merchant_id: id as u32,
        }))
    }


    async fn register_beneficiary(
        &self,
        request: Request<BeneficiaryRequest>,
    ) -> Result<Response<BeneficiaryResponse>, Status> {
        let req = request.into_inner();
        info!(
            "register_beneficiary: name='{}', phi={}..., funding={}:{}",
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

        // Verify payment and add beneficiary to the pending registry
        let store = self.store.lock().await;
        store
            .verify_payment(&outpoint, self.config.min_sats_per_user)
            .map_err(|e| Status::failed_precondition(e))?;
        drop(store);

        let mut registry = self.registery_data.lock().await;
        if registry.beneficiary_count() >= self.config.beneficiary_capacity {
            return Err(Status::failed_precondition("Anonymity set is full"));
        }
        if registry.anonymity_set().contains(&phi) {
            return Err(Status::already_exists("Beneficiary already registered"));
        }
        let index = registry.add_beneficiary(phi, outpoint);
        let count = registry.beneficiary_count();

        info!(
            "register_beneficiary OK: '{}' at index {} ({}/{})",
            req.name, index, count, self.config.beneficiary_capacity
        );
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
        let set_id: [u8; 32] = req
            .set_id
            .try_into()
            .map_err(|_| Status::invalid_argument("set_id must be 32 bytes"))?;
        info!("finalize_set: set {}", hex::encode(set_id));

        let mut registry = self.registery_data.lock().await;
        // Build the CRS from all registered merchants before committing.
        registry.setup();
        let mut store = self.store.lock().await;

        let commitment_txid = store
            .create_tx(&mut registry, self.config.beneficiary_capacity, self.config.min_sats_per_user)
            .map_err(|e| {
                warn!("finalize_set create_tx FAILED for {}: {}", hex::encode(set_id), e);
                Status::internal(e)
            })?;

        let txid_bytes = commitment_txid.to_byte_array().to_vec();
        info!("finalize_set OK: set {} -> commitment txid {}", hex::encode(set_id), hex::encode(&txid_bytes));
        Ok(Response::new(FinalizeSetResponse {
            message: format!("Set {} finalized", hex::encode(set_id)),
            set_id: txid_bytes,
        }))
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
        let set_id: [u8; 32] = req
            .set_id
            .try_into()
            .map_err(|_| Status::invalid_argument("set_id must be 32 bytes"))?;
        let store = self.store.lock().await;
        let registry = store.get_crs(set_id).map_err(|e| {
            warn!("get_crs FAILED for set {}: {}", hex::encode(set_id), e);
            Status::not_found(e)
        })?;
        let crs_bytes = registry.crs.to_bytes();
        let set_info = store.active_sets.get(&set_id);
        let ben_count = set_info.map(|s| s.registry.beneficiary_count()).unwrap_or(0);
        let ben_cap = set_info.map(|s| s.beneficiary_capacity).unwrap_or(0);
        let merchant_count = registry.crs.num_merchants();
        info!(
            "get_crs: set {} -> {} bytes ({} merchants, {}/{} beneficiaries)",
            hex::encode(set_id), crs_bytes.len(), merchant_count, ben_count, ben_cap
        );
        Ok(Response::new(GetCrsResponse { crs_bytes }))
    }

    async fn get_anonymity_set(
        &self,
        request: Request<GetAnonymitySetRequest>,
    ) -> Result<Response<GetAnonymitySetResponse>, Status> {
        let req = request.into_inner();
        let set_id: [u8; 32] = req
            .set_id
            .try_into()
            .map_err(|_| Status::invalid_argument("set_id must be 32 bytes"))?;
        let store = self.store.lock().await;
        let active_set = store.get_anonymity_set(set_id).map_err(|e| {
            warn!("get_anonymity_set FAILED for set {}: {}", hex::encode(set_id), e);
            Status::not_found(e)
        })?;
        info!(
            "get_anonymity_set: set {} -> {}/{} members, finalized={}",
            hex::encode(set_id),
            active_set.registry.beneficiary_count(),
            active_set.beneficiary_capacity,
            active_set.finalized
        );

        // Pad the anonymity set to N (ZK proof requires exactly N = 2^M commitments).
        // Duplicate the last commitment for padding slots.
        use crate::core::utils::N;
        let raw = active_set.registry.anonymity_set();
        let mut commitments: Vec<Vec<u8>> = raw.iter().map(|c| c.0.to_vec()).collect();
        if !commitments.is_empty() && commitments.len() < N {
            let last = commitments.last().unwrap().clone();
            while commitments.len() < N {
                commitments.push(last.clone());
            }
            info!("get_anonymity_set: padded {} -> {} commitments (N={})", raw.len(), commitments.len(), N);
        }

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
        let set_id: [u8; 32] = req
            .set_id
            .try_into()
            .map_err(|_| Status::invalid_argument("set_id must be 32 bytes"))?;
        let store = self.store.lock().await;
        let (address, internal_key) = store
            .get_registry_address(set_id)
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
        info!("get_fees: merchant={} sats, beneficiary={} sats", self.config.merchant_registration_fee, self.config.min_sats_per_user);
        Ok(Response::new(GetFeesResponse {
            beneficiary_fee: self.config.min_sats_per_user,
            merchant_fee: self.config.merchant_registration_fee,
        }))
    }

    async fn subscribe_set_finalization(
        &self,
        request: Request<GetAnonymitySetRequest>,
    ) -> Result<Response<Self::SubscribeSetFinalizationStream>, Status> {
        let set_id: [u8; 32] = request
            .into_inner()
            .set_id
            .try_into()
            .map_err(|_| Status::invalid_argument("set_id must be 32 bytes"))?;
        let store = self.store.clone();

        // Wait for the set to exist, then get a watch receiver.
        // This allows merchants to subscribe before the set is created —
        // they'll block here until setup/init creates it.
        let (already_finalized, mut watch_rx) = loop {
            let store_guard = store.lock().await;
            if let Some(active_set) = store_guard.active_sets.get(&set_id) {
                let ben_count = active_set.registry.beneficiary_count();
                let ben_cap = active_set.beneficiary_capacity;
                info!(
                    "subscribe_set_finalization: set {} ({}/{} beneficiaries, finalized={}, total merchants: {})",
                    hex::encode(set_id), ben_count, ben_cap, active_set.finalized, store_guard.merchant_pool.len()
                );
                break (active_set.finalized, active_set.finalization_tx.subscribe());
            }
            info!("subscribe_set_finalization: set {} not found, waiting...", hex::encode(set_id));
            drop(store_guard); // release lock while sleeping
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
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
            use crate::core::utils::N;
            let store_guard = store.lock().await;
            if let Some(active_set) = store_guard.active_sets.get(&set_id) {
                let raw = active_set.registry.anonymity_set();
                let mut commitments: Vec<Vec<u8>> = raw.iter().map(|c| c.0.to_vec()).collect();
                // Pad to N for ZK proof compatibility
                if !commitments.is_empty() && commitments.len() < N {
                    let last = commitments.last().unwrap().clone();
                    while commitments.len() < N {
                        commitments.push(last.clone());
                    }
                }
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
