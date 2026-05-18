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
        codex: CodexConfig {
            enabled: false,
            command: "codex".to_string(),
            timeout_seconds: 120,
        },
        chat: blockwright_controller::config::ChatConfig {
            config_path: chat_config_path,
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
    let app = test_app(true).await;
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
    let app = test_app(true).await;
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
async fn minecraft_build_message_returns_job_id_for_direct_verification() {
    let app = test_app(true).await;
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
async fn minecraft_modification_uses_nearby_scan_to_target_saved_build() {
    let app = test_app(true).await;
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
async fn minecraft_message_returns_actions_without_queueing_a_job() {
    let app = test_app(true).await;
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
async fn generic_robot_message_with_image_attachment_enters_image_pipeline() {
    let app = test_app(true).await;
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

    assert_eq!(body["queued_job"]["summary"], "说明图片复刻流程");
}

#[tokio::test]
async fn dingtalk_stream_picture_message_queues_image_pipeline_job() {
    let app = test_app(true).await;
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
    assert_eq!(body["result"]["queued_job"]["summary"], "说明图片复刻流程");
    assert_eq!(next_body["job"]["summary"], "说明图片复刻流程");
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
    assert!(body.to_string().contains("DINGTALK_CLIENT_SECRET") == false);
}
