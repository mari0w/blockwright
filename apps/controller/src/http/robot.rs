use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration, Instant};

use crate::{
    domain::types::{ChatAttachment, GameAction, GameJob, PlayerPosition, PlayerState, WorldScan},
    services::chat::IncomingChatMessage,
    services::job_queue::JobQueuePhase,
    services::planner::PlannerInput,
    state::AppState,
};

const LIVE_CONTEXT_TIMEOUT_SECONDS: u64 = 6;

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
    let live_context = read_live_context_for_message(state, &message, target_player.clone()).await;
    let plan = state
        .planner
        .plan_with_context_stores(
            PlannerInput {
                text: message.text,
                player: target_player.clone(),
                codex_session_key: Some(format!(
                    "robot:{}:{}:{}",
                    message.platform, message.conversation_id, message.sender
                )),
                position: message.position,
                player_state: live_context.player_state,
                nearby_scan: live_context.nearby_scan,
                attachments: message.attachments,
                progress_id: None,
            },
            &state.blueprints,
            Some(&state.builds),
        )
        .await;

    let server_id = message
        .server_id
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let has_build = has_build_action(&plan.actions);
    let queued_job = if plan.actions.is_empty() || only_chat_actions(&plan.actions) {
        None
    } else if !has_build && has_scan_action(&plan.actions) {
        if let Some(job) = state
            .jobs
            .merge_pending_scan_job(
                &server_id,
                target_player.as_deref(),
                plan.summary.clone(),
                &plan.actions,
            )
            .await
        {
            Some(job)
        } else {
            Some(
                state
                    .jobs
                    .enqueue(server_id, target_player, plan.summary.clone(), plan.actions)
                    .await,
            )
        }
    } else {
        let job_id = state.jobs.reserve_job_id();
        if has_build {
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
                    reply: "构建记录保存失败，已取消发送建筑任务。".to_string(),
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

#[derive(Default)]
struct LiveContext {
    player_state: Option<PlayerState>,
    nearby_scan: Option<WorldScan>,
}

async fn read_live_context_for_message(
    state: &AppState,
    message: &IncomingChatMessage,
    target_player: Option<String>,
) -> LiveContext {
    if matches!(
        state.llm.provider(),
        crate::config::LlmProviderKind::CodexCli
    ) {
        return LiveContext::default();
    }

    let text = message.text.trim();
    if text.is_empty() {
        return LiveContext::default();
    }

    let server_id = message
        .server_id
        .clone()
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let mut context = LiveContext::default();

    // API/Web 消息不像 Minecraft 直连请求那样天然带现场快照；需要时先通过现有 MCP 查询任务补齐真实数据。
    if should_read_player_state(text) {
        match live_query(
            state,
            server_id.clone(),
            target_player.clone(),
            "读取玩家手持物和物品栏".to_string(),
            vec![GameAction::GetPlayerState {
                player: target_player.clone(),
            }],
        )
        .await
        {
            Ok(result) => context.player_state = result.player_state,
            Err(error) => {
                tracing::warn!(error = %error, "failed to prefetch player state for api chat")
            }
        }
    }

    if should_scan_nearby_blocks(text) {
        match live_query(
            state,
            server_id,
            target_player.clone(),
            "扫描玩家附近方块".to_string(),
            vec![GameAction::ScanNearby {
                player: target_player,
                radius: 0,
            }],
        )
        .await
        {
            Ok(result) => context.nearby_scan = result.nearby_scan,
            Err(error) => {
                tracing::warn!(error = %error, "failed to prefetch nearby scan for api chat")
            }
        }
    }

    context
}

async fn live_query(
    state: &AppState,
    server_id: String,
    target_player: Option<String>,
    summary: String,
    actions: Vec<GameAction>,
) -> Result<crate::domain::types::JobResultRequest, String> {
    let job = state
        .jobs
        .enqueue(server_id.clone(), target_player, summary, actions)
        .await;
    let deadline = Instant::now() + Duration::from_secs(LIVE_CONTEXT_TIMEOUT_SECONDS);

    loop {
        if let Some(status) = state.jobs.status(&job.id).await {
            if matches!(
                status.phase,
                JobQueuePhase::Succeeded | JobQueuePhase::Failed
            ) {
                let result = status
                    .result
                    .ok_or_else(|| format!("查询任务 {} 已结束，但没有回写结果", job.id))?;
                if !result.ok {
                    return Err(result
                        .message
                        .unwrap_or_else(|| format!("查询任务 {} 执行失败", job.id)));
                }
                return Ok(result);
            }
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "等待 Minecraft 插件返回查询结果超时：server_id={server_id}，job_id={}",
                job.id
            ));
        }
        sleep(Duration::from_millis(200)).await;
    }
}

fn should_read_player_state(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    contains_any(
        text,
        &[
            "手上",
            "手里",
            "拿着",
            "背包",
            "物品栏",
            "快捷栏",
            "主手",
            "副手",
        ],
    ) || contains_any(
        &lower,
        &[
            "holding",
            "in my hand",
            "main hand",
            "off hand",
            "inventory",
            "hotbar",
            "selected slot",
        ],
    )
}

fn should_scan_nearby_blocks(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    contains_any(
        text,
        &[
            "附近",
            "周围",
            "旁边",
            "面前",
            "脚下",
            "这里",
            "场地",
            "地形",
            "方块",
            "扫描",
            "看一下",
            "看看",
            "改造",
            "修改",
            "替换",
            "扩建",
            "盖",
            "建",
            "造",
        ],
    ) || contains_any(
        &lower,
        &[
            "nearby",
            "around me",
            "around here",
            "in front",
            "under me",
            "terrain",
            "site",
            "blocks",
            "scan",
            "inspect",
            "build",
            "place",
            "modify",
            "replace",
            "expand",
        ],
    )
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn has_build_action(actions: &[crate::domain::types::GameAction]) -> bool {
    actions
        .iter()
        .any(|action| matches!(action, crate::domain::types::GameAction::PlaceBlocks { .. }))
}

fn has_scan_action(actions: &[crate::domain::types::GameAction]) -> bool {
    actions.iter().any(|action| {
        matches!(
            action,
            crate::domain::types::GameAction::ScanNearbyAndPlan { .. }
        )
    })
}

fn only_chat_actions(actions: &[crate::domain::types::GameAction]) -> bool {
    !actions.is_empty()
        && actions
            .iter()
            .all(|action| matches!(action, crate::domain::types::GameAction::Chat { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_context_detection_covers_common_read_requests() {
        assert!(should_scan_nearby_blocks("附近有什么方块"));
        assert!(should_scan_nearby_blocks("inspect the blocks around me"));
        assert!(should_scan_nearby_blocks("把面前的房子窗户换成蓝色玻璃"));

        assert!(should_read_player_state("我手上拿着什么"));
        assert!(should_read_player_state("what is in my main hand"));
        assert!(should_read_player_state("show my inventory"));
    }

    #[test]
    fn live_context_detection_ignores_plain_chat() {
        assert!(!should_scan_nearby_blocks("先聊一下设计风格"));
        assert!(!should_read_player_state("先聊一下设计风格"));
    }
}
