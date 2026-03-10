use std::sync::Arc;
use tokio::sync::RwLock;
use axum::{routing::{get, post}, Router};
use tower_http::trace::TraceLayer;

use crate::{
    db::Db,
    routes::{
        has::has,
        register::{register, register_identity},
        sets::{get_set, list_sets},
        verify::verify,
    },
    store::RegistryStore,
};

/// Shared application state threaded through every handler.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<RwLock<RegistryStore>>,
    pub db: Arc<Db>,
}

impl AppState {
    pub fn new(store: RegistryStore, db: Db) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
            db: Arc::new(db),
        }
    }
}

/// Build the axum router with all routes mounted under `/api/v1`.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/register",          post(register))
        .route("/api/v1/register-identity", post(register_identity))
        .route("/api/v1/has",               post(has))
        .route("/api/v1/sets",              get(list_sets))
        .route("/api/v1/sets/:id",          get(get_set))
        .route("/api/v1/verify",            post(verify))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
