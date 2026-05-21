use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader},
    time::{sleep, Duration, Instant},
};

use crate::{
    domain::types::{
        BlockOrigin, Blueprint, BlueprintBlock, BuildRecord, ChatAttachment, GameAction,
        JobResultRequest, MaterialCount, PlayerPosition, PlayerState, WorldScan,
    },
    services::planner::PlannerInput,
    state::AppState,
};

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    fn into_value(self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({ "jsonrpc": "2.0", "id": null }))
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

pub async fn serve_stdio(state: AppState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) => handle_request(&state, request).await,
            Err(error) => Some(error_response(
                Value::Null,
                -32700,
                format!("invalid JSON-RPC request: {error}"),
            )),
        };

        let Some(response) = response else {
            continue;
        };
        let encoded = serde_json::to_vec(&response)?;
        stdout.write_all(&encoded).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}

pub async fn serve_stdio_proxy(
    controller_url: String,
    shared_token: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let endpoint = format!("{}/api/mcp", controller_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let request_value = match serde_json::from_str::<Value>(&line) {
            Ok(value) => value,
            Err(error) => {
                let response = error_response(
                    Value::Null,
                    -32700,
                    format!("invalid JSON-RPC request: {error}"),
                );
                let encoded = serde_json::to_vec(&response)?;
                stdout.write_all(&encoded).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
                continue;
            }
        };

        let mut request = client.post(&endpoint).json(&request_value);
        if let Some(token) = shared_token.as_deref().filter(|value| !value.is_empty()) {
            request = request.header("x-blockwright-token", token);
        }
        let response_value = match request.send().await {
            Ok(response) if response.status().is_success() => response.json::<Value>().await?,
            Ok(response) => error_response(
                request_value.get("id").cloned().unwrap_or(Value::Null),
                -32000,
                format!(
                    "Blockwright controller MCP HTTP 请求失败：{}",
                    response.status()
                ),
            )
            .into_value(),
            Err(error) => error_response(
                request_value.get("id").cloned().unwrap_or(Value::Null),
                -32000,
                format!("无法连接 Blockwright controller MCP：{error}"),
            )
            .into_value(),
        };

        let encoded = serde_json::to_vec(&response_value)?;
        stdout.write_all(&encoded).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}

async fn handle_request(state: &AppState, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
    let Some(id) = request.id else {
        return None;
    };

    let result = match request.method.as_str() {
        "initialize" => Ok(initialize_result()),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list_result()),
        "tools/call" => handle_tools_call(state, request.params).await,
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        method => Err((-32601, format!("unsupported MCP method: {method}"))),
    };

    Some(match result {
        Ok(result) => success_response(id, result),
        Err((code, message)) => error_response(id, code, message),
    })
}

