use crate::{
    domain::types::{
        BlockOrigin, Blueprint, ChatAttachment, ChatAttachmentKind, GameAction, PlayerPosition,
        WorldScan,
    },
    integrations::codex::{CodexClient, CodexResponseSchema},
    services::blueprint_store::BlueprintStore,
};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct PlannerInput {
    pub text: String,
    pub player: Option<String>,
    pub codex_session_key: Option<String>,
    pub position: Option<PlayerPosition>,
    pub nearby_scan: Option<WorldScan>,
    pub attachments: Vec<ChatAttachment>,
}

#[derive(Debug, Clone)]
pub struct PlanResult {
    pub reply: String,
    pub summary: String,
    pub actions: Vec<GameAction>,
}

#[derive(Debug, Deserialize)]
struct CodexActionPlan {
    reply: String,
    summary: String,
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
        let text = input.text.trim();
        let lower_text = text.to_lowercase();

        if wants_image_pipeline(text, &lower_text, &input.attachments) {
            if let Some(result) = self.try_codex_blueprint(&input, blueprints).await {
                return result;
            }

            return PlanResult {
                reply: "图片复刻会走图片分析流水线：识别结构、换算材料、生成蓝图、再由插件放置。当前骨架已经预留入口，下一步接入视觉分析。".to_string(),
                summary: "说明图片复刻流程".to_string(),
                actions: vec![GameAction::Chat {
                    message: "图片复刻能力还在接入中。".to_string(),
                }],
            };
        }

        if wants_build_request(text, &lower_text, &input.attachments) {
            if let Some(result) = self.try_codex_blueprint(&input, blueprints).await {
                return result;
            }

            if self.codex_enabled() {
                return PlanResult {
                    reply: "大模型没有生成有效蓝图，所以我没有下发建筑动作。你可以换一种说法，或者检查 controller 的 Codex CLI 日志。".to_string(),
                    summary: "大模型建筑规划失败".to_string(),
                    actions: vec![GameAction::Chat {
                        message: "建筑没有执行：大模型未返回有效蓝图。".to_string(),
                    }],
                };
            }

            if let Some(result) = self.try_builtin_house_blueprint(&input, blueprints).await {
                return result;
            }

            return PlanResult {
                reply: "当前没有启用大模型，也没有匹配到可用的本地建筑蓝图。".to_string(),
                summary: "缺少建筑规划能力".to_string(),
                actions: vec![GameAction::Chat {
                    message: "没有启用 Codex，也没有找到可用建筑蓝图。".to_string(),
                }],
            };
        }

        if let Some(result) = self.try_codex_action_plan(&input).await {
            return result;
        }

        if self.codex_enabled() {
            return PlanResult {
                reply: "Codex 没有返回可执行动作，所以我没有用本地关键词规则冒充理解。请看 controller 日志里的 Codex 错误，修好后再试。".to_string(),
                summary: "大模型动作理解失败".to_string(),
                actions: vec![GameAction::Chat {
                    message: "这次没有执行：Codex 未返回有效动作。".to_string(),
                }],
            };
        }

        if let Some(item) = requested_item(text, &lower_text) {
            return PlanResult {
                reply: format!("可以，已经准备给你{}。", item.reply_name),
                summary: format!("发放{}", item.summary_name),
                actions: vec![GameAction::GiveItem {
                    player: input.player,
                    item: item.item_id.to_string(),
                    count: item.count,
                }],
            };
        }

        if wants_diamonds(text, &lower_text) {
            return PlanResult {
                reply: "可以，已经准备给你 64 个钻石。".to_string(),
                summary: "发放钻石".to_string(),
                actions: vec![GameAction::GiveItem {
                    player: input.player,
                    item: "minecraft:diamond".to_string(),
                    count: 64,
                }],
            };
        }

