use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AdventureSeed {
    pub premise: String,
    pub pc: PcSeed,
    #[serde(default)]
    pub entities: Vec<SeedEntity>,
    pub boundaries: Vec<String>,
    pub decided: Vec<SeedFact>,
    pub proposed: Vec<SeedFact>,
    pub story_language: String,
    /// Themes the Player agreed to, in a few words each.
    #[serde(default)]
    #[schemars(length(max = 8), inner(length(max = 40)))]
    pub themes: Vec<String>,
    /// How the Player wants the GM to run the game, in the Player's words.
    #[serde(default)]
    #[schemars(length(max = 1200))]
    pub gm_agreement: String,
    /// The resolution agreement, as text. No play prompt reads it: the
    /// Ruleset owns dice (ADR 0002).
    #[serde(default)]
    pub resolution: String,
    /// The lowest age of any character. `checks::boundary` refuses a new
    /// NPC below it.
    #[serde(default)]
    pub min_age: Option<u16>,
    /// Where play starts. Read by the World Build and the opening prompt,
    /// never by a play prompt.
    #[serde(default)]
    pub start: StartSeed,
    /// What the Source Text leaves open. Only the World Build reads it.
    #[serde(default)]
    pub open_details: Vec<OpenDetail>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StartSeed {
    #[serde(default)]
    pub place: String,
    /// The names of the characters present at the start.
    #[serde(default)]
    pub present: Vec<String>,
    #[serde(default)]
    pub situation: String,
    #[serde(default)]
    pub when: String,
}

/// A detail the Source Text leaves open, and who settles it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OpenDetail {
    pub text: String,
    pub mode: OpenMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OpenMode {
    /// The Player leaves it to the GM.
    Delegated,
    /// The PC finds it out in play.
    Discover,
    /// The Player and the GM agree on it before play.
    ToAgree,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SeedEntity {
    pub name: String,
    pub kind: super::EntityKind,
    /// A character's age in years. A PC or an NPC has one; nothing else does.
    pub age: Option<u16>,
    #[serde(default)]
    pub other_names: Vec<OtherName>,
    /// A hidden entity is not seen on arrival: the PC does not witness its
    /// location just by entering its place.
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PcSeed {
    pub name: String,
    pub age: u16,
    #[serde(default)]
    pub other_names: Vec<OtherName>,
    pub concept: String,
}

/// A second name of an entity, and the characters who know that it names the
/// same entity. The bearer always knows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OtherName {
    pub name: String,
    pub known_to: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SeedFact {
    pub subject: String,
    pub key: String,
    pub value: String,
    pub text: String,
}
