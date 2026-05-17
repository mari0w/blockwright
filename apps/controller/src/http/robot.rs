use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

use crate::{
    domain::types::{ChatAttachment, GameJob, PlayerPosition},
    services::chat::IncomingChatMessage,
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
    #[serde(default)]
    pub attachments: Vec<ChatAttachment>,
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
    Json(
        queue_chat_message(
            &state,
            IncomingChatMessage {
                platform: request.platform,
                conversation_id: request.conversation_id,
                sender: request.sender,
                server_id: request.server_id,
                target_player: request.target_player,
                text: request.text,
                position: request.position,
                attachments: request.attachments,
            },
        )
        .await,
    )
}

pub(crate) async fn queue_chat_message(
    state: &AppState,
    message: IncomingChatMessage,
) -> RobotMessageResponse {
    let target_player = message.target_player.clone();
    let plan = state
        .planner
        .plan(
            PlannerInput {
                text: message.text,
                player: target_player.clone(),
                position: message.position,
                attachments: message.attachments,
            },
            &state.blueprints,
        )
        .await;

    let server_id = message
        .server_id
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let queued_job = if plan.actions.is_empty() {
        None
    } else {
        let job_id = state.jobs.reserve_job_id();
        if has_build_action(&plan.actions) {
            if let Err(error) = state
                .builds
                .register_planned(
                    job_id.clone(),
                    server_id.clone(),
                    target_player.clone(),
                    plan.summary.clone(),
                    &plan.actions,
                )
                .await
            {
                tracing::error!(error = %error, "failed to register planned robot build");
                return RobotMessageResponse {
                    reply: "构建记录保存失败，已取消下发建筑任务。".to_string(),
                    queued_job: None,
                };
            }
        }

        Some(
            state
                .jobs
                .enqueue_with_id(
                    job_id,
                    server_id,
                    target_player,
                    plan.summary.clone(),
                    plan.actions,
                )
                .await,
        )
    };

    tracing::info!(
        platform = %message.platform,
        conversation_id = %message.conversation_id,
        sender = %message.sender,
        queued = queued_job.is_some(),
        "handled robot message"
    );

    RobotMessageResponse {
        reply: plan.reply,
        queued_job,
    }
}

fn has_build_action(actions: &[crate::domain::types::GameAction]) -> bool {
    actions
        .iter()
        .any(|action| matches!(action, crate::domain::types::GameAction::PlaceBlocks { .. }))
}
