use schemars::schema_for;
use threshold_core::{
    domain::{Charter, CheckDraft, DecisionProposal, EventDraft, Handle, NewNpc, Reaction, Stance},
    normalize, proposal_schema,
};

#[test]
fn handle_round_trip() {
    for s in ["E12", "F40", "W3", "G2", "N1"] {
        assert_eq!(Handle::parse(s).unwrap().to_string(), s);
    }
}

#[test]
fn handle_rejects_garbage() {
    for s in ["", "X1", "F", "F-1", "f12"] {
        assert!(Handle::parse(s).is_err());
    }
}

#[test]
fn stance_is_ordered() {
    assert!(Stance::Oppose < Stance::Resist && Stance::Comply < Stance::Help);
}

#[test]
fn schema_caps_lists() {
    let s = proposal_schema();
    assert_eq!(s.pointer("/properties/blockers/maxItems").unwrap(), 2);
    assert_eq!(s.pointer("/properties/reactions/maxItems").unwrap(), 2);
    assert_eq!(s.pointer("/properties/new_npcs/maxItems").unwrap(), 1);
    assert_eq!(s.pointer("/properties/facts/maxItems").unwrap(), 2);
    assert_eq!(s.pointer("/properties/cancel/maxItems").unwrap(), 1);

    let check = serde_json::to_value(schema_for!(CheckDraft)).unwrap();
    assert_eq!(check.pointer("/properties/factors/maxItems").unwrap(), 3);

    let event = serde_json::to_value(schema_for!(EventDraft)).unwrap();
    assert_eq!(event.pointer("/properties/effects/maxItems").unwrap(), 2);
}

#[test]
fn schema_caps_text() {
    let reaction = serde_json::to_value(schema_for!(Reaction)).unwrap();
    assert_eq!(reaction.pointer("/properties/plan/maxLength").unwrap(), 90);

    let npc = serde_json::to_value(schema_for!(NewNpc)).unwrap();
    assert_eq!(npc.pointer("/properties/voice/maxLength").unwrap(), 120);
}

