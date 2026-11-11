use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn empty_json_array() -> serde_json::Value {
    serde_json::json!([])
}

pub type EntityId = u64;
pub type Key = String;
pub type Value = String;
pub type ClockMin = u64;

/// Keys that hold changing state (normalized). A fact on one of these keys, or
/// on an open key (`is_open_key`), is never Persistent, so the state can
/// change on a later Turn. `alive` is not a state key: a death stays
/// protected.
pub const STATE_KEYS: &[&str] = &[
    "location",
    "condition",
    "injury",
    "fatigue",
    "carrying",
    "mood",
    "reputation",
    "standing",
    "debt",
];

/// True when `key`, once normalized, is in `STATE_KEYS` or is an open key.
pub fn is_state_key(key: &str) -> bool {
    let key = crate::normalize::key(key);
    STATE_KEYS.contains(&key.as_str()) || key.starts_with(OPEN_KEY_PREFIX)
}

/// The prefix of an open key (ADR 0005). A fact on an open key is an open
/// item: planned, proposed or pending, not done. Its outcome slot is the
/// key without the prefix, and stays empty until the item is done.
pub const OPEN_KEY_PREFIX: &str = "pending_";

/// True when `key`, once normalized, starts with `OPEN_KEY_PREFIX`. An open
/// key is a state key, so its fact is never Persistent.
pub fn is_open_key(key: &str) -> bool {
    crate::normalize::key(key).starts_with(OPEN_KEY_PREFIX)
}

/// The outcome slot of an open key: `key` without `OPEN_KEY_PREFIX`. `None`
/// when `key` has no prefix or nothing after it.
pub fn outcome_key(key: &str) -> Option<&str> {
    key.strip_prefix(OPEN_KEY_PREFIX)
        .filter(|rest| !rest.is_empty())
}

/// A plain English sentence for a slot write that has no sentence of its
/// own: a stake, a model fact, a World Event effect (bug pass 3). The
/// Player never reads `key = value`. A few keys have their own template;
/// any other reads "<name>'s <key words> is now <value words>.". An `_` in
/// the key or the value is a space. A value "none", "no", "nothing" or an
/// empty one reads as a plain no: "Ada is not injured.", "Ada has no
/// debt.".
pub fn slot_sentence(name: &str, key: &str, value: &str) -> String {
    let words = |text: &str| text.trim().replace('_', " ");
    let (name, value) = (name.trim(), words(value));
    // A value that says "none" reads as a plain no, never "injured (none)"
    // (review minor).
    if ["", "none", "no", "nothing", "n/a"].contains(&crate::normalize::value(&value).as_str()) {
        return match crate::normalize::key(key).as_str() {
            "location" => format!("{name}'s location is unknown."),
            "injury" => format!("{name} is not injured."),
            "condition" => format!("{name} is well."),
            "carrying" => format!("{name} carries nothing."),
            "grudge" => format!("{name} holds no grudge."),
            other => format!("{name} has no {}.", words(other)),
        };
    }
    match crate::normalize::key(key).as_str() {
        "location" => format!("{name} is at {value}."),
        "injury" => format!("{name} is injured ({value})."),
        "condition" => format!("{name} is {value}."),
        "carrying" => format!("{name} carries {value}."),
        "grudge" => format!("{name} holds a grudge against {value}."),
        // QA (bug pass 5): "Pim Quast's loyalty is now loyal to teodor.".
        // A value that starts with the key's own word reads as a state.
        other if same_stem(other, &value) => format!("{name} is now {value}."),
        other => format!("{name}'s {} is now {value}.", words(other)),
    }
}

/// True when the first word of `value` and the key share a stem of at
/// least 4 letters: "loyalty" and "loyal to Teodor", "anger" and "angry".
fn same_stem(key: &str, value: &str) -> bool {
    let first = value
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let key = key.replace('_', " ").to_lowercase();
    let shared = first
        .chars()
        .zip(key.chars())
        .take_while(|(a, b)| a == b)
        .count();
    shared >= 4 && first.chars().count() >= 4
}

/// The text of a `location` fact the core writes for an NPC who goes
/// somewhere (bug pass 5, M1): "Ludo Brenn left." with no place, "Ludo
/// Brenn went to the stone quay." with one. Past, so it stays true on a
/// card.
pub fn location_sentence(name: &str, place: Option<&str>) -> String {
    match place.map(str::trim).filter(|p| !p.is_empty()) {
        Some(place) => format!("{} went to {place}.", name.trim()),
        None => format!("{} left.", name.trim()),
    }
}

