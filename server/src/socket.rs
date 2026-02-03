use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use core::game::PlayerId;

use crate::dtos::{ClientMessage, Role};
use crate::state::RoomState;

pub struct PlayerSession {
    pub player_id: PlayerId,
    pub name: String,
    pub role: Role,
}

pub async fn handle_socket(socket: WebSocket, room: Arc<RoomState>, session: PlayerSession) {
    let (mut sender, mut receiver) = socket.split();
    let (local_tx, mut local_rx) = mpsc::unbounded_channel::<String>();

    if !room.attach_connection(session.player_id, &session.name, local_tx.clone()) {
        return;
    }

    loop {
        tokio::select! {
            outbound = local_rx.recv() => {
                match outbound {
                    Some(text) => {
                        if sender.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            inbound = receiver.next() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(msg) = parse_client_message(&text) {
                            match msg {
                                ClientMessage::Buzz => {
                                    room.send_buzz(session.player_id);
                                }
                                ClientMessage::StartRound => {
                                    room.start_round(session.player_id);
                                }
                                ClientMessage::SetAdmin { name } => {
                                    let _ = room.set_admin_by_name(session.player_id, &name);
                                }
                                ClientMessage::Kick { name } => {
                                    let _ = room.kick_by_name(session.player_id, &name);
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    room.detach_connection(session.player_id);
}

fn parse_client_message(text: &str) -> Option<ClientMessage> {
    serde_json::from_str(text).ok()
}