pub async fn handle_json_rpc_value(state: &AppState, request: Value) -> Option<Value> {
    let response = match serde_json::from_value::<JsonRpcRequest>(request) {
        Ok(request) => handle_request(state, request).await?,
        Err(error) => error_response(
            Value::Null,
            -32700,
            format!("invalid JSON-RPC request: {error}"),
        ),
    };
    Some(serde_json::to_value(response).unwrap_or_else(|_| json!({ "jsonrpc": "2.0", "id": null })))
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "blockwright",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn tools_list_result() -> Value {
    json!({
        "tools": [
            {
                "name": "blockwright_protocol",
                "description": "Explain the Blockwright action protocol. Use this before planning Minecraft changes through Blockwright.",
                "inputSchema": object_schema(json!({}))
            },
            {
                "name": "blockwright_assistant_message",
                "description": "Ask Blockwright to handle a natural-language Minecraft request. By default this dry-runs and returns planned actions; set execute=true to enqueue the controlled job for the Minecraft execution plugin.",
                "inputSchema": object_schema(json!({
                    "text": { "type": "string", "description": "Natural-language request, for example build a cabin, give an item, change time, adjust a known build, or explain a plan." },
                    "server_id": { "type": "string", "description": "Minecraft server id. Defaults to configured minecraft.default_server_id." },
                    "target_player": { "type": "string", "description": "Target player name." },
                    "sender": { "type": "string", "description": "Assistant/user identity used for Codex session continuity. Defaults to mcp." },
                    "conversation_id": { "type": "string", "description": "Conversation id for grouping requests. Defaults to local." },
                    "execute": { "type": "boolean", "description": "When false, only return the planned reply/actions. When true, enqueue a job for Fabric/Paper to execute." },
                    "position": { "type": "object", "description": "Optional PlayerPosition JSON." },
                    "player_state": { "type": "object", "description": "Optional PlayerState JSON from blockwright_get_player_state or the Minecraft execution side." },
                    "nearby_scan": { "type": "object", "description": "Optional WorldScan JSON from the Minecraft execution side." }
                })).with_required(["text"])
            },
            {
                "name": "blockwright_health",
                "description": "Return Blockwright service identity, server name, environment, and Codex status.",
                "inputSchema": object_schema(json!({}))
            },
            {
                "name": "blockwright_get_player_state",
                "description": "Read a live Minecraft player's server-side position, selected slot, main hand, off hand, and inventory snapshot through the Fabric/Paper execution plugin.",
                "inputSchema": object_schema(json!({
                    "server_id": { "type": "string", "description": "Minecraft server id. Defaults to configured minecraft.default_server_id." },
                    "target_player": { "type": "string", "description": "Online player name. If omitted, the execution plugin chooses its current/default online player." },
                    "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 60, "description": "How long to wait for the Minecraft plugin to answer. Defaults to 8." }
                }))
            },
            {
                "name": "blockwright_scan_nearby_blocks",
                "description": "Ask the live Minecraft plugin to scan non-air blocks around a player with a given radius and return the WorldScan JSON.",
                "inputSchema": object_schema(json!({
                    "server_id": { "type": "string", "description": "Minecraft server id. Defaults to configured minecraft.default_server_id." },
                    "target_player": { "type": "string", "description": "Online player name. If omitted, the execution plugin chooses its current/default online player." },
                    "radius": { "type": "integer", "minimum": 1, "maximum": 32, "description": "Scan radius in blocks. Defaults to plugin config when omitted." },
                    "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 60, "description": "How long to wait for the Minecraft plugin to answer. Defaults to 10." }
                }))
            },
            {
                "name": "blockwright_give_item",
                "description": "Give an item to a player and make the execution plugin put it visibly in the selected/main hand hotbar slot.",
                "inputSchema": object_schema(json!({
                    "server_id": { "type": "string", "description": "Minecraft server id. Defaults to configured minecraft.default_server_id." },
                    "target_player": { "type": "string", "description": "Target player name." },
                    "item": { "type": "string", "description": "Minecraft namespaced item id, for example minecraft:brick." },
                    "count": { "type": "integer", "minimum": 1, "description": "Item count. Defaults to 1." },
                    "summary": { "type": "string", "description": "Optional short Chinese job summary." }
                })).with_required(["item"])
            },
            {
                "name": "blockwright_place_blocks",
                "description": "Place explicit blocks at a world origin through Fabric/Paper with build record registration. This is the direct block-setting tool.",
                "inputSchema": object_schema(json!({
                    "server_id": { "type": "string", "description": "Minecraft server id. Defaults to configured minecraft.default_server_id." },
                    "target_player": { "type": "string", "description": "Target player name." },
                    "summary": { "type": "string", "description": "Optional short Chinese job/build summary." },
                    "blueprint_id": { "type": "string", "description": "Optional blueprint/build part id." },
                    "origin": { "type": "object", "description": "BlockOrigin JSON. Blocks are placed relative to this world origin." },
                    "blocks": { "type": "array", "description": "Array of BlueprintBlock JSON items using relative coordinates and minecraft: materials." },
                    "clear_existing": { "type": "boolean", "description": "Whether existing blocks may be replaced. Defaults to false." }
                })).with_required(["origin", "blocks"])
            },
            {
                "name": "blockwright_run_command",
                "description": "Run a Minecraft command through the execution plugin without a command whitelist.",
                "inputSchema": object_schema(json!({
                    "server_id": { "type": "string", "description": "Minecraft server id. Defaults to configured minecraft.default_server_id." },
                    "target_player": { "type": "string", "description": "Target player name used as command source/context." },
                    "command": { "type": "string", "description": "Minecraft command without leading slash." },
                    "summary": { "type": "string", "description": "Optional short Chinese job summary." }
                })).with_required(["command"])
            },
            {
                "name": "blockwright_send_chat",
                "description": "Send a chat message to Minecraft through the execution plugin.",
                "inputSchema": object_schema(json!({
                    "server_id": { "type": "string", "description": "Minecraft server id. Defaults to configured minecraft.default_server_id." },
                    "target_player": { "type": "string", "description": "Target player name." },
                    "message": { "type": "string", "description": "Chat message." },
                    "summary": { "type": "string", "description": "Optional short Chinese job summary." }
                })).with_required(["message"])
            },
            {
                "name": "blockwright_list_blueprints",
                "description": "List saved Blockwright blueprints.",
                "inputSchema": object_schema(json!({}))
            },
            {
                "name": "blockwright_get_blueprint",
                "description": "Get one saved Blockwright blueprint by id.",
                "inputSchema": object_schema(json!({
                    "id": { "type": "string", "description": "Blueprint id." }
                })).with_required(["id"])
            },
            {
                "name": "blockwright_save_blueprint",
                "description": "Create or update one saved Blockwright blueprint. Use this when the assistant has designed a reusable blueprint object and wants to persist it before execution.",
                "inputSchema": object_schema(json!({
                    "blueprint": { "type": "object", "description": "Complete Blueprint JSON object. Coordinates must be relative." }
                })).with_required(["blueprint"])
            },
            {
                "name": "blockwright_delete_blueprint",
                "description": "Delete one saved Blockwright blueprint by id.",
                "inputSchema": object_schema(json!({
                    "id": { "type": "string", "description": "Blueprint id." }
                })).with_required(["id"])
            },
            {
                "name": "blockwright_validate_blueprint",
                "description": "Validate a blueprint JSON object against Blockwright's basic contract before it is saved or executed.",
                "inputSchema": object_schema(json!({
                    "blueprint": { "type": "object", "description": "Blueprint JSON object." }
                })).with_required(["blueprint"])
            },
            {
                "name": "blockwright_list_builds",
                "description": "List saved build records. Use to find recent or known Blockwright-generated builds.",
                "inputSchema": object_schema(json!({}))
            },
            {
                "name": "blockwright_get_build",
                "description": "Get one saved build record by id.",
                "inputSchema": object_schema(json!({
                    "id": { "type": "string", "description": "Build record id." }
                })).with_required(["id"])
            },
            {
                "name": "blockwright_delete_build",
                "description": "Delete one saved build record by id. Use carefully; edits should usually create a new saved build record instead of rewriting history.",
                "inputSchema": object_schema(json!({
                    "id": { "type": "string", "description": "Build record id." }
                })).with_required(["id"])
            },
            {
                "name": "blockwright_search_builds_nearby",
                "description": "Search saved build records near a world coordinate. Use this to identify nearby existing builds before editing or explaining them.",
                "inputSchema": object_schema(json!({
                    "world": { "type": "string", "description": "Optional Minecraft world id. When present, only same-world build origins match." },
                    "x": { "type": "number", "description": "World X coordinate." },
                    "y": { "type": "number", "description": "World Y coordinate." },
                    "z": { "type": "number", "description": "World Z coordinate." },
                    "radius": { "type": "number", "minimum": 0, "description": "Maximum distance in blocks. Defaults to 32." }
                })).with_required(["x", "y", "z"])
            },
            {
                "name": "blockwright_enqueue_actions",
                "description": "Enqueue explicit controlled Minecraft actions such as give_item, place_blocks, run_command, or chat. This is the direct tool path when the assistant already knows the exact action data.",
                "inputSchema": object_schema(json!({
                    "server_id": { "type": "string", "description": "Minecraft server id. Defaults to configured minecraft.default_server_id." },
                    "target_player": { "type": "string", "description": "Target player name." },
                    "summary": { "type": "string", "description": "Short Chinese summary for the job/build record." },
                    "actions": { "type": "array", "description": "Array of controlled GameAction JSON objects." }
                })).with_required(["summary", "actions"])
            }
        ]
    })
}

async fn handle_tools_call(state: &AppState, params: Value) -> Result<Value, (i32, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| (-32602, "tools/call requires params.name".to_string()))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let result =
        match name {
            "blockwright_protocol" => blockwright_protocol(),
            "blockwright_assistant_message" => assistant_message(state, arguments).await?,
            "blockwright_health" => json!({
                "ok": true,
                "service": state.config.server.app_name,
                "server_name": state.config.server.name,
                "environment": state.config.server.environment,
                "codex_enabled": state.codex.enabled(),
                "codex_timeout_seconds": state.config.codex.timeout_seconds,
            }),
            "blockwright_get_player_state" => get_player_state(state, arguments).await?,
            "blockwright_scan_nearby_blocks" => scan_nearby_blocks(state, arguments).await?,
            "blockwright_give_item" => give_item(state, arguments).await?,
            "blockwright_place_blocks" => place_blocks(state, arguments).await?,
            "blockwright_run_command" => run_command(state, arguments).await?,
            "blockwright_send_chat" => send_chat(state, arguments).await?,
            "blockwright_list_blueprints" => json!({ "items": state.blueprints.list().await }),
            "blockwright_get_blueprint" => {
                let id = required_string(&arguments, "id")?;
                match state.blueprints.get(&id).await {
                    Some(blueprint) => json!(blueprint),
                    None => return Err((-32004, format!("blueprint not found: {id}"))),
                }
            }
            "blockwright_save_blueprint" => save_blueprint(state, arguments).await?,
            "blockwright_delete_blueprint" => {
                let id = required_string(&arguments, "id")?;
                let deleted =
                    state.blueprints.delete(&id).await.map_err(|error| {
                        (-32000, format!("failed to delete blueprint: {error}"))
                    })?;
                json!({ "ok": true, "deleted": deleted, "id": id })
            }
            "blockwright_validate_blueprint" => {
                let blueprint_value = arguments
                    .get("blueprint")
                    .cloned()
                    .ok_or_else(|| (-32602, "missing argument: blueprint".to_string()))?;
                let blueprint = serde_json::from_value::<Blueprint>(blueprint_value)
                    .map_err(|error| (-32602, format!("invalid blueprint JSON: {error}")))?;
                validate_blueprint(&blueprint)
            }
            "blockwright_list_builds" => json!({ "items": state.builds.list().await }),
            "blockwright_get_build" => {
                let id = required_string(&arguments, "id")?;
                match state.builds.get(&id).await {
                    Some(build) => json!(build),
                    None => return Err((-32004, format!("build record not found: {id}"))),
                }
            }
            "blockwright_delete_build" => {
                let id = required_string(&arguments, "id")?;
                let deleted =
                    state.builds.delete(&id).await.map_err(|error| {
                        (-32000, format!("failed to delete build record: {error}"))
                    })?;
                json!({ "ok": true, "deleted": deleted, "id": id })
            }
            "blockwright_search_builds_nearby" => search_builds_nearby(state, arguments).await?,
            "blockwright_enqueue_actions" => enqueue_actions(state, arguments).await?,
            tool => return Err((-32601, format!("unknown Blockwright MCP tool: {tool}"))),
        };

    Ok(tool_result(result))
}