/// `slot_sentence` for the PC, as the Player reads it: in the second person
/// ("You are injured (bruised wrist).", "Your reputation is now
/// respected."). The Player screens use it for a state of the PC that a
/// Turn wrote (bug pass 4: "Teodor Brandt's fatigue is now heavy." read like
/// a machine under "What you learned").
///
/// Fatigue reads right for the values it knows ("rested", "tired",
/// "exhausted"...), and any other value in a neutral form, "Your fatigue:
/// light." (review minor: "rested" read "You are tired (rested).").
pub fn you_sentence(key: &str, value: &str) -> String {
    let words = |text: &str| text.trim().replace('_', " ");
    let value = words(value);
    let key = crate::normalize::key(key);
    let none =
        ["", "none", "no", "nothing", "n/a"].contains(&crate::normalize::value(&value).as_str());
    if key == "fatigue" {
        let normal = crate::normalize::value(&value);
        return if none || FATIGUE_RESTED.contains(&normal.as_str()) {
            "You are rested.".to_owned()
        } else if FATIGUE_TIRED.contains(&normal.as_str()) {
            format!("You are {normal}.")
        } else {
            format!("Your fatigue: {value}.")
        };
    }
    if none {
        return match key.as_str() {
            "location" => "You do not know where you are.".to_owned(),
            "injury" => "You are not injured.".to_owned(),
            "condition" => "You are well.".to_owned(),
            "carrying" => "You carry nothing.".to_owned(),
            other => format!("You have no {}.", words(other)),
        };
    }
    match key.as_str() {
        "location" => format!("You are at {value}."),
        "injury" => format!("You are injured ({value})."),
        "condition" => format!("You are {value}."),
        "carrying" => format!("You carry {value}."),
        other => format!("Your {} is now {value}.", words(other)),
    }
}

/// Fatigue values that mean no fatigue: "You are rested.".
const FATIGUE_RESTED: &[&str] = &["rested", "fresh", "well rested", "not tired"];

/// Fatigue values that read as "You are <value>.".
const FATIGUE_TIRED: &[&str] = &[
    "tired",
    "weary",
    "winded",
    "sleepy",
    "drained",
    "exhausted",
    "spent",
];

/// The prefix of the key of a fact that gives an entity one more name.
pub const ALIAS_KEY_PREFIX: &str = "alias_";

/// True when `key`, once normalized, starts with `ALIAS_KEY_PREFIX`.
pub fn is_alias_key(key: &str) -> bool {
    crate::normalize::key(key).starts_with(ALIAS_KEY_PREFIX)
}

/// The key of the fact that gives an entity the name `other_name`. The one
/// writer of `ALIAS_KEY_PREFIX`.
pub fn alias_key(other_name: &str) -> String {
    format!(
        "{ALIAS_KEY_PREFIX}{}",
        crate::normalize::name(other_name).replace(' ', "_")
    )
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
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
    AdventureSeed,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Done,
}

/// Names an entity inside a Commit: an existing id, or the `local` number of
/// an `AddEntity` op earlier in the same Commit.
///
/// `Id(n)` serializes as the plain number `n`; `New(n)` as `{"new": n}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "EntityRefRepr", into = "EntityRefRepr")]
pub enum EntityRef {
    Id(EntityId),
    New(u8),
}

#[derive(Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
enum EntityRefRepr {
    Id(EntityId),
    New { new: u8 },
}

impl From<EntityRefRepr> for EntityRef {
    fn from(repr: EntityRefRepr) -> Self {
        match repr {
            EntityRefRepr::Id(id) => Self::Id(id),
            EntityRefRepr::New { new } => Self::New(new),
        }
    }
}

impl From<EntityRef> for EntityRefRepr {
    fn from(entity: EntityRef) -> Self {
        match entity {
            EntityRef::Id(id) => Self::Id(id),
            EntityRef::New(new) => Self::New { new },
        }
    }
}

impl From<EntityId> for EntityRef {
    fn from(id: EntityId) -> Self {
        Self::Id(id)
    }
}

