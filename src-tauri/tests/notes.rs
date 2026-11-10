//! Notes and Pins through the shell: they last, and they never reach a
//! prompt (data shape 3.7).

use std::path::Path;

mod support;

use support::{allowed, entity, session, town};
use threshold_core::{RecordedGm, player::Target};

/// Shape test 4a of design 11.
#[test]
fn note_and_pin_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;
    let orsel = entity(&db, "Orsel");
    let about = Target::Entity { entity_id: orsel };
    let note = s
        .add_note("Orsel keeps the ledger close.", Some(about.clone()))
        .unwrap();
    s.set_pin(&about, true).unwrap();
    s.set_pin(&Target::Note { note_id: note.id }, true).unwrap();
    drop(s);

    let mut s = session(dir.path(), &gm);
    s.open_campaign(Path::new(&db)).unwrap();
    let pinned: Vec<Target> = s
        .get_play_view()
        .unwrap()
        .pinned
        .into_iter()
        .map(|p| p.target)
        .collect();
    assert_eq!(pinned, [about, Target::Note { note_id: note.id }]);
    let card = s.get_entity_card(orsel).unwrap();
    assert_eq!(card.notes, [note.clone()]);
    assert!(card.pinned);
    assert_eq!(s.get_journal().unwrap().notes, [note]);
}

/// Shape test 4b of design 11: a Note never reaches a prompt. A marker in
/// a Note, then one Turn and one OOC question: no prompt the model got and
/// no debug row holds it.
#[test]
fn note_text_is_in_no_prompt_of_the_next_turn() {
    const MARKER: &str = "MARKER-7f3a";
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;
    let orsel = Target::Entity {
        entity_id: entity(&db, "Orsel"),
    };
    let free = s.add_note(&format!("{MARKER} free"), None).unwrap();
    s.add_note(&format!("{MARKER} about Orsel"), Some(orsel.clone()))
        .unwrap();
    s.set_pin(&orsel, true).unwrap();
    s.set_pin(&Target::Note { note_id: free.id }, true).unwrap();
    let turn = s.get_play_view().unwrap().story.len();

    gm.push_proposal(allowed());
    gm.push_narration("Orsel looks up from the ledger.".into());
    s.submit_intent("op-marker".into(), "I copy the charter.", &mut |_| {})
        .unwrap();
    gm.push_narration("The archive closes at dusk.".into());
    s.ask_ooc("When does the archive close?").unwrap();
    assert_eq!(s.get_play_view().unwrap().story.len(), turn + 1);

    let calls = gm.calls();
    assert_eq!(calls.len(), 3, "a proposal, a Narration and an OOC answer");
    for call in &calls {
        let p = &call.prompt;
        for field in [&p.system, &p.stable, &p.scene, &p.recent] {
            assert!(
                !field.contains(MARKER),
                "{:?} prompt holds the Note",
                call.kind
            );
        }
        assert!(!call.schema.to_string().contains(MARKER));
    }

    let sql = rusqlite::Connection::open(&db).unwrap();
    let count = |query: &str| -> u32 { sql.query_row(query, [], |r| r.get(0)).unwrap() };
    assert!(
        count("SELECT COUNT(*) FROM debug_log WHERE op_id='op-marker'") >= 2,
        "the Turn left its debug rows"
    );
    assert!(
        count("SELECT COUNT(*) FROM debug_log WHERE op_id LIKE 'ooc-%'") >= 1,
        "the OOC call left its debug row"
    );
    assert_eq!(
        count(&format!(
            "SELECT COUNT(*) FROM debug_log WHERE prompt LIKE '%{MARKER}%' OR raw LIKE '%{MARKER}%'"
        )),
        0,
        "a debug row holds the Note"
    );
    // The search would find it: the Note is in the file, in its own table.
    assert_eq!(
        count(&format!(
            "SELECT COUNT(*) FROM player_note WHERE body LIKE '%{MARKER}%'"
        )),
        2
    );
}

/// The screens show a Pin where its target stands now; removing it there
/// removes the stored Pin on the reverted Turn.
#[test]
fn unpin_on_the_correction_removes_the_pin_of_the_reverted_turn() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    gm.push_proposal(allowed());
    gm.push_narration("Orsel looks up.".into());
    let first = s
        .submit_intent("op-1".into(), "I bow.", &mut |_| {})
        .unwrap()
        .turn_id;
    s.set_pin(&Target::Act { turn_id: first }, true).unwrap();
    gm.push_proposal(allowed());
    gm.push_narration("Orsel nods.".into());
    let fixed = s
        .correct_last("op-2".into(), "I nod.", "I meant a nod", &mut |_| {})
        .unwrap()
        .turn_id;
    let shown = Target::Act { turn_id: fixed };
    assert_eq!(s.get_journal().unwrap().pins, [shown.clone()]);

    s.set_pin(&shown, false).unwrap();
    assert!(s.get_journal().unwrap().pins.is_empty());
    assert!(s.get_play_view().unwrap().pinned.is_empty());
}

#[test]
fn note_errors_reach_the_player_as_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    assert_eq!(
        s.add_note("  ", None).unwrap_err().to_string(),
        "a note needs some text"
    );
    assert_eq!(
        s.edit_note(42, "Text.").unwrap_err().to_string(),
        "there is no note 42"
    );
    assert!(s.get_journal().unwrap().notes.is_empty());
}
