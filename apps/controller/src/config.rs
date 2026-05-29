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
    #[serde(default)]
    pub llm: LlmConfig,
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
pub struct LlmConfig {
    #[serde(default = "default_llm_config_path")]
    pub config_path: PathBuf,
    #[serde(default = "default_env_path")]
    pub env_path: PathBuf,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            config_path: default_llm_config_path(),
            env_path: default_env_path(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatConfig {
    pub config_path: PathBuf,
    #[serde(default = "default_env_path")]
    pub env_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmProviderKind {
    #[serde(rename = "codex_cli", alias = "codex")]
    CodexCli,
    #[serde(rename = "openai", alias = "open_ai")]
    OpenAi,
    #[serde(rename = "deepseek", alias = "deep_seek")]
    DeepSeek,
    #[serde(rename = "doubao", alias = "ark", alias = "volcengine")]
    Doubao,
    #[serde(rename = "gemini", alias = "genmini", alias = "google")]
    Gemini,
}

impl Default for LlmProviderKind {
    fn default() -> Self {
        Self::CodexCli
    }
}

impl LlmProviderKind {
    pub fn label(&self) -> &'static str {
        match self {
            LlmProviderKind::CodexCli => "Codex CLI",
            LlmProviderKind::OpenAi => "OpenAI API",
            LlmProviderKind::DeepSeek => "DeepSeek API",
            LlmProviderKind::Doubao => "Doubao API",
            LlmProviderKind::Gemini => "Gemini API",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LlmRuntimeConfig {
    #[serde(default)]
    pub provider: LlmProviderKind,
    #[serde(default = "default_openai_api_config")]
    pub openai: LlmApiProviderConfig,
    #[serde(default = "default_deepseek_api_config")]
    pub deepseek: LlmApiProviderConfig,
    #[serde(default = "default_doubao_api_config")]
    pub doubao: LlmApiProviderConfig,
    #[serde(default = "default_gemini_api_config")]
    pub gemini: LlmApiProviderConfig,
}

impl Default for LlmRuntimeConfig {
    fn default() -> Self {
        Self {
            provider: LlmProviderKind::CodexCli,
            openai: default_openai_api_config(),
            deepseek: default_deepseek_api_config(),
            doubao: default_doubao_api_config(),
            gemini: default_gemini_api_config(),
        }
    }
}

impl LlmRuntimeConfig {
    pub fn active_api_config(&self) -> Option<&LlmApiProviderConfig> {
        match self.provider {
            LlmProviderKind::OpenAi => Some(&self.openai),
            LlmProviderKind::DeepSeek => Some(&self.deepseek),
            LlmProviderKind::Doubao => Some(&self.doubao),
            LlmProviderKind::Gemini => Some(&self.gemini),
            _ => None,
        }
    }

    pub fn active_api_config_mut(&mut self) -> Option<&mut LlmApiProviderConfig> {
        match self.provider {
            LlmProviderKind::OpenAi => Some(&mut self.openai),
            LlmProviderKind::DeepSeek => Some(&mut self.deepseek),
            LlmProviderKind::Doubao => Some(&mut self.doubao),
            LlmProviderKind::Gemini => Some(&mut self.gemini),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LlmApiProviderConfig {
    pub model: String,
    pub base_url: String,
    pub api_key_env: String,
    #[serde(default)]
    pub supports_images: bool,
    #[serde(default = "default_llm_api_timeout_seconds")]
    pub timeout_seconds: u64,
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

pub fn load_llm_runtime_config(
    path: &Path,
) -> Result<LlmRuntimeConfig, Box<dyn std::error::Error + Send + Sync>> {
    if !path.exists() {
        return Ok(LlmRuntimeConfig::default());
    }

    let source = std::fs::read_to_string(path)?;
    let config = yaml_serde::from_str::<LlmRuntimeConfig>(&source)?;
    validate_llm_runtime_config(&config)?;
    Ok(config)
}

pub fn validate_llm_runtime_config(
    config: &LlmRuntimeConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match config.provider {
        LlmProviderKind::CodexCli => Ok(()),
        LlmProviderKind::OpenAi => validate_llm_api_provider("OpenAI API", &config.openai),
        LlmProviderKind::DeepSeek => validate_llm_api_provider("DeepSeek API", &config.deepseek),
        LlmProviderKind::Doubao => validate_llm_api_provider("Doubao API", &config.doubao),
        LlmProviderKind::Gemini => validate_llm_api_provider("Gemini API", &config.gemini),
    }
}

pub fn write_llm_runtime_config(
    path: &Path,
    config: &LlmRuntimeConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    validate_llm_runtime_config(config)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, yaml_serde::to_string(config)?)?;
    Ok(())
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

fn default_llm_config_path() -> PathBuf {
    PathBuf::from("config/llm.local.yaml")
}

fn default_openai_api_config() -> LlmApiProviderConfig {
    LlmApiProviderConfig {
        model: "gpt-4.1".to_string(),
        base_url: "https://api.openai.com/v1".to_string(),
        api_key_env: "OPENAI_API_KEY".to_string(),
        supports_images: true,
        timeout_seconds: default_llm_api_timeout_seconds(),
    }
}

fn default_deepseek_api_config() -> LlmApiProviderConfig {
    LlmApiProviderConfig {
        model: "deepseek-v4-flash".to_string(),
        base_url: "https://api.deepseek.com".to_string(),
        api_key_env: "DEEPSEEK_API_KEY".to_string(),
        supports_images: false,
        timeout_seconds: default_llm_api_timeout_seconds(),
    }
}

fn default_doubao_api_config() -> LlmApiProviderConfig {
    LlmApiProviderConfig {
        model: "doubao-seed-2-0-lite-260215".to_string(),
        base_url: "https://ark.cn-beijing.volces.com/api/v3".to_string(),
        api_key_env: "ARK_API_KEY".to_string(),
        supports_images: true,
        timeout_seconds: default_llm_api_timeout_seconds(),
    }
}

fn default_gemini_api_config() -> LlmApiProviderConfig {
    LlmApiProviderConfig {
        model: "gemini-2.5-flash".to_string(),
        base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
        api_key_env: "GEMINI_API_KEY".to_string(),
        supports_images: true,
        timeout_seconds: default_llm_api_timeout_seconds(),
    }
}

fn default_llm_api_timeout_seconds() -> u64 {
    1800
}

fn validate_llm_api_provider(
    label: &str,
    config: &LlmApiProviderConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if config.model.trim().is_empty()
        || config.base_url.trim().is_empty()
        || config.api_key_env.trim().is_empty()
    {
        return Err(format!("{label} 的 model、base_url、api_key_env 不能为空").into());
    }
    validate_env_key_name(label, &config.api_key_env)?;
    validate_llm_api_base_url(label, &config.base_url)?;
    if config.timeout_seconds == 0 {
        return Err(format!("{label} 的 timeout_seconds 必须大于 0").into());
    }
    Ok(())
}

pub fn validate_env_key_name(
    label: &str,
    key: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let key = key.trim();
    let Some(first) = key.chars().next() else {
        return Err(format!("{label} 的 api_key_env 不能为空").into());
    };
    if !(first == '_' || first.is_ascii_alphabetic())
        || !key
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        return Err(
            format!("{label} 的 api_key_env 只能使用字母、数字和下划线，且不能以数字开头").into(),
        );
    }
    Ok(())
}

pub fn validate_llm_api_base_url(
    label: &str,
    base_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = reqwest::Url::parse(base_url.trim())
        .map_err(|_| format!("{label} 的 base_url 必须是有效的 http(s) URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("{label} 的 base_url 必须使用 http 或 https").into());
    }
    Ok(())
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
    fn missing_llm_runtime_config_defaults_to_codex_cli() {
        let path = std::env::temp_dir().join(format!(
            "blockwright-missing-llm-config-{}-{}.yaml",
            std::process::id(),
            1
        ));
        let _ = std::fs::remove_file(&path);

        let config = load_llm_runtime_config(&path).unwrap();

        assert_eq!(LlmProviderKind::CodexCli, config.provider);
        assert_eq!("gpt-4.1", config.openai.model);
        assert_eq!("deepseek-v4-flash", config.deepseek.model);
        assert_eq!("doubao-seed-2-0-lite-260215", config.doubao.model);
        assert_eq!("gemini-2.5-flash", config.gemini.model);
        assert_eq!(1800, config.openai.timeout_seconds);
    }

    #[test]
    fn llm_runtime_config_accepts_api_provider_aliases() {
        let source = r#"
provider: genmini
openai:
  model: gpt-4.1-mini
  base_url: https://api.openai.com/v1
  api_key_env: OPENAI_API_KEY
  supports_images: true
  timeout_seconds: 120
deepseek:
  model: deepseek-v4-flash
  base_url: https://api.deepseek.com
  api_key_env: DEEPSEEK_API_KEY
  supports_images: false
  timeout_seconds: 120
doubao:
  model: doubao-seed-2-0-lite-260215
  base_url: https://ark.cn-beijing.volces.com/api/v3
  api_key_env: ARK_API_KEY
  supports_images: true
  timeout_seconds: 120
gemini:
  model: gemini-2.5-flash
  base_url: https://generativelanguage.googleapis.com/v1beta
  api_key_env: GEMINI_API_KEY
  supports_images: true
  timeout_seconds: 120
"#;

        let config = yaml_serde::from_str::<LlmRuntimeConfig>(source).unwrap();

        assert_eq!(LlmProviderKind::Gemini, config.provider);
        assert!(validate_llm_runtime_config(&config).is_ok());
        assert_eq!("gpt-4.1-mini", config.openai.model);
        assert_eq!("doubao-seed-2-0-lite-260215", config.doubao.model);
    }

    #[test]
    fn llm_runtime_config_accepts_openai_deepseek_doubao_and_gemini() {
        for provider in [
            LlmProviderKind::OpenAi,
            LlmProviderKind::DeepSeek,
            LlmProviderKind::Doubao,
            LlmProviderKind::Gemini,
        ] {
            let config = LlmRuntimeConfig {
                provider,
                ..LlmRuntimeConfig::default()
            };

            assert!(validate_llm_runtime_config(&config).is_ok());
        }
    }

    #[test]
    fn llm_runtime_config_rejects_invalid_active_base_url() {
        let mut config = LlmRuntimeConfig {
            provider: LlmProviderKind::OpenAi,
            ..LlmRuntimeConfig::default()
        };
        config.openai.base_url = "not a url".to_string();

        assert!(validate_llm_runtime_config(&config).is_err());
    }

    #[test]
    fn llm_runtime_config_rejects_invalid_api_key_env_name() {
        let mut config = LlmRuntimeConfig {
            provider: LlmProviderKind::OpenAi,
            ..LlmRuntimeConfig::default()
        };
        config.openai.api_key_env = "BAD KEY".to_string();

        assert!(validate_llm_runtime_config(&config).is_err());
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
