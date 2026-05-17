use crate::{
    domain::types::{BlockOrigin, ChatAttachment, ChatAttachmentKind, GameAction, PlayerPosition},
    services::blueprint_store::BlueprintStore,
};

#[derive(Debug, Clone)]
pub struct PlannerInput {
    pub text: String,
    pub player: Option<String>,
    pub position: Option<PlayerPosition>,
    pub attachments: Vec<ChatAttachment>,
}

#[derive(Debug, Clone)]
pub struct PlanResult {
    pub reply: String,
    pub summary: String,
    pub actions: Vec<GameAction>,
}

#[derive(Clone, Default)]
pub struct Planner;

impl Planner {
    pub async fn plan(&self, input: PlannerInput, blueprints: &BlueprintStore) -> PlanResult {
        let text = input.text.trim();
        let lower_text = text.to_lowercase();

        if wants_diamond_sword(text, &lower_text) {
            return PlanResult {
                reply: "可以，已经准备给你一把钻石剑。".to_string(),
                summary: "发放钻石剑".to_string(),
                actions: vec![GameAction::GiveItem {
                    player: input.player,
                    item: "minecraft:diamond_sword".to_string(),
                    count: 1,
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

        if wants_image_pipeline(text, &lower_text, &input.attachments) {
            return PlanResult {
                reply: "图片复刻会走图片分析流水线：识别结构、换算材料、生成蓝图、再由插件放置。当前骨架已经预留入口，下一步接入视觉分析。".to_string(),
                summary: "说明图片复刻流程".to_string(),
                actions: vec![GameAction::Chat {
                    message: "图片复刻能力还在接入中。".to_string(),
                }],
            };
        }

        if wants_house(text, &lower_text) {
            if let Some(blueprint) = blueprints.first_by_tag("house").await {
                let origin = input
                    .position
                    .as_ref()
                    .map(origin_in_front_of_player)
                    .unwrap_or(BlockOrigin {
                        world: None,
                        x: 0,
                        y: 64,
                        z: 0,
                    });

                return PlanResult {
                    reply: format!(
                        "可以，我会按蓝图 `{}` 在你面前生成一个木屋。",
                        blueprint.name
                    ),
                    summary: format!("建造蓝图 {}", blueprint.id),
                    actions: vec![GameAction::PlaceBlocks {
                        blueprint_id: Some(blueprint.id),
                        origin,
                        blocks: blueprint.blocks,
                    }],
                };
            }

            return PlanResult {
                reply: "现在还没有可用的房屋蓝图，需要先导入或保存一个蓝图。".to_string(),
                summary: "缺少房屋蓝图".to_string(),
                actions: vec![GameAction::Chat {
                    message: "没有找到 house 标签的蓝图。".to_string(),
                }],
            };
        }

        PlanResult {
            reply: "我已经收到需求。当前第一版先支持钻石、钻石剑和木屋蓝图，后续会接 Codex 做更完整的理解。".to_string(),
            summary: "普通对话".to_string(),
            actions: vec![GameAction::Chat {
                message: "当前支持：给我钻石剑、给我钻石、帮我盖一个木屋。".to_string(),
            }],
        }
    }
}

fn wants_diamond_sword(original: &str, lower_text: &str) -> bool {
    original.contains("钻石剑") || lower_text.contains("diamond sword")
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

fn wants_house(original: &str, lower_text: &str) -> bool {
    original.contains("房子")
        || original.contains("木屋")
        || original.contains("建筑")
        || original.contains("盖")
        || lower_text.contains("house")
        || lower_text.contains("build")
}

fn origin_in_front_of_player(position: &PlayerPosition) -> BlockOrigin {
    BlockOrigin {
        world: Some(position.world.clone()),
        x: position.x.round() as i32 + 2,
        y: position.y.round() as i32,
        z: position.z.round() as i32 + 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        Blueprint, BlueprintBlock, BlueprintSize, ChatAttachmentSource, MaterialCount,
    };
    use std::{
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

    #[tokio::test]
    async fn plans_diamond_sword() {
        let store = empty_store("sword").await;
        let result = Planner
            .plan(
                PlannerInput {
                    text: "给我一把钻石剑".to_string(),
                    player: Some("Steve".to_string()),
                    position: None,
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
        let result = Planner
            .plan(
                PlannerInput {
                    text: "give me diamonds".to_string(),
                    player: Some("Alex".to_string()),
                    position: None,
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
    async fn plans_house_from_blueprint_tag() {
        let store = empty_store("house").await;
        store
            .save(test_blueprint("test-house", vec!["house"]))
            .await
            .unwrap();

        let result = Planner
            .plan(
                PlannerInput {
                    text: "帮我盖一个木屋".to_string(),
                    player: Some("Steve".to_string()),
                    position: None,
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

        let result = Planner
            .plan(
                PlannerInput {
                    text: "build a house".to_string(),
                    player: Some("Steve".to_string()),
                    position: Some(PlayerPosition {
                        world: "world_nether".to_string(),
                        x: 10.4,
                        y: 65.2,
                        z: -3.6,
                    }),
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
        let result = Planner
            .plan(
                PlannerInput {
                    text: "帮我盖一个木屋".to_string(),
                    player: Some("Steve".to_string()),
                    position: None,
                    attachments: Vec::new(),
                },
                &store,
            )
            .await;

        assert_eq!(result.summary, "缺少房屋蓝图");
        assert!(matches!(result.actions[0], GameAction::Chat { .. }));
    }

    #[tokio::test]
    async fn explains_image_pipeline_and_default_capabilities() {
        let store = empty_store("fallback").await;
        let image_result = Planner
            .plan(
                PlannerInput {
                    text: "帮我根据图片复刻建筑".to_string(),
                    player: None,
                    position: None,
                    attachments: Vec::new(),
                },
                &store,
            )
            .await;
        let fallback_result = Planner
            .plan(
                PlannerInput {
                    text: "你好".to_string(),
                    player: None,
                    position: None,
                    attachments: Vec::new(),
                },
                &store,
            )
            .await;

        assert_eq!(image_result.summary, "说明图片复刻流程");
        assert_eq!(fallback_result.summary, "普通对话");
        assert!(matches!(
            fallback_result.actions[0],
            GameAction::Chat { .. }
        ));
    }

    #[tokio::test]
    async fn image_attachment_enters_image_pipeline_without_magic_text() {
        let store = empty_store("image-attachment").await;
        let result = Planner
            .plan(
                PlannerInput {
                    text: "照这个做".to_string(),
                    player: None,
                    position: None,
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
}
