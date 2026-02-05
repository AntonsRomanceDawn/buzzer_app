use super::*;
use crate::utils::time::now_seconds;
use crate::utils::time::now_millis;

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
        self.broadcast(ServerMessage::RoundStarted {
            ts_ms: now_millis(),
        });
    }

    pub fn participants(&self) -> Vec<ParticipantInfo> {
        let mut list = self
            .names_by_id
            .iter()
            .map(|entry| {
                let player_id = *entry.key();
                let name = entry.value().clone();
                let role = self
                    .roles_by_id
                    .get(&player_id)
                    .map(|role_entry| *role_entry.value())
                    .unwrap_or(Role::Player);
                ParticipantInfo { name, role }
            })
            .collect::<Vec<_>>();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn answer_window_in_ms(&self) -> u64 {
        self.answer_window_in_ms
    }

    // pub fn id(&self) -> &str {
    //     &self.id
    // }

    pub fn admin_present(&self) -> bool {
        let now = now_seconds();
        let admin_id = self.admin_id.lock().ok().and_then(|id| *id);
        let Some(admin_id) = admin_id else {
            return false;
        };
        self.token_exp_by_id
            .get(&admin_id)
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
            ts_ms: now_millis(),
        };
        self.broadcast(msg);
    }

    pub fn send_participants_to(&self, player_id: PlayerId) {
        let msg = ServerMessage::Participants {
            participants: self.participants(),
            ts_ms: now_millis(),
        };
        self.send_to_player(player_id, msg);
    }

    pub fn send_kicked_to(&self, player_id: PlayerId) {
        self.send_to_player(
            player_id,
            ServerMessage::Kicked {
                ts_ms: now_millis(),
            },
        );
    }

    pub fn send_denied_to(&self, player_id: PlayerId, reason: &str) {
        let msg = ServerMessage::ActionDenied {
            reason: reason.to_string(),
            ts_ms: now_millis(),
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
