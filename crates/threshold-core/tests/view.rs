//! The screen types: each one is made from `PcSight` only.

mod support;

use std::path::{Path, PathBuf};

use serde_json::Value;
use support::hidden::*;
use threshold_core::{
    domain::{ActionKind, EntityKind, LedgerOp, Verdict},
    fixture::{load_adventure_seed, load_world},
    player::Marks,
    store::{Store, TurnId, TurnKind, TurnRecord},
    turn::create_campaign,
    view::{Block, NameRef, Part, PlayView, StoryTurn, TurnBody, character_view, play_view},
};

/// The town fixture after one Turn that gave Wren a fact Wren saw and a
/// fact only the GM knows.
const TOWN_CHARACTER: &str = r#"{"name":"Wren","concept":"An outsider scribe, hired to copy the town's charters","clock_min":5,"facts":[{"text":"Wren carries a satchel of quills.","open":false}]}"#;

/// What it gave for the hidden world, from `WorldView`.
const HIDDEN_CHARACTER: &str = r#"{"name":"Ada","concept":"Courier","clock_min":5,"facts":[{"text":"Ada plans to meet Brann at the mill.","open":true}]}"#;

fn town() -> (Store, u64) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/town");
    let seed = load_adventure_seed(&dir.join("adventure-seed.toml")).unwrap();
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, &seed, 11).unwrap();
    load_world(&mut store, campaign, &dir.join("world.toml")).unwrap();
    (store, campaign)
}

#[test]
fn character_view_shows_pc_knowledge_only() {
    let w = HiddenWorld::new();
    let json = serde_json::to_string(&character_view(&w.sight())).unwrap();
    for hidden in HIDDEN {
        assert!(!json.contains(hidden), "{hidden} leaks: {json}");
    }
    assert_eq!(json, HIDDEN_CHARACTER);

    // The town fixture starts with no fact the PC holds about itself, so
    // one Turn adds a seen fact and a GM-only fact.
    let (mut store, campaign) = town();
    let view = store.snapshot(campaign).unwrap();
    let wren = view.entities.iter().find(|e| e.name == "Wren").unwrap().id;
    store
        .commit(
            TurnRecord {
                campaign_id: campaign,
                op_id: "wren-1".into(),
                kind: TurnKind::Play,
                intent: "I check my satchel.".into(),
                proposal_json: "{}".into(),
                clock_before: view.clock_min,
                clock_after: view.clock_min + 5,
            },
            vec![
                set(
                    &view,
                    wren,
                    "carrying",
                    "a satchel of quills",
                    "Wren carries a satchel of quills.",
                    &[wren],
                ),
                set(
                    &view,
                    wren,
                    "watched_by",
                    "the guild",
                    "The guild has Wren watched.",
                    &[],
                ),
                LedgerOp::AdvanceClock { minutes: 5 },
            ],
        )
        .unwrap();
    let json = serde_json::to_string(&character_view(&sight_of(&store, campaign))).unwrap();
    assert_eq!(json, TOWN_CHARACTER);
}

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn view_reads_no_world_view() {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/view"),
        &mut files,
    );
    assert!(!files.is_empty());
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for word in ["WorldView", "Store"] {
            assert!(!text.contains(word), "{} names {word}", file.display());
        }
    }
}

fn view_of(w: &HiddenWorld) -> PlayView {
    play_view(&w.sight(), &Marks::default())
}

fn observe() -> String {
    record(ActionKind::Observe, Verdict::Allowed, None)
}

fn story_turn(view: &PlayView, turn: TurnId) -> &StoryTurn {
    view.story.iter().find(|t| t.turn_id == turn).unwrap()
}

fn blocks(view: &PlayView, turn: TurnId) -> &[Block] {
    match &story_turn(view, turn).body {
        TurnBody::Narration { blocks } => blocks,
        TurnBody::Missing => panic!("turn {turn} has no Narration"),
    }
}

/// The text of `parts`, as the Player reads it.
fn text(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|p| match p {
            Part::Text { text } | Part::Name { text, .. } => text.as_str(),
        })
        .collect()
}

fn prose(block: &Block) -> String {
    match block {
        Block::Prose { parts } => text(parts),
        Block::Speech { .. } => panic!("a bubble: {block:?}"),
    }
}

fn named(id: u64, kind: EntityKind, name: &str) -> NameRef {
    NameRef {
        entity_id: id,
        kind,
        name: Some(name.into()),
    }
}

/// Shape test 1 of design 11.
#[test]
fn play_view_shows_pc_knowledge_only() {
    let w = HiddenWorld::new();
    let json = serde_json::to_string(&view_of(&w)).unwrap();
    for hidden in HIDDEN {
        assert!(!json.contains(hidden), "{hidden} leaks: {json}");
    }
}

