use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Handle, Stance};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecisionProposal {
    pub action: ActionKind,
    pub verdict: Verdict,
    /// The known place the PC goes to, when the Intent moves the PC there;
    /// `None` when the PC stays. The core writes the PC's `location` from it
    /// (ADR 0007); the model never writes that fact itself. The per-Turn
    /// schema makes it required, with the known places' handles and `null`.
    #[serde(default)]
    pub move_to: Option<Handle>,
    /// The Open Items the Player's Intent settles. The core writes each
    /// outcome fact (ADR 0005, ADR 0007). The per-Turn schema makes it
    /// required and limits `item` to the open items the PC holds.
    #[serde(default)]
    #[schemars(length(max = 2))]
    pub closes: Vec<Close>,
    #[serde(default)]
    #[schemars(length(max = 2))]
    pub blockers: Vec<Handle>,
    pub check: Option<CheckDraft>,
    #[serde(default)]
    #[schemars(length(max = 2))]
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    #[schemars(length(max = 1))]
    pub new_npcs: Vec<NewNpc>,
    #[serde(default)]
    #[schemars(length(max = 2))]
    pub facts: Vec<NewFact>,
    #[serde(default)]
    #[schemars(length(max = 1))]
    pub cancel: Vec<Cancel>,
    pub schedule: Option<EventDraft>,
    #[serde(default)]
    pub minutes: u32,
    #[serde(default)]
    pub intimate: bool,
}

/// One Open Item that the Intent settles: `item` names it as the prompt
/// shows it (`world::open_item_ref`), and `outcome` is the value of the
/// outcome fact the core writes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Close {
    pub item: String,
    #[schemars(length(max = 40))]
    pub outcome: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Talk,
    Move,
    Act,
    Observe,
    Wait,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Allowed,
    Impossible,
    Check,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CheckDraft {
    pub against: Option<Handle>,
    /// The stakes. The per-Turn schema requires both; the core drops an
    /// illegal one (a PC key the agency rule refuses), and a Check the core
    /// builds itself has none (ADR 0007). The Roll still runs without them.
    #[serde(default)]
    pub win: Option<SlotDraft>,
    #[serde(default)]
    pub lose: Option<SlotDraft>,
    #[schemars(length(max = 3))]
    pub factors: Vec<Factor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Factor {
    pub fact: Handle,
    pub helps: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Reaction {
    pub npc: Handle,
    pub stance: Stance,
    #[schemars(length(max = 90))]
    pub plan: String,
    pub cause: Option<Cause>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    Check,
    Fact(Handle),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NewNpc {
    pub id: Handle,
    pub name: String,
    /// Age in years.
    #[schemars(range(min = 0, max = 999))]
    pub age: u16,
    #[schemars(length(max = 120))]
    pub voice: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SlotDraft {
    pub subject: Handle,
    #[schemars(length(max = 32))]
    pub key: String,
    #[schemars(length(max = 40))]
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NewFact {
    pub slot: SlotDraft,
    pub seen: Seen,
    pub said_by: Option<Handle>,
    pub because: Vec<Handle>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Seen {
    Pc,
    Present,
    Gm,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Cancel {
    pub event: Handle,
    pub because: Handle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EventDraft {
    pub in_minutes: u32,
    pub actor: Handle,
    pub goal: Option<Handle>,
    #[schemars(length(max = 80))]
    pub text: String,
    #[schemars(length(max = 2))]
    pub effects: Vec<SlotDraft>,
    /// The NPCs who witness the event when it fires, wherever the PC is.
    /// Never the PC: the PC learns of an event only by being there or by
    /// being told. Empty means only the GM knows (`seen = gm`).
    #[serde(default)]
    #[schemars(length(max = 2))]
    pub seen_by: Vec<Handle>,
}
