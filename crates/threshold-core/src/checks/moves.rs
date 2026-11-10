//! Move check: a move that happens writes where the PC is (rule changed
//! after QA, 2026-11-10).

use super::{CheckKind, Origin, Rejection, pc, same_key, writes};
use crate::{
    domain::{ActionKind, DecisionProposal, Handle, Verdict},
    store::WorldView,
};

/// The repair note of a move that writes no PC `location`.
pub const MOVE_NOTE: &str = "a move writes exactly one fact: subject = the PC, key = location, value = the destination place (on a check, the win stake writes it)";

pub(super) fn check(p: &DecisionProposal, view: &WorldView, errs: &mut Vec<Rejection>) {
    if p.action != ActionKind::Move || p.verdict == Verdict::Impossible {
        return;
    }
    let Some(pc) = pc(view) else {
        return;
    };
    // An allowed move writes it as a fact. A Check may write it in the win
    // stake instead, so the PC moves only on a success.
    let moves = writes(p).iter().any(|w| {
        w.subject == pc
            && same_key(w.key, "location")
            && match p.verdict {
                Verdict::Check => matches!(w.origin, Origin::Fact | Origin::WinStake),
                _ => w.origin == Origin::Fact,
            }
    });
    if !moves {
        errs.push(Rejection::new(
            CheckKind::Move,
            Some(Handle::Entity(pc)),
            MOVE_NOTE,
        ));
    }
}
