use std::collections::HashMap;

use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    config::{ChatInboundMode, ChatPlatform},
    domain::types::{ChatAttachment, ChatAttachmentKind, ChatAttachmentSource},
    http::robot::{queue_chat_message, RobotMessageResponse},
    services::chat::IncomingChatMessage,
    state::AppState,
};

const DINGTALK_BOT_MESSAGE_TOPIC: &str = "/v1.0/im/bot/messages/get";

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
            "/chat/dingtalk/stream",
            axum::routing::post(handle_dingtalk_stream),
        )
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
}
