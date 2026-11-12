//! Fade to Black (mature content module, Task 6): the Player stops and
//! hides a Narration with one button. The world keeps what happened; the
//! hidden text is stored nowhere but the debug log (owner's decision 5).
//! Neutral fixtures and marker words only.

mod support;

use std::cell::Cell;

use support::rig::*;
use threshold_core::{
    CallKind,
    domain::{LedgerOp, SceneWhy},
    mature::{FADED_TEMPLATE, neutral_line},
    player::Marks,
    store::{Store, TurnKind},
    turn::{TurnControl, TurnError},
    view::{SceneState, TurnBody, journal_view, ooc_sheet, play_view},
};

/// A marker word in a Narration the Player fades: it must reach no screen
/// and no later prompt.
const MARKER: &str = "MARKER-FADE-1";

/// Ten words, ten chunks of a chunked model. The colon ends the hold of the
/// speech tag filter at once (`speech::TagFilter`), so each chunk reaches
/// the reader as it comes.
const TEN: &str = "Dusk: MARKER-FADE-1 three four five six seven eight nine ten.";

/// The flag rises inside the reader of chunk `at`; `seen` counts the chunks
/// the Player read.
struct Fader {
    up: Cell<bool>,
    seen: Cell<usize>,
    text: std::cell::RefCell<String>,
    at: usize,
}

impl Fader {
    fn at(at: usize) -> Self {
        Self {
            up: Cell::new(false),
            seen: Cell::new(0),
            text: Default::default(),
            at,
        }
    }

    fn up() -> Self {
        let f = Self::at(0);
        f.up.set(true);
        f
    }
}

/// Plays `p` with a Narration of `narration`, the flag rising as `fader`
/// says.
fn play_fading(
    rig: &mut Rig,
    op: &str,
    p: &threshold_core::domain::DecisionProposal,
    narration: &str,
    fader: &Fader,
) -> Result<threshold_core::turn::TurnOutcome, TurnError> {
    rig.gm.push_proposal(serde_json::to_string(p).unwrap());
    rig.gm.push_narration(narration.into());
    run_fading(rig, op, fader)
}

/// Plays one Turn on what the model has queued, the flag rising as `fader`
/// says.
fn run_fading(
    rig: &mut Rig,
    op: &str,
    fader: &Fader,
) -> Result<threshold_core::turn::TurnOutcome, TurnError> {
    let mut on_chunk = |chunk: &str| {
        fader.seen.set(fader.seen.get() + 1);
        fader.text.borrow_mut().push_str(chunk);
        if fader.seen.get() == fader.at {
            fader.up.set(true);
        }
    };
    let faded = || fader.up.get();
    let mut ctl = TurnControl {
        on_chunk: &mut on_chunk,
        faded: &faded,
    };
    rig.engine()
        .play_turn_with(op.into(), "I stay with Brann.", &mut ctl)
}

fn narrate_row(rig: &Rig, op: &str) -> String {
    rig.store()
        .debug_log(op)
        .unwrap()
        .into_iter()
        .rfind(|row| row.step == "narrate")
        .expect("a narrate row")
        .raw
}

#[test]
fn a_fade_stops_the_stream_and_stores_no_text() {
    let mut rig = Rig::new("fade-stops");
    rig.chunked();
    let p = rig.plain();
    let fader = Fader::at(3);
    let out = play_fading(&mut rig, "t1", &p, TEN, &fader).unwrap();
    assert_eq!(fader.seen.get(), 3, "on_chunk got three chunks");
    assert!(out.faded);
    let turn = rig.turn(out.turn_id);
    assert_eq!(turn.narration, None);
    assert!(turn.faded);
    // The model saw `Stop` after the third word: the text it returned, kept
    // in the debug log, is those three words.
    let raw = narrate_row(&rig, "t1");
    assert_eq!(raw, format!("{}\n[faded]", fader.text.borrow()));
    assert!(!raw.contains("four"), "{raw}");
    assert_eq!(out.narration, neutral_line(&turn).unwrap());
}

#[test]
fn a_flag_already_up_makes_no_narration_call() {
    let mut rig = Rig::new("fade-up");
    rig.chunked();
    let p = rig.plain();
    let fader = Fader::up();
    let out = play_fading(&mut rig, "t1", &p, TEN, &fader).unwrap();
    assert_eq!(fader.seen.get(), 0);
    assert!(rig.calls(CallKind::Narrate).is_empty());
    assert!(rig.turn(out.turn_id).faded);
    assert_eq!(narrate_row(&rig, "t1"), "\n[faded]");
}

