mod adapter;
mod auth;
mod dtos;
mod errors;
mod socket;
mod state;
mod utils;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use tokio::net::TcpListener;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};

use dtos::{
    CreateRoomRequest, CreateRoomResponse, JoinRoomRequest, JoinRoomResponse, RefreshTokenResponse,
};
use errors::AppError;
use socket::{PlayerSession, handle_socket};
use state::app_state::AppState;

use crate::state::room_state::RoomConfig;
use tracing::info;

const TICK_IN_MS: u64 = 10;
const DEFAULT_ANSWER_WINDOW_IN_MS: u64 = 5000;
const MIN_ANSWER_WINDOW_IN_MS: u64 = 500;
const MAX_ANSWER_WINDOW_IN_MS: u64 = 60000;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let state = AppState::new();
    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(30)
            .burst_size(60)
            .finish()
            .expect("valid rate limit config"),
    );

    let app = Router::new()
        .route("/api/rooms", post(create_room))
        .route("/api/rooms/{room_id}/join", post(join_room))
        .route("/api/rooms/{room_id}/refresh_token", post(token_refresh))
        .route("/ws/{room_id}", get(ws_handler))
        .layer(GovernorLayer::new(governor_conf))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await.expect("bind");
    info!("Web server running on http://{}", addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("serve");
}

async fn create_room(
    State(state): State<AppState>,
    Json(req): Json<CreateRoomRequest>,
) -> Result<(StatusCode, Json<CreateRoomResponse>), AppError> {
    if req.name.trim().is_empty() {
        return Err(AppError::InvalidEmptyName);
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

    let token = room.create_admin(&req.name).await?;

    let response = CreateRoomResponse {
        room_id,
        token,
        answer_window_in_ms,
    };
    Ok((StatusCode::CREATED, Json(response)))
}

async fn join_room(
    Path(room_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<JoinRoomRequest>,
) -> Result<(StatusCode, Json<JoinRoomResponse>), AppError> {
    let requested_name = req.name.trim().to_string();
    if requested_name.is_empty() {
        return Err(AppError::InvalidEmptyName);
    }

    let room = state.get_room(&room_id)?;
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    let (token, role) = room.join(&requested_name, token).await?;
    let response = JoinRoomResponse {
        room_id: room_id.to_string(),
        token,
        answer_window_in_ms: room.answer_window_in_ms(),
        role,
    };
    Ok((StatusCode::OK, Json(response)))
}

async fn token_refresh(
    Path(room_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RefreshTokenResponse>), AppError> {
    let room = state.get_room(&room_id)?;

    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return Err(AppError::AuthRequired);
    };

    let new_token = room.refresh_token(token).await?;

    Ok((
        StatusCode::OK,
        Json(RefreshTokenResponse {
            room_id: room_id.to_string(),
            new_token,
        }),
    ))
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
) -> Result<axum::response::Response, AppError> {
    info!("[WS] Handshake initiated for room: {}", room_id);
    let room = state.get_room(&room_id)?;

    let claims = state.auth().verify(&query.token)?;

    if claims.room_id != room_id {
        return Err(AppError::RoomMismatch);
    }

    if !room.player_matches(claims.player_id, &claims.name) {
        return Err(AppError::UserNotInRoom);
    }

    let session = PlayerSession {
        player_id: claims.player_id,
        name: claims.name,
    };

    info!(
        "[WS] Handshake accepted for player: {} in room: {}",
        session.name, room_id
    );

    Ok(ws
        .on_upgrade(move |socket| handle_socket(socket, room, session))
        .into_response())
}
