use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};

use crate::{
    domain::types::{BuildStatus, ChatAttachment, ChatAttachmentKind, ChatAttachmentSource},
    http::robot::queue_chat_message,
    services::chat::IncomingChatMessage,
    services::job_queue::{JobQueuePhase, JobQueueStatus},
    state::AppState,
};

const WEB_CHAT_HTML: &str = include_str!("web_chat.html");
const MAX_IMAGES_PER_MESSAGE: usize = 4;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TRANSLATION_CHARS: usize = 4000;

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

#[derive(Debug, Deserialize)]
struct WebTranslateRequest {
    text: String,
    target_language: String,
}

#[derive(Debug, Serialize)]
struct WebTranslateResponse {
    translated_text: String,
    target_language: String,
    translated: bool,
}

#[derive(Debug, Serialize)]
struct WebJobStatusResponse {
    phase: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_status: Option<BuildStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_message: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(web_chat_page))
        .route("/web", get(web_chat_page))
        .route("/web/message", post(handle_web_message))
        .route("/web/translate", post(handle_web_translate))
        .route("/web/jobs/{job_id}/status", get(web_job_status))
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

async fn handle_web_translate(
    State(state): State<AppState>,
    Json(request): Json<WebTranslateRequest>,
) -> Result<Json<WebTranslateResponse>, (StatusCode, String)> {
    let text = request.text.trim();
    if text.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "没有识别到可翻译的语音文字。".to_string(),
        ));
    }
    if text.chars().count() > MAX_TRANSLATION_CHARS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("语音文字太长，最多支持 {MAX_TRANSLATION_CHARS} 个字符。"),
        ));
    }

    let target_language = normalize_translation_language(&request.target_language)?;
    if target_language.code == "original" || !state.codex.enabled() {
        return Ok(Json(WebTranslateResponse {
            translated_text: text.to_string(),
            target_language: target_language.code.to_string(),
            translated: false,
        }));
    }

    let prompt = build_translation_prompt(text, target_language.label);
    let translated = state
        .codex
        .ask(&prompt)
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "web voice translation failed");
            (
                StatusCode::BAD_GATEWAY,
                "Codex 翻译失败，请稍后重试。".to_string(),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                "Codex 未返回翻译结果。".to_string(),
            )
        })?;
    let translated_text = clean_translation_output(&translated);
    if translated_text.is_empty() {
        return Err((
            StatusCode::BAD_GATEWAY,
            "Codex 返回的翻译结果为空。".to_string(),
        ));
    }

    Ok(Json(WebTranslateResponse {
        translated_text,
        target_language: target_language.code.to_string(),
        translated: true,
    }))
}

async fn web_job_status(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<WebJobStatusResponse>, StatusCode> {
    let queue_status = state.jobs.status(&job_id).await;
    let build = state.builds.get(&job_id).await;
    if queue_status.is_none() && build.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(web_job_status_response(queue_status, build)))
}

