use crate::{
    domain::types::{
        BlockOrigin, Blueprint, BlueprintBlock, BlueprintSize, ChatAttachment, GameAction,
        MaterialCount, PlayerPosition, WorldScan, WorldScanBlock,
    },
    integrations::codex::{CodexClient, CodexResponseSchema},
    services::{blueprint_store::BlueprintStore, build_store::BuildStore},
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const PLAYER_SAFETY_RADIUS: i32 = 1;
const PLAYER_SAFETY_HEIGHT_BLOCKS: i32 = 3;
const CONTEXT_BLUEPRINT_LIMIT: usize = 24;
const CONTEXT_BUILD_LIMIT: usize = 12;
const CONTEXT_BLUEPRINT_BLOCK_SAMPLE_LIMIT: usize = 32;
const CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT: usize = 32;

#[derive(Debug, Clone)]
pub struct PlannerInput {
    pub text: String,
    pub player: Option<String>,
    pub codex_session_key: Option<String>,
    pub position: Option<PlayerPosition>,
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

#[derive(Debug, Deserialize)]
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
            reply: "我这次没有整理出可靠的下一步。你可以直接说想聊方案、要我先看附近场地，或者告诉我准备建什么、改什么。"
                .to_string(),
            summary: "继续确认需求".to_string(),
            actions: Vec::new(),
        }
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
        tracing::info!(
            prompt_bytes = prompt.len(),
            available_blueprint_count = context.available_blueprints.len(),
            recent_build_count = context.recent_builds.len(),
            "codex unified planner prompt prepared"
        );
        let output = match codex
            .ask_with_schema_and_progress(
                &prompt,
                CodexResponseSchema::Plan,
                input.codex_session_key.as_deref(),
                input.progress_id.as_deref(),
            )
            .await
        {
            Ok(Some(output)) if !output.trim().is_empty() => output,
            Ok(_) => return None,
            Err(error) => {
                tracing::warn!(error = %error, "codex unified planning failed");
                return Some(PlanResult {
                    reply: "AI 建造助手这次调用失败了，任务还没有发送到 Minecraft。请管理员检查 Codex 登录状态、模型权限、网络连接或 CLI 版本。"
                        .to_string(),
                    summary: "AI 助手调用失败".to_string(),
                    actions: Vec::new(),
                });
            }
        };
        tracing::info!(
            response_bytes = output.len(),
            "codex plan response received; parsing json"
        );

        let plan = match parse_plan_response(&output) {
            Some(plan) => plan,
            None => {
                tracing::warn!("codex unified planning returned invalid json");
                return None;
            }
        };
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
            actions.push(GameAction::PlaceBlocks {
                blueprint_id: Some(format!("{}:site-foundation", blueprint.id)),
                origin: origin.clone(),
                blocks: pre_foundation_blocks,
                clear_existing: true,
            });
        }
        if !pre_clear_blocks.is_empty() {
            actions.push(GameAction::PlaceBlocks {
                blueprint_id: Some(format!("{}:site-clear", blueprint.id)),
                origin: origin.clone(),
                blocks: pre_clear_blocks,
                clear_existing: true,
            });
        }
        actions.push(GameAction::PlaceBlocks {
            blueprint_id: Some(blueprint.id.clone()),
            origin,
            blocks: blueprint.blocks.clone(),
            clear_existing,
        });

        Some((actions, placement_note))
    }
}

