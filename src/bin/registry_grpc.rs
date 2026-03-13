use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};
use veiled::registry::pb::registry_server::RegistryServer;
use veiled::registry::service::RegistryService;
use veiled::registry::store::RegistryStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("veiled_registry_grpc=info".parse().unwrap()),
        )
        .init();

    let addr = "[::1]:50051".parse()?;
    let beneficiary_capacity = 1024;
    let merchant_capacity = 3;

    let store = Arc::new(Mutex::new(RegistryStore::new(
        beneficiary_capacity,
        merchant_capacity,
    )));
    let registry_service = RegistryService::new(store);

    info!("Veiled gRPC Registry listening on {}", addr);

    Server::builder()
        .add_service(RegistryServer::new(registry_service))
        .serve(addr)
        .await?;

    Ok(())
}
