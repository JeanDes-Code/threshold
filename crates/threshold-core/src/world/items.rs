//! Which Open Items a Player's Intent names (bug pass 3, 2026-11-11).
//!
//! A Campaign built from a Source Text can hold a dozen Open Items. Offered
//! all of them on every Turn, the live model closed unrelated ones. The
//! proposal now offers only the items the Intent names: a deterministic
//! word match, no model call. A missed item stays open, which the Player
//! can fix by naming it; a wrong close is the worse error.

use std::collections::HashSet;

use super::beliefs::{pc_knowledge, pc_open_items};
use crate::{
    domain::{EntityId, EntityKind, Key, OPEN_KEY_PREFIX},
    normalize,
    store::WorldView,
};

/// The shortest word that can match. Shorter words ("yes", "ask", "the")
/// say nothing about an item.
const MIN_WORD: usize = 4;

/// Words of at least `MIN_WORD` letters that name no matter: function
/// words, verbs of speech and thought that any Intent uses, and everyday
/// verbs and nouns of play (review M2: "meet", "plans", "time" or "night"
/// offered items that only used the same word). A stop word matches as
/// the Intent spells it, before the stem: "meeting" still counts.
const STOP_WORDS: &[&str] = &[
    // Function words.
    "about",
    "above",
    "after",
    "again",
    "against",
    "also",
    "anything",
    "around",
    "back",
    "been",
    "before",
    "behind",
    "being",
    "below",
    "both",
    "could",
    "does",
    "doing",
    "down",
    "each",
    "else",
    "even",
    "ever",
    "every",
    "from",
    "further",
    "have",
    "having",
    "here",
    "into",
    "just",
    "like",
    "many",
    "more",
    "most",
    "much",
    "must",
    "never",
    "next",
    "nothing",
    "once",
    "only",
    "other",
    "over",
    "please",
    "really",
    "same",
    "should",
    "some",
    "something",
    "still",
    "such",
    "than",
    "that",
    "their",
    "them",
    "then",
    "there",
    "these",
    "they",
    "thing",
    "things",
    "this",
    "those",
    "through",
    "under",
    "until",
    "upon",
    "very",
    "well",
    "were",
    "what",
    "when",
    "where",
    "whether",
    "which",
    "while",
    "will",
    "shall",
    "might",
    "with",
    "would",
    "your",
    "yours",
    "yourself",
    "myself",
    "himself",
    "herself",
    "maybe",
    "perhaps",
    "sure",
    "okay",
    "fine",
    "good",
    "thank",
    "thanks",
    "pleased",
    // Speech and thought.
    "tell",
    "tells",
    "telling",
    "told",
    "said",
    "says",
    "saying",
    "speak",
    "speaks",
    "talk",
    "talks",
    "talking",
    "know",
    "knows",
    "think",
    "thinks",
    "thought",
    "want",
    "wants",
    "wish",
    "need",
    "needs",
    "greet",
    "greets",
    "answer",
    "answers",
    "reply",
    "joke",
    "jokes",
    "story",
    "stories",
    "question",
    "questions",
    // Everyday acts.
    "make",
    "makes",
    "made",
    "meet",
    "meets",
    "plan",
    "plans",
    "planned",
    "planning",
    "accept",
    "accepts",
    "accepted",
    "offer",
    "offers",
    "offered",
    "give",
    "gives",
    "gave",
    "given",
    "take",
    "takes",
    "took",
    "taken",
    "come",
    "comes",
    "came",
    "coming",
    "goes",
    "going",
    "went",
    "gone",
    "look",
    "looks",
    "looking",
    "walk",
    "walks",
    "walking",
    "stand",
    "stands",
    "turn",
    "turns",
    "keep",
    "keeps",
    "kept",
    "leave",
    "leaves",
    "left",
    "stay",
    "stays",
    "wait",
    "waits",
    "find",
    "finds",
    "show",
    "shows",
    "bring",
    "brings",
    "brought",
    "help",
    "helps",
    // Everyday nouns of time and place.
    "time",
    "times",
    "town",
    "night",
    "nights",
    "tonight",
    "today",
    "tomorrow",
    "yesterday",
    "days",
    "morning",
    "evening",
    "week",
    "weeks",
    "year",
    "years",
    "hour",
    "hours",
    "moment",
    "place",
    "people",
    "someone",
    "anyone",
    "everyone",
    "stranger",
    "friend",
];

/// The Open Items the PC holds (`pc_open_items`) that `intent` names, in
/// the same order. An item is named when a content word of the Intent
/// matches a word of the item's text as the PC knows it, of its open key
/// without the prefix, or of its subject's main name (`same_word`). The
/// PC's own names match nothing: the PC is in most items.
pub fn items_named_by(view: &WorldView, intent: &str) -> Vec<(EntityId, Key)> {
    let pc_words: HashSet<String> = view
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::Pc)
        .flat_map(|e| std::iter::once(&e.name).chain(&e.other_names))
        .flat_map(|name| normalize::name_words(name))
        .collect();
    let words = |text: &str| -> Vec<String> {
        normalize::name_words(text)
            .into_iter()
            .filter(|w| w.chars().count() >= MIN_WORD)
            .filter(|w| !STOP_WORDS.contains(&w.as_str()) && !pc_words.contains(w))
            .map(|w| stem(&w))
            .collect()
    };
    let asked = words(intent);
    if asked.is_empty() {
        return Vec::new();
    }
    let known = pc_knowledge(view);
    pc_open_items(view)
        .into_iter()
        .filter(|(subject, key)| {
            let text = known
                .iter()
                .find(|b| b.subject == *subject && b.key == *key)
                .map_or("", |b| b.text.as_str());
            let name = view.entities.iter().find(|e| e.id == *subject);
            let key_words = key.strip_prefix(OPEN_KEY_PREFIX).unwrap_or(key);
            let item = words(&format!(
                "{text} {} {}",
                key_words.replace('_', " "),
                name.map_or("", |e| e.name.as_str())
            ));
            item.iter().any(|w| asked.iter().any(|a| same_word(a, w)))
        })
        .collect()
}

/// A short English stem: "carrying" and "carried" are "carry", "posts" is
/// "post". Enough for a match, not a dictionary.
fn stem(word: &str) -> String {
    for (suffix, with) in [
        ("ies", "y"),
        ("ied", "y"),
        ("ing", ""),
        ("ed", ""),
        ("es", ""),
        ("s", ""),
    ] {
        if let Some(root) = word.strip_suffix(suffix)
            && root.chars().count() + with.len() >= MIN_WORD
            && !word.ends_with("ss")
        {
            return format!("{root}{with}");
        }
    }
    word.to_owned()
}

/// The shortest stem that may match as the start of a longer word.
const MIN_PREFIX: usize = 5;

/// Two stems match when they are equal, or when the shorter, of at least
/// `MIN_PREFIX` letters, starts the longer: "carry" and "carryall", never
/// "even" and "event" or "hand" and "handled".
fn same_word(a: &str, b: &str) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short == long || short.chars().count() >= MIN_PREFIX && long.starts_with(short)
}