fn action_type_name(action: &GameAction) -> &'static str {
    match action {
        GameAction::GiveItem { .. } => "give_item",
        GameAction::PlaceBlocks { .. } => "place_blocks",
        GameAction::RunCommand { .. } => "run_command",
        GameAction::Chat { .. } => "chat",
        GameAction::ScanNearbyAndPlan { .. } => "scan_nearby_and_plan",
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
            controller_role: "controller 提供 context_bundle、保存蓝图、登记构建记录、校验协议和执行安全边界；具体工作流和方案由模型结合 skills 自主决定。优先使用已有上下文与 skills，只有在确实缺数据且 MCP 工具能直接补齐时才发起 MCP。",
            safety_boundary: "执行端仍会拦截危险命令、玩家安全区内放置、超出上限的方块和放置后校验不一致。",
            targeting_policy: "建筑或改造需求先看离玩家位置最近的候选；没有玩家位置时看扫描中心最近候选。最近候选不确定、多个候选都合理或目标部位不明确时，只回复确认问题，不输出 Minecraft 动作。",
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
        },
    }
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
                    actions: record
                        .expected_actions
                        .into_iter()
                        .map(|action| {
                            let block_count = action.blocks.len();
                            BuildActionContext {
                                blueprint_id: action.blueprint_id,
                                origin: action.origin,
                                expected_count: action.expected_count,
                                materials: action.materials,
                                block_sample_limit: CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT,
                                block_sample_truncated: block_count
                                    > CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT,
                                block_sample: block_sample(
                                    &action.blocks,
                                    CONTEXT_BUILD_ACTION_BLOCK_SAMPLE_LIMIT,
                                ),
                            }
                        })
                        .collect(),
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
        r#"你是 Blockwright 的 Minecraft AI 助手。你负责理解玩家意图、选择工作流、设计蓝图或动作，并根据可用 skills 自主决定怎么完成。

工作原则：先消费 context_bundle 里的基础数据源（位置、扫描、历史构建、蓝图、附件），再组合可用 skills 形成闭环方案；缺数据时主动用 scan_nearby_and_plan 或确认问题补齐。
- 能直接从 context_bundle 得到的数据，不要再走 MCP 工具重复获取；只有缺关键信息且 MCP 能低成本补齐时才调用。
- available_blueprints 和 recent_builds 只会在本轮确实可能复用蓝图、匹配或改造既有建筑时预加载；为空不代表没有历史，只代表本轮不需要把历史硬塞进上下文。
- 适合稳定流程复用的步骤交给 skills，适合一次性读写或查询的动作交给工具；不要把简单查询强行包装成复杂 workflow。

controller 的角色只是提供基础数据源、保存蓝图、校验协议和执行安全边界；不要把它当成会替你做规划判断的规则引擎。建筑规范、场地策略、图片复刻、改造和安全命令规则都优先按可用 skills 执行。

让流程丝滑：
- 同一轮尽量给出可执行下一步；能落地就输出动作，不能落地就只问最关键的一个澄清问题。
- 优先复用 available_blueprints 和 recent_builds 的摘要、材料和方块样本，避免无谓重画；如果改造旧建筑需要完整细节而样本不够，先输出 scan_nearby_and_plan 获取现场。
- 需要改造既有建筑时，必须先基于 nearby_scan + recent_builds 做匹配；匹配不唯一就追问，不要直接施工。
- 玩家提到“按图生成”时，优先走 image-to-blueprint 能力；玩家提到“修改/扩建/换材质”时，优先走 existing-build-edit 能力。

输出协议很薄：
- 只返回一个 JSON 对象，字段为 reply、summary、blueprint、site_plan、actions。
- reply 给玩家看，保持自然、简洁，不暴露 JSON、schema、planner、Codex、队列等内部细节。
- 如果只是聊天、解释或需要追问，blueprint=null，site_plan=null，actions=[]。
- 如果输出 blueprint，尽量同时输出 site_plan 来表达你选择的落点、清理、地基或场地融合意图；如果暂时缺少坐标，可以让 site_plan.origin=null。
- 涉及门、床、树叶等方块时，material 里要写完整状态（例如 half/head-foot/persistent），并在蓝图与校验语义上保持一致。
- 建筑审美默认要“可居住 + 好看”：除基础木石外，主动考虑颜色搭配、层次和点缀材料（如染色玻璃、陶瓦、混凝土、灯笼、旗帜、花叶等），避免全程只用最原始素材。
- 如果需要 Minecraft 再扫描现场，输出 scan_nearby_and_plan。
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

