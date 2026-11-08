use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type EntityId = u64;
pub type Key = String;
pub type Value = String;
pub type ClockMin = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Pc,
    Npc,
    Faction,
    Place,
    Item,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Canon {
    Possible,
    Established,
    Persistent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FactSource {
    Charter,
    Proposal,
    Stake,
    WorldEvent,
    Correction,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    Oppose,
    Resist,
    Neutral,
    Comply,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Knowing {
    Witnessed,
    Told,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Slot {
    pub subject: EntityId,
    pub key: Key,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    At(ClockMin),
    When(Slot),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClaimDraft {
    pub speaker: EntityId,
    pub slot: Slot,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    SetFact {
        slot: Slot,
        text: String,
        seen_by: Vec<EntityId>,
    },
    ShiftAttitude {
        from: EntityId,
        to: EntityId,
        delta: i8,
    },
    Tell {
        to: EntityId,
        claim: ClaimDraft,
        delay_min: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LedgerOp {
    AddEntity {
        kind: EntityKind,
        name: String,
        adult: Option<bool>,
    },
    AddFact {
        slot: Slot,
        text: String,
        canon: Canon,
        source: FactSource,
        seen_by: Vec<EntityId>,
    },
    EstablishFact {
        fact_id: u64,
    },
    SupersedeFact {
        fact_id: u64,
        superseded_by: u64,
    },
    AddClaim {
        claim: ClaimDraft,
    },
    Know {
        holder: EntityId,
        fact_id: Option<u64>,
        claim_id: Option<u64>,
        how: Knowing,
        arrives_at: ClockMin,
    },
    ShiftAttitude {
        from: EntityId,
        to: EntityId,
        delta: i8,
    },
    OpenObstacle {
        fact_id: u64,
    },
    ClearObstacle {
        obstacle_id: u64,
    },
    ScheduleEvent {
        text: String,
        trigger: Trigger,
        actors: Vec<EntityId>,
        goal_id: Option<u64>,
        cause_fact_id: Option<u64>,
        effects: Vec<Effect>,
        seen_by: Vec<EntityId>,
    },
    FireEvent {
        event_id: u64,
    },
    CancelEvent {
        event_id: u64,
        cause_fact_id: u64,
    },
    AdvanceClock {
        minutes: u32,
    },
    RecordRoll {
        seed: u64,
        difficulty: i8,
        value: i32,
        outcome: String,
    },
    RevertTurn {
        turn_id: u64,
    },
}
