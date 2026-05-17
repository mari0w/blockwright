use std::sync::Arc;

use crate::{
    config::{self, AppConfig, ChatRuntimeConfig},
    integrations::codex::CodexClient,
    services::{blueprint_store::BlueprintStore, job_queue::JobQueue, planner::Planner},
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub blueprints: BlueprintStore,
    pub jobs: JobQueue,
    pub planner: Planner,
    pub codex: CodexClient,
    pub chat: ChatRuntimeConfig,
}

impl AppState {
    pub async fn new(config: AppConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config = Arc::new(config);
        let blueprints = BlueprintStore::new(config.storage.data_dir.join("blueprints")).await?;
        seed_default_blueprint(&blueprints).await?;
        let chat = config::load_chat_runtime_config(&config.chat.config_path)?;

        Ok(Self {
            codex: CodexClient::new(config.codex.clone()),
            config,
            blueprints,
            jobs: JobQueue::default(),
            planner: Planner::default(),
            chat,
        })
    }
}

async fn seed_default_blueprint(
    blueprints: &BlueprintStore,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !blueprints.list().await.is_empty() {
        return Ok(());
    }

    let source = include_str!("../../../blueprints/examples/oak_house.json");
    let blueprint = serde_json::from_str(source)?;
    blueprints.save(blueprint).await?;
    Ok(())
}
