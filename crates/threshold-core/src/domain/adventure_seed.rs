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
    pub adult: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PcSeed {
    pub name: String,
    pub adult: bool,
    pub concept: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SeedFact {
    pub subject: String,
    pub key: String,
    pub value: String,
    pub text: String,
}
