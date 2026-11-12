//! The speech mark scenario (play experience plan, Task 11; ADR 0006).
//!
//! It plays Intents that invite dialogue and reads what the narrator did
//! with each spoken line: the share of lines that show as a bubble, the
//! marks that do not resolve and why, and the marks that a judge reads as
//! the wrong speaker. It also times each Turn, as `latency` does.

use std::{collections::HashSet, fmt::Write as _};

use threshold_core::{
    domain::EntityKind,
    normalize,
    player::Marks,
    speech::{self, Line, MAX_NAME_WORDS},
    store::{CampaignId, Store, TurnId},
    view::{Block, Part, TurnBody, play_view},
    world::{pc_sight, tagged},
};

use super::{Ctx, pct, scan::percentile, scenario, secs};

/// The judge's answer when the text does not tell who speaks.
pub const CANNOT_TELL: &str = "cannot tell";

/// What one or more Narrations did with speech.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Lines of speech: each quoted span (`"…"`, `“…”`, `«…»`).
    pub spoken: usize,
    /// Lines that `speech::parse` reads as a mark.
    pub marks: usize,
    /// Marks shown as a bubble (a `Speech` block of the Play view).
    pub resolved: usize,
    /// Marks whose tag the view removed: a name the PC cannot use.
    pub removed: usize,
    /// Marks tagged with the PC's name.
    pub pc: usize,
    /// Marks whose tag names no entity.
    pub no_entity: usize,
    /// Other marks shown as prose: an absent NPC, a tag shared by entities.
    pub other: usize,
    /// Lines of the form `Name: words` with an entity's name and no quote
    /// marks: a mark the narrator wrote without its quotes.
    pub unquoted: usize,
}

impl Tally {
    pub fn unresolved(&self) -> usize {
        self.marks - self.resolved
    }

    /// The mark rate as (resolved, spoken); `None` with no line of speech.
    pub fn rate(&self) -> Option<(usize, usize)> {
        (self.spoken > 0).then_some((self.resolved, self.spoken))
    }

    pub fn add(&mut self, other: &Tally) {
        self.spoken += other.spoken;
        self.marks += other.marks;
        self.resolved += other.resolved;
        self.removed += other.removed;
        self.pc += other.pc;
        self.no_entity += other.no_entity;
        self.other += other.other;
        self.unquoted += other.unquoted;
    }
}

/// A bubble the Play view shows, for the judge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bubble {
    /// The speaker's name, as the view shows it.
    pub speaker: String,
    /// The spoken words, quote marks included.
    pub said: String,
    /// The Narration with every speech tag removed.
    pub context: String,
    /// The NPCs present in the Turn whom the PC can name.
    pub present: Vec<String>,
}

/// One Turn's speech.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnSpeech {
    pub tally: Tally,
    pub bubbles: Vec<Bubble>,
}

/// The quote pairs of a line of speech, as `speech` accepts them.
const QUOTES: [(char, char); 3] = [('"', '"'), ('\u{201c}', '\u{201d}'), ('\u{ab}', '\u{bb}')];

/// The quoted spans in `text`. A span opens with `"`, `“` or `«` and ends at
/// its closing mark; a span with no end does not count.
pub fn quoted_spans(text: &str) -> usize {
    let mut spans = 0;
    let mut close: Option<char> = None;
    for c in text.chars() {
        match close {
            Some(end) if c == end => {
                spans += 1;
                close = None;
            }
            Some(_) => {}
            None => {
                close = QUOTES
                    .iter()
                    .find(|(open, _)| *open == c)
                    .map(|(_, end)| *end);
            }
        }
    }
    spans
}

/// True when `line` reads `Name: words` with the name of an entity and no
/// quote mark at all: the narrator meant a mark and lost its quotes.
fn unquoted_mark(line: &str, is_entity: &dyn Fn(&str) -> bool) -> bool {
    let Some((name, said)) = line.split_once(':') else {
        return false;
    };
    let name = name.trim();
    let words = name.split_whitespace().count();
    (1..=MAX_NAME_WORDS).contains(&words)
        && !said.trim().is_empty()
        && !line
            .chars()
            .any(|c| QUOTES.iter().any(|(open, end)| c == *open || c == *end))
        && is_entity(name)
}

fn block_text(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|p| match p {
            Part::Text { text } | Part::Name { text, .. } => text.as_str(),
        })
        .collect()
}

