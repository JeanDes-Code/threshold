//! Deterministic text tools: the word-level leak scan ported from SNEQ
//! (`core/containment.ts`), dialogue extraction, name removal and
//! percentiles.

use std::collections::BTreeSet;

use threshold_core::{domain::is_state_key, store::WorldView, world::pc_knowledge};

/// Below this length a token is a fragment, not a name (SNEQ `MIN_TOKEN_LENGTH`).
const MIN_TOKEN_LENGTH: usize = 3;

/// English stopwords from SNEQ `core/stopwords.ts` (the part this slice needs).
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "am", "i", "you", "he",
    "she", "it", "we", "they", "me", "him", "her", "us", "them", "my", "your", "his", "its", "our",
    "their", "mine", "yours", "hers", "ours", "theirs", "this", "that", "these", "those", "such",
    "and", "or", "but", "so", "yet", "nor", "for", "if", "then", "when", "while", "because", "as",
    "of", "at", "by", "with", "from", "into", "onto", "upon", "over", "under", "through", "yes",
    "no", "ok", "okay", "oh", "ah", "hey", "not", "all", "any", "some", "one", "two",
];

/// Lowercase words separated by single spaces, with a space at each end, so
/// `" grain tithe "` matches whole words only.
pub fn padded_words(text: &str) -> String {
    let mapped: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                ' '
            }
        })
        .collect();
    format!(
        " {} ",
        mapped.split_whitespace().collect::<Vec<_>>().join(" ")
    )
}

/// Can this token carry a secret at all? Not a stopword, not a fragment.
fn distinctive(token: &str) -> bool {
    let t = padded_words(token);
    let t = t.trim();
    t.len() >= MIN_TOKEN_LENGTH && !STOPWORDS.contains(&t)
}

/// One forbidden token.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Token {
    /// Whole words: the value of a fact the PC does not hold.
    Phrase(String),
    /// A word stem, declared by the scenario: matches any word that starts
    /// with it ("skim" matches "skims", "skimming").
    Stem(String),
}

impl Token {
    pub fn text(&self) -> &str {
        match self {
            Self::Phrase(t) | Self::Stem(t) => t,
        }
    }

    fn found_in(&self, padded: &str) -> bool {
        match self {
            Self::Phrase(t) => padded.contains(&format!(" {t} ")),
            Self::Stem(t) => padded.contains(&format!(" {t}")),
        }
    }
}

/// The tokens the PC cannot hold after this Turn (the SNEQ floor, adapted):
/// the value of every live, non-state fact whose slot the PC holds no Belief
/// about, plus the scenario's `declared` stems. A token is never forbidden
/// when it is not distinctive, or when it appears in text the PC can
/// legitimately read: the premise, the PC concept, entity names, Voice Cards
/// (the narrator gets them), the PC's Beliefs, or the Player's own Intents.
pub fn forbidden_tokens(view: &WorldView, intents: &[String], declared: &[String]) -> Vec<Token> {
    let knowledge = pc_knowledge(view);
    let mut allowed = String::new();
    if let Some(seed) = &view.adventure_seed {
        allowed.push_str(&seed.premise);
        allowed.push(' ');
        allowed.push_str(&seed.pc.concept);
        allowed.push(' ');
    }
    for e in &view.entities {
        allowed.push_str(&e.name);
        allowed.push(' ');
    }
    for card in &view.voice_cards {
        allowed.push_str(&card.habits);
        allowed.push(' ');
        allowed.push_str(&card.vocabulary);
        allowed.push(' ');
        for s in &card.samples {
            allowed.push_str(s);
            allowed.push(' ');
        }
    }
    for b in &knowledge {
        allowed.push_str(&b.value);
        allowed.push(' ');
        allowed.push_str(&b.text);
        allowed.push(' ');
    }
    for intent in intents {
        allowed.push_str(intent);
        allowed.push(' ');
    }
    let allowed = padded_words(&allowed);

    let mut out = BTreeSet::new();
    for f in &view.live_facts {
        if is_state_key(&f.slot.key) {
            continue;
        }
        let held = knowledge
            .iter()
            .any(|b| b.subject == f.slot.subject && b.key == f.slot.key);
        if held {
            continue;
        }
        let phrase = padded_words(&f.slot.value).trim().to_owned();
        if distinctive(&phrase) && !allowed.contains(&format!(" {phrase} ")) {
            out.insert(Token::Phrase(phrase));
        }
    }
    for stem in declared {
        let stem = padded_words(stem).trim().to_owned();
        if distinctive(&stem) && !allowed.contains(&format!(" {stem}")) {
            out.insert(Token::Stem(stem));
        }
    }
    out.into_iter().collect()
}

