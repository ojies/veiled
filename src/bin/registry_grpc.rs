use bdk_bitcoind_rpc::bitcoincore_rpc::{Auth, Client};
use clap::Parser;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};
use veiled::registry::pb::registry_server::RegistryServer;
use veiled::registry::service::RegistryService;
use veiled::registry::db;
use veiled::registry::service::Config;
use veiled::registry::store::RegistryStore;

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
    #[arg(long, env = "BENEFICIARY_REGISTRATION_FEE", default_value = "1000")]
    beneficiary_fee: u64,

    /// Merchant registration fee in sats
    #[arg(long, env = "MERCHANT_REGISTRATION_FEE", default_value = "3000")]
    merchant_fee: u64,

    /// Minimum merchants required before auto-creating a set
    #[arg(long, env = "MIN_MERCHANTS", default_value = "2")]
    min_merchants: usize,

    /// Beneficiary capacity per set (must be <= N from ZK proof)
    #[arg(long, env = "BENEFICIARY_CAPACITY", default_value = "4")]
    beneficiary_capacity: usize,

    /// SQLite database path for persistent state
    #[arg(long, env = "REGISTRY_DB_PATH", default_value = "registry.db")]
    db_path: String,
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

    let config = Config {
        min_sats_per_user: args.beneficiary_fee,
        merchant_registration_fee: args.merchant_fee,
        beneficiary_capacity: args.beneficiary_capacity,
        merchant_capacity: args.min_merchants,
    };
    info!(
        "Config: beneficiary_fee={} sats, merchant_fee={} sats, capacity={}, min_merchants={}",
        config.min_sats_per_user, config.merchant_registration_fee,
        config.beneficiary_capacity, config.merchant_capacity
    );

    info!("Opening database at {}", args.db_path);
    let conn = db::open_db(&args.db_path)
        .map_err(|e| format!("Failed to open database: {e}"))?;

    let store = RegistryStore::new(Some(Arc::new(rpc_client)), Some(conn));
    info!("Wallet address: {}", store.wallet.address);
    let store = Arc::new(Mutex::new(store));
    let registry_service = RegistryService::new(store, config);

    info!("Veiled gRPC Registry listening on {}", addr);

    Server::builder()
        .add_service(RegistryServer::new(registry_service))
        .serve(addr)
        .await?;

    Ok(())
}
