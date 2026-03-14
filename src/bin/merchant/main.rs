mod db;
mod service;

mod pb {
    tonic::include_proto!("merchant");
}

use clap::Parser;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};
use veiled::core::crs::Crs;
use veiled::core::merchant::Merchant;
use veiled::core::payment_identity::{
    deserialize_payment_identity_registration_proof, PaymentIdentityRegistration,
};
use veiled::core::types::Commitment;
use veiled::registry::pb::registry_client::RegistryClient;
use veiled::registry::pb::{
    GetAnonymitySetRequest, GetCrsRequest, GetFeesRequest, GetRegistryAddressRequest,
    MerchantRequest,
};

use pb::merchant_service_server::MerchantServiceServer;
use service::MerchantGrpcService;

#[derive(Parser)]
#[command(name = "merchant", about = "Veiled Merchant: registers with registry then serves beneficiaries")]
struct Args {
    /// Merchant name
    #[arg(short, long)]
    name: String,

    /// Merchant origin URL
    #[arg(short, long)]
    origin: String,

    /// Registry gRPC server address
    #[arg(short, long, default_value = "http://[::1]:50051")]
    registry_server: String,

    /// Address to listen on for beneficiary connections
    #[arg(short, long, default_value = "[::1]:50061")]
    listen: String,

    /// Set ID to serve
    #[arg(short, long)]
    set_id: u64,

    /// Funding transaction ID (hex-encoded, 32 bytes) proving payment to registry
    #[arg(long)]
    funding_txid: Option<String>,

    /// Funding output index within the payment transaction
    #[arg(long, default_value = "0")]
    funding_vout: u32,

    /// Path to SQLite database for persistent identity storage
    #[arg(long, default_value = "merchant.db")]
    db_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("merchant=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    // 1. Connect to registry and query address + fees
    info!("Connecting to registry at {}", args.registry_server);
    let mut registry_client = RegistryClient::connect(args.registry_server.clone()).await?;

    let addr_res = registry_client
        .get_registry_address(GetRegistryAddressRequest { set_id: 0 })
        .await?
        .into_inner();
    let fees_res = registry_client.get_fees(GetFeesRequest {}).await?.into_inner();
    info!("Registry address: {}", addr_res.address);
    info!("Required merchant fee: {} sats", fees_res.merchant_fee);

    // 2. Parse funding outpoint and register
    let funding_txid = match &args.funding_txid {
        Some(hex) => {
            let bytes = hex::decode(hex)
                .map_err(|e| format!("Invalid funding_txid hex: {}", e))?;
            if bytes.len() != 32 {
                return Err("funding_txid must be 32 bytes (64 hex chars)".into());
            }
            bytes
        }
        None => {
            eprintln!(
                "ERROR: No funding transaction provided.\n\
                 Pay {} sats to {} and re-run with:\n  \
                 --funding-txid <txid_hex> --funding-vout <vout>",
                fees_res.merchant_fee, addr_res.address
            );
            std::process::exit(1);
        }
    };

    let reg_res = registry_client
        .register_merchant(MerchantRequest {
            name: args.name.clone(),
            origin: args.origin.clone(),
            email: String::new(),
            phone: String::new(),
            funding_txid,
            funding_vout: args.funding_vout,
        })
        .await?
        .into_inner();
    info!("Registered with registry: {}", reg_res.message);

    // 2. Fetch CRS for the set
    let crs_res = registry_client
        .get_crs(GetCrsRequest {
            set_id: args.set_id,
        })
        .await?
        .into_inner();
    let crs = Crs::from_bytes(&crs_res.crs_bytes)?;
    info!("Fetched CRS for set {}", args.set_id);

    // 3. Subscribe and wait for finalized anonymity set
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
        .ok_or("Finalization stream closed without data")?;

    let anonymity_set: Vec<Commitment> = anon_res
        .commitments
        .into_iter()
        .enumerate()
        .map(|(i, bytes)| {
            let arr: [u8; 33] = bytes
                .try_into()
                .map_err(|_| format!("commitment[{}] is not 33 bytes", i))?;
            Ok(Commitment(arr))
        })
        .collect::<Result<Vec<_>, String>>()?;
    info!(
        "Set {} finalized: {} beneficiaries",
        args.set_id,
        anonymity_set.len()
    );

    // 4. Open database and restore previously registered identities
    let db_conn = db::open_db(&args.db_path)
        .map_err(|e| format!("Failed to open merchant DB: {}", e))?;

    let mut merchant = Merchant::new(&args.name, &args.origin);

    let saved = db::load_identities(&db_conn)
        .map_err(|e| format!("Failed to load identities: {}", e))?;
    let restored = saved.len();
    for row in saved {
        let proof = deserialize_payment_identity_registration_proof(&row.proof_blob)
            .map_err(|e| format!("Failed to deserialize stored proof: {}", e))?;
        let reg = PaymentIdentityRegistration {
            pseudonym: row.pseudonym,
            public_nullifier: row.public_nullifier,
            set_id: row.set_id,
            service_index: row.service_index,
            friendly_name: row.friendly_name,
            proof,
        };
        merchant
            .registered_identities
            .insert(row.pseudonym, reg);
    }
    if restored > 0 {
        info!("Restored {} registered identities from DB", restored);
    }

    // 5. Start gRPC server
    let merchant_service = MerchantGrpcService::new(merchant, crs, anonymity_set, Some(db_conn));

    let addr = args.listen.parse()?;
    info!("Merchant '{}' listening on {}", args.name, addr);

    Server::builder()
        .add_service(MerchantServiceServer::new(merchant_service))
        .serve(addr)
        .await?;

    Ok(())
}