/// Shape test 2 of design 11.
#[test]
fn unnamed_character_is_shown_unknown() {
    let w = HiddenWorld::new();
    let view = view_of(&w);
    let stranger = NameRef {
        entity_id: w.kestrel,
        kind: EntityKind::Npc,
        name: None,
    };
    assert!(view.here.contains(&stranger), "{:?}", view.here);
    assert!(view.known.iter().all(|k| k.who.entity_id != w.kestrel));
    assert!(view.open.iter().all(|f| f.subject.entity_id != w.kestrel));
    assert!(
        view.known
            .iter()
            .any(|k| k.who == named(w.orsel, EntityKind::Npc, "Orsel") && k.here)
    );
    assert!(view.here.iter().all(|who| who.entity_id != w.ada));
}

/// Shape test 5 of design 11: a bubble only for a tag that resolves to one
/// NPC who is present and whom the PC can name.
#[test]
fn speech_line_without_a_certain_speaker_stays_prose() {
    let mut w = HiddenWorld::new();
    w.play("I wait.", &observe(), 0, newcomer("Brann", "market", &[]));
    let said = w.play("I listen.", &observe(), 0, vec![]);
    // Two entities share "Brann"; Pip is at the market; the PC cannot name
    // Kestrel; Ada is the PC; the PC does not know Orsel's other name, so a
    // bubble under "Orsel" would tell the Player who the Toll-Wolf is.
    let lines = [
        r#"Brann: "There are two of us.""#,
        r#"Pip: "I am at the market.""#,
        r#"Kestrel: "Nobody knows my name.""#,
        r#"Ada: "I speak for myself.""#,
        r#"The Toll-Wolf: "Pay the toll.""#,
        r#"Orsel: "You are late.""#,
    ];
    w.narrate(said, &lines.join("\n\n"));
    let view = view_of(&w);
    let got = blocks(&view, said);
    assert_eq!(got.len(), lines.len(), "{got:?}");
    for (block, line) in got.iter().zip(&lines[..5]) {
        assert_eq!(prose(block), *line);
    }
    let Block::Speech { speaker, parts } = &got[5] else {
        panic!("Orsel speaks: {:?}", got[5]);
    };
    assert_eq!(*speaker, named(w.orsel, EntityKind::Npc, "Orsel"));
    assert_eq!(text(parts), "You are late.");
    assert_eq!(
        view.here
            .iter()
            .find(|h| h.entity_id == w.kestrel)
            .unwrap()
            .name,
        None
    );
}

/// Review Focus 1: accents and case in a speech tag and in a name link.
#[test]
fn speech_tags_and_links_match_through_normalize() {
    let mut w = HiddenWorld::new();
    let ada = w.ada;
    w.play(
        "I wait.",
        &observe(),
        0,
        newcomer("Chloé", "gate square", &[ada]),
    );
    let chloe = id_of(&w.view(), "Chloé");
    let said = w.play("I greet the newcomer.", &observe(), 0, vec![]);
    w.narrate(said, "CHLOÉ: \"Bonjour.\"\n\nchloe smiles at orsel.");
    let view = view_of(&w);
    let got = blocks(&view, said);
    let Block::Speech { speaker, parts } = &got[0] else {
        panic!("Chloé speaks: {got:?}");
    };
    assert_eq!(*speaker, named(chloe, EntityKind::Npc, "Chloé"));
    assert_eq!(text(parts), "Bonjour.");
    assert_eq!(
        got[1],
        Block::Prose {
            parts: vec![
                Part::Name {
                    entity_id: chloe,
                    text: "chloe".into()
                },
                Part::Text {
                    text: " smiles at ".into()
                },
                Part::Name {
                    entity_id: w.orsel,
                    text: "orsel".into()
                },
                Part::Text { text: ".".into() },
            ]
        }
    );
    // "Rose" is no link inside "Roseanne".
    w.play(
        "I wait.",
        &observe(),
        0,
        newcomer("Rose", "gate square", &[ada]),
    );
    let more = w.play("I look around.", &observe(), 0, vec![]);
    w.narrate(more, "Roseanne waves. The stranger turns to Rose.");
    let view = view_of(&w);
    let rose = id_of(&w.view(), "Rose");
    let names: Vec<u64> = blocks(&view, more)
        .iter()
        .flat_map(|b| match b {
            Block::Prose { parts } | Block::Speech { parts, .. } => parts.clone(),
        })
        .filter_map(|p| match p {
            Part::Name { entity_id, .. } => Some(entity_id),
            Part::Text { .. } => None,
        })
        .collect();
    assert_eq!(names, [rose]);
}

/// Shape test 3 of design 11.
#[test]
fn open_item_leaves_open_in_the_turn_that_closes_it() {
    let mut w = HiddenWorld::new();
    let ada = w.ada;
    let is_open = |v: &PlayView| {
        v.open
            .iter()
            .any(|f| f.belief.subject_id == ada && f.belief.key == "pending_trade")
    };
    let view = w.view();
    w.play(
        "I offer Orsel a trade.",
        &record(ActionKind::Talk, Verdict::Allowed, None),
        0,
        vec![set(
            &view,
            ada,
            "pending_trade",
            "salt for passage",
            "Ada offered Orsel salt for passage.",
            &[ada],
        )],
    );
    assert!(is_open(&view_of(&w)));
    w.play("I wait.", &observe(), 10, vec![]);
    assert!(is_open(&view_of(&w)));
    let view = w.view();
    w.play(
        "I hand over the salt.",
        &record(ActionKind::Act, Verdict::Allowed, None),
        0,
        vec![set(
            &view,
            ada,
            "trade",
            "done",
            "Ada traded the salt.",
            &[ada],
        )],
    );
    assert!(!is_open(&view_of(&w)));
}

