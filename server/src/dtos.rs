use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Player,
}

#[derive(Deserialize)]
pub struct CreateRoomRequest {
    pub name: String,
    pub answer_window_in_ms: Option<u64>,
}

#[derive(Serialize)]
pub struct CreateRoomResponse {
    pub room_id: String,
    pub token: String,
    pub answer_window_in_ms: u64,
}

#[derive(Deserialize)]
pub struct JoinRoomRequest {
    pub name: Option<String>,
}

#[derive(Serialize)]
pub struct JoinRoomResponse {
    pub token: String,
}

#[derive(Serialize)]
pub struct RefreshTokenResponse {
    pub token: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Buzz,
    StartRound,
    SetAdmin { name: String },
    Kick { name: String },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Accepted { name: String, deadline_in_ms: u64 },
    Participants { participants: Vec<ParticipantInfo> },
    RoundStarted,
    Rejected,
    TimedOut { name: String },
    ActionDenied { reason: String },
    Kicked,
}

#[derive(Serialize)]
pub struct ParticipantInfo {
    pub name: String,
    pub role: Role,
}
