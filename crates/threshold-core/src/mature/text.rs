//! What a Narration may tell of an intimate Turn (G4, data shape 3.5).
//! Task 2 of the plan has two modes: a plain Turn is told as any Turn, an
//! intimate Turn by implication only, with the core's own line
//! (`context::IMPLIED_LINE`). No pack and no model output choose the mode.

use crate::store::WorldView;

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
