//! The mature content module through the shell (Task 1): a Refusal reaches
//! the Player as one fixed sentence, with no name, id or age.

mod support;

use std::{
    path::Path,
    sync::{Arc, Mutex, mpsc},
    thread,
};

use support::{base, session, town, town_at_implied, town_in_a_scene};
use threshold_app::session::{AppError, FadeResult, Session, fade_to_black};
use threshold_core::{
    CallKind, Gm, GmError, Prompt, RecordedGm, Stream,
    domain::ActionKind,
    store::{Store, TurnKind},
    view::{REFUSAL_ADULTS_ONLY, REFUSAL_EXCLUDED, SceneState, TurnBody},
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

/// A chunked model that stops after the first narration chunk until the
/// test opens its gate, as `Hooked` in `commands.rs` does for a proposal.
#[derive(Clone)]
struct Gated {
    inner: RecordedGm,
    reached: Arc<Mutex<Option<mpsc::Sender<()>>>>,
    gate: Arc<Mutex<mpsc::Receiver<()>>>,
}

impl Gm for Gated {
    fn propose(&self, prompt: &Prompt, schema: &serde_json::Value) -> Result<String, GmError> {
        self.inner.propose(prompt, schema)
    }

    fn narrate(
        &self,
        prompt: &Prompt,
        max_tokens: Option<u32>,
        on_chunk: &mut dyn FnMut(&str) -> Stream,
    ) -> Result<String, GmError> {
        let mut first = true;
        self.inner.narrate(prompt, max_tokens, &mut |chunk| {
            let next = on_chunk(chunk);
            if std::mem::take(&mut first)
                && let Some(reached) = self.reached.lock().unwrap().take()
            {
                reached.send(()).unwrap();
                self.gate.lock().unwrap().recv().unwrap();
            }
            next
        })
    }
}

/// "Fade to black" while a Turn streams: the command takes no lock, finds
/// the session busy and says `pending`; the running Turn stops its stream
/// at the next chunk and finishes faded.
#[test]
fn fade_while_a_turn_runs_is_pending_and_the_turn_finishes_faded() {
    let dir = tempfile::tempdir().unwrap();
    let inner = RecordedGm::new(vec![], vec![]).chunked();
    let (reached_tx, reached) = mpsc::channel();
    let (open, gate) = mpsc::channel();
    let gated = Gated {
        inner: inner.clone(),
        reached: Arc::new(Mutex::new(Some(reached_tx))),
        gate: Arc::new(Mutex::new(gate)),
    };
    let mut s = Session::new(
        dir.path().to_path_buf(),
        Box::new(move || Box::new(gated.clone()) as Box<dyn Gm + Send>),
    );
    town(&mut s);
    let flag = s.fade_flag();
    let shared = Arc::new(Mutex::new(s));
    inner.push_proposal(serde_json::to_string(&base()).unwrap());
    inner.push_narration("Dusk: the hall empties and the lamps go out.".into());

    let running = shared.clone();
    let turn = thread::spawn(move || {
        let mut shown = String::new();
        let summary = running
            .lock()
            .unwrap()
            .submit_intent("op-1".into(), "I wait.", &mut |c| shown.push_str(c))
            .unwrap();
        (summary, shown)
    });
    reached.recv().unwrap();
    let result = fade_to_black(&shared, &flag, "fade-1".into()).unwrap();
    assert_eq!(result, FadeResult { pending: true });
    open.send(()).unwrap();
    let (summary, shown) = turn.join().unwrap();
    assert!(summary.faded);
    assert_eq!(shown, "Dusk:", "no chunk after the fade reached the screen");
    let s = shared.lock().unwrap();
    let view = s.get_play_view().unwrap();
    let last = view.story.last().unwrap();
    assert_eq!(last.turn_id, summary.turn_id);
    assert!(
        matches!(last.body, TurnBody::Faded { .. }),
        "{:?}",
        last.body
    );
    let narrations = inner
        .calls()
        .iter()
        .filter(|c| c.kind == CallKind::Narrate)
        .count();
    assert_eq!(narrations, 1);
}

/// "Fade to black" with no Turn running: the command takes the lock, clears
/// the flag and calls `Engine::fade`. In a scene that is a Closing Turn
/// born faded; with no scene, nothing happens.
#[test]
fn fade_when_idle_runs_engine_fade() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town_in_a_scene(&mut s, dir.path(), &gm);
    let flag = s.fade_flag();
    let shared = Arc::new(Mutex::new(s));
    let mut p = base();
    p.action = ActionKind::Wait;
    p.minutes = 10;
    gm.push_proposal(serde_json::to_string(&p).unwrap());
    let before = gm.calls().len();
    let result = fade_to_black(&shared, &flag, "fade-1".into()).unwrap();
    assert_eq!(result, FadeResult { pending: false });
    assert!(
        !flag.load(std::sync::atomic::Ordering::SeqCst),
        "the flag is cleared"
    );
    let kinds: Vec<CallKind> = gm.calls()[before..].iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        [CallKind::Propose],
        "one proposal call, no narration call"
    );
    let view = shared.lock().unwrap().get_play_view().unwrap();
    assert_eq!(view.scene, SceneState::Plain);
    let last = view.story.last().unwrap();
    assert_eq!(last.kind, TurnKind::Closing);
    assert!(matches!(last.body, TurnBody::Faded { .. }));

    // No scene now: nothing to fade, and no model call.
    let before = gm.calls().len();
    let result = fade_to_black(&shared, &flag, "fade-2".into()).unwrap();
    assert_eq!(result, FadeResult { pending: false });
    assert_eq!(gm.calls().len(), before);
}

/// The Player pressed "Fade to black" while the Narration streamed, but the
/// Turn ended before it read the flag: the UI sends the fade again with that
/// Turn's op_id, and the shell fades it then.
#[test]
fn a_fade_that_reaches_the_shell_after_the_turn_ended_fades_it() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    gm.push_proposal(serde_json::to_string(&base()).unwrap());
    gm.push_narration("MARKER-FADE-3 The hall empties.".into());
    let summary = s
        .submit_intent("op-1".into(), "I wait.", &mut |_| {})
        .unwrap();
    assert!(!summary.faded);
    let flag = s.fade_flag();
    let shared = Arc::new(Mutex::new(s));
    let calls = gm.calls().len();
    let result = fade_to_black(&shared, &flag, "op-1".into()).unwrap();
    assert_eq!(result, FadeResult { pending: false });
    assert_eq!(gm.calls().len(), calls, "no model call");
    let view = shared.lock().unwrap().get_play_view().unwrap();
    let last = view.story.last().unwrap();
    assert_eq!(last.turn_id, summary.turn_id);
    assert!(matches!(last.body, TurnBody::Faded { .. }));
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains("MARKER-FADE-3")
    );
}