/// The forbidden tokens present in `text`.
pub fn leaks(forbidden: &[Token], text: &str) -> Vec<String> {
    let padded = padded_words(text);
    forbidden
        .iter()
        .filter(|t| t.found_in(&padded))
        .map(|t| t.text().to_owned())
        .collect()
}

/// True when `text` contains one of the `stems` (word-start match).
pub fn mentions_any(text: &str, stems: &[String]) -> bool {
    let padded = padded_words(text);
    stems
        .iter()
        .any(|s| padded.contains(&format!(" {}", padded_words(s).trim())))
}

/// The first quoted line of dialogue ("..." or “...”), if any.
pub fn first_quote(text: &str) -> Option<String> {
    let mut open: Option<(usize, char)> = None;
    for (i, c) in text.char_indices() {
        match (open, c) {
            (None, '"' | '\u{201C}') => open = Some((i + c.len_utf8(), c)),
            (Some((start, '"')), '"') | (Some((start, '\u{201C}')), '\u{201D}') => {
                let quote = text[start..i].trim();
                if quote.chars().any(char::is_alphanumeric) {
                    return Some(quote.to_owned());
                }
                open = None;
            }
            _ => {}
        }
    }
    None
}

/// `text` with every whole word of every `name` replaced by `[name]`.
pub fn without_names(text: &str, names: &[String]) -> String {
    let words: BTreeSet<String> = names
        .iter()
        .flat_map(|n| {
            padded_words(n)
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|w| distinctive(w))
        .collect();
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if words.contains(&word.to_lowercase()) {
            out.push_str("[name]");
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Nearest-rank percentile of `values` (`p` in 0..=100). `None` when empty.
pub fn percentile(values: &[u64], p: u64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let n = sorted.len() as u64;
    let rank = (p * n).div_ceil(100).clamp(1, n);
    Some(sorted[(rank - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_is_nearest_rank() {
        let v: Vec<u64> = (1..=10).collect();
        assert_eq!(percentile(&v, 50), Some(5));
        assert_eq!(percentile(&v, 90), Some(9));
        assert_eq!(percentile(&[7], 90), Some(7));
        assert_eq!(percentile(&[], 50), None);
    }

    #[test]
    fn phrases_match_whole_words_and_stems_match_word_starts() {
        let forbidden = vec![
            Token::Phrase("grain tithe".into()),
            Token::Stem("skim".into()),
        ];
        assert_eq!(
            leaks(&forbidden, "He skims the Grain-Tithe."),
            vec!["grain tithe", "skim"]
        );
        assert!(leaks(&forbidden, "The tithe archive; the grain tithes.").is_empty());
        assert_eq!(leaks(&forbidden, "A brisk skimmer"), vec!["skim"]);
        assert!(leaks(&forbidden, "a risky move").is_empty());
    }

    #[test]
    fn first_quote_reads_straight_and_curly_quotes() {
        assert_eq!(
            first_quote("She looks up. \u{201C}Morning, love.\u{201D} Then \"no\".").as_deref(),
            Some("Morning, love.")
        );
        assert_eq!(
            first_quote("He nods. \"Noted.\"").as_deref(),
            Some("Noted.")
        );
        assert_eq!(first_quote("No dialogue here."), None);
    }

    #[test]
    fn names_are_removed_word_by_word() {
        let names = vec!["Orsel".to_owned(), "Varn town hall".to_owned()];
        assert_eq!(
            without_names("Orsel says: welcome to Varn, scribe.", &names),
            "[name] says: welcome to [name], scribe."
        );
    }
}
