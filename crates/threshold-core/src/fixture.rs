//! Reads a fixture world from TOML data files.
//!
//! A fixture is a folder with two files:
//! - `adventure-seed.toml`: an `AdventureSeed` (premise, PC, entities,
//!   boundaries, decided and proposed facts). `turn::create_campaign` turns
//!   it into the campaign's first Setup Turn.
//! - `world.toml`: a `WorldSetup`, what an Adventure Seed cannot express:
//!   hidden facts, attitudes, goals, Voice Cards, open obstacles and World
//!   Events. `setup::commit_world` commits it as one more Setup Turn.
//!   Entities are named, never numbered. A fact's `known_by` names its
//!   holders (empty means GM only). An `open` fact is an open item (ADR
//!   0005): it commits on the `pending_` key. An obstacle names the live fact it
//!   stands on by subject and key. An event is a World Event due `at`
//!   minutes after the campaign starts; `seen_by` names the NPCs who witness
//!   it (empty means GM only).
//!
//! ```toml
//! [[facts]]
//! subject = "Orsel"
//! key = "skims"
//! value = "the tithe"
//! text = "Orsel skims the tithe."
//! known_by = ["Doul"]
//!
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
//!
//! [[events]]
//! at = 90
//! actor = "Orsel"
//! text = "Orsel ends the session and summons the scribe"
//! effects = [{ subject = "Wren", key = "standing", value = "summoned" }]
//! seen_by = ["Doul"]
//! ```

use std::{fmt, fs, path::Path};

use crate::{
    domain::{AdventureSeed, WorldSetup},
    setup::{SetupError, commit_world},
    store::{CampaignId, Store, StoreError},
};

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

/// Reads an `adventure-seed.toml` file.
pub fn load_adventure_seed(path: &Path) -> Result<AdventureSeed, FixtureError> {
    toml::from_str(&read(path)?)
        .map_err(|e| FixtureError::Invalid(format!("{}: {e}", path.display())))
}

/// Reads a `world.toml` file.
pub fn load_world_setup(path: &Path) -> Result<WorldSetup, FixtureError> {
    toml::from_str(&read(path)?)
        .map_err(|e| FixtureError::Invalid(format!("{}: {e}", path.display())))
}

/// Reads the `world.toml` at `path` and commits it into `campaign` through
/// `setup::commit_world`. Loading twice fails and commits nothing.
pub fn load_world(
    store: &mut Store,
    campaign: CampaignId,
    path: &Path,
) -> Result<(), FixtureError> {
    let world = load_world_setup(path)?;
    commit_world(store, campaign, &world).map_err(|e| match e {
        SetupError::Io(message) => FixtureError::Io(message),
        SetupError::Invalid(message) => {
            FixtureError::Invalid(format!("{}: {message}", path.display()))
        }
        SetupError::Store(e) => FixtureError::Store(e),
    })
}

fn read(path: &Path) -> Result<String, FixtureError> {
    fs::read_to_string(path).map_err(|e| FixtureError::Io(format!("{}: {e}", path.display())))
}
