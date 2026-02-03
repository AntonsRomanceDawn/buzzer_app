use crate::utils::time::now_seconds;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use dashmap::DashMap;
use rand::{Rng, RngCore, distributions::Alphanumeric};

use tokio::sync::mpsc;

use core::game::PlayerId;

use crate::adapter::spawn_room_loop;
use crate::auth::JwtAuth;
use crate::dtos::{ParticipantInfo, Role, ServerMessage};

pub type RoomId = String;

#[derive(Clone, Copy)]
pub struct RoomConfig {
    pub answer_window_in_ms: u64,
}

const CLEANUP_TTL_IN_SECS: u64 = 30 * 60;
const CLEANUP_INTERVAL_IN_SECS: u64 = 60;
const ROOM_CLEANUP_INTERVAL_IN_SECS: u64 = 60;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    rooms: DashMap<RoomId, Arc<RoomState>>,
    auth: Arc<JwtAuth>,
}

impl AppState {
    pub fn new() -> Self {
        let mut secret = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut secret);
        let auth = Arc::new(JwtAuth::new(&secret, CLEANUP_TTL_IN_SECS));
        let inner = Arc::new(AppStateInner {
            rooms: DashMap::new(),
            auth,
        });
        Self::spawn_room_cleanup(Arc::clone(&inner));
        Self { inner }
    }

    pub fn create_room(&self, config: RoomConfig, tick_in_ms: u64) -> (RoomId, Arc<RoomState>) {
        let room_id = self.create_random_room_id();
        let room = RoomState::new(room_id.clone(), config, tick_in_ms);
        self.inner.rooms.insert(room_id.clone(), Arc::clone(&room));
        (room_id, room)
    }

    pub fn get_room(&self, room_id: &str) -> Option<Arc<RoomState>> {
        self.inner
            .rooms
            .get(room_id)
            .map(|entry| Arc::clone(entry.value()))
    }

    pub fn auth(&self) -> Arc<JwtAuth> {
        Arc::clone(&self.inner.auth)
    }

    fn create_random_room_id(&self) -> RoomId {
        let mut rng = rand::thread_rng();
        let id: String = (&mut rng)
            .sample_iter(Alphanumeric)
            .take(12)
            .map(char::from)
            .collect();
        id
    }

    fn spawn_room_cleanup(inner: Arc<AppStateInner>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
                ROOM_CLEANUP_INTERVAL_IN_SECS,
            ));
            loop {
                interval.tick().await;
                let mut to_remove = Vec::new();
                for entry in inner.rooms.iter() {
                    if !entry.value().admin_present() {
                        to_remove.push(entry.key().clone());
                    }
                }
                for room_id in to_remove {
                    if let Some((_, room)) = inner.rooms.remove(&room_id) {
                        room.shutdown();
                    }
                }
            }
        });
    }
}

pub struct RoomState {
    id: RoomId,
    buzz_tx: mpsc::UnboundedSender<PlayerId>,
    routes: Arc<DashMap<PlayerId, mpsc::UnboundedSender<String>>>,
    names_by_id: Arc<DashMap<PlayerId, String>>,
    roles_by_id: Arc<DashMap<PlayerId, Role>>,
    ids_by_name: Arc<DashMap<String, PlayerId>>,
    last_disconnect: Arc<DashMap<PlayerId, u64>>,
    next_id: Mutex<PlayerId>,
    admin_id: Mutex<Option<PlayerId>>,
    reset_flag: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
}

