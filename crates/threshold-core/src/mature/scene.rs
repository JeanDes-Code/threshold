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
    store::{TurnId, WorldView},
};

/// The token cap of a Narration told as intimate, at any level, the Turn
/// that ends its scene and a `renarrate` included (data shape 4, a first
/// value; one rule since review, 2026-11-12): about the size of an ordinary
/// Narration (ADR 0001), now held by the model's token limit. The Player's
/// 15 s Turn budget reads it: at about 43 tokens per second, 280 tokens are
/// about 6.5 s of generation (ADR 0010).
pub const SCENE_NARRATION_MAX_TOKENS: u32 = 280;

/// The minutes of the core's default closing (`default_close`), when the
/// model gives no closing it can commit (data shape 3.7, a first value).
pub const SCENE_CLOSE_DEFAULT_MIN: u32 = 15;

/// The most minutes a Closing Turn may take: `checks::settle_closing` clamps
/// the proposal's `minutes` to 1 to this (data shape 3.7, a first value).
pub const SCENE_CLOSE_MAX_MIN: u32 = 240;

/// The token cap of the short Narration of a Closing Turn (data shape 3.7,
/// a first value): about 6 s at 43 tokens per second.
pub const CLOSING_NARRATION_MAX_TOKENS: u32 = 90;

/// The open scene as every reader outside the ledger sees it: the scene of
/// `view` while G1 passes on it (`scene_gate`), else `None`. The Player
/// screens, the plan limit of `checks::validate` and the buttons read this.
///
/// A scene stays open in the ledger after G1 fails when the Turn that found
/// it is then refused (G2 or G3): nothing commits. The next Turn that
/// commits closes it (`scene_op`, `why: Gate`). In between, no reader treats
/// it as open: no prompt holds its text (G1 and `narration_clearance` find
/// no clearance), and no screen shows it.
pub fn open_scene(view: &WorldView) -> Option<TurnId> {
    view.scene_open.filter(|_| scene_gate(view).is_ok())
}

/// The ops of the core's own closing, when the model fails or both its
/// attempts are rejected (data shape 3.7, "the button always works"): the
/// World Clock moves `SCENE_CLOSE_DEFAULT_MIN` minutes and the scene
/// closes for `why` (`EndScene` or `FadeToBlack`). No reaction, no fact,
/// no PC act. A World Event that falls due in those minutes fires in the
/// next Turn. The Player's choice to stop never waits on a model answer.
pub fn default_close(why: SceneWhy) -> Vec<LedgerOp> {
    vec![
        LedgerOp::AdvanceClock {
            minutes: SCENE_CLOSE_DEFAULT_MIN,
        },
        LedgerOp::SetScene { open: false, why },
    ]
}

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
/// state right after the Turn holds an open scene, that scene is still the
/// open scene `now` as every reader sees it (`open_scene`, so G1 passes on
/// the state now), and G1 passes on the state after the Turn too. `None` in
/// any other case: a `renarrate` of a scene that has ended gets none, and
/// so does a `renarrate` (or the same op sent again) after a character who
/// is not cleared came to the place (review B1, 2026-11-12). The Turn is
/// then told by implication, with no pack fragment, and the other Turns of
/// the scene are their Neutral Lines.
pub fn narration_clearance(view_after: &WorldView, now: &WorldView) -> Option<Cleared> {
    let scene = view_after.scene_open?;
    if open_scene(now) != Some(scene) {
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
        return p.intimacy.intimate().then_some(LedgerOp::SetScene {
            open: true,
            why: SceneWhy::Opened,
        });
    }
    if gate_failed {
        return close(SceneWhy::Gate);
    }
    if !p.intimacy.intimate() {
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
