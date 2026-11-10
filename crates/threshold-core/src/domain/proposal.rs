use std::collections::BTreeMap;

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
    /// One status for each Open Item the PC holds, keyed by the item as the
    /// prompt shows it (`world::open_item_ref`). `open` leaves the item
    /// open; any other status closes it, and the core writes the outcome
    /// fact (ADR 0005, ADR 0007). The per-Turn schema makes it required,
    /// with exactly the held items as keys; with none held it is absent.
    #[serde(default)]
    pub open_items: BTreeMap<String, ItemStatus>,
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

/// The most Open Items one proposal may close (ADR 0007, changed after
/// review). `validate` rejects more; `settle` keeps the first ones.
pub const MAX_CLOSES: usize = 2;

/// What the Player's Intent does to one Open Item the PC holds. A closed
/// list: serde parses no other value. `open` means the item stays open:
/// the Intent does not settle it, or only asks, talks about it or puts it
/// off. Every other status closes it with that outcome (ADR 0007, changed
/// after the live run of cdefde8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Open,
    Accepted,
    Refused,
    Done,
    Failed,
    Dropped,
}

impl ItemStatus {
    /// Every status, `open` first: the order of the per-Turn schema's enum.
    pub const ALL: [Self; 6] = [
        Self::Open,
        Self::Accepted,
        Self::Refused,
        Self::Done,
        Self::Failed,
        Self::Dropped,
    ];

    /// The outcome this status closes the item with; `None` for `open`.
    pub fn outcome(self) -> Option<CloseOutcome> {
        match self {
            Self::Open => None,
            Self::Accepted => Some(CloseOutcome::Accepted),
            Self::Refused => Some(CloseOutcome::Refused),
            Self::Done => Some(CloseOutcome::Done),
            Self::Failed => Some(CloseOutcome::Failed),
            Self::Dropped => Some(CloseOutcome::Dropped),
        }
    }

    /// The status word, as the schema and the prompt spell it.
    pub fn word(self) -> &'static str {
        self.outcome().map_or("open", CloseOutcome::word)
    }
}

/// How an Open Item ended: a closing `ItemStatus`. The core builds the value
/// and the text of the outcome fact from it and the item's own text; no
/// model text reaches that fact (ADR 0007, changed after review).
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