        PlanResult {
            reply: "当前没有启用 Codex，也没有匹配到本地离线动作。请启用 Codex 后再让模型理解这类需求。".to_string(),
            summary: "普通对话".to_string(),
            actions: vec![GameAction::Chat {
                message: "没有启用 Codex，无法理解这类自然语言需求。".to_string(),
            }],
        }
    }

    fn codex_enabled(&self) -> bool {
        self.codex
            .as_ref()
            .map(CodexClient::enabled)
            .unwrap_or(false)
    }

    async fn try_builtin_house_blueprint(
        &self,
        input: &PlannerInput,
        blueprints: &BlueprintStore,
    ) -> Option<PlanResult> {
        let text = input.text.trim();
        let lower_text = text.to_lowercase();
        if !wants_house(text, &lower_text) {
            return None;
        }

        let blueprint = blueprints.first_by_tag("house").await?;
        let PlacementDecision::Ready {
            origin,
            clear_existing,
            pre_foundation_blocks,
            pre_clear_blocks,
            note: _,
        } = assess_placement(input, &blueprint);

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

        Some(PlanResult {
            reply: format!(
                "可以，我会按蓝图 `{}` 在你面前生成一个木屋。",
                blueprint.name
            ),
            summary: format!("建造蓝图 {}", blueprint.id),
            actions,
        })
    }

    async fn try_codex_blueprint(
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
            "starting codex blueprint planner"
        );

        let prompt = build_blueprint_prompt(input);
        let output = match codex
            .ask_with_schema(
                &prompt,
                CodexResponseSchema::Blueprint,
                input.codex_session_key.as_deref(),
            )
            .await
        {
            Ok(Some(output)) if !output.trim().is_empty() => output,
            Ok(_) => return None,
            Err(error) => {
                tracing::warn!(error = %error, "codex blueprint planning failed");
                return None;
            }
        };
        tracing::info!(
            response_bytes = output.len(),
            "codex blueprint response received; parsing blueprint json"
        );

        let blueprint = match parse_blueprint_response(&output) {
            Some(blueprint) => blueprint,
            None => {
                tracing::warn!("codex blueprint planning returned invalid json");
                return None;
            }
        };
        tracing::info!(
            blueprint_id = %blueprint.id,
            block_count = blueprint.blocks.len(),
            material_count = blueprint.materials.len(),
            "codex blueprint json parsed"
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
        tracing::info!(
            blueprint_id = %blueprint.id,
            block_count = blueprint.blocks.len(),
            "planned with codex blueprint planner"
        );
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

        Some(PlanResult {
            reply: format!(
                "我已经生成并保存蓝图 `{}`，{}会按这份蓝图在你面前建造。",
                blueprint.name, placement_note
            ),
            summary: format!("建造蓝图 {}", blueprint.id),
            actions,
        })
    }

    async fn try_codex_action_plan(&self, input: &PlannerInput) -> Option<PlanResult> {
        let codex = self.codex.as_ref()?;
        if !codex.enabled() {
            return None;
        }

        tracing::info!("starting codex action planner");
        let prompt = build_action_plan_prompt(input);
        let output = match codex
            .ask_with_schema(
                &prompt,
                CodexResponseSchema::ActionPlan,
                input.codex_session_key.as_deref(),
            )
            .await
        {
            Ok(Some(output)) if !output.trim().is_empty() => output,
            Ok(_) => return None,
            Err(error) => {
                tracing::warn!(error = %error, "codex action planning failed");
                return None;
            }
        };
        tracing::info!(
            response_bytes = output.len(),
            "codex action response received; parsing action json"
        );

        let plan = match parse_action_plan_response(&output) {
            Some(plan) => plan,
            None => {
                tracing::warn!("codex action planning returned invalid json");
                return None;
            }
        };
        if plan.actions.is_empty() {
            return None;
        }
        tracing::info!(
            summary = %plan.summary,
            action_count = plan.actions.len(),
            "planned with codex action planner"
        );

        Some(PlanResult {
            reply: plan.reply,
            summary: plan.summary,
            actions: plan.actions,
        })
    }
}

