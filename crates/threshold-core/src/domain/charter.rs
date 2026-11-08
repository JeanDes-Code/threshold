use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Charter {
    pub premise: String,
    pub pc: PcSeed,
    pub boundaries: Vec<String>,
    pub decided: Vec<CharterFact>,
    pub proposed: Vec<CharterFact>,
    pub story_language: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PcSeed {
    pub name: String,
    pub adult: bool,
    pub concept: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CharterFact {
    pub subject: String,
    pub key: String,
    pub value: String,
    pub text: String,
}
