//! Notes and Pins: the Player's own marks on the Journal.
//!
//! They live in two tables of the Campaign file, outside the ledger. No
//! prompt and no Commit reads them. `store::migrations` creates the tables
//! from `SCHEMA` and never reads or writes them.

use serde::{Deserialize, Serialize};

use crate::{domain::EntityId, store::TurnId};

/// The Player tables. A Note and a Pin store their `Target` as JSON. One Pin
/// per target: the target is the key of `player_pin`.
pub const SCHEMA: &str = "\
CREATE TABLE player_note (id INTEGER PRIMARY KEY, body TEXT NOT NULL, about_json TEXT, created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL);
CREATE TABLE player_pin (target_json TEXT PRIMARY KEY);
";

/// What a Note is about, or what a Pin keeps in view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// The Scene that holds this Turn.
    Scene {
        turn_id: TurnId,
    },
    Act {
        turn_id: TurnId,
    },
    Belief {
        subject_id: EntityId,
        key: String,
    },
    Entity {
        entity_id: EntityId,
    },
    Note {
        note_id: u64,
    },
}

/// Text the Player writes in the Journal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    pub body: String,
    pub about: Option<Target>,
    pub created_ms: u64,
    pub updated_ms: u64,
}

/// Every Note and Pin of a Campaign file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    pub notes: Vec<Note>,
    pub pins: Vec<Target>,
}