#[test]
fn a_fade_after_the_last_chunk_still_fades() {
    let mut rig = Rig::new("fade-last");
    rig.chunked();
    let p = rig.plain();
    let fader = Fader::at(10);
    let out = play_fading(&mut rig, "t1", &p, TEN, &fader).unwrap();
    assert_eq!(fader.seen.get(), 10, "every chunk was read");
    let turn = rig.turn(out.turn_id);
    assert!(turn.faded);
    assert_eq!(turn.narration, None);
}

/// A second fade changes nothing: the same op returns the faded Turn with no
/// call, and a fade with no Turn running and no scene is `NothingToFade`.
#[test]
fn a_second_fade_changes_nothing() {
    let mut rig = Rig::new("fade-twice");
    let p = rig.plain();
    let out = play_fading(&mut rig, "t1", &p, "Dusk falls.", &Fader::up()).unwrap();
    let calls = rig.gm.calls().len();
    let again = rig.engine().fade("t1".into()).unwrap();
    assert_eq!(again.turn_id, out.turn_id);
    assert!(again.faded);
    let err = rig.engine().fade("f2".into()).unwrap_err();
    assert!(matches!(err, TurnError::NothingToFade), "{err:?}");
    assert_eq!(rig.gm.calls().len(), calls);
    assert!(rig.turn(out.turn_id).faded);
}

/// The ledger of a faded Turn is the ledger of the same Turn read in full.
#[test]
fn a_fade_leaves_the_ledger_as_a_read_turn_does() {
    let mut read = Rig::new("fade-ledger-read");
    let mut faded = Rig::new("fade-ledger-faded");
    let p = read.intimate();
    let a = read.played("t1", &p, "Brann nods.");
    let b = play_fading(&mut faded, "t1", &p, "Brann nods.", &Fader::at(1))
        .unwrap()
        .turn_id;
    assert_eq!(
        read.store().turn_ops(a).unwrap(),
        faded.store().turn_ops(b).unwrap()
    );
    // The faded Turn of a scene is followed by its Closing Turn; the read
    // Turn is not. Before it, the worlds are the same.
    let close = faded
        .store()
        .turns(faded.campaign)
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(close.record.kind, TurnKind::Closing);
    let mut x = read.view();
    let mut y = faded
        .store()
        .snapshot_before(faded.campaign, close.seq)
        .unwrap();
    for v in [&mut x, &mut y] {
        v.recent_turns.clear();
        v.story_names.clear();
        v.turn_seq = 0;
    }
    assert_eq!(x, y);
}

/// Building the play view, the Journal and the OOC sheet makes no model
/// call, and the neutral line holds no name and no fact value.
#[test]
fn the_neutral_line_makes_no_model_call() {
    let mut rig = Rig::new("fade-line");
    let p = rig.plain();
    let t = play_fading(&mut rig, "t1", &p, TEN, &Fader::at(1))
        .unwrap()
        .turn_id;
    let calls = rig.gm.calls().len();
    let sight = rig.sight();
    let marks = Marks::default();
    let _ = play_view(&sight, &marks);
    let _ = journal_view(&sight, &marks);
    let _ = ooc_sheet(&sight);
    assert_eq!(rig.gm.calls().len(), calls);
    let line = neutral_line(&rig.turn(t)).unwrap();
    assert!(FADED_TEMPLATE.contains("{minutes}"));
    let view = rig.view();
    for e in &view.entities {
        assert!(!line.contains(&e.name), "{line} names {}", e.name);
    }
    for f in view.facts.iter().filter(|f| f.slot.value.len() > 2) {
        assert!(
            !line.contains(&f.slot.value),
            "{line} holds {}",
            f.slot.value
        );
    }
}

