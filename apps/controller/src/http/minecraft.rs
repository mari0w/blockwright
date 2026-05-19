use std::collections::BTreeMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::types::{
        ChatAttachment, GameAction, GameJob, JobResultRequest, PlayerPosition, WorldScan,
    },
    services::build_store::BuildMatch,
    services::planner::{
        existing_edit_scan_queued_reply, PlannerInput, PlannerIntent, PlannerIntentKind,
    },
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
            "controller 已收到 Minecraft 请求，准备交给 Codex",
            None,
        );
    }

    let planner_input = PlannerInput {
        text: request.text.clone(),
        player: Some(request.player.clone()),
        codex_session_key: Some(format!("minecraft:{}", request.player)),
        position: request.position.clone(),
        nearby_scan: request.nearby_scan.clone(),
        attachments: request.attachments.clone(),
        progress_id: request.progress_id.clone(),
    };
    let intent = state.planner.classify_intent(&planner_input).await;

    if matches!(
        intent.as_ref().map(|item| item.intent),
        Some(PlannerIntentKind::ExistingBuildEdit)
    ) {
        if let Some(response) =
            handle_existing_build_modification(&state, &request, intent.as_ref()).await
        {
            return response.map(|body| {
                if let Some(progress_id) = request.progress_id.as_deref() {
                    state.progress.finish(
                        progress_id,
                        "controller 已生成回复，准备返回 Minecraft",
                        None,
                    );
                }
                Json(body)
            });
        }
    }

    let plan = state
        .planner
        .plan_with_intent(planner_input, &state.blueprints, intent)
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

    if let Some(progress_id) = request.progress_id.as_deref() {
        state.progress.finish(
            progress_id,
            "controller 已生成回复，准备返回 Minecraft",
            None,
        );
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
    state
        .jobs
        .mark_result(&job_id, request.ok, request.message.clone())
        .await;

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
    intent: Option<&PlannerIntent>,
) -> Option<Result<MinecraftMessageResponse, (StatusCode, String)>> {
    let Some(scan) = request.nearby_scan.as_ref() else {
        return Some(Ok(MinecraftMessageResponse {
            reply: existing_edit_scan_queued_reply(intent.map(|item| item.reply.as_str())),
            actions: vec![GameAction::ScanNearbyAndPlan {
                text: request.text.clone(),
                attachments: request.attachments.clone(),
            }],
            job_id: None,
        }));
    };

    let matches = state.builds.match_scan(&request.server_id, scan).await;
    let useful_matches = matches
        .into_iter()
        .filter(|item| item.matched_blocks >= 3 || item.score >= 0.2)
        .collect::<Vec<_>>();
    if useful_matches.is_empty() {
        match adopt_scanned_build(state, request, scan).await {
            Ok(Some(best)) => return Some(plan_existing_build_edit(state, request, &best).await),
            Ok(None) => {
                return Some(Ok(chat_response(
                    "我扫描了附近方块，但没识别到可改造的建筑结构。请站近一点、对准目标建筑再发同一句。"
                        .to_string(),
                )));
            }
            Err(error) => return Some(Err(error)),
        }
    }

    let ranked_matches = rank_build_matches(useful_matches, request.position.as_ref(), scan);
    let best = ranked_matches[0].item.clone();
    if is_ambiguous_match(
        &ranked_matches[0],
        ranked_matches.get(1),
        request.position.as_ref().is_some(),
    ) {
        return Some(Ok(chat_response(format!(
            "附近匹配到多个可能的建筑，请指定要改哪一个：{}。",
            ranked_matches
                .iter()
                .take(3)
                .map(|item| format!(
                    "{}（匹配 {} 个方块）",
                    item.item.record.id, item.total_matched_blocks
                ))
                .collect::<Vec<_>>()
                .join("、")
        ))));
    }

    Some(plan_existing_build_edit(state, request, &best).await)
}

