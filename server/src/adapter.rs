use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use dashmap::DashMap;
use tokio::{sync::mpsc, time};

use core::adapter::{self, BuzzerInput, BuzzerOutput, TimeSource};
use core::game::{Action, BuzzerGame, Config, PlayerId};

use crate::dtos::ServerMessage;
use crate::utils::time::now_millis;

pub fn spawn_room_loop(
    tick_in_ms: u64,
    answer_window_in_ms: u64,
    buzz_rx: mpsc::UnboundedReceiver<PlayerId>,
    reset_flag: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    routes: Arc<DashMap<PlayerId, mpsc::UnboundedSender<String>>>,
    names_by_id: Arc<DashMap<PlayerId, String>>,
) {
    tokio::spawn(async move {
        let mut game = BuzzerGame::new(Config {
            answer_window_in_ms,
        });
        let mut interval = time::interval(time::Duration::from_millis(tick_in_ms));
        let time = InstantTime {
            start: Instant::now(),
        };
        let mut input = ChannelInput { rx: buzz_rx };
        let mut output = RoutedOutput {
            routes,
            names_by_id,
        };

        loop {
            interval.tick().await;
            if shutdown.load(Ordering::SeqCst) {
                break;
            }
            if reset_flag.swap(false, Ordering::SeqCst) {
                adapter::reset(&mut game);
            }
            adapter::step(&mut game, &time, &mut input, &mut output);
        }
    });
}

struct InstantTime {
    start: Instant,
}

impl TimeSource for InstantTime {
    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

struct ChannelInput {
    rx: mpsc::UnboundedReceiver<PlayerId>,
}

impl BuzzerInput for ChannelInput {
    fn next_buzz(&mut self) -> Option<PlayerId> {
        self.rx.try_recv().ok()
    }
}

struct RoutedOutput {
    routes: Arc<DashMap<PlayerId, mpsc::UnboundedSender<String>>>,
    names_by_id: Arc<DashMap<PlayerId, String>>,
}

impl BuzzerOutput for RoutedOutput {
    fn on_action(&mut self, action: Action) {
        match action {
            Action::Accepted(player, deadline_in_ms) => {
                let name = self.name_for(player);
                let msg = ServerMessage::Accepted {
                    name,
                    deadline_in_ms,
                    ts_ms: now_millis(),
                };
                self.broadcast(msg);
            }
            Action::Rejected(player) => {
                if let Some(tx) = self.routes.get(&player).map(|entry| entry.value().clone()) {
                    let _ = tx.send(serialize(ServerMessage::Rejected {
                        ts_ms: now_millis(),
                    }));
                }
            }
            Action::TimedOut(player) => {
                let name = self.name_for(player);
                let msg = ServerMessage::TimedOut {
                    name,
                    ts_ms: now_millis(),
                };
                self.broadcast(msg);
            }
        }
    }
}

impl RoutedOutput {
    fn name_for(&self, player: PlayerId) -> String {
        self.names_by_id
            .get(&player)
            .map(|entry| entry.value().clone())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn broadcast(&self, msg: ServerMessage) {
        let payload = serialize(msg);
        for entry in self.routes.iter() {
            let _ = entry.value().send(payload.clone());
        }
    }
}

fn serialize(msg: ServerMessage) -> String {
    serde_json::to_string(&msg).expect("serialize server message")
}