#[test]
fn reverted_turn_keeps_prose_only() {
    let mut w = HiddenWorld::new();
    let undone = w.play("I shout at Orsel.", &observe(), 0, vec![]);
    let line = r#"Orsel: "Not so loud.""#;
    w.narrate(undone, &format!("{line}\n\nOrsel frowns."));
    let fixed = w.revert(undone, "I speak quietly.", 0);
    let view = view_of(&w);
    let turn = story_turn(&view, undone);
    assert!(turn.reverted);
    let got = blocks(&view, undone);
    assert_eq!(got.len(), 2);
    assert_eq!(prose(&got[0]), line);
    assert_eq!(prose(&got[1]), "Orsel frowns.");
    assert!(!story_turn(&view, fixed).reverted);
    assert!(!view.can_correct, "the latest Turn is a Correction");
}

#[test]
fn you_card_holds_state_keys_only() {
    let mut w = HiddenWorld::new();
    let ada = w.ada;
    let view = w.view();
    w.play(
        "I light a lamp.",
        &record(ActionKind::Act, Verdict::Allowed, None),
        0,
        vec![
            set(&view, ada, "mood", "calm", "Ada is calm.", &[ada]),
            set(&view, ada, "lamp", "lit", "Ada holds a lit lamp.", &[ada]),
            set(&view, ada, "injury", "cut hand", "Ada has a cut hand.", &[]),
        ],
    );
    let view = view_of(&w);
    let state: Vec<(&str, &str)> = view
        .you
        .state
        .iter()
        .map(|s| (s.key.as_str(), s.value.as_str()))
        .collect();
    assert_eq!(state, [("mood", "calm")]);
    assert_eq!(view.you.name, "Ada");
    assert_eq!(view.you.concept, "Courier");
}

/// A play fact text `X: key = value` reads as `X key words: value`.
#[test]
fn play_fact_text_reads_as_a_line() {
    let mut w = HiddenWorld::new();
    let ada = w.ada;
    let view = w.view();
    w.play(
        "I promise Brann a meal.",
        &record(ActionKind::Talk, Verdict::Allowed, None),
        0,
        vec![set(
            &view,
            ada,
            "pending_meal_promise",
            "tonight",
            "Ada: pending_meal_promise = tonight",
            &[ada],
        )],
    );
    let view = view_of(&w);
    let line = view
        .open
        .iter()
        .find(|f| f.belief.key == "pending_meal_promise")
        .unwrap();
    assert_eq!(line.text, "Ada pending meal promise: tonight");
    assert!(line.open);
    assert_eq!(line.subject, named(ada, EntityKind::Pc, "Ada"));
}

/// The UI reads what the core sends: the core's JSON is kept in
/// `ui/src/fixtures/playView.core.json`, and `api.test.ts` checks it against
/// the shape of the UI fixture. Set `BLESS_PLAY_VIEW=1` to write it again.
#[test]
fn play_view_json_matches_the_ui_types() {
    let mut w = HiddenWorld::new();
    let said = w.play("I greet Orsel.", &observe(), 0, vec![]);
    w.narrate(said, "Orsel: \"You are late.\"\n\nBrann laughs at Orsel.");
    w.play("I wait.", &observe(), 0, vec![]);
    let json = serde_json::to_value(view_of(&w)).unwrap();

    let keys = |v: &Value| -> Vec<String> {
        let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(
        keys(&json),
        [
            "can_correct",
            "clock_min",
            "here",
            "known",
            "needs_begin",
            "open",
            "pc_name",
            "pinned",
            "place",
            "scene",
            "story",
            "you"
        ]
    );
    assert_eq!(json["scene"]["kind"], "plain");
    let story = json["story"].as_array().unwrap();
    let spoken = story.iter().find(|t| t["turn_id"] == said).unwrap();
    assert_eq!(spoken["kind"], "play");
    assert_eq!(spoken["body"]["kind"], "narration");
    assert_eq!(spoken["body"]["blocks"][0]["kind"], "speech");
    assert_eq!(spoken["body"]["blocks"][0]["speaker"]["kind"], "npc");
    assert_eq!(spoken["body"]["blocks"][1]["kind"], "prose");
    assert_eq!(spoken["body"]["blocks"][1]["parts"][0]["kind"], "name");
    assert_eq!(story.last().unwrap()["body"]["kind"], "missing");
    assert_eq!(
        keys(&story[0]),
        ["body", "intent", "kind", "reverted", "roll", "turn_id"]
    );

    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/fixtures/playView.core.json");
    let text = format!("{}\n", serde_json::to_string_pretty(&json).unwrap());
    if std::env::var_os("BLESS_PLAY_VIEW").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let kept = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        kept,
        text,
        "run with BLESS_PLAY_VIEW=1 to write {}",
        path.display()
    );
}
