//! `finds` (ADR 0007, 2026-11-12): what the PC finds in a Turn whose Intent
//! looks for information or content. A required field whose kind of source,
//! `from`, has `none` first; the core keeps it on the Turn record when the
//! act happens, guards its text, and tells it to the narrator and to later
//! prompts.

mod support;

use support::*;
use threshold_core::{
    CallKind, HandleContext, Prompt, RecordedGm,
    checks::{self, CheckKind},
    closing_schema_for,
    context::{FINDS_FIELD_LINE, PROPOSAL_RULES, guard_found},
    domain::{
        DecisionProposal, EntityKind, FINDS_DEFINITION, Finds, FoundItem, Handle, Intimacy,
        SeedEntity, SourceKind, Verdict,
    },
    proposal_schema_for,
    rules::D10,
    store::Store,
    turn::{Engine, TurnError, stored_found},
};

fn json(p: &DecisionProposal) -> String {
    serde_json::to_string(p).unwrap()
}

fn item(title: &str, line: &str) -> FoundItem {
    FoundItem {
        title: title.into(),
        line: line.into(),
    }
}

fn notices() -> Finds {
    Finds {
        from: SourceKind::NoticeOrSign,
        source: "the notice board".into(),
        items: vec![
            item(
                "Court sits on market day",
                "The magistrate hears small claims from the third bell.",
            ),
            item(
                "Lost: a grey mule",
                "Answers to Pell. Reward at the toll house.",
            ),
        ],
    }
}

fn looking(finds: Finds) -> DecisionProposal {
    let mut p = proposal(Verdict::Allowed);
    p.action = threshold_core::domain::ActionKind::Observe;
    p.finds = finds;
    p
}

fn setup(w: World, proposals: Vec<String>, narrations: Vec<&str>) -> (Engine, u64, RecordedGm) {
    let mut w = w;
    let gm = RecordedGm::new(proposals, narrations.into_iter().map(Into::into).collect());
    let store = std::mem::replace(&mut w.store, Store::open_in_memory().unwrap());
    (
        Engine::new(
            store,
            Box::new(gm.clone()),
            Box::new(D10),
            vec![],
            w.campaign,
        ),
        w.campaign,
        gm,
    )
}

fn prompts(gm: &RecordedGm, kind: CallKind) -> Vec<Prompt> {
    gm.calls()
        .into_iter()
        .filter(|c| c.kind == kind)
        .map(|c| c.prompt)
        .collect()
}

fn found_of(engine: &Engine, turn: u64) -> Option<Finds> {
    let stored = engine.store().turn(turn).unwrap().unwrap();
    stored_found(&stored.record.proposal_json)
}

/// The gate world with a hidden NPC and a hidden place: names the PC must
/// not hear of.
fn hidden_world() -> World {
    let mut seed = adventure_seed();
    seed.entities.push(SeedEntity {
        name: "Maren Voss".into(),
        kind: EntityKind::Npc,
        age: Some(40),
        other_names: vec![],
        hidden: true,
    });
    seed.entities.push(SeedEntity {
        name: "Smugglers Cellar".into(),
        kind: EntityKind::Place,
        age: None,
        other_names: vec![],
        hidden: true,
    });
    World::with_seed(seed)
}

// --- The field ---------------------------------------------------------------

