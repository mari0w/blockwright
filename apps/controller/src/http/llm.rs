use std::path::PathBuf;

use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde::{Deserialize, Serialize};

use crate::{
    config::{
        self, write_llm_runtime_config, LlmApiProviderConfig, LlmProviderKind, LlmRuntimeConfig,
    },
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct LlmConfigResponse {
    pub ok: bool,
    pub message: String,
    pub provider: LlmProviderKind,
    pub config_path: String,
    pub env_path: String,
    pub codex_cli: CodexCliInfo,
    pub openai: LlmApiProviderInfo,
    pub deepseek: LlmApiProviderInfo,
    pub doubao: LlmApiProviderInfo,
    pub gemini: LlmApiProviderInfo,
}

#[derive(Debug, Serialize)]
pub struct CodexCliInfo {
    pub enabled: bool,
    pub command: String,
    pub timeout_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct LlmApiProviderInfo {
    pub model: String,
    pub base_url: String,
    pub api_key_env: String,
    pub api_key_configured: bool,
    pub supports_images: bool,
    pub timeout_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct LlmConfigRequest {
    pub provider: LlmProviderKind,
    #[serde(default)]
    pub openai: Option<LlmApiProviderRequest>,
    #[serde(default)]
    pub deepseek: Option<LlmApiProviderRequest>,
    #[serde(default)]
    pub doubao: Option<LlmApiProviderRequest>,
    #[serde(default)]
    pub gemini: Option<LlmApiProviderRequest>,
}

#[derive(Debug, Deserialize)]
pub struct LlmApiProviderRequest {
    pub model: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub supports_images: Option<bool>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/llm/config", get(get_llm_config).put(save_llm_config))
}

async fn get_llm_config(
    State(state): State<AppState>,
) -> Result<Json<LlmConfigResponse>, (StatusCode, String)> {
    let runtime = load_runtime_or_default(&state)?;
    Ok(Json(response_from_runtime(
        &state,
        runtime,
        "LLM configuration loaded.",
    )))
}

async fn save_llm_config(
    State(state): State<AppState>,
    Json(request): Json<LlmConfigRequest>,
) -> Result<Json<LlmConfigResponse>, (StatusCode, String)> {
    let openai_api_key = request
        .openai
        .as_ref()
        .and_then(|config| config.api_key.clone());
    let deepseek_api_key = request
        .deepseek
        .as_ref()
        .and_then(|config| config.api_key.clone());
    let doubao_api_key = request
        .doubao
        .as_ref()
        .and_then(|config| config.api_key.clone());
    let gemini_api_key = request
        .gemini
        .as_ref()
        .and_then(|config| config.api_key.clone());

    let mut runtime = load_runtime_or_default(&state)?;
    runtime.provider = request.provider;
    if let Some(openai) = request.openai {
        apply_api_request(&mut runtime.openai, openai)?;
    }
    if let Some(deepseek) = request.deepseek {
        apply_api_request(&mut runtime.deepseek, deepseek)?;
    }
    if let Some(doubao) = request.doubao {
        apply_api_request(&mut runtime.doubao, doubao)?;
    }
    if let Some(gemini) = request.gemini {
        apply_api_request(&mut runtime.gemini, gemini)?;
    }

    let active_api_key = match runtime.provider {
        LlmProviderKind::OpenAi => openai_api_key,
        LlmProviderKind::DeepSeek => deepseek_api_key,
        LlmProviderKind::Doubao => doubao_api_key,
        LlmProviderKind::Gemini => gemini_api_key,
        _ => None,
    };
    let active_api_config = runtime.active_api_config().cloned();
    if let Some(api_config) = active_api_config.as_ref() {
        ensure_active_api_key(
            &state.config.llm.env_path,
            api_config,
            active_api_key.as_deref(),
        )?;
    }
    write_llm_runtime_config(&state.config.llm.config_path, &runtime)
        .map_err(internal_error_response)?;
    if let (Some(api_config), Some(api_key)) = (active_api_config.as_ref(), active_api_key.as_ref())
    {
        let api_key = api_key.trim();
        if !api_key.is_empty() {
            ensure_env_value(&state.config.llm.env_path, &api_config.api_key_env, api_key)
                .map_err(internal_error_response)?;
            std::env::set_var(&api_config.api_key_env, api_key);
        }
    }

    Ok(Json(response_from_runtime(
        &state,
        runtime,
        "LLM configuration saved.",
    )))
}

fn apply_api_request(
    target: &mut LlmApiProviderConfig,
    request: LlmApiProviderRequest,
) -> Result<(), (StatusCode, String)> {
    if contains_line_break(&request.model)
        || contains_line_break(&request.base_url)
        || request
            .api_key_env
            .as_deref()
            .map(contains_line_break)
            .unwrap_or(false)
        || request
            .api_key
            .as_deref()
            .map(contains_line_break)
            .unwrap_or(false)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "LLM configuration values cannot contain line breaks.".to_string(),
        ));
    }

    let model = request.model.trim();
    let base_url = request.base_url.trim();
    if model.is_empty() || base_url.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Model and Base URL are required.".to_string(),
        ));
    }
    config::validate_llm_api_base_url("LLM API", base_url)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;

    target.model = model.to_string();
    target.base_url = base_url.to_string();
    if let Some(api_key_env) = normalize_optional_string(request.api_key_env.as_deref()) {
        config::validate_env_key_name("LLM API", &api_key_env)
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
        target.api_key_env = api_key_env;
    }
    if let Some(supports_images) = request.supports_images {
        target.supports_images = supports_images;
    }
    if let Some(timeout_seconds) = request.timeout_seconds {
        if timeout_seconds == 0 {
            return Err((
                StatusCode::BAD_REQUEST,
                "Timeout seconds must be greater than 0.".to_string(),
            ));
        }
        target.timeout_seconds = timeout_seconds;
    }
    Ok(())
}

