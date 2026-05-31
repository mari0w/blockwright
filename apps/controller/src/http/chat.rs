use std::collections::HashMap;
use std::path::PathBuf;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    config::{
        self, ChatInboundMode, ChatPlatform, ChatRuntimeConfig, ChatToolConfig, DingTalkChatConfig,
        MatrixChatConfig,
    },
    domain::types::{ChatAttachment, ChatAttachmentKind, ChatAttachmentSource},
    http::robot::{queue_chat_message, RobotMessageResponse},
    integrations::matrix,
    services::chat::IncomingChatMessage,
    state::AppState,
};

const DINGTALK_BOT_MESSAGE_TOPIC: &str = "/v1.0/im/bot/messages/get";
const DINGTALK_LOCAL_TOOL_NAME: &str = "dingtalk-local";
const DINGTALK_CLIENT_ID_ENV: &str = "DINGTALK_CLIENT_ID";
const DINGTALK_CLIENT_SECRET_ENV: &str = "DINGTALK_CLIENT_SECRET";
const MATRIX_LOCAL_TOOL_NAME: &str = "element-local";
const MATRIX_ACCESS_TOKEN_ENV: &str = "MATRIX_ACCESS_TOKEN";

#[derive(Debug, Serialize)]
pub struct ChatAdaptersResponse {
    pub tools: Vec<ChatAdapterInfo>,
}

