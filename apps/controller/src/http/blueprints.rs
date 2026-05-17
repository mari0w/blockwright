use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::{domain::types::Blueprint, state::AppState};

#[derive(Debug, Serialize)]
pub struct ListBlueprintsResponse {
    pub items: Vec<Blueprint>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/blueprints", get(list_blueprints).post(save_blueprint))
        .route("/blueprints/{id}", get(get_blueprint))
}

async fn list_blueprints(State(state): State<AppState>) -> Json<ListBlueprintsResponse> {
    Json(ListBlueprintsResponse {
        items: state.blueprints.list().await,
    })
}

async fn get_blueprint(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Blueprint>, (StatusCode, String)> {
    state
        .blueprints
        .get(&id)
        .await
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("blueprint not found: {id}")))
}

async fn save_blueprint(
    State(state): State<AppState>,
    Json(blueprint): Json<Blueprint>,
) -> Result<Json<Blueprint>, (StatusCode, String)> {
    state
        .blueprints
        .save(blueprint)
        .await
        .map(Json)
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))
}