#[test]
fn proposal_minimal_parses() {
    let p: DecisionProposal =
        serde_json::from_str(r#"{"action":"talk","verdict":"allowed"}"#).unwrap();
    assert!(p.facts.is_empty() && p.check.is_none());
}

#[test]
fn normalize_french_names() {
    assert_eq!(
        normalize::name("Orsel d'Aubrac"),
        normalize::name("  orsel D’Aubrac ")
    );
    assert_eq!(
        normalize::name("l'Hôtel de Ville"),
        normalize::name("L'Hôtel  de ville")
    );
    assert_eq!(normalize::key("Gate State"), "gate_state");
}

#[test]
fn normalize_uses_nfkc_and_strips_english_articles() {
    assert_eq!(normalize::name("  The  Ｈôtel  "), "hôtel");
    assert_eq!(normalize::value(" An  Orsel d’Aubrac "), "orsel d'aubrac");
}

#[test]
fn charter_parses_from_toml() {
    let charter: Charter = toml::from_str(
        r#"
premise = "A city on the edge"
story_language = "fr"
boundaries = ["No graphic violence"]
proposed = []

[pc]
name = "Orsel"
adult = true
concept = "A traveling mediator"

[[decided]]
subject = "Orsel"
key = "home"
value = "Aubrac"
text = "Orsel comes from Aubrac."
"#,
    )
    .unwrap();

    assert_eq!(charter.pc.name, "Orsel");
    assert!(charter.pc.adult);
    assert_eq!(charter.decided[0].key, "home");
    assert!(charter.proposed.is_empty());
}

#[test]
fn proposal_parses_handles() {
    let p: DecisionProposal =
        serde_json::from_str(r#"{"action":"talk","verdict":"allowed","blockers":["F40"]}"#)
            .unwrap();

    assert_eq!(p.blockers, [Handle::Fact(40)]);
}

#[test]
fn revert_turn_serializes_its_turn_id() {
    let op = threshold_core::domain::LedgerOp::RevertTurn { turn_id: 12 };

    assert_eq!(
        serde_json::to_value(op).unwrap(),
        serde_json::json!({"revert_turn":{"turn_id":12}})
    );
}

#[test]
fn schema_for_limits_entity_handles() {
    use threshold_core::{HandleContext, proposal_schema_for};
    let ctx = HandleContext {
        entities: vec![Handle::Entity(1), Handle::Entity(5)],
        facts: vec![Handle::Fact(2)],
        events: vec![Handle::WorldEvent(3)],
        goals: vec![Handle::Goal(4)],
        new_slots: 1,
    };
    let s = proposal_schema_for(&ctx);
    assert_eq!(
        s.pointer("/$defs/Reaction/properties/npc/enum").unwrap(),
        &serde_json::json!(["E1", "E5", "N1"])
    );
    assert_eq!(
        s.pointer("/$defs/SlotDraft/properties/subject/enum")
            .unwrap(),
        &serde_json::json!(["E1", "E5", "N1"])
    );
    assert_eq!(
        s.pointer("/$defs/Factor/properties/fact/enum").unwrap(),
        &serde_json::json!(["F2"])
    );
    assert_eq!(
        s.pointer("/$defs/Cause/oneOf/1/properties/fact/enum")
            .unwrap(),
        &serde_json::json!(["F2"])
    );
    assert_eq!(
        s.pointer("/$defs/Cancel/properties/event/enum").unwrap(),
        &serde_json::json!(["W3"])
    );
    assert_eq!(
        s.pointer("/$defs/NewNpc/properties/id/enum").unwrap(),
        &serde_json::json!(["N1"])
    );
}

#[test]
fn schema_for_omits_event_fields_without_events() {
    use threshold_core::{HandleContext, proposal_schema_for};
    let ctx = HandleContext {
        entities: vec![Handle::Entity(1)],
        facts: vec![],
        events: vec![],
        goals: vec![],
        new_slots: 0,
    };
    let s = proposal_schema_for(&ctx);
    assert_eq!(s["additionalProperties"], false);
    assert_eq!(s["$defs"]["EventDraft"]["additionalProperties"], false);
    assert_eq!(s["$defs"]["Cause"]["oneOf"].as_array().unwrap().len(), 1);
    assert!(s.pointer("/properties/cancel").is_none());
    assert!(s.pointer("/$defs/EventDraft/properties/goal").is_none());
    assert!(s.pointer("/properties/new_npcs").is_none());
    assert!(s.pointer("/properties/blockers").is_none());
    assert_eq!(
        s.pointer("/$defs/CheckDraft/properties/factors/maxItems")
            .unwrap(),
        0
    );
}

#[test]
fn allowed_handle_proposal_parses_and_validates_shape() {
    use threshold_core::validate_shape;
    let p: DecisionProposal = serde_json::from_str(r#"{"action":"act","verdict":"check","blockers":["F2"],"check":{"win":{"subject":"E1","key":"locked","value":"no"},"lose":{"subject":"E1","key":"locked","value":"yes"},"factors":[{"fact":"F2","helps":false}]},"reactions":[{"npc":"N1","stance":"neutral","plan":"wait"}]}"#).unwrap();
    assert!(validate_shape(&p).is_ok());
}

#[test]
fn shape_requires_check_iff_verdict_is_check() {
    use threshold_core::validate_shape;
    let missing: DecisionProposal =
        serde_json::from_str(r#"{"action":"act","verdict":"check"}"#).unwrap();
    assert!(
        validate_shape(&missing)
            .unwrap_err()
            .to_string()
            .contains("check")
    );
    let extra: DecisionProposal = serde_json::from_str(r#"{"action":"act","verdict":"allowed","check":{"win":{"subject":"E1","key":"x","value":"y"},"lose":{"subject":"E1","key":"x","value":"z"},"factors":[]}}"#).unwrap();
    assert!(
        validate_shape(&extra)
            .unwrap_err()
            .to_string()
            .contains("check")
    );
    let ordinary: DecisionProposal =
        serde_json::from_str(r#"{"action":"act","verdict":"allowed"}"#).unwrap();
    assert!(validate_shape(&ordinary).is_ok());
}

#[test]
fn schema_caps_stakes_and_event_text() {
    use threshold_core::domain::SlotDraft;
    let slot = serde_json::to_value(schema_for!(SlotDraft)).unwrap();
    assert_eq!(slot.pointer("/properties/key/maxLength").unwrap(), 32);
    assert_eq!(slot.pointer("/properties/value/maxLength").unwrap(), 40);
    let event = serde_json::to_value(schema_for!(EventDraft)).unwrap();
    assert_eq!(event.pointer("/properties/text/maxLength").unwrap(), 80);
}
