use std::num::NonZeroU32;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use governor::{Quota, RateLimiter};
use tokio::sync::mpsc;
use tracing::{info, warn};

use core::game::PlayerId;

use crate::dtos::ClientMessage;
use crate::state::room_state::RoomState;

pub struct PlayerSession {
    pub player_id: PlayerId,
    pub name: String,
}

pub async fn handle_socket(socket: WebSocket, room: Arc<RoomState>, session: PlayerSession) {
    let (mut sender, mut receiver) = socket.split();
    let (local_tx, mut local_rx) = mpsc::unbounded_channel::<String>();

    let attached = room
        .attach_connection(session.player_id, &session.name, local_tx.clone())
        .await
        .unwrap_or(false);
    if !attached {
        warn!(
            "[WS] Failed to attach connection for player {} (id: {})",
            session.name, session.player_id
        );
        return;
    }

    info!(
        "[WS] Attached connection for player {} (id: {})",
        session.name, session.player_id
    );

    let inbound_limiter = RateLimiter::direct(Quota::per_second(
        NonZeroU32::new(20).expect("non-zero ws inbound quota"),
    ));

    loop {
        tokio::select! {
            outbound = local_rx.recv() => {
                match outbound {
                    Some(text) => {
                        if sender.send(Message::Text(text.into())).await.is_err() {
                            warn!("[WS] Failed to send message to player {}", session.player_id);
                            break;
                        }
                    }
                    None => break,
                }
            }
            inbound = receiver.next() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        if inbound_limiter.check().is_err() {
                            warn!("[WS] Rate limit exceeded for player {}", session.player_id);
                            room.send_denied_to(session.player_id, "rate_limited");
                            continue;
                        }
                        if let Some(msg) = parse_client_message(&text) {
                            match msg {
                                ClientMessage::Buzz => {
                                    room.send_buzz(session.player_id);
                                }
                                ClientMessage::StartRound => {
                                    room.start_round(session.player_id);
                                }
                                ClientMessage::SetAdmin { name } => {
                                    let _ = room.set_admin_by_name(session.player_id, &name).await;
                                }
                                ClientMessage::Kick { name } => {
                                    let _ = room.kick_by_name(session.player_id, &name).await;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("[WS] Client closed connection for player {}", session.player_id);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    info!("[WS] Detaching connection for player {}", session.player_id);
    room.detach_connection(session.player_id);
}

fn parse_client_message(text: &str) -> Option<ClientMessage> {
    serde_json::from_str(text).ok()
}
