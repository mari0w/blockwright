use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    Router,
};
use blockwright_controller::{
    app,
    config::{
        AppConfig, CodexConfig, MinecraftConfig, SecurityConfig, ServerConfig, StorageConfig,
    },
    state::AppState,
};
use serde_json::{json, Value};
use tower::ServiceExt;

static NEXT_DIR_ID: AtomicU64 = AtomicU64::new(1);

fn temp_dir(name: &str) -> PathBuf {
    let number = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blockwright-api-{name}-{}-{number}",
        std::process::id()
    ))
}

fn config(require_token: bool) -> AppConfig {
    config_with_chat_path(
        require_token,
        temp_dir("chat-config").join("chat.local.yaml"),
    )
}

fn config_with_chat_path(require_token: bool, chat_config_path: PathBuf) -> AppConfig {
    let env_path = chat_config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(".env");
    config_with_chat_path_and_codex(
        require_token,
        chat_config_path,
        env_path,
        CodexConfig {
            enabled: false,
            command: "codex".to_string(),
            timeout_seconds: 1800,
        },
    )
}

fn config_with_chat_path_and_codex(
    require_token: bool,
    chat_config_path: PathBuf,
    env_path: PathBuf,
    codex: CodexConfig,
) -> AppConfig {
    AppConfig {
        server: ServerConfig {
            name: "local".to_string(),
            environment: "test".to_string(),
            app_name: "blockwright-controller".to_string(),
            host: "127.0.0.1".to_string(),
            port: 8765,
        },
        storage: StorageConfig {
            data_dir: temp_dir("data"),
        },
        minecraft: MinecraftConfig {
            default_server_id: "local-paper".to_string(),
        },
        security: SecurityConfig {
            shared_token: "test-token".to_string(),
            require_token,
        },
        codex,
        chat: blockwright_controller::config::ChatConfig {
            config_path: chat_config_path,
            env_path,
        },
    }
}

async fn test_app(require_token: bool) -> Router {
    let state = AppState::new(config(require_token)).await.unwrap();
    app::build_app(state)
}

