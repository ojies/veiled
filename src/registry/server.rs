use std::sync::Arc;
use tokio::sync::RwLock;
use axum::{routing::{get, post}, Router};
use tower_http::trace::TraceLayer;

use crate::core::crs::Crs;
use crate::core::verifier::VerifierState;
use crate::registry::{
    db::Db,
    routes::{
        has::has,
        register::{register, register_identity},
        sets::{get_set, list_sets},
        verify::verify,
        verify_registration::verify_registration,
    },
    store::RegistryStore,
};

/// Shared application state threaded through every handler.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<RwLock<RegistryStore>>,
    pub db: Arc<Db>,
    /// CRS — required for Phase 4 verifier mode.
    pub crs: Option<Arc<Crs>>,
    /// Phase 4 verifier state — present when this registry acts as a verifier.
    pub verifier_state: Option<Arc<RwLock<VerifierState>>>,
}

impl AppState {
    /// Create state without verifier (backward-compatible).
    pub fn new(store: RegistryStore, db: Db) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
            db: Arc::new(db),
            crs: None,
            verifier_state: None,
        }
    }

    /// Create state with a Phase 4 verifier configured.
    pub fn with_verifier(store: RegistryStore, db: Db, crs: Crs, user_index: usize) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
            db: Arc::new(db),
            crs: Some(Arc::new(crs)),
            verifier_state: Some(Arc::new(RwLock::new(VerifierState::new(user_index)))),
        }
    }
}

/// Build the axum router with all routes mounted under `/api/v1`.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/register",               post(register))
        .route("/api/v1/register-identity",      post(register_identity))
        .route("/api/v1/has",                    post(has))
        .route("/api/v1/sets",                   get(list_sets))
        .route("/api/v1/sets/:id",               get(get_set))
        .route("/api/v1/verify",                 post(verify))
        .route("/api/v1/verify-registration",    post(verify_registration))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
