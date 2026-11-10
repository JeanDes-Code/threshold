//! Move check: a move that happens writes where the PC is (rule changed
//! after QA, 2026-11-10).

use super::{CheckKind, Origin, Rejection, pc, same_key, writes};
use crate::{
    domain::{ActionKind, DecisionProposal, Handle, Verdict},
    store::WorldView,
};

/// The repair note of a move that writes no PC `location`: the exact fact
/// to add, with the PC's handle (made concrete after the first live run).
pub fn move_note(pc: Handle) -> String {
    format!(
        "a move needs this fact in facts: {{\"slot\":{{\"subject\":\"{pc}\",\"key\":\"location\",\"value\":\"<the destination place>\"}},\"seen\":\"pc\",\"said_by\":null,\"because\":[]}} (on a check, put that slot in the win stake instead)"
    )
}

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
        let pc = Handle::Entity(pc);
        errs.push(Rejection::new(CheckKind::Move, Some(pc), move_note(pc)));
    }
}
