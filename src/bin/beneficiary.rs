mod merchant_pb {
    tonic::include_proto!("merchant");
}

use clap::Parser;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};
use veiled::core::beneficiary::Beneficiary;
use veiled::core::crs::Crs;
use veiled::core::payment_identity::serialize_payment_identity_registration_proof;
use veiled::core::request::create_payment_request;
use veiled::core::types::{Commitment, Name};
use veiled::registry::pb::registry_client::RegistryClient;
use veiled::registry::pb::{
    BeneficiaryRequest, GetAnonymitySetRequest, GetCrsRequest, GetMerchantsRequest,
    GetVtxoTreeRequest,
};

use merchant_pb::merchant_service_client::MerchantServiceClient;
use merchant_pb::{PaymentRegistrationRequest, PaymentRequestMsg};

#[derive(Parser)]
#[command(name = "beneficiary", about = "Veiled Beneficiary: creates credential and registers with registry")]
struct Args {
    /// Friendly name for this beneficiary
    #[arg(short, long)]
    name: String,

    /// Registry gRPC server address
    #[arg(short, long, default_value = "http://[::1]:50051")]
    registry_server: String,

    /// Set ID to join
    #[arg(short, long)]
    set_id: u64,

    /// Merchant gRPC server address (for payment registration and requests)
    #[arg(short, long)]
    merchant_server: Option<String>,

    /// Merchant ID (1-indexed) to register payment identity with
    #[arg(long)]
    merchant_id: Option<u32>,

    /// Merchant name override (default: fetched from registry by merchant_id)
    #[arg(long)]
    merchant_name: Option<String>,

    /// Amount in sats for payment request (triggers Phase 5)
    #[arg(long)]
    payment_amount: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("beneficiary=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    // 1. Connect to registry and fetch CRS (Phase 1)
    info!("Connecting to registry at {}", args.registry_server);
    let mut registry_client = RegistryClient::connect(args.registry_server.clone()).await?;

    let crs_res = registry_client
        .get_crs(GetCrsRequest {
            set_id: args.set_id,
        })
        .await?
        .into_inner();
    let crs = Crs::from_bytes(&crs_res.crs_bytes)?;
    info!("Fetched CRS for set {}", args.set_id);

    // 2. Create beneficiary credential locally (Phase 1)
    let mut beneficiary = Beneficiary::new(&crs, &args.name);
    info!(
        "Created credential for '{}', phi: {:?}",
        args.name,
        &beneficiary.credential.phi.0[..4]
    );

    // 3. Register phi with the registry (Phase 2)
    let ben_res = registry_client
        .register_beneficiary(BeneficiaryRequest {
            set_id: args.set_id,
            phi: beneficiary.credential.phi.0.to_vec(),
            name: args.name.clone(),
            email: String::new(),
            phone: String::new(),
        })
        .await?
        .into_inner();
    info!(
        "Registered with registry at index {}: {}",
        ben_res.index, ben_res.message
    );

    // 4. Subscribe and wait for finalized anonymity set (Phase 2)
    info!("Subscribing to set {} finalization...", args.set_id);
    let response = registry_client
        .subscribe_set_finalization(GetAnonymitySetRequest {
            set_id: args.set_id,
        })
        .await?;
    let mut stream = response.into_inner();

    let anon_res = stream
        .message()
        .await?
        .ok_or("Finalization stream closed without sending data")?;

    let anonymity_set: Vec<Commitment> = anon_res
        .commitments
        .into_iter()
        .map(|bytes| {
            let arr: [u8; 33] = bytes.try_into().expect("commitment must be 33 bytes");
            Commitment(arr)
        })
        .collect();
    info!(
        "Set {} finalized: {} members",
        args.set_id,
        anonymity_set.len()
    );

    // 5. Register with the anonymity set locally (Phase 2 complete)
    beneficiary.register(args.set_id, anonymity_set.clone())?;
    info!(
        "Registered locally at index {}",
        beneficiary.index.unwrap()
    );

    // 6. Fetch VTxO tree (Phase 2)
    let vtxo_res = registry_client
        .get_vtxo_tree(GetVtxoTreeRequest {
            set_id: args.set_id,
        })
        .await?
        .into_inner();
    info!(
        "Downloaded VTxO tree: root_tx={} bytes, fanout_tx={} bytes",
        vtxo_res.root_tx.len(),
        vtxo_res.fanout_tx.len()
    );

    // 7. Optionally connect to merchant for Phase 3-5
    if let (Some(merchant_addr), Some(merchant_id)) = (&args.merchant_server, args.merchant_id) {
        info!("Connecting to merchant at {}", merchant_addr);
        let mut merchant_client = MerchantServiceClient::connect(merchant_addr.clone()).await?;

        // Resolve merchant name: CLI override or fetch from registry
        let merchant_name = if let Some(name) = &args.merchant_name {
            name.clone()
        } else {
            let merchants_res = registry_client
                .get_merchants(GetMerchantsRequest {})
                .await?
                .into_inner();
            let idx = merchant_id as usize;
            merchants_res
                .merchants
                .get(idx - 1) // merchant_id is 1-indexed
                .ok_or_else(|| format!("Merchant ID {} not found in registry", merchant_id))?
                .name
                .clone()
        };
        info!("Resolved merchant name: '{}'", merchant_name);

        // Phase 3-4: Payment identity registration
        let payment_reg = beneficiary
            .create_payment_registration(&crs, merchant_id as usize)
            .map_err(|e| format!("Failed to create payment registration: {}", e))?;

        let proof_bytes = serialize_payment_identity_registration_proof(&payment_reg.proof);

        let reg_res = merchant_client
            .submit_payment_registration(PaymentRegistrationRequest {
                pseudonym: payment_reg.pseudonym.to_vec(),
                public_nullifier: payment_reg.public_nullifier.to_vec(),
                set_id: payment_reg.set_id,
                service_index: payment_reg.service_index as u32,
                friendly_name: payment_reg.friendly_name.clone(),
                proof: proof_bytes,
            })
            .await?
            .into_inner();

        info!(
            "Payment registration accepted by merchant: {}",
            reg_res.message
        );

        // Phase 5: Payment request (optional, triggered by --payment-amount)
        if let Some(amount) = args.payment_amount {
            let merchant_name_typed = Name(merchant_name.clone());
            let payment_request = create_payment_request(
                &beneficiary.credential.r,
                &merchant_name_typed,
                &crs.g,
                amount,
            );

            let req_res = merchant_client
                .submit_payment_request(PaymentRequestMsg {
                    amount,
                    pseudonym: payment_request.pseudonym.to_vec(),
                    proof_r: payment_request.proof.r.to_vec(),
                    proof_s: payment_request.proof.s.to_vec(),
                })
                .await?
                .into_inner();

            info!(
                "Payment request accepted: address={}, name={}",
                req_res.address, req_res.friendly_name
            );
        }
    }

    Ok(())
}
