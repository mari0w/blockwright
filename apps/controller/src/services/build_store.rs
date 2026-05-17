use std::{collections::HashMap, path::PathBuf, sync::Arc};

use tokio::sync::RwLock;

use crate::domain::types::{
    ActionExecutionReport, BuildRecord, BuildStatus, ExpectedBuildAction, GameAction,
    JobResultRequest, MaterialCount, WorldScan,
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
            if record.server_id != server_id || record.status != BuildStatus::Succeeded {
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
                    if *actual == block.material {
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
    use crate::domain::types::{BlockMismatch, BlockOrigin, BlueprintBlock, JobExecutionReport};
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
        }
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
