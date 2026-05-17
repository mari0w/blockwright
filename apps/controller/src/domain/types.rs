use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerPosition {
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yaw: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldScan {
    pub world: String,
    pub center_x: i32,
    pub center_y: i32,
    pub center_z: i32,
    pub radius: u32,
    pub blocks: Vec<WorldScanBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldScanBlock {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub material: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatAttachmentKind {
    Image,
    File,
    Audio,
    Video,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatAttachmentSource {
    Url {
        url: String,
    },
    DingTalkDownloadCode {
        download_code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        picture_download_code: Option<String>,
    },
    LocalPath {
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatAttachment {
    pub kind: ChatAttachmentKind,
    pub source: ChatAttachmentSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockOrigin {
    pub world: Option<String>,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueprintSize {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialCount {
    pub material: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueprintBlock {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub material: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blueprint {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size: BlueprintSize,
    pub materials: Vec<MaterialCount>,
    pub blocks: Vec<BlueprintBlock>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GameAction {
    GiveItem {
        player: Option<String>,
        item: String,
        count: u32,
    },
    PlaceBlocks {
        blueprint_id: Option<String>,
        origin: BlockOrigin,
        blocks: Vec<BlueprintBlock>,
        #[serde(default, skip_serializing_if = "is_false")]
        clear_existing: bool,
    },
    RunCommand {
        command: String,
    },
    Chat {
        message: String,
    },
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameJob {
    pub id: String,
    pub server_id: String,
    pub target_player: Option<String>,
    pub summary: String,
    pub actions: Vec<GameAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResultRequest {
    pub ok: bool,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<JobExecutionReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobExecutionReport {
    pub actions: Vec<ActionExecutionReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionExecutionReport {
    pub action_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blueprint_id: Option<String>,
    pub expected_count: u32,
    pub placed_count: u32,
    pub skipped_existing_count: u32,
    pub skipped_limit_count: u32,
    pub verified_count: u32,
    pub mismatch_count: u32,
    #[serde(default)]
    pub mismatches: Vec<BlockMismatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMismatch {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub expected: String,
    pub actual: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildStatus {
    Planned,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildRecord {
    pub id: String,
    pub server_id: String,
    pub target_player: Option<String>,
    pub summary: String,
    pub status: BuildStatus,
    pub expected_actions: Vec<ExpectedBuildAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<JobExecutionReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedBuildAction {
    pub blueprint_id: Option<String>,
    pub origin: BlockOrigin,
    pub expected_count: u32,
    pub materials: Vec<MaterialCount>,
    pub blocks: Vec<BlueprintBlock>,
}
