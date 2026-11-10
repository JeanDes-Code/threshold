//! The screen types: each one is made from `PcSight` only.

mod support;

use std::path::{Path, PathBuf};

use serde_json::Value;
use support::hidden::*;
use threshold_core::{
    domain::{ActionKind, EntityKind, LedgerOp, Verdict},
    fixture::{load_adventure_seed, load_world},
    player::{Marks, Note, Target},
    store::{Store, TurnId, TurnKind, TurnRecord},
    turn::create_campaign,
    view::{
        Block, JournalView, NameRef, Part, PlayView, StoryTurn, TurnBody, character_view,
        entity_card, journal_view, pins_resolving_to, play_view,
    },
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

/// Shape test 5 of design 11, with the rule that changed after review
/// (data shape 3.1, ADR 0006): a bubble only for a tag that resolves to one
/// NPC who is present and whom the PC can name. A tag that names an entity
/// the PC cannot name by that name is removed, so the line keeps only its
/// quoted words. Any other tag stays as the narrator wrote it.
#[test]
fn speech_line_without_a_certain_speaker_stays_prose() {
    let mut w = HiddenWorld::new();
    w.play("I wait.", &observe(), 0, newcomer("Brann", "market", &[]));
    let said = w.play("I listen.", &observe(), 0, vec![]);
    // (line, the prose the Player reads). Two named entities share "Brann";
    // Pip is a seed entry the Player read, but at the market; Kestrel and
    // the hidden Vey have no name the PC knows; Ada is the PC; the PC does
    // not know Orsel's other name; "Thunder" names no entity.
    let lines = [
        (
            r#"Brann: "There are two of us.""#,
            r#"Brann: "There are two of us.""#,
        ),
        (
            r#"Pip: "I am at the market.""#,
            r#"Pip: "I am at the market.""#,
        ),
        (
            r#"Kestrel: "Nobody knows my name.""#,
            r#""Nobody knows my name.""#,
        ),
        (
            r#"Ada: "I speak for myself.""#,
            r#"Ada: "I speak for myself.""#,
        ),
        (r#"The Toll-Wolf: "Pay the toll.""#, r#""Pay the toll.""#),
        (
            "Vey: \u{ab}Stay in the light.\u{bb}",
            "\u{ab}Stay in the light.\u{bb}",
        ),
        (r#"Thunder: "Boom.""#, r#"Thunder: "Boom.""#),
        (r#"Orsel: "You are late.""#, ""),
    ];
    let written: Vec<&str> = lines.iter().map(|(line, _)| *line).collect();
    w.narrate(said, &written.join("\n\n"));
    let view = view_of(&w);
    let got = blocks(&view, said);
    assert_eq!(got.len(), lines.len(), "{got:?}");
    for (block, (line, shown)) in got.iter().zip(&lines[..lines.len() - 1]) {
        assert_eq!(prose(block), *shown, "{line}");
    }
    let Block::Speech { speaker, parts } = &got[lines.len() - 1] else {
        panic!("Orsel speaks: {:?}", got[lines.len() - 1]);
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

    // No screen shows a name that the marks wrote and the PC cannot name.
    // Pip is in `HIDDEN` as an absent NPC; the Player read that name in
    // the review, so a tag may print it.
    let sight = w.sight();
    let marks = Marks::default();
    let mut screens = vec![
        serde_json::to_string(&play_view(&sight, &marks)).unwrap(),
        serde_json::to_string(&journal_view(&sight, &marks)).unwrap(),
    ];
    screens.extend(
        sight
            .known
            .iter()
            .filter_map(|k| entity_card(&sight, &marks, k.id))
            .map(|card| serde_json::to_string(&card).unwrap()),
    );
    for json in &screens {
        for hidden in HIDDEN.iter().filter(|h| **h != "Pip") {
            assert!(!json.contains(hidden), "{hidden} leaks: {json}");
        }
    }
}

/// A tag shared by a name the PC knows and a name it does not know is
/// removed too: a bubble or a tag could tell the Player that both exist.
#[test]
fn tag_shared_with_a_name_the_pc_cannot_name_is_removed() {
    let mut w = HiddenWorld::new();
    let ada = w.ada;
    w.play(
        "I wait.",
        &observe(),
        0,
        newcomer("Vey", "gate square", &[ada]),
    );
    let view = w.view();
    let other = view
        .entities
        .iter()
        .filter(|e| e.name == "Vey")
        .map(|e| e.id)
        .max()
        .unwrap();
    assert_ne!(other, w.vey);
    w.play(
        "I ask the newcomer what she sells.",
        &observe(),
        0,
        vec![set(
            &view,
            other,
            "sells",
            "salt",
            "She sells salt.",
            &[ada],
        )],
    );
    let said = w.play("I listen.", &observe(), 0, vec![]);
    w.narrate(said, r#"Vey: "Salt, fresh salt.""#);
    let view = view_of(&w);
    let got = blocks(&view, said);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(prose(&got[0]), r#""Salt, fresh salt.""#);
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

/// Shape 3.6: a card holds what the PC knows about the entity, and no more.
#[test]
fn entity_card_shows_pc_knowledge_only() {
    let mut w = HiddenWorld::new();
    let (ada, orsel) = (w.ada, w.orsel);
    let view = w.view();
    w.play(
        "I ask Orsel the price of passage.",
        &record(ActionKind::Talk, Verdict::Allowed, None),
        0,
        vec![set(
            &view,
            orsel,
            "pending_toll",
            "two coins at dusk",
            "Orsel wants two coins at dusk.",
            &[ada],
        )],
    );
    let sight = w.sight();
    let card = entity_card(&sight, &Marks::default(), orsel).expect("the PC knows Orsel");
    let json = serde_json::to_string(&card).unwrap();
    for hidden in HIDDEN {
        assert!(!json.contains(hidden), "{hidden} leaks: {json}");
    }
    assert_eq!(card.who, named(orsel, EntityKind::Npc, "Orsel"));
    assert!(card.here);
    assert!(
        card.other_names.is_empty(),
        "the PC does not know the Toll-Wolf"
    );
    // Its facts are the PC Beliefs about Orsel, open items marked.
    let mut want: Vec<&str> = sight
        .beliefs
        .iter()
        .filter(|b| b.subject == orsel)
        .map(|b| b.key.as_str())
        .collect();
    let mut got: Vec<&str> = card.facts.iter().map(|f| f.belief.key.as_str()).collect();
    want.sort_unstable();
    got.sort_unstable();
    assert_eq!(got, want);
    assert!(got.contains(&"pending_toll"), "{got:?}");
    for fact in &card.facts {
        assert_eq!(fact.subject, card.who);
        assert_eq!(fact.open, fact.belief.key == "pending_toll", "{fact:?}");
    }
    assert!(card.notes.is_empty());
    assert!(!card.pinned);
}

/// Shape 3.6: no card for an entity the PC does not know.
#[test]
fn entity_card_refuses_an_entity_the_pc_does_not_know() {
    let w = HiddenWorld::new();
    let sight = w.sight();
    let marks = Marks::default();
    let beyond = w.view().entities.iter().map(|e| e.id).max().unwrap() + 1;
    for id in [w.vey, w.pip, beyond] {
        assert_eq!(entity_card(&sight, &marks, id), None, "entity {id}");
    }
    assert!(entity_card(&sight, &marks, w.orsel).is_some());
}

/// A character the PC perceives and cannot name has a card with no text.
#[test]
fn stranger_card_holds_no_text() {
    let w = HiddenWorld::new();
    let card = entity_card(&w.sight(), &Marks::default(), w.kestrel).expect("the PC sees them");
    assert_eq!(card.who.name, None);
    assert!(card.facts.is_empty(), "{:?}", card.facts);
    assert!(card.other_names.is_empty());
    assert!(card.here);
    let json = serde_json::to_string(&card).unwrap();
    assert!(!json.contains("Kestrel"), "{json}");
}

/// Every entity the Play view links (a name part, a speaker, a `here` line,
/// a `known` line) opens a card: the core gives one for each id it shows.
#[test]
fn every_entity_the_view_links_has_a_card() {
    let mut w = HiddenWorld::new();
    let said = w.play("I listen.", &observe(), 0, vec![]);
    w.narrate(said, "Orsel: \"Brann, the gate.\"\n\nBrann nods.");
    let sight = w.sight();
    let marks = Marks::default();
    let view = play_view(&sight, &marks);
    let mut linked: Vec<u64> = view
        .here
        .iter()
        .map(|who| who.entity_id)
        .chain(view.known.iter().map(|line| line.who.entity_id))
        .collect();
    for turn in &view.story {
        if let TurnBody::Narration { blocks } = &turn.body {
            for block in blocks {
                let parts = match block {
                    Block::Prose { parts } => parts,
                    Block::Speech { speaker, parts } => {
                        linked.push(speaker.entity_id);
                        parts
                    }
                };
                linked.extend(parts.iter().filter_map(|p| match p {
                    Part::Name { entity_id, .. } => Some(*entity_id),
                    Part::Text { .. } => None,
                }));
            }
        }
    }
    for id in [w.orsel, w.brann, w.kestrel] {
        assert!(linked.contains(&id), "{id} is linked: {linked:?}");
    }
    for id in linked {
        let card = entity_card(&sight, &marks, id).unwrap_or_else(|| panic!("no card for {id}"));
        assert_eq!(card.who.entity_id, id);
    }
}

/// The card joins the Player's Notes about the entity and its Pin.
#[test]
fn entity_card_joins_its_notes_and_pin() {
    let w = HiddenWorld::new();
    let note = |id: u64, about: Target| Note {
        id,
        body: format!("note {id}"),
        about: Some(about),
        created_ms: 1,
        updated_ms: 1,
    };
    let marks = Marks {
        notes: vec![
            note(1, Target::Entity { entity_id: w.orsel }),
            note(2, Target::Entity { entity_id: w.brann }),
        ],
        pins: vec![Target::Entity { entity_id: w.orsel }],
    };
    let card = entity_card(&w.sight(), &marks, w.orsel).unwrap();
    assert_eq!(card.notes.iter().map(|n| n.id).collect::<Vec<_>>(), [1]);
    assert!(card.pinned);
    let card = entity_card(&w.sight(), &marks, w.brann).unwrap();
    assert!(!card.pinned);
}

fn journal_of(w: &HiddenWorld) -> JournalView {
    journal_view(&w.sight(), &Marks::default())
}

/// The hidden world, then: Ada learns what Brann trades in, offers Orsel a
/// trade and closes it, walks to the market and meets Tamsin there.
struct Story {
    w: HiddenWorld,
    first: TurnId,
    second: TurnId,
    talk: TurnId,
    offer: TurnId,
    trade: TurnId,
    walk: TurnId,
    tamsin: u64,
}

fn story() -> Story {
    let mut w = HiddenWorld::new();
    let (ada, brann) = (w.ada, w.brann);
    let turns: Vec<TurnId> = w
        .store
        .turns(w.campaign)
        .unwrap()
        .iter()
        .filter(|t| t.record.kind == TurnKind::Play)
        .map(|t| t.id)
        .collect();
    let view = w.view();
    let talk = w.play(
        "I ask Brann what he sells.",
        &record(ActionKind::Talk, Verdict::Allowed, None),
        10,
        vec![set(
            &view,
            brann,
            "trade",
            "rope",
            "Brann trades in rope.",
            &[ada],
        )],
    );
    let view = w.view();
    let offer = w.play(
        "I offer Orsel salt for passage.",
        &record(ActionKind::Talk, Verdict::Allowed, None),
        5,
        vec![set(
            &view,
            ada,
            "pending_trade",
            "salt for passage",
            "Ada offered Orsel salt for passage.",
            &[ada],
        )],
    );
    let view = w.view();
    let trade = w.play(
        "I hand over the salt.",
        &record(ActionKind::Act, Verdict::Allowed, None),
        5,
        vec![set(
            &view,
            ada,
            "trade",
            "done",
            "Ada traded the salt.",
            &[ada],
        )],
    );
    let view = w.view();
    let mut ops = vec![set(
        &view,
        ada,
        "location",
        "market",
        "Ada walks to the market.",
        &[ada],
    )];
    ops.extend(newcomer("Tamsin", "market", &[ada]));
    let walk = w.play(
        "I walk to the market.",
        &record(ActionKind::Move, Verdict::Allowed, None),
        20,
        ops,
    );
    w.narrate(walk, "Tamsin sells salt under a striped awning.");
    let tamsin = id_of(&w.view(), "Tamsin");
    Story {
        first: turns[0],
        second: turns[1],
        w,
        talk,
        offer,
        trade,
        walk,
        tamsin,
    }
}

/// Shape 3.6: the Journal holds what the PC knows, and no more.
#[test]
fn journal_view_shows_pc_knowledge_only() {
    let s = story();
    let journal = journal_of(&s.w);
    let json = serde_json::to_value(&journal).unwrap();
    let text = json.to_string();
    for hidden in HIDDEN {
        assert!(!text.contains(hidden), "{hidden} leaks: {text}");
    }
    let keys = |v: &Value| -> Vec<String> {
        let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(
        keys(&json),
        ["closed", "known_before", "notes", "open", "pins", "scenes"]
    );
    assert_eq!(
        keys(&json["scenes"][0]),
        [
            "acts",
            "clock_from",
            "clock_to",
            "learned",
            "place",
            "present",
            "turn_id"
        ]
    );
    assert_eq!(
        keys(&json["scenes"][0]["acts"][0]),
        ["decision", "intent", "turn_id"]
    );
    assert_eq!(keys(&json["closed"][0]), ["closed_in", "item"]);
}

/// Shape 3.3: what the PC knew from the start is "Known before the
/// Campaign", and no Scene lists it as learned.
#[test]
fn seed_knowledge_is_known_before() {
    let s = story();
    let journal = journal_of(&s.w);
    let meet = journal
        .known_before
        .iter()
        .find(|f| f.belief.key == "pending_meet")
        .expect("the PC knew its plan before the Campaign");
    assert_eq!(meet.text, "Ada plans to meet Brann at the mill.");
    let before: Vec<_> = journal.known_before.iter().map(|f| &f.belief).collect();
    for scene in &journal.scenes {
        for line in &scene.learned {
            assert!(!before.contains(&&line.belief), "{line:?} in {scene:?}");
        }
    }
}

#[test]
fn scene_entry_holds_place_time_people_acts_and_learned() {
    let s = story();
    let sight = s.w.sight();
    let journal = journal_of(&s.w);
    assert_eq!(journal.scenes.len(), 2, "{:#?}", journal.scenes);
    let (gate, market) = (&journal.scenes[0], &journal.scenes[1]);

    // Place and time: the first and the last Moment of each Scene.
    let clock = |turn: TurnId| {
        sight
            .moments
            .iter()
            .find(|m| m.turn == turn)
            .unwrap()
            .clock_after
    };
    assert_eq!(gate.turn_id, s.w.opening);
    assert_eq!(gate.place, "gate square");
    assert_eq!(gate.clock_from, clock(s.w.opening));
    assert_eq!(gate.clock_to, clock(s.trade));
    assert_eq!(market.turn_id, s.walk);
    assert_eq!(market.place, "market");
    assert_eq!(
        (market.clock_from, market.clock_to),
        (clock(s.walk), clock(s.walk))
    );
    assert!(gate.clock_from < gate.clock_to && gate.clock_to < market.clock_from);

    // Who was there, the PC left out; a stranger stays unnamed.
    let ids = |scene: &threshold_core::view::SceneEntry| -> Vec<u64> {
        scene.present.iter().map(|who| who.entity_id).collect()
    };
    assert!(!ids(gate).contains(&s.w.ada) && !ids(market).contains(&s.w.ada));
    for id in [s.w.orsel, s.w.brann, s.w.kestrel] {
        assert!(ids(gate).contains(&id), "{:?}", gate.present);
    }
    assert_eq!(ids(market), [s.tamsin]);
    let stranger = gate.present.iter().find(|w| w.entity_id == s.w.kestrel);
    assert_eq!(stranger.unwrap().name, None);

    // What the PC did: each Intent, the Opening left out.
    let acts: Vec<(TurnId, &str)> = gate
        .acts
        .iter()
        .map(|a| (a.turn_id, a.intent.as_str()))
        .collect();
    assert_eq!(
        acts,
        [
            (s.first, "I wait by the gate."),
            (s.second, "I ask Brann about the road."),
            (s.talk, "I ask Brann what he sells."),
            (s.offer, "I offer Orsel salt for passage."),
            (s.trade, "I hand over the salt."),
        ]
    );
    let decision = |turn| {
        gate.acts
            .iter()
            .find(|a| a.turn_id == turn)
            .unwrap()
            .decision
    };
    assert!(!decision(s.first) && decision(s.trade));
    assert_eq!(market.acts.len(), 1);
    assert_eq!(market.acts[0].intent, "I walk to the market.");

    // What the PC learned, in the Scene where it learned it.
    let learned = |scene: &threshold_core::view::SceneEntry, key: &str, subject: u64| {
        scene
            .learned
            .iter()
            .any(|f| f.belief.key == key && f.belief.subject_id == subject)
    };
    assert!(learned(gate, "trade", s.w.brann));
    assert!(!learned(market, "trade", s.w.brann));
    // Tamsin's place is presence, not something learned.
    assert!(!learned(market, "location", s.tamsin));
}

#[test]
fn closed_item_names_the_turn_that_closed_it() {
    let s = story();
    let journal = journal_of(&s.w);
    let closed = journal
        .closed
        .iter()
        .find(|c| c.item.belief.key == "pending_trade")
        .expect("the trade is done");
    assert_eq!(closed.closed_in, s.trade);
    assert_eq!(closed.item.text, "Ada offered Orsel salt for passage.");
    assert!(!closed.item.open);
    assert!(journal.open.iter().all(|f| f.belief.key != "pending_trade"));
    assert!(journal.open.iter().any(|f| f.belief.key == "pending_meet"));
    assert!(
        journal
            .closed
            .iter()
            .all(|c| c.item.belief.key != "pending_meet")
    );
}

#[test]
fn reverted_turn_is_in_no_scene_entry() {
    let mut w = HiddenWorld::new();
    let undone = w.play("I shout at Orsel.", &observe(), 0, vec![]);
    let fixed = w.revert(undone, "I speak quietly to Orsel.", 0);
    let journal = journal_of(&w);
    let turns: Vec<TurnId> = journal
        .scenes
        .iter()
        .flat_map(|s| std::iter::once(s.turn_id).chain(s.acts.iter().map(|a| a.turn_id)))
        .collect();
    assert!(!turns.contains(&undone), "{turns:?}");
    assert!(turns.contains(&fixed), "{turns:?}");
    let intents: Vec<&str> = journal
        .scenes
        .iter()
        .flat_map(|s| s.acts.iter().map(|a| a.intent.as_str()))
        .collect();
    assert!(!intents.contains(&"I shout at Orsel."), "{intents:?}");
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

/// Review Focus 5: a Pin and a Note on a Turn that a Correction reverts
/// follow the Correction Turn. A Pin on a Belief the Correction removed
/// hides, and its Note shows as a free Note.
#[test]
fn pin_follows_the_correction_and_lost_target_hides() {
    let mut w = HiddenWorld::new();
    let ada = w.ada;
    let view = w.view();
    let undone = w.play(
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
    // The Player writes the key in another case: one target all the same.
    let belief = Target::Belief {
        subject_id: ada,
        key: "Pending_Trade".into(),
    };
    let note = |id: u64, about: Target| Note {
        id,
        body: format!("note {id}"),
        about: Some(about),
        created_ms: 1,
        updated_ms: 1,
    };
    let marks = Marks {
        notes: vec![
            note(1, Target::Act { turn_id: undone }),
            note(2, belief.clone()),
            note(3, Target::Scene { turn_id: undone }),
        ],
        pins: vec![
            Target::Act { turn_id: undone },
            belief.clone(),
            Target::Note { note_id: 2 },
        ],
    };
    // Before the Correction every target resolves.
    let pinned: Vec<Target> = play_view(&w.sight(), &marks)
        .pinned
        .into_iter()
        .map(|p| p.target)
        .collect();
    assert_eq!(
        pinned,
        [
            Target::Act { turn_id: undone },
            Target::Belief {
                subject_id: ada,
                key: "pending_trade".into()
            },
            Target::Note { note_id: 2 },
        ]
    );

    let fixed = w.revert(undone, "I only greet Orsel.", 0);
    let sight = w.sight();
    let view = play_view(&sight, &marks);
    let pinned: Vec<&Target> = view.pinned.iter().map(|p| &p.target).collect();
    assert_eq!(
        pinned,
        [
            &Target::Act { turn_id: fixed },
            &Target::Note { note_id: 2 }
        ]
    );
    assert_eq!(view.pinned[0].title, "I only greet Orsel.");
    assert_eq!(view.pinned[1].text, "note 2");

    let journal = journal_view(&sight, &marks);
    assert_eq!(
        journal.pins,
        [Target::Act { turn_id: fixed }, Target::Note { note_id: 2 }]
    );
    let about: Vec<Option<Target>> = journal.notes.iter().map(|n| n.about.clone()).collect();
    let scene_of_fixed = journal
        .scenes
        .iter()
        .find(|s| s.acts.iter().any(|a| a.turn_id == fixed))
        .unwrap()
        .turn_id;
    assert_eq!(
        about,
        [
            Some(Target::Act { turn_id: fixed }),
            None,
            Some(Target::Scene {
                turn_id: scene_of_fixed
            }),
        ]
    );
    // The stored marks are not changed: the lost target is not lost.
    assert_eq!(marks.notes[1].about, Some(belief));

    // Removing the Pin as the screen shows it finds the stored Pin.
    assert_eq!(
        pins_resolving_to(&sight, &marks, &Target::Act { turn_id: fixed }),
        [Target::Act { turn_id: undone }]
    );
}