impl JsonSchema for EntityRef {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "EntityRef".into()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        EntityRefRepr::json_schema(generator)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Slot<E = EntityRef> {
    pub subject: E,
    pub key: Key,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Trigger<E = EntityRef> {
    At(ClockMin),
    When(Slot<E>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClaimDraft<E = EntityRef> {
    pub speaker: E,
    pub slot: Slot<E>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FactDraft<E = EntityRef> {
    pub slot: Slot<E>,
    pub text: String,
    pub canon: Canon,
    pub source: FactSource,
    pub seen_by: Vec<E>,
    #[serde(default)]
    pub causes: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Effect<E = EntityRef> {
    SetFact {
        slot: Slot<E>,
        text: String,
        seen_by: Vec<E>,
    },
    ShiftAttitude {
        from: E,
        to: E,
        delta: i8,
    },
    Tell {
        to: E,
        claim: ClaimDraft<E>,
        delay_min: u32,
    },
}

/// A ledger operation. Ops handed to `Store::commit` use `EntityRef`; the
/// ledger and the projections hold resolved ops, `LedgerOp<EntityId>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", bound(deserialize = "E: Deserialize<'de>"))]
pub enum LedgerOp<E = EntityRef> {
    AddEntity {
        /// The Commit-local number that later `EntityRef::New` refs name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local: Option<u8>,
        kind: EntityKind,
        name: String,
        /// A PC or an NPC has an age; nothing else does.
        age: Option<u16>,
        #[serde(default)]
        other_names: Vec<String>,
        /// The Player confirmed the age through the core (Age
        /// Confirmation). A ledger line stored before the field existed
        /// reads as unconfirmed.
        #[serde(default)]
        age_confirmed: bool,
    },
    /// The Player confirms that a PC or an NPC is an adult. The store
    /// refuses it for any other entity, and for a stored age under
    /// `domain::ADULT_AGE`. It never changes the age.
    ConfirmAge {
        entity: E,
    },
    AddFact {
        slot: Slot<E>,
        text: String,
        canon: Canon,
        source: FactSource,
        seen_by: Vec<E>,
        #[serde(default)]
        causes: Vec<u64>,
    },
    EstablishFact {
        fact_id: u64,
    },
    SupersedeFact {
        old: u64,
        new: FactDraft<E>,
    },
    AddClaim {
        claim: ClaimDraft<E>,
        #[serde(default)]
        listeners: Vec<E>,
        #[serde(default)]
        delay_min: u32,
    },
    AddVoiceCard {
        npc: E,
        habits: String,
        vocabulary: String,
        samples: Vec<String>,
    },
    AddGoal {
        npc: E,
        text: String,
        against: Option<E>,
    },
    SetGoalStatus {
        goal: u64,
        status: GoalStatus,
    },
    RecordCorrection {
        target_turn: u64,
        what_wrong: String,
        why: String,
    },
    Know {
        holder: E,
        fact_id: Option<u64>,
        claim_id: Option<u64>,
        how: Knowing,
        arrives_at: ClockMin,
    },
    ShiftAttitude {
        from: E,
        to: E,
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
        trigger: Trigger<E>,
        actors: Vec<E>,
        goal_id: Option<u64>,
        cause_fact_id: Option<u64>,
        effects: Vec<Effect<E>>,
        seen_by: Vec<E>,
        /// Everyone at the event's place sees or hears what it changes (an
        /// announcement, a public act). False when only `seen_by` notice
        /// it. A PC who witnesses a public event learns its effects
        /// (`world::with_witnessed_events`). An op stored before the field
        /// existed reads as not public.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        public: bool,
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
        /// The NPC the Check opposes, if any (bug pass 3): the narrator
        /// tells the contest against them.
        #[serde(
            default = "Option::default",
            skip_serializing_if = "Option::<E>::is_none"
        )]
        against: Option<E>,
        #[serde(default = "empty_json_array")]
        stakes: serde_json::Value,
        #[serde(default = "empty_json_array")]
        factors: serde_json::Value,
        value: i32,
        outcome: String,
    },
    RevertTurn {
        turn_id: u64,
    },
}

fn map_all<E, T, X>(items: Vec<E>, f: &mut impl FnMut(E) -> Result<T, X>) -> Result<Vec<T>, X> {
    items.into_iter().map(f).collect()
}

impl<E> Slot<E> {
    pub fn try_map<T, X>(self, f: &mut impl FnMut(E) -> Result<T, X>) -> Result<Slot<T>, X> {
        Ok(Slot {
            subject: f(self.subject)?,
            key: self.key,
            value: self.value,
        })
    }
}

impl<E> Trigger<E> {
    pub fn try_map<T, X>(self, f: &mut impl FnMut(E) -> Result<T, X>) -> Result<Trigger<T>, X> {
        Ok(match self {
            Self::At(n) => Trigger::At(n),
            Self::When(slot) => Trigger::When(slot.try_map(f)?),
        })
    }
}

impl<E> ClaimDraft<E> {
    pub fn try_map<T, X>(self, f: &mut impl FnMut(E) -> Result<T, X>) -> Result<ClaimDraft<T>, X> {
        Ok(ClaimDraft {
            speaker: f(self.speaker)?,
            slot: self.slot.try_map(f)?,
            text: self.text,
        })
    }
}

impl<E> FactDraft<E> {
    pub fn try_map<T, X>(self, f: &mut impl FnMut(E) -> Result<T, X>) -> Result<FactDraft<T>, X> {
        Ok(FactDraft {
            slot: self.slot.try_map(f)?,
            text: self.text,
            canon: self.canon,
            source: self.source,
            seen_by: map_all(self.seen_by, f)?,
            causes: self.causes,
        })
    }
}

impl<E> Effect<E> {
    pub fn try_map<T, X>(self, f: &mut impl FnMut(E) -> Result<T, X>) -> Result<Effect<T>, X> {
        Ok(match self {
            Self::SetFact {
                slot,
                text,
                seen_by,
            } => Effect::SetFact {
                slot: slot.try_map(f)?,
                text,
                seen_by: map_all(seen_by, f)?,
            },
            Self::ShiftAttitude { from, to, delta } => Effect::ShiftAttitude {
                from: f(from)?,
                to: f(to)?,
                delta,
            },
            Self::Tell {
                to,
                claim,
                delay_min,
            } => Effect::Tell {
                to: f(to)?,
                claim: claim.try_map(f)?,
                delay_min,
            },
        })
    }
}

impl<E> LedgerOp<E> {
    /// Maps every entity reference, keeping the order of the fields.
    pub fn try_map<T, X>(self, f: &mut impl FnMut(E) -> Result<T, X>) -> Result<LedgerOp<T>, X> {
        Ok(match self {
            Self::AddEntity {
                local,
                kind,
                name,
                age,
                other_names,
                age_confirmed,
            } => LedgerOp::AddEntity {
                local,
                kind,
                name,
                age,
                other_names,
                age_confirmed,
            },
            Self::ConfirmAge { entity } => LedgerOp::ConfirmAge { entity: f(entity)? },
            Self::AddFact {
                slot,
                text,
                canon,
                source,
                seen_by,
                causes,
            } => LedgerOp::AddFact {
                slot: slot.try_map(f)?,
                text,
                canon,
                source,
                seen_by: map_all(seen_by, f)?,
                causes,
            },
            Self::EstablishFact { fact_id } => LedgerOp::EstablishFact { fact_id },
            Self::SupersedeFact { old, new } => LedgerOp::SupersedeFact {
                old,
                new: new.try_map(f)?,
            },
            Self::AddClaim {
                claim,
                listeners,
                delay_min,
            } => LedgerOp::AddClaim {
                claim: claim.try_map(f)?,
                listeners: map_all(listeners, f)?,
                delay_min,
            },
            Self::AddVoiceCard {
                npc,
                habits,
                vocabulary,
                samples,
            } => LedgerOp::AddVoiceCard {
                npc: f(npc)?,
                habits,
                vocabulary,
                samples,
            },
            Self::AddGoal { npc, text, against } => LedgerOp::AddGoal {
                npc: f(npc)?,
                text,
                against: against.map(&mut *f).transpose()?,
            },
            Self::SetGoalStatus { goal, status } => LedgerOp::SetGoalStatus { goal, status },
            Self::RecordCorrection {
                target_turn,
                what_wrong,
                why,
            } => LedgerOp::RecordCorrection {
                target_turn,
                what_wrong,
                why,
            },
            Self::Know {
                holder,
                fact_id,
                claim_id,
                how,
                arrives_at,
            } => LedgerOp::Know {
                holder: f(holder)?,
                fact_id,
                claim_id,
                how,
                arrives_at,
            },
            Self::ShiftAttitude { from, to, delta } => LedgerOp::ShiftAttitude {
                from: f(from)?,
                to: f(to)?,
                delta,
            },
            Self::OpenObstacle { fact_id } => LedgerOp::OpenObstacle { fact_id },
            Self::ClearObstacle { obstacle_id } => LedgerOp::ClearObstacle { obstacle_id },
            Self::ScheduleEvent {
                text,
                trigger,
                actors,
                goal_id,
                cause_fact_id,
                effects,
                seen_by,
                public,
            } => LedgerOp::ScheduleEvent {
                text,
                trigger: trigger.try_map(f)?,
                actors: map_all(actors, f)?,
                goal_id,
                cause_fact_id,
                effects: effects
                    .into_iter()
                    .map(|effect| effect.try_map(f))
                    .collect::<Result<_, _>>()?,
                seen_by: map_all(seen_by, f)?,
                public,
            },
            Self::FireEvent { event_id } => LedgerOp::FireEvent { event_id },
            Self::CancelEvent {
                event_id,
                cause_fact_id,
            } => LedgerOp::CancelEvent {
                event_id,
                cause_fact_id,
            },
            Self::AdvanceClock { minutes } => LedgerOp::AdvanceClock { minutes },
            Self::RecordRoll {
                seed,
                difficulty,
                against,
                stakes,
                factors,
                value,
                outcome,
            } => LedgerOp::RecordRoll {
                seed,
                difficulty,
                against: against.map(&mut *f).transpose()?,
                stakes,
                factors,
                value,
                outcome,
            },
            Self::RevertTurn { turn_id } => LedgerOp::RevertTurn { turn_id },
        })
    }
}
