use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::types::{GameAction, GameJob, JobResultRequest, PlayerPosition, WorldScan},
    services::build_store::BuildMatch,
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
    pub nearby_scan: Option<WorldScan>,
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

    if let Some(response) = handle_existing_build_modification(&state, &request).await {
        return response.map(Json);
    }

    let plan = state
        .planner
        .plan(
            PlannerInput {
                text: request.text,
                player: Some(request.player.clone()),
                codex_session_key: Some(format!("minecraft:{}", request.player)),
                position: request.position,
                nearby_scan: request.nearby_scan.clone(),
                attachments: Vec::new(),
            },
            &state.blueprints,
        )
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
                    "构建记录保存失败，已取消下发建筑动作。".to_string(),
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

    Ok(Json(MinecraftMessageResponse {
        reply: plan.reply,
        actions: plan.actions,
        job_id,
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

    Ok(Json(JobResultResponse { ok: true }))
}

fn has_build_action(actions: &[GameAction]) -> bool {
    actions
        .iter()
        .any(|action| matches!(action, GameAction::PlaceBlocks { .. }))
}

async fn handle_existing_build_modification(
    state: &AppState,
    request: &MinecraftMessageRequest,
) -> Option<Result<MinecraftMessageResponse, (StatusCode, String)>> {
    if !wants_existing_build_modification(&request.text) {
        return None;
    }

    let Some(scan) = request.nearby_scan.as_ref() else {
        return Some(Ok(chat_response(
            "这个需求需要先扫描你附近的建筑。请在游戏内站到目标建筑前面重新执行同一句 `/bw ...`，HMCL/Fabric 模组会自动带上附近方块信息。"
                .to_string(),
        )));
    };

    let matches = state.builds.match_scan(&request.server_id, scan).await;
    let useful_matches = matches
        .into_iter()
        .filter(|item| item.matched_blocks >= 3 || item.score >= 0.2)
        .collect::<Vec<_>>();
    if useful_matches.is_empty() {
        return Some(Ok(chat_response(
            "我扫描了附近方块，但没有匹配到 Blockwright 已记录的建筑。请先确认这个建筑是通过 Blockwright 生成的，或者先保存/登记蓝图后再改造。"
                .to_string(),
        )));
    }

    let best = &useful_matches[0];
    if is_ambiguous_match(best, useful_matches.get(1)) {
        return Some(Ok(chat_response(format!(
            "附近匹配到多个可能的建筑，请指定要改哪一个：{}。",
            useful_matches
                .iter()
                .take(3)
                .map(|item| format!("{}（匹配 {} 个方块）", item.record.id, item.matched_blocks))
                .collect::<Vec<_>>()
                .join("、")
        ))));
    }

    match planned_window_replacement(&request.text, best) {
        Ok(Some(action)) => {
            let job_id = state.jobs.reserve_job_id();
            let summary = format!("改造构建 {} 的窗户", best.record.id);
            if let Err(error) = state
                .builds
                .register_planned(
                    job_id.clone(),
                    request.server_id.clone(),
                    Some(request.player.clone()),
                    summary,
                    &[action.clone()],
                )
                .await
            {
                tracing::error!(error = %error, "failed to register planned modification build");
                return Some(Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "构建记录保存失败，已取消下发改造动作。".to_string(),
                )));
            }

            Some(Ok(MinecraftMessageResponse {
                reply: format!(
                    "已匹配到建筑 `{}`，这次只改造识别到的窗户方块。完成后会逐块校验并写回构建记录。",
                    best.record.id
                ),
                actions: vec![action],
                job_id: Some(job_id),
            }))
        }
        Ok(None) => Some(Ok(chat_response(
            "我识别到了目标建筑，但没有找到可替换的窗户/玻璃方块。请说得更具体一点，例如“把一楼正面的窗户换成蓝色玻璃”。"
                .to_string(),
        ))),
        Err(message) => Some(Ok(chat_response(message))),
    }
}

fn wants_existing_build_modification(text: &str) -> bool {
    let lower = text.to_lowercase();
    (text.contains("改")
        || text.contains("换")
        || text.contains("调整")
        || text.contains("替换")
        || lower.contains("replace")
        || lower.contains("modify"))
        && (text.contains("面前")
            || text.contains("附近")
            || text.contains("这个")
            || text.contains("那栋")
            || text.contains("房子")
            || text.contains("建筑")
            || lower.contains("nearby")
            || lower.contains("this"))
}

fn is_ambiguous_match(best: &BuildMatch, second: Option<&BuildMatch>) -> bool {
    let Some(second) = second else {
        return false;
    };
    second.matched_blocks + 2 >= best.matched_blocks && second.score >= best.score * 0.8
}

fn planned_window_replacement(
    text: &str,
    candidate: &BuildMatch,
) -> Result<Option<GameAction>, String> {
    if !(text.contains("窗") || text.contains("玻璃")) {
        return Err("我已经匹配到附近建筑，但还不知道要改哪个部位。请补充目标，例如“把正面的窗户换成蓝色玻璃”。".to_string());
    }

    let Some(target_material) = replacement_glass_material(text) else {
        return Err("我知道你要改窗户/玻璃，但还不确定要换成哪种玻璃。请明确说“换成蓝色玻璃/红色玻璃/普通玻璃/玻璃板”。".to_string());
    };

    let action = &candidate.record.expected_actions[candidate.action_index];
    let mut blocks = action
        .blocks
        .iter()
        .filter(|block| block.material.contains("glass"))
        .cloned()
        .collect::<Vec<_>>();

    if text.contains("二楼") || text.contains("2楼") {
        let min_y = action.blocks.iter().map(|block| block.y).min().unwrap_or(0);
        let max_y = action.blocks.iter().map(|block| block.y).max().unwrap_or(0);
        if max_y - min_y < 4 {
            return Err("我匹配到这个建筑，但从已保存蓝图看不出明确的二楼窗户。请指定更明确的位置，例如“正面左边窗户”。".to_string());
        }
        let split_y = min_y + (max_y - min_y) / 2 + 1;
        blocks.retain(|block| block.y >= split_y);
    }

    if blocks.is_empty() {
        return Ok(None);
    }
    for block in &mut blocks {
        block.material = target_material.clone();
    }

    Ok(Some(GameAction::PlaceBlocks {
        blueprint_id: Some(format!("{}:window-modification", candidate.record.id)),
        origin: action.origin.clone(),
        blocks,
        clear_existing: false,
    }))
}

fn replacement_glass_material(text: &str) -> Option<String> {
    let material = if text.contains("玻璃板") {
        "minecraft:glass_pane"
    } else if text.contains("蓝") {
        "minecraft:blue_stained_glass"
    } else if text.contains("红") {
        "minecraft:red_stained_glass"
    } else if text.contains("绿") {
        "minecraft:green_stained_glass"
    } else if text.contains("黄") {
        "minecraft:yellow_stained_glass"
    } else if text.contains("黑") {
        "minecraft:black_stained_glass"
    } else if text.contains("白") || text.contains("普通") || text.contains("透明") {
        "minecraft:glass"
    } else {
        return None;
    };
    Some(material.to_string())
}

fn chat_response(message: String) -> MinecraftMessageResponse {
    MinecraftMessageResponse {
        reply: message.clone(),
        actions: vec![GameAction::Chat { message }],
        job_id: None,
    }
}
