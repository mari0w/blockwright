use crate::{
    domain::types::{
        BlockOrigin, Blueprint, ChatAttachment, GameAction, PlayerPosition, WorldScan,
    },
    integrations::codex::{CodexClient, CodexResponseSchema},
    services::blueprint_store::BlueprintStore,
};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const PLAYER_SAFETY_RADIUS: i32 = 1;
const PLAYER_SAFETY_HEIGHT_BLOCKS: i32 = 3;

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
    #[serde(default)]
    blueprint: Option<Blueprint>,
    #[serde(default)]
    actions: Vec<GameAction>,
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
        if !self.codex_enabled() {
            return PlanResult {
                reply: "我现在还没有连上 AI 建造助手，暂时不能理解自然语言请求。请先让管理员检查后台配置。".to_string(),
                summary: "AI 助手未启用".to_string(),
                actions: Vec::new(),
            };
        }

        if let Some(result) = self.try_codex_plan(&input, blueprints).await {
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

        let prompt = build_plan_prompt(input);
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
                return None;
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
                .actions_for_blueprint(input, blueprints, blueprint)
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
        } = assess_placement(input, &blueprint);
        tracing::info!(
            blueprint_id = %blueprint.id,
            world = ?origin.world,
            origin_x = origin.x,
            origin_y = origin.y,
            origin_z = origin.z,
            clear_existing,
            pre_foundation_count = pre_foundation_blocks.len(),
            pre_clear_count = pre_clear_blocks.len(),
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

fn build_plan_prompt(input: &PlannerInput) -> String {
    let attachments =
        serde_json::to_string(&input.attachments).unwrap_or_else(|_| "[]".to_string());
    let site_context = build_site_context(input);
    format!(
        r#"你是 Blockwright 的 Minecraft AI 助手。流程由你和可用 skills 决定，外层服务只负责协议校验和安全边界，不做关键词识别，也不替你选择工作流。

输出协议：
- 只返回一个 JSON 对象，字段为 reply、summary、blueprint、actions。
- reply 给普通玩家看，必须自然、简短、好懂；不要暴露 JSON、schema、队列、内部服务名、planner、Codex 错误等技术细节。
- summary 是中文短摘要。
- 只聊天、追问、解释方案时：blueprint=null，actions=[]。
- 新建建筑、模型或场景时：调用并遵循 blockwright-build-planning skill，把结果放到 blueprint，actions=[]；Blockwright 会保存蓝图、选择落点并生成 Minecraft 放置动作。
- 需要先让 Minecraft 扫描现场时：blueprint=null，actions 输出 scan_nearby_and_plan，并把用户文字和附件带回去。
- 已经有足够世界坐标、方块清单或现场扫描时，可以直接在 actions 输出 place_blocks、give_item、run_command 或 chat。
- 建筑改造、整体重做、按图片调整、补细节等调用并遵循 blockwright-existing-build-edit skill；新建建筑调用 blockwright-build-planning skill。如果你无法确定目标或做法，就先在 reply 里追问，不要硬下动作。
- Minecraft 方块 material 使用命名空间 ID，可携带方块状态；蓝图 blocks 用相对坐标。
- run_command 不带开头的 `/`，只做小范围、明确、安全的操作。

玩家名：
{player}

用户文字：
{text}

场地摘要：
{site_context}

附件元数据：
{attachments}
"#,
        player = input.player.as_deref().unwrap_or("unknown"),
        text = input.text.trim(),
        site_context = site_context,
        attachments = attachments
    )
}

fn build_site_context(input: &PlannerInput) -> String {
    let Some(scan) = input.nearby_scan.as_ref() else {
        return "未收到附近场地扫描；只能按玩家位置估算地面和落点。".to_string();
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
    let mut material_counts = HashMap::<String, u32>::new();
    for block in &scan.blocks {
        *material_counts.entry(block.material.clone()).or_default() += 1;
    }
    let mut materials = material_counts.into_iter().collect::<Vec<_>>();
    materials.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let material_summary = materials
        .into_iter()
        .take(6)
        .map(|(material, count)| format!("{material} x{count}"))
        .collect::<Vec<_>>()
        .join("、");

    format!(
        "world={}，扫描中心=({},{},{})，半径={}，非空气方块={}，估算地面 y={}，主要材料={}。落点原则：扫描中心是玩家面前目标点，优先在这里或小范围内创建；地形不理想时做美观的场地融合、支撑、台阶或平台，入口要保留可达路径。",
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
    )
}

fn parse_plan_response(output: &str) -> Option<CodexPlan> {
    let json = extract_json_object(output.trim())?;
    serde_json::from_str(json).ok()
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
schema=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-last-message)
      last_message="$2"
      shift 2
      ;;
    --output-schema)
      schema="$2"
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
case "$schema" in
  *plan.schema.json)
    cat "{plan_path}" > "$last_message"
    ;;
  *)
    exit 3
    ;;
esac
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
            r#"{"reply":"可以，我们先聊方案。你想偏木屋、城堡还是现代风？","summary":"讨论建造方案","blueprint":null,"actions":[]}"#,
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

    #[test]
    fn plan_prompt_keeps_workflow_in_codex_and_skills() {
        let prompt = build_plan_prompt(&PlannerInput {
            text: "照图片盖一个小塔".to_string(),
            player: None,
            codex_session_key: None,
            position: None,
            nearby_scan: None,
            attachments: Vec::new(),
            progress_id: None,
        });

        assert!(prompt.contains("blockwright-build-planning skill"));
        assert!(prompt.contains("只返回一个 JSON 对象"));
        assert!(prompt.contains("不做关键词识别"));
        assert!(prompt.contains("流程由你和可用 skills 决定"));
        assert!(prompt.contains("blockwright-existing-build-edit skill"));
        assert!(prompt.contains("blueprint=null，actions=[]"));
        assert!(prompt.contains("scan_nearby_and_plan"));
        assert!(prompt.contains("place_blocks"));
        assert!(prompt.contains("相对坐标"));
        assert!(prompt.contains("命名空间 ID"));
        assert!(prompt.contains("give_item"));
        assert!(prompt.contains("run_command"));
    }

    #[test]
    fn parses_codex_plan_for_diamond_pickaxe() {
        let output = r#"{
  "reply": "可以，已经准备给你一把钻石镐。",
  "summary": "发放钻石镐",
  "blueprint": null,
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
    fn parses_codex_plan_for_minecraft_command() {
        let output = r#"{
  "reply": "可以，已经切到白天。",
  "summary": "设置为白天",
  "blueprint": null,
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
  "actions": []
}
```"#;

        let plan = parse_plan_response(output).unwrap();
        let blueprint = plan.blueprint.unwrap();

        assert_eq!(blueprint.id, "tiny-tower");
        assert_eq!(blueprint.blocks[0].material, "minecraft:stone");
    }
}
