//! Agency check: the proposal sets no voluntary action, speech, feeling or
//! motive for the PC.

use super::{CheckKind, Origin, PC_INVOLUNTARY_KEYS, Rejection, pc, same_key, writes};
use crate::{
    domain::{ActionKind, DecisionProposal, Handle},
    store::WorldView,
};

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
    for w in writes(p).iter().filter(|w| Handle::Entity(w.subject) == pc) {
        let involuntary = PC_INVOLUNTARY_KEYS.iter().any(|k| same_key(k, w.key));
        // The Player's Intent chose the move, so a move Turn may set where the PC is.
        let chosen_move =
            w.origin == Origin::Fact && p.action == ActionKind::Move && same_key(w.key, "location");
        if !involuntary && !chosen_move {
            err(&format!(
                "a write on the PC may only set {} (or location on a move Turn); got {}",
                PC_INVOLUNTARY_KEYS.join(", "),
                w.key
            ));
        }
    }
}
