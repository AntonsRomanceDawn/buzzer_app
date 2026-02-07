use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use dashmap::DashMap;
use tokio::{sync::mpsc, time};

use core::adapter::{self, GameInput, GameOutput, TimeSource};
use core::game::{BuzzerGame, Config, OutputEvent, PlayerId};

use crate::dtos::ServerMessage;

pub fn spawn_room_loop(
    tick_in_ms: u64,
    answer_window_in_ms: u64,
    buzz_rx: mpsc::UnboundedReceiver<PlayerId>,
    reset_flag: Arc<AtomicBool>,
    continue_flag: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    locked_out_mask: Arc<Mutex<u128>>,
    routes: Arc<DashMap<PlayerId, mpsc::UnboundedSender<String>>>,
    names_by_id: Arc<DashMap<PlayerId, String>>,
    next_id: Arc<Mutex<PlayerId>>,
) {
    tokio::spawn(async move {
        let mut game = BuzzerGame::new(Config {
            answer_window_in_ms,
        });
        let mut interval = time::interval(time::Duration::from_millis(tick_in_ms));
        let time = InstantTime {
            start: Instant::now(),
        };
        let mut input = ChannelInput {
            rx: buzz_rx,
            next_player_id: next_id,
        };
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
                adapter::start_round(&mut game, &input, &mut output);
            }
            if continue_flag.swap(false, Ordering::SeqCst) {
                adapter::continue_round(&mut game, &mut output);
            }
            adapter::step(&mut game, &time, &mut input, &mut output);
            if let Ok(mut mask) = locked_out_mask.lock() {
                *mask = game.locked_out_players();
            }
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
    next_player_id: Arc<Mutex<PlayerId>>,
}

impl GameInput for ChannelInput {
    fn next_buzz(&mut self) -> Option<PlayerId> {
        self.rx.try_recv().ok()
    }

    fn current_player_count(&self) -> PlayerId {
        *self.next_player_id.lock().expect("next_id lock")
    }
}

struct RoutedOutput {
    routes: Arc<DashMap<PlayerId, mpsc::UnboundedSender<String>>>,
    names_by_id: Arc<DashMap<PlayerId, String>>,
}

impl GameOutput for RoutedOutput {
    fn on_event(&mut self, event: OutputEvent) {
        match event {
            OutputEvent::Accepted(player_id, _) => {
                let name = self.name_for(player_id);
                let msg = ServerMessage::Accepted { name };
                self.broadcast(msg);
            }
            OutputEvent::Rejected(player_id) => {
                if let Some(tx) = self
                    .routes
                    .get(&player_id)
                    .map(|entry| entry.value().clone())
                {
                    let _ = tx.send(serialize(ServerMessage::Rejected));
                }
            }
            OutputEvent::TimedOut(player_id) => {
                let name = self.name_for(player_id);
                let msg = ServerMessage::TimedOut { name };
                self.broadcast(msg);
            }
            OutputEvent::RoundStarted => {
                let msg = ServerMessage::RoundStarted;
                self.broadcast(msg);
            }
            OutputEvent::RoundContinued => {
                let msg = ServerMessage::RoundContinued;
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
