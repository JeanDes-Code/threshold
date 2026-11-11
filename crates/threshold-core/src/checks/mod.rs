//! Commit checks (data-shape §4, design §6.3) and the conversion of a valid
//! proposal into ledger operations. Deterministic code; no model calls.

mod agency;
mod boundary;
mod contest;
mod ops;
mod references;
mod settle;
mod shortcuts;

use std::fmt;

use crate::{
    domain::{Canon, DecisionProposal, EntityId, EntityKind, Handle, Stance},
    normalize,
    store::{Fact, WorldView},
};

pub(crate) use agency::is_involuntary_key;
pub use contest::{checks_now, contest_note, default_check, oppose};
pub use ops::{Resolved, to_ops};
pub use settle::settle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckKind {
    Schema,
    References,
    Agency,
    Contest,
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
            Self::Contest => "contest",
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
/// PC, never what the PC chooses, says, feels or wants. `reputation`,
/// `standing` and `debt` are how others see the PC and what the PC owes, not
/// PC choices.
pub const PC_INVOLUNTARY_KEYS: &[&str] = &[
    "condition",
    "injury",
    "fatigue",
    "carrying",
    "reputation",
    "standing",
    "debt",
];

/// The highest stance `npc` may take toward the PC without a cause. An NPC with
/// no attitude row (for example one created this Turn) has attitude 0.
pub fn ceiling(view: &WorldView, npc: EntityId) -> Stance {
    let Some(pc) = pc(view) else {
        return attitude_ceiling(0);
    };
    let attitude = view
        .attitudes
        .iter()
        .find(|a| a.from == npc && a.to == pc)
        .map_or(0, |a| a.attitude);
    let against_pc = view
        .goals_active
        .iter()
        .any(|g| g.npc_id == npc && g.against_id == Some(pc));
    if against_pc {
        attitude_ceiling(attitude).min(GOAL_AGAINST_PC_CEILING)
    } else {
        attitude_ceiling(attitude)
    }
}

/// The ceiling for an attitude toward the PC, from `STANCE_CEILINGS`.
fn attitude_ceiling(attitude: i8) -> Stance {
    STANCE_CEILINGS
        .iter()
        .rev()
        .find(|(lowest, _)| attitude >= *lowest)
        .map_or(Stance::Resist, |(_, stance)| *stance)
}

/// The ceiling of a reaction's NPC. A new NPC (`N` handle) has no attitude
/// row and no goal yet, so its ceiling is the one of attitude 0.
fn handle_ceiling(view: &WorldView, npc: Handle) -> Stance {
    match npc {
        Handle::Entity(id) => ceiling(view, id),
        _ => attitude_ceiling(0),
    }
}

/// A proposal that passed `validate`. Only `validate` builds one, so
/// unvalidated input cannot reach `to_ops`.
#[derive(Clone, Copy, Debug)]
pub struct Validated<'a>(&'a DecisionProposal);

impl<'a> Validated<'a> {
    pub fn proposal(&self) -> &'a DecisionProposal {
        self.0
    }
}

/// Runs every Commit check that comes before the Roll. References and shape
/// run first; when they fail, the other checks are skipped, so they never see
/// an unknown handle.
pub fn validate<'a>(
    p: &'a DecisionProposal,
    view: &WorldView,
) -> Result<Validated<'a>, Vec<Rejection>> {
    let mut errs = references::check(p, view);
    if errs.is_empty() {
        agency::check(p, view, &mut errs);
        shortcuts::check(p, view, &mut errs);
        boundary::check(p, view, &mut errs);
    }
    if errs.is_empty() {
        Ok(Validated(p))
    } else {
        Err(errs)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Fact,
    Effect,
    WinStake,
    LoseStake,
    /// The PC's `location` that the core writes from `move_to` (ADR 0007).
    Move,
    /// An outcome fact that the core writes from a closing `settles`
    /// (ADR 0007).
    Close,
}

/// A slot write of the proposal on an entity that exists before this Turn:
/// non-claim facts, schedule effects, both stakes, the PC's `location`
/// from `move_to`, and the outcome of a closing `settles`.
pub(super) struct Write<'a> {
    pub subject: EntityId,
    pub key: &'a str,
    pub value: &'a str,
    pub because: &'a [Handle],
    pub origin: Origin,
}

pub(super) fn writes<'a>(p: &'a DecisionProposal, view: &'a WorldView) -> Vec<Write<'a>> {
    let mut out = Vec::new();
    let mut push = |subject: Handle, key, value, because, origin| {
        if let Handle::Entity(subject) = subject {
            out.push(Write {
                subject,
                key,
                value,
                because,
                origin,
            });
        }
    };
    for f in p.facts.iter().filter(|f| f.said_by.is_none()) {
        push(
            f.slot.subject,
            &f.slot.key,
            &f.slot.value,
            &f.because,
            Origin::Fact,
        );
    }
    if let Some(s) = &p.schedule {
        for e in &s.effects {
            push(e.subject, &e.key, &e.value, &[], Origin::Effect);
        }
    }
    if let Some(c) = &p.check {
        for (stake, origin) in [(&c.win, Origin::WinStake), (&c.lose, Origin::LoseStake)] {
            if let Some(stake) = stake {
                push(stake.subject, &stake.key, &stake.value, &[], origin);
            }
        }
    }
    // `move_to` and a close apply only when the action can happen: an
    // impossible verdict writes neither. On a Check they apply on a success
    // only, like the win stake (`to_ops`).
    let happens = p.verdict != crate::domain::Verdict::Impossible;
    let moves = happens && p.action == crate::domain::ActionKind::Move;
    if let (true, Some(pc), Some(place)) = (moves, pc(view), p.move_to.and_then(|h| view.entity(h)))
    {
        push(
            Handle::Entity(pc),
            "location",
            &place.name,
            &[],
            Origin::Move,
        );
    }
    for close in closed(p, view).into_iter().filter(|_| happens) {
        push(
            Handle::Entity(close.subject),
            close.key,
            close.outcome.word(),
            &[],
            Origin::Close,
        );
    }
    out
}

/// The Open Item that a closing `settles` closes.
pub(super) struct Closed<'a> {
    pub subject: EntityId,
    /// The outcome key: the open key without its prefix (ADR 0005).
    pub key: &'a str,
    pub outcome: crate::domain::CloseOutcome,
    /// The item's text as the PC holds it (`world::pc_item_text`).
    pub text: String,
}

/// The Open Item that `settles` closes (any outcome but `open`), when this
/// Turn offered it (`DecisionProposal::offered`, review M1), the PC holds it
/// and holds a text for it. At most one. With no text the PC
/// holds, nothing closes: the outcome fact is built from that text, never
/// from the live fact, which a GM-only write may have replaced (review B3).
pub(super) fn closed<'a>(p: &'a DecisionProposal, view: &WorldView) -> Vec<Closed<'a>> {
    let held = crate::world::pc_open_items(view);
    p.settles
        .as_ref()
        .and_then(|s| s.closing())
        .filter(|(item, _)| p.offered.iter().any(|o| o == item))
        .and_then(|(item, outcome)| {
            let (subject, key) = held
                .iter()
                .find(|(subject, key)| crate::world::open_item_ref(*subject, key) == item)?;
            let outcome_key = crate::domain::outcome_key(key)?;
            let text = crate::world::pc_item_text(view, *subject, key)?;
            // The item text ends with the open key, so it ends with the
            // outcome key too: borrow it from there.
            let at = item.len() - outcome_key.len();
            Some(Closed {
                subject: *subject,
                key: &item[at..],
                outcome,
                text,
            })
        })
        .into_iter()
        .collect()
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
