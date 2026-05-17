use include_dir::{include_dir, Dir};
use serde::Deserialize;
use std::path::PathBuf;

static SERVER_CONFIG_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../config/servers");

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub minecraft: MinecraftConfig,
    pub security: SecurityConfig,
    pub codex: CodexConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub environment: String,
    pub app_name: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinecraftConfig {
    pub default_server_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    pub shared_token: String,
    pub require_token: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodexConfig {
    pub enabled: bool,
    pub command: String,
    pub timeout_seconds: u64,
}

// 根据 SERVER_NAME 选择编译进二进制里的服务器配置。
pub fn load() -> Result<AppConfig, Box<dyn std::error::Error + Send + Sync>> {
    let server_name = std::env::var("SERVER_NAME").unwrap_or_else(|_| "local".to_string());
    let config_path = format!("{server_name}.yaml");
    let config_file = SERVER_CONFIG_DIR.get_file(&config_path).ok_or_else(|| {
        format!(
            "missing server config: config/servers/{config_path}; available: {}",
            available_server_names()
        )
    })?;
    let config_source = config_file
        .contents_utf8()
        .ok_or_else(|| format!("server config is not valid UTF-8: config/servers/{config_path}"))?;

    let config = yaml_serde::from_str::<AppConfig>(config_source)?;
    if config.server.name != server_name {
        tracing::warn!(
            env_server_name = %server_name,
            config_server_name = %config.server.name,
            "SERVER_NAME and selected config server.name are different"
        );
    }

    Ok(config)
}

fn available_server_names() -> String {
    let names = SERVER_CONFIG_DIR
        .files()
        .filter_map(|file| file.path().file_stem())
        .filter_map(|stem| stem.to_str())
        .collect::<Vec<_>>();

    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}
