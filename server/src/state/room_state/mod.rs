use crate::adapter::spawn_room_loop;
use crate::auth::JwtAuth;
use crate::dtos::{ParticipantInfo, Role, ServerMessage};
use crate::errors::AppError;
use core::game::PlayerId;
use dashmap::DashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{mpsc, oneshot};

mod commands;
mod lifecycle;
mod membership;
mod messaging;

const ROOM_CLEANUP_INTERVAL_IN_SECS: u64 = 30 * 60;

pub type RoomId = String;

#[derive(Clone, Copy)]
pub struct RoomConfig {
    pub answer_window_in_ms: u64,
}

pub struct RoomState {
    // id: RoomId,
    room_id: RoomId,
    auth: Arc<JwtAuth>,
    answer_window_in_ms: u64,
    buzz_tx: mpsc::UnboundedSender<PlayerId>,
    routes: Arc<DashMap<PlayerId, mpsc::UnboundedSender<String>>>,
    names_by_id: Arc<DashMap<PlayerId, String>>,
    roles_by_id: Arc<DashMap<PlayerId, Role>>,
    ids_by_name: Arc<DashMap<String, PlayerId>>,
    token_exp_by_id: Arc<DashMap<PlayerId, u64>>,
    command_tx: mpsc::UnboundedSender<RoomCommand>,
    next_id: Mutex<PlayerId>,
    admin_id: Mutex<Option<PlayerId>>,
    reset_flag: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
}

enum RoomCommand {
    CreateAdmin {
        name: String,
        resp: oneshot::Sender<Result<String, AppError>>,
    },
    Join {
        requested_name: String,
        token: Option<String>,
        resp: oneshot::Sender<Result<String, AppError>>,
    },
    RefreshToken {
        token: String,
        resp: oneshot::Sender<Result<String, AppError>>,
    },
    AttachConnection {
        player_id: PlayerId,
        name: String,
        sender: mpsc::UnboundedSender<String>,
        resp: oneshot::Sender<bool>,
    },
    DetachConnection {
        player_id: PlayerId,
    },
    SetAdminByName {
        requester_id: PlayerId,
        name: String,
        resp: oneshot::Sender<bool>,
    },
    KickByName {
        requester_id: PlayerId,
        name: String,
        resp: oneshot::Sender<bool>,
    },
    StartRound {
        requester_id: PlayerId,
    },
    CleanupExpired,
}

impl RoomState {
    pub(super) fn new(
        id: RoomId,
        config: RoomConfig,
        tick_ms: u64,
        auth: Arc<JwtAuth>,
    ) -> Arc<Self> {
        let (buzz_tx, buzz_rx) = mpsc::unbounded_channel::<PlayerId>();
        let routes = Arc::new(DashMap::new());
        let names_by_id = Arc::new(DashMap::new());
        let roles_by_id = Arc::new(DashMap::new());
        let ids_by_name = Arc::new(DashMap::new());
        let token_exp_by_id = Arc::new(DashMap::new());
        let reset_flag = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));
        let (command_tx, command_rx) = mpsc::unbounded_channel::<RoomCommand>();

        spawn_room_loop(
            tick_ms,
            config.answer_window_in_ms,
            buzz_rx,
            Arc::clone(&reset_flag),
            Arc::clone(&shutdown),
            Arc::clone(&routes),
            Arc::clone(&names_by_id),
        );

        let room = Arc::new(Self {
            // id,
            room_id: id,
            auth,
            answer_window_in_ms: config.answer_window_in_ms,
            buzz_tx,
            routes,
            names_by_id,
            roles_by_id,
            ids_by_name,
            token_exp_by_id,
            command_tx,
            next_id: Mutex::new(0),
            admin_id: Mutex::new(None),
            reset_flag,
            shutdown,
        });

        RoomState::spawn_command_loop(Arc::clone(&room), command_rx);
        RoomState::spawn_cleanup(Arc::clone(&room));
        room
    }
}