fn parse_plan_response(output: &str) -> Option<CodexPlan> {
    let json = extract_json_object(output.trim())?;
    let value = serde_json::from_str::<serde_json::Value>(json).ok()?;
    if !has_required_plan_protocol_fields(&value) {
        return None;
    }
    serde_json::from_value(value).ok()
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

fn extract_json_object(output: &str) -> Option<&str> {
    let start = output.find('{')?;
    let end = output.rfind('}')?;
    if start > end {
        return None;
    }
    Some(&output[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::CodexConfig,
        domain::types::{
            Blueprint, BlueprintBlock, BlueprintSize, ChatAttachmentKind, ChatAttachmentSource,
            MaterialCount, WorldScanBlock,
        },
    };
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
        let dir = temp_dir(name);
        fs::create_dir_all(&dir).unwrap();
        let plan_path = dir.join("plan.json");
        fs::write(&plan_path, plan_message).unwrap();
        let script_path = dir.join("fake-codex.sh");
        fs::write(
            &script_path,
            format!(
                r#"#!/usr/bin/env bash
set -euo pipefail
last_message=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-last-message)
      last_message="$2"
      shift 2
      ;;
    --output-schema)
      echo "unexpected --output-schema" >&2
      exit 3
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
cat "{plan_path}" > "$last_message"
"#,
                plan_path = plan_path.to_string_lossy()
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
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "继续确认需求");
        assert!(result.actions.is_empty());
        assert!(result.reply.contains("可靠的下一步"));
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
    }

    #[tokio::test]
    async fn build_failure_reply_contains_rephrase_hints_when_codex_enabled() {
        let store = empty_store("codex-invalid-blueprint").await;
        let planner = planner_with_fake_codex("codex-invalid-blueprint", "not json");

        let result = planner
            .plan(
                PlannerInput {
                    text: "帮我盖一个木屋".to_string(),
                    player: Some("Alex".to_string()),
                    codex_session_key: None,
                    position: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                    progress_id: None,
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "继续确认需求");
        assert!(result.actions.is_empty());
        assert!(result.reply.contains("准备建什么、改什么"));
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
  "name": "图片参考小屋",
  "description": "按附件图片生成的简化建筑。",
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
    async fn plan_prompt_uses_context_bundle_and_keeps_workflow_in_codex() {
        let store = empty_store("prompt-context").await;
        let input = PlannerInput {
            text: "照图片盖一个小塔".to_string(),
            player: None,
            codex_session_key: None,
            position: None,
            nearby_scan: None,
            attachments: Vec::new(),
            progress_id: None,
        };
        let context = build_context_bundle(&input, &store, None).await;
        let prompt = render_plan_prompt(&context);

        assert!(prompt.contains("context_bundle"));
        assert!(prompt.contains("controller 的角色只是提供基础数据源"));
        assert!(prompt.contains("只返回一个 JSON 对象"));
        assert!(prompt.contains("site_plan"));
        assert!(prompt.contains("可用 skills 自主决定"));
        assert!(prompt.contains("available_blueprints"));
        assert!(prompt.contains("recent_builds"));
        assert!(prompt.contains("建筑或改造需求先看离玩家位置最近的候选"));
        assert!(prompt.contains("blockwright-site-selection"));
        assert!(prompt.contains("scan_nearby_and_plan"));
        assert!(prompt.contains("相对坐标"));
        assert!(prompt.contains("命名空间 ID"));
        assert!(prompt.contains("give_item"));
        assert!(prompt.contains("run_command"));
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
    fn rejects_codex_plan_missing_required_protocol_fields() {
        let output = r#"{
  "reply": "可以，已经准备给你一把钻石镐。",
  "summary": "发放钻石镐",
  "actions": [
    {"type":"give_item","player":null,"item":"minecraft:diamond_pickaxe","count":1}
  ]
}"#;

        assert!(parse_plan_response(output).is_none());
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
