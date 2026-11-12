//! The mature content module (data shape `2026-11-11-mature-content-module`,
//! section 2): the one home of the adults-only rule.
//!
//! `gate` holds the rule itself and takes nothing but the world: no prompt,
//! no model output and no Content Pack reaches it, so nothing outside the
//! stored and confirmed ages can turn it off or weaken it (ADR 0008). It is
//! the only caller of `domain::is_adult`.
//!
//! Task 1 of the plan lands the gate before the proposal is committed (G2,
//! `check`) and the final Refusal. Task 2 adds the same rule after the
//! World Events of the Turn (G3, `after` and `check_after`) and the
//! telling of an intimate Turn (`told`, in `text`). Task 4 adds the
//! Intimate Scene (`scene`): the gate before the proposal call (G1), the
//! one op a Turn adds to open or close a scene, and what each reader gets
//! of a Turn committed with `intimate` (`turn_text`). Task 5 adds the
//! Closing Turn of "End scene": its limits and the core's own closing
//! (`default_close`), and `open_scene`, the scene as every reader outside
//! the ledger sees it. Task 6 adds the Neutral Line of a faded Turn
//! (`FADED_TEMPLATE`): a faded Turn is its line for every reader. Task 7
//! adds the effective Detail Level (`effective`) and explicit telling
//! (`Told::Explicit`): explicit text reaches a prompt only with a `Cleared`
//! value (ADR 0009). `text` names `pack` for that one rule; `gate` names
//! none. Task 8 adds the Age Confirmation in play (`confirmable`) and the
//! characters a Refusal may offer to confirm (`to_confirm`), which read
//! what the PC perceives and never who blocked.

mod gate;
mod scene;
mod text;
mod words;

use serde::{Deserialize, Serialize};

pub use gate::{
    Blocked, Cleared, Presence, after, check, check_after, checked_here, clearance, confirmable,
    to_confirm,
};
pub use scene::{
    CLOSING_NARRATION_MAX_TOKENS, SCENE_CLOSE_DEFAULT_MIN, SCENE_CLOSE_MAX_MIN,
    SCENE_NARRATION_MAX_TOKENS, default_close, narration_clearance, open_scene, scene_gate,
    scene_op,
};
pub use text::{
    CLOSING_TEMPLATE, FADED_TEMPLATE, INTIMATE_TEMPLATE, Told, TurnText, effective, neutral_line,
    reading_level, stored_intimacy, stored_intimate, told, told_level, turn_text,
};
pub use words::{Floor, ROMANCE_WORDS, folded, intimacy_floor, romance_floor};

use crate::{
    checks::{CheckKind, Rejection},
    domain::{EntityId, Handle},
};

/// Why the core refused a Turn (a Refusal, glossary). The Player reads one
/// fixed sentence per kind (`view::refusal_text`), never a name, an id or
/// an age.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalKind {
    /// The Campaign's Detail Level is `excluded`.
    Excluded,
    /// A character in the place, or a reacting or new NPC, is not cleared.
    AdultsOnly,
    /// No NPC goes along with the intimate proposal, after the one retry.
    Unwilling,
}

/// The Refusal that `errs` holds, if one of them is final: its kind, and
/// the ids of the characters who block it, sorted, each once.
/// `CheckKind::is_final` is the one list of final kinds; this function
/// reads it and keeps no list of its own. `Excluded` wins; any other final
/// kind is an `AdultsOnly` Refusal, so a new final kind fails closed.
pub(crate) fn final_refusal(errs: &[Rejection]) -> Option<(RefusalKind, Vec<EntityId>)> {
    let finals: Vec<&Rejection> = errs.iter().filter(|r| r.check.is_final()).collect();
    if finals.is_empty() {
        return None;
    }
    if finals.iter().any(|r| r.check == CheckKind::DetailExcluded) {
        return Some((RefusalKind::Excluded, Vec::new()));
    }
    let mut blocked: Vec<EntityId> = finals
        .iter()
        .filter_map(|r| match r.handle {
            Some(Handle::Entity(id)) => Some(id),
            _ => None,
        })
        .collect();
    blocked.sort_unstable();
    blocked.dedup();
    Some((RefusalKind::AdultsOnly, blocked))
}

/// True when every error of a second attempt is the stance rule: the Turn
/// ends as an `Unwilling` Refusal, not as a rejected Turn.
pub(crate) fn only_unwilling(errs: &[Rejection]) -> bool {
    !errs.is_empty() && errs.iter().all(|r| r.check == CheckKind::Unwilling)
}
