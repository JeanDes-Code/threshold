//! The Intimate Scene (data shape 3.4, ADR 0010): the gate before the
//! proposal call of a Turn (G1) and the one `SetScene` op a Turn may add to
//! its Commit. The scene state is the projection `WorldView::scene_open`;
//! only `scene_op` decides how a Turn changes it.

use std::collections::BTreeSet;

use super::gate::{Blocked, Cleared, Presence, clearance};
use crate::{
    context::seen_present,
    domain::{DecisionProposal, EntityId, EntityKind, LedgerOp, SceneWhy},
    normalize,
    store::WorldView,
};

/// The token cap of a Narration told by implication inside an Intimate
/// Scene (data shape 4, a first value): about the size of an ordinary
/// Narration (ADR 0001), now held by the model's token limit. The Player's
/// 15 s Turn budget reads it: at about 43 tokens per second, 280 tokens are
/// about 6.5 s of generation (ADR 0010).
pub const SCENE_NARRATION_MAX_TOKENS: u32 = 280;

/// G1: the clearance of an open scene, before the proposal call of a Turn.
/// `Ok(None)` when no scene is open; `Ok(Some(_))` when every character the
/// gate checks at the PC's place is cleared (`clearance`); `Err` when one is
/// not. The caller then plans the Turn as a plain one, and the Turn's own
/// Commit closes the scene (`scene_op` with `gate_failed`).
pub fn scene_gate(view: &WorldView) -> Result<Option<Cleared>, Blocked> {
    if view.scene_open.is_none() {
        return Ok(None);
    }
    clearance(view, &[]).map(Some)
}

/// The clearance a narration call of an open scene may hold (G4): the
/// state right after the Turn holds an open scene, it is still the open
/// scene `now`, and G1 passes on the state after the Turn. `None` in any
/// other case: a `renarrate` of a scene that has ended gets none.
pub fn narration_clearance(view_after: &WorldView, now: &WorldView) -> Option<Cleared> {
    if view_after.scene_open.is_none() || now.scene_open != view_after.scene_open {
        return None;
    }
    scene_gate(view_after).ok().flatten()
}

/// The `SetScene` op one Turn adds to its Commit, if any (data shape 3.4):
///
/// | scene before | the Turn | op |
/// |---|---|---|
/// | open | G1 failed when the Turn started (`gate_failed`) | close, `Gate` |
/// | closed | `intimate` (G2 and G3 passed) | open, `Opened` |
/// | closed | not `intimate` | none |
/// | open | not `intimate` | close, `NotIntimate` |
/// | open | `intimate`, the PC's place changes | close, `Moved` |
/// | open | `intimate`, a character the PC did not see here now sees here | close, `Arrival` |
/// | open | `intimate`, none of the above | none |
///
/// `view` is the state before the Turn and `after` the presence once its
/// ops are committed (`mature::after`). A Turn that closes a scene never
/// opens one: the next intimate Turn opens a new scene. The plan's row 9
/// departs from the data shape: a G1 failure closes the scene in the
/// Turn's own Commit, not in an OOC Turn before it, so "Correct last Turn"
/// still finds the Turn it corrects (plan deviation 1).
pub fn scene_op(
    view: &WorldView,
    p: &DecisionProposal,
    after: &Presence,
    gate_failed: bool,
) -> Option<LedgerOp> {
    let close = |why| Some(LedgerOp::SetScene { open: false, why });
    if view.scene_open.is_none() {
        return p.intimate.then_some(LedgerOp::SetScene {
            open: true,
            why: SceneWhy::Opened,
        });
    }
    if gate_failed {
        return close(SceneWhy::Gate);
    }
    if !p.intimate {
        return close(SceneWhy::NotIntimate);
    }
    let place_before = view
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Pc)
        .and_then(|pc| view.live_fact(pc.id, "location"))
        .map(|f| normalize::value(&f.slot.value));
    if after.pc_place != place_before {
        return close(SceneWhy::Moved);
    }
    let seen_before: BTreeSet<EntityId> = seen_present(view).into_iter().collect();
    // An id the view does not hold is a character this Turn made: it counts.
    let is_character = |id: &EntityId| {
        view.entities
            .iter()
            .find(|e| e.id == *id)
            .is_none_or(|e| matches!(e.kind, EntityKind::Pc | EntityKind::Npc))
    };
    if after
        .seen
        .iter()
        .any(|id| !seen_before.contains(id) && is_character(id))
    {
        return close(SceneWhy::Arrival);
    }
    None
}
