pub mod checks;
pub mod context;
pub mod domain;
pub mod fixture;
pub mod model;
pub mod normalize;
pub mod pack;
pub mod rules;
pub mod setup;
pub mod store;
pub mod turn;
pub mod world;

pub use model::{CallKind, Gm, GmError, Prompt, RecordedCall, RecordedGm};

use domain::DecisionProposal;

pub fn proposal_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(DecisionProposal))
        .expect("DecisionProposal schema must serialize")
}

#[derive(Clone, Debug, Default)]
pub struct HandleContext {
    pub entities: Vec<domain::Handle>,
    pub facts: Vec<domain::Handle>,
    pub events: Vec<domain::Handle>,
    pub goals: Vec<domain::Handle>,
    pub new_slots: u8,
}

pub fn proposal_schema_for(ctx: &HandleContext) -> serde_json::Value {
    use domain::Handle;
    use serde_json::json;

    let mut schema = proposal_schema();
    schema["additionalProperties"] = json!(false);
    for def in schema["$defs"].as_object_mut().unwrap().values_mut() {
        if def.get("type") == Some(&json!("object")) {
            def["additionalProperties"] = json!(false);
        }
    }
    let entities: Vec<String> = ctx
        .entities
        .iter()
        .filter(|h| matches!(h, Handle::Entity(_)))
        .map(ToString::to_string)
        .chain((1..=ctx.new_slots).map(|i| format!("N{i}")))
        .collect();
    let new: Vec<String> = (1..=ctx.new_slots).map(|i| format!("N{i}")).collect();
    let facts: Vec<String> = ctx
        .facts
        .iter()
        .filter(|h| matches!(h, Handle::Fact(_)))
        .map(ToString::to_string)
        .collect();
    let events: Vec<String> = ctx
        .events
        .iter()
        .filter(|h| matches!(h, Handle::WorldEvent(_)))
        .map(ToString::to_string)
        .collect();
    let goals: Vec<String> = ctx
        .goals
        .iter()
        .filter(|h| matches!(h, Handle::Goal(_)))
        .map(ToString::to_string)
        .collect();

    fn set(schema: &mut serde_json::Value, path: &str, values: &[String]) {
        *schema
            .pointer_mut(path)
            .expect("proposal schema property exists") = json!({"type":"string", "enum":values});
    }
    fn omit(schema: &mut serde_json::Value, path: &str, key: &str) {
        schema
            .pointer_mut(path)
            .expect("proposal schema properties exist")
            .as_object_mut()
            .unwrap()
            .remove(key);
    }
    if !entities.is_empty() {
        for path in [
            "/$defs/Reaction/properties/npc",
            "/$defs/SlotDraft/properties/subject",
            "/$defs/CheckDraft/properties/against",
            "/$defs/NewFact/properties/said_by",
            "/$defs/EventDraft/properties/actor",
            "/$defs/EventDraft/properties/seen_by/items",
        ] {
            set(&mut schema, path, &entities);
        }
    } else {
        for key in ["check", "reactions", "facts", "schedule"] {
            omit(&mut schema, "/properties", key);
        }
    }
    if new.is_empty() {
        omit(&mut schema, "/properties", "new_npcs");
    } else {
        set(&mut schema, "/$defs/NewNpc/properties/id", &new);
    }
    if facts.is_empty() {
        schema["$defs"]["Cause"]["oneOf"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        omit(&mut schema, "/properties", "blockers");
        schema["$defs"]["CheckDraft"]["properties"]["factors"]["maxItems"] = json!(0);
        schema["$defs"]["NewFact"]["properties"]["because"]["maxItems"] = json!(0);
        omit(&mut schema, "/properties", "cancel");
    } else {
        set(&mut schema, "/properties/blockers/items", &facts);
        set(&mut schema, "/$defs/Factor/properties/fact", &facts);
        set(&mut schema, "/$defs/Cause/oneOf/1/properties/fact", &facts);
        set(
            &mut schema,
            "/$defs/NewFact/properties/because/items",
            &facts,
        );
        set(&mut schema, "/$defs/Cancel/properties/because", &facts);
    }
    if events.is_empty() {
        omit(&mut schema, "/properties", "cancel");
    } else {
        set(&mut schema, "/$defs/Cancel/properties/event", &events);
    }
    if goals.is_empty() {
        omit(&mut schema, "/$defs/EventDraft/properties", "goal");
    } else {
        set(&mut schema, "/$defs/EventDraft/properties/goal", &goals);
    }
    schema
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeError {
    MissingCheck,
    UnexpectedCheck,
}

impl std::fmt::Display for ShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MissingCheck => "verdict is check but check is missing",
            Self::UnexpectedCheck => "check is present but verdict is not check",
        })
    }
}
impl std::error::Error for ShapeError {}

pub fn validate_shape(proposal: &DecisionProposal) -> Result<(), ShapeError> {
    match (
        proposal.verdict == domain::Verdict::Check,
        proposal.check.is_some(),
    ) {
        (true, false) => Err(ShapeError::MissingCheck),
        (false, true) => Err(ShapeError::UnexpectedCheck),
        _ => Ok(()),
    }
}
