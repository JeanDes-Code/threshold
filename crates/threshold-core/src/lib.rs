pub mod build;
pub mod checks;
pub mod context;
pub mod domain;
pub mod fixture;
pub mod model;
pub mod normalize;
pub mod pack;
pub mod player;
pub mod rules;
pub mod setup;
pub mod speech;
pub mod store;
pub mod turn;
pub mod view;
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
    /// The places the PC knows (`world::known_places`): the values of
    /// `move_to` besides `null`.
    pub places: Vec<domain::Handle>,
    /// The Open Items the Intent names (`world::items_named_by`), as
    /// `world::open_item_ref` names them: the values of `settles.item`
    /// besides `none`.
    pub open_items: Vec<String>,
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
    // ADR 0007: the fields the code needs are required and enum-typed.
    let places: Vec<serde_json::Value> = ctx
        .places
        .iter()
        .filter(|h| matches!(h, Handle::Entity(_)))
        .map(|h| json!(h.to_string()))
        .chain([serde_json::Value::Null])
        .collect();
    schema["properties"]["move_to"] = json!({"type": ["string", "null"], "enum": places});
    require(&mut schema, "", "move_to");
    // One choice: `none` or one Open Item the Intent names, and its outcome
    // (bug pass 3). With no item offered, the field is absent.
    if ctx.open_items.is_empty() {
        omit(&mut schema, "/properties", "settles");
    } else {
        let items: Vec<&str> = std::iter::once(domain::NO_ITEM)
            .chain(ctx.open_items.iter().map(String::as_str))
            .collect();
        let outcomes: Vec<&str> = domain::ItemStatus::ALL.iter().map(|s| s.word()).collect();
        schema["properties"]["settles"] = json!({
            "type": "object",
            "properties": {
                "item": {"type": "string", "enum": items},
                "outcome": {"type": "string", "enum": outcomes},
            },
            "required": ["item", "outcome"],
            "additionalProperties": false,
        });
        require(&mut schema, "", "settles");
    }
    // A scheduled event says whether everyone at its place perceives what
    // it changes (review of bug pass 4: the visibility of an effect is
    // data, never read from the event's text).
    if let Some(public) = schema.pointer_mut("/$defs/EventDraft/properties/public") {
        if let Some(public) = public.as_object_mut() {
            public.remove("default");
        }
        require(&mut schema, "/$defs/EventDraft", "public");
    }
    schema["properties"]["contested"] = json!({"type": "boolean"});
    require(&mut schema, "", "contested");
    for stake in ["win", "lose"] {
        schema["$defs"]["CheckDraft"]["properties"][stake] = json!({"$ref": "#/$defs/SlotDraft"});
        require(&mut schema, "/$defs/CheckDraft", stake);
    }
    schema
}

/// Adds `key` to the `required` list of the object schema at `path`.
fn require(schema: &mut serde_json::Value, path: &str, key: &str) {
    let required = schema
        .pointer_mut(path)
        .and_then(serde_json::Value::as_object_mut)
        .expect("the proposal schema has this object")
        .entry("required")
        .or_insert_with(|| serde_json::json!([]));
    if let Some(list) = required.as_array_mut()
        && !list.iter().any(|k| k == key)
    {
        list.push(serde_json::json!(key));
    }
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