/// Path 23: the hidden text is in no later prompt, not in the Journal and
/// not on the Play screen. The debug log keeps it (owner's decision 5).
#[test]
fn a_faded_turn_is_in_no_later_prompt_and_not_in_the_journal() {
    let mut rig = Rig::new("fade-later");
    rig.chunked();
    let p = rig.plain();
    let t = play_fading(&mut rig, "t1", &p, TEN, &Fader::at(3))
        .unwrap()
        .turn_id;
    let p = rig.plain();
    rig.played("t2", &p, "Night.");
    let propose = rig.last_call(CallKind::Propose);
    let narrate = rig.last_call(CallKind::Narrate);
    let line = neutral_line(&rig.turn(t)).unwrap();
    for call in [&propose, &narrate] {
        let text = prompt_text(call);
        assert!(!text.contains(MARKER), "{text}");
        assert!(text.contains(&line), "{text}");
    }
    let sight = rig.sight();
    let marks = Marks::default();
    let journal = serde_json::to_string(&journal_view(&sight, &marks)).unwrap();
    let play = serde_json::to_string(&play_view(&sight, &marks)).unwrap();
    let ooc = ooc_sheet(&sight);
    for text in [&journal, &play, &ooc] {
        assert!(!text.contains(MARKER), "{text}");
    }
    assert!(journal.contains(&line), "the Journal shows the line");
    assert!(ooc.contains(&line), "the OOC sheet shows the line");
    let raw = narrate_row(&rig, "t1");
    assert!(raw.contains(MARKER) && raw.ends_with("[faded]"), "{raw}");
}

/// A fade in an Intimate Scene closes it: the faded Turn, then a Closing
/// Turn born faded (one proposal call, no narration call), `FadeToBlack`.
#[test]
fn a_fade_in_a_scene_closes_it_with_a_faded_closing_turn() {
    let mut rig = Rig::new("fade-scene");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let p = rig.intimate();
    let closing = rig.closing();
    rig.gm.push_proposal(serde_json::to_string(&p).unwrap());
    rig.gm
        .push_proposal(serde_json::to_string(&closing).unwrap());
    rig.gm.push_narration("Brann stays.".into());
    let before = rig.gm.calls().len();
    let out = run_fading(&mut rig, "t2", &Fader::at(1)).unwrap();
    assert!(out.faded);
    assert_eq!(rig.scene(), None);
    let turns = rig.store().turns(rig.campaign).unwrap();
    let close = turns.last().unwrap();
    assert_eq!(close.record.kind, TurnKind::Closing);
    assert!(close.faded);
    assert_eq!(close.narration, None);
    assert_eq!(rig.scene_ops(close.id), [(false, SceneWhy::FadeToBlack)]);
    let kinds: Vec<CallKind> = rig.gm.calls()[before..].iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        [CallKind::Propose, CallKind::Narrate, CallKind::Propose]
    );
    // The Turns already read keep their text; the faded one shows its line.
    let view = play_view(&rig.sight(), &Marks::default());
    assert_eq!(view.scene, SceneState::Plain);
    assert!(!view.can_correct);
    let faded = view
        .story
        .iter()
        .find(|t| t.turn_id == out.turn_id)
        .unwrap();
    assert!(matches!(faded.body, TurnBody::Faded { .. }));
    assert_eq!(faded.intent, "I stay with Brann.");

    // No Turn runs and a scene is open: a Closing Turn born faded.
    let mut rig = Rig::new("fade-idle-scene");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    let closing = rig.closing();
    rig.gm
        .push_proposal(serde_json::to_string(&closing).unwrap());
    let before = rig.gm.calls().len();
    let out = rig.engine().fade("f1".into()).unwrap();
    assert!(out.faded);
    assert_eq!(rig.scene(), None);
    let kinds: Vec<CallKind> = rig.gm.calls()[before..].iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [CallKind::Propose]);
    let close = rig.turn(out.turn_id);
    assert_eq!(close.record.kind, TurnKind::Closing);
    assert!(close.faded);
    assert!(
        !rig.turn(t1).faded,
        "the Turns already read keep their text"
    );
}

#[test]
fn fade_with_no_turn_and_no_scene_is_nothing_to_fade() {
    let mut rig = Rig::new("fade-nothing");
    let err = rig.engine().fade("f1".into()).unwrap_err();
    assert!(matches!(err, TurnError::NothingToFade), "{err:?}");
    assert!(rig.gm.calls().is_empty());
}

