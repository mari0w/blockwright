use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use base64::{engine::general_purpose, Engine as _};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::Duration;

use crate::{
    config::{self, LlmApiProviderConfig, LlmConfig, LlmProviderKind, LlmRuntimeConfig},
    integrations::codex::{CodexClient, CodexResponseSchema},
    services::progress::ProgressStore,
};

static NEXT_LLM_TRACE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct LlmClient {
    codex: CodexClient,
    config: LlmConfig,
    runtime_override: Option<LlmRuntimeConfig>,
    progress: Option<ProgressStore>,
    http: reqwest::Client,
}

#[derive(Debug, Clone)]
struct ActiveApiProvider {
    kind: LlmProviderKind,
    config: LlmApiProviderConfig,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<Value>,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: &'static str,
    content: ChatMessageContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ChatMessageContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ChatContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ChatImageUrl },
}

#[derive(Debug, Serialize)]
struct ChatImageUrl {
    url: String,
}

#[derive(Debug, Serialize)]
struct GeminiGenerateContentRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    role: &'static str,
    parts: Vec<GeminiContentPart>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum GeminiContentPart {
    Text {
        text: String,
    },
    InlineData {
        #[serde(rename = "inline_data")]
        inline_data: GeminiInlineData,
    },
}

#[derive(Debug, Serialize)]
struct GeminiInlineData {
    #[serde(rename = "mime_type")]
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "responseMimeType")]
    response_mime_type: &'static str,
    #[serde(rename = "responseJsonSchema")]
    response_json_schema: Value,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiGenerateContentResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiResponseContent>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiResponsePart>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponsePart {
    text: Option<String>,
}

impl LlmClient {
    pub fn new(codex: CodexClient, config: LlmConfig) -> Self {
        Self {
            codex,
            config,
            runtime_override: None,
            progress: None,
            http: reqwest::Client::new(),
        }
    }

    pub fn with_progress(mut self, progress: ProgressStore) -> Self {
        self.progress = Some(progress);
        self
    }

    pub fn enabled(&self) -> bool {
        match self
            .runtime_config()
            .map(|config| config.provider)
            .unwrap_or(LlmProviderKind::CodexCli)
        {
            LlmProviderKind::CodexCli => self.codex.enabled(),
            LlmProviderKind::OpenAi
            | LlmProviderKind::DeepSeek
            | LlmProviderKind::Doubao
            | LlmProviderKind::Gemini => true,
        }
    }

    pub fn provider(&self) -> LlmProviderKind {
        self.runtime_config()
            .map(|config| config.provider)
            .unwrap_or(LlmProviderKind::CodexCli)
    }

    pub fn image_input_available(&self) -> bool {
        match self.runtime_config() {
            Ok(config) => match config.provider {
                LlmProviderKind::CodexCli => self.codex.enabled(),
                LlmProviderKind::OpenAi => config.openai.supports_images || self.codex.enabled(),
                LlmProviderKind::DeepSeek => {
                    config.deepseek.supports_images || self.codex.enabled()
                }
                LlmProviderKind::Doubao => config.doubao.supports_images || self.codex.enabled(),
                LlmProviderKind::Gemini => config.gemini.supports_images || self.codex.enabled(),
            },
            Err(_) => self.codex.enabled(),
        }
    }

    pub async fn ask(
        &self,
        prompt: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.ask_inner(prompt, None, None, None, &[]).await
    }

    pub async fn ask_with_schema_and_progress_and_images(
        &self,
        prompt: &str,
        schema: CodexResponseSchema,
        session_key: Option<&str>,
        progress_id: Option<&str>,
        image_paths: &[PathBuf],
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.ask_inner(prompt, Some(schema), session_key, progress_id, image_paths)
            .await
    }

