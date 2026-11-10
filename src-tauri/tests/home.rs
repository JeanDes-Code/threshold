//! The home screen: one card per Campaign file, read without opening it.

use std::path::Path;

mod support;

use support::{allowed, session, town};
use threshold_app::session::OLD_FILE;
use threshold_core::{RecordedGm, store};

/// Shape 3.6: the card reads the seed and counts the Turns that stand.
#[test]
fn campaign_card_reads_the_seed_and_counts_turns_only() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;

    let cards = s.list_campaigns();
    assert_eq!(cards.len(), 1);
    assert!(cards[0].needs_begin);
    assert_eq!(cards[0].turn_count, 0);

    gm.push_narration("The town hall is quiet.".into());
    s.begin(&mut |_| {}).unwrap();
    for i in 0..3 {
        gm.push_proposal(allowed());
        gm.push_narration(format!("Orsel looks up, {i}."));
        s.submit_intent(format!("op-{i}"), "I copy the charter.", &mut |_| {})
            .unwrap();
    }
    gm.push_proposal(allowed());
    gm.push_narration("Orsel nods.".into());
    s.correct_last("fix".into(), "I copy it slowly.", "too fast", &mut |_| {})
        .unwrap();

    let card = s.list_campaigns().remove(0);
    let seed = std::fs::read_to_string(support::town_dir().join("adventure-seed.toml")).unwrap();
    assert_eq!(card.db_path, db);
    assert_eq!(card.pc_name, "Wren");
    assert!(!card.premise.is_empty() && seed.contains(&card.premise));
    // Three Play Turns, one reverted, one Correction; the Opening does not count.
    assert_eq!(card.turn_count, 3);
    assert!(!card.needs_begin);
    assert_eq!(card.problem, None);
    assert!(card.last_played_ms > 0);
}

#[test]
fn schema_2_file_shows_a_problem_card_and_does_not_open() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let good = s.summary().unwrap().db_path;
    let old = dir.path().join("campaigns").join("campaign-old.sqlite");
    let sql = rusqlite::Connection::open(&old).unwrap();
    sql.execute_batch(
        "CREATE TABLE campaign (id INTEGER PRIMARY KEY, adventure_seed_json TEXT NOT NULL, clock_min INTEGER NOT NULL, seed INTEGER NOT NULL);
         PRAGMA user_version=2;",
    )
    .unwrap();
    drop(sql);

    let cards = s.list_campaigns();
    assert_eq!(cards.len(), 2);
    let card = cards
        .iter()
        .find(|c| c.db_path == old.display().to_string())
        .unwrap();
    assert_eq!(card.problem.as_deref(), Some(OLD_FILE));
    assert!(card.pc_name.is_empty());
    let fine = cards.iter().find(|c| c.db_path == good).unwrap();
    assert_eq!(fine.problem, None);

    let err = s.open_campaign(&old).unwrap_err().to_string();
    assert!(err.contains("version 2"), "{err}");
    // The open Campaign is still the good one.
    assert_eq!(s.summary().unwrap().db_path, good);
}

#[test]
fn peek_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;
    drop(s);
    let path = Path::new(&db);
    let bytes = std::fs::read(path).unwrap();
    let modified = std::fs::metadata(path).unwrap().modified().unwrap();

    let peek = store::peek(path).unwrap();
    assert_eq!(peek.pc_name, "Wren");
    assert!(peek.needs_begin);

    assert_eq!(std::fs::read(path).unwrap(), bytes, "the file changed");
    assert_eq!(
        std::fs::metadata(path).unwrap().modified().unwrap(),
        modified
    );
    let side: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with("-journal") || n.ends_with("-wal"))
        .collect();
    assert!(side.is_empty(), "{side:?}");
}

#[test]
fn a_foreign_file_shows_a_problem_card() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let s = session(dir.path(), &gm);
    let campaigns = dir.path().join("campaigns");
    std::fs::create_dir_all(&campaigns).unwrap();
    std::fs::write(campaigns.join("notes.sqlite"), b"not a database").unwrap();
    let cards = s.list_campaigns();
    assert_eq!(cards.len(), 1);
    let problem = cards[0].problem.clone().unwrap();
    assert!(
        problem.starts_with("this file cannot be opened"),
        "{problem}"
    );
    assert_ne!(problem, OLD_FILE);
}
