//! Content Boundary: an intimate proposal involves adults only. Participants
//! are the present entities, the reacting NPCs and the new NPCs.

use std::collections::BTreeSet;

use super::{CheckKind, Rejection};
use crate::{
    domain::{DecisionProposal, EntityKind, Handle},
    store::WorldView,
};

pub(super) fn check(p: &DecisionProposal, view: &WorldView, errs: &mut Vec<Rejection>) {
    if !p.intimate {
        return;
    }
    let mut participants: BTreeSet<u64> = view
        .present()
        .into_iter()
        .filter(|id| {
            view.entities
                .iter()
                .any(|e| e.id == *id && matches!(e.kind, EntityKind::Pc | EntityKind::Npc))
        })
        .collect();
    for r in &p.reactions {
        if let Handle::Entity(id) = r.npc {
            participants.insert(id);
        }
    }
    for id in participants {
        let adult = view
            .entities
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| e.adult);
        if adult != Some(true) {
            errs.push(Rejection::new(
                CheckKind::ContentBoundary,
                Some(Handle::Entity(id)),
                "an intimate scene involves adults only; this participant is not a stored adult",
            ));
        }
    }
    for npc in p.new_npcs.iter().filter(|n| !n.adult) {
        errs.push(Rejection::new(
            CheckKind::ContentBoundary,
            Some(npc.id),
            "a new NPC who is not an adult cannot appear in an intimate scene",
        ));
    }
}
