use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::State,
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};

use crate::{
    domain::types::{ChatAttachment, ChatAttachmentKind, ChatAttachmentSource},
    http::robot::queue_chat_message,
    services::chat::IncomingChatMessage,
    state::AppState,
};

const WEB_CHAT_HTML: &str = include_str!("web_chat.html");
const MAX_IMAGES_PER_MESSAGE: usize = 4;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct WebChatRequest {
    username: String,
    text: String,
    #[serde(default)]
    server_id: Option<String>,
    #[serde(default)]
    target_player: Option<String>,
    #[serde(default)]
    images: Vec<WebImageUpload>,
}

#[derive(Debug, Deserialize)]
struct WebImageUpload {
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    data_url: String,
}

#[derive(Debug, Serialize)]
struct WebChatResponse {
    reply: String,
    queued_job_id: Option<String>,
    queued_summary: Option<String>,
    attachment_count: usize,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(web_chat_page))
        .route("/web", get(web_chat_page))
        .route("/web/message", post(handle_web_message))
}

async fn web_chat_page() -> Html<&'static str> {
    Html(WEB_CHAT_HTML)
}

async fn handle_web_message(
    State(state): State<AppState>,
    Json(request): Json<WebChatRequest>,
) -> Result<Json<WebChatResponse>, (StatusCode, String)> {
    let username = request.username.trim();
    if username.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "请先填写用户名。".to_string()));
    }
    if request.images.len() > MAX_IMAGES_PER_MESSAGE {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("一次最多上传 {MAX_IMAGES_PER_MESSAGE} 张图片。"),
        ));
    }

    let attachments = save_uploaded_images(&state, username, &request.images).await?;
    let text = normalized_web_text(&request.text, !attachments.is_empty());
    let response = queue_chat_message(
        &state,
        IncomingChatMessage {
            platform: "web".to_string(),
            conversation_id: format!("web:{}", safe_segment(username)),
            sender: username.to_string(),
            server_id: request.server_id.filter(|value| !value.trim().is_empty()),
            target_player: request
                .target_player
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some(username.to_string())),
            text,
            position: None,
            attachments,
        },
    )
    .await;

    let queued_job_id = response.queued_job.as_ref().map(|job| job.id.clone());
    let queued_summary = response.queued_job.as_ref().map(|job| job.summary.clone());
    Ok(Json(WebChatResponse {
        reply: response.reply,
        queued_job_id,
        queued_summary,
        attachment_count: request.images.len(),
    }))
}

async fn save_uploaded_images(
    state: &AppState,
    username: &str,
    images: &[WebImageUpload],
) -> Result<Vec<ChatAttachment>, (StatusCode, String)> {
    let mut attachments = Vec::new();
    if images.is_empty() {
        return Ok(attachments);
    }

    let upload_dir = absolute_data_dir(&state.config.storage.data_dir)
        .join("uploads")
        .join("web")
        .join(safe_segment(username));
    tokio::fs::create_dir_all(&upload_dir).await.map_err(|error| {
        tracing::error!(error = %error, path = %upload_dir.display(), "failed to create web upload dir");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "图片上传目录创建失败。".to_string(),
        )
    })?;

    for (index, image) in images.iter().enumerate() {
        let decoded = decode_image_data_url(&image.data_url)?;
        if decoded.bytes.len() > MAX_IMAGE_BYTES {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("单张图片不能超过 {}MB。", MAX_IMAGE_BYTES / 1024 / 1024),
            ));
        }
        let mime_type = image
            .mime_type
            .clone()
            .filter(|value| value.starts_with("image/"))
            .or_else(|| Some(decoded.mime_type.clone()));
        let extension = upload_extension(image.file_name.as_deref(), mime_type.as_deref());
        let file_name = format!("{}-{}{}", timestamp_millis(), index + 1, extension);
        let path = upload_dir.join(file_name);
        tokio::fs::write(&path, &decoded.bytes)
            .await
            .map_err(|error| {
                tracing::error!(error = %error, path = %path.display(), "failed to save web upload");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "图片保存失败。".to_string(),
                )
            })?;

        attachments.push(ChatAttachment {
            kind: ChatAttachmentKind::Image,
            source: ChatAttachmentSource::LocalPath {
                path: path.to_string_lossy().to_string(),
            },
            file_name: image.file_name.as_deref().map(safe_file_name),
            mime_type,
        });
    }

    Ok(attachments)
}

struct DecodedImage {
    mime_type: String,
    bytes: Vec<u8>,
}

fn decode_image_data_url(data_url: &str) -> Result<DecodedImage, (StatusCode, String)> {
    let Some((header, payload)) = data_url.split_once(',') else {
        return Err((StatusCode::BAD_REQUEST, "图片数据格式不正确。".to_string()));
    };
    let Some(meta) = header.strip_prefix("data:") else {
        return Err((StatusCode::BAD_REQUEST, "图片数据格式不正确。".to_string()));
    };
    let Some((mime_type, encoding)) = meta.split_once(';') else {
        return Err((StatusCode::BAD_REQUEST, "图片数据格式不正确。".to_string()));
    };
    if !mime_type.starts_with("image/") || encoding != "base64" {
        return Err((
            StatusCode::BAD_REQUEST,
            "只支持 base64 图片上传。".to_string(),
        ));
    }

    let bytes = general_purpose::STANDARD.decode(payload).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "图片 base64 内容无法解析。".to_string(),
        )
    })?;

    Ok(DecodedImage {
        mime_type: mime_type.to_string(),
        bytes,
    })
}

fn normalized_web_text(text: &str, has_image: bool) -> String {
    let text = text.trim();
    if !text.is_empty() {
        return text.to_string();
    }
    if has_image {
        "参考这张图片帮我设计一个 Minecraft 建筑。".to_string()
    } else {
        "你好".to_string()
    }
}

fn absolute_data_dir(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn upload_extension(file_name: Option<&str>, mime_type: Option<&str>) -> &'static str {
    if let Some(file_name) = file_name {
        let lower = file_name.to_ascii_lowercase();
        if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            return ".jpg";
        }
        if lower.ends_with(".webp") {
            return ".webp";
        }
        if lower.ends_with(".gif") {
            return ".gif";
        }
    }

    match mime_type.unwrap_or_default() {
        "image/jpeg" => ".jpg",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        _ => ".png",
    }
}

fn safe_segment(value: &str) -> String {
    let segment = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .take(48)
        .collect::<String>();
    if segment.is_empty() {
        "user".to_string()
    } else {
        segment
    }
}

fn safe_file_name(value: &str) -> String {
    let name = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(*ch, '-' | '_' | '.'))
        .take(96)
        .collect::<String>();
    if name.is_empty() {
        "image".to_string()
    } else {
        name
    }
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}
