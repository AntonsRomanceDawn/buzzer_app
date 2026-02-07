//! Platform adapter traits for no_std integration.
//! This keeps I/O (buttons, timers, LEDs, sounds) outside the core game logic.

use crate::game::{BuzzerGame, OutputEvent, PlayerId};

/// Source of time for the game engine.
pub trait TimeSource {
    /// Milliseconds since some monotonic start.
    fn now_ms(&self) -> u64;
}

pub trait GameInput {
    /// Return the next buzzing player, or None if no pending buzzes.
    fn next_buzz(&mut self) -> Option<PlayerId>;
    fn current_player_count(&self) -> PlayerId;
}

/// Output sink for game actions (LEDs, sounds, UI updates, etc.).
pub trait GameOutput {
    fn on_event(&mut self, event: OutputEvent);
}

/// Minimal "runner" that ties inputs + time + outputs to the game logic.
/// Call this in a loop (or from a timer tick) on any platform.
pub fn step<T, I, O>(game: &mut BuzzerGame, time: &T, input: &mut I, output: &mut O)
where
    T: TimeSource,
    I: GameInput,
    O: GameOutput,
{
    let now = time.now_ms();

    while let Some(player) = input.next_buzz() {
        let event = game.buzz(player, now);
        output.on_event(event);
    }

    if let Some(event) = game.tick(now) {
        output.on_event(event);
    }
}

/// Reset the game state (clears locks and returns to idle).
pub fn start_round<I: GameInput, O: GameOutput>(game: &mut BuzzerGame, input: &I, output: &mut O) {
    game.set_curr_player_id(input.current_player_count());
    let event = game.start_round();
    output.on_event(event);
}

pub fn continue_round<O: GameOutput>(game: &mut BuzzerGame, output: &mut O) {
    let event = game.continue_round();
    output.on_event(event);
}
