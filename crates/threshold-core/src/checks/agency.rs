//! Agency check: the proposal sets no voluntary action, speech, feeling or
//! motive for the PC.

use super::{CheckKind, Origin, PC_INVOLUNTARY_KEYS, Rejection, pc, same_key, writes};
use crate::{
    domain::{ActionKind, DecisionProposal, Handle},
    store::WorldView,
    world::pc_open_outcomes,
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
    // The outcome keys of the Open Items the PC holds: the Intent may settle
    // one with a fact (changed after QA, 2026-11-10).
    let settles = pc_open_outcomes(view);
    // Every slot write on the PC: facts, both stakes and schedule effects.
    for w in writes(p).iter().filter(|w| Handle::Entity(w.subject) == pc) {
        // The Player's Intent chose the move, so a move Turn may set where the
        // PC is: in a fact, or in the win stake of its Check (changed after
        // QA, 2026-11-10). The lose stake never moves the PC.
        let chosen_move = matches!(w.origin, Origin::Fact | Origin::WinStake)
            && p.action == ActionKind::Move
            && same_key(w.key, "location");
        let settled = w.origin == Origin::Fact && settles.iter().any(|k| same_key(k, w.key));
        if !is_involuntary_key(w.key) && !chosen_move && !settled {
            err(&format!(
                "a write on the PC may only set {} (or location on a move Turn, or the outcome of an open item the PC holds); got {}",
                PC_INVOLUNTARY_KEYS.join(", "),
                w.key
            ));
        }
    }
}
