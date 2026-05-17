use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerPosition {
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
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
    },
    Chat {
        message: String,
    },
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
}