fn web_job_status_response(
    queue_status: Option<JobQueueStatus>,
    build: Option<crate::domain::types::BuildRecord>,
) -> WebJobStatusResponse {
    let summary = build.as_ref().map(|item| item.summary.clone()).or_else(|| {
        queue_status
            .as_ref()
            .and_then(|item| item.job.as_ref().map(|job| job.summary.clone()))
    });
    let build_status = build.as_ref().map(|item| item.status.clone());
    let result_message = build
        .as_ref()
        .and_then(|item| item.message.clone())
        .or_else(|| queue_status.as_ref().and_then(|item| item.message.clone()));

    match build.as_ref().map(|item| &item.status) {
        Some(BuildStatus::Succeeded) => WebJobStatusResponse {
            phase: "succeeded".to_string(),
            message: "Minecraft 执行完成，校验报告已回写。".to_string(),
            summary,
            build_status,
            result_message,
        },
        Some(BuildStatus::Failed) => WebJobStatusResponse {
            phase: "failed".to_string(),
            message: "Minecraft 执行失败，已回写失败原因。".to_string(),
            summary,
            build_status,
            result_message,
        },
        Some(BuildStatus::Planned) => {
            let pending = matches!(
                queue_status.as_ref().map(|item| &item.phase),
                Some(JobQueuePhase::Pending)
            );
            WebJobStatusResponse {
                phase: if pending { "queued" } else { "running" }.to_string(),
                message: if pending {
                    "任务还在队列里，等待 Minecraft 执行端领取。".to_string()
                } else {
                    "Minecraft 执行端已领取任务，正在放置方块并生成校验报告。".to_string()
                },
                summary,
                build_status,
                result_message,
            }
        }
        None => match queue_status.as_ref().map(|item| &item.phase) {
            Some(JobQueuePhase::Pending) => WebJobStatusResponse {
                phase: "queued".to_string(),
                message: "任务还在队列里，等待 Minecraft 执行端领取。".to_string(),
                summary,
                build_status,
                result_message,
            },
            Some(JobQueuePhase::Claimed) => WebJobStatusResponse {
                phase: "running".to_string(),
                message: "Minecraft 执行端已领取任务，正在扫描或执行。".to_string(),
                summary,
                build_status,
                result_message,
            },
            Some(JobQueuePhase::Succeeded) => WebJobStatusResponse {
                phase: "succeeded".to_string(),
                message: "Minecraft 执行端已回写成功。".to_string(),
                summary,
                build_status,
                result_message,
            },
            Some(JobQueuePhase::Failed) => WebJobStatusResponse {
                phase: "failed".to_string(),
                message: "Minecraft 执行端已回写失败。".to_string(),
                summary,
                build_status,
                result_message,
            },
            None => WebJobStatusResponse {
                phase: "unknown".to_string(),
                message: "没有找到这个任务的实时状态。".to_string(),
                summary,
                build_status,
                result_message,
            },
        },
    }
}

struct TranslationLanguage {
    code: &'static str,
    label: &'static str,
}

fn normalize_translation_language(
    value: &str,
) -> Result<TranslationLanguage, (StatusCode, String)> {
    match value.trim() {
        "" | "original" => Ok(TranslationLanguage {
            code: "original",
            label: "原文",
        }),
        "zh" | "zh-CN" | "中文" => Ok(TranslationLanguage {
            code: "zh-CN",
            label: "中文",
        }),
        "en" | "en-US" | "English" => Ok(TranslationLanguage {
            code: "en",
            label: "英文",
        }),
        "pt" | "pt-BR" | "Português" => Ok(TranslationLanguage {
            code: "pt-BR",
            label: "巴西葡萄牙语",
        }),
        "hi" | "hi-IN" | "Hindi" => Ok(TranslationLanguage {
            code: "hi-IN",
            label: "印地语",
        }),
        "es" | "es-ES" | "Spanish" => Ok(TranslationLanguage {
            code: "es",
            label: "西班牙语",
        }),
        _ => Err((
            StatusCode::BAD_REQUEST,
            "不支持这个翻译目标语言。".to_string(),
        )),
    }
}

fn build_translation_prompt(text: &str, target_language: &str) -> String {
    format!(
        r#"你是 Blockwright 网页语音输入的翻译器。
把 <speech_text> 里的语音识别文字翻译成{target_language}。
只输出翻译后的文本，不要解释，不要加引号，不要 Markdown。
如果原文已经是目标语言，只做必要的错别字和口语整理。

<speech_text>
{text}
</speech_text>"#
    )
}

fn clean_translation_output(value: &str) -> String {
    let mut text = value.trim();
    if let Some(stripped) = text.strip_prefix("```") {
        text = stripped.trim_start();
        if let Some((_, rest)) = text.split_once('\n') {
            text = rest;
        }
        if let Some((body, _)) = text.rsplit_once("```") {
            text = body.trim();
        }
    }
    text.trim_matches(|ch| matches!(ch, '"' | '\'' | '“' | '”' | '‘' | '’'))
        .trim()
        .to_string()
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
