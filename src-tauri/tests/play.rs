//! The Play view command: `get_play_view` over a real Campaign file.

use std::{path::Path, time::Instant};

mod support;

use support::{allowed, entity, session, town, town_dir, with_fact};
use threshold_core::{
    RecordedGm,
    store::TurnKind,
    view::{Block, PLAY_VIEW_MAX_MS, TurnBody},
};

/// Data shape 3.6: one call returns the whole story, so it must stay fast as
/// the Campaign grows. A debug build gets ten times the budget.
#[test]
fn play_view_builds_within_budget_at_300_turns() {
    const TURNS: usize = 300;
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;
    let wren = entity(&db, "Wren");
    let fatigue = ["rested", "tired", "weary", "spent"];
    for i in 0..TURNS {
        if i % 5 == 0 {
            gm.push_proposal(with_fact(
                wren,
                "fatigue",
                fatigue[i / 5 % fatigue.len()],
                None,
            ));
        } else {
            gm.push_proposal(allowed());
        }
        gm.push_narration(format!(
            "Orsel looks up from the ledger, and Doul shifts by the door.\n\n\
             Orsel: \"Turn {i}. Keep copying.\"\n\n\
             Hessa watches from the gallery of the Varn town hall."
        ));
        s.submit_intent(format!("op-{i}"), "I keep copying.", &mut |_| {})
            .unwrap();
    }

    let start = Instant::now();
    let view = s.get_play_view().unwrap();
    let ms = start.elapsed().as_millis() as u64;

    assert_eq!(view.story.len(), TURNS);
    let budget = if cfg!(debug_assertions) {
        PLAY_VIEW_MAX_MS * 10
    } else {
        PLAY_VIEW_MAX_MS
    };
    eprintln!("get_play_view at {TURNS} Turns: {ms} ms (budget {budget} ms)");
    assert!(ms <= budget, "{ms} ms over the budget of {budget} ms");
}

/// Review Focus 3: probing `get_entity_card` with ids must not tell a hidden
/// entity from an id that does not exist.
#[test]
fn card_refusal_does_not_tell_hidden_from_missing() {
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("adventure-seed.toml");
    let mut text = std::fs::read_to_string(town_dir().join("adventure-seed.toml")).unwrap();
    text.push_str(
        "\n[[entities]]\nname = \"Vey\"\nkind = \"npc\"\nage = 40\nhidden = true\n\n\
         [[decided]]\nsubject = \"Vey\"\nkey = \"location\"\nvalue = \"Varn town hall\"\n\
         text = \"Vey waits unseen behind the town hall's screen.\"\n",
    );
    std::fs::write(&seed, text).unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    s.new_campaign(&seed, &town_dir().join("world.toml"), 11)
        .unwrap();
    let db = s.summary().unwrap().db_path;

    let vey = s
        .get_entity_card(entity(&db, "Vey"))
        .unwrap_err()
        .to_string();
    let missing = s.get_entity_card(99_999).unwrap_err().to_string();
    assert_eq!(vey, missing);
    assert_eq!(vey, "the PC does not know this one");

    let orsel = s.get_entity_card(entity(&db, "Orsel")).unwrap();
    assert_eq!(orsel.who.name.as_deref(), Some("Orsel"));
}

/// The Journal over a real file: a Scene opens at Begin, each Turn adds its
/// Intent, and a reopened file gives the same Journal.
#[test]
fn get_journal_after_begin_and_turns() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(
        vec![allowed(), allowed()],
        vec![
            "The town hall is quiet.".into(),
            "Orsel looks up.".into(),
            "Hessa writes on.".into(),
        ],
    );
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;
    assert!(s.get_journal().unwrap().scenes.is_empty());

    let opening = s.begin(&mut |_| {}).unwrap().turn_id;
    s.submit_intent("op-1".into(), "I greet the magistrate.", &mut |_| {})
        .unwrap();
    s.submit_intent("op-2".into(), "I sit down to copy.", &mut |_| {})
        .unwrap();
    let journal = s.get_journal().unwrap();
    assert_eq!(journal.scenes.len(), 1, "{:?}", journal.scenes);
    let scene = &journal.scenes[0];
    assert_eq!(scene.turn_id, opening);
    assert_eq!(scene.place, "Varn town hall");
    let intents: Vec<&str> = scene.acts.iter().map(|a| a.intent.as_str()).collect();
    assert_eq!(intents, ["I greet the magistrate.", "I sit down to copy."]);
    assert!(
        scene
            .present
            .iter()
            .any(|who| who.entity_id == entity(&db, "Orsel"))
    );

    let mut reopened = session(dir.path(), &gm);
    reopened.open_campaign(Path::new(&db)).unwrap();
    assert_eq!(reopened.get_journal().unwrap(), journal);
}

#[test]
fn get_play_view_after_open_and_after_a_turn() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(
        vec![allowed()],
        vec![
            "The town hall is quiet.".into(),
            "Orsel: \"Sit down, scribe.\"".into(),
        ],
    );
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;

    let view = s.get_play_view().unwrap();
    assert!(view.needs_begin);
    assert!(view.story.is_empty());
    assert_eq!(view.pc_name, "Wren");
    // The place reads as the place entity's name, not the stored value.
    let place = view.place.as_ref().unwrap();
    assert_eq!(place.entity_id, Some(entity(&db, "Varn town hall")));
    assert_eq!(place.name, "Varn town hall");

    s.begin(&mut |_| {}).unwrap();
    let view = s.get_play_view().unwrap();
    assert!(!view.needs_begin);
    assert_eq!(view.story.len(), 1);
    assert_eq!(view.story[0].kind, TurnKind::Opening);

    let turn = s
        .submit_intent("op-1".into(), "I sit down.", &mut |_| {})
        .unwrap();
    let view = s.get_play_view().unwrap();
    assert_eq!(view.story.len(), 2);
    let last = view.story.last().unwrap();
    assert_eq!(last.turn_id, turn.turn_id);
    assert_eq!(last.intent, "I sit down.");
    assert!(view.can_correct);
    let TurnBody::Narration { blocks } = &last.body else {
        panic!("the Turn has its Narration: {last:?}");
    };
    let Block::Speech { speaker, .. } = &blocks[0] else {
        panic!("Orsel is present and named: {blocks:?}");
    };
    assert_eq!(speaker.entity_id, entity(&db, "Orsel"));

    // A reopened file gives the same view.
    let mut reopened = session(dir.path(), &gm);
    reopened.open_campaign(Path::new(&db)).unwrap();
    assert_eq!(reopened.get_play_view().unwrap(), view);
}