/// From the same state and the same recorded proposal, "End scene" and a
/// Fade to Black commit the same ops, except `SceneWhy`.
#[test]
fn fade_and_end_scene_commit_the_same_closing_ops() {
    let mut a = Rig::new("same-end");
    let mut b = Rig::new("same-fade");
    let p = a.intimate();
    a.played("t1", &p, "Brann nods.");
    b.played("t1", &p, "Brann nods.");
    let closing = a.closing();
    let ended = a
        .end_scene("c1", &closing, "Brann steps back.")
        .unwrap()
        .turn_id;
    b.gm.push_proposal(serde_json::to_string(&closing).unwrap());
    let faded = b.engine().fade("c1".into()).unwrap().turn_id;
    let why = |ops: Vec<LedgerOp<u64>>| -> Vec<LedgerOp<u64>> {
        ops.into_iter()
            .map(|op| match op {
                LedgerOp::SetScene { open, .. } => LedgerOp::SetScene {
                    open,
                    why: SceneWhy::EndScene,
                },
                op => op,
            })
            .collect()
    };
    let x = a.store().turn_ops(ended).unwrap();
    let y = b.store().turn_ops(faded).unwrap();
    assert_ne!(x, y, "the reason differs");
    assert_eq!(why(x), why(y));
}

#[test]
fn rebuild_keeps_the_fade_and_finds_no_text() {
    let mut rig = Rig::new("fade-rebuild");
    rig.chunked();
    let p = rig.plain();
    let t = play_fading(&mut rig, "t1", &p, TEN, &Fader::at(3))
        .unwrap()
        .turn_id;
    let path = rig.path.clone();
    let campaign = rig.campaign;
    rig.restart();
    drop(rig.engine.take());
    let mut store = Store::open(&path).unwrap();
    store.rebuild_projections(campaign).unwrap();
    let turn = store.turn(t).unwrap().unwrap();
    assert!(turn.faded);
    assert_eq!(turn.narration, None);
    let db = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        "SELECT count(*) FROM turn WHERE narration LIKE '%MARKER-FADE-1%'",
        "SELECT count(*) FROM ledger WHERE op_json LIKE '%MARKER-FADE-1%'",
        "SELECT count(*) FROM fact WHERE text LIKE '%MARKER-FADE-1%' OR value LIKE '%MARKER-FADE-1%'",
    ] {
        let n: i64 = db.query_row(sql, [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "{sql}");
    }
    let n: i64 = db
        .query_row(
            "SELECT count(*) FROM debug_log WHERE raw LIKE '%MARKER-FADE-1%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        n, 1,
        "the debug log keeps the text received before the stop"
    );
    rig.engine = Some(rig.make_engine(store));
}

#[test]
fn historical_view_keeps_the_fade() {
    let mut rig = Rig::new("fade-historical");
    let p = rig.plain();
    let t = play_fading(&mut rig, "t1", &p, "Dusk falls.", &Fader::at(1))
        .unwrap()
        .turn_id;
    let p = rig.plain();
    rig.played("t2", &p, "Night.");
    let seq = rig.turn(t).seq;
    let then = rig.store().snapshot_before(rig.campaign, seq + 1).unwrap();
    let turn = then.recent_turns.iter().find(|x| x.id == t).unwrap();
    assert!(turn.faded);
    assert_eq!(turn.narration, None);
}

#[test]
fn renarrate_refuses_a_faded_turn() {
    let mut rig = Rig::new("fade-renarrate");
    let p = rig.plain();
    let t = play_fading(&mut rig, "t1", &p, "Dusk falls.", &Fader::at(1))
        .unwrap()
        .turn_id;
    let calls = rig.gm.calls().len();
    let err = rig.engine().renarrate(t, &mut |_| {}).unwrap_err();
    assert!(matches!(err, TurnError::Faded), "{err:?}");
    assert_eq!(rig.gm.calls().len(), calls);
    // No un-fade: the store refuses a Narration for a faded Turn.
    let path = rig.path.clone();
    drop(rig.engine.take());
    let mut store = Store::open(&path).unwrap();
    assert!(store.set_narration(t, "Dusk falls.").is_err());
    assert_eq!(store.turn(t).unwrap().unwrap().narration, None);
    rig.engine = Some(rig.make_engine(store));
}