async fn assistant_message(state: &AppState, arguments: Value) -> Result<Value, (i32, String)> {
    let text = required_string(&arguments, "text")?;
    let sender = optional_string(&arguments, "sender").unwrap_or_else(|| "mcp".to_string());
    let conversation_id =
        optional_string(&arguments, "conversation_id").unwrap_or_else(|| "local".to_string());
    let server_id = optional_string(&arguments, "server_id")
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let target_player = optional_string(&arguments, "target_player");
    let execute = arguments
        .get("execute")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let position = optional_from_value::<PlayerPosition>(&arguments, "position")?;
    let player_state = optional_from_value::<PlayerState>(&arguments, "player_state")?;
    let nearby_scan = optional_from_value::<WorldScan>(&arguments, "nearby_scan")?;
    let attachments =
        optional_from_value::<Vec<ChatAttachment>>(&arguments, "attachments")?.unwrap_or_default();

    let plan = state
        .planner
        .plan_with_context_stores(
            PlannerInput {
                text,
                player: target_player.clone(),
                codex_session_key: Some(format!("mcp:{conversation_id}:{sender}")),
                position,
                player_state,
                nearby_scan,
                attachments,
                progress_id: None,
            },
            &state.blueprints,
            Some(&state.builds),
        )
        .await;

    if !execute {
        return Ok(json!({
            "executed": false,
            "reply": plan.reply,
            "summary": plan.summary,
            "actions": plan.actions,
            "note": "Dry run only. Call again with execute=true to enqueue this kind of controlled Blockwright job."
        }));
    }

    let queued_job = enqueue_controlled_actions(
        state,
        server_id,
        target_player,
        plan.summary.clone(),
        plan.actions.clone(),
    )
    .await?;

    Ok(json!({
        "executed": queued_job.is_some(),
        "reply": plan.reply,
        "summary": plan.summary,
        "queued_job": queued_job
    }))
}