fn ensure_active_api_key(
    env_path: &PathBuf,
    api_config: &LlmApiProviderConfig,
    api_key: Option<&str>,
) -> Result<(), (StatusCode, String)> {
    if api_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        return Ok(());
    }
    if env_key_configured(env_path, &api_config.api_key_env) {
        return Ok(());
    }

    Err((
        StatusCode::BAD_REQUEST,
        format!(
            "{} is required. If it is already configured in .env, leave the field blank.",
            api_config.api_key_env
        ),
    ))
}

fn response_from_runtime(
    state: &AppState,
    runtime: LlmRuntimeConfig,
    message: &str,
) -> LlmConfigResponse {
    LlmConfigResponse {
        ok: true,
        message: message.to_string(),
        provider: runtime.provider,
        config_path: state.config.llm.config_path.display().to_string(),
        env_path: state.config.llm.env_path.display().to_string(),
        codex_cli: CodexCliInfo {
            enabled: state.config.codex.enabled,
            command: state.config.codex.command.clone(),
            timeout_seconds: state.config.codex.timeout_seconds,
        },
        openai: api_info(&state.config.llm.env_path, &runtime.openai),
        deepseek: api_info(&state.config.llm.env_path, &runtime.deepseek),
        doubao: api_info(&state.config.llm.env_path, &runtime.doubao),
        gemini: api_info(&state.config.llm.env_path, &runtime.gemini),
    }
}

fn api_info(env_path: &PathBuf, config: &LlmApiProviderConfig) -> LlmApiProviderInfo {
    LlmApiProviderInfo {
        model: config.model.clone(),
        base_url: config.base_url.clone(),
        api_key_env: config.api_key_env.clone(),
        api_key_configured: env_key_configured(env_path, &config.api_key_env),
        supports_images: config.supports_images,
        timeout_seconds: config.timeout_seconds,
    }
}

fn load_runtime_or_default(state: &AppState) -> Result<LlmRuntimeConfig, (StatusCode, String)> {
    config::load_llm_runtime_config(&state.config.llm.config_path).map_err(internal_error_response)
}

fn env_key_configured(path: &PathBuf, key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || dotenvy::from_path_iter(path)
            .map(|iter| {
                iter.filter_map(Result::ok)
                    .any(|(line_key, value)| line_key == key && !value.trim().is_empty())
            })
            .unwrap_or(false)
}

fn ensure_env_value(
    path: &PathBuf,
    key: &str,
    value: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut updated = Vec::new();
    let mut replaced = false;
    for line in existing.lines() {
        if env_line_uses_key(line, key) {
            updated.push(format!("{key}={value}"));
            replaced = true;
        } else {
            updated.push(line.to_string());
        }
    }
    if !replaced {
        updated.push(format!("{key}={value}"));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{}\n", updated.join("\n")))?;
    Ok(())
}

fn env_line_uses_key(line: &str, key: &str) -> bool {
    let line = line.trim_start();
    let line = line.strip_prefix("export ").unwrap_or(line);
    line.starts_with(&format!("{key}="))
}

fn normalize_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn contains_line_break(value: &str) -> bool {
    value.contains('\n') || value.contains('\r')
}

fn internal_error_response(
    error: Box<dyn std::error::Error + Send + Sync>,
) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
