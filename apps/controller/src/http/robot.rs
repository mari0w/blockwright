use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

use crate::{
    domain::types::{GameJob, PlayerPosition},
    services::planner::PlannerInput,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct RobotMessageRequest {
    pub platform: String,
    pub conversation_id: String,
    pub sender: String,
    pub server_id: Option<String>,
    pub target_player: Option<String>,
    pub text: String,
    pub position: Option<PlayerPosition>,
}

#[derive(Debug, Serialize)]
pub struct RobotMessageResponse {
    pub reply: String,
    pub queued_job: Option<GameJob>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/robot/message", post(handle_message))
}

async fn handle_message(
    State(state): State<AppState>,
    Json(request): Json<RobotMessageRequest>,
) -> Json<RobotMessageResponse> {
    let target_player = request.target_player.clone();
    let plan = state
        .planner
        .plan(
            PlannerInput {
                text: request.text,
                player: target_player.clone(),
                position: request.position,
            },
            &state.blueprints,
        )
        .await;

    let server_id = request
        .server_id
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let queued_job = if plan.actions.is_empty() {
        None
    } else {
        Some(
            state
                .jobs
                .enqueue(server_id, target_player, plan.summary.clone(), plan.actions)
                .await,
        )
    };

    tracing::info!(
        platform = %request.platform,
        conversation_id = %request.conversation_id,
        sender = %request.sender,
        queued = queued_job.is_some(),
        "handled robot message"
    );

    Json(RobotMessageResponse {
        reply: plan.reply,
        queued_job,
    })
}
