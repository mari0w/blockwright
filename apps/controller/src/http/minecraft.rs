use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::types::{
        ChatAttachment, GameAction, GameJob, JobResultRequest, PlayerPosition, PlayerState,
        WorldScan,
    },
    services::planner::PlannerInput,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct MinecraftMessageRequest {
    pub server_id: String,
    pub player: String,
    pub text: String,
    pub position: Option<PlayerPosition>,
    #[serde(default)]
    pub player_state: Option<PlayerState>,
    #[serde(default)]
    pub nearby_scan: Option<WorldScan>,
    #[serde(default)]
    pub attachments: Vec<ChatAttachment>,
    #[serde(default)]
    pub progress_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MinecraftMessageResponse {
    pub reply: String,
    pub actions: Vec<GameAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NextJobQuery {
    pub server_id: String,
}

#[derive(Debug, Serialize)]
pub struct NextJobResponse {
    pub job: Option<GameJob>,
}

#[derive(Debug, Serialize)]
pub struct JobResultResponse {
    pub ok: bool,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/minecraft/message", post(handle_message))
        .route("/minecraft/progress/{progress_id}", get(progress))
        .route("/minecraft/jobs/next", get(next_job))
        .route("/minecraft/jobs/{job_id}/result", post(job_result))
}

async fn handle_message(
    State(state): State<AppState>,
    Json(request): Json<MinecraftMessageRequest>,
) -> Result<Json<MinecraftMessageResponse>, (StatusCode, String)> {
    tracing::info!(
        server_id = %request.server_id,
        player = %request.player,
        text = %request.text,
        has_nearby_scan = request.nearby_scan.is_some(),
        "received minecraft message"
    );
    if let Some(progress_id) = request.progress_id.as_deref() {
        state.progress.start(
            progress_id,
            "Blockwright 已收到请求，正在交给 AI 助手",
            None,
        );
    }

    let planner_input = PlannerInput {
        text: request.text.clone(),
        player: Some(request.player.clone()),
        codex_session_key: Some(format!("minecraft:{}", request.player)),
        position: request.position.clone(),
        player_state: request.player_state.clone(),
        nearby_scan: request.nearby_scan.clone(),
        attachments: request.attachments.clone(),
        progress_id: request.progress_id.clone(),
    };
    let plan = state
        .planner
        .plan_with_context_stores(planner_input, &state.blueprints, Some(&state.builds))
        .await;

    let job_id = if has_build_action(&plan.actions) {
        let job_id = state.jobs.reserve_job_id();
        state
            .builds
            .register_planned(
                job_id.clone(),
                request.server_id.clone(),
                Some(request.player.clone()),
                plan.summary.clone(),
                &plan.actions,
            )
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "failed to register planned minecraft build");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "构建记录保存失败，已取消发送建筑任务。".to_string(),
                )
            })?;
        Some(job_id)
    } else {
        None
    };

    tracing::info!(
        server_id = %request.server_id,
        summary = %plan.summary,
        action_count = plan.actions.len(),
        "planned minecraft message"
    );

    if let Some(progress_id) = request.progress_id.as_deref() {
        state
            .progress
            .finish(progress_id, "AI 助手已生成回复，准备返回 Minecraft", None);
    }

    Ok(Json(MinecraftMessageResponse {
        reply: plan.reply,
        actions: plan.actions,
        job_id,
    }))
}

async fn progress(
    State(state): State<AppState>,
    Path(progress_id): Path<String>,
) -> Result<Json<crate::services::progress::ProgressSnapshot>, StatusCode> {
    state
        .progress
        .get(&progress_id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn next_job(
    State(state): State<AppState>,
    Query(query): Query<NextJobQuery>,
) -> Json<NextJobResponse> {
    Json(NextJobResponse {
        job: state.jobs.pop_next(&query.server_id).await,
    })
}

async fn job_result(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(request): Json<JobResultRequest>,
) -> Result<Json<JobResultResponse>, (StatusCode, String)> {
    tracing::info!(
        job_id = %job_id,
        ok = request.ok,
        message = ?request.message,
        "minecraft job result"
    );

    let updated = state.builds.apply_result(&job_id, &request).await.map_err(|error| {
        tracing::error!(job_id = %job_id, error = %error, "failed to save minecraft job result");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "任务结果保存失败。".to_string(),
        )
    })?;
    if updated.is_none() {
        tracing::debug!(job_id = %job_id, "minecraft job result has no matching build record");
    }
    state.jobs.mark_job_result(&job_id, request).await;

    Ok(Json(JobResultResponse { ok: true }))
}

fn has_build_action(actions: &[GameAction]) -> bool {
    actions
        .iter()
        .any(|action| matches!(action, GameAction::PlaceBlocks { .. }))
}
