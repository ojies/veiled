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
use veiled::core::types::Commitment;
use veiled::registry::pb::registry_client::RegistryClient;
use veiled::registry::pb::{GetAnonymitySetRequest, GetCrsRequest, MerchantRequest};

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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("merchant=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    // 1. Register with the registry
    info!("Connecting to registry at {}", args.registry_server);
    let mut registry_client = RegistryClient::connect(args.registry_server.clone()).await?;

    let reg_res = registry_client
        .register_merchant(MerchantRequest {
            name: args.name.clone(),
            origin: args.origin.clone(),
            email: String::new(),
            phone: String::new(),
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
        .map(|bytes| {
            let arr: [u8; 33] = bytes.try_into().expect("commitment must be 33 bytes");
            Commitment(arr)
        })
        .collect();
    info!(
        "Set {} finalized: {} beneficiaries",
        args.set_id,
        anonymity_set.len()
    );

    // 4. Create the core Merchant and start gRPC server
    let merchant = Merchant::new(&args.name, &args.origin);
    let merchant_service = MerchantGrpcService::new(merchant, crs, anonymity_set);

    let addr = args.listen.parse()?;
    info!("Merchant '{}' listening on {}", args.name, addr);

    Server::builder()
        .add_service(MerchantServiceServer::new(merchant_service))
        .serve(addr)
        .await?;

    Ok(())
}
