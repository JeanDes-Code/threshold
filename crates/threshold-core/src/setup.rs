//! Turns names into ledger ops, and writes Campaign files.
//!
//! A `WorldSetup` names entities, never ids. `commit_world` resolves each
//! name (main or other) against the campaign and commits the world as one
//! Setup Turn. `create_campaign_file` is the only way to write a Campaign
//! file: it builds the file under a `.part` name and renames it once
//! complete, so a failed setup leaves no file.

use std::{
    ffi::OsString,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use crate::{
    domain::{
        AdventureSeed, Canon, Effect, EntityRef, FactSource, LedgerOp, OPEN_KEY_PREFIX, Slot,
        Trigger, WorldSetup, is_open_key,
    },
    normalize,
    store::{CampaignId, CommitError, Store, StoreError, TurnKind, TurnRecord},
    turn::create_campaign,
};

/// The `op_id` of the Setup Turn that `commit_world` commits.
pub const WORLD_OP_ID: &str = "__world__";

#[derive(Debug)]
pub enum SetupError {
    Io(String),
    /// The world names an unknown entity or fact, or breaks a range.
    Invalid(String),
    Store(StoreError),
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) | Self::Invalid(message) => f.write_str(message),
            Self::Store(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for SetupError {}

impl From<StoreError> for SetupError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

/// Commits `world` into `campaign` as one Setup Turn: facts, attitudes,
/// goals, Voice Cards, obstacles, then World Events (the same
/// `ScheduleEvent` op a proposal's `schedule` commits). Every name must
/// match an entity of the campaign. On any error, and on a second call,
/// nothing is committed.
pub fn commit_world(
    store: &mut Store,
    campaign: CampaignId,
    world: &WorldSetup,
) -> Result<(), SetupError> {
    let view = store.snapshot(campaign)?;
    let id = |name: &str| {
        view.entity_by_name(name)
            .map(|e| e.id)
            .ok_or_else(|| SetupError::Invalid(format!("no entity {name}")))
    };
    let ids = |names: &[String]| {
        names
            .iter()
            .map(|n| id(n).map(EntityRef::Id))
            .collect::<Result<Vec<_>, _>>()
    };

    let mut ops = Vec::new();
    // ADR 0005: an open entry commits on the `pending_` key, so its outcome
    // slot stays empty. A settled entry may not use that prefix.
    for f in &world.facts {
        let key = normalize::key(&f.key);
        let key = if f.open {
            format!("{OPEN_KEY_PREFIX}{key}")
        } else if is_open_key(&key) {
            return Err(SetupError::Invalid(format!(
                "{key} of {} starts with {OPEN_KEY_PREFIX}, but the entry is not open",
                f.subject
            )));
        } else {
            key
        };
        ops.push(LedgerOp::AddFact {
            slot: Slot {
                subject: EntityRef::Id(id(&f.subject)?),
                key,
                value: normalize::value(&f.value),
            },
            text: f.text.clone(),
            canon: Canon::Persistent,
            source: FactSource::AdventureSeed,
            seen_by: ids(&f.known_by)?,
            causes: vec![],
        });
    }
    for a in &world.attitudes {
        if !(-5..=5).contains(&a.attitude) {
            return Err(SetupError::Invalid(format!(
                "attitude {} is outside -5..=5",
                a.attitude
            )));
        }
        ops.push(LedgerOp::ShiftAttitude {
            from: EntityRef::Id(id(&a.from)?),
            to: EntityRef::Id(id(&a.to)?),
            delta: a.attitude,
        });
    }
    for g in &world.goals {
        ops.push(LedgerOp::AddGoal {
            npc: EntityRef::Id(id(&g.npc)?),
            text: g.text.clone(),
            against: g.against.as_deref().map(id).transpose()?.map(EntityRef::Id),
        });
    }
    for v in &world.voice_cards {
        ops.push(LedgerOp::AddVoiceCard {
            npc: EntityRef::Id(id(&v.npc)?),
            habits: v.habits.clone(),
            vocabulary: v.vocabulary.clone(),
            samples: v.samples.clone(),
        });
    }
    for o in &world.obstacles {
        let subject = id(&o.subject)?;
        let fact = view
            .live_fact(subject, &o.key)
            .filter(|f| f.canon != Canon::Possible)
            .ok_or_else(|| {
                SetupError::Invalid(format!("no live fact {} of {}", o.key, o.subject))
            })?;
        ops.push(LedgerOp::OpenObstacle { fact_id: fact.id });
    }
    for ev in &world.events {
        let mut effects = Vec::new();
        for e in &ev.effects {
            effects.push(Effect::SetFact {
                slot: Slot {
                    subject: EntityRef::Id(id(&e.subject)?),
                    key: normalize::key(&e.key),
                    value: normalize::value(&e.value),
                },
                text: format!("{}: {} = {}", e.subject, e.key, e.value),
                seen_by: Vec::new(),
            });
        }
        ops.push(LedgerOp::ScheduleEvent {
            text: ev.text.clone(),
            trigger: Trigger::At(ev.at),
            actors: vec![EntityRef::Id(id(&ev.actor)?)],
            goal_id: None,
            cause_fact_id: None,
            effects,
            seen_by: ids(&ev.seen_by)?,
        });
    }

    let record = TurnRecord {
        campaign_id: campaign,
        op_id: WORLD_OP_ID.into(),
        kind: TurnKind::Setup,
        intent: String::new(),
        proposal_json: "{}".into(),
        clock_before: view.clock_min,
        clock_after: view.clock_min,
    };
    match store.commit(record, ops) {
        Ok(_) => Ok(()),
        Err(CommitError::DuplicateOp(_)) => {
            Err(SetupError::Invalid("the world is already loaded".into()))
        }
        Err(CommitError::Store(e)) => Err(e.into()),
    }
}

/// Writes a new Campaign file at `path`: the Setup Turn of `seed`, then the
/// Setup Turn of `world`. The file is built as `<path>.part` and renamed to
/// `path` once the store is closed. On any error no file stays behind,
/// neither the `.part` file nor an SQLite side file. An existing file at
/// `path` is refused, never overwritten. A `.part` file left by an earlier
/// failure is replaced.
pub fn create_campaign_file(
    path: &Path,
    seed: &AdventureSeed,
    world: &WorldSetup,
    rng_seed: u64,
) -> Result<(), SetupError> {
    let io = |e: io::Error| SetupError::Io(format!("{}: {e}", path.display()));
    if path.try_exists().map_err(io)? {
        return Err(SetupError::Io(format!("{} already exists", path.display())));
    }
    let part = with_suffix(path, ".part");
    remove_part(&part);
    let written =
        write_part(&part, seed, world, rng_seed).and_then(|()| fs::rename(&part, path).map_err(io));
    if written.is_err() {
        remove_part(&part);
    }
    written
}

/// Writes both Setup Turns into a new store at `part`. The store is closed
/// when this returns.
fn write_part(
    part: &Path,
    seed: &AdventureSeed,
    world: &WorldSetup,
    rng_seed: u64,
) -> Result<(), SetupError> {
    let mut store = Store::open(part)?;
    let campaign = create_campaign(&mut store, seed, rng_seed)?;
    commit_world(&mut store, campaign, world)
}

/// `path` with `suffix` added to its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

/// Removes a `.part` file and the side files SQLite may keep next to it.
fn remove_part(part: &Path) {
    for side in ["", "-journal", "-wal", "-shm"] {
        let _ = fs::remove_file(with_suffix(part, side));
    }
}
