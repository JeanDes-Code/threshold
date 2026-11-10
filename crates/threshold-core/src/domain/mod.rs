mod adventure_seed;
mod handle;
mod proposal;
mod types;
mod world_setup;

pub use adventure_seed::{
    AdventureSeed, DEFAULT_START_HOUR, OpenDetail, OpenMode, OtherName, PcSeed, SeedEntity,
    SeedFact, StartSeed,
};
pub use handle::{Handle, HandleError};
pub use proposal::{
    ActionKind, Cancel, Cause, CheckDraft, Close, CloseOutcome, DecisionProposal, EventDraft,
    Factor, MAX_CLOSES, NewFact, NewNpc, Reaction, Seen, SlotDraft, Verdict,
};
pub use types::{
    ALIAS_KEY_PREFIX, Canon, ClaimDraft, ClockMin, Effect, EntityId, EntityKind, EntityRef,
    FactDraft, FactSource, GoalStatus, Key, Knowing, LedgerOp, OPEN_KEY_PREFIX, STATE_KEYS, Slot,
    Stance, Trigger, Value, alias_key, is_alias_key, is_open_key, is_state_key, outcome_key,
};
pub use world_setup::{
    AttitudeEntry, EffectEntry, EventEntry, FactEntry, GoalEntry, ObstacleEntry, VoiceCardEntry,
    WorldSetup,
};

/// The age from which a character is an adult.
pub const ADULT_AGE: u16 = 18;

/// The only derivation of adult status. A character without a stored age is
/// not an adult.
pub fn is_adult(age: Option<u16>) -> bool {
    age.is_some_and(|age| age >= ADULT_AGE)
}
