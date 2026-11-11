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
//! telling of an intimate Turn (`told`, in `text`). The scene and the
//! buttons come in later tasks.

mod gate;
mod text;

use serde::Serialize;

pub use gate::{Blocked, Cleared, Presence, after, check, check_after, checked_here, clearance};
pub use text::{Told, stored_intimate, told};

use crate::{
    checks::{CheckKind, Rejection},
    domain::{EntityId, Handle},
};

/// Why the core refused a Turn (a Refusal, glossary). The Player reads one
/// fixed sentence per kind (`view::refusal_text`), never a name, an id or
/// an age.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalKind {
    /// The Campaign's Detail Level is `excluded`.
    Excluded,
    /// A character in the place, or a reacting or new NPC, is not cleared.
    AdultsOnly,
    /// No NPC goes along with the intimate proposal, after the one retry.
    Unwilling,
}

/// The Refusal that `errs` holds, if one of them is final
/// (`CheckKind::is_final`): its kind, and the ids of the characters who
/// block it, sorted, each once. `Excluded` wins over `AdultsOnly`.
pub(crate) fn final_refusal(errs: &[Rejection]) -> Option<(RefusalKind, Vec<EntityId>)> {
    if errs.iter().any(|r| r.check == CheckKind::DetailExcluded) {
        return Some((RefusalKind::Excluded, Vec::new()));
    }
    if !errs.iter().any(|r| r.check == CheckKind::AdultsOnly) {
        return None;
    }
    let mut blocked: Vec<EntityId> = errs
        .iter()
        .filter(|r| r.check == CheckKind::AdultsOnly)
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
