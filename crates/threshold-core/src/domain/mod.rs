mod adventure_seed;
mod handle;
mod proposal;
mod types;

pub use adventure_seed::{AdventureSeed, OtherName, PcSeed, SeedEntity, SeedFact};
pub use handle::{Handle, HandleError};
pub use proposal::{
    ActionKind, Cancel, Cause, CheckDraft, DecisionProposal, EventDraft, Factor, NewFact, NewNpc,
    Reaction, Seen, SlotDraft, Verdict,
};
pub use types::{
    Canon, ClaimDraft, ClockMin, Effect, EntityId, EntityKind, EntityRef, FactDraft, FactSource,
    GoalStatus, Key, Knowing, LedgerOp, STATE_KEYS, Slot, Stance, Trigger, Value, is_state_key,
};

/// The age from which a character is an adult.
pub const ADULT_AGE: u16 = 18;

/// The only derivation of adult status. A character without a stored age is
/// not an adult.
pub fn is_adult(age: Option<u16>) -> bool {
    age.is_some_and(|age| age >= ADULT_AGE)
}