#[derive(Debug, Serialize)]
pub struct ChatAdapterInfo {
    pub name: String,
    pub platform: ChatPlatform,
    pub enabled: bool,
    pub inbound: ChatInboundMode,
    pub local_friendly: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum ChatToolSelection {
    #[serde(rename = "matrix", alias = "element")]
    Matrix,
    #[serde(rename = "dingtalk", alias = "ding_talk")]
    DingTalk,
}

#[derive(Debug, Deserialize)]
pub struct ChatLocalConfigRequest {
    #[serde(default)]
    pub enabled_tools: Vec<ChatToolSelection>,
    #[serde(default)]
    pub matrix: Option<MatrixLocalConfigRequest>,
    #[serde(default)]
    pub dingtalk: Option<DingTalkLocalConfigRequest>,
}

#[derive(Debug, Serialize)]
pub struct ChatLocalConfigResponse {
    pub ok: bool,
    pub message: String,
    pub enabled_tools: Vec<ChatToolSelection>,
    pub config_path: String,
    pub env_path: String,
    pub matrix: MatrixLocalConfigView,
    pub dingtalk: DingTalkLocalConfigView,
    pub poller_started: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatrixLocalConfigView {
    pub enabled: bool,
    pub homeserver_url: String,
    pub room_id: Option<String>,
    pub allowed_sender: String,
    pub allow_own_user_messages: bool,
    pub auto_join_invites: bool,
    pub default_server_id: Option<String>,
    pub default_target_player: Option<String>,
    pub poll_interval_seconds: Option<u64>,
    pub sync_timeout_seconds: Option<u64>,
    pub token_configured: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MatrixLocalConfigRequest {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub homeserver_url: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub room_id: Option<String>,
    pub allowed_sender: String,
    #[serde(default = "default_true")]
    pub allow_own_user_messages: bool,
    #[serde(default = "default_true")]
    pub auto_join_invites: bool,
    #[serde(default)]
    pub default_server_id: Option<String>,
    #[serde(default)]
    pub default_target_player: Option<String>,
    #[serde(default)]
    pub poll_interval_seconds: Option<u64>,
    #[serde(default)]
    pub sync_timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DingTalkLocalConfigView {
    pub enabled: bool,
    pub robot_code: Option<String>,
    pub default_server_id: Option<String>,
    pub default_target_player: Option<String>,
    pub client_id_configured: bool,
    pub client_secret_configured: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DingTalkLocalConfigRequest {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub robot_code: Option<String>,
    #[serde(default)]
    pub default_server_id: Option<String>,
    #[serde(default)]
    pub default_target_player: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MatrixLocalConfigResponse {
    pub ok: bool,
    pub message: String,
    pub tool_name: String,
    pub config_path: String,
    pub env_path: String,
    pub token_configured: bool,
    pub poller_started: bool,
}

#[derive(Debug, Deserialize)]
pub struct DingTalkStreamRequest {
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub data: String,
}

#[derive(Debug, Serialize)]
pub struct DingTalkStreamResponse {
    pub code: u16,
    pub message: String,
    pub headers: DingTalkStreamResponseHeaders,
    pub data: String,
    pub result: RobotMessageResponse,
}

#[derive(Debug, Serialize)]
pub struct DingTalkStreamResponseHeaders {
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(rename = "contentType")]
    pub content_type: String,
}

#[derive(Debug, Deserialize)]
struct DingTalkBotMessage {
    #[serde(rename = "conversationId")]
    conversation_id: String,
    #[serde(rename = "senderNick")]
    sender_nick: Option<String>,
    #[serde(rename = "senderStaffId")]
    sender_staff_id: Option<String>,
    #[serde(rename = "senderId")]
    sender_id: Option<String>,
    #[serde(rename = "msgtype")]
    msg_type: String,
    text: Option<DingTalkText>,
    content: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct DingTalkText {
    content: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/chat/adapters", get(list_adapters))
        .route(
            "/chat/config",
            get(get_chat_local_config).put(save_chat_local_config),
        )
        .route("/chat/matrix/local-config", put(save_matrix_local_config))
        .route(
            "/chat/dingtalk/stream",
            axum::routing::post(handle_dingtalk_stream),
        )
}

async fn get_chat_local_config(
    State(state): State<AppState>,
) -> Result<Json<ChatLocalConfigResponse>, (StatusCode, String)> {
    let config = config::load_chat_runtime_config(&state.config.chat.config_path)
        .map_err(internal_error_response)?;

    Ok(Json(chat_local_config_response(
        &state,
        &config,
        "Chat configuration loaded.".to_string(),
        false,
    )))
}

async fn save_chat_local_config(
    State(state): State<AppState>,
    Json(request): Json<ChatLocalConfigRequest>,
) -> Result<Json<ChatLocalConfigResponse>, (StatusCode, String)> {
    let enabled_tools = normalized_enabled_tools(&request.enabled_tools);
    let matrix_enabled = enabled_tools.contains(&ChatToolSelection::Matrix);
    let dingtalk_enabled = enabled_tools.contains(&ChatToolSelection::DingTalk);
    let mut runtime_config = config::load_chat_runtime_config(&state.config.chat.config_path)
        .map_err(internal_error_response)?;

    let mut matrix_request = request
        .matrix
        .unwrap_or_else(|| matrix_request_from_config(&runtime_config, matrix_enabled));
    matrix_request.enabled = matrix_enabled;
    let matrix_tool = matrix_tool_from_request(&matrix_request)?;

    let mut dingtalk_request = request
        .dingtalk
        .unwrap_or_else(|| dingtalk_request_from_config(&runtime_config, dingtalk_enabled));
    dingtalk_request.enabled = dingtalk_enabled;
    let dingtalk_tool = dingtalk_tool_from_request(&dingtalk_request)?;

    let env_path = state.config.chat.env_path.clone();
    let matrix_token = matrix_request.access_token.trim();
    let mut matrix_token_configured = env_key_exists(&env_path, MATRIX_ACCESS_TOKEN_ENV)
        || env_var_exists(MATRIX_ACCESS_TOKEN_ENV);
    if matrix_enabled && matrix_token.is_empty() && !matrix_token_configured {
        return Err((
            StatusCode::BAD_REQUEST,
            "Matrix access token is required. If a token is already configured, leave this field blank to keep it.".to_string(),
        ));
    }

    upsert_chat_tool_in_config(&mut runtime_config, matrix_tool.clone());
    upsert_chat_tool_in_config(&mut runtime_config, dingtalk_tool);
    config::write_chat_runtime_config(&state.config.chat.config_path, &runtime_config)
        .map_err(internal_error_response)?;

    if !matrix_token.is_empty() {
        ensure_env_value(&env_path, MATRIX_ACCESS_TOKEN_ENV, matrix_token)
            .map_err(internal_error_response)?;
        std::env::set_var(MATRIX_ACCESS_TOKEN_ENV, matrix_token);
        matrix_token_configured = true;
    }

    let dingtalk_client_id = dingtalk_request.client_id.trim();
    if !dingtalk_client_id.is_empty() {
        ensure_env_value(&env_path, DINGTALK_CLIENT_ID_ENV, dingtalk_client_id)
            .map_err(internal_error_response)?;
        std::env::set_var(DINGTALK_CLIENT_ID_ENV, dingtalk_client_id);
    }
    let dingtalk_client_secret = dingtalk_request.client_secret.trim();
    if !dingtalk_client_secret.is_empty() {
        ensure_env_value(
            &env_path,
            DINGTALK_CLIENT_SECRET_ENV,
            dingtalk_client_secret,
        )
        .map_err(internal_error_response)?;
        std::env::set_var(DINGTALK_CLIENT_SECRET_ENV, dingtalk_client_secret);
    }

    let poller_started = if matrix_enabled && matrix_token_configured {
        matrix::spawn_tool_poller(state.clone(), matrix_tool)
    } else {
        false
    };
    let message = if poller_started {
        "Chat configuration was saved and Matrix polling has started.".to_string()
    } else {
        "Chat configuration was saved.".to_string()
    };

    Ok(Json(chat_local_config_response(
        &state,
        &runtime_config,
        message,
        poller_started,
    )))
}

async fn save_matrix_local_config(
    State(state): State<AppState>,
    Json(request): Json<MatrixLocalConfigRequest>,
) -> Result<Json<MatrixLocalConfigResponse>, (StatusCode, String)> {
    let tool = matrix_tool_from_request(&request)?;
    let token = request.access_token.trim();
    let env_path = state.config.chat.env_path.clone();
    let mut token_configured = env_key_exists(&env_path, MATRIX_ACCESS_TOKEN_ENV)
        || env_var_exists(MATRIX_ACCESS_TOKEN_ENV);
    if request.enabled && token.is_empty() && !token_configured {
        return Err((
            StatusCode::BAD_REQUEST,
            "Matrix access token is required. If a token is already configured, leave this field blank to keep it.".to_string(),
        ));
    }

    upsert_chat_tool(&state.config.chat.config_path, tool.clone())
        .map_err(internal_error_response)?;
    if !token.is_empty() {
        ensure_env_value(&env_path, MATRIX_ACCESS_TOKEN_ENV, token)
            .map_err(internal_error_response)?;
        std::env::set_var(MATRIX_ACCESS_TOKEN_ENV, token);
        token_configured = true;
    }
    let poller_started = if request.enabled {
        matrix::spawn_tool_poller(state.clone(), tool)
    } else {
        false
    };

    Ok(Json(MatrixLocalConfigResponse {
        ok: true,
        message: if poller_started {
            "Matrix/Element local configuration was saved and polling has started.".to_string()
        } else if request.enabled {
            "Matrix/Element local configuration was saved. If polling is already running, the new configuration takes effect after the next controller restart.".to_string()
        } else {
            "Matrix/Element local configuration was saved and is currently disabled.".to_string()
        },
        tool_name: MATRIX_LOCAL_TOOL_NAME.to_string(),
        config_path: state.config.chat.config_path.display().to_string(),
        env_path: env_path.display().to_string(),
        token_configured,
        poller_started,
    }))
}

async fn list_adapters(State(state): State<AppState>) -> Json<ChatAdaptersResponse> {
    Json(ChatAdaptersResponse {
        tools: state
            .chat
            .tools
            .iter()
            .map(|tool| ChatAdapterInfo {
                name: tool.name.clone(),
                platform: tool.platform.clone(),
                enabled: tool.enabled,
                inbound: tool.inbound.clone(),
                local_friendly: tool.inbound.local_friendly(),
            })
            .collect(),
    })
}

async fn handle_dingtalk_stream(
    State(state): State<AppState>,
    Json(request): Json<DingTalkStreamRequest>,
) -> Result<Json<DingTalkStreamResponse>, (StatusCode, String)> {
    let topic = request.headers.get("topic").map(String::as_str);
    if topic != Some(DINGTALK_BOT_MESSAGE_TOPIC) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unsupported DingTalk stream topic: {:?}", topic),
        ));
    }

    let message_id = request
        .headers
        .get("messageId")
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let incoming = dingtalk_stream_to_chat_message(&state, &request.data)?;
    let result = queue_chat_message(&state, incoming).await;

    Ok(Json(DingTalkStreamResponse {
        code: 200,
        message: "OK".to_string(),
        headers: DingTalkStreamResponseHeaders {
            message_id,
            content_type: "application/json".to_string(),
        },
        data: "{\"response\":null}".to_string(),
        result,
    }))
}

fn dingtalk_stream_to_chat_message(
    state: &AppState,
    data: &str,
) -> Result<IncomingChatMessage, (StatusCode, String)> {
    let message = serde_json::from_str::<DingTalkBotMessage>(data)
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
    let (text, attachments) = dingtalk_content_to_parts(&message);
    let tool = state.chat.tools.iter().find(|tool| {
        tool.enabled
            && tool.platform == ChatPlatform::DingTalk
            && tool.inbound == ChatInboundMode::Stream
    });

    Ok(IncomingChatMessage {
        platform: "dingtalk".to_string(),
        conversation_id: message.conversation_id,
        sender: message
            .sender_nick
            .or(message.sender_staff_id)
            .or(message.sender_id)
            .unwrap_or_else(|| "unknown".to_string()),
        server_id: tool.and_then(|tool| tool.default_server_id.clone()),
        target_player: tool.and_then(|tool| tool.default_target_player.clone()),
        text,
        position: None,
        attachments,
    })
}

fn dingtalk_content_to_parts(message: &DingTalkBotMessage) -> (String, Vec<ChatAttachment>) {
    match message.msg_type.as_str() {
        "text" => (
            message
                .text
                .as_ref()
                .map(|text| text.content.trim().to_string())
                .unwrap_or_default(),
            Vec::new(),
        ),
        "picture" => (
            "收到一张图片".to_string(),
            dingtalk_content_attachment(
                ChatAttachmentKind::Image,
                message.content.as_ref(),
                None,
                None,
            )
            .into_iter()
            .collect(),
        ),
        "audio" => {
            let recognition = message
                .content
                .as_ref()
                .and_then(|content| content.get("recognition"))
                .and_then(Value::as_str)
                .unwrap_or("收到一段语音")
                .to_string();
            (
                recognition,
                dingtalk_content_attachment(
                    ChatAttachmentKind::Audio,
                    message.content.as_ref(),
                    None,
                    None,
                )
                .into_iter()
                .collect(),
            )
        }
        "video" => (
            "收到一个视频".to_string(),
            dingtalk_content_attachment(
                ChatAttachmentKind::Video,
                message.content.as_ref(),
                None,
                Some("video/mp4".to_string()),
            )
            .into_iter()
            .collect(),
        ),
        "file" => (
            "收到一个文件".to_string(),
            dingtalk_content_attachment(
                ChatAttachmentKind::File,
                message.content.as_ref(),
                message
                    .content
                    .as_ref()
                    .and_then(|content| content.get("fileName"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                None,
            )
            .into_iter()
            .collect(),
        ),
        "richText" => dingtalk_rich_text_parts(message.content.as_ref()),
        _ => ("收到暂不支持的钉钉消息类型".to_string(), Vec::new()),
    }
}

fn dingtalk_rich_text_parts(content: Option<&Value>) -> (String, Vec<ChatAttachment>) {
    let mut text_parts = Vec::new();
    let mut attachments = Vec::new();
    let Some(items) = content
        .and_then(|content| content.get("richText"))
        .and_then(Value::as_array)
    else {
        return ("收到富文本消息".to_string(), attachments);
    };

    for item in items {
        if let Some(text) = item.get("text").and_then(Value::as_str) {
            text_parts.push(text.trim().to_string());
        }

        if item.get("type").and_then(Value::as_str) == Some("picture") {
            attachments.extend(dingtalk_content_attachment(
                ChatAttachmentKind::Image,
                Some(item),
                None,
                None,
            ));
        }
    }

    let text = text_parts
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if text.is_empty() {
        ("收到富文本消息".to_string(), attachments)
    } else {
        (text, attachments)
    }
}

fn dingtalk_content_attachment(
    kind: ChatAttachmentKind,
    content: Option<&Value>,
    file_name: Option<String>,
    mime_type: Option<String>,
) -> Option<ChatAttachment> {
    let content = content?;
    let download_code = content
        .get("downloadCode")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if download_code.is_empty() {
        return None;
    }

    let picture_download_code = content
        .get("pictureDownloadCode")
        .and_then(Value::as_str)
        .map(str::to_string);

    Some(ChatAttachment {
        kind,
        source: ChatAttachmentSource::DingTalkDownloadCode {
            download_code,
            picture_download_code,
        },
        file_name,
        mime_type,
    })
}

fn normalized_enabled_tools(tools: &[ChatToolSelection]) -> Vec<ChatToolSelection> {
    [ChatToolSelection::Matrix, ChatToolSelection::DingTalk]
        .into_iter()
        .filter(|selection| tools.contains(selection))
        .collect()
}

fn chat_local_config_response(
    state: &AppState,
    config: &ChatRuntimeConfig,
    message: String,
    poller_started: bool,
) -> ChatLocalConfigResponse {
    let matrix = matrix_view_from_config(config, &state.config.chat.env_path);
    let dingtalk = dingtalk_view_from_config(config, &state.config.chat.env_path);
    let mut enabled_tools = Vec::new();
    if matrix.enabled {
        enabled_tools.push(ChatToolSelection::Matrix);
    }
    if dingtalk.enabled {
        enabled_tools.push(ChatToolSelection::DingTalk);
    }

    ChatLocalConfigResponse {
        ok: true,
        message,
        enabled_tools,
        config_path: state.config.chat.config_path.display().to_string(),
        env_path: state.config.chat.env_path.display().to_string(),
        matrix,
        dingtalk,
        poller_started,
    }
}

fn matrix_view_from_config(
    config: &ChatRuntimeConfig,
    env_path: &PathBuf,
) -> MatrixLocalConfigView {
    let tool = matrix_tool_from_config(config);
    let matrix = tool.and_then(|tool| tool.matrix.as_ref());
    let access_token_env = matrix
        .map(|matrix| matrix.access_token_env.as_str())
        .unwrap_or(MATRIX_ACCESS_TOKEN_ENV);

    MatrixLocalConfigView {
        enabled: tool.map(|tool| tool.enabled).unwrap_or(false),
        homeserver_url: matrix
            .map(|matrix| matrix.homeserver_url.clone())
            .unwrap_or_else(|| "https://matrix-client.matrix.org".to_string()),
        room_id: matrix.and_then(|matrix| matrix.room_id.clone()),
        allowed_sender: matrix
            .and_then(|matrix| matrix.allowed_senders.first().cloned())
            .unwrap_or_default(),
        allow_own_user_messages: matrix
            .and_then(|matrix| matrix.allow_own_user_messages)
            .unwrap_or(true),
        auto_join_invites: matrix
            .and_then(|matrix| matrix.auto_join_invites)
            .unwrap_or(true),
        default_server_id: tool.and_then(|tool| tool.default_server_id.clone()),
        default_target_player: tool.and_then(|tool| tool.default_target_player.clone()),
        poll_interval_seconds: matrix.and_then(|matrix| matrix.poll_interval_seconds),
        sync_timeout_seconds: matrix.and_then(|matrix| matrix.sync_timeout_seconds),
        token_configured: env_key_exists(env_path, access_token_env)
            || env_var_exists(access_token_env),
    }
}

fn dingtalk_view_from_config(
    config: &ChatRuntimeConfig,
    env_path: &PathBuf,
) -> DingTalkLocalConfigView {
    let tool = dingtalk_tool_from_config(config);
    let dingtalk = tool.and_then(|tool| tool.dingtalk.as_ref());
    let client_id_env = dingtalk
        .map(|dingtalk| dingtalk.client_id_env.as_str())
        .unwrap_or(DINGTALK_CLIENT_ID_ENV);
    let client_secret_env = dingtalk
        .map(|dingtalk| dingtalk.client_secret_env.as_str())
        .unwrap_or(DINGTALK_CLIENT_SECRET_ENV);

    DingTalkLocalConfigView {
        enabled: tool.map(|tool| tool.enabled).unwrap_or(false),
        robot_code: dingtalk.and_then(|dingtalk| dingtalk.robot_code.clone()),
        default_server_id: tool.and_then(|tool| tool.default_server_id.clone()),
        default_target_player: tool.and_then(|tool| tool.default_target_player.clone()),
        client_id_configured: env_key_exists(env_path, client_id_env)
            || env_var_exists(client_id_env),
        client_secret_configured: env_key_exists(env_path, client_secret_env)
            || env_var_exists(client_secret_env),
    }
}

fn matrix_request_from_config(
    config: &ChatRuntimeConfig,
    enabled: bool,
) -> MatrixLocalConfigRequest {
    let view = matrix_view_from_config(config, &PathBuf::new());
    MatrixLocalConfigRequest {
        enabled,
        homeserver_url: view.homeserver_url,
        access_token: String::new(),
        room_id: view.room_id,
        allowed_sender: view.allowed_sender,
        allow_own_user_messages: view.allow_own_user_messages,
        auto_join_invites: view.auto_join_invites,
        default_server_id: view.default_server_id,
        default_target_player: view.default_target_player,
        poll_interval_seconds: view.poll_interval_seconds,
        sync_timeout_seconds: view.sync_timeout_seconds,
    }
}

fn dingtalk_request_from_config(
    config: &ChatRuntimeConfig,
    enabled: bool,
) -> DingTalkLocalConfigRequest {
    let view = dingtalk_view_from_config(config, &PathBuf::new());
    DingTalkLocalConfigRequest {
        enabled,
        client_id: String::new(),
        client_secret: String::new(),
        robot_code: view.robot_code,
        default_server_id: view.default_server_id,
        default_target_player: view.default_target_player,
    }
}

fn matrix_tool_from_config(config: &ChatRuntimeConfig) -> Option<&ChatToolConfig> {
    config
        .tools
        .iter()
        .find(|tool| tool.name == MATRIX_LOCAL_TOOL_NAME)
        .or_else(|| {
            config
                .tools
                .iter()
                .find(|tool| tool.platform == ChatPlatform::Matrix)
        })
}

fn dingtalk_tool_from_config(config: &ChatRuntimeConfig) -> Option<&ChatToolConfig> {
    config
        .tools
        .iter()
        .find(|tool| tool.name == DINGTALK_LOCAL_TOOL_NAME)
        .or_else(|| {
            config
                .tools
                .iter()
                .find(|tool| tool.platform == ChatPlatform::DingTalk)
        })
}

fn matrix_tool_from_request(
    request: &MatrixLocalConfigRequest,
) -> Result<ChatToolConfig, (StatusCode, String)> {
    let homeserver_url = request.homeserver_url.trim();
    let allowed_sender = request.allowed_sender.trim();
    let access_token = request.access_token.trim();
    if request.enabled && (homeserver_url.is_empty() || allowed_sender.is_empty()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "homeserver_url and allowed_sender are required.".to_string(),
        ));
    }
    if contains_line_break(homeserver_url)
        || contains_line_break(allowed_sender)
        || contains_line_break(access_token)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Matrix configuration values cannot contain line breaks.".to_string(),
        ));
    }

