//! The mature content module through the shell (Task 1): a Refusal reaches
//! the Player as one fixed sentence, with no name, id or age.

mod support;

use std::{
    path::Path,
    sync::{Arc, Mutex, mpsc},
    thread,
};
use threshold_core::domain::Intimacy;

use support::{base, session, town, town_at_implied, town_in_a_scene};
use threshold_app::session::{AppError, FadeResult, PACKS_OP_ID, Session, fade_to_black};
use threshold_core::{
    CallKind, Gm, GmError, Prompt, RecordedGm, Stream,
    domain::{ActionKind, DetailLevel},
    pack::PackRule,
    store::{Store, TurnKind},
    view::{
        CANNOT_CONFIRM, REFUSAL_ADULTS_ONLY, REFUSAL_EXCLUDED, RefusalView, SceneState, TurnBody,
    },
};

/// An intimate proposal with no reaction: the age rule refuses it before the
/// stance rule matters.
fn intimate() -> String {
    let mut p = base();
    p.intimacy = Intimacy::Intimate;
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
    refusal_view(s, op).text
}

fn refusal_view(s: &mut Session, op: &str) -> RefusalView {
    match s.submit_intent(op.into(), "I lean closer.", &mut |_| {}) {
        Err(AppError::Refused(view)) => view,
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
        serde_json::json!({ "refusal": REFUSAL_EXCLUDED, "kind": "excluded", "to_confirm": [] })
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
    let gate = s.fade_gate();
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
    let result = fade_to_black(&shared, &gate, "fade-1".into()).unwrap();
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
    let gate = s.fade_gate();
    let shared = Arc::new(Mutex::new(s));
    let mut p = base();
    p.action = ActionKind::Wait;
    p.minutes = 10;
    gm.push_proposal(serde_json::to_string(&p).unwrap());
    let before = gm.calls().len();
    let result = fade_to_black(&shared, &gate, "fade-1".into()).unwrap();
    assert_eq!(result, FadeResult { pending: false });
    assert!(!gate.raised(), "the flag is down");
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
    let result = fade_to_black(&shared, &gate, "fade-2".into()).unwrap();
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
    let gate = s.fade_gate();
    let shared = Arc::new(Mutex::new(s));
    let calls = gm.calls().len();
    let result = fade_to_black(&shared, &gate, "op-1".into()).unwrap();
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

/// Review M2 (2026-11-12): each Turn command clears the fade flag when it
/// starts, so a flag left up never fades the next Turn. The flag is raised
/// by hand here, as nothing else leaves it up.
#[test]
fn a_flag_left_up_fades_no_later_turn() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let gate = s.fade_gate();

    gate.flag().store(true, std::sync::atomic::Ordering::SeqCst);
    gm.push_proposal(serde_json::to_string(&base()).unwrap());
    gm.push_narration("Dusk falls on the hall.".into());
    let summary = s
        .submit_intent("op-1".into(), "I wait.", &mut |_| {})
        .unwrap();
    assert!(!summary.faded, "submit_intent clears the flag");
    assert_eq!(
        summary.narration.as_deref(),
        Some("Dusk falls on the hall.")
    );
    assert!(!gate.raised());

    gate.flag().store(true, std::sync::atomic::Ordering::SeqCst);
    gm.push_narration("Night falls on the hall.".into());
    let narration = s.renarrate(summary.turn_id, &mut |_| {}).unwrap();
    assert_eq!(narration, "Night falls on the hall.", "renarrate clears it");
    let view = s.get_play_view().unwrap();
    let last = view.story.last().unwrap();
    assert!(
        matches!(last.body, TurnBody::Narration { .. }),
        "{:?}",
        last.body
    );
    assert!(!gate.raised());
}

/// Review minor (2026-11-12): `fade_to_black` tells a running Turn from any
/// other holder of the session lock. With no Turn running it waits for the
/// lock and runs the idle fade, and it never leaves the flag up.
#[test]
fn a_fade_while_another_command_holds_the_lock_runs_the_idle_fade() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town_in_a_scene(&mut s, dir.path(), &gm);
    let gate = s.fade_gate();
    let shared = Arc::new(Mutex::new(s));
    let mut p = base();
    p.action = ActionKind::Wait;
    p.minutes = 10;
    gm.push_proposal(serde_json::to_string(&p).unwrap());

    // Another command (a view call, say) holds the lock; no Turn runs.
    let held = shared.lock().unwrap();
    let (fading, done) = (shared.clone(), gate.clone());
    let fade = thread::spawn(move || fade_to_black(&fading, &done, "fade-1".into()));
    thread::sleep(std::time::Duration::from_millis(150));
    assert!(!fade.is_finished(), "it waits for the lock");
    assert!(!gate.raised(), "no Turn runs: the flag stays down");
    drop(held);
    let result = fade.join().unwrap().unwrap();
    assert_eq!(result, FadeResult { pending: false });
    assert!(!gate.raised());
    let view = shared.lock().unwrap().get_play_view().unwrap();
    assert_eq!(
        view.scene,
        SceneState::Plain,
        "the idle fade closed the scene"
    );
    let last = view.story.last().unwrap();
    assert_eq!(last.kind, TurnKind::Closing);
    assert!(matches!(last.body, TurnBody::Faded { .. }));
}

