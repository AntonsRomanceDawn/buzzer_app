use super::*;

impl RoomState {
    pub(super) fn spawn_command_loop(
        room: Arc<Self>,
        mut command_rx: mpsc::UnboundedReceiver<RoomCommand>,
    ) {
        tokio::spawn(async move {
            while let Some(cmd) = command_rx.recv().await {
                match cmd {
                    RoomCommand::CreateAdmin { name, resp } => {
                        let _ = resp.send(room.create_admin_direct(&name));
                    }
                    RoomCommand::Join {
                        requested_name,
                        token,
                        resp,
                    } => {
                        let result = room.resolve_join_direct(&requested_name, token.as_deref());
                        if result.is_ok() {
                            room.broadcast_participants();
                        }
                        let _ = resp.send(result);
                    }
                    RoomCommand::RefreshToken { token, resp } => {
                        let _ = resp.send(room.refresh_token_direct(&token));
                    }
                    RoomCommand::AttachConnection {
                        player_id,
                        name,
                        sender,
                        resp,
                    } => {
                        let _ = resp.send(room.attach_connection_direct(player_id, &name, sender));
                    }
                    RoomCommand::DetachConnection { player_id } => {
                        room.detach_connection_direct(player_id);
                    }
                    RoomCommand::KickByName {
                        requester_id,
                        name,
                        resp,
                    } => {
                        let _ = resp.send(room.kick_by_name_direct(requester_id, &name));
                    }
                    RoomCommand::StartRound { requester_id } => {
                        room.start_round_direct(requester_id);
                    }
                    RoomCommand::ContinueRound { requester_id } => {
                        room.continue_round_direct(requester_id);
                    }
                    RoomCommand::CleanupExpired => {
                        room.cleanup_expired();
                    }
                }
            }
        });
    }

    pub async fn create_admin(&self, name: &str) -> Result<String, AppError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(RoomCommand::CreateAdmin {
                name: name.to_string(),
                resp: tx,
            })
            .map_err(|_| AppError::Internal)?;
        rx.await.map_err(|_| AppError::Internal)?
    }

    pub async fn join(
        &self,
        requested_name: &str,
        token: Option<&str>,
    ) -> Result<(String, Role), AppError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(RoomCommand::Join {
                requested_name: requested_name.to_string(),
                token: token.map(str::to_string),
                resp: tx,
            })
            .map_err(|_| AppError::Internal)?;
        rx.await.map_err(|_| AppError::Internal)?
    }

    pub async fn refresh_token(&self, token: &str) -> Result<String, AppError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(RoomCommand::RefreshToken {
                token: token.to_string(),
                resp: tx,
            })
            .map_err(|_| AppError::Internal)?;
        rx.await.map_err(|_| AppError::Internal)?
    }

    pub async fn attach_connection(
        &self,
        player_id: PlayerId,
        name: &str,
        sender: mpsc::UnboundedSender<String>,
    ) -> Result<bool, AppError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(RoomCommand::AttachConnection {
                player_id,
                name: name.to_string(),
                sender,
                resp: tx,
            })
            .map_err(|_| AppError::Internal)?;
        rx.await.map_err(|_| AppError::Internal)
    }

    pub fn detach_connection(&self, player_id: PlayerId) {
        let _ = self
            .command_tx
            .send(RoomCommand::DetachConnection { player_id });
    }

    pub async fn kick_by_name(&self, requester_id: PlayerId, name: &str) -> Result<bool, AppError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(RoomCommand::KickByName {
                requester_id,
                name: name.to_string(),
                resp: tx,
            })
            .map_err(|_| AppError::Internal)?;
        rx.await.map_err(|_| AppError::Internal)
    }

    pub fn start_round(&self, requester_id: PlayerId) {
        let _ = self
            .command_tx
            .send(RoomCommand::StartRound { requester_id });
    }

    pub fn continue_round(&self, requester_id: PlayerId) {
        let _ = self
            .command_tx
            .send(RoomCommand::ContinueRound { requester_id });
    }

    pub fn request_cleanup(&self) {
        let _ = self.command_tx.send(RoomCommand::CleanupExpired);
    }
}