async fn give_item(state: &AppState, arguments: Value) -> Result<Value, (i32, String)> {
    let server_id = optional_string(&arguments, "server_id")
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let target_player = optional_target_player(&arguments);
    let item = required_string(&arguments, "item")?;
    let count = optional_u32(&arguments, "count")?.unwrap_or(1).max(1);
    let summary = optional_string(&arguments, "summary")
        .unwrap_or_else(|| format!("发放物品 {item} x{count}"));
    let queued_job = enqueue_controlled_actions(
        state,
        server_id,
        target_player.clone(),
        summary.clone(),
        vec![GameAction::GiveItem {
            player: target_player,
            item,
            count,
        }],
    )
    .await?;

    Ok(json!({
        "ok": queued_job.is_some(),
        "summary": summary,
        "queued_job": queued_job,
    }))
}

async fn place_blocks(state: &AppState, arguments: Value) -> Result<Value, (i32, String)> {
    let server_id = optional_string(&arguments, "server_id")
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let target_player = optional_target_player(&arguments);
    let origin = required_from_value::<BlockOrigin>(&arguments, "origin")?;
    let blocks = required_from_value::<Vec<BlueprintBlock>>(&arguments, "blocks")?;
    if blocks.is_empty() {
        return Err((-32602, "blocks must not be empty".to_string()));
    }
    let blueprint_id = optional_string(&arguments, "blueprint_id");
    let clear_existing = arguments
        .get("clear_existing")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let summary = optional_string(&arguments, "summary").unwrap_or_else(|| {
        blueprint_id
            .as_deref()
            .map(|id| format!("放置方块 {id}"))
            .unwrap_or_else(|| format!("放置 {} 个方块", blocks.len()))
    });
    let queued_job = enqueue_controlled_actions(
        state,
        server_id,
        target_player,
        summary.clone(),
        vec![GameAction::PlaceBlocks {
            blueprint_id,
            origin,
            blocks,
            clear_existing,
        }],
    )
    .await?;

    Ok(json!({
        "ok": queued_job.is_some(),
        "summary": summary,
        "queued_job": queued_job,
    }))
}

async fn run_command(state: &AppState, arguments: Value) -> Result<Value, (i32, String)> {
    let server_id = optional_string(&arguments, "server_id")
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let target_player = optional_target_player(&arguments);
    let command = required_string(&arguments, "command")?;
    let summary =
        optional_string(&arguments, "summary").unwrap_or_else(|| format!("执行指令 /{command}"));
    let queued_job = enqueue_controlled_actions(
        state,
        server_id,
        target_player,
        summary.clone(),
        vec![GameAction::RunCommand { command }],
    )
    .await?;

    Ok(json!({
        "ok": queued_job.is_some(),
        "summary": summary,
        "queued_job": queued_job,
    }))
}

async fn send_chat(state: &AppState, arguments: Value) -> Result<Value, (i32, String)> {
    let server_id = optional_string(&arguments, "server_id")
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let target_player = optional_target_player(&arguments);
    let message = required_string(&arguments, "message")?;
    let summary =
        optional_string(&arguments, "summary").unwrap_or_else(|| "发送聊天消息".to_string());
    let queued_job = enqueue_controlled_actions(
        state,
        server_id,
        target_player,
        summary.clone(),
        vec![GameAction::Chat { message }],
    )
    .await?;

    Ok(json!({
        "ok": queued_job.is_some(),
        "summary": summary,
        "queued_job": queued_job,
    }))
}

async fn save_blueprint(state: &AppState, arguments: Value) -> Result<Value, (i32, String)> {
    let blueprint_value = arguments
        .get("blueprint")
        .cloned()
        .ok_or_else(|| (-32602, "missing argument: blueprint".to_string()))?;
    let blueprint = serde_json::from_value::<Blueprint>(blueprint_value)
        .map_err(|error| (-32602, format!("invalid blueprint JSON: {error}")))?;
    let validation = validate_blueprint(&blueprint);
    if !validation
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err((-32602, format!("blueprint validation failed: {validation}")));
    }
    let saved = state
        .blueprints
        .save(blueprint)
        .await
        .map_err(|error| (-32000, format!("failed to save blueprint: {error}")))?;
    Ok(json!({
        "ok": true,
        "blueprint": saved,
    }))
}

async fn search_builds_nearby(state: &AppState, arguments: Value) -> Result<Value, (i32, String)> {
    let world = optional_string(&arguments, "world");
    let x = required_f64(&arguments, "x")?;
    let y = required_f64(&arguments, "y")?;
    let z = required_f64(&arguments, "z")?;
    let radius = optional_f64(&arguments, "radius")?.unwrap_or(32.0).max(0.0);
    let mut matches = state
        .builds
        .list()
        .await
        .into_iter()
        .filter_map(|record| nearby_build_match(record, world.as_deref(), x, y, z, radius))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        left.get("distance_blocks")
            .and_then(Value::as_f64)
            .unwrap_or(f64::MAX)
            .total_cmp(
                &right
                    .get("distance_blocks")
                    .and_then(Value::as_f64)
                    .unwrap_or(f64::MAX),
            )
    });
    Ok(json!({
        "ok": true,
        "items": matches,
    }))
}

