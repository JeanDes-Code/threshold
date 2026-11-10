//! Content Boundary: no new NPC is younger than the Adventure Seed's
//! `min_age`, and an intimate proposal involves adults only. Participants
//! are the present entities, the reacting NPCs and the new NPCs. Adult status
//! comes from the stored age through `domain::is_adult`, and only here.

use std::collections::BTreeSet;

use super::{CheckKind, Rejection};
use crate::{
    domain::{DecisionProposal, EntityKind, Handle, is_adult},
    store::WorldView,
};

pub(super) fn check(p: &DecisionProposal, view: &WorldView, errs: &mut Vec<Rejection>) {
    if let Some(min_age) = view.adventure_seed.as_ref().and_then(|s| s.min_age) {
        for npc in p.new_npcs.iter().filter(|n| n.age < min_age) {
            errs.push(Rejection::new(
                CheckKind::ContentBoundary,
                Some(npc.id),
                format!("every character is at least {min_age} years old"),
            ));
        }
    }
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
        let age = view
            .entities
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| e.age);
        if !is_adult(age) {
            errs.push(Rejection::new(
                CheckKind::ContentBoundary,
                Some(Handle::Entity(id)),
                "an intimate scene involves adults only; this participant has no stored adult age",
            ));
        }
    }
    for npc in p.new_npcs.iter().filter(|n| !is_adult(Some(n.age))) {
        errs.push(Rejection::new(
            CheckKind::ContentBoundary,
            Some(npc.id),
            "a new NPC who is not an adult cannot appear in an intimate scene",
        ));
    }
}
