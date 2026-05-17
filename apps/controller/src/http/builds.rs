use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};

use crate::{domain::types::BuildRecord, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/builds/{build_id}", get(get_build))
}

async fn get_build(
    State(state): State<AppState>,
    Path(build_id): Path<String>,
) -> Result<Json<BuildRecord>, StatusCode> {
    state
        .builds
        .get(&build_id)
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}
