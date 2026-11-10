//! Notes and Pins: the Player's own marks on the Journal.
//!
//! They live in two tables of the Campaign file, outside the ledger. No
//! prompt and no Commit reads them. `store::migrations` creates the tables
//! from `SCHEMA` and never reads or writes them. `PlayerStore` has its own
//! connection, which the shell owns next to the `Engine`; the `Engine` never
//! holds it (data shape 3.7).

use std::{fmt, path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{domain::EntityId, normalize, store::TurnId};

/// The Player tables. A Note and a Pin store their `Target` as JSON. One Pin
/// per target: the target is the key of `player_pin`.
pub const SCHEMA: &str = "\
CREATE TABLE player_note (id INTEGER PRIMARY KEY, body TEXT NOT NULL, about_json TEXT, created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL);
CREATE TABLE player_pin (target_json TEXT PRIMARY KEY);
";

/// The longest Note body, in characters (not bytes): an emoji counts as one.
pub const NOTE_MAX_CHARS: usize = 2_000;

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

impl Target {
    /// The one stored form of a target. A Belief key goes through
    /// `normalize::key`, as the store writes fact keys, so "Door" and "door"
    /// are one target and one Pin.
    pub fn normalized(&self) -> Target {
        match self {
            Self::Belief { subject_id, key } => Self::Belief {
                subject_id: *subject_id,
                key: normalize::key(key),
            },
            other => other.clone(),
        }
    }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerError {
    /// The body is empty or holds only white space.
    Empty,
    TooLong {
        max: usize,
    },
    /// A Note cannot be about a Note.
    AboutANote,
    NoSuchNote(u64),
    Store(String),
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("a note needs some text"),
            Self::TooLong { max } => write!(f, "a note holds at most {max} characters"),
            Self::AboutANote => f.write_str("a note cannot be about another note"),
            Self::NoSuchNote(id) => write!(f, "there is no note {id}"),
            Self::Store(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for PlayerError {}

impl From<rusqlite::Error> for PlayerError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(e.to_string())
    }
}

impl From<serde_json::Error> for PlayerError {
    fn from(e: serde_json::Error) -> Self {
        Self::Store(e.to_string())
    }
}

/// The Notes and Pins of one Campaign file, on a connection of their own.
/// The shell's `Session` lock orders its writes with the Turn's.
pub struct PlayerStore {
    db: Connection,
}

impl PlayerStore {
    /// Opens the Player tables of a Campaign file that already exists. It
    /// creates nothing: a file without the tables is refused.
    pub fn open(path: &Path) -> Result<Self, PlayerError> {
        if !path.is_file() {
            return Err(PlayerError::Store(format!("no file at {}", path.display())));
        }
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        db.busy_timeout(Duration::from_secs(5))?;
        let tables: u32 = db.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('player_note','player_pin')",
            [],
            |r| r.get(0),
        )?;
        if tables != 2 {
            return Err(PlayerError::Store(format!(
                "{} has no Player tables",
                path.display()
            )));
        }
        Ok(Self { db })
    }

    /// Every Note, oldest first, and every Pin, in the order it was set.
    pub fn marks(&self) -> Result<Marks, PlayerError> {
        let mut stmt = self.db.prepare(
            "SELECT id,body,about_json,created_ms,updated_ms FROM player_note ORDER BY id",
        )?;
        let notes = stmt
            .query_map([], read_note)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(note, about)| {
                Ok(Note {
                    about: parse(about)?,
                    ..note
                })
            })
            .collect::<Result<Vec<_>, PlayerError>>()?;
        let mut stmt = self
            .db
            .prepare("SELECT target_json FROM player_pin ORDER BY rowid")?;
        let pins = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .map(|json| Ok(serde_json::from_str(json)?))
            .collect::<Result<Vec<_>, PlayerError>>()?;
        Ok(Marks { notes, pins })
    }

    /// A new Note. Nothing is written when the body or the target is refused.
    pub fn add_note(
        &self,
        body: &str,
        about: Option<Target>,
        now_ms: u64,
    ) -> Result<Note, PlayerError> {
        let body = checked_body(body)?;
        if matches!(about, Some(Target::Note { .. })) {
            return Err(PlayerError::AboutANote);
        }
        let about = about.map(|t| t.normalized());
        let about_json = about.as_ref().map(serde_json::to_string).transpose()?;
        self.db.execute(
            "INSERT INTO player_note(body,about_json,created_ms,updated_ms) VALUES(?1,?2,?3,?3)",
            params![body, about_json, now_ms],
        )?;
        Ok(Note {
            id: self.db.last_insert_rowid() as u64,
            body: body.to_owned(),
            about,
            created_ms: now_ms,
            updated_ms: now_ms,
        })
    }

    /// A new body for a Note. What it is about does not change.
    pub fn edit_note(&self, id: u64, body: &str, now_ms: u64) -> Result<Note, PlayerError> {
        let body = checked_body(body)?;
        let changed = self.db.execute(
            "UPDATE player_note SET body=?1, updated_ms=?2 WHERE id=?3",
            params![body, now_ms, id],
        )?;
        if changed == 0 {
            return Err(PlayerError::NoSuchNote(id));
        }
        self.note(id)?.ok_or(PlayerError::NoSuchNote(id))
    }

    /// Removes a Note and its Pin.
    pub fn delete_note(&self, id: u64) -> Result<(), PlayerError> {
        let pin = serde_json::to_string(&Target::Note { note_id: id })?;
        let tx = self.db.unchecked_transaction()?;
        if tx.execute("DELETE FROM player_note WHERE id=?1", [id])? == 0 {
            return Err(PlayerError::NoSuchNote(id));
        }
        tx.execute("DELETE FROM player_pin WHERE target_json=?1", [pin])?;
        tx.commit()?;
        Ok(())
    }

    /// Pins `target`, or removes its Pin. A Pin on a Note needs the Note.
    pub fn set_pin(&self, target: &Target, on: bool) -> Result<(), PlayerError> {
        let target = target.normalized();
        let json = serde_json::to_string(&target)?;
        if !on {
            self.db
                .execute("DELETE FROM player_pin WHERE target_json=?1", [json])?;
            return Ok(());
        }
        if let Target::Note { note_id } = target
            && self.note(note_id)?.is_none()
        {
            return Err(PlayerError::NoSuchNote(note_id));
        }
        self.db.execute(
            "INSERT OR IGNORE INTO player_pin(target_json) VALUES(?1)",
            [json],
        )?;
        Ok(())
    }

    fn note(&self, id: u64) -> Result<Option<Note>, PlayerError> {
        let row = self
            .db
            .query_row(
                "SELECT id,body,about_json,created_ms,updated_ms FROM player_note WHERE id=?1",
                [id],
                read_note,
            )
            .optional()?;
        row.map(|(note, about)| {
            Ok(Note {
                about: parse(about)?,
                ..note
            })
        })
        .transpose()
    }
}

/// The body as stored: trimmed, not empty, at most `NOTE_MAX_CHARS`.
fn checked_body(body: &str) -> Result<&str, PlayerError> {
    let body = body.trim();
    if body.is_empty() {
        return Err(PlayerError::Empty);
    }
    if body.chars().count() > NOTE_MAX_CHARS {
        return Err(PlayerError::TooLong {
            max: NOTE_MAX_CHARS,
        });
    }
    Ok(body)
}

/// A Note row, its `about` still JSON.
fn read_note(r: &rusqlite::Row<'_>) -> rusqlite::Result<(Note, Option<String>)> {
    Ok((
        Note {
            id: r.get(0)?,
            body: r.get(1)?,
            about: None,
            created_ms: r.get(3)?,
            updated_ms: r.get(4)?,
        },
        r.get(2)?,
    ))
}

fn parse(about: Option<String>) -> Result<Option<Target>, PlayerError> {
    Ok(about.map(|json| serde_json::from_str(&json)).transpose()?)
}