async fn test_app_with_chat_config(require_token: bool, chat_config: &str) -> Router {
    let path = temp_dir("chat-config-file").join("chat.local.yaml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, chat_config).unwrap();

    let state = AppState::new(config_with_chat_path(require_token, path))
        .await
        .unwrap();
    app::build_app(state)
}

async fn test_app_with_fake_codex(require_token: bool, name: &str) -> Router {
    let script_path = fake_codex_script(name);
    let state = AppState::new(config_with_chat_path_and_codex(
        require_token,
        temp_dir("chat-config").join("chat.local.yaml"),
        temp_dir("env").join(".env"),
        CodexConfig {
            enabled: true,
            command: script_path.to_string_lossy().to_string(),
            timeout_seconds: 5,
        },
    ))
    .await
    .unwrap();
    app::build_app(state)
}

fn fake_codex_script(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    std::fs::create_dir_all(&dir).unwrap();
    let script_path = dir.join("fake-codex.sh");
    std::fs::write(
        &script_path,
        r#"#!/usr/bin/env bash
set -euo pipefail
last_message=""
schema=""
prompt_file="$(mktemp)"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-last-message)
      last_message="$2"
      shift 2
      ;;
    --output-schema)
      schema="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
cat > "$prompt_file"
if [[ -z "$last_message" ]]; then
  exit 2
fi
if ! grep -q "Blockwright 网页语音输入的翻译器" "$prompt_file" && [[ -z "$schema" ]]; then
  schema="plan.schema.json"
fi
case "$schema" in
  *plan.schema.json)
    if grep -q "给我一把钻石剑" "$prompt_file"; then
      cat > "$last_message" <<'JSON'
{
  "reply": "可以，已经准备给你一把钻石剑。",
  "summary": "发放钻石剑",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {"type": "give_item", "player": "Steve", "item": "minecraft:diamond_sword", "count": 1}
  ]
}
JSON
    elif grep -q "先聊一下" "$prompt_file"; then
      cat > "$last_message" <<'JSON'
{"reply":"可以，我们先聊方案。你想偏木屋、城堡还是现代风？我确认后再开始动工。","summary":"讨论建造方案","blueprint":null,"site_plan":null,"actions":[]}
JSON
    elif grep -q "窗户换成蓝色玻璃" "$prompt_file" && grep -q "未收到附近场地扫描" "$prompt_file"; then
      if grep -q "window.png" "$prompt_file"; then
        cat > "$last_message" <<'JSON'
{
  "reply": "我会基于当前建筑把窗户改成蓝色玻璃，并按你的整体要求继续优化。",
  "summary": "改造窗户颜色",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {
      "type": "scan_nearby_and_plan",
      "text": "把我面前这个建筑的窗户换成蓝色玻璃\n\n用户上传了参考图片，现场扫描后继续结合这次的文字和图片需求处理。",
      "attachments": [
        {
          "kind": "image",
          "source": {"type": "local_path", "path": "/tmp/window.png"},
          "file_name": "window.png",
          "mime_type": "image/png"
        }
      ]
    }
  ]
}
JSON
      else
        if grep -q "更大更复杂" "$prompt_file"; then
          cat > "$last_message" <<'JSON'
{"reply":"我会基于当前建筑把窗户改成蓝色玻璃，并继续做得更大更复杂。","summary":"改造窗户颜色","blueprint":null,"site_plan":null,"actions":[{"type":"scan_nearby_and_plan","text":"把我面前这个建筑的窗户换成蓝色玻璃，还要更大更复杂","attachments":[]}]}
JSON
        else
          cat > "$last_message" <<'JSON'
{"reply":"我会基于当前建筑把窗户改成蓝色玻璃。","summary":"改造窗户颜色","blueprint":null,"site_plan":null,"actions":[{"type":"scan_nearby_and_plan","text":"把我脚下这个建筑的窗户换成蓝色玻璃","attachments":[]}]}
JSON
        fi
      fi
    elif grep -q "窗户换成蓝色玻璃" "$prompt_file"; then
      cat > "$last_message" <<'JSON'
{
  "reply": "已按当前建筑自由调整窗户颜色。",
  "summary": "调整现有建筑窗户",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {
      "type": "place_blocks",
      "blueprint_id": "codex-window-edit",
      "origin": {"world": "world", "x": 20, "y": 64, "z": 30},
      "blocks": [
        {"x": 0, "y": 1, "z": 0, "material": "minecraft:blue_stained_glass"},
        {"x": 1, "y": 1, "z": 0, "material": "minecraft:blue_stained_glass"},
        {"x": 0, "y": 2, "z": 0, "material": "minecraft:blue_stained_glass"},
        {"x": 1, "y": 2, "z": 0, "material": "minecraft:blue_stained_glass"}
      ],
      "clear_existing": false
    }
  ]
}
JSON
    elif grep -q "摩天轮整体放大" "$prompt_file"; then
      cat > "$last_message" <<'JSON'
{
  "reply": "已按当前匹配到的摩天轮整体重做，会先清理旧结构，再放置新的逼真摩天轮。",
  "summary": "整体重做逼真摩天轮",
  "blueprint": null,
  "site_plan": null,
  "actions": [
    {
      "type": "place_blocks",
      "blueprint_id": "codex-realistic-ferris-wheel-clear",
      "origin": {"world": "world", "x": 20, "y": 64, "z": 30},
      "blocks": [{"x": 0, "y": 0, "z": 0, "material": "minecraft:air"}],
      "clear_existing": true
    },
    {
      "type": "place_blocks",
      "blueprint_id": "codex-realistic-ferris-wheel",
      "origin": {"world": "world", "x": 20, "y": 64, "z": 30},
      "blocks": [
        {"x": 0, "y": 0, "z": 0, "material": "minecraft:stone_bricks"},
        {"x": 0, "y": 1, "z": 0, "material": "minecraft:iron_bars"},
        {"x": 0, "y": 2, "z": 0, "material": "minecraft:gold_block"}
      ],
      "clear_existing": true
    }
  ]
}
JSON
    elif grep -q "local_path" "$prompt_file" && grep -q "house.png" "$prompt_file"; then
      cat > "$last_message" <<'JSON'
{
  "reply": "我已经按参考图片规划成更完整的三维复刻蓝图。",
  "summary": "建造蓝图 image-reference-house",
  "blueprint": {
    "id": "image-reference-house",
    "name": "图片复刻小木屋",
    "description": "用于 Web 图片复刻测试的三维小木屋。",
    "size": {"width": 8, "height": 4, "depth": 6},
    "primitives": [
      {"type": "box", "from": {"x": 0, "y": 0, "z": 0}, "to": {"x": 7, "y": 0, "z": 5}, "material": "minecraft:stone_bricks"},
      {"type": "hollow_box", "from": {"x": 0, "y": 1, "z": 0}, "to": {"x": 7, "y": 3, "z": 5}, "material": "minecraft:oak_planks"}
    ],
    "tags": ["house", "image_reference"]
  },
  "site_plan": null,
  "actions": []
}
JSON
    else
      cat > "$last_message" <<'JSON'
{
  "reply": "我已经规划好测试小木屋。",
  "summary": "建造蓝图 oak-house-small",
  "blueprint": {
    "id": "oak-house-small",
    "name": "测试小木屋",
    "description": "用于 API 测试的简化小木屋，包含玻璃窗。",
    "size": {"width": 3, "height": 3, "depth": 3},
    "materials": [
      {"material": "minecraft:oak_planks", "count": 5},
      {"material": "minecraft:glass", "count": 4}
    ],
    "blocks": [
      {"x": 0, "y": 0, "z": 0, "material": "minecraft:oak_planks"},
      {"x": 1, "y": 0, "z": 0, "material": "minecraft:oak_planks"},
      {"x": 2, "y": 0, "z": 0, "material": "minecraft:oak_planks"},
      {"x": 0, "y": 1, "z": 0, "material": "minecraft:glass"},
      {"x": 1, "y": 1, "z": 0, "material": "minecraft:glass"},
      {"x": 0, "y": 2, "z": 0, "material": "minecraft:glass"},
      {"x": 1, "y": 2, "z": 0, "material": "minecraft:glass"},
      {"x": 2, "y": 1, "z": 0, "material": "minecraft:oak_planks"},
      {"x": 2, "y": 2, "z": 0, "material": "minecraft:oak_planks"}
    ],
    "tags": ["house"]
  },
  "site_plan": null,
  "actions": []
}
JSON
    fi
    ;;
  "")
    if grep -q "Blockwright 网页语音输入的翻译器" "$prompt_file"; then
      cat > "$last_message" <<'TEXT'
帮我建一个小木屋
TEXT
    else
      exit 3
    fi
    ;;
  *)
    exit 3
    ;;
esac
rm -f "$prompt_file"
"#,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions).unwrap();
    script_path
}

