use crate::{
    domain::types::{
        BlockOrigin, Blueprint, BlueprintBlock, BlueprintSize, ChatAttachment, ChatAttachmentKind,
        ChatAttachmentSource, ExpectedBuildAction, GameAction, MaterialCount, PlayerPosition,
        PlayerState, WorldScan, WorldScanBlock, PLACE_BLOCKS_CHUNK_SIZE,
    },
    integrations::codex::{CodexClient, CodexResponseSchema},
    services::{
        blueprint_store::BlueprintStore,
        build_store::BuildStore,
        image_blueprint::{
            build_from_first_local_image, should_generate_image_blueprint, ImageBlueprintError,
        },
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

const PLAYER_SAFETY_RADIUS: i32 = 1;
const PLAYER_SAFETY_HEIGHT_BLOCKS: i32 = 3;
const CONTEXT_BLUEPRINT_LIMIT: usize = 24;
const CONTEXT_BUILD_LIMIT: usize = 12;
const CONTEXT_BLUEPRINT_BLOCK_SAMPLE_LIMIT: usize = 32;
const CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT: usize = 32;
const BLUEPRINT_PRIMITIVE_MAX_BLOCKS: usize = 50_000;
#[derive(Debug, Clone)]
pub struct PlannerInput {
    pub text: String,
    pub player: Option<String>,
    pub codex_session_key: Option<String>,
    pub position: Option<PlayerPosition>,
    pub player_state: Option<PlayerState>,
    pub nearby_scan: Option<WorldScan>,
    pub attachments: Vec<ChatAttachment>,
    pub progress_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlanResult {
    pub reply: String,
    pub summary: String,
    pub actions: Vec<GameAction>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CodexPlan {
    reply: String,
    summary: String,
    blueprint: Option<Blueprint>,
    site_plan: Option<CodexSitePlan>,
    actions: Vec<GameAction>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct CodexSitePlan {
    origin: Option<BlockOrigin>,
    clear_existing: Option<bool>,
    pre_clear_blocks: Vec<BlueprintBlock>,
    pre_foundation_blocks: Vec<BlueprintBlock>,
    rationale: Option<String>,
}

#[derive(Debug, Serialize)]
struct PlanContextBundle {
    player: Option<String>,
    user_text: String,
    attachments: Vec<ChatAttachment>,
    position: Option<PlayerPosition>,
    player_state: Option<PlayerState>,
    site: SiteContextBundle,
    available_blueprints: Vec<BlueprintContext>,
    recent_builds: Vec<BuildRecordContext>,
    protocol: PlanProtocolContext,
}

#[derive(Debug, Serialize)]
struct SiteContextBundle {
    summary: String,
    nearby_scan: Option<WorldScan>,
    scan_analysis: Option<ScanAnalysis>,
}

#[derive(Debug, Serialize)]
struct ScanAnalysis {
    bounds: ScanBounds,
    top_materials: Vec<MaterialCount>,
    columns: Vec<ScanColumn>,
}

#[derive(Debug, Serialize)]
struct ScanBounds {
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
    min_z: i32,
    max_z: i32,
}

#[derive(Debug, Serialize)]
struct ScanColumn {
    x: i32,
    z: i32,
    highest_support_y: Option<i32>,
    support_material: Option<String>,
    non_air_count: usize,
}

#[derive(Debug, Serialize)]
struct BlueprintContext {
    id: String,
    name: String,
    description: String,
    size: BlueprintSize,
    tags: Vec<String>,
    spec: Option<serde_json::Value>,
    expanded_hash: Option<String>,
    block_count: usize,
    materials: Vec<MaterialCount>,
    block_sample_limit: usize,
    block_sample_truncated: bool,
    block_sample: Vec<BlueprintBlock>,
}

#[derive(Debug, Serialize)]
struct BuildRecordContext {
    id: String,
    server_id: String,
    target_player: Option<String>,
    summary: String,
    status: String,
    nearest_action_origin: Option<BlockOrigin>,
    distance_to_target_blocks: Option<f64>,
    actions: Vec<BuildActionContext>,
}

#[derive(Debug, Serialize)]
struct BuildActionContext {
    blueprint_id: Option<String>,
    origin: BlockOrigin,
    expected_count: u32,
    materials: Vec<MaterialCount>,
    block_sample_limit: usize,
    block_sample_truncated: bool,
    block_sample: Vec<BlueprintBlock>,
}

#[derive(Debug, Serialize)]
struct PlanProtocolContext {
    output_contract: &'static str,
    controller_role: &'static str,
    safety_boundary: &'static str,
    targeting_policy: &'static str,
    available_skills: [&'static str; 6],
    available_actions: [&'static str; 5],
    available_mcp_tools: Vec<&'static str>,
}

#[derive(Clone)]
struct TargetPoint {
    world: Option<String>,
    x: f64,
    y: f64,
    z: f64,
}

struct BuildContextCandidate {
    context: BuildRecordContext,
    recency_index: usize,
}

#[derive(Debug, Clone, Copy)]
struct ContextHistoryPolicy {
    include_blueprints: bool,
    include_builds: bool,
}

#[derive(Clone, Default)]
pub struct Planner {
    codex: Option<CodexClient>,
}

fn codex_failure_reply(error: &str) -> String {
    let detail = if error.contains("No such file or directory") || error.contains("os error 2") {
        "具体原因：controller 启动环境找不到 codex 命令。"
    } else {
        "请管理员检查 Codex 登录状态、模型权限、网络连接或 CLI 版本。"
    };
    codex_failure_reply_with_log_hint(error, detail, controller_log_hint().as_deref())
}

fn codex_failure_reply_with_log_hint(error: &str, detail: &str, log_path: Option<&str>) -> String {
    let trace_hint = extract_codex_trace_id(error)
        .map(|trace_id| format!("日志关键字：codex_trace_id={trace_id}。"))
        .unwrap_or_default();
    let log_hint = match log_path {
        Some(path) if !path.is_empty() => format!("详细日志：{path}。"),
        _ => "详细日志：controller 控制台；HMCL 自动启动时也会写入 Minecraft logs/blockwright-controller.log。".to_string(),
    };
    format!("AI 建造助手这次调用失败了，任务还没有发送到 Minecraft。{detail}{trace_hint}{log_hint}")
}

fn extract_codex_trace_id(error: &str) -> Option<&str> {
    let start = error.find("codex_trace_id=")? + "codex_trace_id=".len();
    let rest = &error[start..];
    let end = rest
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')))
        .unwrap_or(rest.len());
    let trace_id = &rest[..end];
    if trace_id.is_empty() {
        None
    } else {
        Some(trace_id)
    }
}

fn controller_log_hint() -> Option<String> {
    std::env::var("BLOCKWRIGHT_CONTROLLER_LOG_PATH")
        .ok()
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
}

impl Planner {
    pub fn new(codex: CodexClient) -> Self {
        Self { codex: Some(codex) }
    }

    pub async fn plan(&self, input: PlannerInput, blueprints: &BlueprintStore) -> PlanResult {
        self.plan_with_context_stores(input, blueprints, None).await
    }

    pub async fn plan_with_context_stores(
        &self,
        input: PlannerInput,
        blueprints: &BlueprintStore,
        builds: Option<&BuildStore>,
    ) -> PlanResult {
        if let Some(result) = self.try_image_blueprint_plan(&input, blueprints).await {
            return result;
        }

        if !self.codex_enabled() {
            return PlanResult {
                reply: "我现在还没有连上 AI 建造助手，暂时不能理解自然语言请求。请先让管理员检查后台配置。".to_string(),
                summary: "AI 助手未启用".to_string(),
                actions: Vec::new(),
            };
        }

        if let Some(result) = self.try_codex_plan(&input, blueprints, builds).await {
            return result;
        }

        PlanResult {
            reply: "AI 这次没有生成可靠的操作结果，任务没有发送到 Minecraft。请直接重说要做什么，我会按能读取到的世界数据继续处理。"
                .to_string(),
            summary: "AI 未生成可靠操作".to_string(),
            actions: Vec::new(),
        }
    }

    async fn try_image_blueprint_plan(
        &self,
        input: &PlannerInput,
        blueprints: &BlueprintStore,
    ) -> Option<PlanResult> {
        if !should_generate_image_blueprint(&input.text, &input.attachments, self.codex_enabled()) {
            return None;
        }

        let image_plan = match build_from_first_local_image(&input.text, &input.attachments)? {
            Ok(plan) => plan,
            Err(error @ ImageBlueprintError::Decode { .. }) if self.codex_enabled() => {
                tracing::warn!(
                    error = %error,
                    "local image could not be decoded by deterministic image pipeline; falling back to codex image planning"
                );
                return None;
            }
            Err(error) => {
                return Some(PlanResult {
                    reply: format!("图片复刻蓝图生成失败：{error}。"),
                    summary: "图片复刻蓝图生成失败".to_string(),
                    actions: Vec::new(),
                });
            }
        };
        let output_width = image_plan.output_width;
        let output_height = image_plan.output_height;
        let block_count = image_plan.blueprint.blocks.len();
        let source_path = image_plan.source_path.display().to_string();
        tracing::info!(
            source_path = %source_path,
            original_width = image_plan.original_width,
            original_height = image_plan.original_height,
            output_width,
            output_height,
            block_count,
            "generated deterministic image blueprint"
        );

        let Some((actions, placement_note)) = self
            .actions_for_blueprint(input, blueprints, image_plan.blueprint, None)
            .await
        else {
            return Some(PlanResult {
                reply: "图片蓝图已经生成，但保存失败，任务没有发送到 Minecraft。".to_string(),
                summary: "图片复刻蓝图保存失败".to_string(),
                actions: Vec::new(),
            });
        };

        let reply = append_placement_note(
            format!(
                "已把图片转成 {}x{} 的高保真方块复刻蓝图，共 {} 个方块。可见画面按像素颜色映射到 Minecraft 方块；看不到的三维背面不会在这个像素复刻层里臆造。",
                output_width, output_height, block_count
            ),
            &placement_note,
        );
        Some(PlanResult {
            reply,
            summary: format!(
                "图片复刻 {}x{}，{} 个方块",
                output_width, output_height, block_count
            ),
            actions,
        })
    }

    fn codex_enabled(&self) -> bool {
        self.codex
            .as_ref()
            .map(CodexClient::enabled)
            .unwrap_or(false)
    }

    async fn try_codex_plan(
        &self,
        input: &PlannerInput,
        blueprints: &BlueprintStore,
        builds: Option<&BuildStore>,
    ) -> Option<PlanResult> {
        let codex = self.codex.as_ref()?;
        if !codex.enabled() {
            return None;
        }

        tracing::info!(
            has_nearby_scan = input.nearby_scan.is_some(),
            scan_block_count = input
                .nearby_scan
                .as_ref()
                .map(|scan| scan.blocks.len())
                .unwrap_or_default(),
            attachment_count = input.attachments.len(),
            "starting codex unified planner"
        );

        let context = build_context_bundle(input, blueprints, builds).await;
        let prompt = render_plan_prompt(&context);
        let image_paths = local_image_attachment_paths(&input.attachments);
        tracing::info!(
            prompt_bytes = prompt.len(),
            available_blueprint_count = context.available_blueprints.len(),
            recent_build_count = context.recent_builds.len(),
            local_image_count = image_paths.len(),
            "codex unified planner prompt prepared"
        );
        let output = match codex
            .ask_with_schema_and_progress_and_images(
                &prompt,
                CodexResponseSchema::Plan,
                // 同一个玩家/用户名要沿用同一条 Codex 会话；上下文满时 CodexClient 会清掉旧线程并重试。
                input.codex_session_key.as_deref(),
                input.progress_id.as_deref(),
                &image_paths,
            )
            .await
        {
            Ok(Some(output)) if !output.trim().is_empty() => output,
            Ok(_) => return None,
            Err(error) => {
                tracing::warn!(error = %error, "codex unified planning failed");
                return Some(PlanResult {
                    reply: codex_failure_reply(&error.to_string()),
                    summary: "AI 助手调用失败".to_string(),
                    actions: Vec::new(),
                });
            }
        };
        tracing::info!(
            response_bytes = output.len(),
            "codex plan response received; parsing json"
        );

        let mut plan = match parse_plan_response_for_input(&output, &input.text) {
            Some(plan) => plan,
            None => {
                tracing::warn!("codex unified planning returned invalid json");
                match repair_invalid_codex_plan(codex, input, &context, &output).await {
                    Some(plan) => plan,
                    None => return Some(invalid_codex_plan_fallback(input).await),
                }
            }
        };
        if let Some(issues) = image_recreation_quality_issues(input, plan.blueprint.as_ref()) {
            tracing::warn!(
                issues = ?issues,
                "codex image recreation blueprint did not pass minimum quality gate"
            );
            match repair_low_quality_image_plan(codex, input, &context, &plan, &issues).await {
                Some(repaired)
                    if image_recreation_quality_issues(input, repaired.blueprint.as_ref())
                        .is_none() =>
                {
                    plan = repaired;
                }
                _ => return Some(low_quality_image_plan_fallback(&issues)),
            }
        }
        let mut actions = plan.actions;
        let mut reply = plan.reply;

        if let Some(blueprint) = plan.blueprint {
            let (blueprint_actions, placement_note) = self
                .actions_for_blueprint(input, blueprints, blueprint, plan.site_plan.as_ref())
                .await?;
            reply = append_placement_note(reply, &placement_note);
            actions.extend(blueprint_actions);
        }

        let action_types = actions.iter().map(action_type_name).collect::<Vec<_>>();
        tracing::info!(
            summary = %plan.summary,
            action_count = actions.len(),
            action_types = ?action_types,
            "planned with codex unified planner"
        );

        Some(PlanResult {
            reply,
            summary: plan.summary,
            actions,
        })
    }

    async fn actions_for_blueprint(
        &self,
        input: &PlannerInput,
        blueprints: &BlueprintStore,
        blueprint: Blueprint,
        site_plan: Option<&CodexSitePlan>,
    ) -> Option<(Vec<GameAction>, String)> {
        tracing::info!(
            blueprint_id = %blueprint.id,
            block_count = blueprint.blocks.len(),
            material_count = blueprint.materials.len(),
            "codex plan included blueprint"
        );
        let PlacementDecision::Ready {
            origin,
            clear_existing,
            pre_foundation_blocks,
            pre_clear_blocks,
            note,
        } = placement_decision(input, &blueprint, site_plan);
        tracing::info!(
            blueprint_id = %blueprint.id,
            world = ?origin.world,
            origin_x = origin.x,
            origin_y = origin.y,
            origin_z = origin.z,
            clear_existing,
            pre_foundation_count = pre_foundation_blocks.len(),
            pre_clear_count = pre_clear_blocks.len(),
            model_site_plan = site_plan.is_some(),
            "codex blueprint placement assessed"
        );
        let placement = (
            origin,
            clear_existing,
            pre_foundation_blocks,
            pre_clear_blocks,
            note,
        );
        let blueprint = match blueprints.save(blueprint).await {
            Ok(blueprint) => blueprint,
            Err(error) => {
                tracing::warn!(error = %error, "failed to save codex generated blueprint");
                return None;
            }
        };
        let (origin, clear_existing, pre_foundation_blocks, pre_clear_blocks, placement_note) =
            placement;
        let mut actions = Vec::new();
        if !pre_foundation_blocks.is_empty() {
            push_place_blocks_actions(
                &mut actions,
                format!("{}:site-foundation", blueprint.id),
                origin.clone(),
                pre_foundation_blocks,
                true,
            );
        }
        if !pre_clear_blocks.is_empty() {
            push_place_blocks_actions(
                &mut actions,
                format!("{}:site-clear", blueprint.id),
                origin.clone(),
                pre_clear_blocks,
                true,
            );
        }
        push_place_blocks_actions(
            &mut actions,
            blueprint.id.clone(),
            origin,
            blueprint.blocks.clone(),
            clear_existing,
        );

        Some((actions, placement_note))
    }
}

fn push_place_blocks_actions(
    actions: &mut Vec<GameAction>,
    blueprint_id: String,
    origin: BlockOrigin,
    blocks: Vec<BlueprintBlock>,
    clear_existing: bool,
) {
    if blocks.len() <= PLACE_BLOCKS_CHUNK_SIZE {
        actions.push(GameAction::PlaceBlocks {
            blueprint_id: Some(blueprint_id),
            origin,
            blocks,
            clear_existing,
        });
        return;
    }

    for (index, chunk) in blocks.chunks(PLACE_BLOCKS_CHUNK_SIZE).enumerate() {
        actions.push(GameAction::PlaceBlocks {
            blueprint_id: Some(format!("{blueprint_id}:part-{index:04}")),
            origin: origin.clone(),
            blocks: chunk.to_vec(),
            clear_existing,
        });
    }
}

fn action_type_name(action: &GameAction) -> &'static str {
    match action {
        GameAction::GiveItem { .. } => "give_item",
        GameAction::PlaceBlocks { .. } => "place_blocks",
        GameAction::RunCommand { .. } => "run_command",
        GameAction::Chat { .. } => "chat",
        GameAction::ScanNearbyAndPlan { .. } => "scan_nearby_and_plan",
        GameAction::GetPlayerState { .. } => "get_player_state",
        GameAction::ScanNearby { .. } => "scan_nearby",
    }
}

struct BlueprintBounds {
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
    min_z: i32,
    max_z: i32,
}

struct PlacementCollision {
    x: i32,
    y: i32,
    z: i32,
    material: String,
}

struct PlacementCandidate {
    origin: BlockOrigin,
    target_collisions: Vec<PlacementCollision>,
    volume_collisions: Vec<PlacementCollision>,
    distance_score: i32,
    forward_preference_score: i32,
    player_safety_overlap_count: usize,
    has_known_ground: bool,
    surface_score: PlacementSurfaceScore,
}

#[derive(Clone, Copy, Debug, Default)]
struct PlacementSurfaceScore {
    missing_support_count: usize,
    height_spread: i32,
}

struct FootprintSurface {
    ground_y: i32,
    score: PlacementSurfaceScore,
}

enum PlacementDecision {
    Ready {
        origin: BlockOrigin,
        clear_existing: bool,
        pre_foundation_blocks: Vec<crate::domain::types::BlueprintBlock>,
        pre_clear_blocks: Vec<crate::domain::types::BlueprintBlock>,
        note: String,
    },
}

fn placement_decision(
    input: &PlannerInput,
    blueprint: &Blueprint,
    site_plan: Option<&CodexSitePlan>,
) -> PlacementDecision {
    let Some(site_plan) = site_plan else {
        return assess_placement(input, blueprint);
    };
    if site_plan.origin.is_none()
        && site_plan.clear_existing.is_none()
        && site_plan.pre_clear_blocks.is_empty()
        && site_plan.pre_foundation_blocks.is_empty()
        && site_plan
            .rationale
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return assess_placement(input, blueprint);
    }
    placement_from_model_site_plan(input, blueprint, site_plan)
}

fn placement_from_model_site_plan(
    input: &PlannerInput,
    blueprint: &Blueprint,
    site_plan: &CodexSitePlan,
) -> PlacementDecision {
    let bounds = blueprint_bounds(&blueprint.blocks);
    let mut origin = site_plan
        .origin
        .clone()
        .unwrap_or_else(|| placement_origin(input, bounds.as_ref()));
    let mut note_parts = Vec::new();
    if let Some(rationale) = site_plan
        .rationale
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        note_parts.push(format!("已按模型 site_plan：{rationale}，"));
    } else {
        note_parts.push("已按模型 site_plan 选择落点和场地处理，".to_string());
    }

    if player_safety_overlap_count(input, &origin, bounds.as_ref()) > 0 {
        origin = safe_origin_for_player(input, bounds.as_ref()).unwrap_or(origin);
        note_parts.push("原落点靠近玩家身体，已按安全边界调整，".to_string());
    }

    let pre_foundation_blocks =
        filter_blocks_outside_player_safety(input, &origin, &site_plan.pre_foundation_blocks);
    let pre_clear_blocks =
        filter_blocks_outside_player_safety(input, &origin, &site_plan.pre_clear_blocks);
    if pre_foundation_blocks.len() != site_plan.pre_foundation_blocks.len()
        || pre_clear_blocks.len() != site_plan.pre_clear_blocks.len()
    {
        note_parts.push("已移除玩家安全区内的场地辅助方块，".to_string());
    }

    PlacementDecision::Ready {
        origin,
        clear_existing: site_plan.clear_existing.unwrap_or(false),
        pre_foundation_blocks,
        pre_clear_blocks,
        note: note_parts.join(""),
    }
}

fn assess_placement(input: &PlannerInput, blueprint: &Blueprint) -> PlacementDecision {
    let bounds = blueprint_bounds(&blueprint.blocks);
    let origin = placement_origin(input, bounds.as_ref());
    let Some(scan) = input.nearby_scan.as_ref() else {
        return PlacementDecision::Ready {
            origin,
            clear_existing: false,
            pre_foundation_blocks: Vec::new(),
            pre_clear_blocks: Vec::new(),
            note: "这次没有收到场地扫描数据，按玩家当前位置估算落点，".to_string(),
        };
    };
    if blueprint.blocks.is_empty() {
        return PlacementDecision::Ready {
            origin,
            clear_existing: false,
            pre_foundation_blocks: Vec::new(),
            pre_clear_blocks: Vec::new(),
            note: "蓝图没有方块，".to_string(),
        };
    }

    let candidate = choose_placement_candidate(input, scan, bounds.as_ref(), &blueprint.blocks)
        .unwrap_or_else(|| {
            placement_candidate(
                input,
                scan,
                origin,
                PlacementSurfaceScore::default(),
                false,
                bounds.as_ref(),
                &blueprint.blocks,
            )
        });
    let shifted_note = if candidate.distance_score > 0 {
        format!(
            "已在附近自动选择更合适落点（距扫描中心 {} 格），",
            candidate.distance_score
        )
    } else {
        String::new()
    };
    let origin = candidate.origin;
    let target_collisions = candidate.target_collisions;
    let volume_collisions = candidate.volume_collisions;
    let pre_foundation_blocks = if should_prepare_foundation(blueprint) {
        foundation_blocks_for_footprint(scan, &origin, blueprint, bounds.as_ref())
    } else {
        Vec::new()
    };
    let foundation_note = foundation_note(pre_foundation_blocks.len(), blueprint);
    let all_collisions = target_collisions
        .iter()
        .chain(volume_collisions.iter())
        .collect::<Vec<_>>();

    if all_collisions.is_empty() {
        let origin_y = origin.y;
        return PlacementDecision::Ready {
            origin,
            clear_existing: false,
            pre_foundation_blocks,
            pre_clear_blocks: Vec::new(),
            note: format!(
                "{}已根据附近扫描把地基放在 y={}，{}目标区域没有检测到重叠方块，",
                shifted_note, origin_y, foundation_note
            ),
        };
    }

    let pre_clear_blocks = volume_collisions
        .iter()
        .map(|collision| crate::domain::types::BlueprintBlock {
            x: collision.x - origin.x,
            y: collision.y - origin.y,
            z: collision.z - origin.z,
            material: "minecraft:air".to_string(),
        })
        .collect::<Vec<_>>();
    let origin_y = origin.y;
    let collision_label = if all_collisions
        .iter()
        .all(|collision| is_auto_clear_material(collision.material.as_str()))
    {
        "软阻挡方块"
    } else {
        "已有方块"
    };
    PlacementDecision::Ready {
        origin,
        clear_existing: !target_collisions.is_empty(),
        pre_foundation_blocks,
        pre_clear_blocks,
        note: format!(
            "{}已根据附近扫描把地基放在 y={}，{}并会先处理 {} 个{}，",
            shifted_note,
            origin_y,
            foundation_note,
            all_collisions.len(),
            collision_label
        ),
    }
}

fn choose_placement_candidate(
    input: &PlannerInput,
    scan: &WorldScan,
    bounds: Option<&BlueprintBounds>,
    blocks: &[crate::domain::types::BlueprintBlock],
) -> Option<PlacementCandidate> {
    let (offset_x, offset_z) = blueprint_center_offset(bounds);
    let radius = scan.radius.min(10) as i32;
    let mut best: Option<PlacementCandidate> = None;

    for distance in 0..=radius {
        for dx in -distance..=distance {
            for dz in -distance..=distance {
                if dx.abs().max(dz.abs()) != distance {
                    continue;
                }

                let origin_x = scan.center_x + dx - offset_x;
                let origin_z = scan.center_z + dz - offset_z;
                let surface = surface_for_footprint(input, scan, origin_x, origin_z, bounds);
                let origin_y = surface.as_ref().map_or_else(
                    || {
                        input
                            .position
                            .as_ref()
                            .map(|position| position.y.round() as i32)
                            .unwrap_or(scan.center_y)
                    },
                    |surface| surface.ground_y + 1,
                );
                let candidate = placement_candidate(
                    input,
                    scan,
                    BlockOrigin {
                        world: Some(scan.world.clone()),
                        x: origin_x,
                        y: origin_y,
                        z: origin_z,
                    },
                    surface
                        .as_ref()
                        .map(|surface| surface.score)
                        .unwrap_or_default(),
                    surface.is_some(),
                    bounds,
                    blocks,
                );

                let replace = best
                    .as_ref()
                    .map(|best| {
                        placement_candidate_score(&candidate) < placement_candidate_score(best)
                    })
                    .unwrap_or(true);
                if replace {
                    best = Some(candidate);
                }
            }
        }

        if best
            .as_ref()
            .map(placement_candidate_is_ready)
            .unwrap_or(false)
        {
            break;
        }
    }

    best
}

fn placement_candidate(
    input: &PlannerInput,
    scan: &WorldScan,
    origin: BlockOrigin,
    surface_score: PlacementSurfaceScore,
    has_known_ground: bool,
    bounds: Option<&BlueprintBounds>,
    blocks: &[crate::domain::types::BlueprintBlock],
) -> PlacementCandidate {
    let target_positions = target_position_set(&origin, blocks);
    let target_collisions = placement_collisions(scan, &target_positions);
    let volume_collisions = bounds
        .map(|bounds| placement_volume_collisions(scan, &origin, bounds, &target_positions))
        .unwrap_or_default();
    let (offset_x, offset_z) = blueprint_center_offset(bounds);
    let distance_score =
        (origin.x + offset_x - scan.center_x).abs() + (origin.z + offset_z - scan.center_z).abs();
    let forward_preference_score = forward_preference_score(input, scan, &origin, bounds);
    let player_safety_overlap_count = player_safety_overlap_count(input, &origin, bounds);

    PlacementCandidate {
        origin,
        target_collisions,
        volume_collisions,
        distance_score,
        forward_preference_score,
        player_safety_overlap_count,
        has_known_ground,
        surface_score,
    }
}

fn placement_candidate_score(
    candidate: &PlacementCandidate,
) -> (usize, usize, i32, i32, usize, usize, usize, usize) {
    (
        candidate.player_safety_overlap_count,
        hard_collision_count(candidate),
        candidate.distance_score,
        -candidate.forward_preference_score,
        collision_count(candidate),
        candidate.surface_score.height_spread as usize,
        candidate.surface_score.missing_support_count,
        usize::from(!candidate.has_known_ground),
    )
}

fn collision_count(candidate: &PlacementCandidate) -> usize {
    candidate.target_collisions.len() + candidate.volume_collisions.len()
}

fn placement_candidate_is_ready(candidate: &PlacementCandidate) -> bool {
    candidate.player_safety_overlap_count == 0
        && collision_count(candidate) == 0
        && (candidate.distance_score == 0
            || (candidate.has_known_ground
                && candidate.surface_score.missing_support_count == 0
                && candidate.surface_score.height_spread <= 1))
}

fn hard_collision_count(candidate: &PlacementCandidate) -> usize {
    candidate
        .target_collisions
        .iter()
        .chain(candidate.volume_collisions.iter())
        .filter(|collision| !is_auto_clear_material(collision.material.as_str()))
        .count()
}

fn blueprint_center_offset(bounds: Option<&BlueprintBounds>) -> (i32, i32) {
    bounds
        .map(|bounds| {
            (
                (bounds.min_x + bounds.max_x) / 2,
                (bounds.min_z + bounds.max_z) / 2,
            )
        })
        .unwrap_or((0, 0))
}

fn placement_origin(input: &PlannerInput, bounds: Option<&BlueprintBounds>) -> BlockOrigin {
    let Some(scan) = input.nearby_scan.as_ref() else {
        return input
            .position
            .as_ref()
            .map(|position| origin_in_front_of_player(position, bounds))
            .unwrap_or(BlockOrigin {
                world: None,
                x: 0,
                y: 64,
                z: 0,
            });
    };

    let (offset_x, offset_z) = blueprint_center_offset(bounds);
    let x = scan.center_x - offset_x;
    let z = scan.center_z - offset_z;
    let y = surface_for_footprint(input, scan, x, z, bounds).map_or_else(
        || {
            input
                .position
                .as_ref()
                .map(|position| position.y.round() as i32)
                .unwrap_or(scan.center_y)
        },
        |surface| surface.ground_y + 1,
    );

    BlockOrigin {
        world: Some(scan.world.clone()),
        x,
        y,
        z,
    }
}

fn surface_for_footprint(
    input: &PlannerInput,
    scan: &WorldScan,
    origin_x: i32,
    origin_z: i32,
    bounds: Option<&BlueprintBounds>,
) -> Option<FootprintSurface> {
    let max_ground_y = input
        .position
        .as_ref()
        .map(|position| position.y.floor() as i32 - 1)
        .unwrap_or(scan.center_y - 1);
    let (min_x, max_x, min_z, max_z) = bounds
        .map(|bounds| {
            (
                origin_x + bounds.min_x,
                origin_x + bounds.max_x,
                origin_z + bounds.min_z,
                origin_z + bounds.max_z,
            )
        })
        .unwrap_or((scan.center_x, scan.center_x, scan.center_z, scan.center_z));

    let mut support_ys = Vec::new();
    let mut missing_support_count = 0usize;
    for x in min_x..=max_x {
        for z in min_z..=max_z {
            let support_y = scan
                .blocks
                .iter()
                .filter(|block| block.x == x && block.z == z && block.y <= max_ground_y)
                .filter(|block| is_build_support_material(block.material.as_str()))
                .map(|block| block.y)
                .max();

            if let Some(support_y) = support_y {
                support_ys.push(support_y);
            } else {
                missing_support_count += 1;
            }
        }
    }

    let min_support_y = support_ys.iter().min().copied()?;
    let max_support_y = support_ys.iter().max().copied()?;
    Some(FootprintSurface {
        ground_y: max_support_y,
        score: PlacementSurfaceScore {
            missing_support_count,
            height_spread: max_support_y - min_support_y,
        },
    })
}

fn foundation_blocks_for_footprint(
    scan: &WorldScan,
    origin: &BlockOrigin,
    blueprint: &Blueprint,
    bounds: Option<&BlueprintBounds>,
) -> Vec<crate::domain::types::BlueprintBlock> {
    let Some(bounds) = bounds else {
        return Vec::new();
    };

    let materials = foundation_materials_for_blueprint(blueprint);
    let target_support_y = origin.y + bounds.min_y - 1;
    let mut blocks = Vec::new();
    for x in origin.x + bounds.min_x..=origin.x + bounds.max_x {
        for z in origin.z + bounds.min_z..=origin.z + bounds.max_z {
            let safe_support_y = highest_safe_support_y_at(scan, x, z, target_support_y);
            if safe_support_y == Some(target_support_y) {
                continue;
            }

            let start_y = safe_support_y
                .map(|value| value + 1)
                .unwrap_or(target_support_y);
            for y in start_y..=target_support_y {
                blocks.push(crate::domain::types::BlueprintBlock {
                    x: x - origin.x,
                    y: y - origin.y,
                    z: z - origin.z,
                    material: materials.material_for_layer(y, target_support_y),
                });
            }
        }
    }
    blocks
}

struct FoundationMaterials {
    support: String,
    cap: String,
    label: &'static str,
}

impl FoundationMaterials {
    fn material_for_layer(&self, y: i32, target_support_y: i32) -> String {
        if y == target_support_y {
            self.cap.clone()
        } else {
            self.support.clone()
        }
    }
}

fn foundation_materials_for_blueprint(blueprint: &Blueprint) -> FoundationMaterials {
    let materials = blueprint
        .blocks
        .iter()
        .map(|block| material_id(block.material.as_str()))
        .chain(
            blueprint
                .materials
                .iter()
                .map(|item| material_id(item.material.as_str())),
        )
        .collect::<Vec<_>>();

    if let Some(prefix) = dominant_wood_prefix(&materials) {
        return FoundationMaterials {
            support: format!("minecraft:{prefix}_log[axis=y]"),
            cap: format!("minecraft:{prefix}_planks"),
            label: "木桩平台",
        };
    }

    FoundationMaterials {
        support: "minecraft:stone_bricks".to_string(),
        cap: "minecraft:stone_bricks".to_string(),
        label: "石砖基座",
    }
}

fn dominant_wood_prefix(materials: &[&str]) -> Option<&'static str> {
    let prefixes = [
        "oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry",
    ];

    prefixes.into_iter().find(|prefix| {
        materials.iter().any(|material| {
            material.contains(&format!("{prefix}_planks"))
                || material.contains(&format!("{prefix}_log"))
                || material.contains(&format!("{prefix}_wood"))
                || material.contains(&format!("{prefix}_stairs"))
                || material.contains(&format!("{prefix}_slab"))
        })
    })
}

fn highest_safe_support_y_at(scan: &WorldScan, x: i32, z: i32, max_y: i32) -> Option<i32> {
    scan.blocks
        .iter()
        .filter(|block| block.x == x && block.z == z && block.y <= max_y)
        .filter(|block| is_build_support_material(block.material.as_str()))
        .map(|block| block.y)
        .max()
}

fn should_prepare_foundation(blueprint: &Blueprint) -> bool {
    let special_span_tag = blueprint.tags.iter().any(|tag| {
        let tag = tag.to_lowercase();
        matches!(
            tag.as_str(),
            "bridge" | "dock" | "pier" | "treehouse" | "tree_house"
        )
    });

    !special_span_tag
}

fn foundation_note(count: usize, blueprint: &Blueprint) -> String {
    if count == 0 {
        String::new()
    } else {
        let materials = foundation_materials_for_blueprint(blueprint);
        format!("会先做 {} 个融入地形的{}方块，", count, materials.label)
    }
}

fn target_position_set(
    origin: &BlockOrigin,
    blocks: &[crate::domain::types::BlueprintBlock],
) -> HashSet<(i32, i32, i32)> {
    blocks
        .iter()
        .map(|block| (origin.x + block.x, origin.y + block.y, origin.z + block.z))
        .collect()
}

fn placement_collisions(
    scan: &WorldScan,
    target_positions: &HashSet<(i32, i32, i32)>,
) -> Vec<PlacementCollision> {
    scan.blocks
        .iter()
        .filter(|block| target_positions.contains(&(block.x, block.y, block.z)))
        .map(|block| PlacementCollision {
            x: block.x,
            y: block.y,
            z: block.z,
            material: block.material.clone(),
        })
        .collect()
}

fn placement_volume_collisions(
    scan: &WorldScan,
    origin: &BlockOrigin,
    bounds: &BlueprintBounds,
    target_positions: &HashSet<(i32, i32, i32)>,
) -> Vec<PlacementCollision> {
    let min_x = origin.x + bounds.min_x;
    let max_x = origin.x + bounds.max_x;
    let min_y = origin.y + bounds.min_y;
    let max_y = origin.y + bounds.max_y;
    let min_z = origin.z + bounds.min_z;
    let max_z = origin.z + bounds.max_z;

    scan.blocks
        .iter()
        .filter(|block| {
            block.x >= min_x
                && block.x <= max_x
                && block.y >= min_y
                && block.y <= max_y
                && block.z >= min_z
                && block.z <= max_z
                && !target_positions.contains(&(block.x, block.y, block.z))
        })
        .map(|block| PlacementCollision {
            x: block.x,
            y: block.y,
            z: block.z,
            material: block.material.clone(),
        })
        .collect()
}

fn player_safety_overlap_count(
    input: &PlannerInput,
    origin: &BlockOrigin,
    bounds: Option<&BlueprintBounds>,
) -> usize {
    let Some(bounds) = bounds else {
        return 0;
    };
    let Some(position) = input.position.as_ref() else {
        return 0;
    };
    if let Some(world) = origin.world.as_deref() {
        if world != position.world {
            return 0;
        }
    }

    player_safety_overlap_count_for_position(position, origin, bounds)
}

fn player_safety_overlap_count_for_position(
    position: &PlayerPosition,
    origin: &BlockOrigin,
    bounds: &BlueprintBounds,
) -> usize {
    let player_x = position.x.floor() as i32;
    let player_y = position.y.floor() as i32;
    let player_z = position.z.floor() as i32;

    let safety_min_x = player_x - PLAYER_SAFETY_RADIUS;
    let safety_max_x = player_x + PLAYER_SAFETY_RADIUS;
    let safety_min_y = player_y;
    let safety_max_y = player_y + PLAYER_SAFETY_HEIGHT_BLOCKS - 1;
    let safety_min_z = player_z - PLAYER_SAFETY_RADIUS;
    let safety_max_z = player_z + PLAYER_SAFETY_RADIUS;

    let min_x = origin.x + bounds.min_x;
    let max_x = origin.x + bounds.max_x;
    let min_y = origin.y + bounds.min_y;
    let max_y = origin.y + bounds.max_y;
    let min_z = origin.z + bounds.min_z;
    let max_z = origin.z + bounds.max_z;

    let overlap_min_x = min_x.max(safety_min_x);
    let overlap_max_x = max_x.min(safety_max_x);
    let overlap_min_y = min_y.max(safety_min_y);
    let overlap_max_y = max_y.min(safety_max_y);
    let overlap_min_z = min_z.max(safety_min_z);
    let overlap_max_z = max_z.min(safety_max_z);

    if overlap_min_x > overlap_max_x
        || overlap_min_y > overlap_max_y
        || overlap_min_z > overlap_max_z
    {
        return 0;
    }

    ((overlap_max_x - overlap_min_x + 1)
        * (overlap_max_y - overlap_min_y + 1)
        * (overlap_max_z - overlap_min_z + 1)) as usize
}

fn filter_blocks_outside_player_safety(
    input: &PlannerInput,
    origin: &BlockOrigin,
    blocks: &[BlueprintBlock],
) -> Vec<BlueprintBlock> {
    let Some(position) = input.position.as_ref() else {
        return blocks.to_vec();
    };
    if let Some(world) = origin.world.as_deref() {
        if world != position.world {
            return blocks.to_vec();
        }
    }
    blocks
        .iter()
        .filter(|block| {
            !is_world_block_inside_player_safety(
                origin.x + block.x,
                origin.y + block.y,
                origin.z + block.z,
                position,
            )
        })
        .cloned()
        .collect()
}

fn is_world_block_inside_player_safety(
    target_x: i32,
    target_y: i32,
    target_z: i32,
    position: &PlayerPosition,
) -> bool {
    is_within_player_safety_zone(
        target_x,
        target_y,
        target_z,
        position.x.floor() as i32,
        position.y.floor() as i32,
        position.z.floor() as i32,
    )
}

fn is_within_player_safety_zone(
    target_x: i32,
    target_y: i32,
    target_z: i32,
    player_x: i32,
    player_y: i32,
    player_z: i32,
) -> bool {
    (target_x - player_x).abs() <= PLAYER_SAFETY_RADIUS
        && target_y >= player_y
        && target_y < player_y + PLAYER_SAFETY_HEIGHT_BLOCKS
        && (target_z - player_z).abs() <= PLAYER_SAFETY_RADIUS
}

fn forward_preference_score(
    input: &PlannerInput,
    scan: &WorldScan,
    origin: &BlockOrigin,
    bounds: Option<&BlueprintBounds>,
) -> i32 {
    let Some(position) = input.position.as_ref() else {
        return 0;
    };
    let (offset_x, offset_z) = blueprint_center_offset(bounds);
    let candidate_dx = origin.x + offset_x - scan.center_x;
    let candidate_dz = origin.z + offset_z - scan.center_z;
    let forward_x = scan.center_x as f64 - position.x;
    let forward_z = scan.center_z as f64 - position.z;

    ((candidate_dx as f64 * forward_x) + (candidate_dz as f64 * forward_z)).round() as i32
}

fn blueprint_bounds(blocks: &[crate::domain::types::BlueprintBlock]) -> Option<BlueprintBounds> {
    let first = blocks.first()?;
    let mut bounds = BlueprintBounds {
        min_x: first.x,
        max_x: first.x,
        min_y: first.y,
        max_y: first.y,
        min_z: first.z,
        max_z: first.z,
    };
    for block in blocks.iter().skip(1) {
        bounds.min_x = bounds.min_x.min(block.x);
        bounds.max_x = bounds.max_x.max(block.x);
        bounds.min_y = bounds.min_y.min(block.y);
        bounds.max_y = bounds.max_y.max(block.y);
        bounds.min_z = bounds.min_z.min(block.z);
        bounds.max_z = bounds.max_z.max(block.z);
    }
    Some(bounds)
}

fn is_auto_clear_material(material: &str) -> bool {
    let material = material_id(material);
    matches!(
        material,
        "minecraft:grass"
            | "minecraft:short_grass"
            | "minecraft:tall_grass"
            | "minecraft:fern"
            | "minecraft:large_fern"
            | "minecraft:dead_bush"
            | "minecraft:snow"
            | "minecraft:vine"
            | "minecraft:dandelion"
            | "minecraft:poppy"
            | "minecraft:blue_orchid"
            | "minecraft:allium"
            | "minecraft:azure_bluet"
            | "minecraft:red_tulip"
            | "minecraft:orange_tulip"
            | "minecraft:white_tulip"
            | "minecraft:pink_tulip"
            | "minecraft:oxeye_daisy"
            | "minecraft:cornflower"
            | "minecraft:lily_of_the_valley"
            | "minecraft:brown_mushroom"
            | "minecraft:red_mushroom"
    )
}

fn is_build_support_material(material: &str) -> bool {
    let material = material_id(material);
    !is_auto_clear_material(material) && !is_unsuitable_support_material(material)
}

fn is_unsuitable_support_material(material: &str) -> bool {
    material == "minecraft:water"
        || material == "minecraft:lava"
        || material == "minecraft:fire"
        || material == "minecraft:soul_fire"
        || material == "minecraft:cactus"
        || material == "minecraft:bamboo"
        || material == "minecraft:chest"
        || material == "minecraft:trapped_chest"
        || material == "minecraft:barrel"
        || material == "minecraft:crafting_table"
        || material == "minecraft:furnace"
        || material == "minecraft:door"
        || material.ends_with("_door")
        || material.ends_with("_bed")
        || material.ends_with("_leaves")
        || material.ends_with("_log")
        || material.ends_with("_stem")
        || material.ends_with("_hyphae")
        || material.ends_with("_sapling")
        || material.ends_with("_torch")
        || material.ends_with("_lantern")
        || material.ends_with("_sign")
        || material.ends_with("_crop")
}

fn material_id(material: &str) -> &str {
    material
        .split_once('[')
        .map(|(id, _)| id)
        .unwrap_or(material)
}

fn origin_in_front_of_player(
    position: &PlayerPosition,
    bounds: Option<&BlueprintBounds>,
) -> BlockOrigin {
    let (step_x, step_z) = player_forward_step(position);
    let mut origin = BlockOrigin {
        world: Some(position.world.clone()),
        x: position.x.round() as i32 + step_x * 2,
        y: position.y.round() as i32,
        z: position.z.round() as i32 + step_z * 2,
    };

    if let Some(bounds) = bounds {
        for _ in 0..64 {
            if player_safety_overlap_count_for_position(position, &origin, bounds) == 0 {
                break;
            }
            origin.x += step_x;
            origin.z += step_z;
        }
    }

    origin
}

fn safe_origin_for_player(
    input: &PlannerInput,
    bounds: Option<&BlueprintBounds>,
) -> Option<BlockOrigin> {
    let position = input.position.as_ref()?;
    Some(origin_in_front_of_player(position, bounds))
}

fn player_forward_step(position: &PlayerPosition) -> (i32, i32) {
    if let Some(yaw) = position.yaw {
        let radians = yaw.to_radians();
        let step_x = (-radians.sin()).round() as i32;
        let step_z = radians.cos().round() as i32;
        if step_x != 0 || step_z != 0 {
            return (step_x, step_z);
        }
    }

    (1, 1)
}

async fn build_context_bundle(
    input: &PlannerInput,
    blueprints: &BlueprintStore,
    builds: Option<&BuildStore>,
) -> PlanContextBundle {
    let history_policy = context_history_policy(input);
    PlanContextBundle {
        player: input.player.clone(),
        user_text: input.text.trim().to_string(),
        attachments: input.attachments.clone(),
        position: input.position.clone(),
        player_state: input.player_state.clone(),
        site: build_site_context(input),
        available_blueprints: if history_policy.include_blueprints {
            blueprint_contexts(blueprints).await
        } else {
            Vec::new()
        },
        recent_builds: if history_policy.include_builds {
            build_contexts(builds, input.player.as_deref(), target_point(input)).await
        } else {
            Vec::new()
        },
        protocol: PlanProtocolContext {
            output_contract: "只返回一个 JSON 对象，字段为 reply、summary、blueprint、site_plan、actions。",
            controller_role: "controller 是 Minecraft AI 助手的工具运行时和兼容协议桥：提供 context_bundle、MCP、蓝图保存、构建记录、安全校验和任务队列；具体聊天、工具调用、建筑设计和执行方案由模型结合 skills 自主决定。",
            safety_boundary: "Minecraft 执行只能通过受控 GameAction；run_command 不做命令白名单限制，建筑放置仍会拦截玩家安全区内放置。",
            targeting_policy: "明确请求直接完成；没有指定风格、规模、朝向或坐标时自主选择合理默认值。只有意图冲突、危险，或改造既有建筑且最近候选不确定、多个候选都合理或目标部位不明确时，才回复确认问题并不输出 Minecraft 动作。",
            available_skills: [
                "blockwright-build-planning",
                "blockwright-site-selection",
                "blockwright-blueprint-verification",
                "blockwright-existing-build-edit",
                "blockwright-image-to-blueprint",
                "blockwright-command-actions",
            ],
            available_actions: [
                "give_item",
                "place_blocks",
                "run_command",
                "chat",
                "scan_nearby_and_plan",
            ],
            available_mcp_tools: vec![
                "blockwright_get_player_state",
                "blockwright_scan_nearby_blocks",
                "blockwright_give_item",
                "blockwright_place_blocks",
                "blockwright_run_command",
                "blockwright_send_chat",
                "blockwright_list_blueprints",
                "blockwright_get_blueprint",
                "blockwright_save_blueprint",
                "blockwright_delete_blueprint",
                "blockwright_list_builds",
                "blockwright_get_build",
                "blockwright_delete_build",
                "blockwright_search_builds_nearby",
                "blockwright_enqueue_actions",
            ],
        },
    }
}

fn local_image_attachment_paths(attachments: &[ChatAttachment]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::<PathBuf>::new();
    for attachment in attachments {
        if attachment.kind != ChatAttachmentKind::Image {
            continue;
        }
        let ChatAttachmentSource::LocalPath { path } = &attachment.source else {
            continue;
        };
        let path = PathBuf::from(path);
        if !path.is_file() || !seen.insert(path.clone()) {
            continue;
        }
        paths.push(path);
    }
    paths
}

fn image_recreation_quality_issues(
    input: &PlannerInput,
    blueprint: Option<&Blueprint>,
) -> Option<Vec<String>> {
    if local_image_attachment_paths(&input.attachments).is_empty()
        || image_request_allows_tiny_or_planar(&input.text)
    {
        return None;
    }
    let blueprint = blueprint?;
    let mut issues = Vec::new();
    let solid_blocks = blueprint
        .blocks
        .iter()
        .filter(|block| block.material != "minecraft:air")
        .count();
    if solid_blocks < 96 {
        issues.push(format!(
            "方块数只有 {solid_blocks}，图片复刻不能退化成小模型"
        ));
    }

    if let Some(bounds) = blueprint_bounds(&blueprint.blocks) {
        let width = bounds.max_x - bounds.min_x + 1;
        let height = bounds.max_y - bounds.min_y + 1;
        let depth = bounds.max_z - bounds.min_z + 1;
        if width < 5 {
            issues.push(format!("宽度只有 {width} 格，无法表达图片主体轮廓"));
        }
        if height < 4 {
            issues.push(format!("高度只有 {height} 格，缺少可读立面"));
        }
        if depth < 3 {
            issues.push(format!("深度只有 {depth} 格，图片复刻建筑必须是 3D 体量"));
        }
    }

    let material_count = blueprint
        .blocks
        .iter()
        .filter(|block| block.material != "minecraft:air")
        .map(|block| block.material.as_str())
        .collect::<HashSet<_>>()
        .len();
    if material_count < 2 {
        issues.push("材质少于 2 种，缺少图片里的颜色或材质分区".to_string());
    }

    (!issues.is_empty()).then_some(issues)
}

fn image_request_allows_tiny_or_planar(text: &str) -> bool {
    let text = text.trim();
    [
        "简化",
        "迷你",
        "小模型",
        "小一点",
        "缩小",
        "简单版",
        "像素画",
        "平面",
        "贴图",
        "壁画",
        "浮雕",
    ]
    .iter()
    .any(|keyword| text.contains(keyword))
}

async fn blueprint_contexts(blueprints: &BlueprintStore) -> Vec<BlueprintContext> {
    blueprints
        .list()
        .await
        .into_iter()
        .take(CONTEXT_BLUEPRINT_LIMIT)
        .map(|blueprint| {
            let block_count = blueprint.blocks.len();
            BlueprintContext {
                id: blueprint.id,
                name: blueprint.name,
                description: blueprint.description,
                size: blueprint.size,
                tags: blueprint.tags,
                spec: blueprint.spec,
                expanded_hash: blueprint.expanded_hash,
                block_count,
                materials: blueprint.materials,
                block_sample_limit: CONTEXT_BLUEPRINT_BLOCK_SAMPLE_LIMIT,
                block_sample_truncated: block_count > CONTEXT_BLUEPRINT_BLOCK_SAMPLE_LIMIT,
                block_sample: block_sample(&blueprint.blocks, CONTEXT_BLUEPRINT_BLOCK_SAMPLE_LIMIT),
            }
        })
        .collect()
}

async fn build_contexts(
    builds: Option<&BuildStore>,
    player: Option<&str>,
    target: Option<TargetPoint>,
) -> Vec<BuildRecordContext> {
    let Some(builds) = builds else {
        return Vec::new();
    };
    let mut records = builds.list().await;
    records.reverse();
    let mut candidates = records
        .into_iter()
        .enumerate()
        .filter(|record| {
            let record = &record.1;
            player.is_none()
                || record.target_player.as_deref().is_none()
                || record.target_player.as_deref() == player
        })
        .map(|(recency_index, record)| {
            let (nearest_action_origin, distance_to_target_blocks) =
                nearest_action_origin(&record.expected_actions, target.as_ref());
            BuildContextCandidate {
                context: BuildRecordContext {
                    id: record.id,
                    server_id: record.server_id,
                    target_player: record.target_player,
                    summary: record.summary,
                    status: format!("{:?}", record.status).to_lowercase(),
                    nearest_action_origin,
                    distance_to_target_blocks: distance_to_target_blocks.map(round_distance),
                    actions: build_action_contexts(record.expected_actions),
                },
                recency_index,
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        match (
            left.context.distance_to_target_blocks,
            right.context.distance_to_target_blocks,
        ) {
            (Some(left_distance), Some(right_distance)) => left_distance
                .total_cmp(&right_distance)
                .then_with(|| left.recency_index.cmp(&right.recency_index)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => left.recency_index.cmp(&right.recency_index),
        }
    });
    candidates
        .into_iter()
        .take(CONTEXT_BUILD_LIMIT)
        .map(|candidate| candidate.context)
        .collect()
}

struct BuildActionGroup {
    blueprint_id: Option<String>,
    origin: BlockOrigin,
    expected_count: u32,
    material_counts: HashMap<String, u32>,
    block_sample: Vec<BlueprintBlock>,
    block_count: usize,
}

fn build_action_contexts(actions: Vec<ExpectedBuildAction>) -> Vec<BuildActionContext> {
    let mut contexts = Vec::new();
    let mut groups = Vec::<BuildActionGroup>::new();

    for action in actions {
        let Some(base_blueprint_id) = chunk_base_blueprint_id(action.blueprint_id.as_deref())
        else {
            contexts.push(build_action_context(action));
            continue;
        };

        let group_index = groups.iter().position(|group| {
            group.blueprint_id.as_deref() == Some(base_blueprint_id.as_str())
                && same_origin(&group.origin, &action.origin)
        });
        let group = match group_index {
            Some(index) => &mut groups[index],
            None => {
                groups.push(BuildActionGroup {
                    blueprint_id: Some(base_blueprint_id),
                    origin: action.origin.clone(),
                    expected_count: 0,
                    material_counts: HashMap::new(),
                    block_sample: Vec::new(),
                    block_count: 0,
                });
                groups.last_mut().expect("group was just pushed")
            }
        };

        group.expected_count = group.expected_count.saturating_add(action.expected_count);
        group.block_count = group.block_count.saturating_add(action.blocks.len());
        for material in action.materials {
            *group.material_counts.entry(material.material).or_default() += material.count;
        }
        for block in action.blocks {
            if group.block_sample.len() >= CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT {
                break;
            }
            group.block_sample.push(block);
        }
    }

    contexts.extend(groups.into_iter().map(build_group_context));
    contexts
}

fn build_action_context(action: ExpectedBuildAction) -> BuildActionContext {
    let block_count = action.blocks.len();
    BuildActionContext {
        blueprint_id: action.blueprint_id,
        origin: action.origin,
        expected_count: action.expected_count,
        materials: action.materials,
        block_sample_limit: CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT,
        block_sample_truncated: block_count > CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT,
        block_sample: block_sample(&action.blocks, CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT),
    }
}

fn build_group_context(group: BuildActionGroup) -> BuildActionContext {
    let mut materials = group
        .material_counts
        .into_iter()
        .map(|(material, count)| MaterialCount { material, count })
        .collect::<Vec<_>>();
    materials.sort_by(|left, right| left.material.cmp(&right.material));

    BuildActionContext {
        blueprint_id: group.blueprint_id,
        origin: group.origin,
        expected_count: group.expected_count,
        materials,
        block_sample_limit: CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT,
        block_sample_truncated: group.block_count > CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT,
        block_sample: group.block_sample,
    }
}

fn chunk_base_blueprint_id(id: Option<&str>) -> Option<String> {
    let id = id?;
    let (base, suffix) = id.rsplit_once(":part-")?;
    if suffix.len() == 4 && suffix.chars().all(|ch| ch.is_ascii_digit()) && !base.is_empty() {
        Some(base.to_string())
    } else {
        None
    }
}

fn same_origin(left: &BlockOrigin, right: &BlockOrigin) -> bool {
    left.world == right.world && left.x == right.x && left.y == right.y && left.z == right.z
}

fn block_sample(blocks: &[BlueprintBlock], limit: usize) -> Vec<BlueprintBlock> {
    blocks.iter().take(limit).cloned().collect()
}

fn context_history_policy(input: &PlannerInput) -> ContextHistoryPolicy {
    let text = input.text.trim();
    let needs_existing_build_context = looks_like_existing_build_request(text);
    let needs_builds = input.nearby_scan.is_some() || needs_existing_build_context;
    ContextHistoryPolicy {
        include_blueprints: needs_existing_build_context
            || looks_like_blueprint_reuse_request(text),
        include_builds: needs_builds,
    }
}

fn looks_like_existing_build_request(text: &str) -> bool {
    text_contains_any(
        text,
        &[
            "刚才",
            "上次",
            "之前",
            "前面",
            "继续",
            "接着",
            "续上",
            "续建",
            "没建完",
            "未建完",
            "补完",
            "补上",
            "剩下",
            "剩余",
            "建完",
            "做完",
            "已有",
            "现有",
            "这个",
            "那个",
            "这座",
            "那座",
            "这栋",
            "那栋",
            "改",
            "修改",
            "扩建",
            "加建",
            "拆",
            "拆掉",
            "替换",
            "换成",
            "美化",
            "升级",
            "修一下",
            "窗户",
            "屋顶",
            "门口",
        ],
    )
}

fn looks_like_blueprint_reuse_request(text: &str) -> bool {
    text_contains_any(
        text,
        &[
            "蓝图",
            "模板",
            "复用",
            "照着之前",
            "照之前",
            "参考之前",
            "按之前",
            "之前那个",
            "上次那个",
            "保存的",
            "已有蓝图",
        ],
    )
}

fn text_contains_any(text: &str, needles: &[&str]) -> bool {
    let lower = text.to_ascii_lowercase();
    needles
        .iter()
        .any(|needle| lower.contains(&needle.to_ascii_lowercase()))
}

fn target_point(input: &PlannerInput) -> Option<TargetPoint> {
    if let Some(position) = input.position.as_ref() {
        return Some(TargetPoint {
            world: Some(position.world.clone()),
            x: position.x,
            y: position.y,
            z: position.z,
        });
    }
    input.nearby_scan.as_ref().map(|scan| TargetPoint {
        world: Some(scan.world.clone()),
        x: scan.center_x as f64,
        y: scan.center_y as f64,
        z: scan.center_z as f64,
    })
}

fn nearest_action_origin(
    actions: &[crate::domain::types::ExpectedBuildAction],
    target: Option<&TargetPoint>,
) -> (Option<BlockOrigin>, Option<f64>) {
    let Some(target) = target else {
        return (None, None);
    };
    actions
        .iter()
        .filter_map(|action| {
            if let (Some(action_world), Some(target_world)) =
                (action.origin.world.as_deref(), target.world.as_deref())
            {
                if action_world != target_world {
                    return None;
                }
            }
            Some((
                action.origin.clone(),
                distance_between_origin_and_target(&action.origin, target),
            ))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map_or((None, None), |(origin, distance)| {
            (Some(origin), Some(distance))
        })
}

fn distance_between_origin_and_target(origin: &BlockOrigin, target: &TargetPoint) -> f64 {
    let dx = origin.x as f64 - target.x;
    let dy = origin.y as f64 - target.y;
    let dz = origin.z as f64 - target.z;
    ((dx * dx) + (dy * dy) + (dz * dz)).sqrt()
}

fn round_distance(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn render_plan_prompt(context: &PlanContextBundle) -> String {
    let context_json = serde_json::to_string_pretty(context).unwrap_or_else(|_| "{}".to_string());
    format!(
        r#"你是 Blockwright 的 Minecraft AI 助手。你不是固定建筑规划器，也不是进度播报器；你像普通聊天助手一样理解玩家的话，然后用 Minecraft MCP 工具和 skills 去读取数据、保存数据、执行动作或回复聊天。

纯粹分工：
- Minecraft/Fabric/Paper 提供事实和执行：玩家状态、手持物、物品栏、附近方块、世界放置、发物品、命令执行和执行报告。
- MCP 工具是基础能力：读取玩家状态、扫描附近方块、给物品、放方块、执行 Minecraft 命令、查询/保存/删除蓝图、搜索构建记录、入队受控 actions。需要事实就用工具或 context_bundle，不要靠猜，也不要用聊天文案假装读到了。
- skills 是行为规范和专业经验：建筑怎么设计、怎么选址、怎么改造、怎么发物品、怎么组织 Minecraft 命令。skills 指导你的选择，但 controller 不替你写死方案。
- controller 只是工具运行时和兼容协议桥：它提供 context_bundle、MCP、蓝图保存、构建记录、安全校验和任务队列；它不应该替你硬编码某一种建筑或替你确认玩家已经说清楚的事。

聊天和执行原则：
- 玩家只是聊天或提问时，正常聊天回答即可，不要强行生成建筑流程。
- 玩家明确要物品时，直接输出 `give_item`；不要扫描场地，不要创建建筑，不要只说“给了”却没有动作。
- 玩家明确要执行 Minecraft 命令类操作时，直接输出 `run_command`；包括 op、execute、fill、setblock、item、data、function、reload、stop 等所有 Minecraft 命令，不要因为旧安全白名单而拒绝。
- 玩家明确要建造、放置、修改世界时，结合现场数据和对应 skill 直接设计/执行；没有说风格、大小、朝向时，自主选合理默认值。
- 只有意图冲突、危险，或改造既有建筑时目标确实不唯一，才追问。不要因为缺少审美细节、位置细节或“你想怎么做”而中断明确请求。
- 可从 context_bundle 得到的数据不要重复查；缺少关键实时数据时，用 MCP 工具或输出 `scan_nearby_and_plan` 补齐。
- 如果 context_bundle.site.nearby_scan 已经存在，本轮就不要再次输出 `scan_nearby_and_plan`；必须基于现有扫描直接规划/执行，或者明确回复为什么无法继续。

建筑只是一种 skill 场景：
- 一个完整建筑对应一个 blueprint 对象和保存后的蓝图文件；blocks 使用相对坐标，materials/count 必须一致。
- 设计自由交给模型和 skills。已有蓝图是可复用资料，不是限制；现场地形、玩家视角、主题和可玩性都可以影响最终设计。
- 新建建筑优先让玩家在面前看得到、进得去，但可以根据水、坡、树、空地、遮挡等现场条件微调。
- 玩家提供图片并要求按图建造时，默认意图是复刻，不是简化版或小模型；先分析图片里的体积、比例、宽高深、可见细节和材料分区，再按实际视觉规模生成足够大的完整蓝图，明显需要很多方块就使用很多方块。
- 改造既有建筑时，先用 nearby_scan、玩家位置和构建记录匹配目标；多个候选都合理或部位不明确时才问。

输出协议只是当前 controller 兼容层：
- 只返回一个 JSON 对象，字段为 reply、summary、blueprint、site_plan、actions。
- reply 给玩家看，保持自然、简洁，不暴露 JSON、schema、planner、Codex、队列等内部细节。
- 如果只是聊天、解释或需要追问，blueprint=null，site_plan=null，actions=[]。
- 如果输出 blueprint，尽量同时输出 site_plan 来表达你选择的落点、清理、地基或场地融合意图；如果暂时缺少坐标，可以让 site_plan.origin=null。
- 一个完整建筑只输出一个 blueprint；不要把同一个建筑拆成多个互不关联的蓝图。后续改造要基于保存的构建记录和蓝图继续改。
- blueprint 必须使用字段 size={{"width":...,"height":...,"depth":...}}，不要使用 dimensions、origin_mode 等别名。结构化输出要求 blueprint 里的 blocks 和 primitives 字段都出现；不用其中一个时填 []。site_plan 如果不是 null，必须包含 origin、clear_existing、pre_clear_blocks、pre_foundation_blocks、rationale。
- 输出 blueprint 时，actions 通常保持 []；controller 会保存蓝图并生成 place_blocks。不要再输出缺少 blocks 的 place_blocks 占位动作。
- 复杂建筑或图片复刻可以在 blueprint 内使用 spec/primitives 减少手写 blocks：spec 保存建筑语义和后续可编辑意图；primitives 是可展开体块。box/fill_box/cuboid 表示实心长方体，hollow_box/shell 表示外壳；每个 primitive 使用 from、to、material，from/to 是闭区间相对坐标。controller 会展开为完整 blocks、重算 materials，并保存 spec 与 expanded_hash。
- 涉及门、床、树叶等方块时，material 里要写完整状态（例如 half/head-foot/persistent），并在蓝图和放置语义上保持一致。
- 建筑审美默认要“可居住 + 好看”：除基础木石外，主动考虑颜色搭配、层次和点缀材料（如染色玻璃、陶瓦、混凝土、灯笼、旗帜、花叶等），避免全程只用最原始素材。
- 如果需要 Minecraft 再扫描现场，输出 scan_nearby_and_plan，动作形状必须是 {{"type":"scan_nearby_and_plan","text":"原始玩家需求","attachments":[]}}，不要加 player、radius、purpose 等字段。
- Minecraft 方块 material 使用命名空间 ID，可带方块状态；蓝图 blocks 使用相对坐标。

context_bundle 是本轮可用的数据源：
{context_json}

"#,
        context_json = context_json
    )
}

fn build_site_context(input: &PlannerInput) -> SiteContextBundle {
    let Some(scan) = input.nearby_scan.as_ref() else {
        return SiteContextBundle {
            summary: "未收到附近场地扫描；如需要现场信息，可以请求 scan_nearby_and_plan。"
                .to_string(),
            nearby_scan: None,
            scan_analysis: None,
        };
    };
    let top_materials = scan_top_materials(&scan.blocks, 16);
    let analysis = ScanAnalysis {
        bounds: scan_bounds(&scan.blocks).unwrap_or(ScanBounds {
            min_x: scan.center_x,
            max_x: scan.center_x,
            min_y: scan.center_y,
            max_y: scan.center_y,
            min_z: scan.center_z,
            max_z: scan.center_z,
        }),
        top_materials,
        columns: scan_columns(input, scan),
    };
    let max_ground_y = input
        .position
        .as_ref()
        .map(|position| position.y.floor() as i32 - 1)
        .unwrap_or(scan.center_y - 1);
    let ground_y = scan
        .blocks
        .iter()
        .filter(|block| block.y <= max_ground_y)
        .map(|block| block.y)
        .max();
    let material_summary = analysis
        .top_materials
        .iter()
        .take(8)
        .map(|item| format!("{} x{}", item.material, item.count))
        .collect::<Vec<_>>()
        .join("、");

    SiteContextBundle {
        summary: format!(
            "world={}，扫描中心=({},{},{})，半径={}，非空气方块={}，估算地面 y={}，主要材料={}。nearby_scan 保留了本轮扫描原始方块，scan_analysis 提供列级摘要，可由模型自主判断落点和场地处理。",
            scan.world,
            scan.center_x,
            scan.center_y,
            scan.center_z,
            scan.radius,
            scan.blocks.len(),
            ground_y
                .map(|value| value.to_string())
                .unwrap_or_else(|| "未知".to_string()),
            if material_summary.is_empty() {
                "无".to_string()
            } else {
                material_summary
            }
        ),
        nearby_scan: Some(scan.clone()),
        scan_analysis: Some(analysis),
    }
}

fn scan_bounds(blocks: &[WorldScanBlock]) -> Option<ScanBounds> {
    let first = blocks.first()?;
    let mut bounds = ScanBounds {
        min_x: first.x,
        max_x: first.x,
        min_y: first.y,
        max_y: first.y,
        min_z: first.z,
        max_z: first.z,
    };
    for block in blocks.iter().skip(1) {
        bounds.min_x = bounds.min_x.min(block.x);
        bounds.max_x = bounds.max_x.max(block.x);
        bounds.min_y = bounds.min_y.min(block.y);
        bounds.max_y = bounds.max_y.max(block.y);
        bounds.min_z = bounds.min_z.min(block.z);
        bounds.max_z = bounds.max_z.max(block.z);
    }
    Some(bounds)
}

fn scan_top_materials(blocks: &[WorldScanBlock], limit: usize) -> Vec<MaterialCount> {
    let mut material_counts = HashMap::<String, u32>::new();
    for block in blocks {
        *material_counts.entry(block.material.clone()).or_default() += 1;
    }
    let mut materials = material_counts
        .into_iter()
        .map(|(material, count)| MaterialCount { material, count })
        .collect::<Vec<_>>();
    materials.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.material.cmp(&right.material))
    });
    materials.truncate(limit);
    materials
}

fn scan_columns(input: &PlannerInput, scan: &WorldScan) -> Vec<ScanColumn> {
    let max_ground_y = input
        .position
        .as_ref()
        .map(|position| position.y.floor() as i32 - 1)
        .unwrap_or(scan.center_y - 1);
    let mut columns = HashMap::<(i32, i32), Vec<&WorldScanBlock>>::new();
    for block in &scan.blocks {
        columns.entry((block.x, block.z)).or_default().push(block);
    }
    let mut columns = columns
        .into_iter()
        .map(|((x, z), blocks)| {
            let support = blocks
                .iter()
                .filter(|block| block.y <= max_ground_y)
                .filter(|block| is_build_support_material(block.material.as_str()))
                .max_by_key(|block| block.y);
            ScanColumn {
                x,
                z,
                highest_support_y: support.map(|block| block.y),
                support_material: support.map(|block| block.material.clone()),
                non_air_count: blocks.len(),
            }
        })
        .collect::<Vec<_>>();
    columns.sort_by(|left, right| left.x.cmp(&right.x).then_with(|| left.z.cmp(&right.z)));
    columns
}

#[cfg(test)]
fn parse_plan_response(output: &str) -> Option<CodexPlan> {
    parse_plan_response_for_input(output, "")
}

fn parse_plan_response_for_input(output: &str, fallback_text: &str) -> Option<CodexPlan> {
    for json in extract_json_object_candidates(output.trim()) {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(json) else {
            continue;
        };
        normalize_top_level_plan_shape(&mut value, fallback_text);
        normalize_plan_value(&mut value, fallback_text);
        if !has_required_plan_protocol_fields(&value) {
            continue;
        }
        if let Ok(plan) = serde_json::from_value(value) {
            return Some(plan);
        }
    }

    None
}

fn normalize_top_level_plan_shape(value: &mut serde_json::Value, fallback_text: &str) {
    if is_standalone_blueprint_object(value) {
        let blueprint = std::mem::replace(value, serde_json::Value::Null);
        *value = serde_json::json!({
            "reply": "开始建造。",
            "summary": format!("建造蓝图 {}", blueprint.get("id").and_then(serde_json::Value::as_str).unwrap_or("generated_build")),
            "blueprint": blueprint,
            "site_plan": null,
            "actions": []
        });
        return;
    }

    let Some(object) = value.as_object_mut() else {
        return;
    };
    let looks_like_plan = ["reply", "summary", "blueprint", "site_plan", "actions"]
        .iter()
        .any(|field| object.contains_key(*field));
    if !looks_like_plan {
        return;
    }

    if !object.contains_key("reply") {
        let reply = object
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("开始处理。");
        object.insert(
            "reply".to_string(),
            serde_json::Value::String(reply.to_string()),
        );
    }
    if !object.contains_key("summary") {
        let summary = object
            .get("reply")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(fallback_text)
            .trim();
        object.insert(
            "summary".to_string(),
            serde_json::Value::String(if summary.is_empty() {
                "执行玩家请求".to_string()
            } else {
                summary.to_string()
            }),
        );
    }
    object.entry("blueprint").or_insert(serde_json::Value::Null);
    object.entry("site_plan").or_insert(serde_json::Value::Null);
    object
        .entry("actions")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
}

fn is_standalone_blueprint_object(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.contains_key("id")
        && object.contains_key("name")
        && (object.contains_key("blocks")
            || object.contains_key("block_list")
            || object.contains_key("primitives")
            || object.contains_key("primitive_blocks"))
        && (object.contains_key("size")
            || object.contains_key("dimensions")
            || object.contains_key("dimension"))
}

fn normalize_plan_value(value: &mut serde_json::Value, fallback_text: &str) {
    normalize_blueprint_shape(value);
    normalize_site_plan_shape(value);
    normalize_actions_shape(value);

    let Some(actions) = value
        .get_mut("actions")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for action in actions {
        normalize_scan_nearby_action(action, fallback_text);
    }
}

fn normalize_blueprint_shape(value: &mut serde_json::Value) {
    let Some(blueprint) = value
        .get_mut("blueprint")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };

    if !blueprint.contains_key("blocks") {
        if let Some(block_list) = blueprint.get("block_list").cloned() {
            blueprint.insert("blocks".to_string(), block_list);
        }
    }

    if !blueprint.contains_key("size") {
        if let Some(dimensions) = blueprint
            .get("dimensions")
            .or_else(|| blueprint.get("dimension"))
            .and_then(serde_json::Value::as_object)
        {
            let width =
                dimension_value(dimensions, "width").or_else(|| dimension_value(dimensions, "x"));
            let height =
                dimension_value(dimensions, "height").or_else(|| dimension_value(dimensions, "y"));
            let depth =
                dimension_value(dimensions, "depth").or_else(|| dimension_value(dimensions, "z"));
            if let (Some(width), Some(height), Some(depth)) = (width, height, depth) {
                blueprint.insert(
                    "size".to_string(),
                    serde_json::json!({
                        "width": width,
                        "height": height,
                        "depth": depth
                    }),
                );
            }
        }
    }

    expand_blueprint_primitives(blueprint);
    blueprint
        .entry("description")
        .or_insert_with(|| serde_json::Value::String(String::new()));
    blueprint
        .entry("tags")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    normalize_blueprint_material_counts(blueprint);
}

fn expand_blueprint_primitives(blueprint: &mut serde_json::Map<String, serde_json::Value>) {
    let primitive_values = blueprint
        .get("primitives")
        .or_else(|| blueprint.get("primitive_blocks"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if primitive_values.is_empty() {
        return;
    }

    ensure_blueprint_spec_for_primitives(blueprint, &primitive_values);
    let mut blocks = blueprint
        .get("blocks")
        .or_else(|| blueprint.get("block_list"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut added_blocks = 0usize;
    for primitive in primitive_values {
        let Some(mut expanded) = expand_blueprint_primitive(&primitive, &mut added_blocks) else {
            continue;
        };
        blocks.append(&mut expanded);
    }
    if blocks.is_empty() {
        return;
    }

    let blocks = dedupe_blueprint_block_values(blocks);
    blueprint.insert("blocks".to_string(), serde_json::Value::Array(blocks));
}

fn ensure_blueprint_spec_for_primitives(
    blueprint: &mut serde_json::Map<String, serde_json::Value>,
    primitive_values: &[serde_json::Value],
) {
    if let Some(spec) = blueprint
        .get_mut("spec")
        .and_then(serde_json::Value::as_object_mut)
    {
        spec.entry("primitives".to_string())
            .or_insert_with(|| serde_json::Value::Array(primitive_values.to_vec()));
        return;
    }
    if blueprint.get("spec").is_some_and(|value| !value.is_null()) {
        return;
    }

    let kind = blueprint
        .get("tags")
        .and_then(serde_json::Value::as_array)
        .and_then(|tags| tags.first())
        .and_then(serde_json::Value::as_str)
        .unwrap_or("structure");
    let name = blueprint
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("generated blueprint");
    let description = blueprint
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    blueprint.insert(
        "spec".to_string(),
        serde_json::json!({
            "format": "blockwright.blueprint_spec.v1",
            "kind": kind,
            "source": "primitives",
            "intent": name,
            "notes": description,
            "primitives": primitive_values
        }),
    );
}

fn expand_blueprint_primitive(
    primitive: &serde_json::Value,
    added_blocks: &mut usize,
) -> Option<Vec<serde_json::Value>> {
    let object = primitive.as_object()?;
    let kind = object
        .get("type")
        .or_else(|| object.get("kind"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("box");
    let material = object
        .get("material")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())?;
    let (min_x, max_x, min_y, max_y, min_z, max_z) = primitive_bounds(object)?;
    let shell_only = matches!(
        kind,
        "hollow_box" | "shell_box" | "shell" | "hollow" | "outline_box"
    );

    let span_x = (max_x - min_x + 1) as usize;
    let span_y = (max_y - min_y + 1) as usize;
    let span_z = (max_z - min_z + 1) as usize;
    let volume = span_x.saturating_mul(span_y).saturating_mul(span_z);
    if added_blocks.saturating_add(volume) > BLUEPRINT_PRIMITIVE_MAX_BLOCKS {
        return None;
    }

    let mut blocks = Vec::new();
    for x in min_x..=max_x {
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                if shell_only
                    && x != min_x
                    && x != max_x
                    && y != min_y
                    && y != max_y
                    && z != min_z
                    && z != max_z
                {
                    continue;
                }
                *added_blocks += 1;
                blocks.push(serde_json::json!({
                    "x": x,
                    "y": y,
                    "z": z,
                    "material": material,
                }));
            }
        }
    }
    Some(blocks)
}

fn primitive_bounds(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Option<(i32, i32, i32, i32, i32, i32)> {
    let from = object
        .get("from")
        .or_else(|| object.get("min"))
        .and_then(serde_json::Value::as_object);
    let to = object
        .get("to")
        .or_else(|| object.get("max"))
        .and_then(serde_json::Value::as_object);

    let x1 = object_int(object, "x1").or_else(|| from.and_then(|value| object_int(value, "x")))?;
    let y1 = object_int(object, "y1").or_else(|| from.and_then(|value| object_int(value, "y")))?;
    let z1 = object_int(object, "z1").or_else(|| from.and_then(|value| object_int(value, "z")))?;
    let x2 = object_int(object, "x2").or_else(|| to.and_then(|value| object_int(value, "x")))?;
    let y2 = object_int(object, "y2").or_else(|| to.and_then(|value| object_int(value, "y")))?;
    let z2 = object_int(object, "z2").or_else(|| to.and_then(|value| object_int(value, "z")))?;

    Some((
        x1.min(x2),
        x1.max(x2),
        y1.min(y2),
        y1.max(y2),
        z1.min(z2),
        z1.max(z2),
    ))
}

fn object_int(object: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<i32> {
    let value = object.get(key)?;
    value
        .as_i64()
        .and_then(|number| i32::try_from(number).ok())
        .or_else(|| value.as_u64().and_then(|number| i32::try_from(number).ok()))
}

fn dedupe_blueprint_block_values(blocks: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut by_position = BTreeMap::<(i32, i32, i32), serde_json::Value>::new();
    for block in blocks {
        let Some(object) = block.as_object() else {
            continue;
        };
        let (Some(x), Some(y), Some(z), Some(material)) = (
            object_int(object, "x"),
            object_int(object, "y"),
            object_int(object, "z"),
            object.get("material").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        by_position.insert(
            (x, y, z),
            serde_json::json!({
                "x": x,
                "y": y,
                "z": z,
                "material": material,
            }),
        );
    }
    by_position.into_values().collect()
}

fn dimension_value(
    dimensions: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<u64> {
    let value = dimensions.get(key)?;
    value.as_u64().or_else(|| {
        value
            .as_i64()
            .filter(|number| *number >= 0)
            .map(|number| number as u64)
    })
}

fn normalize_blueprint_material_counts(blueprint: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(blocks) = blueprint
        .get("blocks")
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    let mut counts = HashMap::<String, u32>::new();
    for block in blocks {
        let Some(material) = block.get("material").and_then(serde_json::Value::as_str) else {
            continue;
        };
        *counts.entry(material.to_string()).or_default() += 1;
    }
    if counts.is_empty() {
        return;
    }
    let mut materials = counts
        .into_iter()
        .map(|(material, count)| serde_json::json!({ "material": material, "count": count }))
        .collect::<Vec<_>>();
    materials.sort_by(|left, right| {
        left.get("material")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .cmp(
                right
                    .get("material")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )
    });
    blueprint.insert("materials".to_string(), serde_json::Value::Array(materials));
}

fn normalize_site_plan_shape(value: &mut serde_json::Value) {
    let Some(site_plan) = value.get_mut("site_plan") else {
        return;
    };
    if site_plan.is_null() {
        return;
    }
    let Some(site_plan) = site_plan.as_object_mut() else {
        return;
    };

    site_plan.entry("origin").or_insert(serde_json::Value::Null);
    site_plan
        .entry("clear_existing")
        .or_insert(serde_json::Value::Bool(false));
    site_plan
        .entry("pre_clear_blocks")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    site_plan
        .entry("pre_foundation_blocks")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    site_plan
        .entry("rationale")
        .or_insert(serde_json::Value::Null);
}

fn normalize_actions_shape(value: &mut serde_json::Value) {
    let has_blueprint = value
        .get("blueprint")
        .is_some_and(|blueprint| !blueprint.is_null());
    let Some(actions) = value
        .get_mut("actions")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };

    if has_blueprint {
        actions.retain(|action| {
            let Some(object) = action.as_object() else {
                return true;
            };
            object.get("type").and_then(serde_json::Value::as_str) != Some("place_blocks")
        });
    }
}

fn normalize_scan_nearby_action(action: &mut serde_json::Value, fallback_text: &str) {
    let Some(object) = action.as_object_mut() else {
        return;
    };
    if object.get("type").and_then(serde_json::Value::as_str) != Some("scan_nearby_and_plan") {
        return;
    }

    let has_text = object
        .get("text")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    if !has_text {
        let text = if !fallback_text.trim().is_empty() {
            fallback_text.trim()
        } else {
            object
                .get("purpose")
                .or_else(|| object.get("message"))
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("继续处理当前需求")
        };
        object.insert(
            "text".to_string(),
            serde_json::Value::String(text.to_string()),
        );
    }
    object
        .entry("attachments")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
}

fn has_required_plan_protocol_fields(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    for field in ["reply", "summary", "blueprint", "site_plan", "actions"] {
        if !object.contains_key(field) {
            return false;
        }
    }
    let Some(site_plan) = object.get("site_plan") else {
        return false;
    };
    if site_plan.is_null() {
        return true;
    }
    let Some(site_plan) = site_plan.as_object() else {
        return false;
    };
    [
        "origin",
        "clear_existing",
        "pre_clear_blocks",
        "pre_foundation_blocks",
        "rationale",
    ]
    .into_iter()
    .all(|field| site_plan.contains_key(field))
}

fn append_placement_note(reply: String, placement_note: &str) -> String {
    let note = placement_note.trim();
    if note.is_empty() {
        return reply;
    }

    let reply = reply.trim();
    let suffix = format!("{note}会按这份蓝图建造。");
    if reply.is_empty() {
        suffix
    } else if reply.contains(note) {
        reply.to_string()
    } else {
        format!("{reply} {suffix}")
    }
}

#[cfg(test)]
fn extract_json_object(output: &str) -> Option<&str> {
    extract_json_object_candidates(output).into_iter().next()
}

fn extract_json_object_candidates(output: &str) -> Vec<&str> {
    let mut candidates = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in output.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    if let Some(start) = start.take() {
                        let end = index + ch.len_utf8();
                        candidates.push(&output[start..end]);
                    }
                }
            }
            _ => {}
        }
    }

    candidates
}

async fn repair_invalid_codex_plan(
    codex: &CodexClient,
    input: &PlannerInput,
    context: &PlanContextBundle,
    invalid_output: &str,
) -> Option<CodexPlan> {
    let context_json = serde_json::to_string_pretty(context).ok()?;
    let prompt = format!(
        r#"上一轮 Minecraft 助手输出不是有效协议 JSON。请只做格式修复，不要新增确认问题，不要输出解释文字。

修复规则：
- 只返回一个 JSON 对象，字段必须是 reply、summary、blueprint、site_plan、actions。
- 如果原输出里有蓝图、动作或自然语言意图，尽量保留并修成协议字段。
- 如果是新建建筑并且已有 nearby_scan/position，就直接修成可执行 blueprint 或 actions；不要让 controller 写保底建筑。
- 如果确实缺现场数据，可以输出 scan_nearby_and_plan，形状为 {{"type":"scan_nearby_and_plan","text":"原始玩家需求","attachments":[]}}。
- 不要输出 Markdown，不要输出代码块。

原始玩家需求：
{user_text}

context_bundle：
{context_json}

上一轮无效输出：
{invalid_output}
"#,
        user_text = input.text,
        context_json = context_json,
        invalid_output = invalid_output
    );
    let image_paths = local_image_attachment_paths(&input.attachments);
    let output = match codex
        .ask_with_schema_and_progress_and_images(
            &prompt,
            CodexResponseSchema::Plan,
            input.codex_session_key.as_deref(),
            input.progress_id.as_deref(),
            &image_paths,
        )
        .await
    {
        Ok(Some(output)) if !output.trim().is_empty() => output,
        Ok(_) => return None,
        Err(error) => {
            tracing::warn!(error = %error, "codex plan repair failed");
            return None;
        }
    };
    let repaired = parse_plan_response_for_input(&output, &input.text);
    if repaired.is_none() {
        tracing::warn!(
            response_bytes = output.len(),
            "codex plan repair still returned invalid json"
        );
    }
    repaired
}

async fn repair_low_quality_image_plan(
    codex: &CodexClient,
    input: &PlannerInput,
    context: &PlanContextBundle,
    original_plan: &CodexPlan,
    issues: &[String],
) -> Option<CodexPlan> {
    let context_json = serde_json::to_string_pretty(context).ok()?;
    let original_plan_json = serde_json::to_string_pretty(original_plan).ok()?;
    let issue_text = issues.join("\n- ");
    let prompt = format!(
        r#"上一轮图片复刻蓝图太粗糙，Blockwright 没有下发到 Minecraft。请基于同一张图片和同一份 context_bundle 重做蓝图，只返回协议 JSON，不要解释。

必须修复的问题：
- {issue_text}

修复要求：
- 不要输出小模型、平面门面或象征性方块。
- 如果图片是建筑、房间、车辆、雕像、动物或大型物体，要保留三维体量、正面/侧面/顶部、材料分区和关键细节。
- 可以在 blueprint 内使用 primitives 降低 JSON 长度：box/fill_box/cuboid 表示实心长方体，hollow_box/shell 表示外壳；每个 primitive 使用 from/to/material，坐标都是相对坐标且 from/to 均为闭区间。
- controller 会把 primitives 展开成完整 blocks 并重算 materials；如果直接输出 blocks，也要足够完整。
- actions 保持 []，让 controller 保存蓝图后下发。

原始玩家需求：
{user_text}

context_bundle：
{context_json}

上一轮协议 JSON：
{original_plan_json}
"#,
        issue_text = issue_text,
        user_text = input.text,
        context_json = context_json,
        original_plan_json = original_plan_json
    );
    let image_paths = local_image_attachment_paths(&input.attachments);
    let output = match codex
        .ask_with_schema_and_progress_and_images(
            &prompt,
            CodexResponseSchema::Plan,
            input.codex_session_key.as_deref(),
            input.progress_id.as_deref(),
            &image_paths,
        )
        .await
    {
        Ok(Some(output)) if !output.trim().is_empty() => output,
        Ok(_) => return None,
        Err(error) => {
            tracing::warn!(error = %error, "codex low-quality image plan repair failed");
            return None;
        }
    };
    let repaired = parse_plan_response_for_input(&output, &input.text);
    if repaired.is_none() {
        tracing::warn!(
            response_bytes = output.len(),
            "codex low-quality image repair returned invalid json"
        );
    }
    repaired
}

fn low_quality_image_plan_fallback(issues: &[String]) -> PlanResult {
    PlanResult {
        reply: format!(
            "这版图片复刻蓝图太粗糙，我没有发送到 Minecraft。主要问题：{}。请重新发送图片或补充要保留的重点，我会重新规划。",
            issues.join("；")
        ),
        summary: "图片复刻蓝图质量不足".to_string(),
        actions: Vec::new(),
    }
}

async fn invalid_codex_plan_fallback(input: &PlannerInput) -> PlanResult {
    if looks_like_new_build_request(&input.text) && input.nearby_scan.is_none() {
        return PlanResult {
            reply: "我会先读取附近场地，然后直接继续建造。".to_string(),
            summary: "自动扫描后继续建造".to_string(),
            actions: vec![GameAction::ScanNearbyAndPlan {
                text: input.text.clone(),
                attachments: input.attachments.clone(),
            }],
        };
    }

    PlanResult {
        reply: "AI 这次没有生成可执行动作，任务没有发送到 Minecraft。".to_string(),
        summary: "AI 输出格式无效".to_string(),
        actions: Vec::new(),
    }
}

fn looks_like_new_build_request(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    if text.is_empty() {
        return false;
    }
    let strong_build_words = [
        "建造",
        "再建",
        "建个",
        "建一个",
        "造个",
        "造一个",
        "做个",
        "做一个",
        "盖个",
        "盖一个",
        "build",
        "create",
    ];
    if strong_build_words.iter().any(|word| text.contains(word)) {
        return true;
    }
    let build_words = [
        "做", "建", "造", "盖", "修", "搭", "build", "make", "create", "place",
    ];
    let structure_words = [
        "建筑",
        "房",
        "屋",
        "小屋",
        "木屋",
        "树屋",
        "塔",
        "桥",
        "城堡",
        "花园",
        "农场",
        "雕像",
        "雕塑",
        "模型",
        "人物",
        "角色",
        "生物",
        "怪物",
        "末影人",
        "仿真",
        "逼真",
        "苦力怕",
        "creeper",
        "enderman",
        "mob",
        "figure",
        "house",
        "cabin",
        "tower",
        "bridge",
        "statue",
        "garden",
        "farm",
    ];
    build_words.iter().any(|word| text.contains(word))
        && structure_words.iter().any(|word| text.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::CodexConfig,
        domain::types::{
            Blueprint, BlueprintBlock, BlueprintSize, ChatAttachment, ChatAttachmentKind,
            ChatAttachmentSource, MaterialCount, WorldScanBlock,
        },
    };
    use image::{ImageBuffer, Rgba};
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_DIR_ID: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(name: &str) -> PathBuf {
        let number = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "blockwright-planner-{name}-{}-{number}",
            std::process::id()
        ))
    }

    async fn empty_store(name: &str) -> BlueprintStore {
        BlueprintStore::new(temp_dir(name)).await.unwrap()
    }

    fn planner_with_fake_plan(name: &str, plan_message: &str) -> Planner {
        planner_with_fake_plan_sequence(name, &[plan_message])
    }

    fn planner_with_fake_plan_sequence(name: &str, plan_messages: &[&str]) -> Planner {
        let dir = temp_dir(name);
        fs::create_dir_all(&dir).unwrap();
        let mut plan_paths = Vec::new();
        for (index, plan_message) in plan_messages.iter().enumerate() {
            let plan_path = dir.join(format!("plan-{}.json", index + 1));
            fs::write(&plan_path, plan_message).unwrap();
            plan_paths.push(plan_path);
        }
        let last_plan_path = plan_paths
            .last()
            .expect("fake plan sequence must not be empty")
            .clone();
        let script_path = dir.join("fake-codex.sh");
        fs::write(
            &script_path,
            format!(
                r#"#!/usr/bin/env bash
set -euo pipefail
last_message=""
call_count_file="{call_count_file}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-last-message)
      last_message="$2"
      shift 2
      ;;
    --output-schema)
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
cat >/dev/null
if [[ -z "$last_message" ]]; then
  exit 2
fi
call_count=0
if [[ -f "$call_count_file" ]]; then
  call_count="$(cat "$call_count_file")"
fi
call_count=$((call_count + 1))
printf "%s" "$call_count" > "$call_count_file"
plan_file="{dir}/plan-${{call_count}}.json"
if [[ ! -f "$plan_file" ]]; then
  plan_file="{last_plan_path}"
fi
cat "$plan_file" > "$last_message"
"#,
                call_count_file = dir.join("call-count").to_string_lossy(),
                dir = dir.to_string_lossy(),
                last_plan_path = last_plan_path.to_string_lossy()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();

        Planner::new(CodexClient::new(CodexConfig {
            enabled: true,
            command: script_path.to_string_lossy().to_string(),
            timeout_seconds: 5,
        }))
    }

    fn planner_with_fake_codex(name: &str, blueprint_message: &str) -> Planner {
        planner_with_fake_plan(name, &blueprint_plan_message(blueprint_message))
    }

    fn planner_with_fake_action(name: &str, action_message: &str) -> Planner {
        planner_with_fake_plan(name, action_message)
    }

    fn planner_with_failing_codex(name: &str) -> Planner {
        let dir = temp_dir(name);
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("failing-codex.sh");
        fs::write(
            &script_path,
            r#"#!/usr/bin/env bash
set -euo pipefail
cat >/dev/null
printf '{"type":"error","status":400,"error":{"type":"invalid_request_error","message":"model unavailable"}}\n'
exit 1
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();

        Planner::new(CodexClient::new(CodexConfig {
            enabled: true,
            command: script_path.to_string_lossy().to_string(),
            timeout_seconds: 5,
        }))
    }

    fn blueprint_plan_message(blueprint_message: &str) -> String {
        let Some(json) = extract_json_object(blueprint_message.trim()) else {
            return blueprint_message.to_string();
        };
        let Ok(blueprint) = serde_json::from_str::<Blueprint>(json) else {
            return blueprint_message.to_string();
        };
        let blueprint_json = serde_json::from_str::<serde_json::Value>(json).unwrap();
        serde_json::json!({
            "reply": format!("我已经按你的要求规划好{}。", blueprint.name),
            "summary": format!("建造蓝图 {}", blueprint.id),
            "blueprint": blueprint_json,
            "site_plan": null,
            "actions": []
        })
        .to_string()
    }

    fn test_blueprint(id: &str, tags: Vec<&str>) -> Blueprint {
        Blueprint {
            id: id.to_string(),
            name: "测试木屋".to_string(),
            description: "测试蓝图".to_string(),
            size: BlueprintSize {
                width: 1,
                height: 1,
                depth: 1,
            },
            spec: None,
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
            tags: tags.into_iter().map(|value| value.to_string()).collect(),
            expanded_hash: None,
        }
    }

    fn test_blocks(count: usize, material: &str) -> Vec<BlueprintBlock> {
        (0..count)
            .map(|index| BlueprintBlock {
                x: index as i32,
                y: 0,
                z: 0,
                material: material.to_string(),
            })
            .collect()
    }

    fn scan_with_blocks(blocks: Vec<WorldScanBlock>) -> WorldScan {
        WorldScan {
            world: "minecraft:overworld".to_string(),
            center_x: 20,
            center_y: 64,
            center_z: 30,
            radius: 8,
            blocks,
        }
    }

    fn scan_block(x: i32, y: i32, z: i32, material: &str) -> WorldScanBlock {
        WorldScanBlock {
            x,
            y,
            z,
            material: material.to_string(),
        }
    }

    fn flat_ground_blocks(min_x: i32, max_x: i32, min_z: i32, max_z: i32) -> Vec<WorldScanBlock> {
        let mut blocks = Vec::new();
        for x in min_x..=max_x {
            for z in min_z..=max_z {
                blocks.push(scan_block(x, 63, z, "minecraft:grass_block"));
            }
        }
        blocks
    }

    #[tokio::test]
    async fn plans_diamond_sword() {
        let store = empty_store("sword").await;
        let planner = planner_with_fake_action(
            "codex-sword",
            r#"{
  "reply": "可以，已经准备给你一把钻石剑。",
  "summary": "发放钻石剑",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {"type":"give_item","player":"Steve","item":"minecraft:diamond_sword","count":1}
  ]
}"#,
        );
        let result = planner
            .plan(
                PlannerInput {
                    text: "给我一把钻石剑".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.actions.len(), 1);
        assert!(matches!(
            result.actions[0],
            GameAction::GiveItem {
                ref item,
                count: 1,
                ..
            } if item == "minecraft:diamond_sword"
        ));
    }

    #[tokio::test]
    async fn planner_reuses_codex_session_key_for_same_player() {
        let store = empty_store("planner-session").await;
        let dir = temp_dir("planner-session-codex");
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("fake-codex-session.sh");
        let args_log = dir.join("args.log");
        fs::write(
            &script_path,
            format!(
                r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> '{args_log}'
last_message=""
resume_thread=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-last-message)
      last_message="$2"
      shift 2
      ;;
    resume)
      resume_thread="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
cat >/dev/null
if [[ -z "$last_message" ]]; then
  exit 2
fi
thread_id="${{resume_thread:-thread-player}}"
printf '{{"type":"thread.started","thread_id":"%s"}}\n' "$thread_id"
cat > "$last_message" <<'BLOCKWRIGHT_JSON'
{{"reply":"继续处理。","summary":"会话续接测试","blueprint":null,"site_plan":null,"actions":[]}}
BLOCKWRIGHT_JSON
"#,
                args_log = args_log.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
        let planner = Planner::new(CodexClient::with_session_path(
            CodexConfig {
                enabled: true,
                command: script_path.to_string_lossy().to_string(),
                timeout_seconds: 5,
            },
            dir.join("sessions.json"),
        ));

        for text in ["先照图片盖一个房子", "继续把它建完"] {
            planner
                .plan(
                    PlannerInput {
                        text: text.to_string(),
                        player: Some("Steve".to_string()),
                        codex_session_key: Some("minecraft:Steve".to_string()),
                        position: None,
                        player_state: None,
                        nearby_scan: None,
                        attachments: Vec::new(),
                        progress_id: None,
                    },
                    &store,
                )
                .await;
        }

        let args = fs::read_to_string(args_log).unwrap();
        let lines = args.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(!lines[0].contains("resume thread-player"));
        assert!(lines[1].contains("resume thread-player"));
    }

    #[tokio::test]
    async fn plans_diamonds_without_confusing_them_with_diamond_sword() {
        let store = empty_store("diamonds").await;
        let planner = planner_with_fake_action(
            "codex-diamonds",
            r#"{
  "reply": "可以，已经准备给你 64 个钻石。",
  "summary": "发放钻石",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {"type":"give_item","player":"Alex","item":"minecraft:diamond","count":64}
  ]
}"#,
        );
        let result = planner
            .plan(
                PlannerInput {
                    text: "give me diamonds".to_string(),
                    player: Some("Alex".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert!(matches!(
            result.actions[0],
            GameAction::GiveItem {
                ref item,
                count: 64,
                ..
            } if item == "minecraft:diamond"
        ));
    }

    #[tokio::test]
    async fn codex_plans_diamond_pickaxe_before_loose_diamond_match() {
        let store = empty_store("diamond-pickaxe").await;
        let planner = planner_with_fake_action(
            "codex-diamond-pickaxe",
            r#"{
  "reply": "可以，已经准备给你一把钻石镐。",
  "summary": "发放钻石镐",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {"type":"give_item","player":"Alex","item":"minecraft:diamond_pickaxe","count":1}
  ]
}"#,
        );
        let result = planner
            .plan(
                PlannerInput {
                    text: "我要一个钻石稿子".to_string(),
                    player: Some("Alex".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert!(matches!(
            result.actions[0],
            GameAction::GiveItem {
                ref item,
                count: 1,
                ..
            } if item == "minecraft:diamond_pickaxe"
        ));
    }

    #[tokio::test]
    async fn enabled_codex_failure_does_not_fall_back_to_keyword_rules() {
        let store = empty_store("codex-invalid-action").await;
        let planner = planner_with_fake_plan("codex-invalid-action", "not action json");

        let result = planner
            .plan(
                PlannerInput {
                    text: "给我钻石".to_string(),
                    player: Some("Alex".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "AI 输出格式无效");
        assert!(result.actions.is_empty());
        assert!(result.reply.contains("没有生成可执行动作"));
    }

    #[tokio::test]
    async fn codex_process_failure_replies_with_admin_hint() {
        let store = empty_store("codex-process-failure").await;
        let planner = planner_with_failing_codex("codex-process-failure");

        let result = planner
            .plan(
                PlannerInput {
                    text: "给我一组红色的砖".to_string(),
                    player: Some("Alex".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "AI 助手调用失败");
        assert!(result.actions.is_empty());
        assert!(result.reply.contains("没有发送到 Minecraft"));
        assert!(result.reply.contains("Codex 登录状态"));
        assert!(result.reply.contains("详细日志"));
    }

    #[test]
    fn codex_not_found_failure_mentions_missing_command() {
        let reply = codex_failure_reply("No such file or directory (os error 2)");

        assert!(reply.contains("没有发送到 Minecraft"));
        assert!(reply.contains("找不到 codex 命令"));
        assert!(reply.contains("详细日志"));
    }

    #[test]
    fn codex_failure_reply_includes_trace_id_and_log_path() {
        let reply = codex_failure_reply_with_log_hint(
            "codex_trace_id=codex-123-456: failed",
            "请管理员检查 Codex 登录状态、模型权限、网络连接或 CLI 版本。",
            Some("/tmp/blockwright-controller.log"),
        );

        assert!(reply.contains("codex_trace_id=codex-123-456"));
        assert!(reply.contains("/tmp/blockwright-controller.log"));
    }

    #[tokio::test]
    async fn local_image_recreation_generates_pixel_blueprint_without_codex() {
        let store = empty_store("local-image-pixel-blueprint").await;
        let dir = temp_dir("local-image-pixel-blueprint-upload");
        fs::create_dir_all(&dir).unwrap();
        let image_path = dir.join("house.png");
        let mut image = ImageBuffer::<Rgba<u8>, Vec<u8>>::new(4, 2);
        for x in 0..4 {
            image.put_pixel(x, 0, Rgba([255, 255, 255, 255]));
            image.put_pixel(x, 1, Rgba([30, 30, 30, 255]));
        }
        image.save(&image_path).unwrap();

        let result = Planner::default()
            .plan(
                PlannerInput {
                    text: "按 16 像素复刻这张建筑图片".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: vec![ChatAttachment {
                        kind: ChatAttachmentKind::Image,
                        source: ChatAttachmentSource::LocalPath {
                            path: image_path.to_string_lossy().to_string(),
                        },
                        file_name: Some("house.png".to_string()),
                        mime_type: Some("image/png".to_string()),
                    }],
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert!(result.reply.contains("16x8"));
        assert_eq!(result.actions.len(), 1);
        assert!(matches!(
            result.actions[0],
            GameAction::PlaceBlocks { ref blocks, .. } if blocks.len() == 128
        ));
        let saved = store.list().await;
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].size.width, 16);
        assert_eq!(saved[0].size.height, 8);
    }

    #[test]
    fn large_place_blocks_actions_are_chunked_for_tick_execution() {
        let mut actions = Vec::new();
        let origin = BlockOrigin {
            world: Some("minecraft:overworld".to_string()),
            x: 10,
            y: 64,
            z: 20,
        };

        push_place_blocks_actions(
            &mut actions,
            "large-image".to_string(),
            origin.clone(),
            test_blocks(PLACE_BLOCKS_CHUNK_SIZE + 3, "minecraft:white_concrete"),
            false,
        );

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            GameAction::PlaceBlocks {
                blueprint_id: Some(ref id),
                ref blocks,
                ..
            } if id == "large-image:part-0000" && blocks.len() == PLACE_BLOCKS_CHUNK_SIZE
        ));
        assert!(matches!(
            actions[1],
            GameAction::PlaceBlocks {
                blueprint_id: Some(ref id),
                ref blocks,
                ..
            } if id == "large-image:part-0001" && blocks.len() == 3
        ));
    }

    #[tokio::test]
    async fn codex_enabled_building_image_uses_3d_planner_not_pixel_mural() {
        let store = empty_store("building-image-uses-codex").await;
        let dir = temp_dir("building-image-uses-codex-upload");
        fs::create_dir_all(&dir).unwrap();
        let image_path = dir.join("building.png");
        ImageBuffer::<Rgba<u8>, Vec<u8>>::new(4, 4)
            .save(&image_path)
            .unwrap();
        let planner = planner_with_fake_codex(
            "building-image-uses-codex",
            r#"{
  "id": "image-3d-building",
  "name": "图片三维建筑",
  "description": "按图片可见面复刻并补全背面的三维建筑。",
  "size": {"width": 8, "height": 5, "depth": 6},
  "materials": [
    {"material": "minecraft:oak_planks", "count": 96},
    {"material": "minecraft:glass", "count": 4}
  ],
  "blocks": [
    {"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"},
    {"x": 7, "y": 4, "z": 5, "material": "minecraft:oak_planks"},
    {"x": 3, "y": 2, "z": 0, "material": "minecraft:glass"}
  ],
  "primitives": [
    {"type": "hollow_box", "from": {"x": 0, "y": 0, "z": 0}, "to": {"x": 7, "y": 4, "z": 5}, "material": "minecraft:oak_planks"},
    {"type": "box", "from": {"x": 3, "y": 2, "z": 0}, "to": {"x": 4, "y": 3, "z": 0}, "material": "minecraft:glass"}
  ],
  "tags": ["image_reference", "building"]
}"#,
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "帮我完全复刻这张建筑图片，看不到的背面自己补全".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: vec![ChatAttachment {
                        kind: ChatAttachmentKind::Image,
                        source: ChatAttachmentSource::LocalPath {
                            path: image_path.to_string_lossy().to_string(),
                        },
                        file_name: Some("building.png".to_string()),
                        mime_type: Some("image/png".to_string()),
                    }],
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 image-3d-building");
        assert!(store.get("image-3d-building").await.is_some());
        assert!(store.list().await.iter().all(|item| {
            item.spec
                .as_ref()
                .and_then(|spec| spec.get("source"))
                .and_then(serde_json::Value::as_str)
                != Some("image_to_pixel_blueprint")
        }));
    }

    #[tokio::test]
    async fn build_failure_requests_scan_instead_of_confirmation_when_codex_enabled() {
        let store = empty_store("codex-invalid-blueprint").await;
        let planner = planner_with_fake_codex("codex-invalid-blueprint", "not json");

        let result = planner
            .plan(
                PlannerInput {
                    text: "帮我盖一个木屋".to_string(),
                    player: Some("Alex".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "自动扫描后继续建造");
        assert!(matches!(
            result.actions[0],
            GameAction::ScanNearbyAndPlan { ref text, .. } if text == "帮我盖一个木屋"
        ));
        assert!(!result.reply.contains("确认"));
    }

    #[tokio::test]
    async fn invalid_build_after_scan_asks_codex_to_repair_plan() {
        let store = empty_store("codex-repair-build").await;
        let planner = planner_with_fake_plan_sequence(
            "codex-repair-build",
            &[
                "not json",
                r#"{
  "reply": "我会直接把末影人雕像建出来。",
  "summary": "建造蓝图 repaired_enderman_statue",
  "blueprint": {
    "id": "repaired_enderman_statue",
    "name": "修复后的末影人雕像",
    "description": "由模型修复输出生成的末影人雕像。",
    "size": {"width": 1, "height": 2, "depth": 1},
    "materials": [{"material": "minecraft:black_concrete", "count": 2}],
    "blocks": [
      {"x": 0, "y": 0, "z": 0, "material": "minecraft:black_concrete"},
      {"x": 0, "y": 1, "z": 0, "material": "minecraft:black_concrete"}
    ],
    "tags": ["enderman", "statue"]
  },
  "site_plan": null,
  "actions": []
}"#,
            ],
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "再建造个逼真末影人".to_string(),
                    player: Some("Alex".to_string()),
                    codex_session_key: None,
                    position: Some(PlayerPosition {
                        world: "minecraft:overworld".to_string(),
                        x: 20.0,
                        y: 64.0,
                        z: 30.0,
                        yaw: None,
                        pitch: None,
                    }),
                    player_state: None,
                    nearby_scan: Some(scan_with_blocks(vec![scan_block(
                        20,
                        63,
                        30,
                        "minecraft:grass_block",
                    )])),
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 repaired_enderman_statue");
        assert!(matches!(
            result.actions.last(),
            Some(GameAction::PlaceBlocks {
                blueprint_id: Some(id),
                blocks,
                ..
            }) if id == "repaired_enderman_statue" && blocks.len() == 2
        ));
        assert!(store.get("repaired_enderman_statue").await.is_some());
    }

    #[tokio::test]
    async fn chat_plan_replies_without_minecraft_actions() {
        let store = empty_store("codex-chat-only").await;
        let planner = planner_with_fake_plan(
            "codex-chat-only",
            r#"{"reply":"可以，我们先聊方案。你想偏木屋、城堡还是现代风？","summary":"讨论建造方案","blueprint":null,"site_plan":null,"actions":[]}"#,
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "先聊一下，我还没想好风格".to_string(),
                    player: Some("Alex".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "讨论建造方案");
        assert!(result.reply.contains("先聊方案"));
        assert!(result.actions.is_empty());
    }

    #[tokio::test]
    async fn codex_blueprint_handles_treehouse_request() {
        let store = empty_store("codex-treehouse").await;
        let planner = planner_with_fake_codex(
            "codex-treehouse",
            r#"{
  "id": "generated-tree-house",
  "name": "树屋",
  "description": "先用橡木原木做树干和支撑，再用木板生成小平台和房间。",
  "size": {"width": 2, "height": 2, "depth": 1},
  "materials": [{"material": "minecraft:oak_planks", "count": 2}],
  "blocks": [
    {"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"},
    {"x": 1, "y": 0, "z": 0, "material": "minecraft:oak_planks"}
  ],
  "tags": ["tree_house"]
}"#,
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "我要生成一个树屋".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 generated-tree-house");
        assert!(matches!(
            result.actions[0],
            GameAction::PlaceBlocks {
                blueprint_id: Some(ref blueprint_id),
                ..
            } if blueprint_id == "generated-tree-house"
        ));
        assert!(store.get("generated-tree-house").await.is_some());
    }

    #[tokio::test]
    async fn codex_blueprint_takes_precedence_over_builtin_house_template() {
        let store = empty_store("codex-house-first").await;
        store
            .save(test_blueprint("test-house", vec!["house"]))
            .await
            .unwrap();
        let planner = planner_with_fake_codex(
            "codex-house-first",
            r#"{
  "id": "codex-wood-cabin",
  "name": "大模型木屋",
  "description": "根据玩家描述重新规划一个小木屋，而不是复用内置模板。",
  "size": {"width": 1, "height": 1, "depth": 1},
  "materials": [{"material": "minecraft:oak_planks", "count": 1}],
  "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
  "tags": ["house"]
}"#,
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "帮我盖一个木屋".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 codex-wood-cabin");
        assert!(matches!(
            result.actions[0],
            GameAction::PlaceBlocks {
                blueprint_id: Some(ref blueprint_id),
                ..
            } if blueprint_id == "codex-wood-cabin"
        ));
    }

    #[tokio::test]
    async fn codex_plan_handles_build_edit_without_local_keyword_rules() {
        let store = empty_store("codex-existing-edit").await;
        let planner = planner_with_fake_plan(
            "codex-existing-edit",
            r#"{
  "reply": "已按当前建筑自由改造。",
  "summary": "自由改造现有建筑",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {
      "type": "place_blocks",
      "blueprint_id": "codex-existing-clear",
      "origin": {"world": "minecraft:overworld", "x": 10, "y": 64, "z": 20},
      "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:air"}],
      "clear_existing": true
    },
    {
      "type": "place_blocks",
      "blueprint_id": "codex-existing-edit",
      "origin": {"world": "minecraft:overworld", "x": 10, "y": 65, "z": 20},
      "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
      "clear_existing": true
    }
  ]
}"#,
        );
        let result = planner
            .plan(
                PlannerInput {
                    text: "把它整体升高一点，再做得更精致".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: Some(scan_with_blocks(Vec::new())),
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "自由改造现有建筑");
        assert_eq!(result.actions.len(), 2);
        assert!(matches!(
            &result.actions[0],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                blocks,
                clear_existing: true,
                ..
            } if blueprint_id == "codex-existing-clear"
                && blocks.len() == 1
                && blocks[0].material == "minecraft:air"
        ));
        assert!(matches!(
            &result.actions[1],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                origin: BlockOrigin { y: 65, .. },
                ..
            } if blueprint_id == "codex-existing-edit"
        ));
    }

    #[tokio::test]
    async fn carousel_request_enters_codex_blueprint_planner() {
        let store = empty_store("codex-carousel").await;
        let planner = planner_with_fake_codex(
            "codex-carousel",
            r#"{
  "id": "large-carousel",
  "name": "大气旋转木马",
  "description": "中心立柱、圆形平台和装饰顶棚组成的旋转木马。",
  "size": {"width": 3, "height": 3, "depth": 3},
  "materials": [{"material": "minecraft:oak_planks", "count": 3}],
  "blocks": [
    {"x": 1, "y": 0, "z": 1, "material": "minecraft:oak_planks"},
    {"x": 1, "y": 1, "z": 1, "material": "minecraft:oak_fence"},
    {"x": 1, "y": 2, "z": 1, "material": "minecraft:red_wool"}
  ],
  "tags": ["carousel"]
}"#,
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "给我旋转木马，可以大点，大气点".to_string(),
                    player: Some("Charles".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 large-carousel");
        assert!(matches!(
            result.actions[0],
            GameAction::PlaceBlocks {
                blueprint_id: Some(ref blueprint_id),
                ..
            } if blueprint_id == "large-carousel"
        ));
    }

    #[tokio::test]
    async fn codex_build_uses_scan_ground_and_soft_clear_policy() {
        let store = empty_store("codex-site-soft-clear").await;
        let planner = planner_with_fake_codex(
            "codex-site-soft-clear",
            r#"{
  "id": "site-aware-room",
  "name": "场地感知房间",
  "description": "根据当前地面高度生成一个小房间。",
  "size": {"width": 1, "height": 1, "depth": 1},
  "materials": [{"material": "minecraft:oak_planks", "count": 1}],
  "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
  "tags": ["room"]
}"#,
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "生成一个房间".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: Some(PlayerPosition {
                        world: "minecraft:overworld".to_string(),
                        x: 18.0,
                        y: 64.0,
                        z: 28.0,
                        yaw: None,
                        pitch: None,
                    }),
                    player_state: None,
                    nearby_scan: Some(scan_with_blocks(vec![
                        scan_block(20, 63, 30, "minecraft:grass_block"),
                        scan_block(20, 64, 30, "minecraft:short_grass"),
                    ])),
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert!(result.reply.contains("地基放在 y=64"));
        assert!(matches!(
            result.actions[0],
            GameAction::PlaceBlocks {
                origin: BlockOrigin {
                    x: 20,
                    y: 64,
                    z: 30,
                    ..
                },
                clear_existing: true,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn codex_build_shifts_away_from_hard_overlap() {
        let store = empty_store("codex-site-hard-block").await;
        let planner = planner_with_fake_codex(
            "codex-site-hard-block",
            r#"{
  "id": "blocked-room",
  "name": "会重叠的房间",
  "description": "测试硬方块重叠。",
  "size": {"width": 1, "height": 1, "depth": 1},
  "materials": [{"material": "minecraft:oak_planks", "count": 1}],
  "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
  "tags": ["room"]
}"#,
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "生成一个房间".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: Some(PlayerPosition {
                        world: "minecraft:overworld".to_string(),
                        x: 18.0,
                        y: 64.0,
                        z: 28.0,
                        yaw: None,
                        pitch: None,
                    }),
                    player_state: None,
                    nearby_scan: Some(scan_with_blocks(vec![
                        scan_block(19, 63, 30, "minecraft:grass_block"),
                        scan_block(20, 63, 30, "minecraft:grass_block"),
                        scan_block(20, 64, 30, "minecraft:oak_log"),
                    ])),
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 blocked-room");
        assert!(result.reply.contains("自动选择更合适落点"));
        let main_action = result
            .actions
            .iter()
            .find(|action| {
                matches!(
                    action,
                    GameAction::PlaceBlocks {
                        blueprint_id: Some(blueprint_id),
                        ..
                    } if blueprint_id == "blocked-room"
                )
            })
            .expect("main blueprint action should be present");
        assert!(matches!(
            main_action,
            GameAction::PlaceBlocks {
                origin: BlockOrigin { y: 64, .. },
                clear_existing: false,
                ..
            }
        ));
        assert!(store.get("blocked-room").await.is_some());
    }

    #[tokio::test]
    async fn codex_build_keeps_front_target_and_integrates_water_surface() {
        let store = empty_store("codex-site-water").await;
        let planner = planner_with_fake_codex(
            "codex-site-water",
            r#"{
  "id": "water-aware-room",
  "name": "避开水面的房间",
  "description": "测试不要把水面当地面。",
  "size": {"width": 1, "height": 1, "depth": 1},
  "materials": [{"material": "minecraft:oak_planks", "count": 1}],
  "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
  "tags": ["room"]
}"#,
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "生成一个房间".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: Some(PlayerPosition {
                        world: "minecraft:overworld".to_string(),
                        x: 18.0,
                        y: 64.0,
                        z: 28.0,
                        yaw: None,
                        pitch: None,
                    }),
                    player_state: None,
                    nearby_scan: Some(scan_with_blocks(vec![
                        scan_block(20, 63, 30, "minecraft:water[level=0]"),
                        scan_block(21, 63, 30, "minecraft:grass_block[snowy=false]"),
                    ])),
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 water-aware-room");
        assert!(result.reply.contains("融入地形的木桩平台"));
        assert_eq!(result.actions.len(), 2);
        assert!(matches!(
            &result.actions[0],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                origin: BlockOrigin {
                    x: 20,
                    y: 64,
                    z: 30,
                    ..
                },
                blocks,
                clear_existing: true,
            } if blueprint_id == "water-aware-room:site-foundation"
                && blocks.len() == 1
                && blocks[0].y == -1
                && blocks[0].material == "minecraft:oak_planks"
        ));
        assert!(matches!(
            &result.actions[1],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                origin: BlockOrigin {
                    x: 20,
                    y: 64,
                    z: 30,
                    ..
                },
                ..
            } if blueprint_id == "water-aware-room"
        ));
    }

    #[tokio::test]
    async fn codex_build_prepares_foundation_when_no_good_surface_exists() {
        let store = empty_store("codex-site-foundation").await;
        let planner = planner_with_fake_codex(
            "codex-site-foundation",
            r#"{
  "id": "foundation-room",
  "name": "自动补地基房间",
  "description": "测试不因地面不适合而拒绝。",
  "size": {"width": 1, "height": 1, "depth": 1},
  "materials": [{"material": "minecraft:oak_planks", "count": 1}],
  "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
  "tags": ["room"]
}"#,
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "生成一个房间".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: Some(PlayerPosition {
                        world: "minecraft:overworld".to_string(),
                        x: 18.0,
                        y: 64.0,
                        z: 28.0,
                        yaw: None,
                        pitch: None,
                    }),
                    player_state: None,
                    nearby_scan: Some(scan_with_blocks(vec![scan_block(
                        20,
                        63,
                        30,
                        "minecraft:water[level=0]",
                    )])),
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 foundation-room");
        assert!(result.reply.contains("融入地形的木桩平台"));
        assert_eq!(result.actions.len(), 2);
        assert!(matches!(
            &result.actions[0],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                origin: BlockOrigin {
                    x: 20,
                    y: 64,
                    z: 30,
                    ..
                },
                blocks,
                clear_existing: true,
            } if blueprint_id == "foundation-room:site-foundation"
                && blocks.len() == 1
                && blocks[0].y == -1
                && blocks[0].material == "minecraft:oak_planks"
        ));
        assert!(matches!(
            &result.actions[1],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                ..
            } if blueprint_id == "foundation-room"
        ));
    }

    #[tokio::test]
    async fn codex_build_keeps_front_target_and_prepares_supported_footprint() {
        let store = empty_store("codex-site-supported-footprint").await;
        let planner = planner_with_fake_codex(
            "codex-site-supported-footprint",
            r#"{
  "id": "supported-floor-room",
  "name": "有支撑的房间",
  "description": "测试普通房间优先选择完整地面支撑。",
  "size": {"width": 3, "height": 1, "depth": 3},
  "materials": [{"material": "minecraft:oak_planks", "count": 9}],
  "blocks": [
    {"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"},
    {"x": 0, "y": 0, "z": 1, "material": "minecraft:oak_planks"},
    {"x": 0, "y": 0, "z": 2, "material": "minecraft:oak_planks"},
    {"x": 1, "y": 0, "z": 0, "material": "minecraft:oak_planks"},
    {"x": 1, "y": 0, "z": 1, "material": "minecraft:oak_planks"},
    {"x": 1, "y": 0, "z": 2, "material": "minecraft:oak_planks"},
    {"x": 2, "y": 0, "z": 0, "material": "minecraft:oak_planks"},
    {"x": 2, "y": 0, "z": 1, "material": "minecraft:oak_planks"},
    {"x": 2, "y": 0, "z": 2, "material": "minecraft:oak_planks"}
  ],
  "tags": ["room"]
}"#,
        );
        let mut blocks = vec![scan_block(20, 63, 30, "minecraft:grass_block")];
        for x in 22..=24 {
            for z in 29..=31 {
                blocks.push(scan_block(x, 63, z, "minecraft:grass_block"));
            }
        }
        let player_position = PlayerPosition {
            world: "minecraft:overworld".to_string(),
            x: 18.0,
            y: 64.0,
            z: 28.0,
            yaw: None,
            pitch: None,
        };

        let result = planner
            .plan(
                PlannerInput {
                    text: "生成一个房间".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: Some(player_position.clone()),
                    player_state: None,
                    nearby_scan: Some(scan_with_blocks(blocks)),
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 supported-floor-room");
        assert!(result.reply.contains("融入地形的木桩平台"));
        let main_action = result
            .actions
            .iter()
            .find(|action| {
                matches!(
                    action,
                    GameAction::PlaceBlocks {
                        blueprint_id: Some(blueprint_id),
                        ..
                    } if blueprint_id == "supported-floor-room"
                )
            })
            .expect("main blueprint action should be present");
        let GameAction::PlaceBlocks { origin, blocks, .. } = main_action else {
            panic!("main action should place blocks");
        };
        let bounds = blueprint_bounds(blocks).expect("main blueprint should have bounds");
        assert_eq!(
            0,
            player_safety_overlap_count_for_position(&player_position, origin, &bounds)
        );
        assert!(matches!(
            main_action,
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                origin: BlockOrigin {
                    x,
                    y: 64,
                    z,
                    ..
                },
                ..
            } if blueprint_id == "supported-floor-room" && *x >= 20 && *z >= 29
        ));
    }

    #[tokio::test]
    async fn codex_build_shifts_large_footprint_away_from_player_body() {
        let store = empty_store("codex-player-safe-cake").await;
        let mut blueprint_blocks = Vec::new();
        for x in 0..=10 {
            for z in 0..=10 {
                blueprint_blocks.push(format!(
                    r#"{{"x": {x}, "y": 0, "z": {z}, "material": "minecraft:white_wool"}}"#
                ));
            }
        }
        let planner = planner_with_fake_codex(
            "codex-player-safe-cake",
            &format!(
                r#"{{
  "id": "safe-cake-platform",
  "name": "不会盖住玩家的蛋糕平台",
  "description": "测试大面积蓝图不能覆盖玩家安全区。",
  "size": {{"width": 11, "height": 1, "depth": 11}},
  "materials": [{{"material": "minecraft:white_wool", "count": 121}}],
  "blocks": [{}],
  "tags": ["cake"]
}}"#,
                blueprint_blocks.join(",")
            ),
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "帮我盖个蛋糕".to_string(),
                    player: Some("Charles".to_string()),
                    codex_session_key: None,
                    position: Some(PlayerPosition {
                        world: "minecraft:overworld".to_string(),
                        x: 64.0,
                        y: 64.0,
                        z: 0.0,
                        yaw: Some(0.0),
                        pitch: None,
                    }),
                    player_state: None,
                    nearby_scan: Some(WorldScan {
                        world: "minecraft:overworld".to_string(),
                        center_x: 64,
                        center_y: 64,
                        center_z: 5,
                        radius: 8,
                        blocks: flat_ground_blocks(56, 72, -2, 16),
                    }),
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 safe-cake-platform");
        assert!(matches!(
            &result.actions[0],
            GameAction::PlaceBlocks {
                origin: BlockOrigin { x: 59, y: 64, z, .. },
                ..
            } if *z >= 2
        ));
    }

    #[tokio::test]
    async fn codex_build_auto_clears_when_no_better_position_exists() {
        let store = empty_store("codex-site-auto-clear").await;
        let planner = planner_with_fake_codex(
            "codex-site-auto-clear",
            r#"{
  "id": "auto-clear-room",
  "name": "自动清理房间",
  "description": "测试所有候选点都有硬方块时自动覆盖。",
  "size": {"width": 1, "height": 1, "depth": 1},
  "materials": [{"material": "minecraft:oak_planks", "count": 1}],
  "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
  "tags": ["room"]
}"#,
        );
        let mut blocks = Vec::new();
        blocks.push(scan_block(20, 63, 30, "minecraft:grass_block"));
        for x in 12..=28 {
            for z in 22..=38 {
                blocks.push(scan_block(x, 64, z, "minecraft:oak_log"));
            }
        }

        let result = planner
            .plan(
                PlannerInput {
                    text: "生成一个房间".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: Some(PlayerPosition {
                        world: "minecraft:overworld".to_string(),
                        x: 18.0,
                        y: 64.0,
                        z: 28.0,
                        yaw: None,
                        pitch: None,
                    }),
                    player_state: None,
                    nearby_scan: Some(scan_with_blocks(blocks)),
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 auto-clear-room");
        assert!(result.reply.contains("已有方块"));
        assert!(matches!(
            result.actions[0],
            GameAction::PlaceBlocks {
                clear_existing: true,
                ..
            }
        ));
        assert!(store.get("auto-clear-room").await.is_some());
    }

    #[tokio::test]
    async fn codex_disabled_does_not_use_local_keyword_fallback() {
        let store = empty_store("missing-house").await;
        let result = Planner::default()
            .plan(
                PlannerInput {
                    text: "帮我盖一个木屋".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "AI 助手未启用");
        assert!(result.reply.contains("AI 建造助手"));
        assert!(result.actions.is_empty());
    }

    #[tokio::test]
    async fn image_attachment_enters_codex_blueprint_without_magic_text() {
        let store = empty_store("image-attachment").await;
        let planner = planner_with_fake_codex(
            "codex-image-attachment",
            r#"{
  "id": "image-inspired-house",
  "name": "图片复刻小屋",
  "description": "按附件图片生成的复刻建筑。",
  "size": {"width": 1, "height": 1, "depth": 1},
  "materials": [{"material": "minecraft:oak_planks", "count": 1}],
  "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
  "tags": ["image_reference"]
}"#,
        );
        let result = planner
            .plan(
                PlannerInput {
                    text: "照这个做".to_string(),
                    player: None,
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: vec![ChatAttachment {
                        kind: ChatAttachmentKind::Image,
                        source: ChatAttachmentSource::Url {
                            url: "https://example.com/house.png".to_string(),
                        },
                        file_name: Some("house.png".to_string()),
                        mime_type: Some("image/png".to_string()),
                    }],
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 image-inspired-house");
    }

    #[tokio::test]
    async fn image_recreation_quality_gate_repairs_tiny_blueprint() {
        let store = empty_store("image-quality-repair").await;
        let dir = temp_dir("image-quality-repair-upload");
        fs::create_dir_all(&dir).unwrap();
        let image_path = dir.join("reference.png");
        fs::write(&image_path, b"png").unwrap();
        let planner = planner_with_fake_plan_sequence(
            "codex-image-quality-repair",
            &[
                r#"{
  "reply": "我按图片做一个小模型。",
  "summary": "建造蓝图 tiny-image-token",
  "blueprint": {
    "id": "tiny-image-token",
    "name": "图片小模型",
    "description": "过小的图片复刻。",
    "size": {"width": 1, "height": 1, "depth": 1},
    "materials": [{"material": "minecraft:oak_planks", "count": 1}],
    "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
    "tags": ["image_reference"]
  },
  "site_plan": null,
  "actions": []
}"#,
                r#"{
  "reply": "我已经重做成更完整的三维复刻建筑。",
  "summary": "建造蓝图 image-repaired-house",
  "blueprint": {
    "id": "image-repaired-house",
    "name": "图片复刻建筑",
    "description": "使用 primitives 表达的图片复刻建筑。",
    "size": {"width": 8, "height": 4, "depth": 6},
    "primitives": [
      {"type": "box", "from": {"x": 0, "y": 0, "z": 0}, "to": {"x": 7, "y": 0, "z": 5}, "material": "minecraft:stone_bricks"},
      {"type": "hollow_box", "from": {"x": 0, "y": 1, "z": 0}, "to": {"x": 7, "y": 3, "z": 5}, "material": "minecraft:oak_planks"}
    ],
    "tags": ["image_reference"]
  },
  "site_plan": null,
  "actions": []
}"#,
            ],
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "照这个图片复刻".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: None,
                    attachments: vec![ChatAttachment {
                        kind: ChatAttachmentKind::Image,
                        source: ChatAttachmentSource::LocalPath {
                            path: image_path.to_string_lossy().to_string(),
                        },
                        file_name: Some("reference.png".to_string()),
                        mime_type: Some("image/png".to_string()),
                    }],
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 image-repaired-house");
        assert!(store.get("tiny-image-token").await.is_none());
        let repaired = store.get("image-repaired-house").await.unwrap();
        assert!(repaired.blocks.len() >= 96);
        assert!(result.actions.iter().any(|action| matches!(
            action,
            GameAction::PlaceBlocks {
                blueprint_id: Some(id),
                ..
            } if id == "image-repaired-house"
        )));
    }

    #[test]
    fn local_image_attachment_paths_collects_existing_local_images() {
        let dir = temp_dir("local-image-paths");
        fs::create_dir_all(&dir).unwrap();
        let image_path = dir.join("house.png");
        fs::write(&image_path, b"png").unwrap();
        let missing_path = dir.join("missing.png");

        let paths = local_image_attachment_paths(&[
            ChatAttachment {
                kind: ChatAttachmentKind::Image,
                source: ChatAttachmentSource::LocalPath {
                    path: image_path.to_string_lossy().to_string(),
                },
                file_name: Some("house.png".to_string()),
                mime_type: Some("image/png".to_string()),
            },
            ChatAttachment {
                kind: ChatAttachmentKind::Image,
                source: ChatAttachmentSource::LocalPath {
                    path: image_path.to_string_lossy().to_string(),
                },
                file_name: Some("house-duplicate.png".to_string()),
                mime_type: Some("image/png".to_string()),
            },
            ChatAttachment {
                kind: ChatAttachmentKind::Image,
                source: ChatAttachmentSource::LocalPath {
                    path: missing_path.to_string_lossy().to_string(),
                },
                file_name: Some("missing.png".to_string()),
                mime_type: Some("image/png".to_string()),
            },
            ChatAttachment {
                kind: ChatAttachmentKind::Image,
                source: ChatAttachmentSource::Url {
                    url: "https://example.com/house.png".to_string(),
                },
                file_name: Some("remote.png".to_string()),
                mime_type: Some("image/png".to_string()),
            },
            ChatAttachment {
                kind: ChatAttachmentKind::File,
                source: ChatAttachmentSource::LocalPath {
                    path: image_path.to_string_lossy().to_string(),
                },
                file_name: Some("house.txt".to_string()),
                mime_type: Some("text/plain".to_string()),
            },
        ]);

        assert_eq!(paths, vec![image_path]);
    }

    #[tokio::test]
    async fn plan_prompt_uses_context_bundle_and_keeps_workflow_in_codex() {
        let store = empty_store("prompt-context").await;
        let input = PlannerInput {
            text: "照图片盖一个小塔".to_string(),
            player: None,
            codex_session_key: None,
            position: None,
            player_state: None,
            nearby_scan: None,
            attachments: Vec::new(),
            progress_id: None,
        };
        let context = build_context_bundle(&input, &store, None).await;
        let prompt = render_plan_prompt(&context);

        assert!(prompt.contains("context_bundle"));
        assert!(prompt.contains("普通聊天助手"));
        assert!(prompt.contains("controller 只是工具运行时和兼容协议桥"));
        assert!(prompt.contains("只返回一个 JSON 对象"));
        assert!(prompt.contains("site_plan"));
        assert!(prompt.contains("skills 是行为规范和专业经验"));
        assert!(prompt.contains("available_blueprints"));
        assert!(prompt.contains("recent_builds"));
        assert!(prompt.contains("明确请求直接完成"));
        assert!(prompt.contains("blockwright-site-selection"));
        assert!(prompt.contains("scan_nearby_and_plan"));
        assert!(prompt.contains("blockwright_enqueue_actions"));
        assert!(prompt.contains("相对坐标"));
        assert!(prompt.contains("命名空间 ID"));
        assert!(prompt.contains("give_item"));
        assert!(prompt.contains("run_command"));
        assert!(prompt.contains("默认意图是复刻"));
        assert!(prompt.contains("不是简化版或小模型"));
        assert!(prompt.contains("明显需要很多方块就使用很多方块"));
        assert!(prompt.contains("primitives"));
        assert!(prompt.contains("hollow_box"));
        assert!(!prompt.contains("新建建筑、模型或场景时：调用并遵循"));
    }

    #[tokio::test]
    async fn simple_action_context_omits_blueprints_and_build_history() {
        let blueprints = empty_store("simple-action-history-omitted").await;
        blueprints
            .save(test_blueprint("stored-cabin", vec!["house"]))
            .await
            .unwrap();
        let builds = BuildStore::new(temp_dir("simple-action-build-history"))
            .await
            .unwrap();
        builds
            .register_planned(
                "job-red-wall".to_string(),
                "local".to_string(),
                Some("Steve".to_string()),
                "历史红墙".to_string(),
                &[GameAction::PlaceBlocks {
                    blueprint_id: Some("stored-cabin".to_string()),
                    origin: BlockOrigin {
                        world: Some("minecraft:overworld".to_string()),
                        x: 10,
                        y: 64,
                        z: 10,
                    },
                    blocks: test_blocks(8, "minecraft:red_concrete"),
                    clear_existing: false,
                }],
            )
            .await
            .unwrap();
        let input = PlannerInput {
            text: "给我一组红色的砖".to_string(),
            player: Some("Steve".to_string()),
            codex_session_key: None,
            position: None,
            player_state: None,
            nearby_scan: None,
            attachments: Vec::new(),
            progress_id: None,
        };

        let context = build_context_bundle(&input, &blueprints, Some(&builds)).await;

        assert!(context.available_blueprints.is_empty());
        assert!(context.recent_builds.is_empty());
    }

    #[tokio::test]
    async fn context_bundle_exposes_blueprint_and_build_blocks_as_data_sources() {
        let blueprints = empty_store("context-data-sources").await;
        blueprints
            .save(test_blueprint("stored-cabin", vec!["house"]))
            .await
            .unwrap();
        let builds = BuildStore::new(temp_dir("context-build-records"))
            .await
            .unwrap();
        let near_action = GameAction::PlaceBlocks {
            blueprint_id: Some("stored-cabin".to_string()),
            origin: BlockOrigin {
                world: Some("minecraft:overworld".to_string()),
                x: 30,
                y: 64,
                z: 40,
            },
            blocks: vec![BlueprintBlock {
                x: 0,
                y: 0,
                z: 0,
                material: "minecraft:oak_planks".to_string(),
            }],
            clear_existing: false,
        };
        let far_action = GameAction::PlaceBlocks {
            blueprint_id: Some("stored-cabin".to_string()),
            origin: BlockOrigin {
                world: Some("minecraft:overworld".to_string()),
                x: 100,
                y: 64,
                z: 40,
            },
            blocks: vec![BlueprintBlock {
                x: 0,
                y: 0,
                z: 0,
                material: "minecraft:oak_planks".to_string(),
            }],
            clear_existing: false,
        };
        builds
            .register_planned(
                "job-a-near".to_string(),
                "local".to_string(),
                Some("Steve".to_string()),
                "近处建筑记录".to_string(),
                &[near_action],
            )
            .await
            .unwrap();
        builds
            .register_planned(
                "job-z-far".to_string(),
                "local".to_string(),
                Some("Steve".to_string()),
                "远处建筑记录".to_string(),
                &[far_action],
            )
            .await
            .unwrap();
        let input = PlannerInput {
            text: "把刚才的小屋窗户改大一点".to_string(),
            player: Some("Steve".to_string()),
            codex_session_key: None,
            position: Some(PlayerPosition {
                world: "minecraft:overworld".to_string(),
                x: 31.0,
                y: 64.0,
                z: 40.0,
                yaw: None,
                pitch: None,
            }),
            player_state: None,
            nearby_scan: None,
            attachments: Vec::new(),
            progress_id: None,
        };

        let context = build_context_bundle(&input, &blueprints, Some(&builds)).await;

        assert_eq!(context.available_blueprints.len(), 1);
        assert_eq!(context.available_blueprints[0].id, "stored-cabin");
        assert_eq!(context.available_blueprints[0].block_sample.len(), 1);
        assert_eq!(context.recent_builds.len(), 2);
        assert_eq!(context.recent_builds[0].id, "job-a-near");
        assert_eq!(context.recent_builds[1].id, "job-z-far");
        assert_eq!(
            context.recent_builds[0].distance_to_target_blocks,
            Some(1.0)
        );
        assert_eq!(
            context.recent_builds[0]
                .nearest_action_origin
                .as_ref()
                .map(|origin| origin.x),
            Some(30)
        );
        assert_eq!(context.recent_builds[0].actions[0].block_sample.len(), 1);
        assert_eq!(
            context.recent_builds[0].actions[0].origin.world.as_deref(),
            Some("minecraft:overworld")
        );
        assert_eq!(context.recent_builds[0].actions[0].origin.x, 30);
        assert_eq!(context.recent_builds[0].actions[0].origin.y, 64);
        assert_eq!(context.recent_builds[0].actions[0].origin.z, 40);
    }

    #[tokio::test]
    async fn context_bundle_bounds_large_blueprint_and_build_block_samples() {
        let blueprints = empty_store("bounded-context-blueprints").await;
        let blueprint_block_count = CONTEXT_BLUEPRINT_BLOCK_SAMPLE_LIMIT + 9;
        let mut blueprint = test_blueprint("large-stored-wall", vec!["wall"]);
        blueprint.blocks = test_blocks(blueprint_block_count, "minecraft:red_concrete");
        blueprint.materials = vec![MaterialCount {
            material: "minecraft:red_concrete".to_string(),
            count: blueprint_block_count as u32,
        }];
        blueprints.save(blueprint).await.unwrap();

        let builds = BuildStore::new(temp_dir("bounded-context-builds"))
            .await
            .unwrap();
        let build_block_count = CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT + 11;
        let build_blocks = test_blocks(build_block_count, "minecraft:red_wool");
        builds
            .register_planned(
                "job-large-red-wall".to_string(),
                "local".to_string(),
                Some("Steve".to_string()),
                "大面积红色墙体".to_string(),
                &[GameAction::PlaceBlocks {
                    blueprint_id: Some("large-stored-wall".to_string()),
                    origin: BlockOrigin {
                        world: Some("minecraft:overworld".to_string()),
                        x: 10,
                        y: 64,
                        z: 10,
                    },
                    blocks: build_blocks,
                    clear_existing: false,
                }],
            )
            .await
            .unwrap();

        let input = PlannerInput {
            text: "复用之前的大面积红色墙体蓝图，把刚才的墙体改一下".to_string(),
            player: Some("Steve".to_string()),
            codex_session_key: None,
            position: None,
            player_state: None,
            nearby_scan: None,
            attachments: Vec::new(),
            progress_id: None,
        };

        let context = build_context_bundle(&input, &blueprints, Some(&builds)).await;

        let blueprint_context = &context.available_blueprints[0];
        assert_eq!(blueprint_context.block_count, blueprint_block_count);
        assert_eq!(
            blueprint_context.block_sample_limit,
            CONTEXT_BLUEPRINT_BLOCK_SAMPLE_LIMIT
        );
        assert!(blueprint_context.block_sample_truncated);
        assert_eq!(
            blueprint_context.block_sample.len(),
            CONTEXT_BLUEPRINT_BLOCK_SAMPLE_LIMIT
        );
        assert_eq!(
            blueprint_context.block_sample[0].material,
            "minecraft:red_concrete"
        );

        let action_context = &context.recent_builds[0].actions[0];
        assert_eq!(action_context.expected_count, build_block_count as u32);
        assert_eq!(
            action_context.block_sample_limit,
            CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT
        );
        assert!(action_context.block_sample_truncated);
        assert_eq!(
            action_context.block_sample.len(),
            CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT
        );
        assert_eq!(
            action_context.block_sample[0].material,
            "minecraft:red_wool"
        );
    }

    #[tokio::test]
    async fn context_bundle_collapses_chunked_build_actions() {
        let blueprints = empty_store("chunked-context-blueprints").await;
        let builds = BuildStore::new(temp_dir("chunked-context-builds"))
            .await
            .unwrap();
        let origin = BlockOrigin {
            world: Some("minecraft:overworld".to_string()),
            x: 10,
            y: 64,
            z: 10,
        };
        builds
            .register_planned(
                "job-chunked-image".to_string(),
                "local".to_string(),
                Some("Steve".to_string()),
                "大图复刻".to_string(),
                &[
                    GameAction::PlaceBlocks {
                        blueprint_id: Some("portrait:part-0000".to_string()),
                        origin: origin.clone(),
                        blocks: test_blocks(4, "minecraft:white_concrete"),
                        clear_existing: false,
                    },
                    GameAction::PlaceBlocks {
                        blueprint_id: Some("portrait:part-0001".to_string()),
                        origin,
                        blocks: test_blocks(3, "minecraft:black_concrete"),
                        clear_existing: false,
                    },
                ],
            )
            .await
            .unwrap();
        let input = PlannerInput {
            text: "把刚才的人像往左挪一点".to_string(),
            player: Some("Steve".to_string()),
            codex_session_key: None,
            position: None,
            player_state: None,
            nearby_scan: None,
            attachments: Vec::new(),
            progress_id: None,
        };

        let context = build_context_bundle(&input, &blueprints, Some(&builds)).await;

        assert_eq!(context.recent_builds.len(), 1);
        assert_eq!(context.recent_builds[0].actions.len(), 1);
        let action = &context.recent_builds[0].actions[0];
        assert_eq!(action.blueprint_id.as_deref(), Some("portrait"));
        assert_eq!(action.expected_count, 7);
        assert_eq!(action.materials.len(), 2);
    }

    #[tokio::test]
    async fn codex_site_plan_controls_origin_clearing_and_foundation() {
        let store = empty_store("codex-site-plan").await;
        let planner = planner_with_fake_plan(
            "codex-site-plan",
            r#"{
  "reply": "我按湖边做一个带基座的小平台。",
  "summary": "建造蓝图 model-site-platform",
  "blueprint": {
    "id": "model-site-platform",
    "name": "模型选址平台",
    "description": "测试模型输出 site_plan 控制落点和场地辅助块。",
    "size": {"width": 1, "height": 1, "depth": 1},
    "materials": [{"material": "minecraft:oak_planks", "count": 1}],
    "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
    "tags": ["platform"]
  },
  "site_plan": {
    "origin": {"world": "minecraft:overworld", "x": 100, "y": 70, "z": 200},
    "clear_existing": true,
    "pre_clear_blocks": [{"x": 1, "y": 0, "z": 0, "material": "minecraft:air"}],
    "pre_foundation_blocks": [{"x": 0, "y": -1, "z": 0, "material": "minecraft:stone_bricks"}],
    "rationale": "贴着湖边但入口朝玩家"
  },
  "actions": []
}"#,
        );

        let result = planner
            .plan(
                PlannerInput {
                    text: "在湖边做一个平台".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    player_state: None,
                    nearby_scan: Some(scan_with_blocks(vec![scan_block(
                        20,
                        63,
                        30,
                        "minecraft:grass_block",
                    )])),
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 model-site-platform");
        assert!(result.reply.contains("贴着湖边但入口朝玩家"));
        assert_eq!(result.actions.len(), 3);
        assert!(matches!(
            &result.actions[0],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                origin: BlockOrigin {
                    world: Some(world),
                    x: 100,
                    y: 70,
                    z: 200,
                },
                blocks,
                clear_existing: true,
            } if blueprint_id == "model-site-platform:site-foundation"
                && world == "minecraft:overworld"
                && blocks.len() == 1
                && blocks[0].material == "minecraft:stone_bricks"
        ));
        assert!(matches!(
            &result.actions[1],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                origin: BlockOrigin { x: 100, y: 70, z: 200, .. },
                blocks,
                clear_existing: true,
            } if blueprint_id == "model-site-platform:site-clear"
                && blocks.len() == 1
                && blocks[0].material == "minecraft:air"
        ));
        assert!(matches!(
            &result.actions[2],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                origin: BlockOrigin { x: 100, y: 70, z: 200, .. },
                clear_existing: true,
                ..
            } if blueprint_id == "model-site-platform"
        ));
        assert!(store.get("model-site-platform").await.is_some());
    }

    #[test]
    fn parses_codex_plan_for_diamond_pickaxe() {
        let output = r#"{
  "reply": "可以，已经准备给你一把钻石镐。",
  "summary": "发放钻石镐",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {"type":"give_item","player":null,"item":"minecraft:diamond_pickaxe","count":1}
  ]
}"#;

        let plan = parse_plan_response(output).unwrap();

        assert_eq!(plan.summary, "发放钻石镐");
        assert!(plan.blueprint.is_none());
        assert!(matches!(
            plan.actions[0],
            GameAction::GiveItem {
                ref item,
                count: 1,
                ..
            } if item == "minecraft:diamond_pickaxe"
        ));
    }

    #[test]
    fn parses_structured_output_action_with_unused_nullable_fields() {
        let output = r#"{
  "reply": "可以，已经准备给你一把钻石镐。",
  "summary": "发放钻石镐",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {
      "type": "give_item",
      "player": null,
      "item": "minecraft:diamond_pickaxe",
      "count": 1,
      "blueprint_id": null,
      "origin": null,
      "blocks": [],
      "clear_existing": null,
      "command": null,
      "message": null,
      "text": null,
      "attachments": []
    }
  ]
}"#;

        let plan = parse_plan_response(output).unwrap();

        assert!(matches!(
            plan.actions[0],
            GameAction::GiveItem {
                ref item,
                count: 1,
                ..
            } if item == "minecraft:diamond_pickaxe"
        ));
    }

    #[test]
    fn repairs_plan_missing_safe_protocol_defaults() {
        let output = r#"{
  "reply": "可以，已经准备给你一把钻石镐。",
  "summary": "发放钻石镐",
  "actions": [
    {"type":"give_item","player":null,"item":"minecraft:diamond_pickaxe","count":1}
  ]
}"#;

        let plan = parse_plan_response(output).unwrap();

        assert!(plan.blueprint.is_none());
        assert!(plan.site_plan.is_none());
        assert!(matches!(
            plan.actions[0],
            GameAction::GiveItem {
                ref item,
                count: 1,
                ..
            } if item == "minecraft:diamond_pickaxe"
        ));
    }

    #[test]
    fn parses_codex_plan_for_minecraft_command() {
        let output = r#"{
  "reply": "可以，已经切到白天。",
  "summary": "设置为白天",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {"type":"run_command","command":"time set day"}
  ]
}"#;

        let plan = parse_plan_response(output).unwrap();

        assert_eq!(plan.summary, "设置为白天");
        assert!(matches!(
            plan.actions[0],
            GameAction::RunCommand { ref command } if command == "time set day"
        ));
    }

    #[test]
    fn parses_scan_request_plan() {
        let output = r#"{
  "reply": "我先看一下你面前的场地，再继续处理。",
  "summary": "扫描现场",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {"type":"scan_nearby_and_plan","text":"把我面前这个建筑的窗户换成蓝色玻璃","attachments":[]}
  ]
}"#;

        let plan = parse_plan_response(output).unwrap();

        assert_eq!(plan.summary, "扫描现场");
        assert!(matches!(
            plan.actions[0],
            GameAction::ScanNearbyAndPlan { ref text, .. } if text.contains("窗户")
        ));
    }

    #[test]
    fn repairs_scan_request_with_tool_like_fields() {
        let output = r#"{
  "reply": "先看地形。",
  "summary": "扫描后建造苦力怕建筑",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {"type":"scan_nearby_and_plan","player":"Charles","radius":16,"purpose":"选择苦力怕建筑落点"}
  ]
}"#;

        let plan = parse_plan_response_for_input(output, "给我做个苦力怕建筑物").unwrap();

        assert!(matches!(
            plan.actions[0],
            GameAction::ScanNearbyAndPlan { ref text, ref attachments }
                if text == "给我做个苦力怕建筑物" && attachments.is_empty()
        ));
    }

    #[test]
    fn repairs_common_blueprint_shape_from_model_output() {
        let output = r#"{
  "reply": "开始建苦力怕小屋。",
  "summary": "建造苦力怕小屋",
  "blueprint": {
    "id": "creeper_house_compact",
    "name": "苦力怕小屋",
    "origin_mode": "site_plan",
    "dimensions": {"x": 2, "y": 2, "z": 1},
    "materials": [{"material": "minecraft:green_concrete", "count": 99}],
    "blocks": [
      {"x": 0, "y": 0, "z": 0, "material": "minecraft:green_concrete"},
      {"x": 1, "y": 0, "z": 0, "material": "minecraft:black_concrete"}
    ]
  },
  "site_plan": {
    "origin": {"world": "minecraft:overworld", "x": 10, "y": 64, "z": 20},
    "pre_clear_blocks": [],
    "pre_foundation_blocks": [],
    "rationale": "放在玩家面前。"
  },
  "actions": [
    {"type": "place_blocks", "blueprint_id": "creeper_house_compact", "origin": {"world": "minecraft:overworld", "x": 10, "y": 64, "z": 20}}
  ]
}"#;

        let plan = parse_plan_response(output).unwrap();
        let blueprint = plan.blueprint.unwrap();
        let site_plan = plan.site_plan.unwrap();

        assert_eq!(blueprint.size.width, 2);
        assert_eq!(blueprint.size.height, 2);
        assert_eq!(blueprint.size.depth, 1);
        assert_eq!(blueprint.materials.len(), 2);
        assert!(site_plan.clear_existing.is_some_and(|value| !value));
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn expands_blueprint_primitives_before_parsing_plan() {
        let output = r#"{
  "reply": "开始复刻。",
  "summary": "建造蓝图 primitive-house",
  "blueprint": {
    "id": "primitive-house",
    "name": "Primitive 小屋",
    "description": "用 primitives 表达的蓝图。",
    "size": {"width": 5, "height": 4, "depth": 4},
    "spec": null,
    "materials": [],
    "blocks": [],
    "primitives": [
      {"type": "box", "from": {"x": 0, "y": 0, "z": 0}, "to": {"x": 4, "y": 0, "z": 3}, "material": "minecraft:stone_bricks"},
      {"type": "hollow_box", "from": {"x": 0, "y": 1, "z": 0}, "to": {"x": 4, "y": 3, "z": 3}, "material": "minecraft:oak_planks"}
    ],
    "tags": ["image_reference"]
  },
  "site_plan": null,
  "actions": []
}"#;

        let plan = parse_plan_response(output).unwrap();
        let blueprint = plan.blueprint.unwrap();

        assert_eq!(blueprint.blocks.len(), 74);
        assert_eq!(
            blueprint
                .materials
                .iter()
                .find(|item| item.material == "minecraft:stone_bricks")
                .map(|item| item.count),
            Some(20)
        );
        assert_eq!(
            blueprint
                .materials
                .iter()
                .find(|item| item.material == "minecraft:oak_planks")
                .map(|item| item.count),
            Some(54)
        );
        let spec = blueprint.spec.unwrap();
        assert_eq!(
            spec.get("format").and_then(serde_json::Value::as_str),
            Some("blockwright.blueprint_spec.v1")
        );
        assert_eq!(
            spec.get("primitives")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(2)
        );
    }

    #[test]
    fn parses_codex_plan_with_blueprint_even_when_wrapped() {
        let output = r#"这里是结果：
```json
{
  "reply": "开始做小塔。",
  "summary": "建造蓝图 tiny-tower",
  "blueprint": {
    "id": "tiny-tower",
    "name": "小塔",
    "description": "测试",
    "size": {"width": 1, "height": 1, "depth": 1},
    "materials": [{"material": "minecraft:stone", "count": 1}],
    "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:stone"}],
    "tags": ["tower"]
  },
  "site_plan": null,
  "actions": []
}
```"#;

        let plan = parse_plan_response(output).unwrap();
        let blueprint = plan.blueprint.unwrap();

        assert_eq!(blueprint.id, "tiny-tower");
        assert_eq!(blueprint.blocks[0].material, "minecraft:stone");
    }
}