    Ok(ChatToolConfig {
        name: MATRIX_LOCAL_TOOL_NAME.to_string(),
        platform: ChatPlatform::Matrix,
        enabled: request.enabled,
        inbound: ChatInboundMode::Polling,
        default_server_id: normalize_optional_string(request.default_server_id.as_deref()),
        default_target_player: normalize_optional_string(request.default_target_player.as_deref()),
        dingtalk: None,
        matrix: Some(MatrixChatConfig {
            homeserver_url: if homeserver_url.is_empty() {
                "https://matrix-client.matrix.org".to_string()
            } else {
                homeserver_url.to_string()
            },
            access_token_env: MATRIX_ACCESS_TOKEN_ENV.to_string(),
            room_id: normalize_optional_string(request.room_id.as_deref()),
            allowed_senders: normalize_optional_string(Some(allowed_sender))
                .into_iter()
                .collect(),
            allow_own_user_messages: Some(request.allow_own_user_messages),
            auto_join_invites: Some(request.auto_join_invites),
            poll_interval_seconds: request.poll_interval_seconds,
            sync_timeout_seconds: request.sync_timeout_seconds,
        }),
    })
}

fn dingtalk_tool_from_request(
    request: &DingTalkLocalConfigRequest,
) -> Result<ChatToolConfig, (StatusCode, String)> {
    let client_id = request.client_id.trim();
    let client_secret = request.client_secret.trim();
    let robot_code = normalize_optional_string(request.robot_code.as_deref());
    if contains_line_break(client_id)
        || contains_line_break(client_secret)
        || robot_code
            .as_deref()
            .map(contains_line_break)
            .unwrap_or(false)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "DingTalk configuration values cannot contain line breaks.".to_string(),
        ));
    }

    Ok(ChatToolConfig {
        name: DINGTALK_LOCAL_TOOL_NAME.to_string(),
        platform: ChatPlatform::DingTalk,
        enabled: request.enabled,
        inbound: ChatInboundMode::Stream,
        default_server_id: normalize_optional_string(request.default_server_id.as_deref()),
        default_target_player: normalize_optional_string(request.default_target_player.as_deref()),
        dingtalk: Some(DingTalkChatConfig {
            client_id_env: DINGTALK_CLIENT_ID_ENV.to_string(),
            client_secret_env: DINGTALK_CLIENT_SECRET_ENV.to_string(),
            robot_code,
        }),
        matrix: None,
    })
}

