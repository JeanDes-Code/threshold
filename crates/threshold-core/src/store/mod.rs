mod migrations;
mod project;

use std::{fmt, path::Path};

use crate::domain::{
    AdventureSeed, Canon, ClaimDraft, ClockMin, Effect, EntityId, EntityKind, FactSource, Handle,
    Knowing, LedgerOp, Slot, Trigger,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub type CampaignId = u64;
pub type TurnId = u64;
pub type OpId = String;

#[derive(Debug)]
pub enum StoreError {
    Database(String),
    Invalid(String),
}
impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(s) | Self::Invalid(s) => f.write_str(s),
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
    pub adult: Option<bool>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub id: u64,
    pub slot: Slot,
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
    pub claim: ClaimDraft,
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
    pub trigger: Trigger,
    pub actors: Vec<EntityId>,
    pub goal_id: Option<u64>,
    pub cause_fact_id: Option<u64>,
    pub scheduled_turn: TurnId,
    pub effects: Vec<Effect>,
    pub seen_by: Vec<EntityId>,
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
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldView {
    pub clock_min: ClockMin,
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
}
impl WorldView {
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

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::init(Connection::open(path)?)
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
        if crate::normalize::name(&adventure_seed.pc.name) == "world" {
            return Err(StoreError::Invalid(
                "world is a reserved adventure seed subject".into(),
            ));
        }
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO campaign(adventure_seed_json,clock_min,seed) VALUES(?1,0,?2)",
            params![serde_json::to_string(adventure_seed)?, rng_seed as i64],
        )?;
        let campaign = tx.last_insert_rowid() as u64;
        let rec = TurnRecord {
            campaign_id: campaign,
            op_id: "__seed__".into(),
            kind: TurnKind::Setup,
            intent: String::new(),
            proposal_json: "{}".into(),
            clock_before: 0,
            clock_after: 0,
        };
        let turn = insert_turn(&tx, &rec)?;
        let mut names = std::collections::HashMap::new();
        let pc = project::append(
            &tx,
            campaign,
            turn,
            &LedgerOp::AddEntity {
                kind: EntityKind::Pc,
                name: adventure_seed.pc.name.clone(),
                adult: Some(adventure_seed.pc.adult),
            },
        )?
        .ok_or_else(|| StoreError::Invalid("missing PC id".into()))?;
        names.insert(crate::normalize::name(&adventure_seed.pc.name), pc);
        for e in &adventure_seed.entities {
            let name = crate::normalize::name(&e.name);
            if name == "world" || names.contains_key(&name) {
                return Err(StoreError::Invalid(
                    "duplicate or reserved adventure seed entity name".into(),
                ));
            }
            let id = project::append(
                &tx,
                campaign,
                turn,
                &LedgerOp::AddEntity {
                    kind: e.kind,
                    name: e.name.clone(),
                    adult: e.adult,
                },
            )?
            .ok_or_else(|| StoreError::Invalid("missing entity id".into()))?;
            names.insert(name, id);
        }
        for (facts, canon) in [
            (&adventure_seed.decided, Canon::Persistent),
            (&adventure_seed.proposed, Canon::Possible),
        ] {
            for f in facts {
                let normalized = crate::normalize::name(&f.subject);
                let subject = if let Some(id) = names.get(&normalized) {
                    *id
                } else if normalized == "world" {
                    let id = project::append(
                        &tx,
                        campaign,
                        turn,
                        &LedgerOp::AddEntity {
                            kind: EntityKind::Place,
                            name: f.subject.clone(),
                            adult: None,
                        },
                    )?
                    .ok_or_else(|| StoreError::Invalid("missing subject id".into()))?;
                    names.insert(normalized, id);
                    id
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
                        canon,
                        source: FactSource::AdventureSeed,
                        seen_by: vec![],
                        causes: vec![],
                    },
                )?;
            }
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
        let turn = insert_turn(&tx, &rec)?;
        let revert = ops
            .iter()
            .any(|op| matches!(op, LedgerOp::RevertTurn { .. }));
        if revert {
            for op in &ops {
                if matches!(op, LedgerOp::RevertTurn { .. }) {
                    project::append(&tx, rec.campaign_id, turn, op)?;
                }
            }
            project::rebuild(&tx, rec.campaign_id)?;
        }
        for op in &ops {
            if matches!(op, LedgerOp::RevertTurn { .. }) {
                continue;
            }
            let id = project::append(&tx, rec.campaign_id, turn, op)?;
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
                    let explicit = ops.iter().any(|candidate| matches!(candidate, LedgerOp::Know { holder: h, fact_id: Some(f), claim_id: None, how: Knowing::Witnessed, .. } if h == holder && *f == fact_id));
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
    pub fn log_debug(&self, e: DebugEntry) -> Result<(), StoreError> {
        self.db.execute(
            "INSERT INTO debug_log(op_id,step,prompt,raw,ms) VALUES(?1,?2,?3,?4,?5)",
            params![e.op_id, e.step, e.prompt, e.raw, e.ms],
        )?;
        Ok(())
    }
}
fn insert_turn(db: &Connection, r: &TurnRecord) -> Result<TurnId, StoreError> {
    let kind = project::enum_text(&r.kind)?;
    db.execute("INSERT INTO turn(campaign_id,seq,op_id,kind,intent,proposal_json,clock_before,clock_after) SELECT id,COALESCE((SELECT MAX(seq)+1 FROM turn WHERE campaign_id=?1),1),?2,?3,?4,?5,?6,?7 FROM campaign WHERE id=?1", params![r.campaign_id,r.op_id,kind,r.intent,r.proposal_json,r.clock_before,r.clock_after])?;
    if db.changes() == 0 {
        return Err(StoreError::Invalid("unknown campaign".into()));
    }
    Ok(db.last_insert_rowid() as u64)
}