/// Review minor (2026-11-12): the UI sends a late fade of a renarration
/// under the op_id its chunks came with. The shell fades the Turn that
/// renarration told, when it is the latest Turn.
#[test]
fn a_late_fade_of_a_renarration_fades_its_turn() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    gm.push_proposal(serde_json::to_string(&base()).unwrap());
    let summary = s
        .submit_intent("op-1".into(), "I wait.", &mut |_| {})
        .unwrap();
    assert!(summary.narration_error.is_some(), "the narration fails");
    let turn_id = summary.turn_id;
    gm.push_narration("MARKER-FADE-4 The hall empties.".into());
    s.renarrate(turn_id, &mut |_| {}).unwrap();
    let gate = s.fade_gate();
    let shared = Arc::new(Mutex::new(s));
    let calls = gm.calls().len();
    let op = threshold_app::session::renarrate_op_id(turn_id);
    let result = fade_to_black(&shared, &gate, op).unwrap();
    assert_eq!(result, FadeResult { pending: false });
    assert_eq!(gm.calls().len(), calls, "no model call");
    let view = shared.lock().unwrap().get_play_view().unwrap();
    let last = view.story.last().unwrap();
    assert_eq!(last.turn_id, turn_id);
    assert!(
        matches!(last.body, TurnBody::Faded { .. }),
        "{:?}",
        last.body
    );
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains("MARKER-FADE-4")
    );
}

// --- Content Packs from the data folder (Task 7) -------------------------------

/// Copies the sample pack of the repository to `<data_dir>/packs/<folder>`.
fn install_sample(data_dir: &Path, folder: &str) {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/packs/test-pack");
    let to = data_dir.join("packs").join(folder);
    std::fs::create_dir_all(&to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
}

/// The town fixture with its seed at Detail Level `explicit`, every adult
/// confirmed.
fn town_at_explicit(s: &mut Session, dir: &Path) {
    let seed = std::fs::read_to_string(support::town_dir().join("adventure-seed.toml")).unwrap();
    let seed_path = dir.join("adventure-seed.toml");
    std::fs::write(&seed_path, format!("detail = \"explicit\"\n{seed}")).unwrap();
    s.new_campaign(&seed_path, &support::town_dir().join("world.toml"), 11)
        .unwrap();
    support::confirm_all(s);
}

/// The home menu's pack list: a refused pack shows by its folder and one
/// fixed sentence for its rule, never its text; a loaded pack by its name,
/// whether it gives explicit description, and its recommended model.
#[test]
fn list_packs_shows_a_refused_pack_by_folder_and_rule() {
    let dir = tempfile::tempdir().unwrap();
    install_sample(dir.path(), "b-sample");
    // A pack of format 1, as the repository held before Task 7.
    let old = dir.path().join("packs/a-old");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(
        old.join("pack.toml"),
        "name = \"old\"\n[[fragments]]\nat = \"tone\"\nfile = \"prompt.md\"\n",
    )
    .unwrap();
    std::fs::write(old.join("prompt.md"), "UNIQUEOLDPACKTEXT stays private.").unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let s = session(dir.path(), &gm);
    let lines = s.list_packs();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].folder, "a-old");
    assert_eq!(lines[0].name, None);
    assert!(!lines[0].explicit);
    let problem = lines[0].problem.as_deref().expect("a refused pack");
    assert_eq!(problem, PackRule::Format.sentence());
    assert_eq!(lines[1].folder, "b-sample");
    assert_eq!(lines[1].name.as_deref(), Some("sample"));
    assert!(lines[1].explicit);
    assert_eq!(
        lines[1].recommended_model.as_deref(),
        Some("sample-model:latest")
    );
    assert_eq!(lines[1].problem, None);
    let json = serde_json::to_string(&lines).unwrap();
    for private in ["UNIQUEOLDPACKTEXT", "SAMPLE FRAGMENT"] {
        assert!(!json.contains(private), "{json}");
    }
}

