//! Commit checks (data-shape §4, design §6.3) and the conversion of a valid
//! proposal into ledger operations. Deterministic code; no model calls.

mod agency;
mod boundary;
mod ops;
mod references;
mod shortcuts;

use std::fmt;

use crate::{
    domain::{Canon, DecisionProposal, EntityId, EntityKind, Handle, Stance},
    normalize,
    store::{Fact, WorldView},
};

pub use ops::{Resolved, to_ops};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckKind {
    Schema,
    References,
    Agency,
    Coincidence,
    NpcFolds,
    FactBends,
    TensionEvaporates,
    ContentBoundary,
}

impl fmt::Display for CheckKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Schema => "schema",
            Self::References => "references",
            Self::Agency => "agency",
            Self::Coincidence => "coincidence",
            Self::NpcFolds => "npc folds",
            Self::FactBends => "fact bends",
            Self::TensionEvaporates => "tension evaporates",
            Self::ContentBoundary => "content boundary",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub check: CheckKind,
    pub handle: Option<Handle>,
    pub reason: String,
}

impl Rejection {
    fn new(check: CheckKind, handle: Option<Handle>, reason: impl Into<String>) -> Self {
        Self {
            check,
            handle,
            reason: reason.into(),
        }
    }
}

impl fmt::Display for Rejection {
    /// One line, so a retry prompt lists one error per line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.check)?;
        if let Some(handle) = self.handle {
            write!(f, " {handle}")?;
        }
        let reason = self.reason.split_whitespace().collect::<Vec<_>>().join(" ");
        write!(f, ": {reason}")
    }
}

/// Stance ceiling of an NPC toward the PC: `(lowest attitude, ceiling)`,
/// in ascending order. Attitude <= -3 gives Resist, -2..=-1 Neutral,
/// 0..=2 Comply, >= 3 Help (data-shape §4).
pub const STANCE_CEILINGS: [(i8, Stance); 4] = [
    (i8::MIN, Stance::Resist),
    (-2, Stance::Neutral),
    (0, Stance::Comply),
    (3, Stance::Help),
];

/// An active goal against the PC caps the ceiling at this stance.
pub const GOAL_AGAINST_PC_CEILING: Stance = Stance::Resist;

/// The only keys a proposal fact may set on the PC: things that happen to the
/// PC, never what the PC chooses, says, feels or wants.
pub const PC_INVOLUNTARY_KEYS: &[&str] = &["condition", "injury", "fatigue", "carrying"];

/// The highest stance `npc` may take toward the PC without a cause. An NPC with
/// no attitude row (for example one created this Turn) has attitude 0.
pub fn ceiling(view: &WorldView, npc: EntityId) -> Stance {
    let Some(pc) = pc(view) else {
        return Stance::Comply;
    };
    let attitude = view
        .attitudes
        .iter()
        .find(|a| a.from == npc && a.to == pc)
        .map_or(0, |a| a.attitude);
    let by_attitude = STANCE_CEILINGS
        .iter()
        .rev()
        .find(|(lowest, _)| attitude >= *lowest)
        .map_or(Stance::Resist, |(_, stance)| *stance);
    let against_pc = view
        .goals_active
        .iter()
        .any(|g| g.npc_id == npc && g.against_id == Some(pc));
    if against_pc {
        by_attitude.min(GOAL_AGAINST_PC_CEILING)
    } else {
        by_attitude
    }
}

/// Runs every Commit check that comes before the Roll. References and shape
/// run first; when they fail, the other checks are skipped, so they never see
/// an unknown handle.
pub fn validate(p: &DecisionProposal, view: &WorldView) -> Result<(), Vec<Rejection>> {
    let mut errs = references::check(p, view);
    if errs.is_empty() {
        agency::check(p, view, &mut errs);
        shortcuts::check(p, view, &mut errs);
        boundary::check(p, view, &mut errs);
    }
    if errs.is_empty() { Ok(()) } else { Err(errs) }
}

fn pc(view: &WorldView) -> Option<EntityId> {
    view.entities
        .iter()
        .find(|e| e.kind == EntityKind::Pc)
        .map(|e| e.id)
}

/// A valid cause: a fact that exists before this Turn and is Established or
/// Persistent. Every fact in the view predates this Turn, because the facts of
/// a proposal are written by its own Commit and have no handle yet.
fn valid_cause(view: &WorldView, handle: Handle) -> Option<&Fact> {
    view.fact(handle).filter(|f| f.canon != Canon::Possible)
}

fn same(a: &str, b: &str) -> bool {
    normalize::value(a) == normalize::value(b)
}

fn same_key(a: &str, b: &str) -> bool {
    normalize::key(a) == normalize::key(b)
}

/// True when writing `value` changes the slot's live value (or the slot has
/// none). Restating the live value is not a write.
fn changes(view: &WorldView, subject: EntityId, key: &str, value: &str) -> bool {
    view.live_fact(subject, key)
        .is_none_or(|f| !same(&f.slot.value, value))
}