struct RequestedItem {
    item_id: &'static str,
    summary_name: &'static str,
    reply_name: &'static str,
    count: u32,
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
    let pre_foundation_blocks = if should_prepare_foundation(input, blueprint) {
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

    PlacementCandidate {
        origin,
        target_collisions,
        volume_collisions,
        distance_score,
        has_known_ground,
        surface_score,
    }
}

fn placement_candidate_score(
    candidate: &PlacementCandidate,
) -> (usize, i32, usize, usize, usize, usize) {
    (
        hard_collision_count(candidate),
        candidate.distance_score,
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
    collision_count(candidate) == 0
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
            .map(origin_in_front_of_player)
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

fn should_prepare_foundation(input: &PlannerInput, blueprint: &Blueprint) -> bool {
    let text = input.text.to_lowercase();
    let special_span_request = input.text.contains('桥')
        || input.text.contains("码头")
        || input.text.contains("栈桥")
        || input.text.contains("树屋")
        || text.contains("bridge")
        || text.contains("dock")
        || text.contains("pier")
        || text.contains("treehouse")
        || text.contains("tree house");
    let special_span_tag = blueprint.tags.iter().any(|tag| {
        let tag = tag.to_lowercase();
        matches!(
            tag.as_str(),
            "bridge" | "dock" | "pier" | "treehouse" | "tree_house"
        )
    });

    !special_span_request && !special_span_tag
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

fn requested_item(original: &str, lower_text: &str) -> Option<RequestedItem> {
    let items = [
        RequestedItem {
            item_id: "minecraft:diamond_pickaxe",
            summary_name: "钻石镐",
            reply_name: "一把钻石镐",
            count: 1,
        },
        RequestedItem {
            item_id: "minecraft:diamond_axe",
            summary_name: "钻石斧",
            reply_name: "一把钻石斧",
            count: 1,
        },
        RequestedItem {
            item_id: "minecraft:diamond_shovel",
            summary_name: "钻石铲",
            reply_name: "一把钻石铲",
            count: 1,
        },
        RequestedItem {
            item_id: "minecraft:diamond_hoe",
            summary_name: "钻石锄",
            reply_name: "一把钻石锄",
            count: 1,
        },
        RequestedItem {
            item_id: "minecraft:diamond_sword",
            summary_name: "钻石剑",
            reply_name: "一把钻石剑",
            count: 1,
        },
        RequestedItem {
            item_id: "minecraft:diamond_helmet",
            summary_name: "钻石头盔",
            reply_name: "一个钻石头盔",
            count: 1,
        },
        RequestedItem {
            item_id: "minecraft:diamond_chestplate",
            summary_name: "钻石胸甲",
            reply_name: "一个钻石胸甲",
            count: 1,
        },
        RequestedItem {
            item_id: "minecraft:diamond_leggings",
            summary_name: "钻石护腿",
            reply_name: "一个钻石护腿",
            count: 1,
        },
        RequestedItem {
            item_id: "minecraft:diamond_boots",
            summary_name: "钻石靴子",
            reply_name: "一双钻石靴子",
            count: 1,
        },
        RequestedItem {
            item_id: "minecraft:diamond_block",
            summary_name: "钻石块",
            reply_name: "64 个钻石块",
            count: 64,
        },
    ];

    let aliases = [
        (
            &items[0],
            &["钻石镐子", "钻石镐", "钻石稿子", "钻石稿"][..],
            "diamond pickaxe",
        ),
        (&items[1], &["钻石斧子", "钻石斧"][..], "diamond axe"),
        (&items[2], &["钻石铲子", "钻石铲"][..], "diamond shovel"),
        (&items[3], &["钻石锄头", "钻石锄"][..], "diamond hoe"),
        (&items[4], &["钻石剑"][..], "diamond sword"),
        (&items[5], &["钻石头盔"][..], "diamond helmet"),
        (&items[6], &["钻石胸甲"][..], "diamond chestplate"),
        (&items[7], &["钻石护腿"][..], "diamond leggings"),
        (&items[8], &["钻石靴子", "钻石鞋"][..], "diamond boots"),
        (&items[9], &["钻石块"][..], "diamond block"),
    ];

    aliases
        .iter()
        .find(|(item, chinese_aliases, english_alias)| {
            chinese_aliases.iter().any(|alias| original.contains(alias))
                || lower_text.contains(english_alias)
                || lower_text.contains(
                    item.item_id
                        .strip_prefix("minecraft:")
                        .unwrap_or(item.item_id),
                )
        })
        .map(|(item, _, _)| RequestedItem {
            item_id: item.item_id,
            summary_name: item.summary_name,
            reply_name: item.reply_name,
            count: item.count,
        })
}

fn wants_diamonds(original: &str, lower_text: &str) -> bool {
    original.contains("钻石") || lower_text.contains("diamond")
}

fn wants_image_pipeline(original: &str, lower_text: &str, attachments: &[ChatAttachment]) -> bool {
    original.contains("图片")
        || lower_text.contains("image")
        || attachments
            .iter()
            .any(|item| item.kind == ChatAttachmentKind::Image)
}

fn wants_build_request(original: &str, lower_text: &str, attachments: &[ChatAttachment]) -> bool {
    !wants_image_pipeline(original, lower_text, attachments)
        && (wants_house(original, lower_text)
            || wants_custom_build(original, lower_text)
            || [
                "建造",
                "修建",
                "搭建",
                "造一个",
                "做一个",
                "房间",
                "树屋",
                "小屋",
                "屋子",
                "别墅",
                "高楼",
                "城墙",
                "桥",
                "花园",
                "庭院",
                "农场",
                "仓库",
                "码头",
            ]
            .iter()
            .any(|keyword| original.contains(keyword))
            || [
                "tree house",
                "treehouse",
                "room",
                "cabin",
                "building",
                "bridge",
                "garden",
                "farm",
                "warehouse",
                "dock",
            ]
            .iter()
            .any(|keyword| lower_text.contains(keyword)))
}

fn wants_house(original: &str, lower_text: &str) -> bool {
    original.contains("房子") || original.contains("木屋") || lower_text.contains("house")
}

fn wants_custom_build(original: &str, lower_text: &str) -> bool {
    original.contains("建筑")
        || original.contains("盖")
        || original.contains("城堡")
        || original.contains("塔")
        || lower_text.contains("build")
        || lower_text.contains("castle")
        || lower_text.contains("tower")
}

fn origin_in_front_of_player(position: &PlayerPosition) -> BlockOrigin {
    BlockOrigin {
        world: Some(position.world.clone()),
        x: position.x.round() as i32 + 2,
        y: position.y.round() as i32,
        z: position.z.round() as i32 + 2,
    }
}

fn build_blueprint_prompt(input: &PlannerInput) -> String {
    let attachments =
        serde_json::to_string(&input.attachments).unwrap_or_else(|_| "[]".to_string());
    let site_context = build_site_context(input);
    format!(
        r#"你是 Blockwright 的 Minecraft 建筑规划器。请把用户需求规划成一个可保存、可执行、可校验的蓝图 JSON。

硬性规则：
- 只输出一个 JSON 对象，不要输出 Markdown、解释或代码块。
- JSON 必须符合字段：id、name、description、size、materials、blocks、tags。
- blocks 里的 x/y/z 必须是相对坐标，不能输出世界绝对坐标。
- 方块材质必须使用 Minecraft 命名空间 ID，例如 minecraft:oak_planks。
- 需要表达方块状态时可以写在 material 里，例如 minecraft:oak_leaves[persistent=true]、minecraft:oak_door[half=lower,facing=south]。
- 先生成蓝图，再由执行端按同一份 blocks 放置；不要输出命令步骤、背包操作或玩家右键操作。
- 第一阶段蓝图规模控制在 500 个方块以内，优先用常见原版方块。
- materials 必须和 blocks 统计一致。
- 先理解玩家真正想要的建筑，再规划结构、尺寸、材料、关键部位和摆放方式。
- 蓝图最低的普通地板/地基层默认从相对 y=0 开始；不要把世界绝对高度写进 blocks，也不要无故使用负 y。
- 默认假设 controller 会把蓝图 y=0 放在玩家面向目标点附近的第一层空气上，优先使用玩家正面目标点，而不是随便搬到远处空地。
- 如果目标点是坑、水边、坡地或奇怪地形，不要拒绝；要把地形融入设计，让建筑通过平台、露台、木桩、石砖基座、楼梯、桥接或挡土墙自然贴合场地。
- 住宅、木屋、树屋、房间这类可居住建筑，默认要能实际使用：至少有完整地板、墙、屋顶、可通行入口、两格高室内空间、床、照明和基础窗户，除非玩家明确只要外观模型。
- 门要按两格结构输出上下两块，例如同一个位置 y=1 用 minecraft:oak_door[half=lower,facing=south]，y=2 用 minecraft:oak_door[half=upper,facing=south]，并让入口前后留出通行空间。
- 床要按 head/foot 两块输出，朝向一致，周围至少留一格可站立空间。
- 树屋、庭院、树冠、装饰树叶必须避免自然凋零：优先使用 minecraft:oak_leaves[persistent=true] 这类 persistent=true 叶子；如果不使用 persistent=true，就必须保证叶子离对应原木足够近。
- 室内不能被实心方块填满；家具、床、火把、梯子、楼梯等要留出玩家移动路径，不要只生成封闭外壳。
- 照明优先用 torch、lantern、glowstone 等稳定光源，封闭建筑内部至少放一个光源，避免夜晚不可用。
- 悬空建筑、树屋和二楼必须有可到达路径，例如梯子、楼梯或台阶；不要生成玩家无法进入的房间。
- 入口要面向或连通玩家侧的室外路径，不能把唯一入口贴在墙、悬崖、水面、坑洞或不可通行区域上；如果目标地形复杂，要设计台阶、平台或桥接让入口可达。
- 较宽建筑要有完整地板或美观地基，让它看起来坐落在地形里；不要只靠一个角或一根柱子支撑普通房屋。
- 水、岩浆、火、沙子/沙砾、红石机关、门、床、告示牌等有特殊状态或物理特性的方块，只有能明确表达状态和安全放置时才使用。
- description 用中文简短写清楚设计思路和处理方式。
- 玩家说“生成/建造/做一个/我要一个 + 建筑物名”时，直接生成可执行小型蓝图，不要返回聊天提示。
- 你会收到 controller 的场地摘要；生成蓝图时要假设扫描中心就是玩家面前想要处理的位置。controller 会尽量在这个目标点或很小范围内落位；地形不理想时会优先做场地融合和美观支撑，而不是直接拒绝或远距离迁移。
- 如果这是同一会话里的后续反馈，例如“抬高一点”“往左一点”“纠正地基”“重新设计入口”，要理解成对当前建筑的调整思路，而不是重新换一块地。

用户文字：
{text}

场地摘要：
{site_context}

附件元数据：
{attachments}
"#,
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

fn build_action_plan_prompt(input: &PlannerInput) -> String {
    format!(
        r#"你是 Blockwright 的 Minecraft 指令理解器。请把玩家自然语言转换成 Blockwright controller 可执行的动作 JSON。

硬性规则：
- 只输出一个 JSON 对象，不要输出 Markdown、解释或代码块。
- JSON 必须符合：{{"reply":"中文回复","summary":"短中文摘要","actions":[...]}}
- actions 当前只允许：
  1. 发物品：{{"type":"give_item","player":null,"item":"minecraft:diamond_pickaxe","count":1}}
  2. 执行 Minecraft 指令：{{"type":"run_command","command":"time set day"}}
  3. 聊天提示：{{"type":"chat","message":"中文提示"}}
- 如果结构化输出 schema 要求保留未使用字段，未使用字段填 null，不要填假值。
- 需要识别完整物品名，不能只因为文本包含“钻石”就发 minecraft:diamond。
- 例如“钻石镐/钻石稿子/diamond pickaxe”应是 minecraft:diamond_pickaxe。
- 例如“钻石斧/diamond axe”应是 minecraft:diamond_axe。
- 例如“钻石剑/diamond sword”应是 minecraft:diamond_sword。
- 例如“给我钻石”才是 minecraft:diamond，count 为 64。
- 这个动作理解器只处理物品和普通聊天；建筑需求会在进入这里之前由蓝图规划器处理。
- 对能用原版 Minecraft 指令完成的需求，输出 run_command。command 不要带开头的 `/`。
- 例如“我想白天/天亮吧”应是 time set day。
- 例如“我想晚上”应是 time set night。
- 例如“别下雨/天气晴朗”应是 weather clear。
- 例如“下雨吧”应是 weather rain。
- 例如“我想创造模式”应是 gamemode creative {player}。
- 例如“我想回生存”应是 gamemode survival {player}。
- 例如“给我速度/我要夜视”可用 effect give {player} minecraft:speed 120 1 true / effect give {player} minecraft:night_vision 600 0 true。
- 允许使用的命令根只包括：time、weather、difficulty、gamerule、gamemode、effect、enchant、experience、xp、tp、teleport、spawnpoint、setworldspawn、summon。
- 不要输出 op、deop、stop、reload、ban、kick、whitelist、save-all、execute、fill、setblock、data、function 等危险或大范围命令。
- 如果用户文字不能安全映射成物品或白名单 Minecraft 指令，返回普通 chat 提示。
- item 必须使用 Minecraft 命名空间 ID，count 必须大于 0。

玩家名：
{player}

用户文字：
{text}
"#,
        player = input.player.as_deref().unwrap_or("unknown"),
        text = input.text.trim()
    )
}

fn parse_blueprint_response(output: &str) -> Option<Blueprint> {
    let json = extract_json_object(output.trim())?;
    serde_json::from_str(json).ok()
}

fn parse_action_plan_response(output: &str) -> Option<CodexActionPlan> {
    let json = extract_json_object(output.trim())?;
    serde_json::from_str(json).ok()
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
            Blueprint, BlueprintBlock, BlueprintSize, ChatAttachmentSource, MaterialCount,
            WorldScanBlock,
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

    fn planner_with_fake_codex(name: &str, final_message: &str) -> Planner {
        let dir = temp_dir(name);
        fs::create_dir_all(&dir).unwrap();
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
    *)
      shift
      ;;
  esac
done
cat >/dev/null
if [[ -z "$last_message" ]]; then
  exit 2
fi
cat > "$last_message" <<'BLOCKWRIGHT_JSON'
{final_message}
BLOCKWRIGHT_JSON
"#
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

    #[tokio::test]
    async fn plans_diamond_sword() {
        let store = empty_store("sword").await;
        let result = Planner::default()
            .plan(
                PlannerInput {
                    text: "给我一把钻石剑".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
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
        let result = Planner::default()
            .plan(
                PlannerInput {
                    text: "give me diamonds".to_string(),
                    player: Some("Alex".to_string()),
                    codex_session_key: None,
                    position: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
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
    async fn fallback_plans_diamond_pickaxe_before_loose_diamond_match() {
        let store = empty_store("diamond-pickaxe").await;
        let result = Planner::default()
            .plan(
                PlannerInput {
                    text: "我要一个钻石稿子".to_string(),
                    player: Some("Alex".to_string()),
                    codex_session_key: None,
                    position: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
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
        let planner = planner_with_fake_codex("codex-invalid-action", "not json");

        let result = planner
            .plan(
                PlannerInput {
                    text: "给我钻石".to_string(),
                    player: Some("Alex".to_string()),
                    codex_session_key: None,
                    position: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "大模型动作理解失败");
        assert!(matches!(result.actions[0], GameAction::Chat { .. }));
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
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 blocked-room");
        assert!(result.reply.contains("自动选择更合适落点"));
        assert!(matches!(
            result.actions[0],
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
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 supported-floor-room");
        assert!(result.reply.contains("融入地形的木桩平台"));
        assert_eq!(result.actions.len(), 2);
        assert!(matches!(
            &result.actions[0],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                origin: BlockOrigin {
                    x: 19,
                    y: 64,
                    z: 29,
                    ..
                },
                blocks,
                clear_existing: true,
            } if blueprint_id == "supported-floor-room:site-foundation"
                && blocks.len() == 8
                && blocks.iter().all(|block| block.y == -1)
                && blocks.iter().all(|block| block.material == "minecraft:oak_planks")
        ));
        assert!(matches!(
            &result.actions[1],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                origin: BlockOrigin {
                    x: 19,
                    y: 64,
                    z: 29,
                    ..
                },
                ..
            } if blueprint_id == "supported-floor-room"
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
    async fn plans_house_from_blueprint_tag() {
        let store = empty_store("house").await;
        store
            .save(test_blueprint("test-house", vec!["house"]))
            .await
            .unwrap();

        let result = Planner::default()
            .plan(
                PlannerInput {
                    text: "帮我盖一个木屋".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                },
                &store,
            )
            .await;

        assert!(matches!(result.actions[0], GameAction::PlaceBlocks { .. }));
    }

    #[tokio::test]
    async fn places_house_in_front_of_player_position() {
        let store = empty_store("house-origin").await;
        store
            .save(test_blueprint("test-house", vec!["house"]))
            .await
            .unwrap();

        let result = Planner::default()
            .plan(
                PlannerInput {
                    text: "build a house".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: Some(PlayerPosition {
                        world: "world_nether".to_string(),
                        x: 10.4,
                        y: 65.2,
                        z: -3.6,
                        yaw: None,
                        pitch: None,
                    }),
                    nearby_scan: None,
                    attachments: Vec::new(),
                },
                &store,
            )
            .await;

        assert!(matches!(
            result.actions[0],
            GameAction::PlaceBlocks {
                origin: BlockOrigin {
                    ref world,
                    x: 12,
                    y: 65,
                    z: -2
                },
                ..
            } if world.as_deref() == Some("world_nether")
        ));
    }

    #[tokio::test]
    async fn explains_missing_house_blueprint() {
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
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "缺少建筑规划能力");
        assert!(matches!(result.actions[0], GameAction::Chat { .. }));
    }

    #[tokio::test]
    async fn explains_image_pipeline_and_default_capabilities() {
        let store = empty_store("fallback").await;
        let image_result = Planner::default()
            .plan(
                PlannerInput {
                    text: "帮我根据图片复刻建筑".to_string(),
                    player: None,
                    codex_session_key: None,
                    position: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                },
                &store,
            )
            .await;
        let fallback_result = Planner::default()
            .plan(
                PlannerInput {
                    text: "你好".to_string(),
                    player: None,
                    codex_session_key: None,
                    position: None,
                    nearby_scan: None,
                    attachments: Vec::new(),
                },
                &store,
            )
            .await;

        assert_eq!(image_result.summary, "说明图片复刻流程");
        assert_eq!(fallback_result.summary, "普通对话");
        assert!(!fallback_result.reply.contains("第一版"));
        assert!(!fallback_result.reply.contains("后续会接 Codex"));
        assert!(matches!(
            fallback_result.actions[0],
            GameAction::Chat { .. }
        ));
    }

    #[tokio::test]
    async fn image_attachment_enters_image_pipeline_without_magic_text() {
        let store = empty_store("image-attachment").await;
        let result = Planner::default()
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
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "说明图片复刻流程");
    }

    #[test]
    fn blueprint_prompt_embeds_consistency_rules_for_codex() {
        let prompt = build_blueprint_prompt(&PlannerInput {
            text: "照图片盖一个小塔".to_string(),
            player: None,
            codex_session_key: None,
            position: None,
            nearby_scan: None,
            attachments: Vec::new(),
        });

        assert!(prompt.contains("只输出一个 JSON 对象"));
        assert!(prompt.contains("相对坐标"));
        assert!(prompt.contains("同一份 blocks 放置"));
        assert!(prompt.contains("设计思路"));
        assert!(prompt.contains("minecraft:oak_leaves[persistent=true]"));
        assert!(prompt.contains("床"));
        assert!(prompt.contains("两格高室内空间"));
        assert!(prompt.contains("half=lower"));
        assert!(prompt.contains("玩家面向目标点"));
        assert!(prompt.contains("相对 y=0"));
        assert!(prompt.contains("入口要面向或连通玩家侧"));
        assert!(prompt.contains("同一会话里的后续反馈"));
    }

    #[test]
    fn action_plan_prompt_requires_complete_item_understanding() {
        let prompt = build_action_plan_prompt(&PlannerInput {
            text: "我要钻石稿子".to_string(),
            player: Some("Steve".to_string()),
            codex_session_key: None,
            position: None,
            nearby_scan: None,
            attachments: Vec::new(),
        });

        assert!(prompt.contains("不能只因为文本包含“钻石”"));
        assert!(prompt.contains("minecraft:diamond_pickaxe"));
        assert!(prompt.contains("time set day"));
        assert!(prompt.contains("gamemode creative Steve"));
        assert!(!prompt.contains("需要走建筑规划流程"));
    }

    #[test]
    fn parses_codex_action_plan_for_diamond_pickaxe() {
        let output = r#"{
  "reply": "可以，已经准备给你一把钻石镐。",
  "summary": "发放钻石镐",
  "actions": [
    {"type":"give_item","player":null,"item":"minecraft:diamond_pickaxe","count":1}
  ]
}"#;

        let plan = parse_action_plan_response(output).unwrap();

        assert_eq!(plan.summary, "发放钻石镐");
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
    fn parses_codex_action_plan_for_minecraft_command() {
        let output = r#"{
  "reply": "可以，已经切到白天。",
  "summary": "设置为白天",
  "actions": [
    {"type":"run_command","command":"time set day"}
  ]
}"#;

        let plan = parse_action_plan_response(output).unwrap();

        assert_eq!(plan.summary, "设置为白天");
        assert!(matches!(
            plan.actions[0],
            GameAction::RunCommand { ref command } if command == "time set day"
        ));
    }

    #[test]
    fn parses_schema_constrained_action_plan_with_nullable_variant_fields() {
        let output = r#"{
  "reply": "可以，已经切到白天。",
  "summary": "设置为白天",
  "actions": [
    {"type":"run_command","player":null,"item":null,"count":null,"command":"time set day","message":null}
  ]
}"#;

        let plan = parse_action_plan_response(output).unwrap();

        assert_eq!(plan.summary, "设置为白天");
        assert!(matches!(
            plan.actions[0],
            GameAction::RunCommand { ref command } if command == "time set day"
        ));
    }

    #[test]
    fn parses_codex_blueprint_json_even_when_wrapped() {
        let output = r#"这里是结果：
```json
{
  "id": "tiny-tower",
  "name": "小塔",
  "description": "测试",
  "size": {"width": 1, "height": 1, "depth": 1},
  "materials": [{"material": "minecraft:stone", "count": 1}],
  "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:stone"}],
  "tags": ["tower"]
}
```"#;

        let blueprint = parse_blueprint_response(output).unwrap();

        assert_eq!(blueprint.id, "tiny-tower");
        assert_eq!(blueprint.blocks[0].material, "minecraft:stone");
    }
}