fn upsert_chat_tool(
    path: &PathBuf,
    tool: ChatToolConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut config = config::load_chat_runtime_config(path)?;
    upsert_chat_tool_in_config(&mut config, tool);
    config::write_chat_runtime_config(path, &config)
}

fn upsert_chat_tool_in_config(config: &mut ChatRuntimeConfig, tool: ChatToolConfig) {
    if let Some(existing) = config
        .tools
        .iter_mut()
        .find(|existing| existing.name == tool.name)
    {
        *existing = tool;
    } else {
        config.tools.push(tool);
    }
}

fn env_key_exists(path: &PathBuf, key: &str) -> bool {
    std::fs::read_to_string(path)
        .map(|source| {
            source.lines().any(|line| {
                let line = line.trim_start();
                line.starts_with(&format!("{key}="))
            })
        })
        .unwrap_or(false)
}

fn env_var_exists(key: &str) -> bool {
    std::env::var(key)
        .map(|value| !value.trim().is_empty())
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
        if line.trim_start().starts_with(&format!("{key}=")) {
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

fn normalize_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn contains_line_break(value: &str) -> bool {
    value.contains('\n') || value.contains('\r')
}

fn default_true() -> bool {
    true
}

fn internal_error_response(
    error: Box<dyn std::error::Error + Send + Sync>,
) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bot_message(msg_type: &str, body: Value) -> DingTalkBotMessage {
        serde_json::from_value(json_with_base(msg_type, body)).unwrap()
    }

    fn json_with_base(msg_type: &str, extra: Value) -> Value {
        let mut value = serde_json::json!({
            "conversationId": "cid-1",
            "senderNick": "张三",
            "senderStaffId": "001",
            "senderId": "sender-1",
            "msgtype": msg_type
        });
        let map = value.as_object_mut().unwrap();
        for (key, value) in extra.as_object().unwrap() {
            map.insert(key.to_string(), value.clone());
        }
        value
    }

    #[test]
    fn parses_dingtalk_text_message() {
        let message = bot_message(
            "text",
            serde_json::json!({
                "text": {
                    "content": " 帮我盖一个木屋 "
                }
            }),
        );

        let (text, attachments) = dingtalk_content_to_parts(&message);

        assert_eq!(text, "帮我盖一个木屋");
        assert!(attachments.is_empty());
    }

    #[test]
    fn parses_dingtalk_picture_message_as_image_attachment() {
        let message = bot_message(
            "picture",
            serde_json::json!({
                "content": {
                    "pictureDownloadCode": "picture-code",
                    "downloadCode": "download-code"
                }
            }),
        );

        let (text, attachments) = dingtalk_content_to_parts(&message);

        assert_eq!(text, "收到一张图片");
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].kind, ChatAttachmentKind::Image);
        assert!(matches!(
            attachments[0].source,
            ChatAttachmentSource::DingTalkDownloadCode {
                ref download_code,
                ..
            } if download_code == "download-code"
        ));
    }

    #[test]
    fn parses_dingtalk_rich_text_with_text_and_image() {
        let message = bot_message(
            "richText",
            serde_json::json!({
                "content": {
                    "richText": [
                        { "text": "照这个做" },
                        {
                            "type": "picture",
                            "pictureDownloadCode": "picture-code",
                            "downloadCode": "download-code"
                        }
                    ]
                }
            }),
        );

        let (text, attachments) = dingtalk_content_to_parts(&message);

        assert_eq!(text, "照这个做");
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].kind, ChatAttachmentKind::Image);
    }

    #[test]
    fn matrix_local_config_request_writes_chat_tool_without_secret() {
        let path = unique_temp_path("chat.yaml");
        let request = MatrixLocalConfigRequest {
            enabled: true,
            homeserver_url: " https://matrix-client.matrix.org/ ".to_string(),
            access_token: "secret-token".to_string(),
            room_id: None,
            allowed_sender: " @enochzzg:matrix.org ".to_string(),
            allow_own_user_messages: true,
            auto_join_invites: true,
            default_server_id: Some("local-java".to_string()),
            default_target_player: Some("Charles".to_string()),
            poll_interval_seconds: Some(2),
            sync_timeout_seconds: Some(30),
        };

        let tool = matrix_tool_from_request(&request).unwrap();
        upsert_chat_tool(&path, tool).unwrap();

        let source = std::fs::read_to_string(&path).unwrap();
        assert!(source.contains("element-local"));
        assert!(source.contains("https://matrix-client.matrix.org/"));
        assert!(source.contains("@enochzzg:matrix.org"));
        assert!(source.contains("MATRIX_ACCESS_TOKEN"));
        assert!(!source.contains("secret-token"));
    }

    #[test]
    fn env_value_is_upserted_without_leaking_duplicates() {
        let path = unique_temp_path(".env");
        std::fs::write(&path, "OTHER=value\nMATRIX_ACCESS_TOKEN=old\n").unwrap();

        ensure_env_value(&path, MATRIX_ACCESS_TOKEN_ENV, "new-token").unwrap();

        let source = std::fs::read_to_string(&path).unwrap();
        assert!(source.contains("OTHER=value"));
        assert!(source.contains("MATRIX_ACCESS_TOKEN=new-token"));
        assert_eq!(source.matches("MATRIX_ACCESS_TOKEN=").count(), 1);
    }

    fn unique_temp_path(file_name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("blockwright-chat-test-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(file_name)
    }
}