async fn adopt_scanned_build(
    state: &AppState,
    request: &MinecraftMessageRequest,
    scan: &WorldScan,
) -> Result<Option<BuildMatch>, (StatusCode, String)> {
    let build_id = state.jobs.reserve_job_id();
    state
        .builds
        .adopt_scan_as_build(
            build_id,
            request.server_id.clone(),
            Some(request.player.clone()),
            scan,
        )
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "failed to auto-adopt scanned minecraft build");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "附近建筑自动记录失败，已取消本次改造。".to_string(),
            )
        })
}

async fn plan_existing_build_edit(
    state: &AppState,
    request: &MinecraftMessageRequest,
    best: &BuildMatch,
) -> Result<MinecraftMessageResponse, (StatusCode, String)> {
    let planner_input = PlannerInput {
        text: request.text.clone(),
        player: Some(request.player.clone()),
        codex_session_key: Some(format!("minecraft:{}", request.player)),
        position: request.position.clone(),
        nearby_scan: request.nearby_scan.clone(),
        attachments: request.attachments.clone(),
        progress_id: request.progress_id.clone(),
    };

    let Some(plan) = state
        .planner
        .plan_existing_build_edit(&planner_input, &best.record)
        .await
    else {
        return Ok(chat_response(format!(
            "已匹配到目标建筑 `{}`，但 Codex 没有返回有效改造计划，所以没有下发动作。请换一种说法重试。",
            best.record.id
        )));
    };

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
            tracing::error!(error = %error, "failed to register planned replacement build");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "构建记录保存失败，已取消下发改造动作。".to_string(),
            )
        })?;

    Ok(MinecraftMessageResponse {
        reply: plan.reply,
        actions: plan.actions,
        job_id: Some(job_id),
    })
}

#[derive(Debug, Clone)]
struct RankedBuildMatch {
    item: BuildMatch,
    total_matched_blocks: u32,
    score: f32,
    in_front: bool,
    lateral_distance: f64,
    distance: f64,
}

fn rank_build_matches(
    matches: Vec<BuildMatch>,
    position: Option<&PlayerPosition>,
    scan: &WorldScan,
) -> Vec<RankedBuildMatch> {
    let mut grouped = BTreeMap::<String, RankedBuildMatch>::new();
    for item in matches {
        let metrics = spatial_metrics(&item, position, scan);
        grouped
            .entry(item.record.id.clone())
            .and_modify(|existing| {
                existing.total_matched_blocks += item.matched_blocks;
                existing.score = existing.score.max(item.score);
                if prefer_stronger_match(&item, &existing.item) {
                    existing.item = item.clone();
                }
            })
            .or_insert_with(|| RankedBuildMatch {
                item,
                total_matched_blocks: metrics.matched_blocks,
                score: metrics.score,
                in_front: metrics.in_front,
                lateral_distance: metrics.lateral_distance,
                distance: metrics.distance,
            });
    }

    let mut ranked = grouped.into_values().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .in_front
            .cmp(&left.in_front)
            .then_with(|| left.lateral_distance.total_cmp(&right.lateral_distance))
            .then_with(|| left.distance.total_cmp(&right.distance))
            .then_with(|| right.total_matched_blocks.cmp(&left.total_matched_blocks))
            .then_with(|| right.score.total_cmp(&left.score))
            .then_with(|| left.item.record.id.cmp(&right.item.record.id))
    });
    ranked
}

fn is_ambiguous_match(
    best: &RankedBuildMatch,
    second: Option<&RankedBuildMatch>,
    has_player_direction: bool,
) -> bool {
    let Some(second) = second else {
        return false;
    };
    if has_player_direction {
        if best.in_front {
            return false;
        }
        if best.distance + 3.0 < second.distance {
            return false;
        }
    }

    second.total_matched_blocks + 2 >= best.total_matched_blocks && second.score >= best.score * 0.8
}

#[derive(Debug)]
struct SpatialMetrics {
    matched_blocks: u32,
    score: f32,
    in_front: bool,
    lateral_distance: f64,
    distance: f64,
}

