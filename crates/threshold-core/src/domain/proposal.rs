use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Handle, Stance};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DecisionProposal {
    pub action: ActionKind,
    /// True when an NPC actively opposes this act and the result is
    /// uncertain. Such an act needs a Check (ADR 0007). Required in the
    /// per-Turn schema.
    #[serde(default)]
    pub contested: bool,
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
    /// Set by the core only, never by the model: the core built this
    /// proposal's Check (`checks::default_check`), so its `facts`, written
    /// for an act that simply happens, apply on a won Roll only (ADR 0007,
    /// changed after review). Not in the schema and not parsed.
    #[serde(skip)]
    pub facts_on_win: bool,
}

/// The most `closes` items one proposal may hold (ADR 0007, changed after
/// review). The per-Turn schema and the core both enforce it.
pub const MAX_CLOSES: usize = 2;

/// One Open Item that the Intent settles: `item` names it as the prompt
/// shows it (`world::open_item_ref`), and `outcome` says how it ended. The
/// core builds the value and the text of the outcome fact from the item's
/// own text and the outcome word; no model text reaches that fact (ADR
/// 0007, changed after review).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Close {
    pub item: String,
    pub outcome: CloseOutcome,
}

/// How an Open Item ended. A closed list: serde parses no other value, so
/// free model text can never state a PC act or feeling here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CloseOutcome {
    Accepted,
    Refused,
    Done,
    Failed,
    Dropped,
}

impl CloseOutcome {
    /// The value of the outcome fact: the enum word.
    pub fn word(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Refused => "refused",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Dropped => "dropped",
        }
    }

    /// The text of the outcome fact, from a fixed English template and the
    /// Open Item's own stored text.
    pub fn text(self, item_text: &str) -> String {
        let item = item_text.trim();
        let head = match self {
            Self::Accepted => "Accepted",
            Self::Refused => "Refused",
            Self::Done => "Done",
            Self::Failed => "Failed",
            Self::Dropped => "Dropped",
        };
        format!("{head}: {item}")
    }
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
