use super::*;
use crate::utils::time::now_seconds;

impl RoomState {
    pub(super) fn spawn_cleanup(room: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
                ROOM_CLEANUP_INTERVAL_IN_SECS,
            ));
            loop {
                interval.tick().await;
                if room.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                room.request_cleanup();
            }
        });
    }

    pub(super) fn cleanup_expired(&self) {
        let now = now_seconds();
        let mut expired = Vec::new();
        for entry in self.token_exp_by_id.iter() {
            let player_id = *entry.key();
            let token_exp = *entry.value();
            if now >= token_exp {
                expired.push(player_id);
            }
        }

        for player_id in expired {
            self.send_kicked_to(player_id);
            let _ = self.remove_player(player_id);
        }
    }
}
