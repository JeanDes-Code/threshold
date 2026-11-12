//! What a Narration may tell of an intimate Turn (G4, data shape 3.5), and
//! what a reader gets of a Turn committed with `intimate` (data shape 3.6).
//! Task 2 of the plan has two telling modes: a plain Turn is told as any
//! Turn, an intimate Turn by implication only, with the core's own line
//! (`context::IMPLIED_LINE`). No pack and no model output choose the mode.
//! Task 4 adds `turn_text`, the one function that says what a prompt or
//! the OOC sheet reads of a Turn, and its neutral line.

use super::gate::Cleared;
use crate::store::{StoredTurn, TurnId, TurnKind, WorldView};

/// The Neutral Line of a Turn committed with `intimate` (glossary). The
/// core fills `{minutes}` with the minutes between the Turn's
/// `clock_before` and `clock_after` ("5 minutes"). It holds no name, no
/// fact value and no model text. The owner reads it before the merge.
pub const INTIMATE_TEMPLATE: &str = "An intimate moment passes ({minutes}). It is not retold here.";

/// The Neutral Line of a Closing Turn (glossary). The core fills
/// `{minutes}` as for `INTIMATE_TEMPLATE`. It holds no name, no fact value,
/// no model text and no act of the PC. The owner reads it before the merge.
pub const CLOSING_TEMPLATE: &str = "The intimate scene ends ({minutes}).";

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
        Some(_) if cleared.is_some() && scene.is_some_and(|since| since <= turn.id) => full,
        Some(line) => TurnText::Line(line),
    }
}

/// The Neutral Line of `turn`: `Some` for a Play or Correction Turn
/// committed with `intimate` (`INTIMATE_TEMPLATE`) and for a Closing Turn
/// (`CLOSING_TEMPLATE`), `None` for any other Turn. Made from the Turn row
/// only; no model call.
pub fn neutral_line(turn: &StoredTurn) -> Option<String> {
    let template = if turn.record.kind == TurnKind::Closing {
        CLOSING_TEMPLATE
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
#[derive(Debug, PartialEq, Eq)]
pub enum Told {
    /// Not an intimate Turn: told as any Turn, under the core's stable rule
    /// that a Narration describes no intimate content unless a line of the
    /// Turn allows it (`context::INTIMATE_RULE`).
    Plain,
    /// An intimate Turn: told by implication only. The narration prompt
    /// opens its `recent` block with `context::IMPLIED_LINE`.
    Implied,
}

/// The telling of a Turn committed with `intimate`: `Implied`. Any other
/// Turn: `Plain`. The state right after the Turn and the state now are
/// unread for now: explicit telling (Task 7 of the plan) reads both.
pub fn told(_view_after: &WorldView, _now: &WorldView, intimate: bool) -> Told {
    if intimate { Told::Implied } else { Told::Plain }
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
