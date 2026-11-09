//! Agency check: the proposal sets no voluntary action, speech, feeling or
//! motive for the PC.

use super::{CheckKind, PC_INVOLUNTARY_KEYS, Rejection, pc, same_key};
use crate::{
    domain::{DecisionProposal, Handle},
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
    for f in &p.facts {
        if f.said_by == Some(pc) {
            err("a claim cannot put words in the PC's mouth");
        } else if f.said_by.is_none()
            && f.slot.subject == pc
            && !PC_INVOLUNTARY_KEYS.iter().any(|k| same_key(k, &f.slot.key))
        {
            err(&format!(
                "a fact about the PC may only set {}; got {}",
                PC_INVOLUNTARY_KEYS.join(", "),
                f.slot.key
            ));
        }
    }
}
