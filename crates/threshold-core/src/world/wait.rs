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

/// The verbs by which an Intent asks for a wait, as whole words (bug pass
/// 3). "rest" is in `asks_to_wait`, apart: it is also a noun.
const WAIT_VERBS: &[&str] = &[
    "wait",
    "waits",
    "waiting",
    "sleep",
    "sleeps",
    "sleeping",
    "nap",
    "naps",
    "napping",
    "doze",
    "dozes",
    "dozing",
    "linger",
    "lingers",
    "lingering",
    "bide",
    "idle",
    "relax",
    "relaxes",
    "relaxing",
];

/// Phrases that ask for a wait, as runs of whole words.
const WAIT_PHRASES: &[&[&str]] = &[
    &["pass", "the", "time"],
    &["to", "bed"],
    &["keep", "watch"],
    &["keeps", "watch"],
];

/// Verbs of staying: a wait only with a duration after them.
const STAY_VERBS: &[&str] = &[
    "stay",
    "stays",
    "staying",
    "remain",
    "remains",
    "remaining",
    "sit",
    "sits",
    "sitting",
];

/// The words of a duration after a verb of staying.
const DURATION_WORDS: &[&str] = &[
    "minute", "minutes", "hour", "hours", "while", "moment", "night", "day", "days", "dawn",
    "dusk", "morning", "evening", "noon",
];

/// True when `intent` asks for a wait (bug pass 3, changed after review):
/// a verb of waiting, sleeping or resting as a whole word; a phrase such
/// as "pass the time", "to bed" or "keep watch"; or a verb of staying with
/// a duration after it ("I stay here for two hours", "I sit here a
/// while"). "rest" counts as the verb only: "the rest of the story" asks
/// for nothing. "while" or "until" alone ask for nothing: "I talk with
/// Orsel while I keep an eye on the gate" is a talk. The model labels a
/// passive act `wait` ("I warm my hands at the fire" took 60 minutes in
/// QA), so the engine owns the length of a wait only when the Player asked
/// for one.
pub fn asks_to_wait(intent: &str) -> bool {
    let words = crate::normalize::name_words(intent);
    let is = |at: usize, set: &[&str]| words.get(at).is_some_and(|w| set.contains(&w.as_str()));
    let rest = (0..words.len()).any(|i| {
        is(i, &["rest", "rests", "resting"])
            && !(i > 0 && is(i - 1, &["the", "a", "any", "some"]))
            && !is(i + 1, &["of"])
    });
    let stays = (0..words.len())
        .any(|i| is(i, STAY_VERBS) && (i + 1..words.len()).any(|j| is(j, DURATION_WORDS)));
    words.iter().any(|w| WAIT_VERBS.contains(&w.as_str()))
        || rest
        || stays
        || WAIT_PHRASES
            .iter()
            .any(|phrase| words.windows(phrase.len()).any(|w| w == *phrase))
}
