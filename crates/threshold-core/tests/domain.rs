use schemars::schema_for;
use threshold_core::{
    domain::{
        ADULT_AGE, AdventureSeed, CheckDraft, DecisionProposal, EventDraft, Handle, NewNpc,
        Reaction, Stance, is_adult,
    },
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
    assert_eq!(event.pointer("/properties/seen_by/maxItems").unwrap(), 2);
}

#[test]
fn schema_caps_text() {
    let reaction = serde_json::to_value(schema_for!(Reaction)).unwrap();
    assert_eq!(reaction.pointer("/properties/plan/maxLength").unwrap(), 90);

    let npc = serde_json::to_value(schema_for!(NewNpc)).unwrap();
    assert_eq!(npc.pointer("/properties/voice/maxLength").unwrap(), 120);
}

#[test]
fn is_adult_needs_a_stored_age() {
    assert_eq!(ADULT_AGE, 18);
    assert!(!is_adult(None));
    assert!(!is_adult(Some(17)));
    assert!(is_adult(Some(18)));
    assert!(is_adult(Some(80)));
}

#[test]
fn new_npc_schema_requires_age() {
    let npc = serde_json::to_value(schema_for!(NewNpc)).unwrap();
    let required = npc.pointer("/required").unwrap().as_array().unwrap();
    assert!(required.contains(&"age".into()), "{required:?}");
    assert_eq!(npc.pointer("/properties/age/minimum").unwrap(), 0);
    assert_eq!(npc.pointer("/properties/age/maximum").unwrap(), 999);
    assert!(npc.pointer("/properties/adult").is_none());
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
    assert_eq!(normalize::name("  The  Ｈôtel  "), "hotel");
    assert_eq!(normalize::value(" An  Orsel d’Aubrac "), "orsel d'aubrac");
}

#[test]
fn adventure_seed_parses_from_toml() {
    let adventure_seed: AdventureSeed = toml::from_str(
        r#"
premise = "A city on the edge"
story_language = "fr"
boundaries = ["No graphic violence"]
proposed = []

[pc]
name = "Orsel"
age = 34
concept = "A traveling mediator"

[[decided]]
subject = "Orsel"
key = "home"
value = "Aubrac"
text = "Orsel comes from Aubrac."
"#,
    )
    .unwrap();

    assert_eq!(adventure_seed.pc.name, "Orsel");
    assert_eq!(adventure_seed.pc.age, 34);
    assert!(adventure_seed.pc.other_names.is_empty());
    assert_eq!(adventure_seed.decided[0].key, "home");
    assert!(adventure_seed.proposed.is_empty());
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
    let op: threshold_core::domain::LedgerOp =
        threshold_core::domain::LedgerOp::RevertTurn { turn_id: 12 };

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
        ..Default::default()
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
        s.pointer("/$defs/EventDraft/properties/seen_by/items/enum")
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
        ..Default::default()
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

#[test]
fn entity_ref_serializes_ids_as_plain_numbers() {
    use threshold_core::domain::EntityRef;

    assert_eq!(
        serde_json::to_value(EntityRef::Id(7)).unwrap(),
        serde_json::json!(7)
    );
    assert_eq!(
        serde_json::to_value(EntityRef::New(1)).unwrap(),
        serde_json::json!({"new": 1})
    );
    assert_eq!(
        serde_json::from_value::<EntityRef>(serde_json::json!({"new": 2})).unwrap(),
        EntityRef::New(2)
    );
    assert_eq!(
        serde_json::from_value::<EntityRef>(serde_json::json!(3)).unwrap(),
        EntityRef::Id(3)
    );
}

#[test]
fn outcome_key_strips_the_open_prefix_only() {
    use threshold_core::domain::outcome_key;

    assert_eq!(outcome_key("pending_trip"), Some("trip"));
    assert_eq!(outcome_key("trip"), None);
    assert_eq!(outcome_key("pending_"), None);
}

#[test]
fn alias_key_is_the_one_writer() {
    use threshold_core::domain::{alias_key, is_alias_key};

    assert_eq!(alias_key("Jade Moss"), "alias_jade_moss");
    assert_eq!(alias_key("Chloé"), "alias_chloe");
    assert!(is_alias_key(&alias_key("Jade")));
    assert!(!is_alias_key("location"));
}

/// Bug pass 3: a slot write the model makes has no sentence of its own. The
/// core builds a plain one: the Player never reads `key = value`.
#[test]
fn a_slot_reads_as_a_plain_sentence() {
    use threshold_core::domain::slot_sentence;
    for (name, key, value, sentence) in [
        (
            "Teodor Brandt",
            "injury",
            "bruised_wrist",
            "Teodor Brandt is injured (bruised wrist).",
        ),
        (
            "Teodor Brandt",
            "location",
            "The Copper Kettle",
            "Teodor Brandt is at The Copper Kettle.",
        ),
        (
            "Tithe archive",
            "condition",
            "locked",
            "Tithe archive is locked.",
        ),
        ("Ada", "carrying", "a lantern", "Ada carries a lantern."),
        (
            "Doul",
            "grudge",
            "Wren",
            "Doul holds a grudge against Wren.",
        ),
        (
            "Ludo Brenn",
            "debt",
            "increased",
            "Ludo Brenn's debt is now increased.",
        ),
        (
            "Doul",
            "mood_now",
            "sore_loser",
            "Doul's mood now is now sore loser.",
        ),
        // Review minor (T8): a value that says "none" reads as a plain no.
        ("Ada", "injury", "none", "Ada is not injured."),
        ("Ada", "injury", "", "Ada is not injured."),
        ("Ada", "carrying", "nothing", "Ada carries nothing."),
        ("Ada", "condition", "none", "Ada is well."),
        ("Doul", "grudge", "none", "Doul holds no grudge."),
        ("Ada", "debt", "no", "Ada has no debt."),
        (
            "Ada",
            "condition",
            "no longer tired",
            "Ada is no longer tired.",
        ),
    ] {
        assert_eq!(slot_sentence(name, key, value), sentence, "{key} {value:?}");
        assert!(!sentence.contains(" = "));
        assert!(!sentence.contains("(none)"), "{sentence}");
    }
}

/// Review minor: "fatigue = rested" read "You are tired (rested).". A state
/// sentence of the PC is right for the values it knows, and falls back to a
/// neutral form for any other.
#[test]
fn a_state_of_the_pc_reads_right_for_known_values() {
    use threshold_core::domain::you_sentence;
    for (key, value, sentence) in [
        ("fatigue", "rested", "You are rested."),
        ("fatigue", "none", "You are rested."),
        ("fatigue", "", "You are rested."),
        ("fatigue", "tired", "You are tired."),
        ("fatigue", "winded", "You are winded."),
        ("fatigue", "exhausted", "You are exhausted."),
        ("fatigue", "light", "Your fatigue: light."),
        ("fatigue", "heavy", "Your fatigue: heavy."),
        (
            "injury",
            "bruised_wrist",
            "You are injured (bruised wrist).",
        ),
        ("injury", "none", "You are not injured."),
        (
            "reputation",
            "respected",
            "Your reputation is now respected.",
        ),
    ] {
        assert_eq!(you_sentence(key, value), sentence, "{key} {value:?}");
    }
}
