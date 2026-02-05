use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub enum AppError {
    RoomNotFound,
    InvalidEmptyName,
    NameTaken,
    FullRoom,
    AuthRequired,
    InvalidToken,
    RoomMismatch,
    UserNotInRoom,
    SessionExpired,
    Kicked,
    Internal,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::RoomNotFound => (StatusCode::NOT_FOUND, "room_not_found").into_response(),
            AppError::InvalidEmptyName => {
                (StatusCode::BAD_REQUEST, "invalid_empty_name").into_response()
            }
            AppError::NameTaken => (StatusCode::CONFLICT, "name_taken").into_response(),
            AppError::FullRoom => (StatusCode::CONFLICT, "full_room").into_response(),
            AppError::AuthRequired => (StatusCode::UNAUTHORIZED, "auth_required").into_response(),
            AppError::InvalidToken => (StatusCode::UNAUTHORIZED, "invalid_token").into_response(),
            AppError::RoomMismatch => (StatusCode::FORBIDDEN, "room_mismatch").into_response(),
            AppError::UserNotInRoom => (StatusCode::FORBIDDEN, "user_not_in_room").into_response(),
            AppError::SessionExpired => (StatusCode::FORBIDDEN, "session_expired").into_response(),
            AppError::Kicked => (StatusCode::FORBIDDEN, "kicked").into_response(),
            AppError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}