fn nearby_build_match(
    record: BuildRecord,
    world: Option<&str>,
    x: f64,
    y: f64,
    z: f64,
    radius: f64,
) -> Option<Value> {
    let (nearest_origin, distance) = record
        .expected_actions
        .iter()
        .filter_map(|action| {
            if let (Some(expected_world), Some(origin_world)) =
                (world, action.origin.world.as_deref())
            {
                if expected_world != origin_world {
                    return None;
                }
            }
            let dx = action.origin.x as f64 - x;
            let dy = action.origin.y as f64 - y;
            let dz = action.origin.z as f64 - z;
            let distance = ((dx * dx) + (dy * dy) + (dz * dz)).sqrt();
            Some((action.origin.clone(), distance))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))?;
    if distance > radius {
        return None;
    }
    Some(json!({
        "id": record.id,
        "server_id": record.server_id,
        "target_player": record.target_player,
        "summary": record.summary,
        "status": record.status,
        "nearest_action_origin": nearest_origin,
        "distance_blocks": (distance * 100.0).round() / 100.0,
        "action_count": record.expected_actions.len(),
    }))
}

async fn enqueue_actions(state: &AppState, arguments: Value) -> Result<Value, (i32, String)> {
    let server_id = optional_string(&arguments, "server_id")
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let target_player = optional_target_player(&arguments);
    let summary = required_string(&arguments, "summary")?;
    let actions_value = arguments
        .get("actions")
        .cloned()
        .ok_or_else(|| (-32602, "missing argument: actions".to_string()))?;
    let actions = serde_json::from_value::<Vec<GameAction>>(actions_value)
        .map_err(|error| (-32602, format!("invalid actions JSON: {error}")))?;
    if actions.is_empty() {
        return Err((-32602, "actions must not be empty".to_string()));
    }
    if let Some(action_type) = actions.iter().find_map(disallowed_direct_action_type) {
        return Err((
            -32602,
            format!("action type `{action_type}` is not accepted by blockwright_enqueue_actions; use the dedicated query/planning tool instead"),
        ));
    }

    let queued_job =
        enqueue_controlled_actions(state, server_id, target_player, summary, actions).await?;
    Ok(json!({
        "ok": queued_job.is_some(),
        "queued_job": queued_job,
    }))
}

async fn enqueue_controlled_actions(
    state: &AppState,
    server_id: String,
    target_player: Option<String>,
    summary: String,
    actions: Vec<GameAction>,
) -> Result<Option<crate::domain::types::GameJob>, (i32, String)> {
    if actions.is_empty() {
        return Ok(None);
    }
    let job_id = state.jobs.reserve_job_id();
    if has_build_action(&actions) {
        state
            .builds
            .register_planned(
                job_id.clone(),
                server_id.clone(),
                target_player.clone(),
                summary.clone(),
                &actions,
            )
            .await
            .map_err(|error| {
                (
                    -32000,
                    format!("failed to register planned build before enqueue: {error}"),
                )
            })?;
    }

    Ok(Some(
        state
            .jobs
            .enqueue_with_id(job_id, server_id, target_player, summary, actions)
            .await,
    ))
}

struct LiveQueryResult {
    job_id: String,
    result: JobResultRequest,
}

async fn get_player_state(state: &AppState, arguments: Value) -> Result<Value, (i32, String)> {
    let server_id = optional_string(&arguments, "server_id")
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let target_player = optional_target_player(&arguments);
    let timeout_seconds = optional_u64(&arguments, "timeout_seconds")?
        .unwrap_or(8)
        .clamp(1, 60);
    let query = live_query(
        state,
        server_id.clone(),
        target_player.clone(),
        "读取玩家手持物和物品栏".to_string(),
        vec![GameAction::GetPlayerState {
            player: target_player.clone(),
        }],
        timeout_seconds,
    )
    .await?;
    let player_state = query.result.player_state.ok_or_else(|| {
        (
            -32002,
            "Minecraft 执行端已回复，但没有返回玩家状态。请确认 Fabric/Paper 插件已更新。"
                .to_string(),
        )
    })?;

    Ok(json!({
        "ok": true,
        "job_id": query.job_id,
        "server_id": server_id,
        "target_player": target_player,
        "player_state": player_state,
    }))
}

async fn scan_nearby_blocks(state: &AppState, arguments: Value) -> Result<Value, (i32, String)> {
    let server_id = optional_string(&arguments, "server_id")
        .unwrap_or_else(|| state.config.minecraft.default_server_id.clone());
    let target_player = optional_target_player(&arguments);
    let radius = optional_u32(&arguments, "radius")?.unwrap_or(0).min(32);
    let timeout_seconds = optional_u64(&arguments, "timeout_seconds")?
        .unwrap_or(10)
        .clamp(1, 60);
    let query = live_query(
        state,
        server_id.clone(),
        target_player.clone(),
        "扫描玩家附近方块".to_string(),
        vec![GameAction::ScanNearby {
            player: target_player.clone(),
            radius,
        }],
        timeout_seconds,
    )
    .await?;
    let nearby_scan = query.result.nearby_scan.ok_or_else(|| {
        (
            -32002,
            "Minecraft 执行端已回复，但没有返回附近方块扫描。请确认 Fabric/Paper 插件已更新。"
                .to_string(),
        )
    })?;

    Ok(json!({
        "ok": true,
        "job_id": query.job_id,
        "server_id": server_id,
        "target_player": target_player,
        "nearby_scan": nearby_scan,
    }))
}

