//! Loads a fixture world from TOML data files.
//!
//! A fixture is a folder with two files:
//! - `adventure-seed.toml`: an `AdventureSeed` (premise, PC, entities,
//!   boundaries, decided and proposed facts). `turn::create_campaign` turns
//!   it into the campaign's first Setup Turn.
//! - `world.toml`: what an Adventure Seed cannot express: attitudes, goals,
//!   Voice Cards and open obstacles. `load_world` commits it as one more
//!   Setup Turn. Entities are named, never numbered; an obstacle names the
//!   live fact it stands on by subject and key.
//!
//! ```toml
//! [[attitudes]]
//! from = "Orsel"
//! to = "Wren"
//! attitude = -1
//!
//! [[goals]]
//! npc = "Orsel"
//! text = "Protect his reputation"
//! # against = "Wren"
//!
//! [[voice_cards]]
//! npc = "Orsel"
//! habits = "Formal, never raises his voice"
//! vocabulary = "statute, writ, in accordance with"
//! samples = ["The law is patient."]
//!
//! [[obstacles]]
//! subject = "Tithe archive"
//! key = "condition"
//! ```

use std::{fmt, fs, path::Path};

use serde::Deserialize;

use crate::{
    domain::{AdventureSeed, Canon, EntityId, EntityRef, LedgerOp},
    normalize,
    store::{CampaignId, CommitError, Store, StoreError, TurnKind, TurnRecord, WorldView},
};

/// The `op_id` of the Setup Turn that `load_world` commits.
pub const WORLD_OP_ID: &str = "__world__";

#[derive(Debug)]
pub enum FixtureError {
    Io(String),
    Invalid(String),
    Store(StoreError),
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) | Self::Invalid(message) => f.write_str(message),
            Self::Store(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for FixtureError {}

impl From<StoreError> for FixtureError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldFile {
    #[serde(default)]
    attitudes: Vec<AttitudeEntry>,
    #[serde(default)]
    goals: Vec<GoalEntry>,
    #[serde(default)]
    voice_cards: Vec<VoiceCardEntry>,
    #[serde(default)]
    obstacles: Vec<ObstacleEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttitudeEntry {
    from: String,
    to: String,
    attitude: i8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GoalEntry {
    npc: String,
    text: String,
    against: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VoiceCardEntry {
    npc: String,
    habits: String,
    vocabulary: String,
    samples: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObstacleEntry {
    subject: String,
    key: String,
}

/// Reads an `adventure-seed.toml` file.
pub fn load_adventure_seed(path: &Path) -> Result<AdventureSeed, FixtureError> {
    toml::from_str(&read(path)?)
        .map_err(|e| FixtureError::Invalid(format!("{}: {e}", path.display())))
}

/// Commits the `world.toml` at `path` into `campaign` as one Setup Turn:
/// attitudes, goals, Voice Cards, then obstacles. Every name must match an
/// entity of the campaign. Loading twice fails and commits nothing.
pub fn load_world(
    store: &mut Store,
    campaign: CampaignId,
    path: &Path,
) -> Result<(), FixtureError> {
    let invalid = |message: String| FixtureError::Invalid(format!("{}: {message}", path.display()));
    let world: WorldFile = toml::from_str(&read(path)?).map_err(|e| invalid(e.to_string()))?;
    let view = store.snapshot(campaign)?;
    let id = |name: &str| entity(&view, name).ok_or_else(|| invalid(format!("no entity {name}")));

    let mut ops = Vec::new();
    for a in &world.attitudes {
        if !(-5..=5).contains(&a.attitude) {
            return Err(invalid(format!(
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
            .ok_or_else(|| invalid(format!("no live fact {} of {}", o.key, o.subject)))?;
        ops.push(LedgerOp::OpenObstacle { fact_id: fact.id });
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
        Err(CommitError::DuplicateOp(_)) => Err(invalid("the world is already loaded".into())),
        Err(CommitError::Store(e)) => Err(e.into()),
    }
}

fn entity(view: &WorldView, name: &str) -> Option<EntityId> {
    let name = normalize::name(name);
    view.entities
        .iter()
        .find(|e| e.norm_name == name)
        .map(|e| e.id)
}

fn read(path: &Path) -> Result<String, FixtureError> {
    fs::read_to_string(path).map_err(|e| FixtureError::Io(format!("{}: {e}", path.display())))
}
