use super::*;
use crate::state::app_state::ADMIN_PLAYER_ID;
use crate::utils::time::now_seconds;

impl RoomState {
    pub(super) fn attach_connection_direct(
        &self,
        player_id: PlayerId,
        name: &str,
        sender: mpsc::UnboundedSender<String>,
    ) -> bool {
        if !self.player_matches(player_id, name) {
            return false;
        }

        self.routes.insert(player_id, sender);
        self.send_participants_to(player_id);
        true
    }

    pub(super) fn detach_connection_direct(&self, player_id: PlayerId) {
        self.routes.remove(&player_id);
    }

    pub fn send_buzz(&self, player_id: PlayerId) {
        let _ = self.buzz_tx.send(player_id);
    }

    pub(super) fn start_round_direct(&self, requester_id: PlayerId) {
        if !self.is_admin(requester_id) {
            self.send_denied_to(requester_id, "forbidden");
            return;
        }
        self.reset_flag.store(true, Ordering::SeqCst);
    }

    pub(super) fn continue_round_direct(&self, requester_id: PlayerId) {
        if !self.is_admin(requester_id) {
            self.send_denied_to(requester_id, "forbidden");
            return;
        }
        self.continue_flag.store(true, Ordering::SeqCst);
    }

    pub fn participants(&self) -> Vec<ParticipantInfo> {
        let mask = *self.locked_out_mask.lock().expect("lock shared mask");
        let mut list = self
            .names_by_id
            .iter()
            .map(|entry| {
                let player_id = *entry.key();
                let name = entry.value().clone();
                let role = if player_id == ADMIN_PLAYER_ID {
                    Role::Admin
                } else {
                    Role::Player
                };
                let locked_out = if player_id < 128 {
                    (mask & (1u128 << player_id)) != 0
                } else {
                    false
                };
                ParticipantInfo {
                    name,
                    role,
                    locked_out,
                }
            })
            .collect::<Vec<_>>();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn answer_window_in_ms(&self) -> u64 {
        self.answer_window_in_ms
    }

    pub fn admin_present(&self) -> bool {
        let now = now_seconds();
        self.token_exp_by_id
            .get(&ADMIN_PLAYER_ID)
            .map(|entry| now < *entry.value())
            .unwrap_or(false)
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
}
