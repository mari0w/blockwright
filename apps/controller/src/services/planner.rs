use crate::{
    domain::types::{
        BlockOrigin, Blueprint, BuildRecord, ChatAttachment, GameAction, PlayerPosition, WorldScan,
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

#[derive(Debug, Deserialize)]
struct CodexExistingEditPlan {
    reply: String,
    summary: String,
    mode: ExistingEditMode,
    actions: Vec<GameAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExistingEditMode {
    Patch,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannerIntentKind {
    Blueprint,
    Action,
    ExistingBuildEdit,
    Chat,
}

impl PlannerIntentKind {
    fn as_str(self) -> &'static str {
        match self {
            PlannerIntentKind::Blueprint => "blueprint",
            PlannerIntentKind::Action => "action",
            PlannerIntentKind::ExistingBuildEdit => "existing_build_edit",
            PlannerIntentKind::Chat => "chat",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlannerIntent {
    pub intent: PlannerIntentKind,
    pub reply: String,
    pub summary: String,
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
        let intent = self.classify_intent(&input).await;
        self.plan_with_intent(input, blueprints, intent).await
    }

    pub async fn classify_intent(&self, input: &PlannerInput) -> Option<PlannerIntent> {
        let codex = self.codex.as_ref()?;
        if !codex.enabled() {
            return None;
        }

        tracing::info!(
            has_nearby_scan = input.nearby_scan.is_some(),
            attachment_count = input.attachments.len(),
            "starting codex intent classifier"
        );
        let prompt = build_intent_prompt(input);
        let output = match codex
            .ask_with_schema(
                &prompt,
                CodexResponseSchema::Intent,
                input.codex_session_key.as_deref(),
            )
            .await
        {
            Ok(Some(output)) if !output.trim().is_empty() => output,
            Ok(_) => return None,
            Err(error) => {
                tracing::warn!(error = %error, "codex intent classification failed");
                return None;
            }
        };
        tracing::info!(
            response_bytes = output.len(),
            "codex intent response received; parsing intent json"
        );

        let intent = match parse_intent_response(&output) {
            Some(intent) => intent,
            None => {
                tracing::warn!("codex intent classification returned invalid json");
                return None;
            }
        };
        tracing::info!(
            intent = intent.intent.as_str(),
            summary = %intent.summary,
            "codex intent classified"
        );
        Some(intent)
    }

    pub async fn plan_with_intent(
        &self,
        input: PlannerInput,
        blueprints: &BlueprintStore,
        intent: Option<PlannerIntent>,
    ) -> PlanResult {
        if !self.codex_enabled() {
            return PlanResult {
                reply: "当前没有启用 Codex，已关闭本地关键词意图匹配，无法判断建筑或动作需求。请先启用 Codex。".to_string(),
                summary: "Codex 未启用".to_string(),
                actions: vec![GameAction::Chat {
                    message: "没有启用 Codex，无法理解这类自然语言需求。".to_string(),
                }],
            };
        }

        let Some(intent) = intent else {
            return PlanResult {
                reply: format!(
                    "Codex 没有返回有效意图分类，所以我没有下发任何动作。{}",
                    short_rephrase_hint()
                ),
                summary: "大模型意图识别失败".to_string(),
                actions: vec![GameAction::Chat {
                    message: format!(
                        "这次没有执行：Codex 未返回有效意图分类。{}",
                        short_rephrase_hint()
                    ),
                }],
            };
        };

        match intent.intent {
            PlannerIntentKind::Blueprint => {
                if let Some(result) = self.try_codex_blueprint(&input, blueprints).await {
                    return result;
                }

                PlanResult {
                    reply: format!(
                        "大模型没有生成有效蓝图，所以我没有下发建筑动作。{}",
                        rephrase_hints_for_build()
                    ),
                    summary: "大模型建筑规划失败".to_string(),
                    actions: vec![GameAction::Chat {
                        message: format!(
                            "建筑没有执行：大模型未返回有效蓝图。{}",
                            short_rephrase_hint()
                        ),
                    }],
                }
            }
            PlannerIntentKind::Action => {
                if let Some(result) = self.try_codex_action_plan(&input).await {
                    return result;
                }

                PlanResult {
                    reply: format!(
                        "Codex 没有返回可执行动作，所以我没有用本地关键词规则冒充理解。{}",
                        rephrase_hints_for_common_actions()
                    ),
                    summary: "大模型动作理解失败".to_string(),
                    actions: vec![GameAction::Chat {
                        message: format!(
                            "这次没有执行：Codex 未返回有效动作。{}",
                            short_rephrase_hint()
                        ),
                    }],
                }
            }
            PlannerIntentKind::ExistingBuildEdit => PlanResult {
                reply: "这个请求被 Codex 识别为改造现有建筑，需要 Minecraft 端带附近扫描并匹配已保存构建记录。请站到目标建筑前面重新执行同一句 `/bw ...`。".to_string(),
                summary: "需要建筑改造入口".to_string(),
                actions: vec![GameAction::Chat {
                    message: "请站到目标建筑前面重新发送，这类改造要先匹配附近已记录建筑。".to_string(),
                }],
            },
            PlannerIntentKind::Chat => PlanResult {
                reply: intent.reply.clone(),
                summary: intent.summary,
                actions: vec![GameAction::Chat {
                    message: intent.reply,
                }],
            },
        }
    }

    fn codex_enabled(&self) -> bool {
        self.codex
            .as_ref()
            .map(CodexClient::enabled)
            .unwrap_or(false)
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

    pub async fn plan_existing_build_edit(
        &self,
        input: &PlannerInput,
        existing: &BuildRecord,
    ) -> Option<PlanResult> {
        let codex = self.codex.as_ref()?;
        if !codex.enabled() {
            return None;
        }

        tracing::info!(
            build_id = %existing.id,
            has_nearby_scan = input.nearby_scan.is_some(),
            "starting codex existing-build edit planner"
        );
        let prompt = build_existing_edit_prompt(input, existing);
        let output = match codex
            .ask_with_schema(
                &prompt,
                CodexResponseSchema::ExistingEditPlan,
                input.codex_session_key.as_deref(),
            )
            .await
        {
            Ok(Some(output)) if !output.trim().is_empty() => output,
            Ok(_) => return None,
            Err(error) => {
                tracing::warn!(
                    build_id = %existing.id,
                    error = %error,
                    "codex existing-build edit planning failed"
                );
                return None;
            }
        };
        tracing::info!(
            build_id = %existing.id,
            response_bytes = output.len(),
            "codex existing-build edit response received; parsing json"
        );

        let plan = match parse_existing_edit_plan_response(&output) {
            Some(plan) => plan,
            None => {
                tracing::warn!(build_id = %existing.id, "codex existing-build edit returned invalid json");
                return None;
            }
        };
        if plan.actions.is_empty() {
            return None;
        }

        let mut actions = if plan.mode == ExistingEditMode::Replace {
            clear_existing_build_actions(existing)
        } else {
            Vec::new()
        };
        actions.extend(plan.actions);

        Some(PlanResult {
            reply: plan.reply,
            summary: plan.summary,
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
        let action_types = plan
            .actions
            .iter()
            .map(action_type_name)
            .collect::<Vec<_>>();
        tracing::info!(
            summary = %plan.summary,
            action_count = plan.actions.len(),
            action_types = ?action_types,
            "planned with codex action planner"
        );

        Some(PlanResult {
            reply: plan.reply,
            summary: plan.summary,
            actions: plan.actions,
        })
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

fn short_rephrase_hint() -> &'static str {
    "请用更具体的一句话重试。"
}

fn rephrase_hints_for_build() -> &'static str {
    "建议直接给尺寸和材质，例如“在我前方盖一个 7x7 橡木小屋，带门、窗户、床和火把”。也可以先说“先 dry-run 预览，不要执行”。"
}

fn rephrase_hints_for_common_actions() -> &'static str {
    "建议改成明确动作，例如“给我一把钻石剑”“把时间调到白天”“把天气改成晴天”。"
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

fn expected_record_bounds(record: &BuildRecord) -> Option<BlueprintBounds> {
    let mut bounds: Option<BlueprintBounds> = None;
    for action in &record.expected_actions {
        for block in &action.blocks {
            let x = action.origin.x + block.x;
            let y = action.origin.y + block.y;
            let z = action.origin.z + block.z;
            bounds = Some(match bounds {
                Some(existing) => BlueprintBounds {
                    min_x: existing.min_x.min(x),
                    max_x: existing.max_x.max(x),
                    min_y: existing.min_y.min(y),
                    max_y: existing.max_y.max(y),
                    min_z: existing.min_z.min(z),
                    max_z: existing.max_z.max(z),
                },
                None => BlueprintBounds {
                    min_x: x,
                    max_x: x,
                    min_y: y,
                    max_y: y,
                    min_z: z,
                    max_z: z,
                },
            });
        }
    }
    bounds
}

fn clear_existing_build_actions(record: &BuildRecord) -> Vec<GameAction> {
    record
        .expected_actions
        .iter()
        .enumerate()
        .map(|(index, action)| GameAction::PlaceBlocks {
            blueprint_id: Some(format!("{}:replacement-clear-{index}", record.id)),
            origin: action.origin.clone(),
            blocks: action
                .blocks
                .iter()
                .map(|block| crate::domain::types::BlueprintBlock {
                    x: block.x,
                    y: block.y,
                    z: block.z,
                    material: "minecraft:air".to_string(),
                })
                .collect(),
            clear_existing: true,
        })
        .collect()
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

fn build_intent_prompt(input: &PlannerInput) -> String {
    let attachments =
        serde_json::to_string(&input.attachments).unwrap_or_else(|_| "[]".to_string());
    let site_context = build_site_context(input);
    format!(
        r#"你是 Blockwright 的 Minecraft 意图分类器。请只判断玩家这句话接下来应该进入哪一个规划器，不要生成执行动作或蓝图。

硬性规则：
- 只输出一个 JSON 对象，不要输出 Markdown、解释或代码块。
- JSON 必须符合：{{"intent":"blueprint|action|existing_build_edit|chat","reply":"中文短回复","summary":"短中文摘要"}}
- 这里由 Codex 做自然语言意图识别，controller 不再使用本地关键词表兜底；你必须根据完整语义、上下文、附件和场地信息判断。
- intent=blueprint：玩家想新建、生成、设计、摆放一个实体结构、装饰物、模型、建筑、场景、图片复刻结构或游戏内实物。只要目标是“在世界里出现一个东西”，就选它。
- intent=existing_build_edit：玩家想改造、移动、升降、替换材料、修正入口/窗户/地基等，且语义指向附近或已经存在的 Blockwright 建筑。
- intent=action：玩家想发物品、调整时间/天气/游戏模式/效果，或执行可由安全 Minecraft 指令完成的非建筑动作。
- intent=chat：普通聊天、说明、无法判断、拒绝执行或缺少关键信息。
- 如果在 blueprint 和 action 之间犹豫，只要结果会放置方块或生成实物，就选 blueprint。
- 如果在 blueprint 和 existing_build_edit 之间犹豫：新做一个选 blueprint；改已有目标选 existing_build_edit。
- reply 是给玩家看的简短中文说明；summary 是日志和任务列表用的中文短摘要。

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
- 蓝图本身不要设计成覆盖玩家脚下、头部或贴身一圈活动空间；建筑再大也必须整体在玩家前方，不能把玩家包进结构里。
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

fn build_existing_edit_prompt(input: &PlannerInput, existing: &BuildRecord) -> String {
    let site_context = build_site_context(input);
    let existing_context = build_existing_record_context(existing);
    format!(
        r#"你是 Blockwright 的 Minecraft 现有建筑自由改造规划器。玩家已经站在目标建筑前，controller 已经按附近扫描和空间位置匹配到了当前要改的整栋建筑。请直接输出可执行改造计划 JSON。

硬性规则：
- 只输出一个 JSON 对象，不要输出 Markdown、解释或代码块。
- JSON 必须符合字段：reply、summary、mode、actions。
- mode 只能是 patch 或 replace。
- actions 只能输出 place_blocks；每个 action 的 origin 是世界绝对放置原点，blocks 是相对 origin 的方块列表。
- 方块材质必须使用 Minecraft 命名空间 ID，例如 minecraft:oak_planks；需要清除单个方块时可以使用 minecraft:air。
- 不要再要求玩家说明“哪个部位”，也不要因为目标不够模板化就拒绝。你要根据玩家这句话自己判断是局部补丁还是整栋重做。
- 小范围材料替换、局部细节、补灯、加装饰、修正窗口/入口，优先 mode=patch，只输出需要改的方块，clear_existing=false。
- 整体放大、重做、变逼真、换主题、移动、升降、旋转、整体结构变化，使用 mode=replace。replace 时 actions 只输出新的最终建筑；controller 会先清掉旧建筑。
- 如果是“移动/升降/换位置”这类整体调整，mode=replace，并把新 actions 的 origin 改到目标位置，不要把 origin 仍然放在旧位置。
- 如果是“放大/重做/逼真/升级/更真实/更大气”，mode=replace，输出完整的新最终结构，不要只输出几个装饰方块。
- 第一阶段单次 actions 总方块量控制在 700 个以内，优先用常见原版方块。
- 对摩天轮、旋转木马、桥、塔、雕塑等模型，要有可辨认的整体轮廓、支撑结构、中心轴、座舱/装饰和地基。
- reply 用中文说明会怎么改，summary 用中文短摘要。

玩家文字：
{text}

已匹配建筑：
{existing_context}

场地摘要：
{site_context}
"#,
        text = input.text.trim(),
        existing_context = existing_context,
        site_context = site_context
    )
}

fn build_existing_record_context(existing: &BuildRecord) -> String {
    let action_count = existing.expected_actions.len();
    let total_blocks = existing
        .expected_actions
        .iter()
        .map(|action| action.expected_count)
        .sum::<u32>();
    let bounds = expected_record_bounds(existing)
        .map(|bounds| {
            format!(
                "bounds=({}, {}, {})..({}, {}, {})",
                bounds.min_x, bounds.min_y, bounds.min_z, bounds.max_x, bounds.max_y, bounds.max_z
            )
        })
        .unwrap_or_else(|| "bounds=未知".to_string());
    let actions_json = build_existing_actions_context(existing, 700);
    format!(
        "id={}，summary={}，action_count={}，recorded_blocks={}，{}，actions={}",
        existing.id, existing.summary, action_count, total_blocks, bounds, actions_json
    )
}

fn build_existing_actions_context(existing: &BuildRecord, max_blocks: usize) -> String {
    let mut remaining = max_blocks;
    let mut truncated = false;
    let actions = existing
        .expected_actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let take = remaining.min(action.blocks.len());
            remaining -= take;
            if take < action.blocks.len() {
                truncated = true;
            }
            let blocks = action.blocks.iter().take(take).collect::<Vec<_>>();
            serde_json::json!({
                "index": index,
                "blueprint_id": action.blueprint_id.clone(),
                "origin": action.origin.clone(),
                "expected_count": action.expected_count,
                "included_blocks": blocks,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "truncated": truncated,
        "max_included_blocks": max_blocks,
        "actions": actions,
    })
    .to_string()
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
- 这个动作理解器只处理物品、安全 Minecraft 指令和普通聊天；建筑和改造需求已经由 Codex 意图分类器拦截。
- 如果这里仍收到建筑、改造或放置方块类需求，返回普通 chat 提示，不要输出 place_blocks 或危险命令。
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

fn parse_intent_response(output: &str) -> Option<PlannerIntent> {
    let json = extract_json_object(output.trim())?;
    serde_json::from_str(json).ok()
}

fn parse_action_plan_response(output: &str) -> Option<CodexActionPlan> {
    let json = extract_json_object(output.trim())?;
    serde_json::from_str(json).ok()
}

fn parse_existing_edit_plan_response(output: &str) -> Option<CodexExistingEditPlan> {
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
            Blueprint, BlueprintBlock, BlueprintSize, BuildRecord, BuildStatus, ChatAttachmentKind,
            ChatAttachmentSource, ExpectedBuildAction, MaterialCount, WorldScanBlock,
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

    fn planner_with_fake_codex_responses(
        name: &str,
        intent_message: &str,
        blueprint_message: &str,
        action_message: &str,
    ) -> Planner {
        let dir = temp_dir(name);
        fs::create_dir_all(&dir).unwrap();
        let intent_path = dir.join("intent.json");
        let blueprint_path = dir.join("blueprint.json");
        let action_path = dir.join("action.json");
        let existing_edit_path = dir.join("existing-edit.json");
        fs::write(&intent_path, intent_message).unwrap();
        fs::write(&blueprint_path, blueprint_message).unwrap();
        fs::write(&action_path, action_message).unwrap();
        fs::write(
            &existing_edit_path,
            r#"{
  "reply": "已按当前建筑自由改造。",
  "summary": "自由改造现有建筑",
  "mode": "replace",
  "actions": [
    {
      "type": "place_blocks",
      "blueprint_id": "codex-existing-edit",
      "origin": {"world": "minecraft:overworld", "x": 10, "y": 65, "z": 20},
      "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
      "clear_existing": true
    }
  ]
}"#,
        )
        .unwrap();
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
  *intent.schema.json)
    cat "{intent_path}" > "$last_message"
    ;;
  *action-plan.schema.json)
    cat "{action_path}" > "$last_message"
    ;;
  *blueprint.schema.json)
    cat "{blueprint_path}" > "$last_message"
    ;;
  *existing-edit-plan.schema.json)
    cat "{existing_edit_path}" > "$last_message"
    ;;
  *)
    exit 3
    ;;
esac
"#,
                intent_path = intent_path.to_string_lossy(),
                action_path = action_path.to_string_lossy(),
                blueprint_path = blueprint_path.to_string_lossy(),
                existing_edit_path = existing_edit_path.to_string_lossy()
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
        planner_with_fake_codex_responses(
            name,
            r#"{"intent":"blueprint","reply":"按建筑处理。","summary":"建筑需求"}"#,
            blueprint_message,
            r#"{"reply":"无法处理建筑动作。","summary":"普通提示","actions":[{"type":"chat","player":null,"item":null,"count":null,"command":null,"message":"无法处理建筑动作。"}]}"#,
        )
    }

    fn planner_with_fake_action(name: &str, action_message: &str) -> Planner {
        planner_with_fake_codex_responses(
            name,
            r#"{"intent":"action","reply":"按动作处理。","summary":"动作需求"}"#,
            r#"not blueprint json"#,
            action_message,
        )
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

    fn test_build_record() -> BuildRecord {
        BuildRecord {
            id: "hm-job-1".to_string(),
            server_id: "hmcl-lan".to_string(),
            target_player: Some("Steve".to_string()),
            summary: "建造蓝图 old-wheel".to_string(),
            status: BuildStatus::Succeeded,
            expected_actions: vec![ExpectedBuildAction {
                blueprint_id: Some("old-wheel".to_string()),
                origin: BlockOrigin {
                    world: Some("minecraft:overworld".to_string()),
                    x: 10,
                    y: 64,
                    z: 20,
                },
                expected_count: 2,
                materials: vec![MaterialCount {
                    material: "minecraft:oak_planks".to_string(),
                    count: 2,
                }],
                blocks: vec![
                    BlueprintBlock {
                        x: 0,
                        y: 0,
                        z: 0,
                        material: "minecraft:oak_planks".to_string(),
                    },
                    BlueprintBlock {
                        x: 1,
                        y: 0,
                        z: 0,
                        material: "minecraft:oak_planks".to_string(),
                    },
                ],
            }],
            result: None,
            message: None,
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
        let planner = planner_with_fake_codex_responses(
            "codex-invalid-action",
            r#"{"intent":"action","reply":"按动作处理。","summary":"动作需求"}"#,
            "not blueprint json",
            "not action json",
        );

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
        assert!(result.reply.contains("给我一把钻石剑"));
        assert!(result.reply.contains("把时间调到白天"));
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
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "大模型建筑规划失败");
        assert!(result.reply.contains("7x7"));
        assert!(result.reply.contains("dry-run 预览"));
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
    async fn existing_build_edit_uses_codex_plan_without_local_keyword_rules() {
        let planner = planner_with_fake_codex(
            "codex-existing-edit",
            r#"{
  "id": "unused-blueprint",
  "name": "未使用蓝图",
  "description": "这个测试直接走 existing edit schema。",
  "size": {"width": 1, "height": 1, "depth": 1},
  "materials": [{"material": "minecraft:oak_planks", "count": 1}],
  "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"}],
  "tags": ["test"]
}"#,
        );
        let result = planner
            .plan_existing_build_edit(
                &PlannerInput {
                    text: "把它整体升高一点，再做得更精致".to_string(),
                    player: Some("Steve".to_string()),
                    codex_session_key: None,
                    position: None,
                    nearby_scan: Some(scan_with_blocks(Vec::new())),
                    attachments: Vec::new(),
                },
                &test_build_record(),
            )
            .await
            .unwrap();

        assert_eq!(result.summary, "自由改造现有建筑");
        assert_eq!(result.actions.len(), 2);
        assert!(matches!(
            &result.actions[0],
            GameAction::PlaceBlocks {
                blueprint_id: Some(blueprint_id),
                blocks,
                clear_existing: true,
                ..
            } if blueprint_id == "hm-job-1:replacement-clear-0"
                && blocks.len() == 2
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
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "Codex 未启用");
        assert!(result.reply.contains("已关闭本地关键词意图匹配"));
        assert!(matches!(result.actions[0], GameAction::Chat { .. }));
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
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "建造蓝图 image-inspired-house");
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
    fn intent_prompt_routes_builds_actions_and_existing_edits() {
        let prompt = build_intent_prompt(&PlannerInput {
            text: "给我旋转木马，可以大点，大气点".to_string(),
            player: Some("Charles".to_string()),
            codex_session_key: None,
            position: None,
            nearby_scan: None,
            attachments: Vec::new(),
        });

        assert!(prompt.contains("意图分类器"));
        assert!(prompt.contains("controller 不再使用本地关键词表兜底"));
        assert!(prompt.contains("intent=blueprint"));
        assert!(prompt.contains("intent=action"));
        assert!(prompt.contains("intent=existing_build_edit"));
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
        assert!(prompt.contains("Codex 意图分类器"));
    }

    #[test]
    fn parses_codex_intent_json() {
        let output = r#"{"intent":"blueprint","reply":"按建筑处理。","summary":"建筑需求"}"#;

        let intent = parse_intent_response(output).unwrap();

        assert_eq!(intent.intent, PlannerIntentKind::Blueprint);
        assert_eq!(intent.summary, "建筑需求");
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
