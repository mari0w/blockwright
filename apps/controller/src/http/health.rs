use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::state::AppState;

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
    service: String,
    server_name: String,
    environment: String,
    codex_enabled: bool,
    codex_timeout_seconds: u64,
    llm_enabled: bool,
    llm_provider: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

pub fn api_router() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: state.config.server.app_name.clone(),
        server_name: state.config.server.name.clone(),
        environment: state.config.server.environment.clone(),
        codex_enabled: state.codex.enabled(),
        codex_timeout_seconds: state.config.codex.timeout_seconds,
        llm_enabled: state.llm.enabled(),
        llm_provider: state.llm.provider().label().to_string(),
    })
}
