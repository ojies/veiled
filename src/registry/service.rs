use crate::core::types::{Commitment, Name};
use crate::registry::pb::registry_server::Registry;
use crate::registry::pb::{
    BeneficiaryRequest, BeneficiaryResponse, FinalizeSetRequest, FinalizeSetResponse,
    MerchantRequest, MerchantResponse,
};
use crate::registry::store::RegistryStore;
use std::sync::Arc;
use tokio::sync::Mutex;
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
    async fn register_merchant(
        &self,
        request: Request<MerchantRequest>,
    ) -> Result<Response<MerchantResponse>, Status> {
        let req = request.into_inner();

        let mut gen = [0u8; 33];
        if req.credential_generator.len() != 33 {
            return Err(Status::invalid_argument(
                "credential_generator must be 33 bytes",
            ));
        }
        gen.copy_from_slice(&req.credential_generator);

        let mut store = self.store.lock().await;
        store.register_merchant(
            Name::new(&req.name),
            gen,
            req.origin,
            req.email,
            req.phone,
            req.address,
        );

        Ok(Response::new(MerchantResponse {
            success: true,
            message: format!("Merchant {} registered", req.name),
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

        let mut store = self.store.lock().await;
        match store.register_beneficiary(
            req.set_id,
            phi,
            req.name,
            req.email,
            req.phone,
            req.address,
            req.merchant_names,
        ) {
            Ok(index) => Ok(Response::new(BeneficiaryResponse {
                success: true,
                message: "Beneficiary registered".to_string(),
                index: index as u32,
            })),
            Err(e) => Ok(Response::new(BeneficiaryResponse {
                success: false,
                message: e,
                index: 0,
            })),
        }
    }

    async fn finalize_set(
        &self,
        request: Request<FinalizeSetRequest>,
    ) -> Result<Response<FinalizeSetResponse>, Status> {
        let req = request.into_inner();
        let mut store = self.store.lock().await;
        match store.finalize_set(req.set_id) {
            Ok(_) => Ok(Response::new(FinalizeSetResponse {
                success: true,
                message: format!("Set {} finalized", req.set_id),
            })),
            Err(e) => Ok(Response::new(FinalizeSetResponse {
                success: false,
                message: e,
            })),
        }
    }
}
