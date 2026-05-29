use std::sync::Arc;

use crate::{
    config::{self, AppConfig, ChatRuntimeConfig},
    integrations::{codex::CodexClient, codex_home::prepare_project_codex_home, llm::LlmClient},
    services::{
        blueprint_store::BlueprintStore, build_store::BuildStore, job_queue::JobQueue,
        planner::Planner, progress::ProgressStore,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub blueprints: BlueprintStore,
    pub builds: BuildStore,
    pub jobs: JobQueue,
    pub progress: ProgressStore,
    pub planner: Planner,
    pub codex: CodexClient,
    pub llm: LlmClient,
    pub chat: ChatRuntimeConfig,
}

impl AppState {
    pub async fn new(config: AppConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config = Arc::new(config);
        let blueprints = BlueprintStore::new(config.storage.data_dir.join("blueprints")).await?;
        let builds = BuildStore::new(config.storage.data_dir.join("builds")).await?;
        seed_default_blueprint(&blueprints).await?;
        let chat = config::load_chat_runtime_config(&config.chat.config_path)?;
        let codex_home = config.storage.data_dir.join("codex_home");
        let codex_home = if config.codex.enabled {
            let controller_url = local_controller_url(&config);
            prepare_project_codex_home(
                &codex_home,
                &controller_url,
                Some(config.security.shared_token.as_str()),
            )
            .await?;
            Some(codex_home)
        } else {
            None
        };
        let progress = ProgressStore::default();
        let codex = CodexClient::with_session_path_and_home(
            config.codex.clone(),
            config.storage.data_dir.join("codex_sessions.json"),
            codex_home,
        )
        .with_progress(progress.clone());
        let llm = LlmClient::new(codex.clone(), config.llm.clone()).with_progress(progress.clone());

        Ok(Self {
            codex: codex.clone(),
            llm: llm.clone(),
            config,
            blueprints,
            builds,
            jobs: JobQueue::default(),
            progress,
            planner: Planner::new(llm),
            chat,
        })
    }
}

fn local_controller_url(config: &AppConfig) -> String {
    let host = if config.server.host == "0.0.0.0" || config.server.host == "::" {
        "127.0.0.1"
    } else {
        config.server.host.as_str()
    };
    format!("http://{}:{}", host, config.server.port)
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
