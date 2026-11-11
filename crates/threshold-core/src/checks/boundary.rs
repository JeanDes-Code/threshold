//! Content Boundary: no new NPC is younger than the Adventure Seed's
//! `min_age`. The adults-only rule of an intimate proposal is not here: it
//! lives in `mature` (`mature::check`), which `validate` calls directly.

use super::{CheckKind, Rejection};
use crate::{domain::DecisionProposal, store::WorldView};

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
}
