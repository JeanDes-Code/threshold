//! The speech mark: who speaks a line of Narration (play experience data
//! shape 3.1, ADR 0006).
//!
//! The narrator writes an NPC's spoken line on a line of its own: the
//! NPC's name, a colon, then the words in quotes. `RULE` asks for it and
//! `parse` reads it, so the writer side and the reader side of the format
//! live here. The mark only says who the narrator meant; `world::pc_sight`
//! decides if the line may show as a bubble.

/// The text of `RULE`, as a literal, so `NARRATION_RULES` can hold it with
/// `concat!`.
macro_rules! rule_text {
    () => {
        "When an NPC speaks, put each spoken line on a line of its own: the NPC's name, a colon, then the words in quotes."
    };
}
pub(crate) use rule_text;

/// The one rule line the narrator reads. It is the last line of
/// `context::NARRATION_RULES`.
pub const RULE: &str = rule_text!();

/// A mark names its speaker in one to four words.
pub const MAX_NAME_WORDS: usize = 4;

/// The quote pairs a mark accepts: straight, curly and French.
const QUOTES: [(char, char); 3] = [('"', '"'), ('\u{201c}', '\u{201d}'), ('\u{ab}', '\u{bb}')];

/// One line of a Narration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line<'a> {
    /// A line with no mark, as the Narration wrote it.
    Prose(&'a str),
    /// A speech mark. `line` is the whole line as written, for the case
    /// where the mark does not resolve and the line shows as prose. `said`
    /// is the spoken words with their quote marks, for the case where the
    /// tag is a name the PC cannot name and must not show (ADR 0006).
    Tagged {
        line: &'a str,
        name: &'a str,
        words: &'a str,
        said: &'a str,
    },
}

/// The lines of `narration`, blank lines left out. A line is a mark when
/// the whole line is a name of one to four words, a colon, then the spoken
/// words in `"…"`, `“…”` or `«…»`, with nothing after the closing quote.
/// Any other line is prose, with its text unchanged.
pub fn parse(narration: &str) -> Vec<Line<'_>> {
    narration
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| mark(line).unwrap_or(Line::Prose(line)))
        .collect()
}

fn mark(line: &str) -> Option<Line<'_>> {
    let (name, said) = line.split_once(':')?;
    let name = name.trim();
    let words = name.split_whitespace().count();
    if !(1..=MAX_NAME_WORDS).contains(&words)
        || name.contains(|c| QUOTES.iter().any(|&(open, close)| c == open || c == close))
    {
        return None;
    }
    let said = said.trim();
    let (open, close) = QUOTES
        .into_iter()
        .find(|&(open, _)| said.starts_with(open))?;
    let inner = said.strip_prefix(open)?.strip_suffix(close)?;
    let inner_words = inner.trim();
    if inner_words.is_empty() || inner.contains(close) {
        return None;
    }
    Some(Line::Tagged {
        line,
        name,
        words: inner_words,
        said,
    })
}
