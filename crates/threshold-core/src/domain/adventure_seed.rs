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
