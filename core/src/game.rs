pub type PlayerId = usize;

pub const MAX_PLAYER_ID: PlayerId = 127;

pub struct Config {
    pub answer_window_in_ms: u64,
}

#[derive(PartialEq, Eq)]
enum Phase {
    Idle,
    Answering {
        player: PlayerId,
        deadline_in_ms: u64,
    },
}

struct State {
    phase: Phase,
    locked_out_players: u128, // only 128 players allowed
    curr_player_id: PlayerId,
}

pub enum OutputEvent {
    Accepted(PlayerId, u64), // deadline in ms
    Rejected(PlayerId),
    TimedOut(PlayerId), // timed out player
    RoundStarted,
    RoundContinued,
}

pub struct BuzzerGame {
    config: Config,
    state: State,
}

impl BuzzerGame {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: State {
                phase: Phase::Idle,
                locked_out_players: 0,
                curr_player_id: 0,
            },
        }
    }

    pub fn set_curr_player_id(&mut self, id: PlayerId) {
        self.state.curr_player_id = id;
    }

    pub fn locked_out_players(&self) -> u128 {
        let explicit = self.state.locked_out_players;
        if self.state.curr_player_id >= 128 {
            return explicit;
        }
        let implicit = u128::MAX << self.state.curr_player_id;
        explicit | implicit
    }

    pub fn buzz(&mut self, player: PlayerId, now_in_ms: u64) -> OutputEvent {
        if player > MAX_PLAYER_ID {
            return OutputEvent::Rejected(player);
        }
        if !self.is_phase_idle() || self.is_locked_out(player) {
            return OutputEvent::Rejected(player);
        }

        let deadline_in_ms = now_in_ms + self.config.answer_window_in_ms;
        self.set_phase_answering(player, deadline_in_ms);
        OutputEvent::Accepted(player, deadline_in_ms)
    }

    pub fn start_round(&mut self) -> OutputEvent {
        self.reset_locked_players();
        self.set_phase_idle();
        OutputEvent::RoundStarted
    }

    pub fn continue_round(&mut self) -> OutputEvent {
        match self.state.phase {
            Phase::Answering { player, .. } => {
                self.set_locked_out(player);
            }
            _ => {}
        }
        self.set_phase_idle();
        OutputEvent::RoundContinued
    }

    pub fn tick(&mut self, now_in_ms: u64) -> Option<OutputEvent> {
        match self.state.phase {
            Phase::Answering {
                player,
                deadline_in_ms,
            } if now_in_ms >= deadline_in_ms => {
                self.set_phase_idle();
                self.set_locked_out(player);
                Some(OutputEvent::TimedOut(player))
            }
            _ => None,
        }
    }

    fn is_locked_out(&self, player: PlayerId) -> bool {
        if player > MAX_PLAYER_ID {
            return false;
        }

        if player >= self.state.curr_player_id {
            return true;
        }

        let mask = 1u128 << player;
        self.state.locked_out_players & mask != 0
    }

    fn set_locked_out(&mut self, player: PlayerId) {
        if player > MAX_PLAYER_ID {
            return;
        }
        let mask = 1u128 << player;
        self.state.locked_out_players |= mask;
    }

    fn reset_locked_players(&mut self) {
        self.state.locked_out_players = 0u128;
    }

    fn is_phase_idle(&self) -> bool {
        self.state.phase.eq(&Phase::Idle)
    }

    fn set_phase_idle(&mut self) {
        self.state.phase = Phase::Idle;
    }

    fn set_phase_answering(&mut self, player: PlayerId, deadline_in_ms: u64) {
        self.state.phase = Phase::Answering {
            player,
            deadline_in_ms,
        };
    }
}
