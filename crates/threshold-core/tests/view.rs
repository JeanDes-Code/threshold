//! The screen types: each one is made from `PcSight` only.

mod support;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

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
    world::unnameable_names,
};

/// The town fixture after one Turn that gave Wren a fact Wren saw and a
/// fact only the GM knows.
const TOWN_CHARACTER: &str = r#"{"name":"Wren","concept":"An outsider scribe, hired to copy the town's charters","clock_min":485,"facts":[{"text":"Wren carries a satchel of quills."}],"open":[],"state":[{"key":"carrying","value":"satchel of quills","text":"Wren carries satchel of quills."}]}"#;

/// What it gave for the hidden world, from `WorldView`.
const HIDDEN_CHARACTER: &str = r#"{"name":"Ada","concept":"Courier","clock_min":5,"facts":[],"open":[{"belief":{"subject_id":1,"key":"pending_meet"},"subject":{"entity_id":1,"kind":"pc","name":"Ada"},"text":"Ada plans to meet Brann at the mill.","open":true,"told_by":null}],"state":[]}"#;

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

/// A narrator often tags a speaker by the first name of a longer main name:
/// `Agathe:` for Agathe Morrow. That tag resolves when the PC can name the
/// NPC. When the PC cannot, the tag goes, on the screens and in the stream
/// filter's set, as a full name would.
#[test]
fn speech_tag_by_first_name_resolves_or_is_removed() {
    let mut w = HiddenWorld::new();
    let ada = w.ada;
    let met = w.play(
        "I wait.",
        &observe(),
        0,
        newcomer("Marta Holt", "gate square", &[ada]),
    );
    w.narrate(met, "Marta Holt sets down a basket.");
    w.play(
        "I wait.",
        &observe(),
        0,
        newcomer("Sola Brandt", "gate square", &[ada]),
    );
    let said = w.play("I listen.", &observe(), 0, vec![]);
    w.narrate(
        said,
        "Marta: \"Fresh bread.\"\n\nSola: \"You do not know me.\"\n\nMarta Holt: \"Still warm.\"",
    );
    let marta = id_of(&w.view(), "Marta Holt");
    let view = view_of(&w);
    let got = blocks(&view, said);
    assert_eq!(got.len(), 3, "{got:?}");
    for (block, words) in [(&got[0], "Fresh bread."), (&got[2], "Still warm.")] {
        let Block::Speech { speaker, parts } = block else {
            panic!("Marta speaks: {block:?}");
        };
        assert_eq!(*speaker, named(marta, EntityKind::Npc, "Marta Holt"));
        assert_eq!(text(parts), words);
    }
    assert_eq!(prose(&got[1]), r#""You do not know me.""#);

    let turns = w.store.turns(w.campaign).unwrap();
    let set = unnameable_names(&w.view(), &turns, &HashSet::new());
    assert!(
        set.contains("sola") && set.contains("sola brandt"),
        "{set:?}"
    );
    assert!(
        !set.contains("marta") && !set.contains("marta holt"),
        "{set:?}"
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
    w.narrate(
        said,
        "CHLOÉ: \"Bonjour.\"\n\nChloe smiles at orsel and Orsel.",
    );
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
                    text: "Chloe".into()
                },
                // Review B1: a link needs the name's own capitals.
                Part::Text {
                    text: " smiles at orsel and ".into()
                },
                Part::Name {
                    entity_id: w.orsel,
                    text: "Orsel".into()
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

/// A play fact text `X: key = value`, as Campaign files made before bug
/// pass 3 hold it, reads as a plain sentence (`domain::slot_sentence`), the
/// `pending_` prefix left out. The Player never reads `key = value`.
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
    assert_eq!(line.text, "Ada's meal promise is now tonight.");
    let view = w.view();
    w.play(
        "I arm-wrestle Brann.",
        &record(ActionKind::Act, Verdict::Allowed, None),
        0,
        vec![set(
            &view,
            ada,
            "injury",
            "bruised_wrist",
            "Ada: injury = bruised_wrist",
            &[ada],
        )],
    );
    let sight = w.sight();
    let character = threshold_core::view::character_view(&sight);
    assert!(
        character
            .facts
            .iter()
            .any(|f| f.text == "Ada is injured (bruised wrist)."),
        "{:?}",
        character.facts
    );
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

/// B7 (QA, 2026-11-10): a place card was empty because the facts sit on
/// people. It now lists who the PC believes is located there, from the
/// PC's location Beliefs only; one the PC cannot name has no name.
#[test]
fn place_card_lists_who_the_pc_knows_is_there() {
    let (mut store, campaign) = town();
    let view = store.snapshot(campaign).unwrap();
    let id = |name: &str| view.entities.iter().find(|e| e.name == name).unwrap().id;
    let (wren, orsel, doul, hessa, tobin) = (
        id("Wren"),
        id("Orsel"),
        id("Doul"),
        id("Hessa"),
        id("Tobin"),
    );
    let (hall, archive) = (id("Varn town hall"), id("Tithe archive"));
    // A stranger the PC sees in the hall but cannot name.
    store
        .commit(
            TurnRecord {
                campaign_id: campaign,
                op_id: "stranger".into(),
                kind: TurnKind::Play,
                intent: "I look around.".into(),
                proposal_json: record(ActionKind::Observe, Verdict::Allowed, None),
                clock_before: view.clock_min,
                clock_after: view.clock_min,
            },
            newcomer("Quillon", "Varn town hall", &[wren]),
        )
        .unwrap();
    let sight = sight_of(&store, campaign);
    let quillon = store
        .snapshot(campaign)
        .unwrap()
        .entities
        .iter()
        .find(|e| e.name == "Quillon")
        .unwrap()
        .id;

    let card = entity_card(&sight, &Marks::default(), hall).expect("the PC knows the hall");
    let people: Vec<(u64, Option<String>)> = card
        .people_here
        .iter()
        .map(|p| (p.entity_id, p.name.clone()))
        .collect();
    assert_eq!(
        people,
        [
            (orsel, Some("Orsel".into())),
            (doul, Some("Doul".into())),
            (hessa, Some("Hessa".into())),
            (quillon, None),
        ]
    );
    let json = serde_json::to_string(&card).unwrap();
    assert!(!json.contains("Quillon"), "{json}");
    assert!(!people.iter().any(|(p, _)| *p == tobin || *p == wren));

    // The archive is a seed place the PC can name (bug pass 3): it has a
    // card, and the PC knows nobody there. A person lists nobody.
    let archive_card = entity_card(&sight, &Marks::default(), archive).unwrap();
    assert!(archive_card.people_here.is_empty());
    assert!(archive_card.facts.is_empty());
    let person = entity_card(&sight, &Marks::default(), orsel).unwrap();
    assert!(person.people_here.is_empty());
}

/// Bug pass 3, M4: "Harrowmere" was an Adventure Seed place, named in eight
/// facts the PC held and the target of the main request, but search in
/// People and places said "No match". The PC can name it (data shape 3.5:
/// a seed entry that is not hidden), yet `PcSight.known` held only the
/// subjects of PC Beliefs. Every entity the PC can name is now known, and
/// its card lists the facts the PC holds that name it.
#[test]
fn a_seed_place_with_no_belief_has_a_card() {
    let mut seed = support::adventure_seed();
    seed.entities.push(support::place("Harrowmere"));
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, &seed, 3).unwrap();
    let view = store.snapshot(campaign).unwrap();
    let harrowmere = view.entity_by_name("Harrowmere").unwrap().id;
    let pip = view.entity_by_name("Pip").unwrap().id;
    let ada = view.entity_by_name("Ada").unwrap().id;
    store
        .commit(
            TurnRecord {
                campaign_id: campaign,
                op_id: "told".into(),
                kind: TurnKind::Play,
                intent: "I ask Pip about his mother.".into(),
                proposal_json: record(ActionKind::Talk, Verdict::Allowed, None),
                clock_before: view.clock_min,
                clock_after: view.clock_min,
            },
            vec![set(
                &view,
                pip,
                "mother",
                "at harrowmere",
                "Pip's mother sells clay in Harrowmere.",
                &[ada],
            )],
        )
        .unwrap();
    let view = store.snapshot(campaign).unwrap();
    let turns = store.turns(campaign).unwrap();
    let sight = threshold_core::world::pc_sight(&view, &turns, &HashSet::new()).unwrap();
    assert!(
        sight.beliefs.iter().all(|b| b.subject != harrowmere),
        "no Belief is about the place"
    );
    let play = play_view(&sight, &Marks::default());
    assert!(
        play.known
            .iter()
            .any(|k| k.who == named(harrowmere, EntityKind::Place, "Harrowmere")),
        "{:?}",
        play.known
    );
    let card = entity_card(&sight, &Marks::default(), harrowmere).unwrap();
    assert!(card.facts.is_empty());
    let named_in: Vec<&str> = card.named_in.iter().map(|f| f.text.as_str()).collect();
    assert_eq!(named_in, ["Pip's mother sells clay in Harrowmere."]);
    assert_eq!(card.named_in[0].subject.entity_id, pip);
    // Pip's own card does not list its own fact again.
    let pips = entity_card(&sight, &Marks::default(), pip).unwrap();
    assert!(pips.named_in.is_empty(), "{:?}", pips.named_in);
}

/// Bug pass 3, M5: the Character screen listed closed outcome facts
/// ("Done: …", "Accepted: …") under what the PC knows about itself, and the
/// same items again under Open; it showed 4 open items while the side
/// panel showed 9, because it kept only the items about the PC. Its Open
/// list is now the side panel's (`open_lines`), and its facts hold neither
/// an open key nor the outcome of an Open Item.
#[test]
fn character_open_items_are_the_side_panel_s() {
    let mut w = HiddenWorld::new();
    let (ada, brann) = (w.ada, w.brann);
    let view = w.view();
    w.play(
        "I promise Brann a meal.",
        &record(ActionKind::Talk, Verdict::Allowed, None),
        0,
        vec![set(
            &view,
            brann,
            "pending_meal",
            "tonight",
            "Brann expects a meal from Ada tonight.",
            &[ada],
        )],
    );
    let sight = w.sight();
    let character = character_view(&sight);
    let play = play_view(&sight, &Marks::default());
    assert_eq!(character.open, play.open);
    assert_eq!(character.open.len(), 2, "{:?}", character.open);
    // Ada settles her own item: the outcome fact is on Ada.
    let view = w.view();
    w.play(
        "I meet Brann at the mill.",
        &record(ActionKind::Talk, Verdict::Allowed, None),
        0,
        vec![set(
            &view,
            ada,
            "meet",
            "done",
            "Done: Ada plans to meet Brann at the mill.",
            &[ada],
        )],
    );
    let sight = w.sight();
    let character = character_view(&sight);
    let play = play_view(&sight, &Marks::default());
    assert_eq!(character.open, play.open);
    assert_eq!(character.open.len(), 1);
    let texts: Vec<&str> = character.facts.iter().map(|f| f.text.as_str()).collect();
    assert!(!texts.iter().any(|t| t.starts_with("Done:")), "{texts:?}");
    assert!(
        !texts.iter().any(|t| t.contains("plans to meet")),
        "{texts:?}"
    );
    // The Journal still lists the closed item.
    let journal = journal_view(&sight, &Marks::default());
    assert!(
        journal
            .closed
            .iter()
            .any(|c| c.item.belief.key == "pending_meet")
    );
}

/// Bug pass 3: a state line has a sentence for the Player, never
/// `key value` as data.
#[test]
fn a_state_line_reads_as_a_sentence() {
    let mut w = HiddenWorld::new();
    let ada = w.ada;
    let view = w.view();
    w.play(
        "I arm-wrestle Brann.",
        &record(ActionKind::Act, Verdict::Allowed, None),
        0,
        vec![set(
            &view,
            ada,
            "injury",
            "bruised_wrist",
            "Ada is injured (bruised wrist).",
            &[ada],
        )],
    );
    let sight = w.sight();
    let play = play_view(&sight, &Marks::default());
    let line = play.you.state.iter().find(|s| s.key == "injury").unwrap();
    assert_eq!(line.text, "Ada is injured (bruised wrist).");
    assert_eq!(character_view(&sight).state[0].text, line.text);
}

/// Review B1: "gate" in "gate square" is no link to the place Gate; "the
/// Gate" is.
#[test]
fn a_link_needs_the_name_s_own_capitals() {
    let mut w = HiddenWorld::new();
    let said = w.play("I look around.", &observe(), 0, vec![]);
    w.narrate(said, "Rain falls on the gate square. The Gate stays shut.");
    let view = view_of(&w);
    let links: Vec<(u64, String)> = blocks(&view, said)
        .iter()
        .flat_map(|b| match b {
            Block::Prose { parts } | Block::Speech { parts, .. } => parts.clone(),
        })
        .filter_map(|p| match p {
            Part::Name { entity_id, text } => Some((entity_id, text)),
            Part::Text { .. } => None,
        })
        .collect();
    assert_eq!(links, [(w.gate, "Gate".to_owned())]);
}

/// Review minor: the side panel shows the first 5 Open Items, and they
/// came sorted by subject id. The view now lists the items about someone
/// present first, then the newest: the order a Player needs in play.
#[test]
fn open_items_list_people_here_first_then_the_newest() {
    let mut w = HiddenWorld::new();
    let (ada, orsel, pip) = (w.ada, w.orsel, w.pip);
    let view = w.view();
    w.play(
        "I listen.",
        &observe(),
        0,
        vec![set(
            &view,
            orsel,
            "pending_toll",
            "unpaid",
            "Orsel wants the toll paid by dusk.",
            &[ada, orsel],
        )],
    );
    let view = w.view();
    w.play(
        "I listen.",
        &observe(),
        0,
        vec![set(
            &view,
            pip,
            "pending_errand",
            "asked",
            "Pip asked Ada to fetch a lantern.",
            &[ada],
        )],
    );
    let order: Vec<(u64, String)> = view_of(&w)
        .open
        .iter()
        .map(|f| (f.belief.subject_id, f.belief.key.clone()))
        .collect();
    assert_eq!(
        order,
        [
            (orsel, "pending_toll".to_owned()),
            (pip, "pending_errand".to_owned()),
            (ada, "pending_meet".to_owned()),
        ]
    );
}

/// Bug pass 4, B1, on the hidden world: a World Event that fires in front of
/// the PC, seen by Brann only, writes Orsel's debt. The PC witnesses the
/// event, not its hidden effect: no screen shows the debt.
#[test]
fn a_witnessed_event_s_hidden_effect_reaches_no_screen() {
    use threshold_core::{
        RecordedGm,
        domain::{Effect, EntityRef, Slot, Trigger},
    };
    let mut w = HiddenWorld::new();
    let (orsel, brann) = (w.orsel, w.brann);
    w.commit(
        TurnKind::Setup,
        "",
        "{}",
        0,
        vec![LedgerOp::ScheduleEvent {
            text: "Orsel counts his silver".into(),
            trigger: Trigger::At(10),
            actors: vec![EntityRef::Id(orsel)],
            goal_id: None,
            cause_fact_id: None,
            effects: vec![Effect::SetFact {
                slot: Slot {
                    subject: EntityRef::Id(orsel),
                    key: "debt".into(),
                    value: "partially paid".into(),
                },
                text: "Orsel's debt is now partially paid.".into(),
                seen_by: vec![],
            }],
            seen_by: vec![EntityRef::Id(brann)],
        }],
    );
    let mut wait = support::proposal(Verdict::Allowed);
    wait.action = ActionKind::Wait;
    let gm = RecordedGm::new(
        vec![serde_json::to_string(&wait).unwrap()],
        vec!["Orsel counts coins in the corner.".into()],
    );
    let (mut engine, campaign) = w.engine(&gm);
    engine
        .play_turn("wait".into(), "I wait a while.", &mut |_| {})
        .unwrap();
    let view = engine.store().snapshot(campaign).unwrap();
    assert!(view.live_fact(orsel, "debt").is_some(), "the event fired");
    let sight = sight_of(engine.store(), campaign);
    let marks = Marks::default();
    let screens = [
        serde_json::to_string(&play_view(&sight, &marks)).unwrap(),
        serde_json::to_string(&journal_view(&sight, &marks)).unwrap(),
        serde_json::to_string(&character_view(&sight)).unwrap(),
        serde_json::to_string(&entity_card(&sight, &marks, orsel).unwrap()).unwrap(),
    ];
    for json in screens {
        assert!(!json.contains("partially paid"), "the debt leaks: {json}");
    }
}

/// Bug pass 4, MB: the OOC prompt holds what the Play screen shows, from
/// `PcSight` only: the place, the clock in the Player's words, who is here
/// by the names the PC knows, the open and the settled items, the recent
/// Turns. It holds no hidden fact.
#[test]
fn the_ooc_prompt_holds_the_play_screen_and_no_hidden_fact() {
    use threshold_core::{RecordedGm, view::clock_text};
    let mut w = HiddenWorld::new();
    let (ada, meet_on) = (
        w.ada,
        w.view()
            .live_fact(w.ada, "pending_meet")
            .unwrap()
            .slot
            .subject,
    );
    let view = w.view();
    let settled = w.play(
        "I tell Brann: no, I will not meet you at the mill.",
        &observe(),
        2,
        vec![set(
            &view,
            meet_on,
            "meet",
            "refused",
            "Refused: Ada plans to meet Brann at the mill.",
            &[ada],
        )],
    );
    w.narrate(settled, "Brann: \"As you like.\"");
    let gm = RecordedGm::new(vec![], vec!["You are at the gate square.".into()]);
    let clock = w.view().clock_min;
    let (mut engine, _campaign) = w.engine(&gm);
    engine
        .ooc(
            "ooc-1".into(),
            "Who is here, what time is it, and what did I refuse?",
        )
        .unwrap();
    let prompt = &gm.calls()[0].prompt;
    let all = [&prompt.stable, &prompt.scene, &prompt.recent]
        .map(|s| s.as_str())
        .concat();
    assert!(all.contains("Place: gate square"), "{all}");
    assert!(
        all.contains(&format!("Time: {}", clock_text(clock))),
        "{all}"
    );
    assert_eq!(clock_text(clock), "Day 1, 00:07");
    assert!(
        all.contains("Here with the PC: Orsel, Brann, someone the PC cannot name"),
        "{all}"
    );
    assert!(all.contains("SETTLED ITEMS"), "{all}");
    assert!(
        all.contains("- Refused: Ada plans to meet Brann at the mill."),
        "{all}"
    );
    // The closed item is listed once, as settled, never as open.
    assert!(!all.contains("(open, not done) Ada plans"), "{all}");
    assert_eq!(all.matches("Ada plans to meet Brann").count(), 1, "{all}");
    assert!(
        all.contains("Player: I tell Brann: no, I will not meet you at the mill."),
        "{all}"
    );
    assert!(all.contains("Brann: \"As you like.\""), "{all}");
    for hidden in HIDDEN {
        assert!(!all.contains(hidden), "{hidden} leaks: {all}");
    }
}
