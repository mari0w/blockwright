use std::{collections::HashMap, path::PathBuf, sync::Arc};

use tokio::sync::RwLock;

use crate::domain::types::{
    ActionExecutionReport, BlockOrigin, BlueprintBlock, BuildRecord, BuildStatus,
    ExpectedBuildAction, GameAction, JobResultRequest, MaterialCount, WorldScan,
};

#[derive(Debug, Clone)]
pub struct BuildMatch {
    pub record: BuildRecord,
    pub action_index: usize,
    pub matched_blocks: u32,
    pub scanned_expected_blocks: u32,
    pub score: f32,
}

#[derive(Clone)]
pub struct BuildStore {
    data_dir: Arc<PathBuf>,
    items: Arc<RwLock<HashMap<String, BuildRecord>>>,
}

impl BuildStore {
    pub async fn new(data_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        tokio::fs::create_dir_all(&data_dir).await?;
        let store = Self {
            data_dir: Arc::new(data_dir),
            items: Arc::new(RwLock::new(HashMap::new())),
        };
        store.load_from_disk().await?;
        Ok(store)
    }

    pub async fn get(&self, id: &str) -> Option<BuildRecord> {
        self.items.read().await.get(id).cloned()
    }

    pub async fn list(&self) -> Vec<BuildRecord> {
        let mut records = self
            .items
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.id.cmp(&right.id));
        records
    }

    pub async fn register_planned(
        &self,
        id: String,
        server_id: String,
        target_player: Option<String>,
        summary: String,
        actions: &[GameAction],
    ) -> Result<Option<BuildRecord>, Box<dyn std::error::Error + Send + Sync>> {
        let expected_actions = expected_actions(actions);
        if expected_actions.is_empty() {
            return Ok(None);
        }

        let record = BuildRecord {
            id,
            server_id,
            target_player,
            summary,
            status: BuildStatus::Planned,
            expected_actions,
            result: None,
            message: None,
        };
        self.save_record(record.clone()).await?;
        Ok(Some(record))
    }

    pub async fn apply_result(
        &self,
        id: &str,
        request: &JobResultRequest,
    ) -> Result<Option<BuildRecord>, Box<dyn std::error::Error + Send + Sync>> {
        let Some(mut record) = self.get(id).await else {
            return Ok(None);
        };

        record.status = if request.ok {
            BuildStatus::Succeeded
        } else {
            BuildStatus::Failed
        };
        record.result = request.report.clone();
        record.message = request.message.clone();

        // 二次防线：构建记录必须有执行端校验报告，且报告要能对上计划里的每个建筑动作。
        if report_is_inconsistent(&record) {
            record.status = BuildStatus::Failed;
        }

        self.save_record(record.clone()).await?;
        Ok(Some(record))
    }

    pub async fn match_scan(&self, server_id: &str, scan: &WorldScan) -> Vec<BuildMatch> {
        let scan_blocks = scan
            .blocks
            .iter()
            .map(|block| ((block.x, block.y, block.z), block.material.as_str()))
            .collect::<HashMap<_, _>>();

        let mut matches = Vec::new();
        for record in self.items.read().await.values() {
            if record.server_id != server_id {
                continue;
            }

            for (action_index, action) in record.expected_actions.iter().enumerate() {
                if let Some(world) = &action.origin.world {
                    if world != &scan.world {
                        continue;
                    }
                }

                let mut scanned_expected_blocks = 0;
                let mut matched_blocks = 0;
                for block in &action.blocks {
                    let absolute = (
                        action.origin.x + block.x,
                        action.origin.y + block.y,
                        action.origin.z + block.z,
                    );
                    let Some(actual) = scan_blocks.get(&absolute) else {
                        continue;
                    };
                    scanned_expected_blocks += 1;
                    if materials_match(block.material.as_str(), actual) {
                        matched_blocks += 1;
                    }
                }

                if matched_blocks == 0 {
                    continue;
                }
                let score = matched_blocks as f32 / action.expected_count.max(1) as f32;
                matches.push(BuildMatch {
                    record: record.clone(),
                    action_index,
                    matched_blocks,
                    scanned_expected_blocks,
                    score,
                });
            }
        }

        matches.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| right.matched_blocks.cmp(&left.matched_blocks))
                .then_with(|| left.record.id.cmp(&right.record.id))
        });
        matches
    }

    pub async fn adopt_scan_as_build(
        &self,
        id: String,
        server_id: String,
        target_player: Option<String>,
        scan: &WorldScan,
    ) -> Result<Option<BuildMatch>, Box<dyn std::error::Error + Send + Sync>> {
        let Some(action) = expected_action_from_scan(scan) else {
            return Ok(None);
        };
        let matched_blocks = action.blocks.len() as u32;
        let record = BuildRecord {
            id,
            server_id,
            target_player,
            summary: "自动登记附近建筑".to_string(),
            status: BuildStatus::Succeeded,
            expected_actions: vec![action],
            result: None,
            message: Some("由附近扫描自动登记，供后续透明改造使用。".to_string()),
        };

        self.save_record(record.clone()).await?;
        Ok(Some(BuildMatch {
            record,
            action_index: 0,
            matched_blocks,
            scanned_expected_blocks: matched_blocks,
            score: 1.0,
        }))
    }

    async fn load_from_disk(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut dir = tokio::fs::read_dir(self.data_dir.as_ref()).await?;
        let mut loaded = HashMap::new();

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }

            let content = tokio::fs::read_to_string(&path).await?;
            let record = serde_json::from_str::<BuildRecord>(&content)?;
            loaded.insert(record.id.clone(), record);
        }

        *self.items.write().await = loaded;
        Ok(())
    }

    async fn save_record(
        &self,
        record: BuildRecord,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let file_path = self.data_dir.join(format!("{}.json", safe_id(&record.id)));
        let json = serde_json::to_string_pretty(&record)?;
        tokio::fs::write(file_path, json).await?;
        self.items.write().await.insert(record.id.clone(), record);
        Ok(())
    }
}

