use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

static SERVER_CONFIG_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../config/servers");

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub minecraft: MinecraftConfig,
    pub security: SecurityConfig,
    pub codex: CodexConfig,
    pub chat: ChatConfig,
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

#[derive(Debug, Clone, Deserialize)]
pub struct ChatConfig {
    pub config_path: PathBuf,
    #[serde(default = "default_env_path")]
    pub env_path: PathBuf,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ChatRuntimeConfig {
    #[serde(default)]
    pub tools: Vec<ChatToolConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatToolConfig {
    pub name: String,
    pub platform: ChatPlatform,
    pub enabled: bool,
    pub inbound: ChatInboundMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_server_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_target_player: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dingtalk: Option<DingTalkChatConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix: Option<MatrixChatConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum ChatPlatform {
    #[serde(rename = "dingtalk", alias = "ding_talk")]
    DingTalk,
    #[serde(rename = "minecraft")]
    Minecraft,
    #[serde(rename = "telegram")]
    Telegram,
    #[serde(rename = "matrix", alias = "element")]
    Matrix,
    #[serde(rename = "generic")]
    Generic,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatInboundMode {
    LocalCommand,
    Polling,
    Stream,
    Webhook,
}

impl ChatInboundMode {
    pub fn local_friendly(&self) -> bool {
        matches!(
            self,
            ChatInboundMode::LocalCommand | ChatInboundMode::Polling | ChatInboundMode::Stream
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DingTalkChatConfig {
    pub client_id_env: String,
    pub client_secret_env: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub robot_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MatrixChatConfig {
    pub homeserver_url: String,
    pub access_token_env: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_senders: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_own_user_messages: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_join_invites: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_interval_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_timeout_seconds: Option<u64>,
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

pub fn load_chat_runtime_config(
    path: &Path,
) -> Result<ChatRuntimeConfig, Box<dyn std::error::Error + Send + Sync>> {
    if !path.exists() {
        return Ok(ChatRuntimeConfig::default());
    }

    let source = std::fs::read_to_string(path)?;
    let config = yaml_serde::from_str::<ChatRuntimeConfig>(&source)?;
    validate_chat_runtime_config(&config)?;
    Ok(config)
}

fn validate_chat_runtime_config(
    config: &ChatRuntimeConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for tool in &config.tools {
        if tool.enabled && !tool.inbound.local_friendly() {
            return Err(format!(
                "聊天工具 `{}` 使用 webhook-only 接入，不适合本地 Minecraft 场景；请改用 polling、stream 或 local_command",
                tool.name
            )
            .into());
        }

        if tool.enabled && tool.platform == ChatPlatform::DingTalk && tool.dingtalk.is_none() {
            return Err(format!("钉钉聊天工具 `{}` 缺少 dingtalk 配置", tool.name).into());
        }

        if tool.enabled && tool.platform == ChatPlatform::Matrix {
            if tool.inbound != ChatInboundMode::Polling {
                return Err(format!(
                    "Matrix/Element 聊天工具 `{}` 当前只支持 polling 接入",
                    tool.name
                )
                .into());
            }

            let Some(matrix) = tool.matrix.as_ref() else {
                return Err(
                    format!("Matrix/Element 聊天工具 `{}` 缺少 matrix 配置", tool.name).into(),
                );
            };
            if matrix.homeserver_url.trim().is_empty() || matrix.access_token_env.trim().is_empty()
            {
                return Err(format!(
                    "Matrix/Element 聊天工具 `{}` 的 homeserver_url、access_token_env 不能为空",
                    tool.name
                )
                .into());
            }
            if matrix
                .room_id
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
                && matrix.allowed_senders.is_empty()
            {
                return Err(format!(
                    "Matrix/Element 聊天工具 `{}` 至少要配置 room_id 或 allowed_senders",
                    tool.name
                )
                .into());
            }
        }
    }

    Ok(())
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

fn default_env_path() -> PathBuf {
    PathBuf::from(".env")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_friendly_inbound_modes_exclude_webhook() {
        assert!(ChatInboundMode::Polling.local_friendly());
        assert!(ChatInboundMode::Stream.local_friendly());
        assert!(ChatInboundMode::LocalCommand.local_friendly());
        assert!(!ChatInboundMode::Webhook.local_friendly());
    }

    #[test]
    fn local_server_config_uses_thirty_minute_codex_timeout_and_medium_reasoning() {
        let config_source = SERVER_CONFIG_DIR
            .get_file("local.yaml")
            .and_then(|file| file.contents_utf8())
            .expect("local server config should be embedded");

        let config = yaml_serde::from_str::<AppConfig>(config_source).unwrap();

        assert_eq!(1800, config.codex.timeout_seconds);
        assert_eq!("0.0.0.0", config.server.host);
        assert!(config
            .codex
            .command
            .contains("model_reasoning_effort=medium"));
        assert!(config.codex.command.contains("--skip-git-repo-check"));
        assert!(!config.codex.command.contains("--ignore-user-config"));
    }

    #[test]
    fn rejects_enabled_webhook_only_chat_tool() {
        let config = ChatRuntimeConfig {
            tools: vec![ChatToolConfig {
                name: "dingtalk-webhook".to_string(),
                platform: ChatPlatform::DingTalk,
                enabled: true,
                inbound: ChatInboundMode::Webhook,
                default_server_id: None,
                default_target_player: None,
                dingtalk: Some(DingTalkChatConfig {
                    client_id_env: "DINGTALK_CLIENT_ID".to_string(),
                    client_secret_env: "DINGTALK_CLIENT_SECRET".to_string(),
                    robot_code: None,
                }),
                matrix: None,
            }],
        };

        assert!(validate_chat_runtime_config(&config).is_err());
    }

    #[test]
    fn accepts_enabled_dingtalk_stream_tool() {
        let config = ChatRuntimeConfig {
            tools: vec![ChatToolConfig {
                name: "dingtalk-local".to_string(),
                platform: ChatPlatform::DingTalk,
                enabled: true,
                inbound: ChatInboundMode::Stream,
                default_server_id: Some("local-paper".to_string()),
                default_target_player: Some("Steve".to_string()),
                dingtalk: Some(DingTalkChatConfig {
                    client_id_env: "DINGTALK_CLIENT_ID".to_string(),
                    client_secret_env: "DINGTALK_CLIENT_SECRET".to_string(),
                    robot_code: Some("dingxxx".to_string()),
                }),
                matrix: None,
            }],
        };

        assert!(validate_chat_runtime_config(&config).is_ok());
    }

    #[test]
    fn accepts_matrix_polling_tool() {
        let config = ChatRuntimeConfig {
            tools: vec![ChatToolConfig {
                name: "element-local".to_string(),
                platform: ChatPlatform::Matrix,
                enabled: true,
                inbound: ChatInboundMode::Polling,
                default_server_id: Some("hmcl-lan".to_string()),
                default_target_player: Some("Charles".to_string()),
                dingtalk: None,
                matrix: Some(MatrixChatConfig {
                    homeserver_url: "https://matrix.org".to_string(),
                    access_token_env: "MATRIX_ACCESS_TOKEN".to_string(),
                    room_id: Some("!room:matrix.org".to_string()),
                    allowed_senders: Vec::new(),
                    allow_own_user_messages: None,
                    auto_join_invites: None,
                    poll_interval_seconds: None,
                    sync_timeout_seconds: None,
                }),
            }],
        };

        assert!(validate_chat_runtime_config(&config).is_ok());
    }

    #[test]
    fn accepts_matrix_allowed_sender_without_room_id() {
        let config = ChatRuntimeConfig {
            tools: vec![ChatToolConfig {
                name: "element-local".to_string(),
                platform: ChatPlatform::Matrix,
                enabled: true,
                inbound: ChatInboundMode::Polling,
                default_server_id: Some("hmcl-lan".to_string()),
                default_target_player: Some("Charles".to_string()),
                dingtalk: None,
                matrix: Some(MatrixChatConfig {
                    homeserver_url: "https://matrix.org".to_string(),
                    access_token_env: "MATRIX_ACCESS_TOKEN".to_string(),
                    room_id: None,
                    allowed_senders: vec!["@enochzzg:matrix.org".to_string()],
                    allow_own_user_messages: Some(true),
                    auto_join_invites: Some(true),
                    poll_interval_seconds: None,
                    sync_timeout_seconds: None,
                }),
            }],
        };

        assert!(validate_chat_runtime_config(&config).is_ok());
    }

    #[test]
    fn rejects_matrix_non_polling_tool() {
        let config = ChatRuntimeConfig {
            tools: vec![ChatToolConfig {
                name: "element-stream".to_string(),
                platform: ChatPlatform::Matrix,
                enabled: true,
                inbound: ChatInboundMode::Stream,
                default_server_id: None,
                default_target_player: None,
                dingtalk: None,
                matrix: Some(MatrixChatConfig {
                    homeserver_url: "https://matrix.org".to_string(),
                    access_token_env: "MATRIX_ACCESS_TOKEN".to_string(),
                    room_id: Some("!room:matrix.org".to_string()),
                    allowed_senders: Vec::new(),
                    allow_own_user_messages: None,
                    auto_join_invites: None,
                    poll_interval_seconds: None,
                    sync_timeout_seconds: None,
                }),
            }],
        };

        assert!(validate_chat_runtime_config(&config).is_err());
    }
}
