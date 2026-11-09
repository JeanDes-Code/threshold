//! The shell's command layer, driven without a window: `Session` is what
//! every Tauri command calls.

use std::path::{Path, PathBuf};

use threshold_app::session::{AppError, Session};
use threshold_core::{
    Gm, RecordedGm,
    domain::{ActionKind, DecisionProposal, Handle, NewFact, Seen, SlotDraft, Verdict},
    store::Store,
};

fn town_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/town")
}

fn allowed() -> String {
    serde_json::to_string(&base()).unwrap()
}

fn base() -> DecisionProposal {
    DecisionProposal {
        action: ActionKind::Observe,
        verdict: Verdict::Allowed,
        blockers: vec![],
        check: None,
        reactions: vec![],
        new_npcs: vec![],
        facts: vec![],
        cancel: vec![],
        schedule: None,
        minutes: 0,
        intimate: false,
    }
}

/// The id of a town entity, read from the campaign file.
fn entity(db: &str, name: &str) -> u64 {
    let store = Store::open(Path::new(db)).unwrap();
    let campaign = *store.campaigns().unwrap().last().unwrap();
    let view = store.snapshot(campaign).unwrap();
    view.entities.iter().find(|e| e.name == name).unwrap().id
}

/// A proposal with one new fact about `subject`.
fn with_fact(subject: u64, key: &str, value: &str, said_by: Option<u64>) -> String {
    let mut p = base();
    p.facts = vec![NewFact {
        slot: SlotDraft {
            subject: Handle::Entity(subject),
            key: key.into(),
            value: value.into(),
        },
        seen: Seen::Present,
        said_by: said_by.map(Handle::Entity),
        because: vec![],
    }];
    serde_json::to_string(&p).unwrap()
}

/// A session on a fresh data dir whose every Engine shares `gm`.
fn session(dir: &Path, gm: &RecordedGm) -> Session {
    let gm = gm.clone();
    Session::new(
        dir.to_path_buf(),
        Box::new(move || Box::new(gm.clone()) as Box<dyn Gm + Send>),
    )
}

fn town(session: &mut Session) {
    let dir = town_dir();
    session
        .new_campaign(
            &dir.join("adventure-seed.toml"),
            &dir.join("world.toml"),
            11,
        )
        .unwrap();
}

#[test]
fn get_debug_refused_when_debug_off() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![allowed()], vec!["The hall is quiet.".into()]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let turn = s
        .submit_intent("op-1".into(), "I look around.", &mut |_| {})
        .unwrap();

    assert!(matches!(s.get_debug(turn.turn_id), Err(AppError::DebugOff)));

    s.set_debug(true);
    let debug = s.get_debug(turn.turn_id).unwrap();
    assert_eq!(debug.op_id, "op-1");
    assert!(debug.entries.iter().any(|e| e.step == "propose"));
    assert!(debug.entries.iter().any(|e| e.step == "narrate"));

    s.set_debug(false);
    assert!(matches!(s.get_debug(turn.turn_id), Err(AppError::DebugOff)));
}

#[test]
fn journal_hides_gm_only_facts() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);

    let journal = s.get_journal().unwrap();
    let texts: Vec<&str> = journal
        .observations
        .iter()
        .chain(&journal.claims)
        .map(|e| e.text.as_str())
        .collect();
    // The PC witnessed who stands in the town hall at the start.
    assert!(
        journal
            .observations
            .iter()
            .any(|e| e.text.contains("Magistrate Orsel holds court")),
        "{texts:?}"
    );
    // The GM-only secret and the unheard rumor never show.
    assert!(texts.iter().all(|t| !t.contains("skims")), "{texts:?}");
    assert!(texts.iter().all(|t| !t.contains("scales")), "{texts:?}");
    assert!(journal.claims.is_empty());

    let character = s.get_character().unwrap();
    assert_eq!(character.name, "Wren");
    assert!(character.facts.iter().all(|t| !t.contains("skims")));
}