/// Reads one committed Turn the way the Play screen shows it.
pub fn read_turn(store: &Store, campaign: CampaignId, turn: TurnId) -> Result<TurnSpeech, String> {
    let view = store.snapshot(campaign).map_err(|e| e.to_string())?;
    let turns = store.turns(campaign).map_err(|e| e.to_string())?;
    let reverted: HashSet<TurnId> = store
        .reverted_turns(campaign)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let sight = pc_sight(&view, &turns, &reverted).ok_or("the campaign has no PC")?;
    let Some(narration) = sight
        .story
        .iter()
        .find(|t| t.turn == turn)
        .and_then(|t| t.narration.clone())
    else {
        return Ok(TurnSpeech::default());
    };
    let play = play_view(&sight, &Marks::default());
    let blocks = play
        .story
        .iter()
        .find(|t| t.turn_id == turn)
        .and_then(|t| match &t.body {
            TurnBody::Narration { blocks } => Some(blocks.as_slice()),
            TurnBody::Missing | TurnBody::Faded { .. } => None,
        })
        .unwrap_or_default();
    let lines = speech::parse(&narration);
    if lines.len() != blocks.len() {
        return Err(format!(
            "turn {turn}: {} lines but {} blocks",
            lines.len(),
            blocks.len()
        ));
    }
    // The core's tag rule: a main or other name, or the first names of a
    // character's main name.
    let is_entity = |name: &str| tagged(&view, name).next().is_some();
    let present: Vec<String> = sight
        .moments
        .iter()
        .find(|m| m.turn == turn)
        .map(|m| {
            m.present
                .iter()
                .filter_map(|id| sight.known.iter().find(|k| k.id == *id))
                .filter(|k| k.kind == EntityKind::Npc)
                .filter_map(|k| k.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let pc_name = normalize::name(&sight.pc_name);
    let context = lines
        .iter()
        .map(|line| match line {
            Line::Prose(text) => *text,
            Line::Tagged { said, .. } => *said,
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let mut tally = Tally {
        spoken: quoted_spans(&narration),
        ..Tally::default()
    };
    let mut bubbles = Vec::new();
    for (line, block) in lines.iter().zip(blocks) {
        match (line, block) {
            (Line::Prose(text), _) => {
                tally.unquoted += usize::from(unquoted_mark(text, &is_entity));
            }
            (Line::Tagged { said, .. }, Block::Speech { speaker, .. }) => {
                tally.marks += 1;
                tally.resolved += 1;
                bubbles.push(Bubble {
                    speaker: speaker.name.clone().unwrap_or_default(),
                    said: (*said).to_owned(),
                    context: context.clone(),
                    present: present.clone(),
                });
            }
            (Line::Tagged { name, said, .. }, Block::Prose { parts }) => {
                tally.marks += 1;
                if block_text(parts) == *said {
                    tally.removed += 1;
                } else if normalize::name(name) == pc_name {
                    tally.pc += 1;
                } else if !is_entity(name) {
                    tally.no_entity += 1;
                } else {
                    tally.other += 1;
                }
            }
        }
    }
    Ok(TurnSpeech { tally, bubbles })
}

/// The judge's verdict on one bubble.
enum Verdict {
    Right,
    Wrong,
    CannotTell,
    Error,
}

fn judge(ctx: &mut Ctx, bubble: &Bubble) -> Verdict {
    let mut choices: Vec<&str> = bubble.present.iter().map(String::as_str).collect();
    choices.sort_unstable();
    choices.push(CANNOT_TELL);
    let task = format!(
        "Here is a passage of a story. Speaker names were removed from it.\n\n{}\n\nWho speaks this line: {}\nAnswer with one of the characters present, or \"{CANNOT_TELL}\" when the passage does not show who speaks it.",
        bubble.context, bubble.said
    );
    match ctx.ask(&task, &choices) {
        None => Verdict::Error,
        Some(answer) if answer == CANNOT_TELL => Verdict::CannotTell,
        Some(answer) if normalize::name(&answer) == normalize::name(&bubble.speaker) => {
            Verdict::Right
        }
        Some(_) => Verdict::Wrong,
    }
}

/// One run of the scenario.
#[derive(Default)]
struct RunStats {
    total_ms: Vec<u64>,
    failed: usize,
    tally: Tally,
}

fn rate_text(tally: &Tally) -> String {
    match tally.rate() {
        Some((resolved, spoken)) => format!("{resolved} / {spoken} ({})", pct(resolved, spoken)),
        None => "n/a (no line of speech)".into(),
    }
}

pub(crate) fn scenario(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::Speech = scenario::load(&ctx.config.root, "speech")?;
    let mut runs: Vec<RunStats> = Vec::new();
    let (mut wrong, mut right, mut cannot_tell, mut judge_errors) = (0, 0, 0, 0);
    let mut wrong_samples: Vec<String> = Vec::new();
    for run in 1..=ctx.config.runs {
        let mut s = ctx.session(
            &sc.fixture,
            format!("speech-r{run}"),
            7_000 + run as u64,
            |_, _| Ok(()),
        )?;
        let mut stats = RunStats::default();
        for intent in &sc.intents {
            let played = ctx.play(&mut s, intent);
            let turn = match &played.outcome {
                Ok(o) => {
                    stats.total_ms.push(o.timings.total_ms);
                    o.turn_id
                }
                Err(_) => {
                    stats.failed += 1;
                    continue;
                }
            };
            let read = read_turn(s.engine.store(), s.campaign, turn)?;
            stats.tally.add(&read.tally);
            for bubble in &read.bubbles {
                match judge(ctx, bubble) {
                    Verdict::Right => right += 1,
                    Verdict::Wrong => {
                        wrong += 1;
                        if wrong_samples.len() < 5 {
                            wrong_samples.push(format!(
                                "- marked {}: {}",
                                bubble.speaker,
                                bubble.said.chars().take(100).collect::<String>()
                            ));
                        }
                    }
                    Verdict::CannotTell => cannot_tell += 1,
                    Verdict::Error => judge_errors += 1,
                }
            }
        }
        runs.push(stats);
    }

    let mut total = Tally::default();
    let mut times: Vec<u64> = Vec::new();
    let mut failed = 0;
    for r in &runs {
        total.add(&r.tally);
        times.extend(&r.total_ms);
        failed += r.failed;
    }
    let judged = right + wrong;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} Intents per run that invite dialogue, {} committed Turns, {failed} failed. A line of speech is each quoted span of the stored Narration. A mark resolves when the Play view shows it as a bubble. The judge sees the Narration with every speech tag removed and the present NPCs, and names the speaker of each bubble.\n",
        sc.intents.len(),
        times.len()
    );
    let _ = writeln!(
        out,
        "- Turn time p50 (committed Turns only): {}",
        secs(percentile(&times, 50))
    );
    let _ = writeln!(
        out,
        "- Turn time p90 (committed Turns only): {}",
        secs(percentile(&times, 90))
    );
    let _ = writeln!(
        out,
        "- Turn time max: {}",
        secs(times.iter().max().copied())
    );
    let _ = writeln!(out, "- Lines of speech (quoted spans): {}", total.spoken);
    let _ = writeln!(out, "- Mark rate: {}", rate_text(&total));
    let _ = writeln!(
        out,
        "- Marks: {}; resolved {}; not resolved {} (tag removed, a name the PC cannot use: {}; tag of the PC: {}; tag of no entity: {}; absent NPC or shared tag: {})",
        total.marks,
        total.resolved,
        total.unresolved(),
        total.removed,
        total.pc,
        total.no_entity,
        total.other
    );
    let _ = writeln!(out, "- Marked lines without quotes: {}", total.unquoted);
    let _ = writeln!(
        out,
        "- Wrong marks (judge): {wrong} / {judged} judged bubbles ({}); judge could not tell: {cannot_tell}; judge errors: {judge_errors}",
        pct(wrong, judged)
    );
    for (i, r) in runs.iter().enumerate() {
        let _ = writeln!(
            out,
            "- Run {}: Turn time p50 {}, p90 {}, max {}; mark rate {}; {} failed",
            i + 1,
            secs(percentile(&r.total_ms, 50)),
            secs(percentile(&r.total_ms, 90)),
            secs(r.total_ms.iter().max().copied()),
            rate_text(&r.tally),
            r.failed
        );
    }
    if !wrong_samples.is_empty() {
        let _ = writeln!(out, "\nBubbles the judge gave to another speaker:\n");
        for sample in wrong_samples {
            let _ = writeln!(out, "{sample}");
        }
    }
    Ok(out)
}