    async fn ask_inner(
        &self,
        prompt: &str,
        schema: Option<CodexResponseSchema>,
        session_key: Option<&str>,
        progress_id: Option<&str>,
        image_paths: &[PathBuf],
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        let runtime_config = self.runtime_config()?;
        match runtime_config.provider {
            LlmProviderKind::CodexCli => match schema {
                Some(schema) => {
                    self.codex
                        .ask_with_schema_and_progress_and_images(
                            prompt,
                            schema,
                            session_key,
                            progress_id,
                            image_paths,
                        )
                        .await
                }
                None => self.codex.ask(prompt).await,
            },
            LlmProviderKind::OpenAi => {
                self.ask_openai_compatible(
                    ActiveApiProvider {
                        kind: LlmProviderKind::OpenAi,
                        config: runtime_config.openai,
                    },
                    prompt,
                    schema,
                    session_key,
                    progress_id,
                    image_paths,
                )
                .await
            }
            LlmProviderKind::DeepSeek => {
                self.ask_openai_compatible(
                    ActiveApiProvider {
                        kind: LlmProviderKind::DeepSeek,
                        config: runtime_config.deepseek,
                    },
                    prompt,
                    schema,
                    session_key,
                    progress_id,
                    image_paths,
                )
                .await
            }
            LlmProviderKind::Doubao => {
                self.ask_openai_compatible(
                    ActiveApiProvider {
                        kind: LlmProviderKind::Doubao,
                        config: runtime_config.doubao,
                    },
                    prompt,
                    schema,
                    session_key,
                    progress_id,
                    image_paths,
                )
                .await
            }
            LlmProviderKind::Gemini => {
                self.ask_gemini(
                    runtime_config.gemini,
                    prompt,
                    schema,
                    session_key,
                    progress_id,
                    image_paths,
                )
                .await
            }
        }
    }