#[test]
fn the_schema_requires_finds_with_none_first() {
    let w = World::new();
    let ctx = HandleContext {
        entities: vec![Handle::Entity(w.ada), Handle::Entity(w.orsel)],
        ..HandleContext::default()
    };
    let schema = proposal_schema_for(&ctx);
    let required = schema["required"].as_array().unwrap();
    assert!(required.iter().any(|k| k == "finds"), "{required:?}");
    let finds = &schema["properties"]["finds"];
    assert_eq!(finds["description"], FINDS_DEFINITION);
    // The kind of source is a closed enum, `none` first, and it comes
    // before the label and the items (live run of b245f71: a free label
    // let a greeting find "the clerk's reaction").
    assert_eq!(
        finds["properties"]["from"]["enum"],
        serde_json::json!([
            "none",
            "screen",
            "document",
            "notice_or_sign",
            "container",
            "shelf_or_search",
            "list_or_ledger"
        ])
    );
    assert!(finds["properties"].get("what").is_none());
    assert_eq!(
        finds["required"],
        serde_json::json!(["from", "source", "items"])
    );
    // The schema's keys are sorted, and the model writes them in that
    // order: the kind comes first, before any item. (The raw log of b245f71
    // shows `items`, `source`, then `what`: the decision came last.)
    let order: Vec<&String> = finds["properties"].as_object().unwrap().keys().collect();
    assert_eq!(order, ["from", "items", "source"]);
    assert_eq!(finds["additionalProperties"], false);
    assert_eq!(finds["properties"]["source"]["maxLength"], 40);
    assert_eq!(finds["properties"]["items"]["maxItems"], 5);
    let entry = &finds["properties"]["items"]["items"];
    assert_eq!(entry["properties"]["title"]["maxLength"], 60);
    assert_eq!(entry["properties"]["line"]["maxLength"], 140);
    assert_eq!(entry["required"], serde_json::json!(["title", "line"]));

    // A Closing Turn has no Intent, so it looks for nothing.
    let closing = closing_schema_for(&ctx);
    assert!(closing["properties"].get("finds").is_none());
    assert!(
        !closing["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "finds")
    );
}

#[test]
fn the_schema_takes_a_sound_finds_and_refuses_a_long_one() {
    let schema = proposal_schema_for(&HandleContext::default());
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut p = serde_json::json!({
        "action": "observe", "verdict": "allowed", "contested": false,
        "intimacy": "none", "move_to": null,
        "finds": serde_json::to_value(notices()).unwrap(),
    });
    assert!(validator.is_valid(&p), "{:?}", validator.validate(&p));
    p["finds"] = serde_json::to_value(Finds::default()).unwrap();
    assert!(validator.is_valid(&p), "{:?}", validator.validate(&p));
    p["finds"]["from"] = "maybe".into();
    assert!(!validator.is_valid(&p));
    // The kind of a stored record from before `from` is no model value.
    p["finds"]["from"] = "other".into();
    assert!(!validator.is_valid(&p));
    p["finds"] = serde_json::to_value(notices()).unwrap();
    p["finds"]["items"][0]["title"] = "x".repeat(61).into();
    assert!(!validator.is_valid(&p));
    p.as_object_mut().unwrap().remove("finds");
    assert!(!validator.is_valid(&p));
}

#[test]
fn the_finds_definition_is_one_text() {
    assert_eq!(FINDS_FIELD_LINE, format!("- finds: {FINDS_DEFINITION}"));
    // Each kind the model may write is defined in it, and speech is `none`.
    for kind in SourceKind::ALL {
        assert!(
            FINDS_DEFINITION.contains(&format!("{} = ", kind.word())),
            "{}",
            kind.word()
        );
    }
    assert!(!SourceKind::ALL.contains(&SourceKind::Other));
    assert!(FINDS_DEFINITION.contains("what a person says is told through reactions, never here"));
    assert!(
        PROPOSAL_RULES.contains(FINDS_FIELD_LINE),
        "{PROPOSAL_RULES}"
    );
}

#[test]
fn a_raw_play_proposal_without_finds_is_a_schema_error() {
    let mut raw = serde_json::to_value(proposal(Verdict::Allowed)).unwrap();
    raw.as_object_mut().unwrap().remove("finds");
    let (mut engine, _, gm) = setup(
        World::new(),
        vec![raw.to_string(), json(&proposal(Verdict::Allowed))],
        vec!["You look around."],
    );
    engine
        .play_turn("t1".into(), "I look around.", &mut |_| {})
        .unwrap();
    assert_eq!(
        gm.calls().iter().map(|c| c.kind).collect::<Vec<_>>(),
        [CallKind::Propose, CallKind::Propose, CallKind::Narrate]
    );
    let validate: Vec<String> = engine
        .store()
        .debug_log("t1")
        .unwrap()
        .into_iter()
        .filter(|e| e.step == "validate")
        .map(|e| e.raw)
        .collect();
    assert!(validate[0].starts_with("schema"), "{validate:?}");
    assert!(validate[0].contains("finds"), "{validate:?}");
    assert_eq!(validate[1], "ok");
}

#[test]
fn an_old_stored_proposal_reads_nothing_found() {
    let old = r#"{"action":"talk","verdict":"allowed","minutes":5}"#;
    let p: DecisionProposal = serde_json::from_str(old).unwrap();
    assert_eq!(p.finds, Finds::default());
    assert_eq!(p.finds.from, SourceKind::None);
    let record = format!(r#"{{"proposal":{old},"reactions":[],"events":[],"roll":null}}"#);
    assert_eq!(stored_found(&record), None);
    assert_eq!(stored_found("{}"), None);
}

/// A Turn stored with the first shape of the field (b245f71, `what`) still
/// reads: `found` is a result of an unknown kind, `nothing` is `none`.
#[test]
fn a_record_stored_with_what_still_reads() {
    let record = r#"{"proposal":{"action":"act","verdict":"allowed","finds":{"what":"found","source":"the drawer","items":[{"title":"A key","line":"Small and rusty."}]}},"found":{"what":"found","source":"the drawer","items":[{"title":"A key","line":"Small and rusty."}]}}"#;
    let found = stored_found(record).expect("an old found record reads");
    assert_eq!(found.from, SourceKind::Other);
    assert_eq!(found.source, "the drawer");
    assert_eq!(found.items, [item("A key", "Small and rusty.")]);
    let p: serde_json::Value = serde_json::from_str(record).unwrap();
    let p: DecisionProposal = serde_json::from_value(p["proposal"].clone()).unwrap();
    assert_eq!(p.finds.from, SourceKind::Other);
    let nothing: Finds =
        serde_json::from_str(r#"{"what":"nothing","source":"","items":[]}"#).unwrap();
    assert_eq!(nothing, Finds::default());
    // A value written now has `from`, and no `what`.
    let written = serde_json::to_value(notices()).unwrap();
    assert_eq!(written["from"], "notice_or_sign");
    assert!(written.get("what").is_none());
}

// --- settle and validate ----------------------------------------------------

#[test]
fn settle_drops_a_malformed_or_over_long_finds() {
    let w = World::new();
    let long = |n: usize| "x".repeat(n);
    let cases: Vec<(&str, Finds, Finds)> = vec![
        (
            "none with content",
            Finds {
                from: SourceKind::None,
                source: "the drawer".into(),
                items: vec![item("A key", "")],
            },
            Finds::default(),
        ),
        (
            "a kind with no item",
            Finds {
                from: SourceKind::Container,
                source: "the drawer".into(),
                items: vec![],
            },
            Finds::default(),
        ),
        (
            "the kind of a stored record",
            Finds {
                from: SourceKind::Other,
                ..notices()
            },
            Finds::default(),
        ),
        (
            "found with no source",
            Finds {
                source: " ".into(),
                ..notices()
            },
            Finds::default(),
        ),
        (
            "a source over 40",
            Finds {
                source: long(41),
                ..notices()
            },
            Finds::default(),
        ),
        (
            "one item over its limits",
            Finds {
                items: vec![
                    item(&long(61), "too long a title"),
                    item("Fine", &long(141)),
                    item("", "no title"),
                    item("Kept", "A plain line."),
                ],
                ..notices()
            },
            Finds {
                items: vec![item("Kept", "A plain line.")],
                ..notices()
            },
        ),
        (
            "every item over its limits",
            Finds {
                items: vec![item(&long(61), "")],
                ..notices()
            },
            Finds::default(),
        ),
        (
            "six items",
            Finds {
                items: (1..=6).map(|i| item(&format!("Entry {i}"), "")).collect(),
                ..notices()
            },
            Finds {
                items: (1..=5).map(|i| item(&format!("Entry {i}"), "")).collect(),
                ..notices()
            },
        ),
    ];
    for (case, given, kept) in cases {
        let (p, notes) = checks::settle(looking(given), &w.view);
        assert_eq!(p.finds, kept, "{case}");
        assert!(
            notes.iter().any(|n| n.contains("finds")),
            "{case}: {notes:?}"
        );
        assert!(checks::validate(&p, &w.view).is_ok(), "{case}");
    }
    // A sound value stays as it is, with no note.
    let (p, notes) = checks::settle(looking(notices()), &w.view);
    assert_eq!(p.finds, notices());
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn validate_rejects_a_malformed_finds() {
    let w = World::new();
    for bad in [
        Finds {
            from: SourceKind::None,
            source: "the drawer".into(),
            items: vec![],
        },
        Finds {
            from: SourceKind::Other,
            ..notices()
        },
        Finds {
            items: vec![],
            ..notices()
        },
        Finds {
            source: "x".repeat(41),
            ..notices()
        },
        Finds {
            items: vec![item(&"x".repeat(61), "")],
            ..notices()
        },
        Finds {
            items: (1..=6).map(|i| item(&format!("Entry {i}"), "")).collect(),
            ..notices()
        },
    ] {
        let p = looking(bad.clone());
        let errs = checks::validate(&p, &w.view).unwrap_err();
        assert!(
            errs.iter()
                .any(|r| r.check == CheckKind::References && r.reason.contains("finds")),
            "{bad:?}: {errs:?}"
        );
    }
    assert!(checks::validate(&looking(notices()), &w.view).is_ok());
}

/// The gate's collector reads handles only, and a found item is free text.
/// So a Turn the age rule runs for (`advance` or `intimate`) keeps no
/// finds: `settle` drops them, and `validate` rejects them.
#[test]
fn a_tagged_turn_keeps_no_finds() {
    let w = World::new();
    let mut p = looking(notices());
    p.intimacy = Intimacy::Advance;
    let errs = checks::validate(&p, &w.view).unwrap_err();
    assert!(errs.iter().any(|r| r.reason.contains("finds")), "{errs:?}");
    let (settled, notes) = checks::settle(p, &w.view);
    assert_eq!(settled.finds, Finds::default());
    assert!(notes.iter().any(|n| n.contains("finds")), "{notes:?}");
}

// --- What the Turn records --------------------------------------------------

#[test]
fn the_turn_records_what_the_pc_found_when_the_act_happens() {
    let (mut engine, campaign, gm) = setup(
        World::new(),
        vec![json(&looking(notices()))],
        vec!["You read the board."],
    );
    let facts_before = engine.store().snapshot(campaign).unwrap().facts.len();
    let outcome = engine
        .play_turn("t1".into(), "I read the notices on the board.", &mut |_| {})
        .unwrap();
    assert_eq!(found_of(&engine, outcome.turn_id), Some(notices()));
    // No fact comes from it: the Turn record holds it.
    assert_eq!(
        engine.store().snapshot(campaign).unwrap().facts.len(),
        facts_before
    );
    // The narrator gets it in the outcome, in the per-Turn part.
    let told = &prompts(&gm, CallKind::Narrate)[0];
    let outcome_block = &told.recent[told.recent.find("THIS TURN").unwrap()..];
    assert!(
        outcome_block.contains("- What the PC finds (the notice board)"),
        "{outcome_block}"
    );
    assert!(
        outcome_block.contains(
            "  - \"Court sits on market day\": The magistrate hears small claims from the third bell."
        ),
        "{outcome_block}"
    );
    assert!(
        outcome_block
            .contains("  - \"Lost: a grey mule\": Answers to Pell. Reward at the toll house."),
        "{outcome_block}"
    );
    for part in [&told.system, &told.stable, &told.scene] {
        assert!(!part.contains("grey mule"), "{part}");
    }
}

/// The live run of b245f71: "I walk over to the wall and read the notices"
/// came back as a `move` with `move_to: null`. The narrator got "The move
/// does not happen" after the found lines, and told the PC stopping short
/// of the wall in 5 of the 11 lookups that found something. A Turn that
/// keeps found content happened, so the narrator gets where the PC is,
/// not a move that failed.
#[test]
fn a_move_inside_the_place_that_finds_something_is_told_as_happening() {
    let mut p = looking(notices());
    p.action = threshold_core::domain::ActionKind::Move;
    let (mut engine, _, gm) = setup(World::new(), vec![json(&p)], vec!["You read the board."]);
    engine
        .play_turn(
            "t1".into(),
            "I walk over to the board and read the notices.",
            &mut |_| {},
        )
        .unwrap();
    let told = &prompts(&gm, CallKind::Narrate)[0];
    let outcome_block = &told.recent[told.recent.find("THIS TURN").unwrap()..];
    assert!(
        outcome_block.contains("- What the PC finds (the notice board)"),
        "{outcome_block}"
    );
    assert!(
        !outcome_block.contains(threshold_core::context::NOT_MOVED_LINE),
        "{outcome_block}"
    );
    assert!(
        outcome_block.contains(threshold_core::context::PC_PLACE_LINE),
        "{outcome_block}"
    );

    // The same move that finds nothing is still a move that did not happen.
    let mut p = looking(Finds::default());
    p.action = threshold_core::domain::ActionKind::Move;
    let (mut engine, _, gm) = setup(World::new(), vec![json(&p)], vec!["You stay."]);
    engine
        .play_turn("t1".into(), "I walk to the far hills.", &mut |_| {})
        .unwrap();
    let told = &prompts(&gm, CallKind::Narrate)[0];
    assert!(
        told.recent
            .contains(threshold_core::context::NOT_MOVED_LINE),
        "{}",
        told.recent
    );
}

#[test]
fn an_impossible_turn_records_nothing_found() {
    let mut p = looking(notices());
    p.verdict = Verdict::Impossible;
    let (mut engine, _, gm) = setup(World::new(), vec![json(&p)], vec!["Nothing."]);
    let outcome = engine
        .play_turn("t1".into(), "I read the notices on the board.", &mut |_| {})
        .unwrap();
    assert_eq!(found_of(&engine, outcome.turn_id), None);
    let told = &prompts(&gm, CallKind::Narrate)[0];
    assert!(
        !told.recent.contains("What the PC finds"),
        "{}",
        told.recent
    );
}

#[test]
fn only_an_act_that_happens_finds_anything() {
    let w = World::new();
    let mut p = looking(notices());
    let valid = checks::validate(&p, &w.view).unwrap();
    assert_eq!(checks::found(valid, None, &w.view).0, Some(notices()));

    p.verdict = Verdict::Check;
    p.check = Some(check(
        None,
        slot(e(w.gate), "locked", "no"),
        slot(e(w.ada), "fatigue", "winded"),
    ));
    let valid = checks::validate(&p, &w.view).unwrap();
    assert_eq!(
        checks::found(valid, Some(&roll(true)), &w.view).0,
        Some(notices())
    );
    assert_eq!(checks::found(valid, Some(&roll(false)), &w.view).0, None);

    p.verdict = Verdict::Impossible;
    p.check = None;
    let valid = checks::validate(&p, &w.view).unwrap();
    assert_eq!(checks::found(valid, None, &w.view).0, None);

    let valid_nothing = looking(Finds::default());
    let valid = checks::validate(&valid_nothing, &w.view).unwrap();
    assert_eq!(checks::found(valid, None, &w.view).0, None);
}

/// The item text is model text. An item that names an NPC or a place the
/// PC must not hear of, or that states a GM-only value, goes: the guard of
/// the narrator's event text (`context::guard_events`).
#[test]
fn an_item_that_names_a_hidden_entity_or_a_secret_is_dropped() {
    let w = hidden_world();
    let finds = Finds {
        items: vec![
            item("A note from Maren", "Meet me after dark."),
            item("Cellar rent", "Paid to the Smugglers Cellar each month."),
            item("Keys", "Ada still holds the gate key."),
            item("Tolls", "Carts pay two copper at the gate."),
        ],
        ..notices()
    };
    let (kept, notes) = guard_found(&w.view, &finds);
    assert_eq!(
        kept,
        Some(Finds {
            items: vec![item("Tolls", "Carts pay two copper at the gate.")],
            ..notices()
        })
    );
    assert_eq!(notes.len(), 3, "{notes:?}");
    // A note says which item went, never its text.
    assert!(notes.iter().all(|n| !n.contains("Maren")), "{notes:?}");

    // A source that names one drops everything; so does an empty result.
    let hidden_source = Finds {
        source: "the Smugglers Cellar ledger".into(),
        ..notices()
    };
    assert_eq!(guard_found(&w.view, &hidden_source).0, None);
    let only_secret = Finds {
        items: vec![item("A note from Maren", "")],
        ..notices()
    };
    assert_eq!(guard_found(&w.view, &only_secret).0, None);
}

/// The town campaign as the eval plays it: the Adventure Seed, then the
/// world.
fn town() -> threshold_core::store::WorldView {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/town");
    let seed = threshold_core::fixture::load_adventure_seed(&dir.join("adventure-seed.toml"))
        .expect("the town seed");
    let mut store = Store::open_in_memory().unwrap();
    let campaign = threshold_core::turn::create_campaign(&mut store, &seed, 11, None).unwrap();
    threshold_core::fixture::load_world(&mut store, campaign, &dir.join("world.toml"))
        .expect("the town world");
    store.snapshot(campaign).unwrap()
}

/// The live run of b245f71 (`evals/reports/2026-11-finds.md`): the guard
/// dropped notices that said "the magistrate" and a source that said "the
/// clerk", because Orsel's role and Hessa's role are facts the PC holds no
/// Belief about. A one-word value is a common word; a found item states
/// it without stating the secret. The guard of found content keeps what
/// protects hidden content: the names of hidden and un-nameable entities,
/// and GM-only values of at least two content words.
#[test]
fn the_found_guard_keeps_common_words_and_drops_hidden_content() {
    let view = town();
    let finds = Finds {
        source: "the clerk's notice board".into(),
        items: vec![
            item(
                "Trade Decree",
                "A decree from the magistrate's office on trade standards.",
            ),
            item("Grain Notice", "The grain tithe goes up next season."),
            item(
                "Archive hours",
                "The Tithe archive opens at the third bell.",
            ),
        ],
        ..notices()
    };
    let (kept, notes) = guard_found(&view, &finds);
    let kept = kept.expect("the common words keep the result");
    assert_eq!(kept.source, "the clerk's notice board");
    let titles: Vec<&str> = kept.items.iter().map(|i| i.title.as_str()).collect();
    // "the grain tithe" is the value of a GM-only fact (two content words).
    assert!(titles.contains(&"Trade Decree"), "{titles:?} {notes:?}");
    assert!(!titles.contains(&"Grain Notice"), "{titles:?}");
    assert_eq!(notes.len(), titles.len().abs_diff(3), "{notes:?}");

    // The names of hidden and un-nameable entities still go.
    let w = hidden_world();
    let finds = Finds {
        items: vec![
            item("A note from Maren", "Meet me after dark."),
            item("Cellar rent", "Paid to the Smugglers Cellar each month."),
            item("Keys", "Ada still holds the gate key."),
            item("Tolls", "Carts pay two copper at the gate."),
        ],
        ..notices()
    };
    let (kept, _) = guard_found(&w.view, &finds);
    assert_eq!(
        kept.map(|f| f.items),
        Some(vec![item("Tolls", "Carts pay two copper at the gate.")])
    );
}

#[test]
fn a_hidden_name_in_a_found_item_never_reaches_the_record() {
    let w = hidden_world();
    let finds = Finds {
        items: vec![
            item("A note from Maren", "Meet me after dark."),
            item("Tolls", "Carts pay two copper at the gate."),
        ],
        ..notices()
    };
    let (mut engine, _, gm) = setup(w, vec![json(&looking(finds))], vec!["You read."]);
    let outcome = engine
        .play_turn("t1".into(), "I read the notices on the board.", &mut |_| {})
        .unwrap();
    let stored = engine.store().turn(outcome.turn_id).unwrap().unwrap();
    let found = stored_found(&stored.record.proposal_json).unwrap();
    assert_eq!(
        found.items,
        [item("Tolls", "Carts pay two copper at the gate.")]
    );
    let told = &prompts(&gm, CallKind::Narrate)[0];
    assert!(!told.recent.contains("Maren"), "{}", told.recent);
}

// --- Later prompts ------------------------------------------------------------

#[test]
fn the_next_prompts_show_what_an_earlier_turn_found() {
    let (mut engine, _, gm) = setup(
        World::new(),
        vec![json(&looking(notices())), json(&looking(Finds::default()))],
        vec!["You read the board.", "You nod."],
    );
    engine
        .play_turn("t1".into(), "I read the notices on the board.", &mut |_| {})
        .unwrap();
    engine
        .play_turn(
            "t2".into(),
            "I look closer at the mule notice.",
            &mut |_| {},
        )
        .unwrap();
    let line = "Found: the notice board: \"Court sits on market day\": The magistrate hears small claims from the third bell.; \"Lost: a grey mule\": Answers to Pell. Reward at the toll house.\n";
    let proposal = &prompts(&gm, CallKind::Propose)[1];
    assert!(proposal.recent.contains(line), "{}", proposal.recent);
    let narration = &prompts(&gm, CallKind::Narrate)[1];
    let past = &narration.recent[..narration.recent.find("THIS TURN").unwrap()];
    assert!(past.contains(line), "{past}");
}

#[test]
fn a_faded_turn_shows_no_found_content_in_a_later_prompt() {
    let (mut engine, _, gm) = setup(
        World::new(),
        vec![json(&looking(notices())), json(&looking(Finds::default()))],
        vec!["You read the board.", "You nod."],
    );
    engine
        .play_turn("t1".into(), "I read the notices on the board.", &mut |_| {})
        .unwrap();
    engine.fade("t1".into()).unwrap();
    engine
        .play_turn("t2".into(), "I look around.", &mut |_| {})
        .unwrap();
    for kind in [CallKind::Propose, CallKind::Narrate] {
        let later = prompts(&gm, kind).pop().unwrap();
        assert!(!later.recent.contains("grey mule"), "{}", later.recent);
        assert!(!later.recent.contains("Found:"), "{}", later.recent);
    }
}

#[test]
fn renarrate_tells_the_stored_finds_again() {
    let (mut engine, _, gm) = setup(
        World::new(),
        vec![json(&looking(notices()))],
        vec!["You read the board.", "You read it again."],
    );
    let outcome = engine
        .play_turn("t1".into(), "I read the notices on the board.", &mut |_| {})
        .unwrap();
    engine.renarrate(outcome.turn_id, &mut |_| {}).unwrap();
    let again = prompts(&gm, CallKind::Narrate).pop().unwrap();
    assert!(
        again
            .recent
            .contains("- What the PC finds (the notice board)"),
        "{}",
        again.recent
    );
}

/// A Correction plans the Turn again: what it finds is its own record, and
/// the reverted Turn's record shows on no later prompt.
#[test]
fn a_correction_keeps_its_own_finds() {
    let other = Finds {
        source: "the toll house door".into(),
        items: vec![item("Tolls", "Carts pay two copper at the gate.")],
        from: SourceKind::NoticeOrSign,
    };
    let (mut engine, _, gm) = setup(
        World::new(),
        vec![
            json(&looking(notices())),
            json(&looking(other.clone())),
            json(&looking(Finds::default())),
        ],
        vec!["You read the board.", "You read the door.", "You nod."],
    );
    engine
        .play_turn("t1".into(), "I read the notices on the board.", &mut |_| {})
        .unwrap();
    let fixed = engine
        .correct_last(
            "c1".into(),
            "I read the sign on the toll house door.",
            "I meant the door",
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(found_of(&engine, fixed.turn_id), Some(other));
    engine
        .play_turn("t2".into(), "I look around.", &mut |_| {})
        .unwrap();
    let later = prompts(&gm, CallKind::Propose).pop().unwrap();
    assert!(!later.recent.contains("grey mule"), "{}", later.recent);
    assert!(
        later.recent.contains("Found: the toll house door"),
        "{}",
        later.recent
    );
}

#[test]
fn a_rejected_finds_never_commits() {
    let w = World::new();
    let bad = looking(Finds {
        items: vec![],
        ..notices()
    });
    // `settle` repairs it: the Turn commits with nothing found.
    let (mut engine, _, _) = setup(w, vec![json(&bad)], vec!["You look."]);
    let outcome = engine.play_turn("t1".into(), "I look in the drawer.", &mut |_| {});
    assert!(
        !matches!(outcome, Err(TurnError::Rejected(_))),
        "{outcome:?}"
    );
    assert_eq!(found_of(&engine, outcome.unwrap().turn_id), None);
}

// --- Screens ------------------------------------------------------------------

mod screens {
    use super::{item, notices};
    use crate::support::hidden::*;
    use serde_json::json;
    use threshold_core::{
        domain::{ActionKind, Finds, Intimacy, SourceKind, Verdict},
        player::Marks,
        store::TurnId,
        view::{FoundCard, FoundRow, journal_view, ooc_sheet, play_view},
    };

    /// A `proposal_json` as the Engine stores a Turn that found `found`.
    fn finding(found: &Finds, intimacy: Intimacy) -> String {
        let mut p = crate::support::proposal(Verdict::Allowed);
        p.action = ActionKind::Observe;
        p.intimacy = intimacy;
        p.finds = found.clone();
        json!({ "proposal": p, "reactions": [], "events": [], "roll": null, "found": found })
            .to_string()
    }

    fn card() -> FoundCard {
        FoundCard {
            source: "the notice board".into(),
            items: vec![
                FoundRow {
                    title: "Court sits on market day".into(),
                    line: "The magistrate hears small claims from the third bell.".into(),
                },
                FoundRow {
                    title: "Lost: a grey mule".into(),
                    line: "Answers to Pell. Reward at the toll house.".into(),
                },
            ],
        }
    }

    fn read(w: &mut HiddenWorld) -> TurnId {
        let turn = w.play(
            "I read the notices on the board.",
            &finding(&notices(), Intimacy::None),
            5,
            vec![],
        );
        w.narrate(turn, "You read the board by the gate.");
        turn
    }

    #[test]
    fn the_story_shows_what_the_pc_found_under_its_narration() {
        let mut w = HiddenWorld::new();
        let turn = read(&mut w);
        let view = play_view(&w.sight(), &Marks::default());
        for story in &view.story {
            if story.turn_id == turn {
                assert_eq!(story.found, Some(card()));
            } else {
                assert_eq!(story.found, None, "turn {}", story.turn_id);
            }
        }
    }

    #[test]
    fn a_faded_reverted_or_unread_turn_shows_no_found_card() {
        let mut w = HiddenWorld::new();
        let faded = read(&mut w);
        w.store.set_faded(faded).unwrap();
        let reverted = read(&mut w);
        w.revert(reverted, "I look at the gate instead.", 0);
        // A Turn read as its Neutral Line. The core keeps no finds on such a
        // Turn; a record from elsewhere still shows nothing.
        let intimate = w.play(
            "I read the notices on the board.",
            &finding(&notices(), Intimacy::Intimate),
            5,
            vec![],
        );
        w.narrate(intimate, "You read the board.");
        let sight = w.sight();
        for turn in [faded, reverted, intimate] {
            let seen = sight.story.iter().find(|t| t.turn == turn).unwrap();
            assert_eq!(seen.found, None, "turn {turn}");
        }
        let view = play_view(&sight, &Marks::default());
        assert!(view.story.iter().all(|t| t.found.is_none()));
        let journal = journal_view(&sight, &Marks::default());
        assert!(journal.scenes.iter().all(|s| s.found.is_empty()));
        assert!(!ooc_sheet(&sight).contains("grey mule"));
    }

    #[test]
    fn the_journal_lists_what_the_pc_found_in_its_scene() {
        let mut w = HiddenWorld::new();
        let turn = read(&mut w);
        let journal = journal_view(&w.sight(), &Marks::default());
        let scene = journal
            .scenes
            .iter()
            .find(|s| s.acts.iter().any(|a| a.turn_id == turn))
            .expect("the Scene of the Turn");
        assert_eq!(scene.found.len(), 1);
        assert_eq!(scene.found[0].turn_id, turn);
        assert_eq!(scene.found[0].source, card().source);
        assert_eq!(scene.found[0].items, card().items);
        // Not a machine sentence among what the PC learned.
        assert!(scene.learned.iter().all(|l| !l.text.contains("mule")));
    }

    #[test]
    fn the_ooc_sheet_holds_what_recent_turns_found() {
        let mut w = HiddenWorld::new();
        read(&mut w);
        let sheet = ooc_sheet(&w.sight());
        let recent = &sheet[sheet.find("RECENT TURNS").unwrap()..];
        assert!(
            recent.contains(
                "Found: the notice board: \"Court sits on market day\": The magistrate hears small claims from the third bell.; \"Lost: a grey mule\": Answers to Pell. Reward at the toll house.\n"
            ),
            "{recent}"
        );
    }

    /// A record that does not read as a sound result (an older or broken
    /// writer) shows nothing: a screen never shows more than the field
    /// allows.
    #[test]
    fn a_record_that_breaks_the_limits_shows_nothing() {
        let mut w = HiddenWorld::new();
        let broken = Finds {
            from: SourceKind::NoticeOrSign,
            source: "the notice board".into(),
            items: (1..=6).map(|i| item(&format!("Entry {i}"), "")).collect(),
        };
        let turn = w.play(
            "I read the notices on the board.",
            &finding(&broken, Intimacy::None),
            5,
            vec![],
        );
        w.narrate(turn, "You read.");
        let sight = w.sight();
        let seen = sight.story.iter().find(|t| t.turn == turn).unwrap();
        assert_eq!(seen.found, None);
    }
}
