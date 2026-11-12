//! The speech mark: who speaks a line of Narration (play experience data
//! shape 3.1, ADR 0006).
//!
//! The narrator writes an NPC's spoken line on a line of its own: the
//! NPC's name, a colon, then the words in quotes. `RULE` asks for it and
//! `parse` reads it, so the writer side and the reader side of the format
//! live here. The mark only says who the narrator meant; `world::pc_sight`
//! decides if the line may show as a bubble. `TagFilter` drops the tag of a
//! name the PC cannot use from the stream, before the Commit.

use std::collections::HashSet;

use crate::normalize;

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
    /// where the mark does not resolve and the line shows as prose. `note`
    /// is a short stage note before the quote, without its parentheses.
    /// `said` is everything after the colon (the note, then the spoken
    /// words with their quote marks), for the case where the tag is a name
    /// the PC cannot name and must not show (ADR 0006): the stream shows
    /// that text too.
    Tagged {
        line: &'a str,
        name: &'a str,
        note: Option<&'a str>,
        words: &'a str,
        said: &'a str,
    },
}

/// The lines of `narration`, blank lines left out. A line is a mark when
/// the whole line is a name of one to four words, a colon, an optional short
/// stage note in parentheses (kept apart), then the spoken words in `"…"`,
/// `“…”` or `«…»`, with nothing after the closing quote. Any other line is
/// prose, with its text unchanged.
pub fn parse(narration: &str) -> Vec<Line<'_>> {
    narration
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| mark(line).unwrap_or(Line::Prose(line)))
        .collect()
}

/// The tag and the rest of a line that has the shape of a speech mark's
/// start: one to four words with no quote mark, then a colon.
fn tag(line: &str) -> Option<(&str, &str)> {
    let (name, said) = line.split_once(':')?;
    let name = name.trim();
    let words = name.split_whitespace().count();
    if !(1..=MAX_NAME_WORDS).contains(&words)
        || name.contains(|c| QUOTES.iter().any(|&(open, close)| c == open || c == close))
    {
        return None;
    }
    Some((name, said))
}

/// `text` without its last line when the token cap cut that line inside a
/// speech mark (review B2, 2026-11-12): the line opens a quote it does not
/// close, or it has a speech tag (`tag`) and no complete quoted span after
/// it. The reader of a stored Narration (`parse`) would show such a line as
/// prose, its tag included, so a cut could print a name that the full
/// Narration hides; the stream filter held that tag back. The text then
/// ends at the end of the line before it. Any other text comes back
/// unchanged, trailing blanks removed.
pub fn without_cut_mark(text: &str) -> &str {
    let text = text.trim_end();
    let start = text.rfind('\n').map_or(0, |at| at + 1);
    let last = &text[start..];
    let cut = opens_a_quote(last) || tag(last).is_some_and(|(_, said)| !holds_a_quote(said));
    if cut { text[..start].trim_end() } else { text }
}

/// True when `line` opens a quote it does not close.
fn opens_a_quote(line: &str) -> bool {
    QUOTES.iter().any(|&(open, close)| {
        if open == close {
            line.matches(open).count() % 2 == 1
        } else {
            line.matches(open).count() > line.matches(close).count()
        }
    })
}

/// True when `text` holds a complete quoted span: an opening quote mark,
/// then its closing mark after it.
fn holds_a_quote(text: &str) -> bool {
    QUOTES.iter().any(|&(open, close)| {
        text.find(open)
            .is_some_and(|at| text[at + open.len_utf8()..].contains(close))
    })
}

/// The tag and the text after its colon of a prose line that starts like a
/// speech mark: a tag (`tag`), an optional stage note, then a complete
/// quoted span, whatever follows the closing quote (`Vale: "Go." She
/// turns.`). Such a line is no mark (`parse` reads it as prose), but the
/// stream filter drops its tag when the tag is a name the PC cannot use, so
/// the readers of a stored Narration drop it too (ADR 0006, 2026-11-12):
/// `world::pc_sight` shows the text after the colon, and
/// `world::story_names` never counts the tag. `None` for any other line.
pub fn leading_tag(line: &str) -> Option<(&str, &str)> {
    let (name, said) = tag(line)?;
    let said = said.trim();
    let (_, quoted) = stage_note(said);
    let (open, close) = QUOTES
        .into_iter()
        .find(|&(open, _)| quoted.starts_with(open))?;
    quoted[open.len_utf8()..]
        .contains(close)
        .then_some((name, said))
}

/// The tag and the text after its colon of any line, as the stream filter
/// reads them (`TagFilter`): the text before the line's first colon, when
/// it holds fewer than `TAG_HOLD_CHARS` characters, trimmed, and the text
/// after the colon with its leading blanks dropped. Whatever follows the
/// colon: any quote form, emphasis marks, a dash, a stage note of any
/// length, or a quote that closes on a later line (final review minor 1,
/// 2026-11-12). The readers of a stored Narration drop such a tag exactly
/// when the stream does: when it is a name the PC cannot use
/// (`world::pc_sight`), and never count it as a name the PC read
/// (`world::story_names`). `None` with no colon, or with a colon too far
/// for the stream to hold the text before it.
pub fn line_tag(line: &str) -> Option<(&str, &str)> {
    let (name, said) = line.split_once(':')?;
    (name.chars().count() < TAG_HOLD_CHARS).then(|| (name.trim(), said.trim_start()))
}

