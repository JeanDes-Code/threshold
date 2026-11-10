//! Contests (ADR 0007). The proposal says in `contested` whether an NPC
//! actively opposes the act and the result is uncertain. A contested act
//! needs a Check: the first attempt without one gets one repair note; on
//! the retry the core builds a default Check instead of rejecting.

use super::{CheckKind, Rejection, pc};
use crate::{
    domain::{CheckDraft, DecisionProposal, Handle, Verdict},
    store::WorldView,
};

/// True when the proposal needs a Check it does not have: it is contested,
/// or its verdict is `check`, and it carries no Check. An impossible act
/// needs none.
fn lacks_check(p: &DecisionProposal) -> bool {
    p.check.is_none()
        && match p.verdict {
            Verdict::Check => true,
            Verdict::Allowed => p.contested,
            Verdict::Impossible => false,
        }
}

/// The repair note of a contested act with no Check, for the first attempt.
pub fn contest_note(p: &DecisionProposal) -> Option<Rejection> {
    (p.contested && lacks_check(p)).then(|| {
        Rejection::new(
            CheckKind::Contest,
            None,
            "contested is true, so this act needs a Check: set verdict check and add check with against = the opposing NPC, win and lose stakes and factors",
        )
    })
}

/// On the retry: when the proposal still lacks the Check it needs, the
/// core builds one and returns the note for the debug log. The default
/// Check opposes the NPC whose reaction has the lowest stance (none when
/// no NPC reacts). It has no stakes and no factors, so the Ruleset gives
/// its normal difficulty, and the narrator gets only the result. The
/// proposal's facts then apply on a won Roll only.
pub fn default_check(p: &mut DecisionProposal, view: &WorldView) -> Option<String> {
    if !lacks_check(p) {
        return None;
    }
    let pc = pc(view).map(Handle::Entity);
    let against = p
        .reactions
        .iter()
        .filter(|r| Some(r.npc) != pc)
        .min_by_key(|r| r.stance)
        .map(|r| r.npc);
    p.verdict = Verdict::Check;
    // The facts were written for an act that simply happens: with the Roll
    // they apply on a win only (`to_ops`).
    p.facts_on_win = true;
    p.check = Some(CheckDraft {
        against,
        win: None,
        lose: None,
        factors: Vec::new(),
    });
    Some(match against {
        Some(npc) => {
            format!(
                "built a default Check against {npc}: the act is contested and has no check; its facts apply on a win only"
            )
        }
        None => {
            "built a default Check against nobody: the act is contested and has no check; its facts apply on a win only".into()
        }
    })
}