fn spatial_metrics(
    item: &BuildMatch,
    position: Option<&PlayerPosition>,
    scan: &WorldScan,
) -> SpatialMetrics {
    let (origin_x, origin_z, forward) = if let Some(position) = position {
        let forward = player_forward_vector(position, scan);
        (position.x, position.z, forward)
    } else {
        (scan.center_x as f64, scan.center_z as f64, None)
    };
    let (in_front, lateral_distance, distance) =
        record_spatial_distances(item, origin_x, origin_z, forward);

    SpatialMetrics {
        matched_blocks: item.matched_blocks,
        score: item.score,
        in_front,
        lateral_distance,
        distance,
    }
}

fn record_spatial_distances(
    item: &BuildMatch,
    origin_x: f64,
    origin_z: f64,
    forward: Option<(f64, f64)>,
) -> (bool, f64, f64) {
    let mut any_block = false;
    let mut any_in_front = false;
    let mut best_lateral_any = f64::MAX;
    let mut best_lateral_front = f64::MAX;
    let mut best_distance_any = f64::MAX;
    let mut best_distance_front = f64::MAX;

    for action in &item.record.expected_actions {
        for block in &action.blocks {
            any_block = true;
            let x = (action.origin.x + block.x) as f64;
            let z = (action.origin.z + block.z) as f64;
            let dx = x - origin_x;
            let dz = z - origin_z;
            let distance = (dx * dx + dz * dz).sqrt();
            best_distance_any = best_distance_any.min(distance);

            let lateral = if let Some((forward_x, forward_z)) = forward {
                let forward_distance = dx * forward_x + dz * forward_z;
                let lateral = (dx * forward_z - dz * forward_x).abs();
                best_lateral_any = best_lateral_any.min(lateral);
                if forward_distance >= -2.0 {
                    any_in_front = true;
                    best_lateral_front = best_lateral_front.min(lateral);
                    best_distance_front = best_distance_front.min(distance);
                }
                lateral
            } else {
                distance
            };
            best_lateral_any = best_lateral_any.min(lateral);
        }
    }

    if !any_block {
        return (true, 0.0, 0.0);
    }
    if forward.is_some() && any_in_front {
        (true, best_lateral_front, best_distance_front)
    } else {
        (forward.is_none(), best_lateral_any, best_distance_any)
    }
}

fn player_forward_vector(position: &PlayerPosition, scan: &WorldScan) -> Option<(f64, f64)> {
    if let Some(yaw) = position.yaw {
        let radians = yaw.to_radians();
        let forward_x = -radians.sin();
        let forward_z = radians.cos();
        let length = (forward_x * forward_x + forward_z * forward_z).sqrt();
        if length > 0.0 {
            return Some((forward_x / length, forward_z / length));
        }
    }

    let forward_x = scan.center_x as f64 - position.x;
    let forward_z = scan.center_z as f64 - position.z;
    let length = (forward_x * forward_x + forward_z * forward_z).sqrt();
    if length > 0.0 {
        Some((forward_x / length, forward_z / length))
    } else {
        None
    }
}

fn prefer_stronger_match(candidate: &BuildMatch, current: &BuildMatch) -> bool {
    candidate.matched_blocks > current.matched_blocks
        || (candidate.matched_blocks == current.matched_blocks && candidate.score > current.score)
}

