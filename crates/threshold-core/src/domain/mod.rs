mod adventure_seed;
mod handle;
mod proposal;
mod types;
mod world_setup;

pub use adventure_seed::{
    AdventureSeed, DEFAULT_START_HOUR, DetailLevel, OpenDetail, OpenMode, OtherName, PcSeed,
    SeedEntity, SeedFact, StartSeed,
};
pub use handle::{Handle, HandleError};
pub use proposal::{
    AWAY, ActionKind, Cancel, Cause, CheckDraft, CloseOutcome, DecisionProposal, EventDraft,
    FIND_LINE_MAX_CHARS, FIND_MAX_ITEMS, FIND_SOURCE_MAX_CHARS, FIND_TITLE_MAX_CHARS,
    FINDS_DEFINITION, Factor, Finds, FoundItem, INTIMACY_DEFINITION, Intimacy, ItemStatus, Leave,
    NO_ITEM, NewFact, NewNpc, PLAN_MAX_CHARS, Reaction, SCENE_PLAN_MAX_CHARS, Seen, Settle,
    SlotDraft, SourceKind, Verdict, WILLING_DEFINITION, Willing,
};
pub(crate) use proposal::{
    finds_definition_text, intimacy_definition_text, willing_definition_text,
};
pub use types::{
    ALIAS_KEY_PREFIX, Canon, ClaimDraft, ClockMin, Effect, EntityId, EntityKind, EntityRef,
    FactDraft, FactSource, GoalStatus, Key, Knowing, LedgerOp, OPEN_KEY_PREFIX, STATE_KEYS,
    SceneWhy, Slot, Stance, Trigger, Value, alias_key, is_alias_key, is_open_key, is_state_key,
    location_sentence, outcome_key, slot_sentence, you_sentence,
};
pub use world_setup::{
    AttitudeEntry, EffectEntry, EventEntry, FactEntry, GoalEntry, ObstacleEntry, VoiceCardEntry,
    WorldSetup,
};

/// The ages the Player confirmed in a Seed Draft with no gap (Age
/// Confirmation): one `(kind, exact main name, age)` per character
/// (data shape 3.2). Only the core makes one, through
/// `build::confirmed_ages`: the shell and the UI cannot. `Store::
/// create_campaign_with` marks a seed character confirmed only when this set
/// holds its kind, its exact main name and its age.
#[derive(Debug, PartialEq, Eq)]
pub struct ConfirmedAges(std::collections::BTreeSet<(EntityKind, String, u16)>);

impl ConfirmedAges {
    pub(crate) fn new(entries: std::collections::BTreeSet<(EntityKind, String, u16)>) -> Self {
        Self(entries)
    }

    /// True when the Player confirmed `age` for the character of kind
    /// `kind` whose main name is exactly `name`.
    pub fn holds(&self, kind: EntityKind, name: &str, age: u16) -> bool {
        self.0.contains(&(kind, name.to_owned(), age))
    }
}

/// The age from which a character is an adult.
pub const ADULT_AGE: u16 = 18;

/// The only derivation of adult status from an age. A character without a
/// stored age is not an adult. Only `mature` calls it: an age alone never
/// clears a character for an Intimate Scene (`mature::clearance`).
pub fn is_adult(age: Option<u16>) -> bool {
    age.is_some_and(|age| age >= ADULT_AGE)
}
