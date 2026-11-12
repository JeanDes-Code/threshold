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
    /// The NPCs who walk out of the PC's place in this Turn, and where to
    /// (bug pass 5, M1). The core writes each one's `location`. The per-Turn
    /// schema makes it required, with the present NPCs the PC perceives and
    /// the place handles plus `away`; an NPC who is not listed stays.
    #[serde(default)]
    #[schemars(length(max = 2))]
    pub leaves: Vec<Leave>,
    /// The one Open Item that the Player's Intent settles in this Turn, and
    /// how (ADR 0007, changed in bug pass 3). Its `item` is `none` or an
    /// Open Item the Intent names (`world::items_named_by`), as the prompt
    /// shows it (`world::open_item_ref`). The core closes the item only on
    /// an outcome other than `open`, and writes the outcome fact itself
    /// (ADR 0005). The per-Turn schema makes it required while an item is
    /// offered; with none offered it is absent.
    #[serde(default)]
    pub settles: Option<Settle>,
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
    /// True when this Turn's intent or its outcome involves sexual or
    /// erotic content between any characters, whether the PC takes part or
    /// only watches. Required in the per-Turn schema, and `turn::parse` refuses a raw
    /// proposal without it; `default` stays so that a stored proposal
    /// still reads. `mature::check` gates a proposal that sets it.
    #[serde(default)]
    pub intimate: bool,
    /// Set by the core only, never by the model: the core built this
    /// proposal's Check (`checks::default_check`), so its `facts`, written
    /// for an act that simply happens, apply on a won Roll only (ADR 0007,
    /// changed after review). Not in the schema and not parsed.
    #[serde(skip)]
    pub facts_on_win: bool,
    /// Set by the core only, never by the model: the Open Items this Turn
    /// offered (`HandleContext::open_items`, the items the Intent names).
    /// `settle` drops, and `validate` rejects, a `settles` whose item is not
    /// among them; `to_ops` closes no other item (review M1). Not in the
    /// schema and not parsed.
    #[serde(skip)]
    pub offered: Vec<String>,
}

/// The `to` of a `Leave`, or the `moves_actor_to` of an event, that names
/// no place: the NPC goes somewhere the world has no place for.
pub const AWAY: &str = "away";

/// One NPC who walks out of the PC's place in this Turn (bug pass 5, M1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Leave {
    pub npc: Handle,
    /// A place handle (`E12`), or `away`.
    pub to: String,
}

/// The `item` of a `Settle` that names no Open Item.
pub const NO_ITEM: &str = "none";

/// What the Player's Intent does to at most one Open Item. One choice, so at
/// most one item closes per Turn and the model names it instead of filling
/// a status for every held item (bug pass 3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Settle {
    /// `none`, or an Open Item as `world::open_item_ref` names it.
    pub item: String,
    pub outcome: ItemStatus,
}

impl Settle {
    /// The item this closes and how; `None` for item `none` or outcome
    /// `open`.
    pub fn closing(&self) -> Option<(&str, CloseOutcome)> {
        let outcome = self.outcome.outcome()?;
        (self.item != NO_ITEM).then_some((self.item.as_str(), outcome))
    }
}

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

    /// The outcome that `word` names (`word`'s inverse), if any.
    pub fn from_word(word: &str) -> Option<Self> {
        [
            Self::Accepted,
            Self::Refused,
            Self::Done,
            Self::Failed,
            Self::Dropped,
        ]
        .into_iter()
        .find(|o| o.word() == crate::normalize::value(word))
    }

    /// The word as the Player reads it: "Refused".
    pub fn head(self) -> &'static str {
        match self {
            Self::Accepted => "Accepted",
            Self::Refused => "Refused",
            Self::Done => "Done",
            Self::Failed => "Failed",
            Self::Dropped => "Dropped",
        }
    }

    /// The outcome as the Player reads it about the PC, in the second
    /// person: "You refused" (bug pass 5, M2: the OOC sheet).
    pub fn you(self) -> &'static str {
        match self {
            Self::Accepted => "You accepted",
            Self::Refused => "You refused",
            Self::Done => "You did it",
            Self::Failed => "You failed",
            Self::Dropped => "You gave it up",
        }
    }

    /// The text of the outcome fact, from a fixed English template and the
    /// Open Item's own stored text.
    pub fn text(self, item_text: &str) -> String {
        format!("{}: {}", self.head(), item_text.trim())
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

/// The longest reaction plan, in characters, outside an Intimate Scene.
pub const PLAN_MAX_CHARS: usize = 90;

/// The longest reaction plan inside a cleared Intimate Scene, in characters
/// (data shape 3.3, a first value). There a plan is also never empty.
pub const SCENE_PLAN_MAX_CHARS: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Reaction {
    pub npc: Handle,
    pub stance: Stance,
    /// At most `PLAN_MAX_CHARS`; 1 to `SCENE_PLAN_MAX_CHARS` inside a
    /// cleared Intimate Scene (`checks::validate` and the per-Turn schema).
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
    /// True when everyone at the place sees or hears what this event
    /// changes (an announcement, a public act). False when only the people
    /// in seen_by notice it.
    ///
    /// Required in the per-Turn schema (`proposal_schema_for`); a stored
    /// proposal without it reads as not public.
    #[serde(default)]
    pub public: bool,
    /// The place where the actor is when the event happens: a place handle,
    /// or `away` (bug pass 5, M1). The core adds the actor's `location`
    /// effect. `None`: the actor stays where they are. Required and
    /// nullable in the per-Turn schema.
    #[serde(default)]
    pub moves_actor_to: Option<String>,
}