fn chat_response(message: String) -> MinecraftMessageResponse {
    MinecraftMessageResponse {
        reply: message.clone(),
        actions: vec![GameAction::Chat { message }],
        job_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        BlueprintBlock, BuildRecord, BuildStatus, ExpectedBuildAction, MaterialCount,
    };

    fn matched_build() -> BuildMatch {
        BuildMatch {
            record: BuildRecord {
                id: "hm-job-1".to_string(),
                server_id: "hmcl-lan".to_string(),
                target_player: Some("Steve".to_string()),
                summary: "建造蓝图 test-house".to_string(),
                status: BuildStatus::Succeeded,
                expected_actions: vec![ExpectedBuildAction {
                    blueprint_id: Some("test-house".to_string()),
                    origin: crate::domain::types::BlockOrigin {
                        world: Some("minecraft:overworld".to_string()),
                        x: 10,
                        y: 64,
                        z: 20,
                    },
                    expected_count: 1,
                    materials: vec![MaterialCount {
                        material: "minecraft:oak_planks".to_string(),
                        count: 1,
                    }],
                    blocks: vec![BlueprintBlock {
                        x: 0,
                        y: 0,
                        z: 0,
                        material: "minecraft:oak_planks".to_string(),
                    }],
                }],
                result: None,
                message: None,
            },
            action_index: 0,
            matched_blocks: 1,
            scanned_expected_blocks: 1,
            score: 1.0,
        }
    }

    fn build_match_at(id: &str, x: i32, z: i32, matched_blocks: u32, score: f32) -> BuildMatch {
        BuildMatch {
            record: BuildRecord {
                id: id.to_string(),
                server_id: "hmcl-lan".to_string(),
                target_player: Some("Steve".to_string()),
                summary: format!("建造蓝图 {id}"),
                status: BuildStatus::Succeeded,
                expected_actions: vec![ExpectedBuildAction {
                    blueprint_id: Some(id.to_string()),
                    origin: crate::domain::types::BlockOrigin {
                        world: Some("minecraft:overworld".to_string()),
                        x,
                        y: 64,
                        z,
                    },
                    expected_count: 1,
                    materials: vec![MaterialCount {
                        material: "minecraft:oak_planks".to_string(),
                        count: 1,
                    }],
                    blocks: vec![BlueprintBlock {
                        x: 0,
                        y: 0,
                        z: 0,
                        material: "minecraft:oak_planks".to_string(),
                    }],
                }],
                result: None,
                message: None,
            },
            action_index: 0,
            matched_blocks,
            scanned_expected_blocks: matched_blocks,
            score,
        }
    }

    #[test]
    fn rank_build_matches_merges_actions_from_same_record() {
        let mut first = matched_build();
        let mut second = matched_build();
        first.action_index = 0;
        first.matched_blocks = 126;
        first.score = 0.4;
        second.record.expected_actions.push(ExpectedBuildAction {
            blueprint_id: Some("test-house-main".to_string()),
            origin: crate::domain::types::BlockOrigin {
                world: Some("minecraft:overworld".to_string()),
                x: 10,
                y: 64,
                z: 20,
            },
            expected_count: 1,
            materials: vec![MaterialCount {
                material: "minecraft:oak_planks".to_string(),
                count: 1,
            }],
            blocks: vec![BlueprintBlock {
                x: 1,
                y: 0,
                z: 0,
                material: "minecraft:oak_planks".to_string(),
            }],
        });
        second.action_index = 1;
        second.matched_blocks = 346;
        second.score = 0.6;
        let scan = WorldScan {
            world: "minecraft:overworld".to_string(),
            center_x: 10,
            center_y: 64,
            center_z: 20,
            radius: 8,
            blocks: Vec::new(),
        };

        let ranked = rank_build_matches(vec![first, second], None, &scan);

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].item.record.id, "hm-job-1");
        assert_eq!(ranked[0].item.action_index, 1);
        assert_eq!(ranked[0].total_matched_blocks, 472);
        assert!(!is_ambiguous_match(&ranked[0], ranked.get(1), false));
    }

    #[test]
    fn rank_build_matches_prefers_crosshair_direction_before_raw_match_count() {
        let forward = build_match_at("front-wheel", 0, 8, 30, 0.3);
        let side = build_match_at("side-platform", 12, 3, 200, 0.9);
        let scan = WorldScan {
            world: "minecraft:overworld".to_string(),
            center_x: 0,
            center_y: 64,
            center_z: 5,
            radius: 8,
            blocks: Vec::new(),
        };
        let position = PlayerPosition {
            world: "minecraft:overworld".to_string(),
            x: 0.0,
            y: 64.0,
            z: 0.0,
            yaw: Some(0.0),
            pitch: None,
        };

        let ranked = rank_build_matches(vec![side, forward], Some(&position), &scan);

        assert_eq!(ranked[0].item.record.id, "front-wheel");
        assert!(!is_ambiguous_match(&ranked[0], ranked.get(1), true));
    }
}