#[test]
fn turn_streams_and_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![allowed()], vec!["The hall is quiet.".into()]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let mut chunks = Vec::new();
    let turn = s
        .submit_intent("op-1".into(), "I look around.", &mut |c| {
            chunks.push(c.to_owned())
        })
        .unwrap();
    assert_eq!(chunks.concat(), "The hall is quiet.");
    assert_eq!(turn.narration, "The hall is quiet.");
    assert_eq!(turn.roll, None);

    // A retry with the same op_id returns the stored Turn: no second Commit.
    let again = s
        .submit_intent("op-1".into(), "I look around.", &mut |_| {})
        .unwrap();
    assert_eq!(again.turn_id, turn.turn_id);

    let db = s.summary().unwrap().db_path;
    let mut reopened = session(dir.path(), &gm);
    let summary = reopened.open_campaign(Path::new(&db)).unwrap();
    assert_eq!(summary.pc_name, "Wren");
    assert_eq!(summary.turns.len(), 1);
    assert_eq!(summary.turns[0].intent, "I look around.");
    assert_eq!(
        summary.turns[0].narration.as_deref(),
        Some("The hall is quiet.")
    );
}

#[test]
fn commands_need_a_campaign() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    assert!(matches!(s.get_journal(), Err(AppError::NoCampaign)));
    assert!(matches!(
        s.submit_intent("op".into(), "hi", &mut |_| {}),
        Err(AppError::NoCampaign)
    ));
    assert!(s.open_campaign(&dir.path().join("missing.sqlite")).is_err());
}

#[test]
fn config_is_created_once() {
    let dir = tempfile::tempdir().unwrap();
    let config = threshold_app::session::load_config(dir.path()).unwrap();
    assert_eq!(config, threshold_ollama::OllamaConfig::default());
    let path = dir.path().join("threshold.toml");
    std::fs::write(&path, "model = \"other:tag\"\n").unwrap();
    let config = threshold_app::session::load_config(dir.path()).unwrap();
    assert_eq!(config.model, "other:tag");
}

#[test]
fn journal_lists_claims_with_their_source() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec!["Hessa leans in.".into()]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;
    let (hessa, tobin) = (entity(&db, "Hessa"), entity(&db, "Tobin"));
    gm.push_proposal(with_fact(tobin, "cheats", "the scales", Some(hessa)));
    s.submit_intent("op-1".into(), "I ask Hessa about the miller.", &mut |_| {})
        .unwrap();

    let journal = s.get_journal().unwrap();
    let claim = journal
        .claims
        .iter()
        .find(|c| c.subject == "Tobin")
        .expect("the told Belief is a Claim");
    assert_eq!(claim.source.as_deref(), Some("Hessa"));
    assert!(
        journal
            .observations
            .iter()
            .all(|o| o.subject != "Tobin" || !o.text.contains("cheats"))
    );
}

#[test]
fn open_campaign_refuses_a_foreign_file() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);

    let empty = dir.path().join("empty.sqlite");
    std::fs::write(&empty, b"").unwrap();
    assert!(s.open_campaign(&empty).is_err());
    // Refused before any migration: the file is still empty.
    assert_eq!(std::fs::metadata(&empty).unwrap().len(), 0);

    let text = dir.path().join("notes.sqlite");
    std::fs::write(&text, b"just some notes, not a database at all").unwrap();
    assert!(s.open_campaign(&text).is_err());
    assert!(matches!(s.summary(), Err(AppError::NoCampaign)));
}

#[test]
fn rejected_turn_error_hides_gm_only_facts() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;
    let orsel = entity(&db, "Orsel");
    // Both attempts try to bend the GM-only secret.
    let bend = with_fact(orsel, "skims", "nothing at all", None);
    gm.push_proposal(bend.clone());
    gm.push_proposal(bend);

    let err = s
        .submit_intent("op-1".into(), "I accuse Orsel.", &mut |_| {})
        .unwrap_err()
        .to_string();
    assert!(err.contains("fact bends"), "{err}");
    for secret in ["skims", "grain tithe"] {
        assert!(!err.contains(secret), "{err}");
    }
    // The raw reason did name the secret's key: the formatter removed it.
    s.set_debug(true);
    let raw = Store::open(Path::new(&db))
        .unwrap()
        .debug_log("op-1")
        .unwrap();
    assert!(
        raw.iter()
            .any(|e| e.step == "validate" && e.raw.contains("skims"))
    );
}

#[test]
fn debug_flag_is_shared_outside_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let s = session(dir.path(), &gm);
    let flag = s.debug_flag();
    flag.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(s.debug());
}
