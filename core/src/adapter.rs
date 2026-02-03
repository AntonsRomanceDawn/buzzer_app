//! Platform adapter traits for no_std integration.
//! This keeps I/O (buttons, timers, LEDs, sounds) outside the core game logic.

use crate::game::{Action, BuzzerGame, PlayerId};

/// Source of time for the game engine.
pub trait TimeSource {
    /// Milliseconds since some monotonic start.
    fn now_ms(&self) -> u64;
}

/// Input source that can report which player (if any) buzzed.
pub trait BuzzerInput {
    /// Return a buzzing player if a new press occurred since last poll.
    fn next_buzz(&mut self) -> Option<PlayerId>;
}

/// Output sink for game actions (LEDs, sounds, UI updates, etc.).
pub trait BuzzerOutput {
    fn on_action(&mut self, action: Action);
}

/// Minimal "runner" that ties inputs + time + outputs to the game logic.
/// Call this in a loop (or from a timer tick) on any platform.
pub fn step<T, I, O>(game: &mut BuzzerGame, time: &T, input: &mut I, output: &mut O)
where
    T: TimeSource,
    I: BuzzerInput,
    O: BuzzerOutput,
{
    let now = time.now_ms();

    if let Some(player) = input.next_buzz() {
        let action = game.buzz(player, now);
        output.on_action(action);
    }

    if let Some(action) = game.tick(now) {
        output.on_action(action);
    }
}

/// Reset the game state (clears locks and returns to idle).
pub fn reset(game: &mut BuzzerGame) {
    game.reset();
}
