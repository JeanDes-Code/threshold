//! The mature content module through the shell (Task 1): a Refusal reaches
//! the Player as one fixed sentence, with no name, id or age.

mod support;

use std::path::Path;

use support::{base, session, town, town_at_implied, town_in_a_scene};
use threshold_app::session::{AppError, Session};
use threshold_core::{
    RecordedGm,
    domain::ActionKind,
    store::{Store, TurnKind},
    view::{REFUSAL_ADULTS_ONLY, REFUSAL_EXCLUDED, SceneState},
};

/// An intimate proposal with no reaction: the age rule refuses it before the
/// stance rule matters.
fn intimate() -> String {
    let mut p = base();
    p.intimate = true;
    serde_json::to_string(&p).unwrap()
}

/// Every entity name of the open Campaign.
fn names(s: &Session) -> Vec<String> {
    let db = s.summary().unwrap().db_path;
    let store = Store::open(Path::new(&db)).unwrap();
    let campaign = *store.campaigns().unwrap().last().unwrap();
    let view = store.snapshot(campaign).unwrap();
    view.entities
        .iter()
        .flat_map(|e| std::iter::once(e.name.clone()).chain(e.other_names.iter().cloned()))
        .collect()
}

fn refusal(s: &mut Session, op: &str) -> String {
    match s.submit_intent(op.into(), "I lean closer.", &mut |_| {}) {
        Err(AppError::Refused(text)) => text,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_refusal_shows_one_fixed_sentence() {
    for debug in [false, true] {
        // Excluded: the default level of a seed.
        let dir = tempfile::tempdir().unwrap();
        let gm = RecordedGm::new(vec![intimate(), intimate()], vec![]);
        let mut s = session(dir.path(), &gm);
        town(&mut s);
        s.set_debug(debug);
        let text = refusal(&mut s, "op-1");
        assert_eq!(text, REFUSAL_EXCLUDED, "debug {debug}");
        let names = names(&s);
        assert!(!names.is_empty());
        for name in &names {
            assert!(!text.contains(name.as_str()), "{text} names {name}");
        }
        assert!(!text.chars().any(|c| c.is_ascii_digit()), "{text}");
        assert_eq!(gm.calls().len(), 1, "no retry after a refusal");

        // Adults only: every age of the fixture is unconfirmed.
        let dir = tempfile::tempdir().unwrap();
        let gm = RecordedGm::new(vec![intimate(), intimate()], vec![]);
        let mut s = session(dir.path(), &gm);
        town_at_implied(&mut s, dir.path());
        s.set_debug(debug);
        let text = refusal(&mut s, "op-1");
        assert_eq!(text, REFUSAL_ADULTS_ONLY, "debug {debug}");
        for name in &names {
            assert!(!text.contains(name.as_str()), "{text} names {name}");
        }
        assert!(!text.chars().any(|c| c.is_ascii_digit()), "{text}");
        assert_eq!(gm.calls().len(), 1, "no retry after a refusal");
    }
}

/// Review M3: a Refusal is not a failed Turn. It reaches the UI as its own
/// kind, `{"refusal": <sentence>}`, not as a failure text, and the same
/// op_id sent again gets the same Refusal with no model call.
#[test]
fn a_refusal_is_its_own_result_kind() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![intimate(), intimate()], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let err = s
        .submit_intent("op-1".into(), "I lean closer.", &mut |_| {})
        .expect_err("refused");
    assert_eq!(
        serde_json::to_value(&err).unwrap(),
        serde_json::json!({ "refusal": REFUSAL_EXCLUDED })
    );
    assert_eq!(err.to_string(), REFUSAL_EXCLUDED);

    assert_eq!(refusal(&mut s, "op-1"), REFUSAL_EXCLUDED);
    assert_eq!(gm.calls().len(), 1, "the same op runs no model call");
    assert_eq!(refusal(&mut s, "op-2"), REFUSAL_EXCLUDED);
    assert_eq!(gm.calls().len(), 2, "a new op is judged afresh");
}

/// "End scene" through the shell (Task 5): the closing Narration streams
/// through the same sink as an Intent, the Turn commits, and the Play view
/// shows the scene closed, the Closing Turn with no Intent, and nothing to
/// correct.
#[test]
fn end_scene_streams_and_commits() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town_in_a_scene(&mut s, dir.path(), &gm);
    let view = s.get_play_view().unwrap();
    assert!(
        matches!(view.scene, SceneState::Intimate { .. }),
        "{:?}",
        view.scene
    );

    let mut p = base();
    p.action = ActionKind::Wait;
    p.minutes = 10;
    gm.push_proposal(serde_json::to_string(&p).unwrap());
    gm.push_narration("The hall is quiet again.".into());
    let mut chunks = String::new();
    let turn = s
        .end_scene("close-1".into(), &mut |c| chunks.push_str(c))
        .unwrap();
    assert_eq!(chunks, "The hall is quiet again.");
    assert_eq!(turn.narration.as_deref(), Some("The hall is quiet again."));

    let view = s.get_play_view().unwrap();
    assert_eq!(view.scene, SceneState::Plain);
    assert!(!view.can_correct);
    let last = view.story.last().unwrap();
    assert_eq!(last.turn_id, turn.turn_id);
    assert_eq!(last.kind, TurnKind::Closing);
    assert_eq!(last.intent, "");

    // No scene: a plain error, and no model call.
    let calls = gm.calls().len();
    let err = s.end_scene("close-2".into(), &mut |_| {}).unwrap_err();
    assert_eq!(err.to_string(), "no Intimate Scene is open");
    assert_eq!(gm.calls().len(), calls);
}
