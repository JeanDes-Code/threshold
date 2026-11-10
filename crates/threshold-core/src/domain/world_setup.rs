use serde::{Deserialize, Serialize};

/// What an Adventure Seed cannot express: hidden facts, attitudes, goals,
/// Voice Cards, open obstacles and World Events. `setup::commit_world`
/// commits it as the `__world__` Setup Turn. Entries name entities by a main
/// name or an other name, never by id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldSetup {
    #[serde(default)]
    pub facts: Vec<FactEntry>,
    #[serde(default)]
    pub attitudes: Vec<AttitudeEntry>,
    #[serde(default)]
    pub goals: Vec<GoalEntry>,
    #[serde(default)]
    pub voice_cards: Vec<VoiceCardEntry>,
    #[serde(default)]
    pub obstacles: Vec<ObstacleEntry>,
    #[serde(default)]
    pub events: Vec<EventEntry>,
}

/// A fact of the hidden world.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactEntry {
    pub subject: String,
    pub key: String,
    pub value: String,
    pub text: String,
    /// The characters who hold the fact. Empty means GM only.
    #[serde(default)]
    pub known_by: Vec<String>,
    /// An open item of the Source Text (ADR 0005).
    #[serde(default)]
    pub open: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttitudeEntry {
    pub from: String,
    pub to: String,
    pub attitude: i8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalEntry {
    pub npc: String,
    pub text: String,
    pub against: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceCardEntry {
    pub npc: String,
    pub habits: String,
    pub vocabulary: String,
    pub samples: Vec<String>,
}

/// An open obstacle on the live fact `(subject, key)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObstacleEntry {
    pub subject: String,
    pub key: String,
}

/// A World Event due `at` minutes after the campaign starts. `seen_by`
/// names the characters who witness it; empty means GM only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventEntry {
    /// Minutes from the campaign's start.
    pub at: u64,
    pub actor: String,
    pub text: String,
    #[serde(default)]
    pub effects: Vec<EffectEntry>,
    #[serde(default)]
    pub seen_by: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectEntry {
    pub subject: String,
    pub key: String,
    pub value: String,
}
