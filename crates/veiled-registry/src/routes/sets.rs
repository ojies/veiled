use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;

use crate::{error::AppError, server::AppState};

/// `GET /api/v1/sets`
///
/// Returns a summary of every anonymity set.
#[derive(Serialize)]
pub struct SetSummary {
    pub id: u64,
    pub size: usize,
    pub capacity: usize,
    pub full: bool,
}

pub async fn list_sets(State(state): State<AppState>) -> Json<Vec<SetSummary>> {
    let store = state.store.read().await;
    let summaries = store
        .sets
        .iter()
        .map(|s| SetSummary {
            id: s.id,
            size: s.commitments.len(),
            capacity: s.capacity,
            full: s.is_full(),
        })
        .collect();
    Json(summaries)
}

/// `GET /api/v1/sets/:id`
///
/// Returns a full anonymity set including all commitment hex strings.
#[derive(Serialize)]
pub struct SetDetail {
    pub id: u64,
    pub commitments: Vec<String>,
    pub capacity: usize,
    pub full: bool,
}

pub async fn get_set(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<SetDetail>, AppError> {
    let store = state.store.read().await;
    let set = store.get_set(id).ok_or(AppError::SetNotFound(id))?;

    Ok(Json(SetDetail {
        id: set.id,
        commitments: set.commitments.iter().map(|c| c.to_hex()).collect(),
        capacity: set.capacity,
        full: set.is_full(),
    }))
}
