use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        LazyLock, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    config::{ChatInboundMode, ChatPlatform, ChatToolConfig, MatrixChatConfig},
    http::robot::queue_chat_message,
    services::chat::IncomingChatMessage,
    state::AppState,
};

const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 2;
const DEFAULT_SYNC_TIMEOUT_SECONDS: u64 = 30;

static NEXT_TXN_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_POLLERS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
const SENT_EVENT_ID_CACHE_LIMIT: usize = 256;

pub fn spawn_pollers(state: AppState) {
    let tools = state
        .chat
        .tools
        .iter()
        .filter(|tool| {
            tool.enabled
                && tool.platform == ChatPlatform::Matrix
                && tool.inbound == ChatInboundMode::Polling
        })
        .cloned()
        .collect::<Vec<_>>();

    for tool in tools {
        spawn_tool_poller(state.clone(), tool);
    }
}

pub fn spawn_tool_poller(state: AppState, tool: ChatToolConfig) -> bool {
    if !mark_poller_active(&tool.name) {
        return false;
    }
    let tool_name = tool.name.clone();
    tokio::spawn(async move {
        run_matrix_poller(state, tool).await;
        mark_poller_inactive(&tool_name);
    });
    true
}

async fn run_matrix_poller(state: AppState, tool: ChatToolConfig) {
    let Some(matrix) = tool.matrix.clone() else {
        tracing::warn!(tool = %tool.name, "matrix polling tool is missing matrix config");
        return;
    };
    let access_token = match std::env::var(&matrix.access_token_env) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            tracing::warn!(
                tool = %tool.name,
                access_token_env = %matrix.access_token_env,
                "matrix polling disabled because access token env is missing"
            );
            return;
        }
    };
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(
            matrix
                .sync_timeout_seconds
                .unwrap_or(DEFAULT_SYNC_TIMEOUT_SECONDS)
                + 10,
        ))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(tool = %tool.name, error = %error, "failed to create matrix http client");
            return;
        }
    };
    let homeserver_url = trim_trailing_slash(&matrix.homeserver_url);
    let own_user_id = match matrix_whoami(&client, &homeserver_url, &access_token).await {
        Ok(user_id) => user_id,
        Err(error) => {
            if is_matrix_unauthorized_error(&error) {
                tracing::warn!(
                    tool = %tool.name,
                    homeserver = %homeserver_url,
                    access_token_env = %matrix.access_token_env,
                    error = %error,
                    "Matrix is enabled, but the access token is invalid. Reconfigure it in Web settings or disable Matrix."
                );
            } else {
                tracing::warn!(
                    tool = %tool.name,
                    homeserver = %homeserver_url,
                    error = %error,
                    "matrix polling disabled because whoami failed"
                );
            }
            return;
        }
    };
    let token_path = sync_token_path(&state, &tool.name);
    let mut sent_event_ids = SentEventIds::default();
    let mut since = tokio::fs::read_to_string(&token_path)
        .await
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    tracing::info!(
        tool = %tool.name,
        room_id = %matrix.room_id.as_deref().unwrap_or("*"),
        has_since = since.is_some(),
        "starting matrix polling adapter"
    );

    loop {
        let initial_sync = since.is_none();
        match matrix_sync(
            &client,
            &homeserver_url,
            &access_token,
            &matrix,
            since.as_deref(),
        )
        .await
        {
            Ok(sync) => {
                if matrix.auto_join_invites.unwrap_or(true) {
                    for room_id in sync.rooms.invite.keys() {
                        if let Err(error) =
                            matrix_join_room(&client, &homeserver_url, &access_token, room_id).await
                        {
                            tracing::warn!(
                                tool = %tool.name,
                                room_id = %room_id,
                                error = %error,
                                "failed to join matrix invited room"
                            );
                        }
                    }
                }

                if !initial_sync {
                    let messages = matrix_sync_to_chat_messages(
                        &sync,
                        &tool,
                        &matrix,
                        own_user_id.as_str(),
                        &sent_event_ids,
                    );
                    for message in messages {
                        let room_id = message.conversation_id.clone();
                        let result = queue_chat_message(&state, message).await;
                        match matrix_send_text(
                            &client,
                            &homeserver_url,
                            &access_token,
                            &room_id,
                            &result.reply,
                        )
                        .await
                        {
                            Ok(event_id) => sent_event_ids.insert(event_id),
                            Err(error) => {
                                tracing::warn!(
                                    tool = %tool.name,
                                    room_id = %room_id,
                                    error = %error,
                                    "failed to send matrix reply"
                                );
                            }
                        }
                    }
                }

                since = Some(sync.next_batch.clone());
                if let Err(error) = write_sync_token(&token_path, &sync.next_batch).await {
                    tracing::warn!(
                        tool = %tool.name,
                        error = %error,
                        "failed to persist matrix sync token"
                    );
                }
            }
            Err(error) => {
                tracing::warn!(tool = %tool.name, error = %error, "matrix sync failed");
            }
        }

        tokio::time::sleep(Duration::from_secs(
            matrix
                .poll_interval_seconds
                .unwrap_or(DEFAULT_POLL_INTERVAL_SECONDS),
        ))
        .await;
    }
}

