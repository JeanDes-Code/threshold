use super::arrival::pc_witnesses;
use crate::domain::Trigger;
use crate::store::WorldView;

/// The game minutes a `wait` takes when the player names no duration.
pub const WAIT_DEFAULT_MIN: u32 = 60;

/// The longest single wait, in game minutes. A longer wait is cut to this.
pub const WAIT_MAX_MIN: u32 = 480;

/// The minutes a `wait` Decision Proposal takes, owned by the engine.
///
/// The PC waits `requested` minutes, or `WAIT_DEFAULT_MIN` when it is 0, at
/// most `WAIT_MAX_MIN`. The wait ends early at the next World Event with an
/// `At` trigger after the clock that the PC would witness (`pc_witnesses`):
/// the PC waits until something happens in front of them. An unseen event
/// does not end the wait, so its timing does not leak. `When` events are
/// ignored, because no time is known for them. The result is always at
/// least 1.
pub fn wait_minutes(view: &WorldView, requested: u32) -> u32 {
    let wanted = if requested == 0 {
        WAIT_DEFAULT_MIN
    } else {
        requested
    }
    .min(WAIT_MAX_MIN);
    view.events_scheduled
        .iter()
        .filter(|event| pc_witnesses(view, event))
        .filter_map(|event| match event.trigger {
            Trigger::At(at) if at > view.clock_min => Some(at - view.clock_min),
            _ => None,
        })
        .filter_map(|until| u32::try_from(until).ok())
        .fold(wanted, u32::min)
}

/// The words by which an Intent asks for a wait (bug pass 3).
const WAIT_WORDS: &[&str] = &[
    "wait", "waits", "waiting", "rest", "rests", "resting", "sleep", "sleeps", "sleeping", "nap",
    "naps", "doze", "dozes", "linger", "lingers", "until", "while", "bide", "idle", "relax",
];

/// True when `intent` asks for a wait: one of `WAIT_WORDS` as a whole word,
/// or "pass the time". The model labels a passive act `wait` ("I warm my
/// hands at the fire" took 60 minutes in QA), so the engine owns the length
/// of a wait only when the Player asked for one.
pub fn asks_to_wait(intent: &str) -> bool {
    let words = crate::normalize::name_words(intent);
    words.iter().any(|w| WAIT_WORDS.contains(&w.as_str()))
        || words.windows(3).any(|w| w == ["pass", "the", "time"])
}
