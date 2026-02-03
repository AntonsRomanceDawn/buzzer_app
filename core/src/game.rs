pub type PlayerId = usize;

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
}

pub enum Action {
    Accepted(PlayerId, u64), // deadline in ms
    Rejected(PlayerId),
    TimedOut(PlayerId), // timed out player
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
            },
        }
    }

    pub fn buzz(&mut self, player: PlayerId, now_in_ms: u64) -> Action {
        if !self.is_phase_idle() || self.is_locked_out(player) {
            return Action::Rejected(player);
        }

        let deadline_in_ms = now_in_ms + self.config.answer_window_in_ms;
        self.set_phase_answering(player, deadline_in_ms);
        Action::Accepted(player, deadline_in_ms)
    }

    pub fn tick(&mut self, now_in_ms: u64) -> Option<Action> {
        match self.state.phase {
            Phase::Answering {
                player,
                deadline_in_ms,
            } if now_in_ms >= deadline_in_ms => {
                self.set_phase_idle();
                self.set_locked_out(player);
                Some(Action::TimedOut(player))
            }
            _ => None,
        }
    }

    pub fn reset(&mut self) {
        self.reset_locked_players();
        self.set_phase_idle();
    }

    fn is_locked_out(&self, player: PlayerId) -> bool {
        let mask = 1u128 << player;
        self.state.locked_out_players & mask != 0
    }

    fn set_locked_out(&mut self, player: PlayerId) {
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
