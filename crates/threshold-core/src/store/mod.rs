mod migrations;
mod project;

use std::{fmt, path::Path};

use crate::domain::{
    AdventureSeed, Canon, ClaimDraft, ClockMin, ConfirmedAges, DetailLevel, Effect, EntityId,
    EntityKind, EntityRef, FactSource, Handle, Knowing, LedgerOp, OtherName, Slot, Trigger,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub type CampaignId = u64;
pub type TurnId = u64;

/// The schema version of a Campaign file. An older file is refused, with no
/// migration (ADR 0012).
pub const SCHEMA_VERSION: u32 = 4;
pub type OpId = String;

#[derive(Debug)]
pub enum StoreError {
    Database(String),
    Invalid(String),
    /// A Campaign file of another schema version. This build opens
    /// `SCHEMA_VERSION` only and migrates nothing.
    Version(u32),
}
impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(s) | Self::Invalid(s) => f.write_str(s),
            Self::Version(found) if *found > SCHEMA_VERSION => write!(
                f,
                "database schema version {found} is newer than supported version {SCHEMA_VERSION}"
            ),
            Self::Version(found) => write!(
                f,
                "this campaign file uses schema version {found}; this build opens version {SCHEMA_VERSION} only and does not migrate older files"
            ),
        }
    }
}
impl std::error::Error for StoreError {}
impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Database(e.to_string())
    }
}
impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::Invalid(e.to_string())
    }
}
#[derive(Debug)]
pub enum CommitError {
    DuplicateOp(TurnId),
    Store(StoreError),
}
impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateOp(id) => write!(f, "duplicate operation: {id}"),
            Self::Store(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for CommitError {}
impl From<StoreError> for CommitError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}
impl From<rusqlite::Error> for CommitError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(e.into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnKind {
    Setup,
    Play,
    Correction,
    Ooc,
    /// Begin: the opening Narration of the start state, with zero ops and
    /// an empty Intent (data shape 3.7). Stored as `opening`.
    Opening,
    /// The Turn that ends an Intimate Scene ("End scene" or "Fade to
    /// black"): a Narration and no Intent (mature content data shape 3.7).
    /// Stored as `closing`.
    Closing,
}

impl TurnKind {
    /// True for a Turn that has a Narration: it is part of the story.
    pub fn tells(self) -> bool {
        matches!(
            self,
            Self::Opening | Self::Play | Self::Correction | Self::Closing
        )
    }

    /// True for a Turn that carries a Player's Intent: an act of the PC.
    pub fn has_intent(self) -> bool {
        matches!(self, Self::Play | Self::Correction)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnRecord {
    pub campaign_id: CampaignId,
    pub op_id: OpId,
    pub kind: TurnKind,
    pub intent: String,
    pub proposal_json: String,
    pub clock_before: ClockMin,
    pub clock_after: ClockMin,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredTurn {
    pub id: TurnId,
    pub seq: u64,
    pub record: TurnRecord,
    pub narration: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,
    pub name: String,
    pub norm_name: String,
    /// A PC or an NPC has an age; nothing else does.
    pub age: Option<u16>,
    pub other_names: Vec<String>,
    /// The Player confirmed the age through the core (Age Confirmation).
    /// Only `mature::clearance` reads it.
    pub age_confirmed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub id: u64,
    pub slot: Slot<EntityId>,
    pub text: String,
    pub canon: Canon,
    pub source: FactSource,
    pub created_turn: TurnId,
    pub superseded_by: Option<u64>,
    pub seen_by: Vec<EntityId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    pub id: u64,
    pub claim: ClaimDraft<EntityId>,
    pub turn_id: TurnId,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Obstacle {
    pub id: u64,
    pub fact_id: u64,
    pub opened_turn: TurnId,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldEvent {
    pub id: u64,
    pub text: String,
    pub trigger: Trigger<EntityId>,
    pub actors: Vec<EntityId>,
    pub goal_id: Option<u64>,
    pub cause_fact_id: Option<u64>,
    pub scheduled_turn: TurnId,
    pub effects: Vec<Effect<EntityId>>,
    pub seen_by: Vec<EntityId>,
    /// Everyone at the place sees or hears what the event changes. The
    /// projection has no column for it: the store reads it from the
    /// event's ledger op, and an op without it reads as not public.
    pub public: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceCard {
    pub npc: EntityId,
    pub habits: String,
    pub vocabulary: String,
    pub samples: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Goal {
    pub id: u64,
    pub npc_id: EntityId,
    pub text: String,
    pub against_id: Option<EntityId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attitude {
    pub from: EntityId,
    pub to: EntityId,
    pub attitude: i8,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Knowledge {
    pub holder: EntityId,
    pub fact_id: Option<u64>,
    pub claim_id: Option<u64>,
    pub how: Knowing,
    pub learned_at: ClockMin,
    pub arrives_at: ClockMin,
    /// The Turn whose `Know` op wrote the row.
    pub turn: TurnId,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldView {
    pub clock_min: ClockMin,
    /// The campaign's Adventure Seed. `None` only in hand-built views.
    pub adventure_seed: Option<AdventureSeed>,
    /// The Detail Level (projection `campaign.detail_level`): the seed's
    /// `detail` at creation and after a rebuild.
    pub detail: DetailLevel,
    /// The Turn that opened the Intimate Scene, while one is open
    /// (projection `campaign.scene_open_turn`, `LedgerOp::SetScene`).
    pub scene_open: Option<TurnId>,
    pub entities: Vec<Entity>,
    pub live_facts: Vec<Fact>,
    pub facts: Vec<Fact>,
    pub claims: Vec<Claim>,
    pub obstacles_open: Vec<Obstacle>,
    pub events_scheduled: Vec<WorldEvent>,
    pub goals_active: Vec<Goal>,
    pub attitudes: Vec<Attitude>,
    pub knowledge: Vec<Knowledge>,
    pub recent_turns: Vec<StoredTurn>,
    pub turn_seq: u64,
    /// Every Voice Card, ordered by NPC id.
    pub voice_cards: Vec<VoiceCard>,
    /// The entities whose main name the Narration of a standing Opening,
    /// Play or Correction Turn prints, a hidden Adventure Seed entry left
    /// out (rule 4 of "the PC can name", play experience data shape 3.5;
    /// `world::story_names`). The store reads every standing Narration
    /// once per snapshot, so every reader of "the PC can name" sees the
    /// whole story, not the last six Turns (review of bug pass 4, m1).
    pub story_names: std::collections::HashSet<EntityId>,
}
impl WorldView {
    pub fn voice_card(&self, npc: EntityId) -> Option<&VoiceCard> {
        self.voice_cards.iter().find(|card| card.npc == npc)
    }

    pub fn present(&self) -> Vec<EntityId> {
        let Some(pc) = self
            .entities
            .iter()
            .find(|entity| entity.kind == EntityKind::Pc)
        else {
            return Vec::new();
        };
        let location = self
            .live_fact(pc.id, "location")
            .map(|fact| crate::normalize::value(&fact.slot.value));
        self.entities
            .iter()
            .filter(|entity| {
                entity.id == pc.id
                    || location.as_ref().is_some_and(|location| {
                        self.live_fact(entity.id, "location").is_some_and(|fact| {
                            crate::normalize::value(&fact.slot.value) == *location
                        })
                    })
            })
            .map(|entity| entity.id)
            .collect()
    }

    /// The entity that `name` names: its main name or one of its other
    /// names, compared through `normalize::name`. The lowest id wins a tie.
    /// Every name match in the core goes through this function or through
    /// `entities_by_name`.
    pub fn entity_by_name(&self, name: &str) -> Option<&Entity> {
        self.entities_by_name(name).min_by_key(|e| e.id)
    }

    /// Every entity that answers to `name` (main name or other name), in
    /// the order of `entities`. Two entities can share a name in play.
    pub fn entities_by_name<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a Entity> {
        let name = crate::normalize::name(name);
        self.entities.iter().filter(move |e| {
            e.norm_name == name
                || e.other_names
                    .iter()
                    .any(|other| crate::normalize::name(other) == name)
        })
    }

    pub fn entity(&self, handle: Handle) -> Option<&Entity> {
        if let Handle::Entity(id) = handle {
            self.entities.iter().find(|e| e.id == id)
        } else {
            None
        }
    }
    pub fn live_fact(&self, subject: EntityId, key: &str) -> Option<&Fact> {
        let key = crate::normalize::key(key);
        self.live_facts
            .iter()
            .find(|f| f.slot.subject == subject && f.slot.key == key && f.canon != Canon::Possible)
            .or_else(|| {
                self.live_facts
                    .iter()
                    .find(|f| f.slot.subject == subject && f.slot.key == key)
            })
    }
    pub fn fact(&self, handle: Handle) -> Option<&Fact> {
        if let Handle::Fact(id) = handle {
            self.facts.iter().find(|f| f.id == id)
        } else {
            None
        }
    }
}
#[derive(Clone, Debug)]
pub struct DebugEntry {
    pub op_id: OpId,
    pub step: String,
    pub prompt: String,
    pub raw: String,
    pub ms: u64,
}
pub struct Store {
    db: Connection,
}

/// What the home screen shows of a Campaign file, read without opening it
/// for play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignPeek {
    pub pc_name: String,
    pub premise: String,
    /// The Play and Correction Turns that stand. The Opening and the Setup
    /// Turns do not count, nor a reverted Turn.
    pub turn_count: u32,
    /// No Opening Turn and no Play Turn yet.
    pub needs_begin: bool,
}

/// Reads the last Campaign of a file for the home screen. It opens the file
/// read-only, runs no migration and writes nothing. Any schema version but
/// `SCHEMA_VERSION` is refused with `StoreError::Version`.
pub fn peek(path: &Path) -> Result<CampaignPeek, StoreError> {
    let not_ours = || {
        StoreError::Invalid(format!(
            "{} is not a Threshold campaign file",
            path.display()
        ))
    };
    if !path.is_file() {
        return Err(StoreError::Invalid(format!(
            "no file at {}",
            path.display()
        )));
    }
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: u32 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| not_ours())?;
    if version == 0 {
        return Err(not_ours());
    }
    if version != SCHEMA_VERSION {
        return Err(StoreError::Version(version));
    }
    let store = Store { db };
    let campaign = *store
        .campaigns()
        .map_err(|_| not_ours())?
        .last()
        .ok_or_else(|| StoreError::Invalid("the file holds no campaign".into()))?;
    let seed: String = store.db.query_row(
        "SELECT adventure_seed_json FROM campaign WHERE id=?1",
        [campaign],
        |r| r.get(0),
    )?;
    let seed: AdventureSeed = serde_json::from_str(&seed)?;
    let turns = store.turns(campaign)?;
    let reverted = store.reverted_turns(campaign)?;
    let turn_count = turns
        .iter()
        .filter(|t| t.record.kind.has_intent())
        .filter(|t| !reverted.contains(&t.id))
        .count() as u32;
    let needs_begin = !turns.iter().any(|t| t.record.kind.tells());
    Ok(CampaignPeek {
        pc_name: seed.pc.name,
        premise: seed.premise,
        turn_count,
        needs_begin,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::init(Connection::open(path)?)
    }
    /// Opens a campaign file that already exists. Refuses, before any
    /// migration runs, a missing file or one that is not a Threshold
    /// database, so it never writes to a foreign file.
    pub fn open_existing(path: &Path) -> Result<Self, StoreError> {
        let not_ours = || {
            StoreError::Invalid(format!(
                "{} is not a Threshold campaign file",
                path.display()
            ))
        };
        if !path.is_file() {
            return Err(StoreError::Invalid(format!(
                "no file at {}",
                path.display()
            )));
        }
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let version: u32 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|_| not_ours())?;
        let has_campaign: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='campaign')",
                [],
                |r| r.get(0),
            )
            .map_err(|_| not_ours())?;
        if version == 0 || !has_campaign {
            return Err(not_ours());
        }
        Self::init(db)
    }
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }
    fn init(db: Connection) -> Result<Self, StoreError> {
        migrations::apply(&db)?;
        Ok(Self { db })
    }
    pub fn create_campaign(
        &mut self,
        adventure_seed: &AdventureSeed,
        rng_seed: u64,
    ) -> Result<CampaignId, StoreError> {
        self.create_campaign_with(adventure_seed, rng_seed, None, |_| Vec::new())
    }
    /// `create_campaign`, plus the ops `setup` returns for the projected
    /// setup state, in the same Setup Turn and the same transaction.
    ///
    /// `ages` are the ages the Player confirmed in a Seed Draft with no gap
    /// (`build::confirmed_ages`). A seed character's `AddEntity` op has
    /// `age_confirmed: true` only when `ages` holds its kind, its exact main
    /// name and its age. `None` (the fixture path) confirms no age.
    pub fn create_campaign_with(
        &mut self,
        adventure_seed: &AdventureSeed,
        rng_seed: u64,
        ages: Option<&ConfirmedAges>,
        setup: impl FnOnce(&WorldView) -> Vec<LedgerOp>,
    ) -> Result<CampaignId, StoreError> {
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO campaign(adventure_seed_json,clock_min,seed,detail_level) VALUES(?1,0,?2,?3)",
            params![
                serde_json::to_string(adventure_seed)?,
                rng_seed as i64,
                project::enum_text(&adventure_seed.detail)?
            ],
        )?;
        let campaign = tx.last_insert_rowid() as u64;
        // The World Clock starts at the seed's hour of Day 1 (QA,
        // 2026-11-10). A ledger op sets it, so a rebuild finds it again.
        let start = adventure_seed.start.start_minute();
        let rec = TurnRecord {
            campaign_id: campaign,
            op_id: "__seed__".into(),
            kind: TurnKind::Setup,
            intent: String::new(),
            proposal_json: "{}".into(),
            clock_before: 0,
            clock_after: start,
        };
        let turn = insert_turn(&tx, &rec)?;
        if start > 0 {
            let minutes = u32::try_from(start).expect("a start hour is at most 23");
            project::append(&tx, campaign, turn, &LedgerOp::AdvanceClock { minutes })?;
        }
        let pc_seed = &adventure_seed.pc;
        let declared: Vec<(&str, &[OtherName])> =
            std::iter::once((pc_seed.name.as_str(), pc_seed.other_names.as_slice()))
                .chain(
                    adventure_seed
                        .entities
                        .iter()
                        .map(|e| (e.name.as_str(), e.other_names.as_slice())),
                )
                .collect();
        let added = std::iter::once((EntityKind::Pc, Some(pc_seed.age)))
            .chain(adventure_seed.entities.iter().map(|e| (e.kind, e.age)));
        let mut ids = Vec::new();
        for (&(name, other_names), (kind, age)) in declared.iter().zip(added) {
            let names = std::iter::once(name).chain(other_names.iter().map(|o| o.name.as_str()));
            if names
                .into_iter()
                .any(|n| crate::normalize::name(n) == "world")
            {
                return Err(StoreError::Invalid(format!(
                    "world is a reserved adventure seed name, main or other (entity {name})"
                )));
            }
            // The one writer of a confirmed age at creation: the exact
            // entry the Player confirmed in a Seed Draft with no gap.
            let age_confirmed = ages
                .zip(age)
                .is_some_and(|(ages, age)| ages.holds(kind, name, age));
            let id = project::append(
                &tx,
                campaign,
                turn,
                &LedgerOp::AddEntity {
                    local: None,
                    kind,
                    name: name.to_owned(),
                    age,
                    other_names: other_names.iter().map(|o| o.name.clone()).collect(),
                    age_confirmed,
                },
            )?
            .ok_or_else(|| StoreError::Invalid("missing entity id".into()))?;
            ids.push(id);
        }
        let view = project::snapshot(&tx, campaign)?;
        // Each name, main or other, names one entity only. Two other names
        // of one entity with the same normal form would share an alias slot.
        for (&(name, other_names), id) in declared.iter().zip(&ids) {
            let mut own = std::collections::HashSet::new();
            for other in other_names {
                if !own.insert(crate::normalize::name(&other.name)) {
                    return Err(StoreError::Invalid(format!(
                        "{name} has the same other name twice: {}",
                        other.name
                    )));
                }
            }
            let names = std::iter::once(name).chain(other_names.iter().map(|o| o.name.as_str()));
            for n in names {
                if view.entities_by_name(n).any(|e| e.id != *id) {
                    return Err(StoreError::Invalid(format!(
                        "duplicate adventure seed name: {n}"
                    )));
                }
            }
        }
        // Who knows that two names are one entity: one Persistent fact per
        // other name, held by the bearer and each name in `known_to`.
        for (&(name, other_names), &bearer) in declared.iter().zip(&ids) {
            for other in other_names {
                let mut holders = vec![bearer];
                for known_to in &other.known_to {
                    let holder = view
                        .entity_by_name(known_to)
                        .ok_or_else(|| {
                            StoreError::Invalid(format!(
                                "unknown name in known_to of {}: {known_to}",
                                other.name
                            ))
                        })?
                        .id;
                    if !holders.contains(&holder) {
                        holders.push(holder);
                    }
                }
                let fact = project::append(
                    &tx,
                    campaign,
                    turn,
                    &LedgerOp::AddFact {
                        slot: Slot {
                            subject: bearer,
                            key: crate::domain::alias_key(&other.name),
                            value: other.name.clone(),
                        },
                        text: format!("{name} is also called {}.", other.name),
                        canon: Canon::Persistent,
                        source: FactSource::AdventureSeed,
                        seen_by: holders.clone(),
                        causes: vec![],
                    },
                )?
                .ok_or_else(|| StoreError::Invalid("missing fact id".into()))?;
                for holder in holders {
                    project::append(
                        &tx,
                        campaign,
                        turn,
                        &LedgerOp::Know {
                            holder,
                            fact_id: Some(fact),
                            claim_id: None,
                            how: Knowing::Witnessed,
                            arrives_at: 0,
                        },
                    )?;
                }
            }
        }
        let mut world = None;
        for (facts, canon) in [
            (&adventure_seed.decided, Canon::Persistent),
            (&adventure_seed.proposed, Canon::Possible),
        ] {
            for f in facts {
                let subject = if let Some(e) = view.entity_by_name(&f.subject) {
                    e.id
                } else if crate::normalize::name(&f.subject) == "world" {
                    match world {
                        Some(id) => id,
                        None => {
                            let id = project::append(
                                &tx,
                                campaign,
                                turn,
                                &LedgerOp::AddEntity {
                                    local: None,
                                    kind: EntityKind::Place,
                                    name: f.subject.clone(),
                                    age: None,
                                    other_names: vec![],
                                    age_confirmed: false,
                                },
                            )?
                            .ok_or_else(|| StoreError::Invalid("missing subject id".into()))?;
                            world = Some(id);
                            id
                        }
                    }
                } else {
                    return Err(StoreError::Invalid(format!(
                        "unknown adventure seed subject: {}",
                        f.subject
                    )));
                };
                project::append(
                    &tx,
                    campaign,
                    turn,
                    &LedgerOp::AddFact {
                        slot: Slot {
                            subject,
                            key: crate::normalize::key(&f.key),
                            value: crate::normalize::value(&f.value),
                        },
                        text: f.text.clone(),
                        // A decided state key (location, mood...) is Established.
                        canon: if canon == Canon::Persistent && crate::domain::is_state_key(&f.key)
                        {
                            Canon::Established
                        } else {
                            canon
                        },
                        source: FactSource::AdventureSeed,
                        seen_by: vec![],
                        causes: vec![],
                    },
                )?;
            }
        }
        let view = project::snapshot(&tx, campaign)?;
        let assigned = std::collections::HashMap::new();
        for op in setup(&view) {
            project::append(&tx, campaign, turn, &resolve(&op, &assigned)?)?;
        }
        tx.commit()?;
        Ok(campaign)
    }
    pub fn snapshot(&self, c: CampaignId) -> Result<WorldView, StoreError> {
        project::snapshot(&self.db, c)
    }
    pub fn snapshot_before(&self, c: CampaignId, turn_seq: u64) -> Result<WorldView, StoreError> {
        project::historical(&self.db, c, turn_seq)
    }
    pub fn commit(&mut self, rec: TurnRecord, ops: Vec<LedgerOp>) -> Result<TurnId, CommitError> {
        let tx = self.db.transaction()?;
        let previous: Option<u64> = tx
            .query_row(
                "SELECT id FROM turn WHERE campaign_id=?1 AND op_id=?2",
                params![rec.campaign_id, rec.op_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = previous {
            return Err(CommitError::DuplicateOp(id));
        }
        let mut locals = std::collections::HashSet::new();
        for op in &ops {
            if let LedgerOp::AddEntity {
                local: Some(local), ..
            } = op
                && !locals.insert(*local)
            {
                return Err(
                    StoreError::Invalid(format!("duplicate local entity ref {local}")).into(),
                );
            }
        }
        let turn = insert_turn(&tx, &rec)?;
        let mut assigned = std::collections::HashMap::new();
        let revert = ops
            .iter()
            .any(|op| matches!(op, LedgerOp::RevertTurn { .. }));
        if revert {
            for op in &ops {
                if matches!(op, LedgerOp::RevertTurn { .. }) {
                    let op = resolve(op, &assigned)?;
                    project::append(&tx, rec.campaign_id, turn, &op)?;
                }
            }
            project::rebuild(&tx, rec.campaign_id)?;
        }
        for op in &ops {
            if matches!(op, LedgerOp::RevertTurn { .. }) {
                continue;
            }
            let resolved = resolve(op, &assigned)?;
            let id = project::append(&tx, rec.campaign_id, turn, &resolved)?;
            if let LedgerOp::AddEntity {
                local: Some(local), ..
            } = resolved
            {
                let entity = id.ok_or_else(|| StoreError::Invalid("missing entity id".into()))?;
                assigned.insert(local, entity);
            }
            let op = &resolved;
            let seen = match op {
                LedgerOp::AddFact { seen_by, .. } => Some(seen_by),
                LedgerOp::SupersedeFact { new, .. } => Some(&new.seen_by),
                _ => None,
            };
            if let Some(seen_by) = seen {
                let fact_id = id.ok_or_else(|| StoreError::Invalid("missing fact id".into()))?;
                let clock: u64 = tx.query_row(
                    "SELECT clock_min FROM campaign WHERE id=?1",
                    [rec.campaign_id],
                    |r| r.get(0),
                )?;
                let mut holders = std::collections::HashSet::new();
                for holder in seen_by {
                    if !holders.insert(*holder) {
                        continue;
                    }
                    let explicit = ops.iter().any(|candidate| matches!(candidate, LedgerOp::Know { holder: h, fact_id: Some(f), claim_id: None, how: Knowing::Witnessed, .. } if resolve_ref(*h, &assigned).ok() == Some(*holder) && *f == fact_id));
                    if !explicit {
                        project::append(
                            &tx,
                            rec.campaign_id,
                            turn,
                            &LedgerOp::Know {
                                holder: *holder,
                                fact_id: Some(fact_id),
                                claim_id: None,
                                how: Knowing::Witnessed,
                                arrives_at: clock,
                            },
                        )?;
                    }
                }
            }
            if let LedgerOp::AddClaim {
                listeners,
                delay_min,
                ..
            } = op
            {
                let claim_id = id.ok_or_else(|| StoreError::Invalid("missing claim id".into()))?;
                let clock: u64 = tx.query_row(
                    "SELECT clock_min FROM campaign WHERE id=?1",
                    [rec.campaign_id],
                    |r| r.get(0),
                )?;
                let arrives_at = clock
                    .checked_add(u64::from(*delay_min))
                    .ok_or_else(|| StoreError::Invalid("clock overflow".into()))?;
                let mut unique = std::collections::HashSet::new();
                for holder in listeners {
                    if unique.insert(*holder) {
                        project::append(
                            &tx,
                            rec.campaign_id,
                            turn,
                            &LedgerOp::Know {
                                holder: *holder,
                                fact_id: None,
                                claim_id: Some(claim_id),
                                how: Knowing::Told,
                                arrives_at,
                            },
                        )?;
                    }
                }
            }
        }
        project::validate_supersessions(&tx, rec.campaign_id)?;
        tx.commit()?;
        Ok(turn)
    }
    pub fn voice_card(&self, npc: EntityId) -> Result<Option<VoiceCard>, StoreError> {
        let row: Option<(String, String, String)> = self
            .db
            .query_row(
                "SELECT habits,vocabulary,samples_json FROM voice_card WHERE npc_id=?1",
                [npc],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        row.map(|(habits, vocabulary, samples)| {
            Ok(VoiceCard {
                npc,
                habits,
                vocabulary,
                samples: serde_json::from_str(&samples)?,
            })
        })
        .transpose()
    }
    pub fn turn_by_op(&self, op: &OpId) -> Result<Option<StoredTurn>, StoreError> {
        let mut stmt = self
            .db
            .prepare("SELECT id FROM turn WHERE op_id=?1 ORDER BY id LIMIT 2")?;
        let ids = stmt
            .query_map([op], |r| r.get::<_, u64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if ids.len() > 1 {
            return Err(StoreError::Invalid(
                "op_id belongs to multiple campaigns; use turn_by_campaign_op".into(),
            ));
        }
        ids.first()
            .map(|id| project::read_turn(&self.db, *id))
            .transpose()
    }
    pub fn turn_by_campaign_op(
        &self,
        campaign: CampaignId,
        op: &OpId,
    ) -> Result<Option<StoredTurn>, StoreError> {
        let id: Option<u64> = self
            .db
            .query_row(
                "SELECT id FROM turn WHERE campaign_id=?1 AND op_id=?2",
                params![campaign, op],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| project::read_turn(&self.db, id)).transpose()
    }
    /// The campaign's Roll seed, as given to `create_campaign`.
    pub fn campaign_seed(&self, c: CampaignId) -> Result<u64, StoreError> {
        let seed: Option<i64> = self
            .db
            .query_row("SELECT seed FROM campaign WHERE id=?1", [c], |r| r.get(0))
            .optional()?;
        seed.map(|seed| seed as u64)
            .ok_or_else(|| StoreError::Invalid("unknown campaign".into()))
    }
    /// Every campaign id in the file, oldest first.
    pub fn campaigns(&self) -> Result<Vec<CampaignId>, StoreError> {
        let mut stmt = self.db.prepare("SELECT id FROM campaign ORDER BY id")?;
        let ids = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }
    /// Every Turn of a campaign, Setup Turns included, in `seq` order.
    pub fn turns(&self, c: CampaignId) -> Result<Vec<StoredTurn>, StoreError> {
        let mut stmt = self
            .db
            .prepare("SELECT id FROM turn WHERE campaign_id=?1 ORDER BY seq")?;
        let ids = stmt
            .query_map([c], |r| r.get::<_, u64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| project::read_turn(&self.db, id))
            .collect()
    }
    /// The Turns of a campaign that a later Turn reverted (a Correction).
    /// A revert inside a Turn that was itself reverted does not count.
    pub fn reverted_turns(
        &self,
        c: CampaignId,
    ) -> Result<std::collections::BTreeSet<TurnId>, StoreError> {
        let mut reverted = std::collections::BTreeSet::new();
        for turn in self.turns(c)?.iter().rev() {
            if reverted.contains(&turn.id) {
                continue;
            }
            for op in self.turn_ops(turn.id)? {
                if let LedgerOp::RevertTurn { turn_id } = op {
                    reverted.insert(turn_id);
                }
            }
        }
        Ok(reverted)
    }
    /// One Turn row by id.
    pub fn turn(&self, t: TurnId) -> Result<Option<StoredTurn>, StoreError> {
        let exists: bool =
            self.db
                .query_row("SELECT EXISTS(SELECT 1 FROM turn WHERE id=?1)", [t], |r| {
                    r.get(0)
                })?;
        exists.then(|| project::read_turn(&self.db, t)).transpose()
    }
    /// The resolved ledger ops of one Turn, in ledger order. The store adds
    /// the `Know` rows of witnesses and listeners, so they are included.
    pub fn turn_ops(&self, t: TurnId) -> Result<Vec<LedgerOp<EntityId>>, StoreError> {
        project::turn_ops(&self.db, t)
    }
    pub fn set_narration(&mut self, t: TurnId, text: &str) -> Result<(), StoreError> {
        if self
            .db
            .execute("UPDATE turn SET narration=?1 WHERE id=?2", params![text, t])?
            != 1
        {
            return Err(StoreError::Invalid("unknown turn".into()));
        }
        Ok(())
    }
    pub fn rebuild_projections(&mut self, c: CampaignId) -> Result<(), StoreError> {
        let tx = self.db.transaction()?;
        project::rebuild(&tx, c)?;
        tx.commit()?;
        Ok(())
    }
    /// The debug rows of one operation, in the order they were written.
    pub fn debug_log(&self, op_id: &str) -> Result<Vec<DebugEntry>, StoreError> {
        let mut stmt = self
            .db
            .prepare("SELECT op_id,step,prompt,raw,ms FROM debug_log WHERE op_id=?1 ORDER BY id")?;
        let rows = stmt
            .query_map([op_id], |r| {
                Ok(DebugEntry {
                    op_id: r.get(0)?,
                    step: r.get(1)?,
                    prompt: r.get(2)?,
                    raw: r.get(3)?,
                    ms: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn log_debug(&self, e: DebugEntry) -> Result<(), StoreError> {
        self.db.execute(
            "INSERT INTO debug_log(op_id,step,prompt,raw,ms) VALUES(?1,?2,?3,?4,?5)",
            params![e.op_id, e.step, e.prompt, e.raw, e.ms],
        )?;
        Ok(())
    }
}
fn resolve_ref(
    entity: EntityRef,
    assigned: &std::collections::HashMap<u8, EntityId>,
) -> Result<EntityId, StoreError> {
    match entity {
        EntityRef::Id(id) => Ok(id),
        EntityRef::New(local) => assigned.get(&local).copied().ok_or_else(|| {
            StoreError::Invalid(format!(
                "local entity ref {local} has no earlier AddEntity in this commit"
            ))
        }),
    }
}
/// Replaces every `EntityRef::New(n)` with the id that the earlier `AddEntity`
/// with `local == Some(n)` received in this Commit.
fn resolve(
    op: &LedgerOp,
    assigned: &std::collections::HashMap<u8, EntityId>,
) -> Result<LedgerOp<EntityId>, StoreError> {
    op.clone()
        .try_map(&mut |entity| resolve_ref(entity, assigned))
}
fn insert_turn(db: &Connection, r: &TurnRecord) -> Result<TurnId, StoreError> {
    let kind = project::enum_text(&r.kind)?;
    db.execute("INSERT INTO turn(campaign_id,seq,op_id,kind,intent,proposal_json,clock_before,clock_after) SELECT id,COALESCE((SELECT MAX(seq)+1 FROM turn WHERE campaign_id=?1),1),?2,?3,?4,?5,?6,?7 FROM campaign WHERE id=?1", params![r.campaign_id,r.op_id,kind,r.intent,r.proposal_json,r.clock_before,r.clock_after])?;
    if db.changes() == 0 {
        return Err(StoreError::Invalid("unknown campaign".into()));
    }
    Ok(db.last_insert_rowid() as u64)
}
