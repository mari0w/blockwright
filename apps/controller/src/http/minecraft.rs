use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::types::{GameAction, GameJob, JobResultRequest, PlayerPosition},
    services::planner::PlannerInput,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct MinecraftMessageRequest {
    pub server_id: String,
    pub player: String,
    pub text: String,
    pub position: Option<PlayerPosition>,
}

#[derive(Debug, Serialize)]
pub struct MinecraftMessageResponse {
    pub reply: String,
    pub actions: Vec<GameAction>,
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
        .route("/minecraft/jobs/next", get(next_job))
        .route("/minecraft/jobs/{job_id}/result", post(job_result))
}

async fn handle_message(
    State(state): State<AppState>,
    Json(request): Json<MinecraftMessageRequest>,
) -> Result<Json<MinecraftMessageResponse>, (StatusCode, String)> {
    let plan = state
        .planner
        .plan(
            PlannerInput {
                text: request.text,
                player: Some(request.player),
                position: request.position,
            },
            &state.blueprints,
        )
        .await;

    tracing::info!(
        server_id = %request.server_id,
        summary = %plan.summary,
        action_count = plan.actions.len(),
        "planned minecraft message"
    );

    Ok(Json(MinecraftMessageResponse {
        reply: plan.reply,
        actions: plan.actions,
    }))
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
    Path(job_id): Path<String>,
    Json(request): Json<JobResultRequest>,
) -> Json<JobResultResponse> {
    tracing::info!(
        job_id = %job_id,
        ok = request.ok,
        message = ?request.message,
        "minecraft job result"
    );
    Json(JobResultResponse { ok: true })
}
