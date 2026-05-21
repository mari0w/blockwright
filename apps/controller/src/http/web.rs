use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Path as AxumPath, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};

use crate::{
    domain::types::{
        BuildStatus, ChatAttachment, ChatAttachmentKind, ChatAttachmentSource, GameAction,
    },
    http::robot::queue_chat_message,
    https,
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
        .route("/web/https-ca.crt", get(download_https_ca_certificate))
        .route(
            "/web/blockwright-local-root-ca.cer",
            get(download_https_ca_certificate),
        )
        .route("/web/message", post(handle_web_message))
        .route("/web/translate", post(handle_web_translate))
        .route("/web/jobs/{job_id}/status", get(web_job_status))
}

async fn web_chat_page() -> Html<&'static str> {
    Html(WEB_CHAT_HTML)
}

async fn download_https_ca_certificate(
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, String)> {
    let path = https::ca_certificate_path(&state.config.storage.data_dir);
    let source = tokio::fs::read_to_string(&path).await.map_err(|error| {
        (
            StatusCode::NOT_FOUND,
            format!("HTTPS 根证书还没有生成，请先重启 controller：{error}"),
        )
    })?;
    let certificate = https::certificate_der_from_pem(&source).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("HTTPS 根证书格式不正确，请重启 controller 后重试：{error}"),
        )
    })?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-x509-ca-cert"),
            (
                header::CONTENT_DISPOSITION,
                "inline; filename=\"Blockwright-Local-Root-CA.cer\"",
            ),
        ],
        certificate,
    )
        .into_response())
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
    let build = if should_read_build_record(queue_status.as_ref()) {
        state.builds.get(&job_id).await
    } else {
        None
    };
    if queue_status.is_none() && build.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(web_job_status_response(queue_status, build)))
}

fn should_read_build_record(queue_status: Option<&JobQueueStatus>) -> bool {
    queue_status
        .and_then(|status| status.job.as_ref())
        .map(|job| {
            job.actions
                .iter()
                .any(|action| matches!(action, GameAction::PlaceBlocks { .. }))
        })
        .unwrap_or(true)
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
        .or_else(|| queue_status.as_ref().and_then(|item| item.message.clone()))
        .and_then(clean_user_status_detail);

    match build.as_ref().map(|item| &item.status) {
        Some(BuildStatus::Succeeded) => WebJobStatusResponse {
            phase: "succeeded".to_string(),
            message: status_message(JobQueuePhase::Succeeded),
            summary,
            build_status,
            result_message,
        },
        Some(BuildStatus::Failed) => WebJobStatusResponse {
            phase: "failed".to_string(),
            message: status_message(JobQueuePhase::Failed),
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
                message: status_message(if pending {
                    JobQueuePhase::Pending
                } else {
                    JobQueuePhase::Claimed
                }),
                summary,
                build_status,
                result_message,
            }
        }
        None => match queue_status.as_ref().map(|item| &item.phase) {
            Some(JobQueuePhase::Pending) => WebJobStatusResponse {
                phase: "queued".to_string(),
                message: status_message(JobQueuePhase::Pending),
                summary,
                build_status,
                result_message,
            },
            Some(JobQueuePhase::Claimed) => WebJobStatusResponse {
                phase: "running".to_string(),
                message: status_message(JobQueuePhase::Claimed),
                summary,
                build_status,
                result_message,
            },
            Some(JobQueuePhase::Succeeded) => WebJobStatusResponse {
                phase: "succeeded".to_string(),
                message: status_message(JobQueuePhase::Succeeded),
                summary,
                build_status,
                result_message,
            },
            Some(JobQueuePhase::Failed) => WebJobStatusResponse {
                phase: "failed".to_string(),
                message: status_message(JobQueuePhase::Failed),
                summary,
                build_status,
                result_message,
            },
            None => WebJobStatusResponse {
                phase: "unknown".to_string(),
                message: "暂时查不到这次操作的状态。".to_string(),
                summary,
                build_status,
                result_message,
            },
        },
    }
}

fn status_message(phase: JobQueuePhase) -> String {
    let message = match phase {
        JobQueuePhase::Pending => "我已经准备好操作，正在等 Minecraft 接手。",
        JobQueuePhase::Claimed => "Minecraft 正在处理这次操作。",
        JobQueuePhase::Succeeded => "Minecraft 已经完成这次操作。",
        JobQueuePhase::Failed => "Minecraft 这次没有完成执行，请查看具体原因。",
    };
    message.to_string()
}

fn clean_user_status_detail(message: String) -> Option<String> {
    let value = message.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("ok") || value == "成功" {
        return None;
    }
    Some(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{BlockOrigin, BlueprintBlock, GameJob};

    #[test]
    fn non_build_queue_jobs_do_not_read_stale_build_records() {
        let status = JobQueueStatus {
            phase: JobQueuePhase::Succeeded,
            job: Some(GameJob {
                id: "hm-job-1".to_string(),
                server_id: "hmcl-lan".to_string(),
                target_player: Some("Steve".to_string()),
                summary: "发放红砖".to_string(),
                actions: vec![GameAction::GiveItem {
                    player: Some("Steve".to_string()),
                    item: "minecraft:red_concrete".to_string(),
                    count: 64,
                }],
            }),
            message: Some("ok".to_string()),
            result: None,
        };

        assert!(!should_read_build_record(Some(&status)));
    }

    #[test]
    fn build_queue_jobs_still_read_build_records() {
        let status = JobQueueStatus {
            phase: JobQueuePhase::Claimed,
            job: Some(GameJob {
                id: "hm-job-1".to_string(),
                server_id: "hmcl-lan".to_string(),
                target_player: Some("Steve".to_string()),
                summary: "建造".to_string(),
                actions: vec![GameAction::PlaceBlocks {
                    blueprint_id: Some("test".to_string()),
                    origin: BlockOrigin {
                        world: Some("minecraft:overworld".to_string()),
                        x: 0,
                        y: 64,
                        z: 0,
                    },
                    blocks: vec![BlueprintBlock {
                        x: 0,
                        y: 0,
                        z: 0,
                        material: "minecraft:stone".to_string(),
                    }],
                    clear_existing: false,
                }],
            }),
            message: None,
            result: None,
        };

        assert!(should_read_build_record(Some(&status)));
    }

    #[test]
    fn empty_web_text_with_image_defaults_to_recreation_request() {
        let text = normalized_web_text("   ", true);

        assert!(text.contains("复刻"));
        assert!(text.contains("外形"));
        assert!(text.contains("比例"));
        assert!(!text.contains("参考这张图片帮我设计"));
    }

    #[test]
    fn non_empty_web_text_is_preserved() {
        assert_eq!(normalized_web_text("  照这个做  ", true), "照这个做");
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
        "按这张图片复刻一个 Minecraft 建筑，尽量保持外形、比例、材质分区和关键细节。".to_string()
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
        if lower.ends_with(".heic") {
            return ".heic";
        }
        if lower.ends_with(".heif") {
            return ".heif";
        }
    }

    match mime_type.unwrap_or_default() {
        "image/jpeg" => ".jpg",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        "image/heic" => ".heic",
        "image/heif" => ".heif",
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