    async fn ask_openai_compatible(
        &self,
        provider: ActiveApiProvider,
        prompt: &str,
        schema: Option<CodexResponseSchema>,
        session_key: Option<&str>,
        progress_id: Option<&str>,
        image_paths: &[PathBuf],
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        if !image_paths.is_empty() && !provider.config.supports_images {
            return self
                .ask_image_fallback(prompt, schema, session_key, progress_id, image_paths)
                .await;
        }

        let api_key = api_key_from_env_or_file(&self.config.env_path, &provider.config.api_key_env)
            .ok_or_else(|| {
                format!(
                    "{} missing API key. Configure {} in {} or save it from the web settings page.",
                    provider.kind.label(),
                    provider.config.api_key_env,
                    self.config.env_path.display()
                )
            })?;
        let trace_id = llm_trace_id();
        let url = chat_completions_url(&provider.config.base_url);
        self.record_progress(progress_id, "AI API 正在处理请求", None);
        tracing::info!(
            trace_id = %trace_id,
            provider = provider.kind.label(),
            model = %provider.config.model,
            url = %url,
            timeout_seconds = provider.config.timeout_seconds,
            image_count = image_paths.len(),
            "starting llm api request"
        );

        let request = ChatCompletionRequest {
            model: provider.config.model.clone(),
            messages: self
                .messages_for_api_request(prompt, &provider.config, image_paths, session_key)
                .await?,
            response_format: schema.map(|schema| response_format_for(&provider.kind, schema)),
        };
        let response = self
            .http
            .post(&url)
            .bearer_auth(api_key)
            .timeout(Duration::from_secs(provider.config.timeout_seconds))
            .json(&request)
            .send()
            .await
            .map_err(|error| format!("llm_trace_id={trace_id}: {error}"))?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            format!("llm_trace_id={trace_id}: failed to read response: {error}")
        })?;
        if !status.is_success() {
            return Err(llm_api_error(&trace_id, status, &body).into());
        }
        let parsed = serde_json::from_str::<ChatCompletionResponse>(&body).map_err(|error| {
            format!("llm_trace_id={trace_id}: failed to parse chat completion response: {error}")
        })?;
        let answer = parsed
            .choices
            .into_iter()
            .find_map(|choice| choice.message.content)
            .unwrap_or_default();
        if answer.trim().is_empty() {
            return Ok(None);
        }
        self.record_progress(progress_id, "AI API 已返回结果", None);
        tracing::info!(
            trace_id = %trace_id,
            provider = provider.kind.label(),
            model = %provider.config.model,
            response_bytes = answer.len(),
            "finished llm api request"
        );
        Ok(Some(answer))
    }

    async fn ask_gemini(
        &self,
        provider_config: LlmApiProviderConfig,
        prompt: &str,
        schema: Option<CodexResponseSchema>,
        session_key: Option<&str>,
        progress_id: Option<&str>,
        image_paths: &[PathBuf],
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        if !image_paths.is_empty() && !provider_config.supports_images {
            return self
                .ask_image_fallback(prompt, schema, session_key, progress_id, image_paths)
                .await;
        }

        let api_key = api_key_from_env_or_file(&self.config.env_path, &provider_config.api_key_env)
            .ok_or_else(|| {
                format!(
                    "Gemini API missing API key. Configure {} in {} or save it from the web settings page.",
                    provider_config.api_key_env,
                    self.config.env_path.display()
                )
            })?;
        let trace_id = llm_trace_id();
        let url = gemini_generate_content_url(&provider_config.base_url, &provider_config.model);
        self.record_progress(progress_id, "Gemini API 正在处理请求", None);
        tracing::info!(
            trace_id = %trace_id,
            provider = "Gemini API",
            model = %provider_config.model,
            url = %url,
            timeout_seconds = provider_config.timeout_seconds,
            image_count = image_paths.len(),
            "starting gemini api request"
        );

        let request = GeminiGenerateContentRequest {
            contents: self
                .gemini_contents_for_request(prompt, &provider_config, image_paths, session_key)
                .await?,
            generation_config: schema.map(gemini_generation_config_for),
        };
        let response = self
            .http
            .post(&url)
            .header("x-goog-api-key", api_key)
            .timeout(Duration::from_secs(provider_config.timeout_seconds))
            .json(&request)
            .send()
            .await
            .map_err(|error| format!("llm_trace_id={trace_id}: {error}"))?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            format!("llm_trace_id={trace_id}: failed to read response: {error}")
        })?;
        if !status.is_success() {
            return Err(llm_api_error(&trace_id, status, &body).into());
        }
        let parsed =
            serde_json::from_str::<GeminiGenerateContentResponse>(&body).map_err(|error| {
                format!("llm_trace_id={trace_id}: failed to parse Gemini response: {error}")
            })?;
        let answer = parsed
            .candidates
            .into_iter()
            .filter_map(|candidate| candidate.content)
            .flat_map(|content| content.parts)
            .filter_map(|part| part.text)
            .collect::<Vec<_>>()
            .join("");
        if answer.trim().is_empty() {
            return Ok(None);
        }
        self.record_progress(progress_id, "Gemini API 已返回结果", None);
        tracing::info!(
            trace_id = %trace_id,
            provider = "Gemini API",
            model = %provider_config.model,
            response_bytes = answer.len(),
            "finished gemini api request"
        );
        Ok(Some(answer))
    }

    async fn ask_image_fallback(
        &self,
        prompt: &str,
        schema: Option<CodexResponseSchema>,
        session_key: Option<&str>,
        progress_id: Option<&str>,
        image_paths: &[PathBuf],
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        if !self.codex.enabled() {
            return Err(
                "当前模型未声明支持图片输入，且 Codex CLI 未启用；请选择支持图片的模型或启用 Codex CLI 图片兜底"
                    .into(),
            );
        }

        self.record_progress(
            progress_id,
            "当前模型不支持图片输入，改用 Codex CLI 处理图片",
            None,
        );
        match schema {
            Some(schema) => {
                self.codex
                    .ask_with_schema_and_progress_and_images(
                        prompt,
                        schema,
                        session_key,
                        progress_id,
                        image_paths,
                    )
                    .await
            }
            None => self.codex.ask(prompt).await,
        }
    }

    async fn messages_for_api_request(
        &self,
        prompt: &str,
        config: &LlmApiProviderConfig,
        image_paths: &[PathBuf],
        _session_key: Option<&str>,
    ) -> Result<Vec<ChatMessage>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![ChatMessage {
            role: "user",
            content: self.message_content(prompt, config, image_paths).await?,
        }])
    }

    async fn gemini_contents_for_request(
        &self,
        prompt: &str,
        config: &LlmApiProviderConfig,
        image_paths: &[PathBuf],
        _session_key: Option<&str>,
    ) -> Result<Vec<GeminiContent>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![GeminiContent {
            role: "user",
            parts: self.gemini_parts(prompt, config, image_paths).await?,
        }])
    }

    async fn gemini_parts(
        &self,
        prompt: &str,
        config: &LlmApiProviderConfig,
        image_paths: &[PathBuf],
    ) -> Result<Vec<GeminiContentPart>, Box<dyn std::error::Error + Send + Sync>> {
        let mut parts = vec![GeminiContentPart::Text {
            text: prompt.to_string(),
        }];
        if !config.supports_images {
            return Ok(parts);
        }
        for image_path in image_paths {
            let bytes = tokio::fs::read(image_path).await?;
            parts.push(GeminiContentPart::InlineData {
                inline_data: GeminiInlineData {
                    mime_type: image_mime_type(image_path).to_string(),
                    data: general_purpose::STANDARD.encode(bytes),
                },
            });
        }
        Ok(parts)
    }

    async fn message_content(
        &self,
        prompt: &str,
        config: &LlmApiProviderConfig,
        image_paths: &[PathBuf],
    ) -> Result<ChatMessageContent, Box<dyn std::error::Error + Send + Sync>> {
        if image_paths.is_empty() || !config.supports_images {
            return Ok(ChatMessageContent::Text(prompt.to_string()));
        }

        let mut parts = vec![ChatContentPart::Text {
            text: prompt.to_string(),
        }];
        for image_path in image_paths {
            let bytes = tokio::fs::read(image_path).await?;
            let mime = image_mime_type(image_path);
            let encoded = general_purpose::STANDARD.encode(bytes);
            parts.push(ChatContentPart::ImageUrl {
                image_url: ChatImageUrl {
                    url: format!("data:{mime};base64,{encoded}"),
                },
            });
        }
        Ok(ChatMessageContent::Parts(parts))
    }

    fn runtime_config(&self) -> Result<LlmRuntimeConfig, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(config) = self.runtime_override.as_ref() {
            return Ok(config.clone());
        }
        config::load_llm_runtime_config(&self.config.config_path)
    }

    fn record_progress(&self, progress_id: Option<&str>, phase: &str, detail: Option<String>) {
        let (Some(progress), Some(progress_id)) = (self.progress.as_ref(), progress_id) else {
            return;
        };
        progress.record(progress_id, phase, detail);
    }
}

