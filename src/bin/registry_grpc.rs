use clap::Parser;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};
use veiled::registry::pb::registry_server::RegistryServer;
use veiled::registry::service::RegistryService;
use veiled::registry::store::RegistryStore;

#[derive(Parser)]
#[command(name = "veiled-registry-grpc", about = "Veiled Registry gRPC server")]
struct Args {
    /// Address to listen on
    #[arg(short, long, default_value = "[::1]:50051")]
    listen: String,
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

    let store = Arc::new(Mutex::new(RegistryStore::new()));
    let registry_service = RegistryService::new(store);

    info!("Veiled gRPC Registry listening on {}", addr);

    Server::builder()
        .add_service(RegistryServer::new(registry_service))
        .serve(addr)
        .await?;

    Ok(())
}
