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
            format!("The HTTPS root certificate has not been generated yet. Restart the controller and try again: {error}"),
        )
    })?;
    let certificate = https::certificate_der_from_pem(&source).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("The HTTPS root certificate is invalid. Restart the controller and try again: {error}"),
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
        return Err((
            StatusCode::BAD_REQUEST,
            "Enter your username first.".to_string(),
        ));
    }
    if request.images.len() > MAX_IMAGES_PER_MESSAGE {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("You can upload up to {MAX_IMAGES_PER_MESSAGE} images at once."),
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
            "No translatable voice text was detected.".to_string(),
        ));
    }
    if text.chars().count() > MAX_TRANSLATION_CHARS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Voice text is too long. The limit is {MAX_TRANSLATION_CHARS} characters."),
        ));
    }

    let target_language = normalize_translation_language(&request.target_language)?;
    if target_language.code == "original" || !state.llm.enabled() {
        return Ok(Json(WebTranslateResponse {
            translated_text: text.to_string(),
            target_language: target_language.code.to_string(),
            translated: false,
        }));
    }

    let prompt = build_translation_prompt(text, target_language.label);
    let translated = state
        .llm
        .ask(&prompt)
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, "web voice translation failed");
            (
                StatusCode::BAD_GATEWAY,
                "AI translation failed. Try again later.".to_string(),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                "AI did not return a translation.".to_string(),
            )
        })?;
    let translated_text = clean_translation_output(&translated);
    if translated_text.is_empty() {
        return Err((
            StatusCode::BAD_GATEWAY,
            "AI returned an empty translation.".to_string(),
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
                message: "This operation status is not available yet.".to_string(),
                summary,
                build_status,
                result_message,
            },
        },
    }
}

fn status_message(phase: JobQueuePhase) -> String {
    let message = match phase {
        JobQueuePhase::Pending => "The operation is ready and waiting for Minecraft.",
        JobQueuePhase::Claimed => "Minecraft is processing this operation.",
        JobQueuePhase::Succeeded => "Minecraft completed this operation.",
        JobQueuePhase::Failed => "Minecraft did not complete this operation. Check the details.",
    };
    message.to_string()
}

fn clean_user_status_detail(message: String) -> Option<String> {
    let value = message.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("ok") || value == "成功" {
        return None;
    }
    if let Some(player) = value
        .strip_prefix("找不到玩家：")
        .or_else(|| value.strip_prefix("找不到玩家:"))
    {
        return Some(format!("Player not found: {}", player.trim()));
    }
    Some(value.to_string())
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
            label: "original text",
        }),
        "zh" | "zh-CN" | "中文" => Ok(TranslationLanguage {
            code: "zh-CN",
            label: "Chinese",
        }),
        "en" | "en-US" | "English" => Ok(TranslationLanguage {
            code: "en",
            label: "English",
        }),
        "pt" | "pt-BR" | "Português" => Ok(TranslationLanguage {
            code: "pt-BR",
            label: "Brazilian Portuguese",
        }),
        "hi" | "hi-IN" | "Hindi" => Ok(TranslationLanguage {
            code: "hi-IN",
            label: "Hindi",
        }),
        "es" | "es-ES" | "Spanish" => Ok(TranslationLanguage {
            code: "es",
            label: "Spanish",
        }),
        _ => Err((
            StatusCode::BAD_REQUEST,
            "This translation target language is not supported.".to_string(),
        )),
    }
}

fn build_translation_prompt(text: &str, target_language: &str) -> String {
    format!(
        r#"You are the translator for Blockwright web voice input.
Translate the speech recognition text in <speech_text> into {target_language}.
Only output the translated text. Do not explain, add quotes, or use Markdown.
If the original text is already in the target language, only clean up obvious typos and spoken-language roughness.

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
            "Failed to create the image upload directory.".to_string(),
        )
    })?;

    for (index, image) in images.iter().enumerate() {
        let decoded = decode_image_data_url(&image.data_url)?;
        if decoded.bytes.len() > MAX_IMAGE_BYTES {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Each image must be {}MB or smaller.",
                    MAX_IMAGE_BYTES / 1024 / 1024
                ),
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
                    "Failed to save the image.".to_string(),
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
        return Err((
            StatusCode::BAD_REQUEST,
            "The image data format is invalid.".to_string(),
        ));
    };
    let Some(meta) = header.strip_prefix("data:") else {
        return Err((
            StatusCode::BAD_REQUEST,
            "The image data format is invalid.".to_string(),
        ));
    };
    let Some((mime_type, encoding)) = meta.split_once(';') else {
        return Err((
            StatusCode::BAD_REQUEST,
            "The image data format is invalid.".to_string(),
        ));
    };
    if !mime_type.starts_with("image/") || encoding != "base64" {
        return Err((
            StatusCode::BAD_REQUEST,
            "Only base64 image uploads are supported.".to_string(),
        ));
    }

    let bytes = general_purpose::STANDARD.decode(payload).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "The image base64 content could not be decoded.".to_string(),
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
        "Recreate this image as a Minecraft build. Keep the shape, proportions, material areas, and key details as much as possible.".to_string()
    } else {
        "Hello".to_string()
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

        assert!(text.contains("Recreate this image"));
        assert!(text.contains("shape"));
        assert!(text.contains("proportions"));
        assert!(!text.contains("参考这张图片帮我设计"));
    }

    #[test]
    fn non_empty_web_text_is_preserved() {
        assert_eq!(normalized_web_text("  照这个做  ", true), "照这个做");
    }
}
