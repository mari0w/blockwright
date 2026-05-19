use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};

use crate::{mcp, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/mcp", post(handle_mcp))
}

async fn handle_mcp(State(state): State<AppState>, Json(request): Json<Value>) -> Json<Value> {
    Json(
        mcp::handle_json_rpc_value(&state, request)
            .await
            .unwrap_or_else(|| json!({ "jsonrpc": "2.0", "id": null, "result": null })),
    )
}
