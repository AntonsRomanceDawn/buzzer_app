mod adapter;
mod auth;
mod dtos;
mod socket;
mod state;
mod utils;

use std::net::SocketAddr;

use axum::{
    Json, Router,
    extract::{Path, Query, State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use tokio::net::TcpListener;

use dtos::{
    CreateRoomRequest, CreateRoomResponse, JoinRoomRequest, JoinRoomResponse, RefreshTokenResponse,
    Role,
};
use socket::{PlayerSession, handle_socket};
use state::{AppState, RoomConfig};

const TICK_IN_MS: u64 = 10;
const DEFAULT_ANSWER_WINDOW_IN_MS: u64 = 5_000;
const MIN_ANSWER_WINDOW_IN_MS: u64 = 1_000;
const MAX_ANSWER_WINDOW_IN_MS: u64 = 60_000;

#[tokio::main]
async fn main() {
    let state = AppState::new();

    let app = Router::new()
        .route("/api/rooms", post(create_room))
        .route("/api/rooms/:room_id/join", post(join_room))
        .route("/api/rooms/:room_id/refresh_token", post(token_refresh))
        .route("/ws/:room_id", get(ws_handler))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await.expect("bind");
    println!("Web server running on http://{}", addr);
    axum::serve(listener, app).await.expect("serve");
}

async fn create_room(
    State(state): State<AppState>,
    Json(req): Json<CreateRoomRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "invalid_name").into_response();
    }

    let answer_window_in_ms = match req.answer_window_in_ms {
        Some(value) if value < MIN_ANSWER_WINDOW_IN_MS => MIN_ANSWER_WINDOW_IN_MS,
        Some(value) if value > MAX_ANSWER_WINDOW_IN_MS => MAX_ANSWER_WINDOW_IN_MS,
        Some(value) => value,
        None => DEFAULT_ANSWER_WINDOW_IN_MS,
    };

    let (room_id, room) = state.create_room(
        RoomConfig {
            answer_window_in_ms,
        },
        TICK_IN_MS,
    );

    let player_id = match room.insert_player(req.name.clone(), Role::Admin) {
        Some(player_id) => player_id,
        None => return (StatusCode::FORBIDDEN, "full_room").into_response(),
    };

    room.set_admin_id(player_id);

    let token = match state
        .auth()
        .issue(&room_id, player_id, &req.name, Role::Admin)
    {
        Ok(token) => token,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let response = CreateRoomResponse {
        room_id,
        token,
        answer_window_in_ms,
    };
    (StatusCode::CREATED, Json(response)).into_response()
}

async fn join_room(
    Path(room_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<JoinRoomRequest>>,
) -> impl IntoResponse {
    let Some(room) = state.get_room(&room_id) else {
        return (StatusCode::NOT_FOUND, "room_not_found").into_response();
    };

    let token_from_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string);

    if let Some(token) = token_from_header {
        let (rm_id, player_id, name, _role, _iat, _exp) = match state.auth().verify(&token) {
            Ok(c) => (c.room_id, c.player_id, c.name, c.role, c.iat, c.exp),
            Err(_) => return (StatusCode::UNAUTHORIZED, "invalid_token").into_response(),
        };

        if rm_id != room_id {
            return (StatusCode::FORBIDDEN, "room_mismatch").into_response();
        }

        if !room.player_matches(player_id, &name) {
            return (StatusCode::FORBIDDEN, "user_not_in_room").into_response();
        }

        let token = match state.auth().issue(&room_id, player_id, &name, Role::Player) {
            Ok(token) => token,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };

        room.broadcast_participants();
        let response = JoinRoomResponse { token: token };

        return (StatusCode::OK, Json(response)).into_response();
    }

    let Some(Json(req)) = body else {
        return (StatusCode::UNAUTHORIZED, "auth_required").into_response();
    };

    let Some(name) = req.name else {
        return (StatusCode::UNAUTHORIZED, "auth_required").into_response();
    };

    if name.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "invalid_name").into_response();
    }

    if room.name_exists(&name) {
        return (StatusCode::CONFLICT, "name_taken").into_response();
    }

    let player_id = match room.insert_player(name.clone(), Role::Player) {
        Some(player_id) => player_id,
        None => return (StatusCode::CONFLICT, "full_room").into_response(),
    };

    let token = match state.auth().issue(&room_id, player_id, &name, Role::Player) {
        Ok(token) => token,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "auth_failed").into_response(),
    };

    room.broadcast_participants();
    let response = JoinRoomResponse { token: token };

    (StatusCode::OK, Json(response)).into_response()
}

async fn token_refresh(
    Path(room_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(room) = state.get_room(&room_id) else {
        return (StatusCode::NOT_FOUND, "room_not_found").into_response();
    };

    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return (StatusCode::UNAUTHORIZED, "auth_required").into_response();
    };

    let claims = match state.auth().verify(token) {
        Ok(claims) => claims,
        Err(_) => return (StatusCode::UNAUTHORIZED, "invalid_token").into_response(),
    };

    if claims.room_id != room_id {
        return (StatusCode::FORBIDDEN, "room_mismatch").into_response();
    }

    if !room.player_matches(claims.player_id, &claims.name) {
        return (StatusCode::FORBIDDEN, "user_not_in_room").into_response();
    }

    let new_token = match state
        .auth()
        .issue(&room_id, claims.player_id, &claims.name, claims.role)
    {
        Ok(token) => token,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    (
        StatusCode::OK,
        Json(RefreshTokenResponse { token: new_token }),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
struct WsAuthQuery {
    token: String,
}

async fn ws_handler(
    Path(room_id): Path<String>,
    State(state): State<AppState>,
    Query(query): Query<WsAuthQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let Some(room) = state.get_room(&room_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let claims = match state.auth().verify(&query.token) {
        Ok(claims) => claims,
        Err(_) => return StatusCode::UNAUTHORIZED.into_response(),
    };

    if claims.room_id != room_id {
        return StatusCode::FORBIDDEN.into_response();
    }

    if !room.player_matches(claims.player_id, &claims.name) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let session = PlayerSession {
        player_id: claims.player_id,
        name: claims.name,
        role: claims.role,
    };

    ws.on_upgrade(move |socket| handle_socket(socket, room, session))
}
