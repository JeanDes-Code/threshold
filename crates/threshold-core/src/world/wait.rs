use super::arrival::pc_witnesses;
use crate::domain::{ClockMin, Trigger};
use crate::store::WorldView;

/// The game minutes a `wait` takes when the player names no duration.
pub const WAIT_DEFAULT_MIN: u32 = 60;

/// The longest single wait, in game minutes. A longer wait is cut to this.
pub const WAIT_MAX_MIN: u32 = 720;

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

/// Number words a duration may use. A ten and a unit add up ("forty five").
const NUMBER_WORDS: &[(&str, u32)] = &[
    ("a", 1),
    ("an", 1),
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
    ("eleven", 11),
    ("twelve", 12),
    ("fifteen", 15),
    ("twenty", 20),
    ("thirty", 30),
    ("forty", 40),
    ("fifty", 50),
    ("sixty", 60),
    ("ninety", 90),
];

/// The units of a duration, in minutes.
const UNITS: &[(&str, u32)] = &[
    ("minute", 1),
    ("minutes", 1),
    ("min", 1),
    ("mins", 1),
    ("hour", 60),
    ("hours", 60),
    ("hr", 60),
    ("hrs", 60),
    ("day", 1440),
    ("days", 1440),
];

/// The words before a named time of day: "until dawn", "till morning".
const UNTIL: &[&str] = &["until", "till", "til", "untill"];

fn number(word: &str) -> Option<u32> {
    word.parse().ok().or_else(|| {
        NUMBER_WORDS
            .iter()
            .find(|(w, _)| *w == word)
            .map(|(_, n)| *n)
    })
}

/// The minutes of the duration whose unit is `words[i]`, read from the
/// words before it (and "and a half" after it).
fn duration_at(words: &[String], i: usize, unit: u32) -> Option<u32> {
    let word = |at: usize| words.get(at).map(String::as_str);
    let before = |back: usize| i.checked_sub(back).and_then(word);
    let half = unit / 2;
    // "half an hour", "half a minute".
    if before(2) == Some("half") && matches!(before(1), Some("a" | "an")) {
        return Some(half.max(1));
    }
    // "a quarter of an hour".
    if before(3) == Some("quarter") && before(2) == Some("of") {
        return Some((unit / 4).max(1));
    }
    // "one and a half hours".
    let mut minutes = if before(3) == Some("and")
        && before(2) == Some("a")
        && before(1) == Some("half")
        && let Some(n) = before(4).and_then(number)
    {
        n * unit + half
    } else {
        let n = before(1).and_then(number)?;
        // "forty five minutes": a ten, then a unit.
        let tens = before(2)
            .and_then(number)
            .filter(|t| *t >= 20 && t % 10 == 0 && n < 10 && !matches!(before(2), Some("a" | "an")))
            .unwrap_or(0);
        (tens + n) * unit
    };
    // "an hour and a half".
    if word(i + 1) == Some("and") && word(i + 2) == Some("a") && word(i + 3) == Some("half") {
        minutes += half;
    }
    Some(minutes)
}

/// The game minutes the Intent states for a wait (bug pass 5, M3): a number
/// or "a", "an" or "half an" with minutes or hours ("for an hour", "30
/// minutes", "an hour and a half", "eight hours"), or "until" a named time
/// of day (`rules::NAMED_TIMES`: "until dawn" from 21:00 is 540 minutes),
/// counted from `clock`. Two durations joined by "and" add up ("two hours
/// and ten minutes"). `None` when the Intent states none: "I wait a while"
/// leaves the length to the engine. No model call.
pub fn intent_minutes(intent: &str, clock: ClockMin) -> Option<u32> {
    let words = crate::normalize::name_words(intent);
    for (i, word) in words.iter().enumerate() {
        if !UNTIL.contains(&word.as_str()) {
            continue;
        }
        let next = words[i + 1..]
            .iter()
            .find(|w| !matches!(w.as_str(), "the" | "it" | "is" | "its" | "s"));
        if let Some(at) = next.and_then(|w| {
            crate::rules::NAMED_TIMES
                .iter()
                .find(|(name, _)| name == w)
                .map(|(_, at)| *at)
        }) {
            let now = u32::try_from(clock % 1440).unwrap_or(0);
            let until = (at + 1440 - now) % 1440;
            return Some(if until == 0 { 1440 } else { until });
        }
    }
    let mut total: Option<u32> = None;
    let mut last_unit: Option<usize> = None;
    for (i, word) in words.iter().enumerate() {
        let Some(unit) = UNITS.iter().find(|(w, _)| w == word).map(|(_, u)| *u) else {
            continue;
        };
        let Some(minutes) = duration_at(&words, i, unit) else {
            continue;
        };
        match (total, last_unit) {
            (None, _) => total = Some(minutes),
            // A second duration counts when "and" joins it to the first.
            (Some(t), Some(at)) if words[at + 1..i].iter().any(|w| w == "and") && i - at <= 4 => {
                total = Some(t + minutes);
            }
            _ => break,
        }
        last_unit = Some(i);
    }
    total.filter(|m| *m > 0)
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