impl From<CodexClient> for LlmClient {
    fn from(codex: CodexClient) -> Self {
        let mut client = Self::new(codex, LlmConfig::default());
        client.runtime_override = Some(LlmRuntimeConfig::default());
        client
    }
}

fn chat_completions_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

fn gemini_generate_content_url(base_url: &str, model: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with(":generateContent") {
        return base.to_string();
    }
    if base.contains("/models/") {
        return format!("{base}:generateContent");
    }
    format!("{base}/models/{}:generateContent", model.trim())
}

fn response_format_for(provider: &LlmProviderKind, schema: CodexResponseSchema) -> Value {
    if matches!(provider, LlmProviderKind::OpenAi | LlmProviderKind::Doubao) {
        let schema_value =
            serde_json::from_str::<Value>(schema.json_schema()).unwrap_or(Value::Null);
        return json!({
            "type": "json_schema",
            "json_schema": {
                "name": schema.label(),
                "schema": schema_value,
                "strict": false
            }
        });
    }

    json!({"type": "json_object"})
}

fn gemini_generation_config_for(schema: CodexResponseSchema) -> GeminiGenerationConfig {
    GeminiGenerationConfig {
        response_mime_type: "application/json",
        response_json_schema: serde_json::from_str::<Value>(schema.json_schema())
            .unwrap_or(Value::Null),
    }
}

fn api_key_from_env_or_file(env_path: &PathBuf, key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| env_file_value(env_path, key))
}

