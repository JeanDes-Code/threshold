//! Agency check: the proposal sets no voluntary action, speech, feeling or
//! motive for the PC.

use super::{CheckKind, Origin, PC_INVOLUNTARY_KEYS, Rejection, pc, same_key, writes};
use crate::{
    domain::{DecisionProposal, Handle},
    store::WorldView,
};

/// True when a write on the PC may set `key` without a choice of the
/// Player: a key of `PC_INVOLUNTARY_KEYS`, never a voluntary act, word,
/// feeling or motive. The World Build applies the same rule to its facts
/// and event effects.
pub(crate) fn is_involuntary_key(key: &str) -> bool {
    PC_INVOLUNTARY_KEYS.iter().any(|k| same_key(k, key))
}

pub(super) fn check(p: &DecisionProposal, view: &WorldView, errs: &mut Vec<Rejection>) {
    let Some(pc) = pc(view) else {
        return;
    };
    let pc = Handle::Entity(pc);
    let mut err = |reason: &str| {
        errs.push(Rejection::new(CheckKind::Agency, Some(pc), reason));
    };
    if p.reactions.iter().any(|r| r.npc == pc) {
        err("a reaction belongs to an NPC, never to the PC");
    }
    if p.schedule.as_ref().is_some_and(|s| s.actor == pc) {
        err("a scheduled event cannot make the PC act");
    }
    if p.schedule.as_ref().is_some_and(|s| s.seen_by.contains(&pc)) {
        err(
            "a scheduled event's seen_by lists NPCs only; the PC sees an event only by being there",
        );
    }
    for f in &p.facts {
        if f.said_by == Some(pc) {
            err("a claim cannot put words in the PC's mouth");
        }
    }
    // Every slot write on the PC: facts, both stakes and schedule effects.
    // The core writes the PC's location from `move_to` and the outcomes of
    // the closing statuses in `open_items` (ADR 0007): the Player's Intent
    // chose them, so they are no PC act of the model's. A fact on the
    // outcome key of an Open Item the PC holds is no longer allowed:
    // `open_items` replaces it.
    for w in writes(p, view).iter().filter(|w| {
        Handle::Entity(w.subject) == pc && !matches!(w.origin, Origin::Move | Origin::Close)
    }) {
        if !is_involuntary_key(w.key) {
            err(&format!(
                "a write on the PC may only set {}; got {}",
                PC_INVOLUNTARY_KEYS.join(", "),
                w.key
            ));
        }
    }
}
