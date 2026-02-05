use std::sync::Arc;

use dashmap::DashMap;
use rand::RngCore;
use rand::distr::{Alphanumeric, SampleString};

use crate::auth::JwtAuth;
use crate::errors::AppError;

use super::room_state::{RoomConfig, RoomId, RoomState};

pub const TOKEN_TTL_IN_SECS: u64 = 2 * 60 * 60;
pub const APP_CLEANUP_INTERVAL_IN_SECS: u64 = 30 * 60;

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
        rand::rng().fill_bytes(&mut secret);
        let auth = Arc::new(JwtAuth::new(&secret, TOKEN_TTL_IN_SECS));
        let inner = Arc::new(AppStateInner {
            rooms: DashMap::new(),
            auth,
        });
        Self::spawn_room_cleanup(Arc::clone(&inner));
        Self { inner }
    }

    pub fn create_room(&self, config: RoomConfig, tick_in_ms: u64) -> (RoomId, Arc<RoomState>) {
        let room_id = self.create_random_room_id();
        //                                        this room_id in RoomState is not actually ever used in the current model
        let room = RoomState::new(room_id.clone(), config, tick_in_ms, self.auth());
        self.inner.rooms.insert(room_id.clone(), Arc::clone(&room));
        (room_id, room)
    }

    pub fn get_room(&self, room_id: &str) -> Result<Arc<RoomState>, AppError> {
        self.inner
            .rooms
            .get(room_id)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or(AppError::RoomNotFound)
    }

    pub fn auth(&self) -> Arc<JwtAuth> {
        Arc::clone(&self.inner.auth)
    }

    fn create_random_room_id(&self) -> RoomId {
        let mut rng = rand::rng();
        Alphanumeric.sample_string(&mut rng, 6)
    }

    fn spawn_room_cleanup(inner: Arc<AppStateInner>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
                APP_CLEANUP_INTERVAL_IN_SECS,
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
