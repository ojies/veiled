use bdk_bitcoind_rpc::bitcoincore_rpc::{Auth, Client};
use clap::Parser;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};
use veiled::registry::pb::registry_server::RegistryServer;
use veiled::registry::service::RegistryService;
use veiled::registry::store::{FeeConfig, RegistryStore};

#[derive(Parser)]
#[command(name = "veiled-registry-grpc", about = "Veiled Registry gRPC server")]
struct Args {
    /// Address to listen on
    #[arg(short, long, default_value = "[::1]:50051")]
    listen: String,

    /// Bitcoin RPC URL
    #[arg(long, env = "BITCOIN_RPC_URL", default_value = "http://localhost:18443")]
    rpc_url: String,

    /// Bitcoin RPC username
    #[arg(long, env = "BITCOIN_RPC_USER", default_value = "veiled")]
    rpc_user: String,

    /// Bitcoin RPC password
    #[arg(long, env = "BITCOIN_RPC_PASS", default_value = "veiled")]
    rpc_pass: String,

    /// Minimum sats-per-user when creating a set (beneficiary registration fee)
    #[arg(long, env = "BENEFICIARY_REGISTRATION_FEE", default_value = "2000")]
    beneficiary_fee: u64,

    /// Merchant registration fee in sats
    #[arg(long, env = "MERCHANT_REGISTRATION_FEE", default_value = "3000")]
    merchant_fee: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("veiled_registry_grpc=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();
    let addr = args.listen.parse()?;

    let rpc_client = Client::new(
        &args.rpc_url,
        Auth::UserPass(args.rpc_user.clone(), args.rpc_pass.clone()),
    )
    .map_err(|e| format!("Failed to connect to bitcoind: {e}"))?;

    info!("Connected to bitcoind at {}", args.rpc_url);

    let fee_config = FeeConfig {
        min_sats_per_user: args.beneficiary_fee,
        merchant_registration_fee: args.merchant_fee,
    };
    info!(
        "Fee config: beneficiary={} sats, merchant={} sats",
        fee_config.min_sats_per_user, fee_config.merchant_registration_fee
    );

    let store = Arc::new(Mutex::new(RegistryStore::new(
        Some(Arc::new(rpc_client)),
        fee_config,
    )));
    let registry_service = RegistryService::new(store);

    info!("Veiled gRPC Registry listening on {}", addr);

    Server::builder()
        .add_service(RegistryServer::new(registry_service))
        .serve(addr)
        .await?;

    Ok(())
}
