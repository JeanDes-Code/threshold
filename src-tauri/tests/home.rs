//! The home screen: one card per Campaign file, read without opening it.

use std::path::Path;

mod support;

use support::{allowed, session, town, town_in_a_scene};
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

/// B10 (QA, 2026-11-10): "Remove from list" moves an unopenable file into
/// the archive folder of the data dir. It never deletes it, it takes the
/// card's own id (a path never matches), and the open Campaign stays.
#[test]
fn remove_from_list_moves_the_file_to_the_archive() {
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
    let bytes = std::fs::read(&old).unwrap();
    let card = s
        .list_campaigns()
        .into_iter()
        .find(|c| c.problem.is_some())
        .unwrap();
    assert_eq!(card.id, "campaign-old.sqlite");

    // A path is not a card id: nothing moves.
    for id in [
        old.display().to_string(),
        "../campaigns/campaign-old.sqlite".into(),
        "campaign-none.sqlite".into(),
    ] {
        assert!(s.remove_campaign(&id).is_err(), "{id}");
        assert!(old.exists());
    }
    // The open Campaign cannot move.
    let open_id = Path::new(&good)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(s.remove_campaign(&open_id).is_err());
    assert!(Path::new(&good).exists());

    let moved = s.remove_campaign(&card.id).unwrap();
    assert!(!old.exists(), "the file left the campaigns folder");
    assert_eq!(
        moved,
        dir.path().join("archive").join("campaign-old.sqlite")
    );
    assert_eq!(std::fs::read(&moved).unwrap(), bytes, "moved, not deleted");
    let left: Vec<String> = s.list_campaigns().into_iter().map(|c| c.id).collect();
    assert_eq!(left, [open_id]);
}

/// C10: the shell, not the UI, decides what may move. A Campaign file that
/// opens (its card has no problem) is never archived, even when it is not
/// the open one.
#[test]
fn a_campaign_that_opens_cannot_be_archived() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let first = s.summary().unwrap().db_path;
    town(&mut s);
    let open = s.summary().unwrap().db_path;
    assert_ne!(first, open);
    let card = s
        .list_campaigns()
        .into_iter()
        .find(|c| c.db_path == first)
        .unwrap();
    assert!(card.problem.is_none());
    let err = s.remove_campaign(&card.id).unwrap_err();
    assert!(err.to_string().contains("opens"), "{err}");
    assert!(Path::new(&first).exists());
    assert!(!dir.path().join("archive").exists());
}

/// C10: a side file that cannot move is reported, not ignored.
#[test]
fn a_side_file_that_cannot_move_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let s = &mut session(dir.path(), &gm);
    let campaigns = dir.path().join("campaigns");
    std::fs::create_dir_all(&campaigns).unwrap();
    let old = campaigns.join("campaign-old.sqlite");
    std::fs::write(&old, b"not a database").unwrap();
    std::fs::write(campaigns.join("campaign-old.sqlite-wal"), b"wal").unwrap();
    // The archive already holds a non-empty folder where the side file
    // would go, so its rename fails.
    let blocked = dir.path().join("archive").join("campaign-old.sqlite-wal");
    std::fs::create_dir_all(&blocked).unwrap();
    std::fs::write(blocked.join("keep"), b"x").unwrap();
    let err = s.remove_campaign("campaign-old.sqlite").unwrap_err();
    assert!(err.to_string().contains("-wal"), "{err}");
}

/// A Closing Turn has no Intent: the home card does not count it as a Turn
/// (mature content module, Task 5).
#[test]
fn a_closing_turn_counts_no_turn() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town_in_a_scene(&mut s, dir.path(), &gm);
    assert_eq!(s.list_campaigns()[0].turn_count, 1);
    gm.push_proposal(allowed());
    gm.push_narration("The hall is quiet again.".into());
    s.end_scene("close-1".into(), &mut |_| {}).unwrap();
    let card = s.list_campaigns().remove(0);
    assert_eq!(card.turn_count, 1, "the Closing Turn counts no Turn");
    assert!(!card.needs_begin);
}
