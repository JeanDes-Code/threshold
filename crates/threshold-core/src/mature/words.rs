//! The deterministic net under the intimacy tag (ADR 0008, changed after
//! the live evaluation of 2026-11-12): the core reads the Player's Intent
//! for a short list of neutral romance words and phrases, and raises the
//! Turn's tag to at least `advance` when one is there, whatever the model
//! says. It never lowers a model value. It over-blocks by design: a false
//! `advance` costs one plain Turn, or one Refusal where a character is not
//! cleared; a missed tag runs no age rule.

use crate::{domain::Intimacy, normalize};

/// The Romance Words (glossary): neutral words and phrases that mark a
/// romantic or sexual advance or invitation in an Intent. Each term is
/// matched as whole words, in order, with case and accents folded
/// (`normalize::name_words`). Nothing on it is more explicit than these
/// words. Why each group is here, and why some near words are not:
///
/// - Spending the night: "night with" and "night together", not "spend the
///   night" alone ("I spend the night on watch"); "stay with me tonight",
///   not "with me tonight" ("finish the roll with me tonight").
/// - Going home or to bed together: "come home with me"; "to bed with",
///   "come to bed", "into bed with", "share a bed", "share my bed", "share
///   your bed", "share her bed", "share his bed"; not "to bed" alone ("I go
///   to bed early" is a wait) and not "my bed".
/// - Sleeping with someone: the phrase with a person only ("sleep with
///   her"), not "sleep with" alone ("I sleep with my sword at hand").
/// - Acts of affection and courtship: kiss, embrace, caress, undress,
///   seduce, flirt, make love, lover, romance, court her or him, intimate,
///   intimacy, with their usual forms. "court" alone is a place of law.
pub const ROMANCE_WORDS: &[&str] = &[
    "night with",
    "night together",
    "stay with me tonight",
    "come home with me",
    "to bed with",
    "come to bed",
    "into bed with",
    "share a bed",
    "share my bed",
    "share your bed",
    "share her bed",
    "share his bed",
    "sleep with me",
    "sleep with you",
    "sleep with her",
    "sleep with him",
    "sleep with them",
    "slept with me",
    "slept with you",
    "slept with her",
    "slept with him",
    "kiss",
    "kisses",
    "kissed",
    "kissing",
    "embrace",
    "embraces",
    "embraced",
    "embracing",
    "caress",
    "caresses",
    "caressed",
    "caressing",
    "undress",
    "undresses",
    "undressed",
    "undressing",
    "make love",
    "makes love",
    "made love",
    "making love",
    "seduce",
    "seduces",
    "seduced",
    "seducing",
    "seduction",
    "lover",
    "lovers",
    "flirt",
    "flirts",
    "flirted",
    "flirting",
    "romance",
    "romantic",
    "court her",
    "court him",
    "courting",
    "intimate",
    "intimacy",
];

/// The words of `text` as the net reads them: lower case, accents dropped,
/// split on every character that is not a letter or a digit.
pub(crate) fn folded(text: &str) -> Vec<String> {
    normalize::name_words(text)
}

/// True when `words` holds the words of `term` next to each other, in
/// order. A term with no word matches nothing.
pub(crate) fn holds(words: &[String], term: &[String]) -> bool {
    !term.is_empty() && words.windows(term.len()).any(|w| w == term)
}

/// The tag the core's list gives `intent`: `advance` when it holds a
/// Romance Word, else `none`. The Turn's tag is the stronger of this and
/// the model's (`turn::plan`).
pub fn romance_floor(intent: &str) -> Intimacy {
    let words = folded(intent);
    let hit = ROMANCE_WORDS
        .iter()
        .any(|term| holds(&words, &folded(term)));
    if hit {
        Intimacy::Advance
    } else {
        Intimacy::None
    }
}
