//! The Seed Draft, its rules and its store, the stage runner, the draft
//! stages, the World Build stages and their checks, and the types every
//! build stage shares (data shape 2, 3.5 and 3.6).
//! `build` calls the model through `Gm::propose` only, and knows no SQL.

mod checks;
mod draft;
mod draft_stages;
mod stages;
mod store;
mod world_stages;

use serde::{Deserialize, Serialize};

pub use draft::{DraftState, Gap, GapKind, KnownFact, SeedDraft, apply_edit, approve, gaps};
pub use draft_stages::{SOURCE_TEXT_MAX_CHARS, draft_from_text};
pub use store::{DraftLine, DraftStore, DraftStoreError};
pub use world_stages::{NpcState, PC_CENTRED_MAX_SHARE, Placement, build_world};

/// A step of the draft or of the World Build. The first three draft the
/// seed; the others build the hidden world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    DraftSeed,
    DraftRoster,
    DraftKnowledge,
    HiddenFacts,
    NpcState,
    WorldEvents,
    StartState,
    Commit,
}

/// The check that failed a stage. A name only: it never carries model text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildCheck {
    Model,
    Schema,
    References,
    OnePc,
    Kinds,
    Ages,
    Completeness,
    Agency,
    SettledFacts,
    OpenItems,
    WorldAutonomy,
    Secrecy,
    Cancelled,
    NotApproved,
}

/// Why a draft or a build stopped: a stage name and a check name, safe to
/// show the Player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildFailure {
    pub stage: Stage,
    pub check: BuildCheck,
}

/// How far a stage is: a stage name and two counts, nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    pub stage: Stage,
    pub done: u32,
    pub total: u32,
}

/// What a draft or a build reports to, and asks, its caller.
pub struct Control<'a> {
    pub progress: &'a mut dyn FnMut(Progress),
    pub cancelled: &'a dyn Fn() -> bool,
    pub log: &'a mut dyn FnMut(DebugEntry),
}

/// One model call of a draft or a build, for the debug view only. `op_id`
/// is the draft id; `step` names the stage and the item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugEntry {
    pub op_id: String,
    pub step: String,
    pub prompt: String,
    pub raw: String,
    pub ms: u64,
}