fn expected_actions(actions: &[GameAction]) -> Vec<ExpectedBuildAction> {
    actions
        .iter()
        .filter_map(|action| match action {
            GameAction::PlaceBlocks {
                blueprint_id,
                origin,
                blocks,
                ..
            } => Some(ExpectedBuildAction {
                blueprint_id: blueprint_id.clone(),
                origin: origin.clone(),
                expected_count: blocks.len() as u32,
                materials: material_counts(blocks),
                blocks: blocks.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn material_counts(blocks: &[crate::domain::types::BlueprintBlock]) -> Vec<MaterialCount> {
    let mut counts = HashMap::<String, u32>::new();
    for block in blocks {
        *counts.entry(block.material.clone()).or_default() += 1;
    }
    let mut items = counts
        .into_iter()
        .map(|(material, count)| MaterialCount { material, count })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.material.cmp(&right.material));
    items
}

fn expected_action_from_scan(scan: &WorldScan) -> Option<ExpectedBuildAction> {
    let adopted = scan
        .blocks
        .iter()
        .filter(|block| is_adoptable_scan_material(&block.material))
        .collect::<Vec<_>>();
    let first = adopted.first()?;
    let (min_x, min_y, min_z) = adopted.iter().skip(1).fold(
        (first.x, first.y, first.z),
        |(min_x, min_y, min_z), block| (min_x.min(block.x), min_y.min(block.y), min_z.min(block.z)),
    );
    let blocks = adopted
        .into_iter()
        .map(|block| BlueprintBlock {
            x: block.x - min_x,
            y: block.y - min_y,
            z: block.z - min_z,
            material: block.material.clone(),
        })
        .collect::<Vec<_>>();

    Some(ExpectedBuildAction {
        blueprint_id: Some("auto-adopted-nearby-build".to_string()),
        origin: BlockOrigin {
            world: Some(scan.world.clone()),
            x: min_x,
            y: min_y,
            z: min_z,
        },
        expected_count: blocks.len() as u32,
        materials: material_counts(&blocks),
        blocks,
    })
}

fn is_adoptable_scan_material(material: &str) -> bool {
    let material = material_spec(material).0;
    !matches!(
        material,
        "minecraft:water"
            | "minecraft:lava"
            | "minecraft:grass_block"
            | "minecraft:dirt"
            | "minecraft:coarse_dirt"
            | "minecraft:rooted_dirt"
            | "minecraft:podzol"
            | "minecraft:mycelium"
            | "minecraft:stone"
            | "minecraft:deepslate"
            | "minecraft:granite"
            | "minecraft:diorite"
            | "minecraft:andesite"
            | "minecraft:calcite"
            | "minecraft:tuff"
            | "minecraft:gravel"
            | "minecraft:sand"
            | "minecraft:red_sand"
            | "minecraft:sandstone"
            | "minecraft:red_sandstone"
            | "minecraft:clay"
            | "minecraft:ice"
            | "minecraft:packed_ice"
            | "minecraft:blue_ice"
            | "minecraft:snow"
            | "minecraft:snow_block"
            | "minecraft:short_grass"
            | "minecraft:tall_grass"
            | "minecraft:fern"
            | "minecraft:large_fern"
            | "minecraft:seagrass"
            | "minecraft:tall_seagrass"
            | "minecraft:kelp"
            | "minecraft:kelp_plant"
    )
}

fn materials_match(expected: &str, actual: &str) -> bool {
    let expected = material_spec(expected);
    let actual = material_spec(actual);
    if expected.0 != actual.0 {
        return false;
    }
    if expected.1.is_empty() || actual.1.is_empty() {
        return true;
    }
    expected
        .1
        .iter()
        .all(|(key, value)| actual.1.get(key).is_some_and(|actual| actual == value))
}

fn material_spec(material: &str) -> (&str, HashMap<&str, &str>) {
    let Some((id, states)) = material
        .strip_suffix(']')
        .and_then(|value| value.split_once('['))
    else {
        return (material, HashMap::new());
    };

    let states = states
        .split(',')
        .filter_map(|part| part.split_once('='))
        .collect::<HashMap<_, _>>();
    (id, states)
}

fn report_is_inconsistent(record: &BuildRecord) -> bool {
    let Some(report) = record.result.as_ref() else {
        return true;
    };

    let place_reports = report
        .actions
        .iter()
        .filter(|action| action.action_type == "place_blocks")
        .collect::<Vec<_>>();
    if place_reports.len() != record.expected_actions.len() {
        return true;
    }

    record
        .expected_actions
        .iter()
        .zip(place_reports)
        .any(|(expected, action)| action_failed(expected, action))
}

fn action_failed(expected: &ExpectedBuildAction, action: &ActionExecutionReport) -> bool {
    action.blueprint_id != expected.blueprint_id
        || action.expected_count != expected.expected_count
        || action.mismatch_count > 0
        || action.verified_count != expected.expected_count
}

fn safe_id(id: &str) -> String {
    id.chars()
        .filter(|value| value.is_ascii_alphanumeric() || *value == '-' || *value == '_')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        BlockMismatch, BlockOrigin, BlueprintBlock, JobExecutionReport, WorldScanBlock,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR_ID: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(name: &str) -> PathBuf {
        let number = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "blockwright-build-store-{name}-{}-{number}",
            std::process::id()
        ))
    }

    fn place_action() -> GameAction {
        GameAction::PlaceBlocks {
            blueprint_id: Some("test-house".to_string()),
            origin: BlockOrigin {
                world: Some("minecraft:overworld".to_string()),
                x: 10,
                y: 64,
                z: 10,
            },
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
            clear_existing: false,
        }
    }

    #[test]
    fn material_match_accepts_stateful_expected_with_plain_scan_id() {
        assert!(materials_match(
            "minecraft:oak_leaves[persistent=true]",
            "minecraft:oak_leaves"
        ));
        assert!(materials_match(
            "minecraft:oak_door[half=upper,facing=south]",
            "minecraft:oak_door[half=upper,facing=south]"
        ));
        assert!(materials_match(
            "minecraft:oak_door[half=upper,facing=south]",
            "minecraft:oak_door[facing=south,half=upper,hinge=left,open=false,powered=false]"
        ));
        assert!(!materials_match(
            "minecraft:oak_door[half=upper,facing=south]",
            "minecraft:oak_door[half=lower,facing=south]"
        ));
        assert!(!materials_match(
            "minecraft:oak_leaves[persistent=true]",
            "minecraft:birch_leaves"
        ));
    }

    #[tokio::test]
    async fn registers_planned_build_from_place_blocks_action() {
        let store = BuildStore::new(temp_dir("planned")).await.unwrap();

        let record = store
            .register_planned(
                "hm-job-1".to_string(),
                "hmcl-lan".to_string(),
                Some("Steve".to_string()),
                "建造测试".to_string(),
                &[place_action()],
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(record.status, BuildStatus::Planned);
        assert_eq!(record.expected_actions[0].expected_count, 2);
        assert_eq!(record.expected_actions[0].materials[0].count, 2);
        assert!(store.get("hm-job-1").await.is_some());
    }

    #[tokio::test]
    async fn match_scan_uses_planned_records_without_manual_save_step() {
        let store = BuildStore::new(temp_dir("planned-match")).await.unwrap();
        store
            .register_planned(
                "hm-job-1".to_string(),
                "hmcl-lan".to_string(),
                Some("Steve".to_string()),
                "建造测试".to_string(),
                &[place_action()],
            )
            .await
            .unwrap();
        let scan = WorldScan {
            world: "minecraft:overworld".to_string(),
            center_x: 10,
            center_y: 64,
            center_z: 10,
            radius: 8,
            blocks: vec![
                WorldScanBlock {
                    x: 10,
                    y: 64,
                    z: 10,
                    material: "minecraft:oak_planks".to_string(),
                },
                WorldScanBlock {
                    x: 11,
                    y: 64,
                    z: 10,
                    material: "minecraft:oak_planks".to_string(),
                },
            ],
        };

        let matches = store.match_scan("hmcl-lan", &scan).await;

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].record.id, "hm-job-1");
        assert_eq!(matches[0].record.status, BuildStatus::Planned);
    }

    #[tokio::test]
    async fn auto_adopts_scan_as_build_without_user_confirmation() {
        let store = BuildStore::new(temp_dir("auto-adopt")).await.unwrap();
        let scan = WorldScan {
            world: "minecraft:overworld".to_string(),
            center_x: 20,
            center_y: 64,
            center_z: 30,
            radius: 8,
            blocks: vec![
                WorldScanBlock {
                    x: 20,
                    y: 63,
                    z: 30,
                    material: "minecraft:water[level=0]".to_string(),
                },
                WorldScanBlock {
                    x: 20,
                    y: 64,
                    z: 30,
                    material: "minecraft:gold_block".to_string(),
                },
                WorldScanBlock {
                    x: 21,
                    y: 64,
                    z: 30,
                    material: "minecraft:copper_block".to_string(),
                },
            ],
        };

        let adopted = store
            .adopt_scan_as_build(
                "hm-job-2".to_string(),
                "hmcl-lan".to_string(),
                Some("Steve".to_string()),
                &scan,
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(adopted.record.id, "hm-job-2");
        assert_eq!(adopted.record.status, BuildStatus::Succeeded);
        assert_eq!(adopted.record.expected_actions[0].blocks.len(), 2);
        assert_eq!(adopted.record.expected_actions[0].origin.y, 64);
        assert!(adopted.record.expected_actions[0]
            .blocks
            .iter()
            .all(|block| block.material != "minecraft:water[level=0]"));
    }

    #[tokio::test]
    async fn applies_failed_result_when_report_has_mismatch() {
        let store = BuildStore::new(temp_dir("result")).await.unwrap();
        store
            .register_planned(
                "hm-job-1".to_string(),
                "hmcl-lan".to_string(),
                Some("Steve".to_string()),
                "建造测试".to_string(),
                &[place_action()],
            )
            .await
            .unwrap();

        let updated = store
            .apply_result(
                "hm-job-1",
                &JobResultRequest {
                    ok: true,
                    message: Some("verified".to_string()),
                    report: Some(JobExecutionReport {
                        actions: vec![ActionExecutionReport {
                            action_type: "place_blocks".to_string(),
                            blueprint_id: Some("test-house".to_string()),
                            expected_count: 2,
                            placed_count: 1,
                            skipped_existing_count: 0,
                            skipped_limit_count: 0,
                            skipped_player_safety_count: 0,
                            verified_count: 1,
                            mismatch_count: 1,
                            mismatches: vec![BlockMismatch {
                                x: 1,
                                y: 64,
                                z: 10,
                                expected: "minecraft:oak_planks".to_string(),
                                actual: "minecraft:air".to_string(),
                            }],
                        }],
                    }),
                },
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(updated.status, BuildStatus::Failed);
    }

    #[tokio::test]
    async fn applies_failed_result_when_report_is_missing() {
        let store = BuildStore::new(temp_dir("missing-report")).await.unwrap();
        store
            .register_planned(
                "hm-job-1".to_string(),
                "hmcl-lan".to_string(),
                Some("Steve".to_string()),
                "建造测试".to_string(),
                &[place_action()],
            )
            .await
            .unwrap();

        let updated = store
            .apply_result(
                "hm-job-1",
                &JobResultRequest {
                    ok: true,
                    message: Some("ok".to_string()),
                    report: None,
                },
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(updated.status, BuildStatus::Failed);
    }
}