fn request(method: &str, uri: &str, body: Option<Value>, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("x-blockwright-token", token);
    }

    match body {
        Some(value) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn response_text(response: axum::response::Response) -> String {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

async fn response_bytes(response: axum::response::Response) -> Vec<u8> {
    to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

#[tokio::test]
async fn public_health_does_not_require_token() {
    let app = test_app(true).await;

    let response = app
        .oneshot(request("GET", "/health", None, None))
        .await
        .unwrap();
    let status = response.status();
    let body = response_json(response).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["codex_enabled"], false);
}

#[tokio::test]
async fn web_chat_page_and_image_message_work_without_api_token() {
    let app = test_app_with_fake_codex(true, "api-web-chat").await;

    let page_response = app
        .clone()
        .oneshot(request("GET", "/web", None, None))
        .await
        .unwrap();
    assert_eq!(page_response.status(), StatusCode::OK);
    let page_body = response_text(page_response).await;
    assert!(page_body.contains("voiceHold"));
    assert!(page_body.contains("id=\"voiceCancelZone\""));
    assert!(page_body.contains("上滑到这里取消"));
    assert!(page_body.contains("松手取消"));
    assert!(
        page_body.contains("voiceHold.addEventListener('pointermove', updateVoiceCancelGesture)")
    );
    assert!(page_body.contains("function updateVoiceCancelGesture"));
    assert!(page_body.contains("bw.chatHistory.v1"));
    assert!(page_body.contains("bw.activeJob.v1"));
    assert!(page_body.contains("function restoreChatHistory"));
    assert!(page_body.contains("function resumeSavedJobPolling"));
    assert!(page_body.contains("restoringJobStatus"));
    assert!(page_body.contains("viewport-fit=cover"));
    assert!(page_body.contains("手机语音需要 HTTPS 地址"));
    assert!(page_body.contains("id=\"languageToggle\""));
    assert!(page_body.contains("bw.language"));
    assert!(page_body.contains("Switch to English"));
    assert!(page_body.contains("Open settings"));
    assert!(page_body.contains("Enter your Minecraft username"));
    assert!(page_body.contains("function applyLanguage"));
    assert!(page_body.contains("function setLanguage"));
    assert!(page_body.contains("id=\"cameraImage\""));
    assert!(page_body.contains("capture=\"environment\""));
    assert!(page_body.contains("id=\"libraryImages\""));
    assert!(page_body.contains("id=\"addToggle\""));
    assert!(page_body.contains("id=\"addPanel\""));
    assert!(page_body.contains("aria-controls=\"addPanel\""));
    assert!(page_body.contains("--top-control-size: 42px"));
    assert!(page_body.contains("--composer-control-height: 42px"));
    assert!(page_body.contains(".topbar .icon-button"));
    assert!(page_body.contains(
        "grid-template-columns: var(--composer-control-height) minmax(0, 1fr) var(--composer-control-height)"
    ));
    assert!(page_body.contains(">相册</span>"));
    assert!(page_body.contains("id=\"voiceToggleKeyboard\""));
    assert!(page_body.contains(".icon-button svg[hidden]"));
    assert!(page_body.contains("rows=\"1\" wrap=\"off\""));
    assert!(page_body.contains("overflow-x: auto;"));
    assert!(page_body.contains("function keepComposerSingleLine"));
    assert!(page_body.contains("toggle-icon-hidden"));
    assert!(page_body.contains("Minecraft 用户名"));
    assert!(page_body.contains("id=\"usernameGate\""));
    assert!(page_body.contains("进入聊天"));
    assert!(page_body.contains("function showUsernameGate"));
    assert!(page_body.contains("class=\"brand-mark\""));
    assert!(page_body.contains("aria-label=\"打开设置\""));
    assert!(page_body.contains("M21 4h-7"));
    assert!(page_body.contains("id=\"configPage\""));
    assert!(page_body.contains("申请麦克风权限"));
    assert!(page_body.contains("手机 HTTPS"));
    assert!(page_body.contains("/web/blockwright-local-root-ca.cer"));
    assert!(page_body.contains("下载 Blockwright 本地根证书文件"));
    assert!(page_body.contains("Files by Google"));
    assert!(page_body.contains("不是上传到 Google"));
    assert!(page_body.contains("进入设置后不会自动提醒"));
    assert!(page_body.contains("安装证书 > CA 证书"));
    assert!(page_body.contains("请用 Safari 打开证书下载链接"));
    assert!(page_body.contains("已下载描述文件"));
    assert!(page_body.contains("证书信任设置"));
    assert!(page_body.contains("id=\"httpsGuide\""));
    assert!(page_body.contains("HTTPS 设置步骤"));
    assert!(page_body.contains("我已安装证书"));
    assert!(page_body.contains("我已信任证书"));
    assert!(page_body.contains("function certificateInstallHelp"));
    assert!(page_body.contains("function certificateTrustHelp"));
    assert!(page_body.contains("/api/chat/matrix/local-config"));
    assert!(page_body.contains("text.hidden = active"));
    assert!(page_body.contains("function composerControlHeight"));
    assert!(page_body.contains(
        "send.hidden = inputShell.classList.contains('voice-mode') || !hasSendContent()"
    ));
    assert!(page_body.contains("inputShell.classList.toggle('has-send'"));
    assert!(page_body.contains("const currentImages = [...pendingImages];"));
    assert!(page_body.contains(
        "clearComposer();\n        const uploads = await Promise.all(currentImages.map((entry) => readImage(entry.file)));"
    ));
    assert!(page_body.contains("function setAddPanel"));
    assert!(page_body.contains("操作已交给 Minecraft"));
    assert!(!page_body.contains("我已经准备好方案，正在等 Minecraft 接手"));
    assert!(page_body.contains("切换到文字输入"));
    assert!(page_body.contains("navigator.mediaDevices.getUserMedia"));
    assert!(page_body.contains("网站权限设置"));
    assert!(page_body.contains("-webkit-user-select: none"));
    assert!(page_body.contains("preventVoiceHoldSelection"));
    assert!(!page_body.contains("正在听，松手后会发送。"));
    assert!(!page_body.contains("已听到："));
    assert!(!page_body.contains("Minecraft 玩家"));
    assert!(!page_body.contains("class=\"userbar\""));
    assert!(!page_body.contains("id=\"serverId\""));
    assert!(!page_body.contains("服务器</span>"));
    assert!(!page_body.contains("server_id:"));
    assert!(!page_body.contains("default_server_id"));
    assert!(!page_body.contains("voiceTarget"));
    assert!(!page_body.contains("识别语言"));
    assert!(!page_body.contains("翻译为"));
    assert!(!page_body.contains("/web/translate"));

    let message_response = app
        .oneshot(request(
            "POST",
            "/web/message",
            Some(json!({
                "username": "Charles",
                "target_player": "Charles",
                "text": "参考图片盖一个小木屋",
                "images": [
                    {
                        "file_name": "house.png",
                        "mime_type": "image/png",
                        "data_url": "data:image/png;base64,iVBORw0KGgo="
                    }
                ]
            })),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(message_response.status(), StatusCode::OK);
    let body = response_json(message_response).await;

    assert_eq!(body["attachment_count"], 1);
    assert!(body["queued_job_id"]
        .as_str()
        .unwrap()
        .starts_with("hm-job-"));
    assert!(body["reply"].as_str().unwrap().contains("蓝图"));
}

#[tokio::test]
async fn web_https_ca_certificate_download_uses_cer_inline_response() {
    let config = config(false);
    let cert_dir = config.storage.data_dir.join("https");
    std::fs::create_dir_all(&cert_dir).unwrap();
    std::fs::write(
        cert_dir.join("blockwright-local-ca.crt"),
        "-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n",
    )
    .unwrap();
    let state = AppState::new(config).await.unwrap();
    let app = app::build_app(state);

    let response = app
        .oneshot(request(
            "GET",
            "/web/blockwright-local-root-ca.cer",
            None,
            None,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/x-x509-ca-cert"
    );
    assert_eq!(
        response.headers().get(header::CONTENT_DISPOSITION).unwrap(),
        "inline; filename=\"Blockwright-Local-Root-CA.cer\""
    );
    assert_eq!(response_bytes(response).await, vec![1, 2, 3]);
}

#[tokio::test]
async fn web_voice_translate_uses_codex_without_api_token() {
    let app = test_app_with_fake_codex(true, "api-web-voice-translate").await;

    let response = app
        .oneshot(request(
            "POST",
            "/web/translate",
            Some(json!({
                "text": "build me a small wooden house",
                "target_language": "zh-CN"
            })),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;

    assert_eq!(body["translated"], true);
    assert_eq!(body["target_language"], "zh-CN");
    assert_eq!(body["translated_text"], "帮我建一个小木屋");
}

#[tokio::test]
async fn web_chat_reply_does_not_queue_minecraft_job() {
    let app = test_app_with_fake_codex(true, "api-web-chat-only").await;

    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/web/message",
            Some(json!({
                "username": "Charles",
                "target_player": "Charles",
                "server_id": "hmcl-lan",
                "text": "先聊一下，我想做一个建筑但还没想好风格",
                "images": []
            })),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;

    assert!(body["reply"].as_str().unwrap().contains("先聊方案"));
    assert!(body["queued_job_id"].is_null());
    assert!(body["queued_summary"].is_null());

    let next_response = app
        .oneshot(request(
            "GET",
            "/api/minecraft/jobs/next?server_id=hmcl-lan",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let next_body = response_json(next_response).await;
    assert!(next_body["job"].is_null());
}

#[tokio::test]
async fn web_existing_edit_queues_minecraft_scan_instead_of_manual_bw_hint() {
    let app = test_app_with_fake_codex(true, "api-web-existing-edit").await;

    let message_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/web/message",
            Some(json!({
                "username": "Charles",
                "target_player": "Charles",
                "server_id": "hmcl-lan",
                "text": "把我面前这个建筑的窗户换成蓝色玻璃",
                "images": [
                    {
                        "file_name": "window.png",
                        "mime_type": "image/png",
                        "data_url": "data:image/png;base64,iVBORw0KGgo="
                    }
                ]
            })),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(message_response.status(), StatusCode::OK);
    let body = response_json(message_response).await;

    assert!(body["queued_job_id"]
        .as_str()
        .unwrap()
        .starts_with("hm-job-"));
    assert_eq!(body["queued_summary"], "改造窗户颜色");
    assert!(body["reply"].as_str().unwrap().contains("蓝色玻璃"));
    assert!(!body["reply"].as_str().unwrap().contains("扫描"));
    assert!(!body["reply"].as_str().unwrap().contains("/bw"));
    assert!(!body["reply"].as_str().unwrap().contains("重新执行"));

    let next_response = app
        .oneshot(request(
            "GET",
            "/api/minecraft/jobs/next?server_id=hmcl-lan",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let next_body = response_json(next_response).await;
    assert_eq!(
        next_body["job"]["actions"][0]["type"],
        "scan_nearby_and_plan"
    );
    assert_eq!(
        next_body["job"]["actions"][0]["attachments"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(next_body["job"]["actions"][0]["text"]
        .as_str()
        .unwrap()
        .contains("用户上传了参考图片"));
}

#[tokio::test]
async fn web_existing_edit_followups_merge_pending_scan_job() {
    let app = test_app_with_fake_codex(true, "api-web-existing-edit-merge").await;

    let first_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/web/message",
            Some(json!({
                "username": "Charles",
                "target_player": "Charles",
                "server_id": "hmcl-lan",
                "text": "把我面前这个建筑的窗户换成蓝色玻璃",
                "images": []
            })),
            None,
        ))
        .await
        .unwrap();
    let first_body = response_json(first_response).await;
    let first_job_id = first_body["queued_job_id"].as_str().unwrap().to_string();

    let second_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/web/message",
            Some(json!({
                "username": "Charles",
                "target_player": "Charles",
                "server_id": "hmcl-lan",
                "text": "把我面前这个建筑的窗户换成蓝色玻璃，还要更大更复杂",
                "images": []
            })),
            None,
        ))
        .await
        .unwrap();
    let second_body = response_json(second_response).await;

    assert_eq!(
        second_body["queued_job_id"].as_str().unwrap(),
        first_job_id.as_str()
    );
    let next_response = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/minecraft/jobs/next?server_id=hmcl-lan",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let next_body = response_json(next_response).await;
    let action_text = next_body["job"]["actions"][0]["text"].as_str().unwrap();
    assert!(action_text.contains("窗户换成蓝色玻璃"));
    assert!(action_text.contains("更大更复杂"));

    let empty_response = app
        .oneshot(request(
            "GET",
            "/api/minecraft/jobs/next?server_id=hmcl-lan",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let empty_body = response_json(empty_response).await;
    assert!(empty_body["job"].is_null());
}

#[tokio::test]
async fn api_routes_require_shared_token() {
    let app = test_app(true).await;

    let unauthorized = app
        .clone()
        .oneshot(request("GET", "/api/blueprints", None, None))
        .await
        .unwrap();
    let authorized = app
        .oneshot(request("GET", "/api/blueprints", None, Some("test-token")))
        .await
        .unwrap();
    let body = response_json(authorized).await;

    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn robot_message_queues_job_for_minecraft_poller() {
    let app = test_app_with_fake_codex(true, "api-robot-build").await;
    let robot_request = json!({
        "platform": "telegram",
        "conversation_id": "local",
        "sender": "charles",
        "target_player": "Steve",
        "text": "帮我盖一个木屋"
    });

    let robot_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/robot/message",
            Some(robot_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let robot_body = response_json(robot_response).await;

    let next_response = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/minecraft/jobs/next?server_id=local-paper",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let next_body = response_json(next_response).await;

    let empty_response = app
        .oneshot(request(
            "GET",
            "/api/minecraft/jobs/next?server_id=local-paper",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let empty_body = response_json(empty_response).await;

    assert_eq!(robot_body["queued_job"]["server_id"], "local-paper");
    assert_eq!(next_body["job"]["summary"], "建造蓝图 oak-house-small");
    assert!(empty_body["job"].is_null());
}

#[tokio::test]
async fn build_job_result_updates_persisted_build_record() {
    let app = test_app_with_fake_codex(true, "api-build-result").await;
    let robot_request = json!({
        "platform": "telegram",
        "conversation_id": "local",
        "sender": "charles",
        "target_player": "Steve",
        "text": "帮我盖一个木屋"
    });

    let robot_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/robot/message",
            Some(robot_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let robot_body = response_json(robot_response).await;
    let job_id = robot_body["queued_job"]["id"].as_str().unwrap();
    let expected_count = robot_body["queued_job"]["actions"][0]["blocks"]
        .as_array()
        .unwrap()
        .len();

    let result_request = json!({
        "ok": true,
        "message": "ok",
        "report": {
            "actions": [
                {
                    "action_type": "place_blocks",
                    "blueprint_id": "oak-house-small",
                    "expected_count": expected_count,
                    "placed_count": expected_count,
                    "skipped_existing_count": 0,
                    "skipped_limit_count": 0,
                    "verified_count": expected_count,
                    "mismatch_count": 0,
                    "mismatches": []
                }
            ]
        }
    });

    let result_response = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/api/minecraft/jobs/{job_id}/result"),
            Some(result_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let result_body = response_json(result_response).await;

    let build_response = app
        .oneshot(request(
            "GET",
            &format!("/api/builds/{job_id}"),
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let build_body = response_json(build_response).await;

    assert_eq!(result_body["ok"], true);
    assert_eq!(build_body["status"], "succeeded");
    assert_eq!(
        build_body["expected_actions"][0]["expected_count"],
        expected_count
    );
    assert_eq!(
        build_body["result"]["actions"][0]["verified_count"],
        expected_count
    );
}

#[tokio::test]
async fn web_job_status_reports_queue_claim_and_result() {
    let app = test_app_with_fake_codex(true, "api-web-job-status").await;
    let message_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/web/message",
            Some(json!({
                "username": "Charles",
                "target_player": "Charles",
                "server_id": "hmcl-lan",
                "text": "帮我盖一个木屋",
                "images": []
            })),
            None,
        ))
        .await
        .unwrap();
    let message_body = response_json(message_response).await;
    let job_id = message_body["queued_job_id"].as_str().unwrap();

    let queued_response = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/web/jobs/{job_id}/status"),
            None,
            None,
        ))
        .await
        .unwrap();
    let queued_body = response_json(queued_response).await;
    assert_eq!(queued_body["phase"], "queued");
    assert!(queued_body["message"].as_str().unwrap().contains("接手"));

    let next_response = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/minecraft/jobs/next?server_id=hmcl-lan",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let next_body = response_json(next_response).await;
    let expected_count = next_body["job"]["actions"][0]["blocks"]
        .as_array()
        .unwrap()
        .len();

    let running_response = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/web/jobs/{job_id}/status"),
            None,
            None,
        ))
        .await
        .unwrap();
    let running_body = response_json(running_response).await;
    assert_eq!(running_body["phase"], "running");
    assert!(running_body["message"]
        .as_str()
        .unwrap()
        .contains("正在处理"));

    let result_response = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/api/minecraft/jobs/{job_id}/result"),
            Some(json!({
                "ok": true,
                "message": "ok",
                "report": {
                    "actions": [
                        {
                            "action_type": "place_blocks",
                            "blueprint_id": "oak-house-small",
                            "expected_count": expected_count,
                            "placed_count": expected_count,
                            "skipped_existing_count": 0,
                            "skipped_limit_count": 0,
                            "verified_count": expected_count,
                            "mismatch_count": 0,
                            "mismatches": []
                        }
                    ]
                }
            })),
            Some("test-token"),
        ))
        .await
        .unwrap();
    assert_eq!(result_response.status(), StatusCode::OK);

    let succeeded_response = app
        .oneshot(request(
            "GET",
            &format!("/web/jobs/{job_id}/status"),
            None,
            None,
        ))
        .await
        .unwrap();
    let succeeded_body = response_json(succeeded_response).await;
    assert_eq!(succeeded_body["phase"], "succeeded");
    assert_eq!(succeeded_body["build_status"], "succeeded");
    assert!(succeeded_body["result_message"].is_null());
}

#[tokio::test]
async fn web_item_job_status_does_not_blame_player_wording() {
    let app = test_app_with_fake_codex(true, "api-web-item-job-status").await;
    let message_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/web/message",
            Some(json!({
                "username": "Steve",
                "target_player": "Steve",
                "server_id": "hmcl-lan",
                "text": "给我一把钻石剑",
                "images": []
            })),
            None,
        ))
        .await
        .unwrap();
    let message_body = response_json(message_response).await;
    let job_id = message_body["queued_job_id"].as_str().unwrap();

    let queued_response = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/web/jobs/{job_id}/status"),
            None,
            None,
        ))
        .await
        .unwrap();
    let queued_body = response_json(queued_response).await;
    assert_eq!(queued_body["phase"], "queued");
    assert!(queued_body["message"].as_str().unwrap().contains("接手"));

    let next_response = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/minecraft/jobs/next?server_id=hmcl-lan",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let next_body = response_json(next_response).await;
    assert_eq!(next_body["job"]["actions"][0]["type"], "give_item");

    let running_response = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/web/jobs/{job_id}/status"),
            None,
            None,
        ))
        .await
        .unwrap();
    let running_body = response_json(running_response).await;
    assert_eq!(running_body["phase"], "running");
    assert!(running_body["message"]
        .as_str()
        .unwrap()
        .contains("正在处理"));

    let result_response = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/api/minecraft/jobs/{job_id}/result"),
            Some(json!({
                "ok": false,
                "message": "找不到玩家：Steve"
            })),
            Some("test-token"),
        ))
        .await
        .unwrap();
    assert_eq!(result_response.status(), StatusCode::OK);

    let failed_response = app
        .oneshot(request(
            "GET",
            &format!("/web/jobs/{job_id}/status"),
            None,
            None,
        ))
        .await
        .unwrap();
    let failed_body = response_json(failed_response).await;
    let failed_message = failed_body["message"].as_str().unwrap();
    assert_eq!(failed_body["phase"], "failed");
    assert!(failed_message.contains("没有完成执行"));
    assert!(!failed_message.contains("发物品失败"));
    assert!(!failed_message.contains("放到世界"));
    assert!(!failed_message.contains("调整说法"));
    assert_eq!(failed_body["result_message"], "找不到玩家：Steve");
}

#[tokio::test]
async fn minecraft_build_message_returns_job_id_for_direct_execution() {
    let app = test_app_with_fake_codex(true, "api-minecraft-build").await;
    let minecraft_request = json!({
        "server_id": "local-paper",
        "player": "Steve",
        "text": "帮我盖一个木屋",
        "position": {
            "world": "world",
            "x": 0,
            "y": 64,
            "z": 0
        }
    });

    let message_response = app
        .oneshot(request(
            "POST",
            "/api/minecraft/message",
            Some(minecraft_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let message_body = response_json(message_response).await;
    let job_id = message_body["job_id"].as_str().unwrap();

    assert_eq!(message_body["actions"][0]["type"], "place_blocks");
    assert!(job_id.starts_with("hm-job-"));
}

#[tokio::test]
async fn minecraft_modification_without_scan_asks_fabric_to_rescan_once() {
    let app = test_app_with_fake_codex(true, "api-minecraft-missing-scan").await;
    let modification_request = json!({
        "server_id": "hmcl-lan",
        "player": "Steve",
        "text": "把我脚下这个建筑的窗户换成蓝色玻璃",
        "position": {
            "world": "world",
            "x": 0,
            "y": 64,
            "z": 0
        }
    });

    let response = app
        .oneshot(request(
            "POST",
            "/api/minecraft/message",
            Some(modification_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let body = response_json(response).await;

    assert_eq!(body["actions"][0]["type"], "scan_nearby_and_plan");
    assert_eq!(
        body["actions"][0]["text"],
        "把我脚下这个建筑的窗户换成蓝色玻璃"
    );
    assert!(body["job_id"].is_null());
}

#[tokio::test]
async fn minecraft_modification_with_scan_uses_codex_plan() {
    let app = test_app_with_fake_codex(true, "api-minecraft-modification").await;
    let build_request = json!({
        "server_id": "local-paper",
        "player": "Steve",
        "text": "帮我盖一个木屋",
        "position": {
            "world": "world",
            "x": 0,
            "y": 64,
            "z": 0
        }
    });

    let build_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/minecraft/message",
            Some(build_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let build_body = response_json(build_response).await;
    let job_id = build_body["job_id"].as_str().unwrap();
    let build_action = &build_body["actions"][0];
    let origin = &build_action["origin"];
    let blocks = build_action["blocks"].as_array().unwrap();
    let expected_count = blocks.len();

    let scan_blocks = blocks
        .iter()
        .map(|block| {
            json!({
                "x": origin["x"].as_i64().unwrap() + block["x"].as_i64().unwrap(),
                "y": origin["y"].as_i64().unwrap() + block["y"].as_i64().unwrap(),
                "z": origin["z"].as_i64().unwrap() + block["z"].as_i64().unwrap(),
                "material": block["material"]
            })
        })
        .collect::<Vec<_>>();
    let result_request = json!({
        "ok": true,
        "message": "ok",
        "report": {
            "actions": [
                {
                    "action_type": "place_blocks",
                    "blueprint_id": "oak-house-small",
                    "expected_count": expected_count,
                    "placed_count": expected_count,
                    "skipped_existing_count": 0,
                    "skipped_limit_count": 0,
                    "verified_count": expected_count,
                    "mismatch_count": 0,
                    "mismatches": []
                }
            ]
        }
    });
    app.clone()
        .oneshot(request(
            "POST",
            &format!("/api/minecraft/jobs/{job_id}/result"),
            Some(result_request),
            Some("test-token"),
        ))
        .await
        .unwrap();

    let modification_request = json!({
        "server_id": "local-paper",
        "player": "Steve",
        "text": "把我面前这个房子的窗户换成蓝色玻璃",
        "position": {
            "world": "world",
            "x": 0,
            "y": 64,
            "z": 0
        },
        "nearby_scan": {
            "world": "world",
            "center_x": 2,
            "center_y": 64,
            "center_z": 2,
            "radius": 8,
            "blocks": scan_blocks
        }
    });

    let modification_response = app
        .oneshot(request(
            "POST",
            "/api/minecraft/message",
            Some(modification_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let modification_body = response_json(modification_response).await;

    assert_eq!(modification_body["actions"][0]["type"], "place_blocks");
    assert_eq!(
        modification_body["actions"][0]["blocks"][0]["material"],
        "minecraft:blue_stained_glass"
    );
    assert_eq!(
        modification_body["actions"][0]["blocks"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(modification_body["job_id"]
        .as_str()
        .unwrap()
        .starts_with("hm-job-"));
}

#[tokio::test]
async fn minecraft_modification_with_unrecorded_scan_uses_codex_plan() {
    let app = test_app_with_fake_codex(true, "api-minecraft-auto-adopt").await;
    let modification_request = json!({
        "server_id": "local-paper",
        "player": "Steve",
        "text": "把我面前这个建筑的窗户换成蓝色玻璃",
        "position": {
            "world": "world",
            "x": 0,
            "y": 64,
            "z": 0
        },
        "nearby_scan": {
            "world": "world",
            "center_x": 20,
            "center_y": 64,
            "center_z": 30,
            "radius": 8,
            "blocks": [
                {"x": 20, "y": 63, "z": 30, "material": "minecraft:water[level=0]"},
                {"x": 20, "y": 64, "z": 30, "material": "minecraft:gold_block"},
                {"x": 21, "y": 64, "z": 30, "material": "minecraft:copper_block"}
            ]
        }
    });

    let modification_response = app
        .oneshot(request(
            "POST",
            "/api/minecraft/message",
            Some(modification_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let modification_body = response_json(modification_response).await;

    assert_eq!(modification_body["actions"][0]["type"], "place_blocks");
    assert_eq!(
        modification_body["actions"][0]["blocks"][0]["material"],
        "minecraft:blue_stained_glass"
    );
    assert!(!modification_body["reply"]
        .as_str()
        .unwrap()
        .contains("保存"));
    assert!(!modification_body["reply"]
        .as_str()
        .unwrap()
        .contains("登记"));
    assert!(modification_body["job_id"]
        .as_str()
        .unwrap()
        .starts_with("hm-job-"));
}

#[tokio::test]
async fn minecraft_whole_build_replacement_does_not_ask_for_part() {
    let app = test_app_with_fake_codex(true, "api-minecraft-whole-replacement").await;
    let build_request = json!({
        "server_id": "local-paper",
        "player": "Steve",
        "text": "帮我盖一个摩天轮",
        "position": {
            "world": "world",
            "x": 0,
            "y": 64,
            "z": 0
        }
    });

    let build_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/minecraft/message",
            Some(build_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let build_body = response_json(build_response).await;
    let job_id = build_body["job_id"].as_str().unwrap();
    let build_action = &build_body["actions"][0];
    let origin = &build_action["origin"];
    let blocks = build_action["blocks"].as_array().unwrap();
    let expected_count = blocks.len();
    let scan_blocks = blocks
        .iter()
        .map(|block| {
            json!({
                "x": origin["x"].as_i64().unwrap() + block["x"].as_i64().unwrap(),
                "y": origin["y"].as_i64().unwrap() + block["y"].as_i64().unwrap(),
                "z": origin["z"].as_i64().unwrap() + block["z"].as_i64().unwrap(),
                "material": block["material"]
            })
        })
        .collect::<Vec<_>>();
    let result_request = json!({
        "ok": true,
        "message": "ok",
        "report": {
            "actions": [
                {
                    "action_type": "place_blocks",
                    "blueprint_id": "oak-house-small",
                    "expected_count": expected_count,
                    "placed_count": expected_count,
                    "skipped_existing_count": 0,
                    "skipped_limit_count": 0,
                    "verified_count": expected_count,
                    "mismatch_count": 0,
                    "mismatches": []
                }
            ]
        }
    });
    app.clone()
        .oneshot(request(
            "POST",
            &format!("/api/minecraft/jobs/{job_id}/result"),
            Some(result_request),
            Some("test-token"),
        ))
        .await
        .unwrap();

    let modification_request = json!({
        "server_id": "local-paper",
        "player": "Steve",
        "text": "我要改的是摩天轮，把摩天轮整体放大，重做，我要的是逼真的摩天轮",
        "position": {
            "world": "world",
            "x": 0,
            "y": 64,
            "z": 0
        },
        "nearby_scan": {
            "world": "world",
            "center_x": 2,
            "center_y": 64,
            "center_z": 2,
            "radius": 8,
            "blocks": scan_blocks
        }
    });

    let modification_response = app
        .oneshot(request(
            "POST",
            "/api/minecraft/message",
            Some(modification_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let modification_body = response_json(modification_response).await;
    let actions = modification_body["actions"].as_array().unwrap();

    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0]["type"], "place_blocks");
    assert_eq!(actions[0]["blocks"][0]["material"], "minecraft:air");
    assert_eq!(actions[1]["type"], "place_blocks");
    assert!(modification_body["reply"]
        .as_str()
        .unwrap()
        .contains("整体重做"));
    assert!(!modification_body["reply"]
        .as_str()
        .unwrap()
        .contains("哪个部位"));
    assert!(modification_body["job_id"]
        .as_str()
        .unwrap()
        .starts_with("hm-job-"));
}

#[tokio::test]
async fn minecraft_message_returns_actions_without_queueing_a_job() {
    let app = test_app_with_fake_codex(true, "api-minecraft-action").await;
    let minecraft_request = json!({
        "server_id": "local-paper",
        "player": "Steve",
        "text": "给我一把钻石剑",
        "position": {
            "world": "world",
            "x": 0,
            "y": 64,
            "z": 0
        }
    });

    let message_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/minecraft/message",
            Some(minecraft_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let message_body = response_json(message_response).await;

    let next_response = app
        .oneshot(request(
            "GET",
            "/api/minecraft/jobs/next?server_id=local-paper",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let next_body = response_json(next_response).await;

    assert_eq!(message_body["actions"][0]["type"], "give_item");
    assert_eq!(
        message_body["actions"][0]["item"],
        "minecraft:diamond_sword"
    );
    assert!(next_body["job"].is_null());
}

#[tokio::test]
async fn minecraft_progress_endpoint_reports_codex_phase_for_request() {
    let app = test_app_with_fake_codex(true, "api-minecraft-progress").await;
    let message_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/minecraft/message",
            Some(json!({
                "server_id": "local-paper",
                "player": "Steve",
                "text": "给我一把钻石剑",
                "progress_id": "test-progress-1"
            })),
            Some("test-token"),
        ))
        .await
        .unwrap();
    assert_eq!(message_response.status(), StatusCode::OK);

    let progress_response = app
        .oneshot(request(
            "GET",
            "/api/minecraft/progress/test-progress-1",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    assert_eq!(progress_response.status(), StatusCode::OK);
    let progress_body = response_json(progress_response).await;

    assert!(progress_body["sequence"].as_u64().unwrap() >= 1);
    assert_eq!(progress_body["done"], true);
    assert!(progress_body["message"]
        .as_str()
        .unwrap()
        .contains("AI 助手"));
}

#[tokio::test]
async fn generic_robot_message_with_image_attachment_enters_codex_blueprint() {
    let app = test_app_with_fake_codex(true, "api-robot-image").await;
    let robot_request = json!({
        "platform": "telegram",
        "conversation_id": "local",
        "sender": "charles",
        "text": "照这个做",
        "attachments": [
            {
                "kind": "image",
                "source": {
                    "type": "url",
                    "url": "https://example.com/house.png"
                },
                "file_name": "house.png",
                "mime_type": "image/png"
            }
        ]
    });

    let robot_response = app
        .oneshot(request(
            "POST",
            "/api/robot/message",
            Some(robot_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let body = response_json(robot_response).await;

    assert_eq!(body["queued_job"]["summary"], "建造蓝图 oak-house-small");
}

#[tokio::test]
async fn dingtalk_stream_picture_message_queues_codex_blueprint_job() {
    let app = test_app_with_fake_codex(true, "api-dingtalk-image").await;
    let dingtalk_message = json!({
        "conversationId": "cid-1",
        "senderNick": "张三",
        "senderStaffId": "001",
        "senderId": "sender-1",
        "msgtype": "picture",
        "content": {
            "pictureDownloadCode": "picture-code",
            "downloadCode": "download-code"
        }
    });
    let stream_request = json!({
        "headers": {
            "topic": "/v1.0/im/bot/messages/get",
            "messageId": "msg-1"
        },
        "data": dingtalk_message.to_string()
    });

    let stream_response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/chat/dingtalk/stream",
            Some(stream_request),
            Some("test-token"),
        ))
        .await
        .unwrap();
    let body = response_json(stream_response).await;

    let next_response = app
        .oneshot(request(
            "GET",
            "/api/minecraft/jobs/next?server_id=local-paper",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let next_body = response_json(next_response).await;

    assert_eq!(body["code"], 200);
    assert_eq!(body["headers"]["messageId"], "msg-1");
    assert_eq!(
        body["result"]["queued_job"]["summary"],
        "建造蓝图 oak-house-small"
    );
    assert_eq!(next_body["job"]["summary"], "建造蓝图 oak-house-small");
}

#[tokio::test]
async fn chat_adapters_are_loaded_from_untracked_runtime_config() {
    let app = test_app_with_chat_config(
        true,
        r#"
tools:
  - name: dingtalk-local
    platform: dingtalk
    enabled: true
    inbound: stream
    default_server_id: local-paper
    default_target_player: Steve
    dingtalk:
      client_id_env: DINGTALK_CLIENT_ID
      client_secret_env: DINGTALK_CLIENT_SECRET
  - name: element-local
    platform: matrix
    enabled: true
    inbound: polling
    default_server_id: hmcl-lan
    default_target_player: Charles
    matrix:
      homeserver_url: https://matrix.org
      access_token_env: MATRIX_ACCESS_TOKEN
      allowed_senders:
        - "@enochzzg:matrix.org"
      auto_join_invites: true
"#,
    )
    .await;

    let response = app
        .oneshot(request(
            "GET",
            "/api/chat/adapters",
            None,
            Some("test-token"),
        ))
        .await
        .unwrap();
    let body = response_json(response).await;

    assert_eq!(body["tools"][0]["name"], "dingtalk-local");
    assert_eq!(body["tools"][0]["inbound"], "stream");
    assert_eq!(body["tools"][0]["local_friendly"], true);
    assert_eq!(body["tools"][1]["name"], "element-local");
    assert_eq!(body["tools"][1]["platform"], "matrix");
    assert_eq!(body["tools"][1]["inbound"], "polling");
    assert_eq!(body["tools"][1]["local_friendly"], true);
    assert!(body.to_string().contains("DINGTALK_CLIENT_SECRET") == false);
    assert!(body.to_string().contains("MATRIX_ACCESS_TOKEN") == false);
}

#[tokio::test]
async fn matrix_local_config_endpoint_writes_untracked_config_and_env() {
    let chat_path = temp_dir("matrix-local-config").join("chat.local.yaml");
    let env_path = chat_path.parent().unwrap().join(".env");
    let state = AppState::new(config_with_chat_path(true, chat_path.clone()))
        .await
        .unwrap();
    let app = app::build_app(state);

    let response = app
        .oneshot(request(
            "PUT",
            "/api/chat/matrix/local-config",
            Some(json!({
                "enabled": false,
                "homeserver_url": "https://matrix-client.matrix.org",
                "access_token": "test-matrix-token",
                "allowed_sender": "@enochzzg:matrix.org",
                "allow_own_user_messages": true,
                "auto_join_invites": true,
                "default_server_id": "hmcl-lan",
                "default_target_player": "Charles"
            })),
            Some("test-token"),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["token_configured"], true);
    let chat_source = std::fs::read_to_string(chat_path).unwrap();
    let env_source = std::fs::read_to_string(env_path).unwrap();
    assert!(chat_source.contains("element-local"));
    assert!(chat_source.contains("@enochzzg:matrix.org"));
    assert!(chat_source.contains("MATRIX_ACCESS_TOKEN"));
    assert!(!chat_source.contains("test-matrix-token"));
    assert!(env_source.contains("MATRIX_ACCESS_TOKEN=test-matrix-token"));
}