impl RoomState {
    fn new(id: RoomId, config: RoomConfig, tick_ms: u64) -> Arc<Self> {
        let (buzz_tx, buzz_rx) = mpsc::unbounded_channel::<PlayerId>();
        let routes = Arc::new(DashMap::new());
        let names_by_id = Arc::new(DashMap::new());
        let roles_by_id = Arc::new(DashMap::new());
        let ids_by_name = Arc::new(DashMap::new());
        let last_disconnect = Arc::new(DashMap::new());
        let reset_flag = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));

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
            id,
            buzz_tx,
            routes,
            names_by_id,
            roles_by_id,
            ids_by_name,
            last_disconnect,
            next_id: Mutex::new(0),
            admin_id: Mutex::new(None),
            reset_flag,
            shutdown,
        });

        RoomState::spawn_cleanup(Arc::clone(&room));
        room
    }

    pub fn name_exists(&self, name: &str) -> bool {
        self.ids_by_name.contains_key(name)
    }

    pub fn insert_player(&self, name: String, role: Role) -> Option<PlayerId> {
        let player_id = {
            let mut next_id = self.next_id.lock().expect("next_id lock");
            let id = *next_id;
            if id >= 128 as usize {
                return None;
            }
            *next_id = next_id.wrapping_add(1);
            id
        };

        self.ids_by_name.insert(name.clone(), player_id);
        self.names_by_id.insert(player_id, name);
        self.roles_by_id.insert(player_id, role);

        Some(player_id)
    }

    pub fn set_admin_id(&self, player_id: PlayerId) {
        let mut admin_id = self.admin_id.lock().expect("admin_id lock");
        *admin_id = Some(player_id);
    }

    pub fn set_admin_by_name(&self, requester_id: PlayerId, name: &str) -> bool {
        if !self.is_admin(requester_id) {
            self.send_denied_to(requester_id, "forbidden");
            return false;
        }
        let target = name.trim();
        if target.is_empty() {
            self.send_denied_to(requester_id, "user_not_found");
            return false;
        }
        if let Some(requester_name) = self
            .names_by_id
            .get(&requester_id)
            .map(|entry| entry.value().clone())
        {
            if requester_name == target {
                self.send_denied_to(requester_id, "cannot_set_yourself_admin");
                return false;
            }
        }
        let player_id = match self.ids_by_name.get(target) {
            Some(entry) => *entry.value(),
            None => {
                self.send_denied_to(requester_id, "user_not_found");
                return false;
            }
        };
        let old_admin_id = self.admin_id.lock().ok().and_then(|id| *id);
        if let Some(old_admin_id) = old_admin_id {
            self.roles_by_id.insert(old_admin_id, Role::Player);
        }
        self.roles_by_id.insert(player_id, Role::Admin);
        self.set_admin_id(player_id);
        self.broadcast_participants();
        true
    }

    pub fn is_admin(&self, player_id: PlayerId) -> bool {
        self.admin_id
            .lock()
            .ok()
            .and_then(|id| *id)
            .map(|id| id == player_id)
            .unwrap_or(false)
    }

    pub fn kick_by_name(&self, requester_id: PlayerId, name: &str) -> bool {
        if !self.is_admin(requester_id) {
            self.send_denied_to(requester_id, "forbidden");
            return false;
        }
        let target = name.trim();
        if target.is_empty() {
            self.send_denied_to(requester_id, "user_not_found");
            return false;
        }
        if let Some(requester_name) = self
            .names_by_id
            .get(&requester_id)
            .map(|entry| entry.value().clone())
        {
            if requester_name == target {
                self.send_denied_to(requester_id, "cannot_kick_self");
                return false;
            }
        }
        let player_id = match self.ids_by_name.get(target) {
            Some(entry) => *entry.value(),
            None => {
                self.send_denied_to(requester_id, "user_not_found");
                return false;
            }
        };

        self.send_kicked_to(player_id);
        self.routes.remove(&player_id);
        self.ids_by_name.remove(target);
        self.names_by_id.remove(&player_id);
        self.roles_by_id.remove(&player_id);
        self.last_disconnect.remove(&player_id);
        self.broadcast_participants();
        true
    }

    pub fn player_matches(&self, player_id: PlayerId, name: &str) -> bool {
        self.names_by_id
            .get(&player_id)
            .map(|stored| stored.value() == name)
            .unwrap_or(false)
    }

    pub fn player_exists(&self, player_id: PlayerId) -> bool {
        self.names_by_id.contains_key(&player_id)
    }

    pub fn attach_connection(
        &self,
        player_id: PlayerId,
        name: &str,
        sender: mpsc::UnboundedSender<String>,
    ) -> bool {
        if !self.player_matches(player_id, name) {
            return false;
        }

        self.routes.insert(player_id, sender);
        self.last_disconnect.remove(&player_id);
        self.send_participants_to(player_id);
        true
    }

    pub fn detach_connection(&self, player_id: PlayerId) {
        self.routes.remove(&player_id);
        self.last_disconnect.insert(player_id, now_seconds());
    }

    pub fn send_buzz(&self, player_id: PlayerId) {
        let _ = self.buzz_tx.send(player_id);
    }

    pub fn start_round(&self, requester_id: PlayerId) {
        if !self.is_admin(requester_id) {
            self.send_denied_to(requester_id, "forbidden");
            return;
        }
        self.reset_flag.store(true, Ordering::SeqCst);
        self.broadcast(ServerMessage::RoundStarted);
    }

    pub fn participants(&self) -> Vec<ParticipantInfo> {
        let mut list = self
            .names_by_id
            .iter()
            .filter_map(|entry| {
                let player_id = *entry.key();
                let name = entry.value().clone();
                let role = self
                    .roles_by_id
                    .get(&player_id)
                    .map(|role_entry| *role_entry.value())
                    .unwrap_or(Role::Player);
                Some(ParticipantInfo { name, role })
            })
            .collect::<Vec<_>>();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn admin_present(&self) -> bool {
        self.admin_id.lock().ok().and_then(|id| *id).is_some()
    }

    fn broadcast(&self, msg: ServerMessage) {
        let payload = serde_json::to_string(&msg).expect("serialize server message");
        for entry in self.routes.iter() {
            let _ = entry.value().send(payload.clone());
        }
    }

    pub fn broadcast_participants(&self) {
        let msg = ServerMessage::Participants {
            participants: self.participants(),
        };
        self.broadcast(msg);
    }

    pub fn send_participants_to(&self, player_id: PlayerId) {
        let msg = ServerMessage::Participants {
            participants: self.participants(),
        };
        self.send_to_player(player_id, msg);
    }

    pub fn send_kicked_to(&self, player_id: PlayerId) {
        self.send_to_player(player_id, ServerMessage::Kicked);
    }

    pub fn send_denied_to(&self, player_id: PlayerId, reason: &str) {
        let msg = ServerMessage::ActionDenied {
            reason: reason.to_string(),
        };
        self.send_to_player(player_id, msg);
    }

    fn send_to_player(&self, player_id: PlayerId, msg: ServerMessage) {
        if let Some(sender) = self
            .routes
            .get(&player_id)
            .map(|entry| entry.value().clone())
        {
            let payload = serde_json::to_string(&msg).expect("serialize server message");
            let _ = sender.send(payload);
        }
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    fn spawn_cleanup(room: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(CLEANUP_INTERVAL_IN_SECS));
            loop {
                interval.tick().await;
                if room.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                room.cleanup_expired();
            }
        });
    }

    fn cleanup_expired(&self) {
        let now = now_seconds();
        let mut expired = Vec::new();
        for entry in self.last_disconnect.iter() {
            let player_id = *entry.key();
            let disconnected_at = *entry.value();
            if now.saturating_sub(disconnected_at) >= CLEANUP_TTL_IN_SECS {
                expired.push(player_id);
            }
        }

        for player_id in expired {
            if self.routes.contains_key(&player_id) {
                continue;
            }
            self.last_disconnect.remove(&player_id);
            if let Some((_, name)) = self.names_by_id.remove(&player_id) {
                self.ids_by_name.remove(&name);
            }
            self.roles_by_id.remove(&player_id);
            let mut admin_id = self.admin_id.lock().expect("admin_id lock");
            if admin_id.map(|id| id == player_id).unwrap_or(false) {
                *admin_id = None;
            }
        }
    }
}
