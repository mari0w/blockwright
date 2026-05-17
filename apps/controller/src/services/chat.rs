use crate::domain::types::{ChatAttachment, PlayerPosition};

#[derive(Debug, Clone)]
pub struct IncomingChatMessage {
    pub platform: String,
    pub conversation_id: String,
    pub sender: String,
    pub server_id: Option<String>,
    pub target_player: Option<String>,
    pub text: String,
    pub position: Option<PlayerPosition>,
    pub attachments: Vec<ChatAttachment>,
}