async fn live_query(
    state: &AppState,
    server_id: String,
    target_player: Option<String>,
    summary: String,
    actions: Vec<GameAction>,
    timeout_seconds: u64,
) -> Result<LiveQueryResult, (i32, String)> {
    let job = state
        .jobs
        .enqueue(server_id.clone(), target_player.clone(), summary, actions)
        .await;
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);

    loop {
        if let Some(status) = state.jobs.status(&job.id).await {
            if matches!(
                status.phase,
                crate::services::job_queue::JobQueuePhase::Succeeded
                    | crate::services::job_queue::JobQueuePhase::Failed
            ) {
                let Some(result) = status.result else {
                    return Err((
                        -32002,
                        format!("Minecraft 查询任务 {} 已结束，但没有回写结果。", job.id),
                    ));
                };
                if !result.ok {
                    return Err((
                        -32000,
                        result
                            .message
                            .clone()
                            .unwrap_or_else(|| format!("Minecraft 查询任务 {} 执行失败。", job.id)),
                    ));
                }
                return Ok(LiveQueryResult {
                    job_id: job.id,
                    result,
                });
            }
        }

        if Instant::now() >= deadline {
            return Err((
                -32001,
                format!(
                    "等待 Minecraft 插件返回查询结果超时：server_id={server_id}，job_id={}。请确认 Fabric/Paper 已在线轮询 controller。",
                    job.id
                ),
            ));
        }
        sleep(Duration::from_millis(200)).await;
    }
}

fn blockwright_protocol() -> Value {
    json!({
        "boundary": "MCP is the pure tool surface for the Minecraft assistant: read live state, manage blueprints/build records, and enqueue controlled actions. Minecraft commands are passed through run_command without a command whitelist.",
        "actions": ["give_item", "place_blocks", "run_command", "chat", "scan_nearby_and_plan"],
        "live_read_tools": ["blockwright_get_player_state", "blockwright_scan_nearby_blocks"],
        "data_tools": ["blockwright_list_blueprints", "blockwright_get_blueprint", "blockwright_save_blueprint", "blockwright_delete_blueprint", "blockwright_list_builds", "blockwright_get_build", "blockwright_delete_build", "blockwright_search_builds_nearby"],
        "write_tools": ["blockwright_give_item", "blockwright_place_blocks", "blockwright_run_command", "blockwright_send_chat", "blockwright_enqueue_actions"],
        "building_contract": [
            "Blueprint blocks use relative coordinates.",
            "The assistant chooses the player-facing target from scan data and skills; controller only validates and executes the protocol.",
            "Blockwright saves build records before Minecraft execution.",
            "Fabric/Paper executes actions through server world APIs and returns execution reports.",
            "Block material state strings are part of consistency, for example minecraft:oak_door[half=lower,facing=south]."
        ],
        "forbidden": [
            "Do not expose raw inventory-click APIs.",
            "Do not simulate player inventory clicks or right-click placement."
        ],
        "preferred_flow": [
            "Use tools to inspect, save, update, delete, or search blueprints/build records instead of relying on conversation memory.",
            "Use live read tools for current player hand/inventory/position or nearby world blocks instead of guessing.",
            "Use direct write tools such as blockwright_give_item or blockwright_place_blocks when the exact operation is known.",
            "Use blockwright_enqueue_actions when exact controlled action data is already known.",
            "Use blockwright_assistant_message for the compatibility natural-language bridge; keep execute=false until ready to enqueue.",
            "Let Fabric/Paper execute world changes."
        ]
    })
}

fn validate_blueprint(blueprint: &Blueprint) -> Value {
    let mut issues = Vec::<String>::new();

    if blueprint.id.trim().is_empty() {
        issues.push("blueprint id is empty".to_string());
    }
    if blueprint.blocks.is_empty() {
        issues.push("blueprint has no blocks".to_string());
    }
    if blueprint
        .blocks
        .iter()
        .any(|block| !block.material.starts_with("minecraft:"))
    {
        issues.push("all block materials must use the minecraft: namespace".to_string());
    }
    if blueprint.blocks.iter().any(|block| block.y < 0) {
        issues.push(
            "normal blueprints should not use negative relative y unless explicitly underground"
                .to_string(),
        );
    }

    let actual_materials = material_counts(&blueprint.blocks);
    if normalized_materials(&blueprint.materials) != actual_materials {
        issues.push("materials counts do not match blocks".to_string());
    }

    json!({
        "ok": issues.is_empty(),
        "issues": issues,
        "block_count": blueprint.blocks.len(),
        "material_count": blueprint.materials.len()
    })
}

fn material_counts(blocks: &[crate::domain::types::BlueprintBlock]) -> Vec<(String, u32)> {
    let mut counts = std::collections::HashMap::<String, u32>::new();
    for block in blocks {
        *counts.entry(block.material.clone()).or_default() += 1;
    }
    let mut items = counts.into_iter().collect::<Vec<_>>();
    items.sort_by(|left, right| left.0.cmp(&right.0));
    items
}

fn normalized_materials(materials: &[MaterialCount]) -> Vec<(String, u32)> {
    let mut items = materials
        .iter()
        .map(|item| (item.material.clone(), item.count))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.0.cmp(&right.0));
    items
}