/// The packs of `<data_dir>/packs` reach every Engine: a scene Narration at
/// `explicit` holds the sample's explicit fragment, and the Play view says
/// which level is in use. With no pack, the same Campaign plays at
/// `implied`. The debug log names the packs and holds no fragment.
#[test]
fn the_data_folder_packs_reach_every_engine() {
    for with_pack in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        if with_pack {
            install_sample(dir.path(), "sample");
        }
        let gm = RecordedGm::new(vec![], vec![]);
        let mut s = session(dir.path(), &gm);
        town_at_explicit(&mut s, dir.path());
        let db = s.summary().unwrap().db_path;
        gm.push_proposal(support::intimate_with_hessa(&db));
        gm.push_narration("Hessa smiles.".into());
        s.submit_intent("scene-1".into(), "I sit with Hessa.", &mut |_| {})
            .unwrap();
        let narrate = gm
            .calls()
            .into_iter()
            .rfind(|c| c.kind == CallKind::Narrate)
            .unwrap();
        assert_eq!(
            narrate.prompt.recent.contains("SAMPLE FRAGMENT D"),
            with_pack
        );
        let view = s.get_play_view().unwrap();
        assert!(matches!(view.scene, SceneState::Intimate { .. }));
        assert_eq!(view.detail.stored, DetailLevel::Explicit);
        let effective = if with_pack {
            DetailLevel::Explicit
        } else {
            DetailLevel::Implied
        };
        assert_eq!(view.detail.effective, effective);
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("SAMPLE FRAGMENT"), "{json}");

        let store = Store::open(Path::new(&db)).unwrap();
        let rows = store.debug_log(PACKS_OP_ID).unwrap();
        if with_pack {
            assert!(!rows.is_empty());
            for row in &rows {
                assert!(row.raw.contains("sample"), "{}", row.raw);
                assert!(!row.raw.contains("SAMPLE FRAGMENT"), "{}", row.raw);
            }
        } else {
            assert!(rows.is_empty(), "no packs folder: no row");
        }
    }
}

// --- Task 8: settings in play ------------------------------------------------

/// Task 8: an `adults_only` Refusal reaches the UI with the characters the
/// Player may confirm, by name and with no age. "Confirm" through the
/// shell is the core's op: the Refusal then lists the others only, and the
/// command answers a character it cannot confirm with one fixed sentence.
#[test]
fn a_refusal_carries_the_characters_to_confirm() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town_at_implied(&mut s, dir.path());
    let db = s.summary().unwrap().db_path;
    gm.push_proposal(support::intimate_with_hessa(&db));
    let refused = refusal_view(&mut s, "op-1");
    assert_eq!(refused.text, REFUSAL_ADULTS_ONLY);
    let names = |view: &RefusalView| -> Vec<String> {
        let mut names: Vec<String> = view
            .to_confirm
            .iter()
            .map(|n| n.name.clone().expect("a name"))
            .collect();
        names.sort();
        names
    };
    // Everyone in the town hall: the PC, Orsel, Doul and Hessa. Not Tobin,
    // at the mill.
    assert_eq!(names(&refused), ["Doul", "Hessa", "Orsel", "Wren"]);
    let json = serde_json::to_value(AppError::Refused(refused.clone())).unwrap();
    assert_eq!(json["refusal"], REFUSAL_ADULTS_ONLY);
    assert_eq!(json["kind"], "adults_only");
    assert_eq!(json["to_confirm"].as_array().unwrap().len(), 4);
    assert!(!json.to_string().contains("age"), "{json}");

    let hessa = support::entity(&db, "Hessa");
    let view = s.confirm_age_in_play(hessa, "c-1".into()).unwrap();
    assert!(!view.can_correct, "a settings change closes the Correction");
    gm.push_proposal(support::intimate_with_hessa(&db));
    assert_eq!(
        names(&refusal_view(&mut s, "op-2")),
        ["Doul", "Orsel", "Wren"]
    );

    // A character the core cannot confirm: one fixed sentence.
    let archive = support::entity(&db, "Tithe archive");
    let err = s.confirm_age_in_play(archive, "c-2".into()).unwrap_err();
    assert_eq!(err.to_string(), CANNOT_CONFIRM);
    assert_eq!(gm.calls().len(), 2, "no model call for a setting");
}

/// Task 8: the Detail Level changes in play through the core's op; the
/// Play view shows the stored level; `excluded` refuses the next intimate
/// Turn with the sentence that says where to change it.
#[test]
fn set_detail_level_changes_the_play_view() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town_in_a_scene(&mut s, dir.path(), &gm);
    let view = s
        .set_detail_level(DetailLevel::Explicit, "lvl-1".into())
        .unwrap();
    assert_eq!(view.detail.stored, DetailLevel::Explicit);
    assert!(matches!(view.scene, SceneState::Intimate { .. }));
    let view = s
        .set_detail_level(DetailLevel::Excluded, "lvl-2".into())
        .unwrap();
    assert_eq!(view.detail.stored, DetailLevel::Excluded);
    assert_eq!(view.scene, SceneState::Plain, "excluded closes the scene");
    let db = s.summary().unwrap().db_path;
    gm.push_proposal(support::intimate_with_hessa(&db));
    let refused = refusal_view(&mut s, "op-1");
    assert_eq!(refused.text, REFUSAL_EXCLUDED);
    assert!(refused.text.contains("Ask the GM"));
    assert!(refused.to_confirm.is_empty());
}

fn rust_sources(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// Task 8: only the core writes the mature ops. No file of the shell builds
/// a `ConfirmAge`, a `SetDetailLevel` or a `SetScene` op: each settings
/// command calls an Engine method.
#[test]
fn no_shell_file_writes_a_mature_op() {
    let mut files = Vec::new();
    rust_sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    assert!(!files.is_empty());
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for op in ["ConfirmAge", "SetDetailLevel", "SetScene"] {
            assert!(!text.contains(op), "{} names {op}", file.display());
        }
    }
}