fn mark(line: &str) -> Option<Line<'_>> {
    let (name, said) = tag(line)?;
    let said = said.trim();
    let (note, quoted) = stage_note(said);
    let (open, close) = QUOTES
        .into_iter()
        .find(|&(open, _)| quoted.starts_with(open))?;
    let inner = quoted.strip_prefix(open)?.strip_suffix(close)?;
    let inner_words = inner.trim();
    if inner_words.is_empty() || inner.contains(close) {
        return None;
    }
    Some(Line::Tagged {
        line,
        name,
        note,
        words: inner_words,
        said,
    })
}

/// The longest stage note a mark may hold between its colon and its quote,
/// parentheses included, in characters.
pub const MAX_STAGE_NOTE_CHARS: usize = 60;

/// A short stage note before the quote, without its parentheses, and the
/// text after it: QA saw `Agathe Morrow: (Wiping the bar) "…"` shown as a
/// script line (bug pass 4). The note is kept for the bubble's stage line
/// (review minor: "hands you the sealed letter" was lost after the
/// Commit). A note that is long, holds a quote mark or never closes is no
/// note: `said` comes back whole.
fn stage_note(said: &str) -> (Option<&str>, &str) {
    let Some(rest) = said.strip_prefix('(') else {
        return (None, said);
    };
    let Some(end) = rest.find(')') else {
        return (None, said);
    };
    let note = &rest[..end];
    let quoted = note.contains(|c| QUOTES.iter().any(|&(open, close)| c == open || c == close));
    if quoted || note.chars().count() + 2 > MAX_STAGE_NOTE_CHARS || note.trim().is_empty() {
        return (None, said);
    }
    (Some(note.trim()), rest[end + 1..].trim_start())
}

/// The longest text the stream filter holds back at the start of a line,
/// in characters, before it knows that the line has no tag to drop.
pub const TAG_HOLD_CHARS: usize = 48;

/// Where the stream filter is in the current line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum At {
    /// At the start of a line: the text is held until a colon, a newline or
    /// `TAG_HOLD_CHARS` characters.
    Start,
    /// A tag was dropped: the blanks after its colon are dropped too.
    AfterTag,
    /// The rest of a line: each character passes at once.
    Rest,
}

/// The stream side of the tag rule (data shape 3.1, ADR 0006). It sits
/// between the narrator's chunks and the chunks the Player sees. At each
/// line start it holds the text back until a colon, a newline or
/// `TAG_HOLD_CHARS` characters. When the text before the colon is, through
/// `normalize::name`, one of the `hidden` names, the filter drops it, the
/// colon and the blanks after the colon; any other text passes unchanged.
/// So a tag that names someone the PC cannot name never shows, not even
/// while the Narration streams. The stored Narration does not change. It
/// makes no model call, and it holds back at most one line start.
#[derive(Clone, Debug)]
pub struct TagFilter {
    hidden: HashSet<String>,
    held: String,
    held_chars: usize,
    at: At,
}

impl TagFilter {
    /// A filter for one Narration. `hidden` holds the names, through
    /// `normalize::name`, that a tag must not print
    /// (`world::unnameable_names`).
    pub fn new(hidden: HashSet<String>) -> Self {
        Self {
            hidden,
            held: String::new(),
            held_chars: 0,
            at: At::Start,
        }
    }

    /// Takes one chunk of the narrator; returns the text to show now, which
    /// can be empty.
    pub fn push(&mut self, chunk: &str) -> String {
        let mut out = String::with_capacity(chunk.len());
        for c in chunk.chars() {
            match self.at {
                At::Start => match c {
                    '\n' => {
                        out.push_str(&self.release());
                        out.push(c);
                    }
                    ':' => {
                        let name = normalize::name(&self.held);
                        if !name.is_empty() && self.hidden.contains(&name) {
                            self.release();
                            self.at = At::AfterTag;
                        } else {
                            out.push_str(&self.release());
                            out.push(c);
                            self.at = At::Rest;
                        }
                    }
                    _ => {
                        self.held.push(c);
                        self.held_chars += 1;
                        if self.held_chars >= TAG_HOLD_CHARS {
                            out.push_str(&self.release());
                            self.at = At::Rest;
                        }
                    }
                },
                At::AfterTag if c == ' ' || c == '\t' => {}
                At::AfterTag | At::Rest => {
                    out.push(c);
                    self.at = if c == '\n' { At::Start } else { At::Rest };
                }
            }
        }
        out
    }

    /// The end of the Narration: returns the text still held.
    pub fn finish(&mut self) -> String {
        self.at = At::Start;
        self.release()
    }

    fn release(&mut self) -> String {
        self.held_chars = 0;
        std::mem::take(&mut self.held)
    }
}

/// `text` as the stream filter shows it: each line whose text before its
/// first colon is one of the `hidden` names loses that text, the colon and
/// the blanks after it, unless the line reached `TAG_HOLD_CHARS` characters
/// before the colon.
pub fn strip_tags(text: &str, hidden: &HashSet<String>) -> String {
    let mut filter = TagFilter::new(hidden.clone());
    let mut out = filter.push(text);
    out.push_str(&filter.finish());
    out
}