#[test]
fn a_retry_of_a_faded_turn_gives_the_line_and_no_call() {
    let mut rig = Rig::new("fade-retry");
    let p = rig.plain();
    let out = play_fading(&mut rig, "t1", &p, "Dusk falls.", &Fader::at(1)).unwrap();
    let calls = rig.gm.calls().len();
    let again = rig
        .engine()
        .play_turn("t1".into(), "I stay with Brann.", &mut |_| {})
        .unwrap();
    assert_eq!(again.turn_id, out.turn_id);
    assert!(again.faded);
    assert_eq!(
        again.narration,
        neutral_line(&rig.turn(out.turn_id)).unwrap()
    );
    assert_eq!(rig.gm.calls().len(), calls);
}

/// No scene: a faded Play Turn is the last Turn and can be corrected. The
/// Correction is read in full; the reverted Turn stays as its line.
#[test]
fn a_faded_play_turn_can_be_corrected() {
    let mut rig = Rig::new("fade-correct");
    let p = rig.plain();
    let t = play_fading(&mut rig, "t1", &p, "Dusk falls.", &Fader::at(1))
        .unwrap()
        .turn_id;
    assert!(rig.sight().can_correct);
    rig.gm.push_proposal(serde_json::to_string(&p).unwrap());
    rig.gm.push_narration("Dusk again.".into());
    let c = rig
        .engine()
        .correct_last("c1".into(), "I wait.", "slower", &mut |_| {})
        .unwrap();
    assert!(!c.faded);
    let corrected = rig.turn(c.turn_id);
    assert_eq!(corrected.narration.as_deref(), Some("Dusk again."));
    let view = play_view(&rig.sight(), &Marks::default());
    let old = view.story.iter().find(|x| x.turn_id == t).unwrap();
    assert!(old.reverted);
    assert!(matches!(old.body, TurnBody::Faded { .. }));
}

/// Review focus 5: the app stops between `set_faded` and the Closing Turn.
/// After a restart the Turn shows faded, the scene is still open, and "End
/// scene" closes it.
#[test]
fn a_restart_between_fade_and_closing_keeps_a_faded_turn_and_an_open_scene() {
    let mut rig = Rig::new("fade-restart");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    let path = rig.path.clone();
    drop(rig.engine.take());
    let mut store = Store::open(&path).unwrap();
    store.set_faded(t1).unwrap();
    rig.engine = Some(rig.make_engine(store));
    rig.restart();
    assert_eq!(rig.scene(), Some(t1));
    let view = play_view(&rig.sight(), &Marks::default());
    assert!(matches!(view.scene, SceneState::Intimate { .. }));
    let story = view.story.iter().find(|x| x.turn_id == t1).unwrap();
    assert!(matches!(story.body, TurnBody::Faded { .. }));
    let closing = rig.closing();
    rig.end_scene("c1", &closing, "Brann steps back.").unwrap();
    assert_eq!(rig.scene(), None);
}

/// The Player pressed the button while the Narration streamed, but the
/// Turn ended before the flag was read: the shell then calls `fade` with
/// that Turn's op. The latest Turn is faded now, and its text leaves the
/// screens. An older Turn, or the Opening, is never faded this way.
#[test]
fn a_fade_that_names_the_turn_that_just_ended_fades_it() {
    let mut rig = Rig::new("fade-late");
    let p = rig.plain();
    let t1 = rig.played("t1", &p, "Dusk falls.");
    let calls = rig.gm.calls().len();
    let out = rig.engine().fade("t1".into()).unwrap();
    assert_eq!(out.turn_id, t1);
    assert!(out.faded);
    let turn = rig.turn(t1);
    assert!(turn.faded);
    assert_eq!(turn.narration, None);
    assert_eq!(rig.gm.calls().len(), calls, "no model call");

    // An older Turn stays as it was read.
    let t2 = rig.played("t2", &p, "Night.");
    let t3 = rig.played("t3", &p, "Rain.");
    let out = rig.engine().fade("t2".into()).unwrap();
    assert_eq!(out.turn_id, t2);
    assert!(!out.faded);
    assert!(!rig.turn(t2).faded);
    assert!(!rig.turn(t3).faded);

    // In a scene, the late fade also closes the scene, faded.
    let mut rig = Rig::new("fade-late-scene");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    let closing = rig.closing();
    rig.gm
        .push_proposal(serde_json::to_string(&closing).unwrap());
    rig.engine().fade("t1".into()).unwrap();
    assert!(rig.turn(t1).faded);
    assert_eq!(rig.scene(), None);
}
