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
    pub name: String,
}

#[derive(Serialize)]
pub struct JoinRoomResponse {
    pub room_id: String,
    pub token: String,
    pub answer_window_in_ms: u64,
    pub role: Role,
}

#[derive(Serialize)]
pub struct RefreshTokenResponse {
    pub room_id: String,
    pub new_token: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Buzz,
    StartRound,
    ContinueRound,
    Kick { name: String },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Accepted { name: String },
    Participants { participants: Vec<ParticipantInfo> },
    RoundStarted,
    RoundContinued,
    Rejected,
    TimedOut { name: String },
    ActionDenied { reason: String },
    Kicked,
}

#[derive(Serialize)]
pub struct ParticipantInfo {
    pub name: String,
    pub role: Role,
    pub locked_out: bool,
}
