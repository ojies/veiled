use veiled::registry::db::Db;
use veiled::registry::server::{AppState, build_router};
use veiled::registry::store::DEFAULT_SET_CAPACITY;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() {
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("veiled_registry=info".parse().unwrap()))
        .init();

    let db_path = std::env::var("VEILED_DB").unwrap_or_else(|_| "veiled.db".to_string());
    let db = Db::open(&db_path).expect("failed to open SQLite database");
    let store = db.load_store(DEFAULT_SET_CAPACITY).expect("failed to load store from database");

    info!("database: {db_path}");
    info!("loaded {} set(s), {} nullifier(s)", store.sets.len(), store.nullifiers.len());

    let state = AppState::new(store, db);
    let router = build_router(state);

    let port = std::env::var("VEILED_PORT").unwrap_or_else(|_| "7271".to_string());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    info!("veiled registry listening on {addr}");
    info!("anonymity set capacity: {DEFAULT_SET_CAPACITY}");

    axum::serve(listener, router).await.unwrap();
}
