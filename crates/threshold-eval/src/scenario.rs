//! The scenario files in `evals/scenarios/<name>.toml`.

use std::{fs, path::Path};

use serde::Deserialize;
use serde::de::DeserializeOwned;

fn default_fixture() -> String {
    "town".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Latency {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub intents: Vec<String>,
}

/// Intents that invite dialogue, for the speech mark (ADR 0006).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Speech {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub intents: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Memory {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub total_turns: usize,
    pub plant_turn: usize,
    pub plant: String,
    pub probe: String,
    pub detail: String,
    pub expect: Vec<String>,
    pub filler: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Voices {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub lines: Vec<VoiceLine>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceLine {
    pub speaker: String,
    pub intent: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shortcuts {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub traps: Vec<Trap>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trap {
    /// The Commit check, as `CheckKind` prints it ("npc folds").
    pub shortcut: String,
    /// What taking the Shortcut looks like, for the judge.
    pub taken: String,
    pub intents: Vec<String>,
    /// Slots (`[subject, key]`) the Shortcut would change: coincidence and
    /// fact bends read them from the ledger after the Turn.
    #[serde(default)]
    pub slots: Vec<[String; 2]>,
    /// The NPC whose stance npc folds reads.
    pub npc: Option<String>,
    pub event: Option<TrapEvent>,
}

/// A World Event committed as a Setup Turn before the trap Turn.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrapEvent {
    pub actor: String,
    pub text: String,
    pub in_minutes: u64,
    pub subject: String,
    pub key: String,
    pub value: String,
    /// NPCs who witness the event when it fires.
    #[serde(default)]
    pub seen_by: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgencyLeaks {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub secrets: Vec<String>,
    pub secret_tokens: Vec<String>,
    pub intents: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offscreen {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    /// The consequence, seeded as a Setup Turn so the engine path is
    /// measured whatever the model schedules.
    pub event: TrapEvent,
    pub provoke: String,
    pub away: String,
    pub back: String,
    pub ask: String,
}

/// Play basics (QA, 2026-11-10): one campaign per run plays each step in
/// order. The pending item is committed as a Setup Turn before play, held by
/// the PC.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayBasics {
    #[serde(default = "default_fixture")]
    pub fixture: String,
    pub pending: Pending,
    pub steps: Vec<BasicStep>,
}

/// An Open Item: a fact on a `pending_` key, witnessed by the PC.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub subject: String,
    pub key: String,
    pub value: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasicStep {
    pub kind: BasicKind,
    pub intent: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasicKind {
    /// The PC goes elsewhere: the scene place must change.
    Move,
    /// The world prevents it: the Narration must tell a failure.
    Impossible,
    /// An opposed contest with an NPC: the Turn must have a Roll.
    Contest,
    /// The PC accepts the pending item: it must close.
    Accept,
    /// The PC heads for a place that is not an entity (ADR 0007): the
    /// Turn commits, the scene place stays, and the Narration must tell
    /// that the PC does not arrive.
    UnknownMove,
}

/// A Seed Draft and a World Build from a Source Text, scored against a
/// hand-made list. A relative path starts at the repository root; the
/// command line can name files outside it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldBuild {
    pub source: String,
    pub expected: String,
}

pub fn load<T: DeserializeOwned>(root: &Path, name: &str) -> Result<T, String> {
    let path = root.join("evals/scenarios").join(format!("{name}.toml"));
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