fn mark_poller_active(tool_name: &str) -> bool {
    match ACTIVE_POLLERS.lock() {
        Ok(mut active) => active.insert(tool_name.to_string()),
        Err(_) => false,
    }
}

fn mark_poller_inactive(tool_name: &str) {
    if let Ok(mut active) = ACTIVE_POLLERS.lock() {
        active.remove(tool_name);
    }
}

async fn matrix_whoami(
    client: &reqwest::Client,
    homeserver_url: &str,
    access_token: &str,
) -> Result<String, String> {
    let url = format!("{homeserver_url}/_matrix/client/v3/account/whoami");
    let response = client
        .get(url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    ensure_matrix_success(response.status()).map_err(|error| format!("whoami {error}"))?;
    let body = response
        .json::<MatrixWhoamiResponse>()
        .await
        .map_err(|error| error.to_string())?;
    Ok(body.user_id)
}

async fn matrix_sync(
    client: &reqwest::Client,
    homeserver_url: &str,
    access_token: &str,
    matrix: &MatrixChatConfig,
    since: Option<&str>,
) -> Result<MatrixSyncResponse, String> {
    let timeout_ms = matrix
        .sync_timeout_seconds
        .unwrap_or(DEFAULT_SYNC_TIMEOUT_SECONDS)
        * 1000;
    let url = format!("{homeserver_url}/_matrix/client/v3/sync");
    let mut request = client
        .get(url)
        .bearer_auth(access_token)
        .query(&[("timeout", timeout_ms.to_string())]);
    if let Some(since) = since {
        request = request.query(&[("since", since)]);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    ensure_matrix_success(response.status()).map_err(|error| format!("sync {error}"))?;
    response
        .json::<MatrixSyncResponse>()
        .await
        .map_err(|error| error.to_string())
}

async fn matrix_send_text(
    client: &reqwest::Client,
    homeserver_url: &str,
    access_token: &str,
    room_id: &str,
    body: &str,
) -> Result<String, String> {
    let room_id = path_encode(room_id);
    let txn_id = next_txn_id();
    let url =
        format!("{homeserver_url}/_matrix/client/v3/rooms/{room_id}/send/m.room.message/{txn_id}");
    let response = client
        .put(url)
        .bearer_auth(access_token)
        .json(&MatrixSendMessageRequest {
            msgtype: "m.text",
            body,
        })
        .send()
        .await
        .map_err(|error| error.to_string())?;
    ensure_matrix_success(response.status()).map_err(|error| format!("send {error}"))?;
    let body = response
        .json::<MatrixSendMessageResponse>()
        .await
        .map_err(|error| error.to_string())?;
    Ok(body.event_id)
}

async fn matrix_join_room(
    client: &reqwest::Client,
    homeserver_url: &str,
    access_token: &str,
    room_id: &str,
) -> Result<(), String> {
    let room_id = path_encode(room_id);
    let url = format!("{homeserver_url}/_matrix/client/v3/join/{room_id}");
    let response = client
        .post(url)
        .bearer_auth(access_token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    ensure_matrix_success(response.status()).map_err(|error| format!("join {error}"))?;
    Ok(())
}

fn matrix_sync_to_chat_messages(
    sync: &MatrixSyncResponse,
    tool: &ChatToolConfig,
    matrix: &MatrixChatConfig,
    own_user_id: &str,
    sent_event_ids: &SentEventIds,
) -> Vec<IncomingChatMessage> {
    let rooms = if let Some(room_id) = matrix.room_id.as_deref() {
        sync.rooms
            .join
            .get(room_id)
            .map(|room| vec![(room_id, room)])
            .unwrap_or_default()
    } else {
        sync.rooms
            .join
            .iter()
            .map(|(room_id, room)| (room_id.as_str(), room))
            .collect::<Vec<_>>()
    };

    rooms
        .into_iter()
        .flat_map(|(room_id, room)| {
            room.timeline.events.iter().filter_map(move |event| {
                matrix_event_to_chat_message(
                    event,
                    tool,
                    matrix,
                    room_id,
                    own_user_id,
                    sent_event_ids,
                )
            })
        })
        .collect()
}

fn matrix_event_to_chat_message(
    event: &MatrixEvent,
    tool: &ChatToolConfig,
    matrix: &MatrixChatConfig,
    room_id: &str,
    own_user_id: &str,
    sent_event_ids: &SentEventIds,
) -> Option<IncomingChatMessage> {
    if event.event_type != "m.room.message" || sent_event_ids.contains(event.event_id.as_deref()) {
        return None;
    }
    if event
        .unsigned
        .transaction_id
        .as_deref()
        .is_some_and(|txn_id| txn_id.starts_with("blockwright-"))
    {
        return None;
    }
    if event.sender == own_user_id && !matrix.allow_own_user_messages.unwrap_or(false) {
        return None;
    }
    if !matrix.allowed_senders.is_empty()
        && !matrix
            .allowed_senders
            .iter()
            .any(|sender| sender == &event.sender)
    {
        return None;
    }
    let msgtype = event.content.get("msgtype").and_then(Value::as_str)?;
    if msgtype != "m.text" && msgtype != "m.notice" {
        return None;
    }
    let text = event.content.get("body").and_then(Value::as_str)?.trim();
    if text.is_empty() {
        return None;
    }

    Some(IncomingChatMessage {
        platform: "matrix".to_string(),
        conversation_id: room_id.to_string(),
        sender: event.sender.clone(),
        server_id: tool.default_server_id.clone(),
        target_player: tool.default_target_player.clone(),
        text: text.to_string(),
        position: None,
        attachments: Vec::new(),
    })
}

async fn write_sync_token(path: &PathBuf, token: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, token).await
}

fn sync_token_path(state: &AppState, tool_name: &str) -> PathBuf {
    state
        .config
        .storage
        .data_dir
        .join("matrix")
        .join(format!("{}.sync", safe_file_name(tool_name)))
}

fn safe_file_name(value: &str) -> String {
    let safe = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect::<String>();
    if safe.is_empty() {
        "matrix".to_string()
    } else {
        safe
    }
}

fn trim_trailing_slash(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn path_encode(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

fn next_txn_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = NEXT_TXN_ID.fetch_add(1, Ordering::Relaxed);
    format!("blockwright-{millis}-{sequence}")
}

fn ensure_matrix_success(status: StatusCode) -> Result<(), String> {
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("returned HTTP {status}"))
    }
}

fn is_matrix_unauthorized_error(error: &str) -> bool {
    error.contains("401") || error.to_ascii_lowercase().contains("unauthorized")
}

#[derive(Debug, Default)]
struct SentEventIds {
    order: VecDeque<String>,
    values: HashSet<String>,
}

impl SentEventIds {
    fn insert(&mut self, event_id: String) {
        if self.values.insert(event_id.clone()) {
            self.order.push_back(event_id);
        }
        while self.order.len() > SENT_EVENT_ID_CACHE_LIMIT {
            if let Some(old_event_id) = self.order.pop_front() {
                self.values.remove(&old_event_id);
            }
        }
    }

    fn contains(&self, event_id: Option<&str>) -> bool {
        event_id.is_some_and(|event_id| self.values.contains(event_id))
    }
}

#[derive(Debug, Deserialize)]
struct MatrixWhoamiResponse {
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct MatrixSendMessageResponse {
    event_id: String,
}

#[derive(Debug, Deserialize)]
struct MatrixSyncResponse {
    next_batch: String,
    #[serde(default)]
    rooms: MatrixRooms,
}

#[derive(Debug, Default, Deserialize)]
struct MatrixRooms {
    #[serde(default)]
    join: HashMap<String, MatrixJoinedRoom>,
    #[serde(default)]
    invite: HashMap<String, MatrixInvitedRoom>,
}

#[derive(Debug, Deserialize)]
struct MatrixJoinedRoom {
    #[serde(default)]
    timeline: MatrixTimeline,
}

#[derive(Debug, Deserialize)]
struct MatrixInvitedRoom {}

#[derive(Debug, Default, Deserialize)]
struct MatrixTimeline {
    #[serde(default)]
    events: Vec<MatrixEvent>,
}

#[derive(Debug, Deserialize)]
struct MatrixEvent {
    #[serde(default)]
    event_id: Option<String>,
    #[serde(rename = "type")]
    event_type: String,
    sender: String,
    #[serde(default)]
    content: Value,
    #[serde(default)]
    unsigned: MatrixUnsigned,
}

#[derive(Debug, Default, Deserialize)]
struct MatrixUnsigned {
    transaction_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct MatrixSendMessageRequest<'a> {
    msgtype: &'a str,
    body: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix_tool() -> ChatToolConfig {
        ChatToolConfig {
            name: "element-local".to_string(),
            platform: ChatPlatform::Matrix,
            enabled: true,
            inbound: ChatInboundMode::Polling,
            default_server_id: Some("local-java".to_string()),
            default_target_player: Some("Charles".to_string()),
            dingtalk: None,
            matrix: Some(MatrixChatConfig {
                homeserver_url: "https://matrix.org".to_string(),
                access_token_env: "MATRIX_ACCESS_TOKEN".to_string(),
                room_id: Some("!room:matrix.org".to_string()),
                allowed_senders: Vec::new(),
                allow_own_user_messages: None,
                auto_join_invites: None,
                poll_interval_seconds: None,
                sync_timeout_seconds: None,
            }),
        }
    }

    #[test]
    fn parses_matrix_text_events_from_configured_room() {
        let sync = serde_json::from_value::<MatrixSyncResponse>(serde_json::json!({
            "next_batch": "s2",
            "rooms": {
                "join": {
                    "!room:matrix.org": {
                        "timeline": {
                            "events": [
                                {
                                    "type": "m.room.message",
                                    "sender": "@charles:matrix.org",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "帮我盖一个木屋"
                                    }
                                },
                                {
                                    "type": "m.room.message",
                                    "sender": "@bot:matrix.org",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "我自己的回复"
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        }))
        .unwrap();

        let tool = matrix_tool();
        let matrix = tool.matrix.as_ref().unwrap();
        let messages = matrix_sync_to_chat_messages(
            &sync,
            &tool,
            matrix,
            "@bot:matrix.org",
            &SentEventIds::default(),
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].platform, "matrix");
        assert_eq!(messages[0].conversation_id, "!room:matrix.org");
        assert_eq!(messages[0].sender, "@charles:matrix.org");
        assert_eq!(messages[0].text, "帮我盖一个木屋");
        assert_eq!(messages[0].target_player.as_deref(), Some("Charles"));
    }

    #[test]
    fn ignores_non_text_or_other_room_events() {
        let sync = serde_json::from_value::<MatrixSyncResponse>(serde_json::json!({
            "next_batch": "s2",
            "rooms": {
                "join": {
                    "!other:matrix.org": {
                        "timeline": {
                            "events": [
                                {
                                    "type": "m.room.message",
                                    "sender": "@charles:matrix.org",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "不该处理"
                                    }
                                }
                            ]
                        }
                    },
                    "!room:matrix.org": {
                        "timeline": {
                            "events": [
                                {
                                    "type": "m.room.message",
                                    "sender": "@charles:matrix.org",
                                    "content": {
                                        "msgtype": "m.image",
                                        "body": "image.png"
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        }))
        .unwrap();

        let tool = matrix_tool();
        let matrix = tool.matrix.as_ref().unwrap();
        let messages = matrix_sync_to_chat_messages(
            &sync,
            &tool,
            matrix,
            "@bot:matrix.org",
            &SentEventIds::default(),
        );

        assert!(messages.is_empty());
    }

    #[test]
    fn can_listen_to_any_joined_room_for_allowed_sender() {
        let mut tool = matrix_tool();
        let matrix = tool.matrix.as_mut().unwrap();
        matrix.room_id = None;
        matrix.allowed_senders = vec!["@enochzzg:matrix.org".to_string()];
        let sync = serde_json::from_value::<MatrixSyncResponse>(serde_json::json!({
            "next_batch": "s2",
            "rooms": {
                "join": {
                    "!room-a:matrix.org": {
                        "timeline": {
                            "events": [
                                {
                                    "type": "m.room.message",
                                    "sender": "@other:matrix.org",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "忽略"
                                    }
                                }
                            ]
                        }
                    },
                    "!room-b:matrix.org": {
                        "timeline": {
                            "events": [
                                {
                                    "type": "m.room.message",
                                    "sender": "@enochzzg:matrix.org",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "给我一把钻石剑"
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        }))
        .unwrap();

        let matrix = tool.matrix.as_ref().unwrap();
        let messages = matrix_sync_to_chat_messages(
            &sync,
            &tool,
            matrix,
            "@bot:matrix.org",
            &SentEventIds::default(),
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].conversation_id, "!room-b:matrix.org");
        assert_eq!(messages[0].sender, "@enochzzg:matrix.org");
    }

    #[test]
    fn allows_personal_token_messages_but_skips_sent_replies() {
        let mut tool = matrix_tool();
        let matrix = tool.matrix.as_mut().unwrap();
        matrix.allowed_senders = vec!["@enochzzg:matrix.org".to_string()];
        matrix.allow_own_user_messages = Some(true);
        let sync = serde_json::from_value::<MatrixSyncResponse>(serde_json::json!({
            "next_batch": "s2",
            "rooms": {
                "join": {
                    "!room:matrix.org": {
                        "timeline": {
                            "events": [
                                {
                                    "event_id": "$user-command",
                                    "type": "m.room.message",
                                    "sender": "@enochzzg:matrix.org",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "给我盖一个亭子"
                                    }
                                },
                                {
                                    "event_id": "$controller-reply",
                                    "type": "m.room.message",
                                    "sender": "@enochzzg:matrix.org",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "已开始处理"
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        }))
        .unwrap();
        let mut sent_event_ids = SentEventIds::default();
        sent_event_ids.insert("$controller-reply".to_string());

        let matrix = tool.matrix.as_ref().unwrap();
        let messages = matrix_sync_to_chat_messages(
            &sync,
            &tool,
            matrix,
            "@enochzzg:matrix.org",
            &sent_event_ids,
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "给我盖一个亭子");
    }

    #[test]
    fn matrix_unauthorized_error_is_detected_for_config_hint() {
        assert!(is_matrix_unauthorized_error(
            "whoami returned HTTP 401 Unauthorized"
        ));
        assert!(is_matrix_unauthorized_error(
            "WHOAMI returned http 403 unauthorized"
        ));
        assert!(!is_matrix_unauthorized_error(
            "sync returned HTTP 500 Internal Server Error"
        ));
    }
}
