use super::*;
use crate::state::app_state::ADMIN_PLAYER_ID;

impl RoomState {
    fn name_exists(&self, name: &str) -> bool {
        self.ids_by_name.contains_key(name)
    }

    pub fn insert_player(&self, name: String, role: Role) -> Result<PlayerId, AppError> {
        let mut next_id = self.next_id.lock().expect("next_id lock");
        let player_id = *next_id;
        if player_id >= core::game::MAX_PLAYER_ID {
            return Err(AppError::FullRoom);
        }
        *next_id = next_id.wrapping_add(1);

        self.ids_by_name.insert(name.clone(), player_id);
        self.names_by_id.insert(player_id, name);

        Ok(player_id)
    }

    pub fn remove_player(&self, player_id: PlayerId) -> Result<(String, Role), AppError> {
        self.routes.remove(&player_id);
        self.token_exp_by_id.remove(&player_id);
        let name = self
            .names_by_id
            .remove(&player_id)
            .map(|(_, name)| name)
            .map_or(Err(AppError::Kicked), Ok)?;
        self.ids_by_name.remove(&name);

        let role = if player_id == ADMIN_PLAYER_ID {
            Role::Admin
        } else {
            Role::Player
        };
        Ok((name, role))
    }

    fn set_token_expiry(&self, player_id: PlayerId, exp: u64) {
        self.token_exp_by_id.insert(player_id, exp);
    }

    pub fn is_admin(&self, player_id: PlayerId) -> bool {
        player_id == ADMIN_PLAYER_ID
    }

    pub(super) fn kick_by_name_direct(&self, requester_id: PlayerId, name: &str) -> bool {
        if !self.is_admin(requester_id) {
            self.send_denied_to(requester_id, "forbidden");
            return false;
        }
        let target = name.trim();
        if target.is_empty() {
            self.send_denied_to(requester_id, "user_not_found");
            return false;
        }

        let target_id = match self.ids_by_name.get(target) {
            Some(entry) => *entry.value(),
            None => {
                self.send_denied_to(requester_id, "user_not_found");
                return false;
            }
        };

        if target_id == requester_id {
            self.send_denied_to(requester_id, "cannot_kick_self");
            return false;
        }

        self.send_kicked_to(target_id);
        let _ = self.remove_player(target_id);
        self.broadcast_participants();
        true
    }

    pub fn player_matches(&self, player_id: PlayerId, name: &str) -> bool {
        self.names_by_id
            .get(&player_id)
            .map(|stored| stored.value() == name)
            .unwrap_or(false)
    }

    fn issue_token(&self, player_id: PlayerId, name: &str, role: Role) -> Result<String, AppError> {
        let (token, exp) = self.auth.issue(&self.room_id, player_id, name, role)?;
        self.set_token_expiry(player_id, exp);
        Ok(token)
    }

    pub(super) fn resolve_join_direct(
        &self,
        requested_name: &str,
        token: Option<&str>,
    ) -> Result<(String, Role), AppError> {
        if let Some(token) = token {
            let claims = self.auth.verify(token)?;
            if claims.room_id != self.room_id {
                return Err(AppError::RoomMismatch);
            }

            if !self.player_matches(claims.player_id, &claims.name) {
                return Err(AppError::Kicked);
            }

            self.ids_by_name
                .insert(claims.name.clone(), claims.player_id);
            self.ids_by_name
                .insert(requested_name.to_string(), claims.player_id);
            self.names_by_id
                .insert(claims.player_id, requested_name.to_string());
            let new_token = self.issue_token(claims.player_id, requested_name, claims.role)?;
            return Ok((new_token, claims.role));
        }

        if self.name_exists(requested_name) {
            return Err(AppError::NameTaken);
        }

        let role = Role::Player;
        let player_id = self.insert_player(requested_name.to_string(), role)?;
        let token = self.issue_token(player_id, requested_name, role)?;
        Ok((token, role))
    }

    pub(super) fn refresh_token_direct(&self, token: &str) -> Result<String, AppError> {
        let claims = self.auth.verify(token)?;
        if claims.room_id != self.room_id {
            return Err(AppError::RoomMismatch);
        }
        if !self.player_matches(claims.player_id, &claims.name) {
            return Err(AppError::UserNotInRoom);
        }
        self.issue_token(claims.player_id, &claims.name, claims.role)
    }

    pub(super) fn create_admin_direct(&self, name: &str) -> Result<String, AppError> {
        let player_id = self.insert_player(name.to_string(), Role::Admin)?;
        self.issue_token(player_id, name, Role::Admin)
    }
}
