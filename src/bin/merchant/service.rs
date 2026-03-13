use veiled::core::crs::Crs;
use veiled::core::merchant::Merchant;
use veiled::core::payment_identity::{
    deserialize_payment_identity_registration_proof, PaymentIdentityRegistration,
};
use veiled::core::request::{pseudonym_to_address, verify_payment_request, PaymentRequestProof};
use veiled::core::types::Commitment;

use crate::pb::merchant_service_server::MerchantService;
use crate::pb::{
    PaymentRegistrationRequest, PaymentRegistrationResponse, PaymentRequestMsg,
    PaymentRequestResponse,
};
use bitcoin::Network;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

pub struct MerchantGrpcService {
    pub merchant: Arc<Mutex<Merchant>>,
    pub crs: Arc<Crs>,
    pub anonymity_set: Arc<Vec<Commitment>>,
}

impl MerchantGrpcService {
    pub fn new(merchant: Merchant, crs: Crs, anonymity_set: Vec<Commitment>) -> Self {
        Self {
            merchant: Arc::new(Mutex::new(merchant)),
            crs: Arc::new(crs),
            anonymity_set: Arc::new(anonymity_set),
        }
    }
}

#[tonic::async_trait]
impl MerchantService for MerchantGrpcService {
    async fn submit_payment_registration(
        &self,
        request: Request<PaymentRegistrationRequest>,
    ) -> Result<Response<PaymentRegistrationResponse>, Status> {
        let req = request.into_inner();

        let pseudonym: [u8; 33] = req
            .pseudonym
            .try_into()
            .map_err(|_| Status::invalid_argument("pseudonym must be 33 bytes"))?;

        let public_nullifier: [u8; 33] = req
            .public_nullifier
            .try_into()
            .map_err(|_| Status::invalid_argument("public_nullifier must be 33 bytes"))?;

        let proof = deserialize_payment_identity_registration_proof(&req.proof)
            .map_err(|e| Status::invalid_argument(format!("invalid proof: {}", e)))?;

        let registration = PaymentIdentityRegistration {
            pseudonym,
            public_nullifier,
            set_id: req.set_id,
            service_index: req.service_index as usize,
            friendly_name: req.friendly_name.clone(),
            proof,
        };

        let mut merchant = self.merchant.lock().await;
        merchant
            .receive_payment_registration(&self.crs, &self.anonymity_set, &registration)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        Ok(Response::new(PaymentRegistrationResponse {
            message: format!(
                "Payment identity '{}' registered successfully",
                req.friendly_name
            ),
        }))
    }

    async fn submit_payment_request(
        &self,
        request: Request<PaymentRequestMsg>,
    ) -> Result<Response<PaymentRequestResponse>, Status> {
        let req = request.into_inner();

        let pseudonym: [u8; 33] = req
            .pseudonym
            .try_into()
            .map_err(|_| Status::invalid_argument("pseudonym must be 33 bytes"))?;

        let proof_r: [u8; 33] = req
            .proof_r
            .try_into()
            .map_err(|_| Status::invalid_argument("proof_r must be 33 bytes"))?;

        let proof_s: [u8; 32] = req
            .proof_s
            .try_into()
            .map_err(|_| Status::invalid_argument("proof_s must be 32 bytes"))?;

        let proof = PaymentRequestProof {
            r: proof_r,
            s: proof_s,
        };

        let verified = verify_payment_request(&self.crs.g, &pseudonym, &proof);
        if !verified {
            return Err(Status::invalid_argument("payment request proof is invalid"));
        }

        let merchant = self.merchant.lock().await;
        let registered = merchant
            .registered_identities
            .get(&pseudonym)
            .ok_or_else(|| Status::not_found("pseudonym not registered with this merchant"))?;

        let address = pseudonym_to_address(&pseudonym, Network::Bitcoin)
            .map_err(|e| Status::internal(format!("failed to derive address: {}", e)))?;

        Ok(Response::new(PaymentRequestResponse {
            address: address.to_string(),
            friendly_name: registered.friendly_name.clone(),
        }))
    }
}
