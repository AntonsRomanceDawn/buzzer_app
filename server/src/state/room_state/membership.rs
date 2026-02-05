use super::*;

impl RoomState {
    fn name_exists(&self, name: &str) -> bool {
        self.ids_by_name.contains_key(name)
    }

    pub fn insert_player(&self, name: String, role: Role) -> Result<PlayerId, AppError> {
        let player_id = {
            let mut next_id = self.next_id.lock().expect("next_id lock");
            let id = *next_id;
            if id >= 128 as usize {
                return Err(AppError::FullRoom);
            }
            *next_id = next_id.wrapping_add(1);
            id
        };

        self.ids_by_name.insert(name.clone(), player_id);
        self.names_by_id.insert(player_id, name);
        self.roles_by_id.insert(player_id, role);

        if role == Role::Admin {
            self.set_admin_id(player_id);
        }

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
        let role = self
            .roles_by_id
            .remove(&player_id)
            .map(|(_, role)| role)
            .unwrap_or(Role::Player);
        let mut admin_id = self.admin_id.lock().expect("admin_id lock");
        if admin_id.map(|id| id == player_id).unwrap_or(false) {
            *admin_id = None;
        }
        Ok((name, role))
    }

    pub fn set_admin_id(&self, player_id: PlayerId) {
        let mut admin_id = self.admin_id.lock().expect("admin_id lock");
        *admin_id = Some(player_id);
    }
    fn set_token_expiry(&self, player_id: PlayerId, exp: u64) {
        self.token_exp_by_id.insert(player_id, exp);
    }

    pub(super) fn set_admin_by_name_direct(&self, requester_id: PlayerId, name: &str) -> bool {
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
        let _ = self.remove_player(player_id);
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
    ) -> Result<String, AppError> {
        let mut role = Role::Player;
        if let Some(token) = token {
            let claims = self.auth.verify(token)?;
            if claims.room_id != self.room_id {
                return Err(AppError::RoomMismatch);
            }
            let (_, r) = self.remove_player(claims.player_id)?;
            role = r;
        }
        if self.name_exists(requested_name) {
            return Err(AppError::NameTaken);
        }
        let player_id = self.insert_player(requested_name.to_string(), role)?;
        self.issue_token(player_id, requested_name, role)
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