fn required_string(arguments: &Value, name: &str) -> Result<String, (i32, String)> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| (-32602, format!("missing string argument: {name}")))
}

fn optional_string(arguments: &Value, name: &str) -> Option<String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn optional_target_player(arguments: &Value) -> Option<String> {
    optional_string(arguments, "target_player").or_else(|| optional_string(arguments, "player"))
}

fn optional_u32(arguments: &Value, name: &str) -> Result<Option<u32>, (i32, String)> {
    let Some(value) = arguments.get(name) else {
        return Ok(None);
    };
    let Some(number) = value.as_u64() else {
        return Err((-32602, format!("invalid integer argument: {name}")));
    };
    u32::try_from(number)
        .map(Some)
        .map_err(|_| (-32602, format!("integer argument out of range: {name}")))
}

fn optional_u64(arguments: &Value, name: &str) -> Result<Option<u64>, (i32, String)> {
    let Some(value) = arguments.get(name) else {
        return Ok(None);
    };
    value
        .as_u64()
        .map(Some)
        .ok_or_else(|| (-32602, format!("invalid integer argument: {name}")))
}

fn required_f64(arguments: &Value, name: &str) -> Result<f64, (i32, String)> {
    arguments
        .get(name)
        .and_then(Value::as_f64)
        .ok_or_else(|| (-32602, format!("missing number argument: {name}")))
}

fn optional_f64(arguments: &Value, name: &str) -> Result<Option<f64>, (i32, String)> {
    let Some(value) = arguments.get(name) else {
        return Ok(None);
    };
    value
        .as_f64()
        .map(Some)
        .ok_or_else(|| (-32602, format!("invalid number argument: {name}")))
}

fn optional_from_value<T: serde::de::DeserializeOwned>(
    arguments: &Value,
    name: &str,
) -> Result<Option<T>, (i32, String)> {
    let Some(value) = arguments.get(name) else {
        return Ok(None);
    };
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|error| (-32602, format!("invalid argument `{name}`: {error}")))
}

fn required_from_value<T: serde::de::DeserializeOwned>(
    arguments: &Value,
    name: &str,
) -> Result<T, (i32, String)> {
    let value = arguments
        .get(name)
        .cloned()
        .ok_or_else(|| (-32602, format!("missing argument: {name}")))?;
    serde_json::from_value(value)
        .map_err(|error| (-32602, format!("invalid argument `{name}`: {error}")))
}

fn has_build_action(actions: &[GameAction]) -> bool {
    actions
        .iter()
        .any(|action| matches!(action, GameAction::PlaceBlocks { .. }))
}

fn disallowed_direct_action_type(action: &GameAction) -> Option<&'static str> {
    match action {
        GameAction::ScanNearbyAndPlan { .. } => Some("scan_nearby_and_plan"),
        GameAction::GetPlayerState { .. } => Some("get_player_state"),
        GameAction::ScanNearby { .. } => Some("scan_nearby"),
        GameAction::GiveItem { .. }
        | GameAction::PlaceBlocks { .. }
        | GameAction::RunCommand { .. }
        | GameAction::Chat { .. } => None,
    }
}

fn tool_result(value: Value) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
            }
        ],
        "isError": false
    })
}

fn object_schema(properties: Value) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false
    })
}

trait JsonSchemaExt {
    fn with_required(self, required: impl IntoIterator<Item = &'static str>) -> Self;
}

impl JsonSchemaExt for Value {
    fn with_required(mut self, required: impl IntoIterator<Item = &'static str>) -> Self {
        if let Some(object) = self.as_object_mut() {
            object.insert(
                "required".to_string(),
                Value::Array(
                    required
                        .into_iter()
                        .map(|value| Value::String(value.to_string()))
                        .collect(),
                ),
            );
        }
        self
    }
}

fn success_response(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn error_response(id: Value, code: i32, message: String) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError { code, message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use crate::{
        config::{
            AppConfig, ChatConfig, CodexConfig, MinecraftConfig, SecurityConfig, ServerConfig,
            StorageConfig,
        },
        domain::types::{BlueprintBlock, BlueprintSize, BuildStatus},
        state::AppState,
    };

    static NEXT_DIR_ID: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(name: &str) -> PathBuf {
        let number = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "blockwright-mcp-{name}-{}-{number}",
            std::process::id()
        ))
    }

    async fn test_state(name: &str) -> AppState {
        AppState::new(AppConfig {
            server: ServerConfig {
                name: "local".to_string(),
                environment: "test".to_string(),
                app_name: "blockwright-controller".to_string(),
                host: "127.0.0.1".to_string(),
                port: 8765,
            },
            storage: StorageConfig {
                data_dir: temp_dir(name),
            },
            minecraft: MinecraftConfig {
                default_server_id: "hmcl-lan".to_string(),
            },
            security: SecurityConfig {
                shared_token: "test-token".to_string(),
                require_token: false,
            },
            codex: CodexConfig {
                enabled: false,
                command: "codex".to_string(),
                timeout_seconds: 1800,
            },
            chat: ChatConfig {
                config_path: temp_dir(name).join("chat.local.yaml"),
                env_path: temp_dir(name).join(".env"),
            },
        })
        .await
        .unwrap()
    }

    fn valid_blueprint() -> Blueprint {
        Blueprint {
            id: "tiny-room".to_string(),
            name: "小房间".to_string(),
            description: "测试蓝图".to_string(),
            size: BlueprintSize {
                width: 1,
                height: 1,
                depth: 1,
            },
            materials: vec![MaterialCount {
                material: "minecraft:oak_planks".to_string(),
                count: 1,
            }],
            blocks: vec![BlueprintBlock {
                x: 0,
                y: 0,
                z: 0,
                material: "minecraft:oak_planks".to_string(),
            }],
            tags: vec!["room".to_string()],
        }
    }

