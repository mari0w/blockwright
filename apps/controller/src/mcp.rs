use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::{
    domain::types::{
        Blueprint, ChatAttachment, GameAction, MaterialCount, PlayerPosition, WorldScan,
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
                "description": "Explain the safe Blockwright action protocol. Use this before planning Minecraft changes through Blockwright.",
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
                    "nearby_scan": { "type": "object", "description": "Optional WorldScan JSON from the Minecraft execution side." }
                })).with_required(["text"])
            },
            {
                "name": "blockwright_health",
                "description": "Return Blockwright service identity, server name, environment, and Codex status.",
                "inputSchema": object_schema(json!({}))
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

    let result = match name {
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
        "blockwright_list_blueprints" => json!({ "items": state.blueprints.list().await }),
        "blockwright_get_blueprint" => {
            let id = required_string(&arguments, "id")?;
            match state.blueprints.get(&id).await {
                Some(blueprint) => json!(blueprint),
                None => return Err((-32004, format!("blueprint not found: {id}"))),
            }
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
    let nearby_scan = optional_from_value::<WorldScan>(&arguments, "nearby_scan")?;
    let attachments =
        optional_from_value::<Vec<ChatAttachment>>(&arguments, "attachments")?.unwrap_or_default();

    let plan = state
        .planner
        .plan(
            PlannerInput {
                text,
                player: target_player.clone(),
                codex_session_key: Some(format!("mcp:{conversation_id}:{sender}")),
                position,
                nearby_scan,
                attachments,
                progress_id: None,
            },
            &state.blueprints,
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

    let queued_job = if plan.actions.is_empty() {
        None
    } else {
        let job_id = state.jobs.reserve_job_id();
        if has_build_action(&plan.actions) {
            state
                .builds
                .register_planned(
                    job_id.clone(),
                    server_id.clone(),
                    target_player.clone(),
                    plan.summary.clone(),
                    &plan.actions,
                )
                .await
                .map_err(|error| {
                    (
                        -32000,
                        format!("failed to register planned build before enqueue: {error}"),
                    )
                })?;
        }

        Some(
            state
                .jobs
                .enqueue_with_id(
                    job_id,
                    server_id,
                    target_player,
                    plan.summary.clone(),
                    plan.actions.clone(),
                )
                .await,
        )
    };

    Ok(json!({
        "executed": queued_job.is_some(),
        "reply": plan.reply,
        "summary": plan.summary,
        "queued_job": queued_job
    }))
}

fn blockwright_protocol() -> Value {
    json!({
        "boundary": "MCP exposes high-level Blockwright context and validation only. Do not call raw Minecraft setBlock/fill/inventory APIs through MCP.",
        "safe_actions": ["give_item", "place_blocks", "run_command", "chat", "scan_nearby_and_plan"],
        "building_contract": [
            "Blueprint blocks use relative coordinates.",
            "Blockwright chooses the player-facing target from scan data and may prepare the site tastefully.",
            "Blockwright saves build records before Minecraft execution.",
            "Fabric/Paper executes actions through server world APIs and returns verification reports.",
            "Block material state strings are part of consistency, for example minecraft:oak_door[half=lower,facing=south]."
        ],
        "forbidden": [
            "Do not expose arbitrary Minecraft commands.",
            "Do not expose raw setBlock/fill tools.",
            "Do not simulate player inventory clicks or right-click placement."
        ],
        "preferred_flow": [
            "Use tools to inspect blueprints/builds or validate blueprint JSON.",
            "Use blockwright_assistant_message for natural-language assistant behavior; keep execute=false until ready to enqueue.",
            "Return Blockwright action/blueprint JSON through this protocol.",
            "Let Fabric/Paper execute and verify world changes."
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

fn has_build_action(actions: &[GameAction]) -> bool {
    actions
        .iter()
        .any(|action| matches!(action, GameAction::PlaceBlocks { .. }))
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
    use crate::domain::types::{BlueprintBlock, BlueprintSize};

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
    fn tools_list_exposes_safe_blockwright_tools_only() {
        let tools = tools_list_result();
        let encoded = tools.to_string();

        assert!(encoded.contains("blockwright_protocol"));
        assert!(encoded.contains("blockwright_assistant_message"));
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
}