fn env_file_value(env_path: &PathBuf, key: &str) -> Option<String> {
    dotenvy::from_path_iter(env_path).ok()?.find_map(|item| {
        let (line_key, value) = item.ok()?;
        (line_key == key)
            .then(|| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn image_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "image/png",
    }
}

fn llm_api_error(trace_id: &str, status: StatusCode, body: &str) -> String {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.pointer("/message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            if body.trim().is_empty() {
                "empty response body".to_string()
            } else {
                body.chars().take(400).collect()
            }
        });
    format!("llm_trace_id={trace_id}: LLM API returned {status}: {message}")
}

fn llm_trace_id() -> String {
    let sequence = NEXT_LLM_TRACE_ID.fetch_add(1, Ordering::Relaxed);
    format!("llm-{}-{sequence}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CodexConfig;
    use std::{fs, os::unix::fs::PermissionsExt};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    #[test]
    fn chat_completion_url_accepts_base_or_full_endpoint() {
        assert_eq!(
            "https://api.openai.com/v1/chat/completions",
            chat_completions_url("https://api.openai.com/v1")
        );
        assert_eq!(
            "https://example.test/chat/completions",
            chat_completions_url("https://example.test/chat/completions")
        );
    }

    #[test]
    fn gemini_generate_content_url_accepts_base_or_full_endpoint() {
        assert_eq!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent",
            gemini_generate_content_url(
                "https://generativelanguage.googleapis.com/v1beta",
                "gemini-2.5-flash"
            )
        );
        assert_eq!(
            "https://example.test/models/custom:generateContent",
            gemini_generate_content_url("https://example.test/models/custom", "ignored")
        );
    }

    #[test]
    fn openai_and_doubao_use_json_schema_response_format() {
        for provider in [LlmProviderKind::OpenAi, LlmProviderKind::Doubao] {
            let format = response_format_for(&provider, CodexResponseSchema::Plan);
            assert_eq!("json_schema", format["type"]);
            assert_eq!("plan", format["json_schema"]["name"]);
        }
    }

    #[test]
    fn reads_api_key_from_env_file_without_requiring_process_env() {
        let path = std::env::temp_dir().join(format!(
            "blockwright-llm-env-{}-{}.env",
            std::process::id(),
            NEXT_LLM_TRACE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, "export OPENAI_API_KEY='test-key'\n").unwrap();

        assert_eq!(
            Some("test-key".to_string()),
            env_file_value(&path, "OPENAI_API_KEY")
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn image_input_available_tracks_provider_support_and_codex_fallback() {
        let mut runtime = LlmRuntimeConfig {
            provider: LlmProviderKind::DeepSeek,
            ..LlmRuntimeConfig::default()
        };
        runtime.deepseek.supports_images = false;
        let mut client = LlmClient::new(
            CodexClient::new(CodexConfig {
                enabled: false,
                command: "codex".to_string(),
                timeout_seconds: 5,
            }),
            LlmConfig::default(),
        );
        client.runtime_override = Some(runtime.clone());

        assert!(!client.image_input_available());

        runtime.deepseek.supports_images = true;
        client.runtime_override = Some(runtime.clone());
        assert!(client.image_input_available());

        runtime.deepseek.supports_images = false;
        let mut client_with_codex = LlmClient::new(
            CodexClient::new(CodexConfig {
                enabled: true,
                command: "codex".to_string(),
                timeout_seconds: 5,
            }),
            LlmConfig::default(),
        );
        client_with_codex.runtime_override = Some(runtime);
        assert!(client_with_codex.image_input_available());
    }

    #[tokio::test]
    async fn api_provider_does_not_replay_local_session_history() {
        let client = LlmClient::new(
            CodexClient::new(CodexConfig {
                enabled: false,
                command: "codex".to_string(),
                timeout_seconds: 5,
            }),
            LlmConfig::default(),
        );

        let messages = client
            .messages_for_api_request(
                "继续刚才的规划",
                &LlmRuntimeConfig::default().openai,
                &[],
                Some("minecraft:steve"),
            )
            .await
            .unwrap();

        assert_eq!(1, messages.len());
        assert_eq!("user", messages[0].role);
        assert_eq!("继续刚才的规划", text_content(&messages[0]));
    }

    #[tokio::test]
    async fn gemini_request_sends_only_current_message_and_inline_images() {
        let dir = std::env::temp_dir().join(format!(
            "blockwright-gemini-request-{}-{}",
            std::process::id(),
            NEXT_LLM_TRACE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let image_path = dir.join("input.png");
        fs::write(&image_path, b"image-bytes").unwrap();
        let client = LlmClient::new(
            CodexClient::new(CodexConfig {
                enabled: false,
                command: "codex".to_string(),
                timeout_seconds: 5,
            }),
            LlmConfig::default(),
        );

        let contents = client
            .gemini_contents_for_request(
                "继续刚才的规划",
                &LlmRuntimeConfig::default().gemini,
                &[image_path],
                Some("minecraft:steve"),
            )
            .await
            .unwrap();
        let serialized = serde_json::to_value(&contents).unwrap();

        assert_eq!(1, serialized.as_array().unwrap().len());
        assert_eq!("user", serialized[0]["role"]);
        assert_eq!("继续刚才的规划", serialized[0]["parts"][0]["text"]);
        assert_eq!(
            "image/png",
            serialized[0]["parts"][1]["inline_data"]["mime_type"]
        );
        assert_eq!(
            "aW1hZ2UtYnl0ZXM=",
            serialized[0]["parts"][1]["inline_data"]["data"]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn doubao_provider_sends_openai_compatible_chat_completion_request() {
        let response_body = r#"{"choices":[{"message":{"content":"{\"reply\":\"ok\",\"summary\":\"ok\",\"blueprint\":null,\"site_plan\":null,\"actions\":[]}"}}]}"#;
        let (base_url, request_rx) = spawn_json_server(response_body).await;
        let env_path = temp_env_file("doubao-http", "ARK_API_KEY=doubao-secret\n");
        let mut runtime = LlmRuntimeConfig {
            provider: LlmProviderKind::Doubao,
            ..LlmRuntimeConfig::default()
        };
        runtime.doubao.model = "doubao-test-model".to_string();
        runtime.doubao.base_url = base_url;
        runtime.doubao.api_key_env = "ARK_API_KEY".to_string();
        let mut client = LlmClient::new(
            CodexClient::new(CodexConfig {
                enabled: false,
                command: "codex".to_string(),
                timeout_seconds: 5,
            }),
            LlmConfig {
                config_path: PathBuf::from("unused.yaml"),
                env_path: env_path.clone(),
            },
        );
        client.runtime_override = Some(runtime);

        let answer = client
            .ask_with_schema_and_progress_and_images(
                "请只输出 JSON",
                CodexResponseSchema::Plan,
                Some("minecraft:steve"),
                None,
                &[],
            )
            .await
            .unwrap()
            .unwrap();
        let raw_request = request_rx.await.unwrap();
        let lower_request = raw_request.to_ascii_lowercase();

        assert!(answer.contains("\"reply\":\"ok\""));
        assert!(raw_request.starts_with("POST /chat/completions "));
        assert!(lower_request.contains("authorization: bearer doubao-secret"));
        assert!(raw_request.contains("\"model\":\"doubao-test-model\""));
        assert!(raw_request.contains("\"type\":\"json_schema\""));
        let _ = fs::remove_file(env_path);
    }

    #[tokio::test]
    async fn gemini_provider_sends_generate_content_request() {
        let response_body = r#"{"candidates":[{"content":{"parts":[{"text":"{\"reply\":\"ok\",\"summary\":\"ok\",\"blueprint\":null,\"site_plan\":null,\"actions\":[]}"}]}}]}"#;
        let (server_base_url, request_rx) = spawn_json_server(response_body).await;
        let env_path = temp_env_file("gemini-http", "GEMINI_API_KEY=gemini-secret\n");
        let mut runtime = LlmRuntimeConfig {
            provider: LlmProviderKind::Gemini,
            ..LlmRuntimeConfig::default()
        };
        runtime.gemini.model = "gemini-test-model".to_string();
        runtime.gemini.base_url = format!("{server_base_url}/v1beta");
        runtime.gemini.api_key_env = "GEMINI_API_KEY".to_string();
        let mut client = LlmClient::new(
            CodexClient::new(CodexConfig {
                enabled: false,
                command: "codex".to_string(),
                timeout_seconds: 5,
            }),
            LlmConfig {
                config_path: PathBuf::from("unused.yaml"),
                env_path: env_path.clone(),
            },
        );
        client.runtime_override = Some(runtime);

        let answer = client
            .ask_with_schema_and_progress_and_images(
                "请只输出 JSON",
                CodexResponseSchema::Plan,
                Some("minecraft:steve"),
                None,
                &[],
            )
            .await
            .unwrap()
            .unwrap();
        let raw_request = request_rx.await.unwrap();
        let lower_request = raw_request.to_ascii_lowercase();

        assert!(answer.contains("\"reply\":\"ok\""));
        assert!(raw_request.starts_with("POST /v1beta/models/gemini-test-model:generateContent "));
        assert!(lower_request.contains("x-goog-api-key: gemini-secret"));
        assert!(raw_request.contains("\"responseMimeType\":\"application/json\""));
        assert!(raw_request.contains("\"responseJsonSchema\""));
        let _ = fs::remove_file(env_path);
    }

    #[tokio::test]
    async fn text_only_api_provider_falls_back_to_codex_for_image_input() {
        let dir = std::env::temp_dir().join(format!(
            "blockwright-llm-image-fallback-{}-{}",
            std::process::id(),
            NEXT_LLM_TRACE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("fake-codex.sh");
        fs::write(
            &script_path,
            r#"#!/usr/bin/env bash
set -euo pipefail
last_message=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-last-message)
      last_message="$2"
      shift 2
      ;;
    --output-schema)
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
{"reply":"图片请求已由 Codex CLI 处理。","summary":"图片兜底","blueprint":null,"site_plan":null,"actions":[]}
BLOCKWRIGHT_JSON
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
        let image_path = dir.join("input.png");
        fs::write(&image_path, b"not-real-png-but-path-exists").unwrap();

        let mut client = LlmClient::new(
            CodexClient::new(CodexConfig {
                enabled: true,
                command: script_path.to_string_lossy().to_string(),
                timeout_seconds: 5,
            }),
            LlmConfig::default(),
        );
        let mut runtime = LlmRuntimeConfig {
            provider: LlmProviderKind::DeepSeek,
            ..LlmRuntimeConfig::default()
        };
        runtime.deepseek.supports_images = false;
        client.runtime_override = Some(runtime);

        let answer = client
            .ask_with_schema_and_progress_and_images(
                "照着图片盖一个房子",
                CodexResponseSchema::Plan,
                Some("minecraft:steve"),
                None,
                &[image_path],
            )
            .await
            .unwrap()
            .unwrap();

        assert!(answer.contains("图片请求已由 Codex CLI 处理"));
        let _ = fs::remove_dir_all(dir);
    }

    fn text_content(message: &ChatMessage) -> &str {
        match &message.content {
            ChatMessageContent::Text(text) => text,
            ChatMessageContent::Parts(_) => "",
        }
    }

    fn temp_env_file(name: &str, source: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "blockwright-{name}-{}-{}.env",
            std::process::id(),
            NEXT_LLM_TRACE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, source).unwrap();
        path
    }

    async fn spawn_json_server(response_body: &'static str) -> (String, oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let raw_request = read_http_request(&mut stream).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            let _ = request_tx.send(raw_request);
        });
        (format!("http://{addr}"), request_rx)
    }

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0, "connection closed before request completed");
            request.extend_from_slice(&buffer[..read]);
            if let Some(headers_end) = find_subsequence(&request, b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..headers_end]).to_ascii_lowercase();
                let content_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let expected_len = headers_end + 4 + content_length;
                while request.len() < expected_len {
                    let read = stream.read(&mut buffer).await.unwrap();
                    assert!(read > 0, "connection closed before body completed");
                    request.extend_from_slice(&buffer[..read]);
                }
                return String::from_utf8_lossy(&request).to_string();
            }
        }
    }

    fn find_subsequence(source: &[u8], needle: &[u8]) -> Option<usize> {
        source
            .windows(needle.len())
            .position(|window| window == needle)
    }
}
