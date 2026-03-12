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

    // If VEILED_USER_INDEX is set, configure Phase 4 verifier mode.
    // Also requires VEILED_CRS_PROVIDERS (comma-separated list of user names).
    let state = match (std::env::var("VEILED_USER_INDEX"), std::env::var("VEILED_CRS_PROVIDERS")) {
        (Ok(idx_str), Ok(providers_str)) => {
            let user_index: usize = idx_str.parse()
                .expect("VEILED_USER_INDEX must be a positive integer (1-indexed)");
            assert!(user_index >= 1, "VEILED_USER_INDEX must be >= 1");

            let providers: Vec<veiled::core::crs::Merchant> = providers_str
                .split(',')
                .map(|name| veiled::core::crs::Merchant {
                    name: veiled::core::types::Name::new(name.trim()),
                    credential_generator: [0x02; 33],
                    origin: String::new(),
                })
                .collect();

            let crs = veiled::core::crs::Crs::setup(providers);
            info!("verifier mode: user_index={user_index}, CRS with {} providers", crs.num_merchants());
            AppState::with_verifier(store, db, crs, user_index)
        }
        (Ok(_), Err(_)) => {
            panic!("VEILED_USER_INDEX set but VEILED_CRS_PROVIDERS missing");
        }
        _ => {
            info!("registry-only mode (no verifier)");
            AppState::new(store, db)
        }
    };

    let router = build_router(state);

    let port = std::env::var("VEILED_PORT").unwrap_or_else(|_| "7271".to_string());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    info!("veiled registry listening on {addr}");
    info!("anonymity set capacity: {DEFAULT_SET_CAPACITY}");

    axum::serve(listener, router).await.unwrap();
}