    #[test]
    fn tools_list_exposes_blockwright_tools_only() {
        let tools = tools_list_result();
        let encoded = tools.to_string();

        assert!(encoded.contains("blockwright_protocol"));
        assert!(encoded.contains("blockwright_assistant_message"));
        assert!(encoded.contains("blockwright_get_player_state"));
        assert!(encoded.contains("blockwright_scan_nearby_blocks"));
        assert!(encoded.contains("blockwright_give_item"));
        assert!(encoded.contains("blockwright_place_blocks"));
        assert!(encoded.contains("blockwright_run_command"));
        assert!(encoded.contains("blockwright_send_chat"));
        assert!(encoded.contains("blockwright_save_blueprint"));
        assert!(encoded.contains("blockwright_delete_blueprint"));
        assert!(encoded.contains("blockwright_delete_build"));
        assert!(encoded.contains("blockwright_search_builds_nearby"));
        assert!(encoded.contains("blockwright_enqueue_actions"));
        assert!(encoded.contains("blockwright_validate_blueprint"));
        assert!(!encoded.contains("setBlock"));
        assert!(!encoded.contains("fill"));
    }

    #[test]
    fn validates_blueprint_material_counts() {
        let result = validate_blueprint(&valid_blueprint());

        assert_eq!(result["ok"], true);
        assert_eq!(result["block_count"], 1);
    }

    #[test]
    fn validation_reports_mismatched_materials() {
        let mut blueprint = valid_blueprint();
        blueprint.materials[0].count = 2;

        let result = validate_blueprint(&blueprint);

        assert_eq!(result["ok"], false);
        assert!(result["issues"][0]
            .as_str()
            .unwrap()
            .contains("materials counts"));
    }

    #[test]
    fn nearby_build_match_filters_by_world_and_radius() {
        let record = BuildRecord {
            id: "job-1".to_string(),
            server_id: "hmcl-lan".to_string(),
            target_player: Some("Steve".to_string()),
            summary: "测试建筑".to_string(),
            status: crate::domain::types::BuildStatus::Succeeded,
            expected_actions: vec![crate::domain::types::ExpectedBuildAction {
                blueprint_id: Some("house".to_string()),
                origin: crate::domain::types::BlockOrigin {
                    world: Some("minecraft:overworld".to_string()),
                    x: 10,
                    y: 64,
                    z: 20,
                },
                expected_count: 1,
                materials: Vec::new(),
                blocks: Vec::new(),
            }],
            result: None,
            message: None,
        };

        let hit = nearby_build_match(
            record.clone(),
            Some("minecraft:overworld"),
            11.0,
            64.0,
            20.0,
            2.0,
        )
        .unwrap();
        assert_eq!(hit["id"], "job-1");

        assert!(nearby_build_match(
            record.clone(),
            Some("minecraft:the_nether"),
            11.0,
            64.0,
            20.0,
            2.0
        )
        .is_none());
        assert!(
            nearby_build_match(record, Some("minecraft:overworld"), 30.0, 64.0, 20.0, 2.0)
                .is_none()
        );
    }

    #[tokio::test]
    async fn give_item_tool_enqueues_visible_hand_delivery() {
        let state = test_state("give-item").await;

        let result = give_item(
            &state,
            json!({
                "target_player": "Charles",
                "item": "minecraft:brick",
                "count": 64
            }),
        )
        .await
        .unwrap();

        let job_id = result["queued_job"]["id"].as_str().unwrap();
        let status = state.jobs.status(job_id).await.unwrap();
        let job = status.job.unwrap();
        assert_eq!(job.server_id, "hmcl-lan");
        assert_eq!(job.target_player.as_deref(), Some("Charles"));
        assert!(matches!(
            &job.actions[0],
            GameAction::GiveItem { player, item, count }
                if player.as_deref() == Some("Charles")
                    && item == "minecraft:brick"
                    && *count == 64
        ));
    }

    #[tokio::test]
    async fn place_blocks_tool_registers_build_before_enqueue() {
        let state = test_state("place-blocks").await;

        let result = place_blocks(
            &state,
            json!({
                "target_player": "Charles",
                "summary": "放置测试方块",
                "blueprint_id": "test-tower",
                "origin": {"world": "minecraft:overworld", "x": 10, "y": 64, "z": 20},
                "blocks": [
                    {"x": 0, "y": 0, "z": 0, "material": "minecraft:stone"}
                ],
                "clear_existing": true
            }),
        )
        .await
        .unwrap();

        let job_id = result["queued_job"]["id"].as_str().unwrap();
        let record = state.builds.get(job_id).await.unwrap();
        assert_eq!(record.status, BuildStatus::Planned);
        assert_eq!(record.summary, "放置测试方块");
        assert_eq!(record.target_player.as_deref(), Some("Charles"));
        assert_eq!(record.expected_actions.len(), 1);
        assert_eq!(
            record.expected_actions[0].blueprint_id.as_deref(),
            Some("test-tower")
        );
        assert_eq!(record.expected_actions[0].expected_count, 1);

        let job = state.jobs.pop_next("hmcl-lan").await.unwrap();
        assert_eq!(job.id, job_id);
        assert!(matches!(
            &job.actions[0],
            GameAction::PlaceBlocks {
                blueprint_id,
                clear_existing: true,
                ..
            } if blueprint_id.as_deref() == Some("test-tower")
        ));
    }
}
