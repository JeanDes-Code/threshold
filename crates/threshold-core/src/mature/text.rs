//! What a Narration may tell of an intimate Turn (G4, data shape 3.5), and
//! what a reader gets of a Turn committed with `intimate` (data shape 3.6).
//! A plain Turn is told as any Turn; an intimate Turn by implication, with
//! the core's own line (`context::IMPLIED_LINE`); and, inside a cleared
//! open scene at an effective `explicit` level, with the explicit-only
//! fragments of a capable Content Pack (`Told::Explicit`, ADR 0009). The
//! switch is this code, never pack wording or model output. `turn_text` is
//! the one function that says what a prompt or the OOC sheet reads of a
//! Turn, and its neutral line.

use super::{gate::Cleared, scene::narration_clearance};
use crate::{
    domain::DetailLevel,
    pack::ContentPack,
    store::{StoredTurn, TurnId, TurnKind, WorldView},
};

/// The Neutral Line of a Turn committed with `intimate` (glossary). The
/// core fills `{minutes}` with the minutes between the Turn's
/// `clock_before` and `clock_after` ("5 minutes"). It holds no name, no
/// fact value and no model text. The owner reads it before the merge.
pub const INTIMATE_TEMPLATE: &str = "An intimate moment passes ({minutes}). It is not retold here.";

/// The Neutral Line of a Closing Turn (glossary). The core fills
/// `{minutes}` as for `INTIMATE_TEMPLATE`. It holds no name, no fact value,
/// no model text and no act of the PC. The owner reads it before the merge.
pub const CLOSING_TEMPLATE: &str = "The intimate scene ends ({minutes}).";

/// The Neutral Line of a faded Play or Correction Turn (Fade to Black,
/// glossary). The core fills `{minutes}` as for `INTIMATE_TEMPLATE`. It says
/// only that the Player skipped this part: no name, no fact value, no model
/// text, and nothing about what kind of Turn it was. The owner reads it
/// before the merge.
pub const FADED_TEMPLATE: &str = "This part of the story is skipped ({minutes}).";

/// What a reader gets for one Turn: its Intent and its Narration in full,
/// or one Neutral Line in place of both.
#[derive(Debug, PartialEq, Eq)]
pub enum TurnText<'a> {
    Full {
        intent: &'a str,
        narration: Option<&'a str>,
    },
    Line(String),
}

/// What a prompt (the `recent` block of both prompts) or the OOC sheet
/// reads of `turn`. A Turn committed with `intimate` is read `Full` only by
/// a prompt that holds a `Cleared` value and whose open scene (`scene`, the
/// Turn that opened it) started at or before that Turn; every other reader
/// gets its Neutral Line: a plain Turn after the scene, the OOC sheet, a
/// prompt whose place is no longer cleared. Any other Turn is `Full`.
pub fn turn_text<'a>(
    turn: &'a StoredTurn,
    scene: Option<TurnId>,
    cleared: Option<&Cleared>,
) -> TurnText<'a> {
    let full = TurnText::Full {
        intent: &turn.record.intent,
        narration: turn.narration.as_deref(),
    };
    match neutral_line(turn) {
        None => full,
        // A faded Turn is its line for every reader (ADR 0011).
        Some(line) if turn.faded => TurnText::Line(line),
        Some(_) if cleared.is_some() && scene.is_some_and(|since| since <= turn.id) => full,
        Some(line) => TurnText::Line(line),
    }
}

/// The Neutral Line of `turn`, by kind: a Closing Turn, faded or not
/// (`CLOSING_TEMPLATE`); a faded Play or Correction Turn (`FADED_TEMPLATE`);
/// a Play or Correction Turn committed with `intimate`
/// (`INTIMATE_TEMPLATE`). `None` for any other Turn. Made from the Turn row
/// only; no model call.
pub fn neutral_line(turn: &StoredTurn) -> Option<String> {
    let template = if turn.record.kind == TurnKind::Closing {
        CLOSING_TEMPLATE
    } else if turn.faded && turn.record.kind.has_intent() {
        FADED_TEMPLATE
    } else if turn.record.kind.has_intent() && stored_intimate(&turn.record.proposal_json) {
        INTIMATE_TEMPLATE
    } else {
        return None;
    };
    let minutes = turn
        .record
        .clock_after
        .saturating_sub(turn.record.clock_before);
    let minutes = if minutes == 1 {
        "1 minute".to_owned()
    } else {
        format!("{minutes} minutes")
    };
    Some(template.replace("{minutes}", &minutes))
}

/// How the narrator tells one Turn.
#[derive(Debug)]
pub enum Told {
    /// Not an intimate Turn: told as any Turn, under the core's stable rule
    /// that a Narration describes no intimate content unless a line of the
    /// Turn allows it (`context::INTIMATE_RULE`).
    Plain,
    /// An intimate Turn: told by implication only. The narration prompt
    /// opens its `recent` block with `context::IMPLIED_LINE`.
    Implied,
    /// An intimate Turn of a cleared open scene at an effective `explicit`
    /// level: the narration prompt opens its `recent` block with
    /// `context::EXPLICIT_LINE` and the narration scene fragments of the
    /// capable packs, the explicit-only ones included. It holds the
    /// clearance of the state after the Turn: only `clearance` makes one.
    Explicit(Cleared),
}

/// The Detail Level a Campaign plays at (data shape 3.1): `explicit` only
/// when the stored level is `explicit` and a loaded pack declares that it
/// gives explicit description; `implied` for a stored `explicit` in any
/// other case. Any other stored level is itself. The stored level never
/// changes here.
pub fn effective(stored: DetailLevel, packs: &[ContentPack]) -> DetailLevel {
    match stored {
        DetailLevel::Explicit if packs.iter().any(ContentPack::explicit) => DetailLevel::Explicit,
        DetailLevel::Explicit => DetailLevel::Implied,
        other => other,
    }
}

/// G4 (data shape 3.5): how the narrator tells a Turn.
///
/// ```text
/// not intimate                                          -> Plain
/// intimate, and the scene is open after this Commit,
///   and it is still the open scene now (`open_scene(now)`),
///   and effective(now.detail, packs) is explicit,
///   and clearance(view_after) passes,
///   and clearance(now) passes                           -> Explicit(Cleared)
/// intimate, any other case                              -> Implied
/// ```
///
/// The clearance is `narration_clearance` (review B1, 2026-11-12).
///
/// A Turn that ends a scene is never told at `Explicit`, and neither is a
/// `renarrate` of a Turn of a scene that has ended. `now` and `packs` are
/// read at each call: a pack removed or a level changed during a scene
/// takes effect on the next Narration.
pub fn told(
    view_after: &WorldView,
    now: &WorldView,
    intimate: bool,
    packs: &[ContentPack],
) -> Told {
    if !intimate {
        return Told::Plain;
    }
    if effective(now.detail, packs) != DetailLevel::Explicit {
        return Told::Implied;
    }
    narration_clearance(view_after, now).map_or(Told::Implied, Told::Explicit)
}

/// True when a Turn's stored `proposal_json` holds a proposal committed with
/// `intimate: true`. A record without the key, or that does not parse,
/// reads as false: such a Turn was committed before the tag existed, or is
/// not a Play or Correction Turn.
pub fn stored_intimate(proposal_json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(proposal_json).is_ok_and(|record| {
        record.pointer("/proposal/intimate") == Some(&serde_json::Value::Bool(true))
    })
}
