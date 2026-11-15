//! The Intimate Scene (mature content module, Task 4): the scene state the
//! ledger keeps, the automatic endings of data shape 3.4, the gate before
//! the proposal call (G1), and what each prompt may read of a scene.
//! Neutral fixtures and marker words only.

mod support;

use std::{fs, path::Path};
use threshold_core::domain::Intimacy;

use support::{rig::*, *};
use threshold_core::{
    CallKind, Gm, GmError, HandleContext, Prompt, Stream,
    checks::{CheckKind, settle_closing, validate},
    closing_schema_for,
    context::{
        ADVANCE_LINE, CLOSING_ASK, CLOSING_LINE, EXPLICIT_LINE, IMPLIED_LINE, PACK_SCENE_HEAD,
        SCENE_LINE, opening_prompt,
    },
    domain::{
        ActionKind, DecisionProposal, DetailLevel, Handle, LedgerOp, SCENE_PLAN_MAX_CHARS,
        SceneWhy, Seen, Stance, Verdict,
    },
    mature::{
        CLOSING_NARRATION_MAX_TOKENS, INTIMATE_TEMPLATE, SCENE_CLOSE_DEFAULT_MIN,
        SCENE_CLOSE_MAX_MIN, SCENE_NARRATION_MAX_TOKENS, SCENE_PLAIN_MAX_MIN,
        SCENE_PLAIN_TURNS_MAX, neutral_line, plain_run, stored_intimate,
    },
    pack::load_all,
    store::{Store, StoredTurn, TurnId, TurnKind, WorldView},
    turn::{TurnControl, TurnError},
    view::{SceneState, journal_view, play_view},
};

/// A marker word in a scene Narration: it must never reach a prompt that
/// holds no clearance.
const MARKER: &str = "MARKER-SCENE-1";

// --- Data shape 3.4, one test per row -----------------------------------------

/// Row 1: a closed scene, an intimate Turn that passes G2 and G3.
#[test]
fn an_intimate_turn_opens_a_scene() {
    let mut rig = Rig::new("opens");
    assert_eq!(rig.scene(), None);
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    assert_eq!(rig.scene(), Some(t1));
    assert_eq!(rig.scene_ops(t1), [(true, SceneWhy::Opened)]);
}

/// Row 2: a closed scene, a plain Turn.
#[test]
fn a_plain_turn_opens_no_scene() {
    let mut rig = Rig::new("plain");
    let p = rig.plain();
    let t1 = rig.played("t1", &p, "Dusk falls.");
    assert_eq!(rig.scene(), None);
    assert!(rig.scene_ops(t1).is_empty());
}

/// Row 3 (changed in the polish pass, 2026-11-12): an open scene, a plain
/// Turn of `SCENE_PLAIN_MAX_MIN` minutes or less with no move and no
/// arrival. A line of talk in the middle of a scene keeps it open: the
/// screen still shows the scene, "End scene" still closes it, and the next
/// intimate Turn is a Turn of the same scene.
#[test]
fn a_short_plain_turn_keeps_the_scene_open() {
    let mut rig = Rig::new("plain-keeps");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    let mut plain = rig.plain();
    plain.minutes = SCENE_PLAIN_MAX_MIN;
    let t2 = rig.played("t2", &plain, "Dusk falls.");
    assert_eq!(rig.scene(), Some(t1));
    assert!(rig.scene_ops(t2).is_empty());
    let view = play_view(&rig.sight(), &Default::default());
    assert_eq!(view.scene, SceneState::Intimate { since_turn: t1 });
    let t3 = rig.played("t3", &p, "Brann stays.");
    assert_eq!(rig.scene(), Some(t1), "the same scene");
    assert!(rig.scene_ops(t3).is_empty());
    let close = rig.closing();
    let end = rig
        .end_scene("end", &close, "The evening goes on.")
        .unwrap();
    assert_eq!(rig.scene_ops(end.turn_id), [(false, SceneWhy::EndScene)]);
    assert_eq!(rig.scene(), None);
}

/// Row 3: "End scene" works right after a plain Turn of the scene.
#[test]
fn end_scene_works_after_a_plain_turn() {
    let mut rig = Rig::new("plain-end");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let plain = rig.plain();
    rig.played("t2", &plain, "Dusk falls.");
    let close = rig.closing();
    let end = rig
        .end_scene("end", &close, "The evening goes on.")
        .unwrap();
    assert_eq!(rig.scene_ops(end.turn_id), [(false, SceneWhy::EndScene)]);
    assert_eq!(
        play_view(&rig.sight(), &Default::default()).scene,
        SceneState::Plain
    );
}

/// Row 4 (changed in the polish pass): an open scene, a plain Turn that
/// takes more than `SCENE_PLAIN_MAX_MIN` minutes closes it.
#[test]
fn a_long_plain_turn_closes_the_scene() {
    let mut rig = Rig::new("plain-long");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let mut plain = rig.plain();
    plain.action = ActionKind::Wait;
    plain.minutes = 60;
    rig.gm.push_proposal(serde_json::to_string(&plain).unwrap());
    rig.gm.push_narration("An hour passes.".into());
    let t2 = rig
        .engine()
        .play_turn("t2".into(), "I wait here for an hour.", &mut |_| {})
        .unwrap()
        .turn_id;
    assert!(SCENE_PLAIN_MAX_MIN < 60);
    assert_eq!(rig.scene(), None);
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::NotIntimate)]);
}

/// Rows 2 and 3 for a plain Turn (polish pass): a move or an arrival the
/// PC sees closes the scene whatever the tag.
#[test]
fn a_plain_move_or_arrival_closes_the_scene() {
    let mut rig = Rig::new("plain-move");
    let yard = rig.place("Mill Yard");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let mut plain = rig.plain();
    plain.action = ActionKind::Move;
    plain.move_to = Some(Handle::Entity(yard));
    let t2 = rig.played("t2", &plain, "The yard is quiet.");
    assert_eq!(rig.scene(), None);
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Moved)]);

    let mut rig = Rig::new("plain-arrival");
    let kett = rig.add_npc("Kett", 34, true, "far road");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    rig.arrival(kett);
    let plain = rig.plain();
    let t2 = rig.played("t2", &plain, "Someone comes.");
    assert_eq!(rig.scene(), None);
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Arrival)]);
}

/// A plain Turn inside an open scene is told as a plain Turn (polish pass):
/// at `explicit`, with a pack that gives explicit description, its
/// narration prompt holds no intimate line, no pack scene fragment and no
/// scene Narration, and it gets no scene cap. Its proposal prompt is the
/// scene's (the model needs the scene to tag the Turn).
#[test]
fn a_plain_turn_inside_a_scene_is_told_plain() {
    let (packs, _) = load_all(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/packs"));
    let pack = packs
        .into_iter()
        .find(|p| p.explicit())
        .expect("the test pack gives explicit description");
    let mut rig = Rig::with("plain-told", DetailLevel::Explicit, vec![pack]);
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    let scene = rig.last_call(CallKind::Narrate);
    assert!(
        scene.prompt.recent.contains(EXPLICIT_LINE),
        "the scene Turn"
    );
    let plain = rig.plain();
    rig.played("t2", &plain, "Dusk falls.");
    assert_eq!(rig.scene(), Some(t1));
    let propose = rig.last_call(CallKind::Propose);
    assert!(propose.prompt.recent.starts_with(SCENE_LINE));
    let narrate = rig.last_call(CallKind::Narrate);
    let text = prompt_text(&narrate);
    for line in [EXPLICIT_LINE, IMPLIED_LINE, PACK_SCENE_HEAD, MARKER] {
        assert!(!text.contains(line), "{line}: {text}");
    }
    assert!(
        text.contains(&neutral_line(&rig.turn(t1)).unwrap()),
        "{text}"
    );
    assert_eq!(narrate.max_tokens, None);
    // The scene goes on: the next intimate Turn reads the scene in full.
    rig.played("t3", &p, "Brann stays.");
    let narrate = rig.last_call(CallKind::Narrate);
    assert!(narrate.prompt.recent.contains(EXPLICIT_LINE));
    assert!(narrate.prompt.recent.contains(MARKER));
}

/// Row 4: an open scene, an intimate Turn in which the PC moves.
#[test]
fn a_move_closes_the_scene() {
    let mut rig = Rig::new("move");
    let yard = rig.place("Mill Yard");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let p = rig.intimate_move(yard);
    let t2 = rig.played("t2", &p, "The yard is quiet.");
    assert_eq!(rig.scene(), None);
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Moved)]);
}

/// Row 5: an open scene, an intimate Turn in which a character the PC sees
/// arrives.
#[test]
fn an_arrival_the_pc_sees_closes_the_scene() {
    let mut rig = Rig::new("arrival");
    let kett = rig.add_npc("Kett", 34, true, "far road");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    rig.arrival(kett);
    let p = rig.intimate();
    let t2 = rig.played("t2", &p, "Someone comes.");
    assert!(
        threshold_core::context::seen_present(&rig.view()).contains(&kett),
        "the PC sees Kett"
    );
    assert_eq!(rig.scene(), None);
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Arrival)]);
}

/// Row 6: an open scene, an intimate Turn with no move and no arrival.
#[test]
fn an_intimate_turn_keeps_the_scene_open() {
    let mut rig = Rig::new("keeps");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    let t2 = rig.played("t2", &p, "Brann stays.");
    assert_eq!(rig.scene(), Some(t1));
    assert!(rig.scene_ops(t2).is_empty());
}

/// "Another character arrives" reads what the PC perceives: a confirmed
/// adult who arrives unseen does not end the scene. The age rule reads
/// world truth, and the adult passes it.
#[test]
fn a_hidden_adult_arrival_keeps_the_scene_open() {
    let mut rig = Rig::new("hidden-arrival");
    let vale = rig.vale;
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    rig.arrival(vale);
    let t2 = rig.played("t2", &p, "Brann stays.");
    let view = rig.view();
    assert!(view.present().contains(&vale), "Vale is here");
    assert!(!threshold_core::context::seen_present(&view).contains(&vale));
    assert_eq!(rig.scene(), Some(t1));
    assert!(rig.scene_ops(t2).is_empty());
}

/// A Turn that closes a scene never opens one: the next intimate Turn
/// opens a new scene.
#[test]
fn a_turn_that_closes_a_scene_never_opens_one() {
    let mut rig = Rig::new("never-opens");
    let yard = rig.place("Mill Yard");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let p = rig.intimate_move(yard);
    let t2 = rig.played("t2", &p, "The yard is quiet.");
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Moved)]);
    assert_eq!(
        rig.scene(),
        None,
        "an intimate Turn that closes stays closed"
    );
    let p = rig.intimate();
    let t3 = rig.played("t3", &p, "Brann follows.");
    assert_eq!(rig.scene(), Some(t3), "a new scene");
    assert_eq!(rig.scene_ops(t3), [(true, SceneWhy::Opened)]);
}

// --- G1 -----------------------------------------------------------------------

/// G1: another program brings an unconfirmed NPC into the place of an open
/// scene. The next Turn is planned as a plain one: its proposal prompt holds
/// no scene line and no scene Narration, and its Commit closes the scene
/// with `why: Gate`.
#[test]
fn a_scene_that_is_not_cleared_is_closed_before_the_proposal_call() {
    let mut rig = Rig::new("g1");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    assert_eq!(rig.scene(), Some(t1));
    rig.add_npc("Kett", 34, false, "gate square");
    let p = rig.plain();
    let t2 = rig.played("t2", &p, "Dusk falls.");
    let call = rig.last_call(CallKind::Propose);
    let text = prompt_text(&call);
    assert!(!text.contains(SCENE_LINE), "{}", call.prompt.recent);
    assert!(!text.contains(MARKER), "{}", call.prompt.recent);
    let line = neutral_line(&rig.turn(t1)).unwrap();
    assert!(call.prompt.recent.contains(&line), "{}", call.prompt.recent);
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Gate)]);
    assert_eq!(rig.scene(), None);
}

/// G1 in a Correction: the Correction plans on the state before the Turn it
/// corrects, through the same gate as a Play Turn.
#[test]
fn a_correction_in_a_scene_runs_g1() {
    // The state before the corrected Turn holds a cleared open scene: the
    // Correction's prompt holds the scene line and the scene Turn in full.
    let mut rig = Rig::new("g1-correction");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    let plain = rig.plain();
    long_plain(&mut rig, "t2");
    assert_eq!(rig.scene(), None, "the long plain Turn closed the scene");
    rig.gm.push_proposal(serde_json::to_string(&plain).unwrap());
    rig.gm.push_narration("Dusk again.".into());
    let c = rig
        .engine()
        .correct_last("c1".into(), "I wait.", "slower", &mut |_| {})
        .unwrap();
    let call = rig.last_call(CallKind::Propose);
    assert!(call.prompt.recent.starts_with(SCENE_LINE));
    assert!(call.prompt.recent.contains(MARKER));
    assert!(rig.scene_ops(c.turn_id).is_empty(), "a short plain Turn");
    assert_eq!(rig.scene(), Some(t1));

    // The state before the corrected Turn holds an open scene that is not
    // cleared: the Correction plans as a plain one and closes it by G1.
    let mut rig = Rig::new("g1-correction-2");
    let p = rig.intimate();
    rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    rig.add_npc("Kett", 34, false, "gate square");
    let plain = rig.plain();
    let t2 = rig.played("t2", &plain, "Dusk.");
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Gate)]);
    rig.gm.push_proposal(serde_json::to_string(&plain).unwrap());
    rig.gm.push_narration("Dusk again.".into());
    let c = rig
        .engine()
        .correct_last("c1".into(), "I wait.", "slower", &mut |_| {})
        .unwrap();
    let call = rig.last_call(CallKind::Propose);
    assert!(!call.prompt.recent.contains(SCENE_LINE));
    assert!(!call.prompt.recent.contains(MARKER));
    assert_eq!(rig.scene_ops(c.turn_id), [(false, SceneWhy::Gate)]);
    assert_eq!(rig.scene(), None);
}

// --- Restart, oracle, Correction ---------------------------------------------

#[test]
fn the_scene_state_survives_a_restart() {
    let mut rig = Rig::new("restart");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    rig.restart();
    assert_eq!(rig.scene(), Some(t1));
}

/// The rule the projection must equal (data shape 3.4, "why an op";
/// changed in the polish pass, 2026-11-12): over the standing Play,
/// Correction and Closing Turns, the scene starts at the first Turn
/// committed with `intimate` after the last Turn that closed a scene, and
/// stays open through the Turns after it. The rule reads the closing ops,
/// so it holds for the round 2 bound too: up to `SCENE_PLAIN_TURNS_MAX`
/// short plain Turns close nothing and do not end the scene.
fn last_turn_rule(store: &Store, campaign: u64) -> Option<TurnId> {
    let reverted = store.reverted_turns(campaign).unwrap();
    let standing: Vec<StoredTurn> = store
        .turns(campaign)
        .unwrap()
        .into_iter()
        .filter(|t| t.record.kind.has_intent() || t.record.kind == TurnKind::Closing)
        .filter(|t| !reverted.contains(&t.id))
        .collect();
    let mut since = None;
    for t in &standing {
        let closes = store
            .turn_ops(t.id)
            .unwrap()
            .iter()
            .any(|op| matches!(op, LedgerOp::SetScene { open: false, .. }));
        if closes {
            since = None;
        } else if since.is_none() && stored_intimate(&t.record.proposal_json) {
            since = Some(t.id);
        }
    }
    since
}

/// A plain Turn of an hour ("I wait here for an hour."): longer than
/// `SCENE_PLAIN_MAX_MIN`, so it closes an open scene.
fn long_plain(rig: &mut Rig, op: &str) -> TurnId {
    let mut plain = rig.plain();
    plain.action = ActionKind::Wait;
    plain.minutes = 60;
    rig.gm.push_proposal(serde_json::to_string(&plain).unwrap());
    rig.gm.push_narration("An hour passes.".into());
    rig.engine()
        .play_turn(op.into(), "I wait here for an hour.", &mut |_| {})
        .unwrap()
        .turn_id
}

/// One Correction of the last Play Turn, with `p` as its proposal.
fn correct(rig: &mut Rig, op: &str, p: &DecisionProposal) {
    rig.gm.push_proposal(serde_json::to_string(p).unwrap());
    rig.gm.push_narration("Again.".into());
    rig.engine()
        .correct_last(op.into(), "I stay with Brann.", "again", &mut |_| {})
        .unwrap();
}

/// Oracle: over a scripted run of Turns (plain, intimate, a short plain
/// Turn inside the scene, a long plain Turn, a seen arrival, a hidden
/// arrival, a move, two Corrections, a run of short plain Turns past
/// `SCENE_PLAIN_TURNS_MAX`), the projection equals the rule over the
/// standing Turns.
#[test]
fn scene_projection_matches_the_last_turn_rule() {
    let mut rig = Rig::new("oracle");
    let yard = rig.place("Mill Yard");
    let kett = rig.add_npc("Kett", 34, true, "far road");
    let vale = rig.vale;
    let (intimate, plain) = (rig.intimate(), rig.plain());
    let check = |rig: &Rig, step: &str| {
        assert_eq!(
            rig.scene(),
            last_turn_rule(rig.store(), rig.campaign),
            "after {step}"
        );
    };
    rig.played("t1", &plain, "Dusk.");
    check(&rig, "t1 plain");
    let t2 = rig.played("t2", &intimate, "Brann nods.");
    check(&rig, "t2 opens");
    rig.played("t3", &intimate, "Brann stays.");
    check(&rig, "t3 keeps");
    assert_eq!(rig.scene(), Some(t2));
    correct(&mut rig, "c3", &plain);
    check(&rig, "c3, a short plain Turn, keeps the scene of t2");
    assert_eq!(rig.scene(), Some(t2));
    rig.played("t3b", &plain, "Brann talks.");
    check(&rig, "t3b, a short plain Turn, keeps");
    rig.played("t3c", &intimate, "Brann stays.");
    check(&rig, "t3c goes on in the same scene");
    assert_eq!(rig.scene(), Some(t2));
    long_plain(&mut rig, "t3d");
    check(&rig, "t3d, a long plain Turn, closes");
    assert_eq!(rig.scene(), None);
    let t4 = rig.played("t4", &intimate, "Brann nods.");
    check(&rig, "t4 opens");
    long_plain(&mut rig, "t5");
    check(&rig, "t5 closes");
    correct(&mut rig, "c5", &intimate);
    check(&rig, "c5 keeps the scene t5 closed");
    assert_eq!(rig.scene(), Some(t4));
    rig.arrival(vale);
    rig.played("t6", &intimate, "Brann stays.");
    check(&rig, "t6 hidden arrival keeps");
    rig.arrival(kett);
    rig.played("t7", &intimate, "Someone comes.");
    check(&rig, "t7 seen arrival closes");
    assert_eq!(rig.scene(), None);
    rig.played("t8", &intimate, "Brann nods.");
    check(&rig, "t8 opens again");
    let p = rig.intimate_move(yard);
    rig.played("t9", &p, "The yard.");
    check(&rig, "t9 move closes");
    let t10 = rig.played("t10", &intimate, "Brann follows.");
    check(&rig, "t10 opens at the yard");
    rig.played("t11", &plain, "Quiet.");
    check(&rig, "t11 keeps");
    rig.played("t11b", &plain, "Quiet.");
    check(&rig, "t11b, the second short plain Turn, keeps");
    assert_eq!(rig.scene(), Some(t10));
    rig.played("t12", &plain, "Quiet.");
    check(&rig, "t12, past SCENE_PLAIN_TURNS_MAX, closes");
    assert_eq!(rig.scene(), None);
}

/// A scene Turn, then a plain Turn that closes it. Correcting the plain
/// Turn with an intimate proposal leaves the scene open, from the scene
/// Turn on.
#[test]
fn a_correction_reopens_the_scene_its_turn_closed() {
    let mut rig = Rig::new("reopens");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    long_plain(&mut rig, "t2");
    assert_eq!(rig.scene(), None);
    rig.gm.push_proposal(serde_json::to_string(&p).unwrap());
    rig.gm.push_narration("Brann stays.".into());
    let c = rig
        .engine()
        .correct_last("c1".into(), "I stay with Brann.", "I meant it", &mut |_| {})
        .unwrap();
    assert!(rig.scene_ops(c.turn_id).is_empty());
    assert_eq!(rig.scene(), Some(t1));
}

// --- What each prompt may read ----------------------------------------------

/// In an open cleared scene the proposal prompt holds the core's scene line
/// and the scene Turns in full, and so does the narration prompt.
#[test]
fn a_scene_prompt_holds_the_scene_line_and_full_turns() {
    let mut rig = Rig::new("full");
    let p = rig.intimate();
    rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    rig.played("t2", &p, "Brann stays.");
    let propose = rig.last_call(CallKind::Propose);
    assert!(
        propose.prompt.recent.starts_with(SCENE_LINE),
        "{}",
        propose.prompt.recent
    );
    assert!(propose.prompt.recent.contains(MARKER));
    let narrate = rig.last_call(CallKind::Narrate);
    assert!(narrate.prompt.recent.contains(MARKER));
    assert!(narrate.prompt.recent.contains(IMPLIED_LINE));
    // The scene line names the longer plan limit.
    assert!(SCENE_LINE.contains(&SCENE_PLAN_MAX_CHARS.to_string()));
    assert!(!SCENE_LINE.trim_end().contains('\n'), "one line");
}

/// The prompt prefix (system, stable, scene blocks) does not change when a
/// scene opens or ends: only the `recent` block does.
#[test]
fn opening_a_scene_keeps_the_prompt_prefix() {
    let mut rig = Rig::new("prefix");
    let (intimate, plain) = (rig.intimate(), rig.plain());
    // Before the scene, in it, and after it: the Turns change no fact.
    rig.played("t1", &intimate, "Brann nods.");
    let before = rig.last_call(CallKind::Propose);
    rig.played("t2", &intimate, "Brann stays.");
    let during = rig.last_call(CallKind::Propose);
    rig.played("t3", &plain, "Dusk.");
    let after = rig.last_call(CallKind::Propose);
    for (a, b) in [(&before, &during), (&during, &after)] {
        assert_eq!(a.prompt.system, b.prompt.system);
        assert_eq!(a.prompt.stable, b.prompt.stable);
        assert_eq!(a.prompt.scene, b.prompt.scene);
    }
    assert!(!before.prompt.recent.contains(SCENE_LINE));
    assert!(during.prompt.recent.starts_with(SCENE_LINE));
}

/// Review M3 (2026-11-12): the names a prompt finds in past Turns
/// (MENTIONED ENTITIES, and the handles of the schema) come through
/// `mature::turn_text` too. Kett, away on the far road, is named only in a
/// scene Narration and in the Intent of a faded Turn: once the scene has
/// ended, no proposal prompt and no schema names Kett. Inside the cleared
/// scene, the scene Narration is read in full, Kett included.
#[test]
fn a_past_turn_names_nobody_through_text_a_prompt_may_not_read() {
    let mut rig = Rig::new("mentioned");
    let kett = rig.add_npc("Kett", 34, true, "far road");
    let handle = format!("\"E{kett}\"");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods. Kett is far away.");
    rig.played("t2", &p, "Brann stays.");
    let in_scene = rig.last_call(CallKind::Propose);
    assert!(in_scene.prompt.recent.contains("Kett"), "read in full");
    let plain = rig.plain();
    long_plain(&mut rig, "t3");
    rig.played("t4", &plain, "Night.");
    let after = rig.last_call(CallKind::Propose);
    assert!(
        !prompt_text(&after).contains("Kett"),
        "{}",
        after.prompt.recent
    );
    assert!(!after.schema.to_string().contains(&handle));

    // A faded Turn: its Intent names Kett.
    rig.gm.push_proposal(serde_json::to_string(&plain).unwrap());
    rig.gm.push_narration("Unused.".into());
    let mut ctl = TurnControl {
        on_chunk: &mut |_| {},
        faded: &|| true,
    };
    let faded = rig
        .engine()
        .play_turn_with("t5".into(), "I ask where Kett is.", &mut ctl)
        .unwrap();
    assert!(faded.faded);
    rig.played("t6", &plain, "Night again.");
    let after = rig.last_call(CallKind::Propose);
    assert!(
        !prompt_text(&after).contains("Kett"),
        "{}",
        after.prompt.recent
    );
    assert!(!after.schema.to_string().contains(&handle));
}

/// Review M2 (2026-11-12): a cleared scene reads in full only its own
/// Turns. The Narration of an earlier scene is its Neutral Line in every
/// prompt of the next scene (`mature::turn_text`: the scene started at or
/// before the Turn).
#[test]
fn an_earlier_scene_is_in_no_prompt_of_the_next_scene() {
    const EARLIER: &str = "MARKER-SCENE-EARLIER";
    let mut rig = Rig::new("two-scenes");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{EARLIER} Brann nods."));
    long_plain(&mut rig, "t2");
    assert_eq!(rig.scene(), None);
    let t3 = rig.played("t3", &p, &format!("{MARKER} Brann returns."));
    assert_eq!(rig.scene(), Some(t3), "a second scene");
    rig.played("t4", &p, "Brann stays.");
    let line = neutral_line(&rig.turn(t1)).unwrap();
    for kind in [CallKind::Propose, CallKind::Narrate] {
        let call = rig.last_call(kind);
        let text = prompt_text(&call);
        assert!(text.contains(MARKER), "{kind:?}: the open scene in full");
        assert!(!text.contains(EARLIER), "{kind:?}: {text}");
        assert!(call.prompt.recent.contains(&line), "{kind:?}: {text}");
    }
}

/// After a plain Turn closes the scene, the next prompts hold each scene
/// Turn's neutral line and none of its Narration.
#[test]
fn scene_narration_is_in_no_prompt_after_the_scene() {
    let mut rig = Rig::new("after");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    let t2 = rig.played("t2", &p, &format!("{MARKER} Brann stays."));
    let plain = rig.plain();
    long_plain(&mut rig, "t3");
    let closing_narration = rig.last_call(CallKind::Narrate);
    rig.played("t4", &plain, "Night.");
    let next_proposal = rig.calls(CallKind::Propose).pop().unwrap();
    let next_narration = rig.last_call(CallKind::Narrate);
    let lines = [
        neutral_line(&rig.turn(t1)).unwrap(),
        neutral_line(&rig.turn(t2)).unwrap(),
    ];
    for call in [&closing_narration, &next_proposal, &next_narration] {
        let text = prompt_text(call);
        assert!(!text.contains(MARKER), "{:?}: {}", call.kind, text);
        assert!(!text.contains(SCENE_LINE));
        for line in &lines {
            assert!(text.contains(line.as_str()), "{:?}: {}", call.kind, text);
        }
    }
    // The template is fixed text: minutes, no name and no value.
    assert!(lines[0].contains('5'), "{}", lines[0]);
    assert!(INTIMATE_TEMPLATE.contains("{minutes}"));
    for name in ["Ada", "Brann", "Orsel", "gate"] {
        assert!(!lines[0].contains(name), "{}", lines[0]);
    }
}

/// The plan limit rises to `SCENE_PLAN_MAX_CHARS` in a cleared scene only,
/// in the per-Turn schema and in `validate`.
#[test]
fn the_plan_limit_rises_in_a_cleared_scene_only() {
    let mut rig = Rig::new("plan");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let outside = rig.last_call(CallKind::Propose).schema;
    rig.played("t2", &p, "Brann stays.");
    let inside = rig.last_call(CallKind::Propose).schema;
    let plan =
        |schema: &serde_json::Value| schema["$defs"]["Reaction"]["properties"]["plan"].clone();
    assert_eq!(plan(&outside)["maxLength"], 90);
    assert!(plan(&outside).get("minLength").is_none());
    assert_eq!(plan(&inside)["maxLength"], SCENE_PLAN_MAX_CHARS);
    assert_eq!(plan(&inside)["minLength"], 1);

    let with_plan = |chars: usize| {
        let mut p = rig.intimate();
        p.reactions[0].plan = "x".repeat(chars);
        p
    };
    let refs = |p: &DecisionProposal, view: &WorldView| {
        validate(p, view)
            .err()
            .unwrap_or_default()
            .iter()
            .filter(|r| r.check == CheckKind::References)
            .count()
    };
    let scene = rig.view();
    assert!(scene.scene_open.is_some());
    assert_eq!(refs(&with_plan(200), &scene), 0);
    assert_eq!(refs(&with_plan(201), &scene), 1);
    assert_eq!(
        refs(&with_plan(0), &scene),
        1,
        "a scene plan is never empty"
    );
    let mut plain = scene.clone();
    plain.scene_open = None;
    assert_eq!(refs(&with_plan(90), &plain), 0);
    assert_eq!(refs(&with_plan(91), &plain), 1);
    assert_eq!(refs(&with_plan(0), &plain), 0);
}

/// A Narration told by implication inside a scene carries the token cap;
/// any other Narration carries none.
#[test]
fn a_scene_narration_is_capped() {
    let mut rig = Rig::new("cap");
    let plain = rig.plain();
    rig.played("t0", &plain, "Dusk.");
    assert_eq!(rig.last_call(CallKind::Narrate).max_tokens, None);
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    assert_eq!(
        rig.last_call(CallKind::Narrate).max_tokens,
        Some(SCENE_NARRATION_MAX_TOKENS)
    );
    rig.played("t2", &p, "Brann stays.");
    assert_eq!(rig.last_call(CallKind::Narrate).max_tokens, Some(280));
    rig.played("t3", &plain, "Dusk.");
    assert_eq!(rig.last_call(CallKind::Narrate).max_tokens, None);

    // Review minor (2026-11-12): one rule. A Turn told as intimate gets the
    // scene cap, the Turn that ends its scene by a move or an arrival
    // included, as a `renarrate` of an ended scene does.
    let yard = rig.place("Mill Yard");
    rig.played("t4", &p, "Brann nods.");
    rig.played("t5", &rig.intimate_move(yard), "The yard is quiet.");
    assert_eq!(rig.scene(), None, "the move closed the scene");
    assert_eq!(rig.last_call(CallKind::Narrate).max_tokens, Some(280));
    let mut rig = Rig::new("cap-arrival");
    let kett = rig.add_npc("Kett", 34, true, "far road");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    rig.arrival(kett);
    rig.played("t2", &p, "Someone comes.");
    assert_eq!(rig.scene(), None, "the arrival closed the scene");
    assert_eq!(rig.last_call(CallKind::Narrate).max_tokens, Some(280));
}

/// G4 of a `renarrate`: a scene Turn narrated again after the scene ended
/// is told by implication, and the earlier scene Turns reach it as their
/// neutral lines only.
#[test]
fn renarrate_of_an_ended_scene_is_implied() {
    let mut rig = Rig::new("renarrate");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    let t2 = rig.played("t2", &p, "Brann stays.");
    long_plain(&mut rig, "t3");
    rig.gm.push_narration("Brann stays again.".into());
    rig.engine().renarrate(t2, &mut |_| {}).unwrap();
    let call = rig.last_call(CallKind::Narrate);
    assert!(call.prompt.recent.starts_with(IMPLIED_LINE));
    assert!(!prompt_text(&call).contains(MARKER));
    let line = neutral_line(&rig.turn(t1)).unwrap();
    assert!(call.prompt.recent.contains(&line), "{}", call.prompt.recent);
}

/// Path 22: the Opening prompt and the OOC prompt of a Campaign with an
/// open scene hold no scene line, no implied or explicit line, no scene
/// Narration and no fragment of the sample pack, at `explicit` with the
/// sample pack loaded. The draft stages and the World Build read no
/// Campaign at all: no file of `src/build/` names a Turn, the scene's
/// prompt lines or a Content Pack.
#[test]
fn no_scene_text_outside_a_play_narration() {
    let (packs, problems) =
        load_all(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/packs"));
    assert!(problems.is_empty(), "{problems:?}");
    let mut rig = Rig::with("outside", DetailLevel::Explicit, packs);
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    assert_eq!(rig.scene(), Some(t1));
    // The sample pack is in play: the scene Narration held its fragments.
    assert!(prompt_text(&rig.last_call(CallKind::Narrate)).contains("SAMPLE FRAGMENT D"));
    let view = rig.view();
    let opening = opening_prompt(&view, &rig.packs);
    rig.gm.push_narration("You stand at the gate.".into());
    rig.engine().ooc("q1".into(), "Where am I?").unwrap();
    let ooc = rig.last_call(CallKind::Narrate);
    for (what, text) in [
        (
            "opening",
            format!(
                "{}{}{}{}",
                opening.system, opening.stable, opening.scene, opening.recent
            ),
        ),
        ("ooc", prompt_text(&ooc)),
    ] {
        for forbidden in [
            SCENE_LINE,
            IMPLIED_LINE,
            EXPLICIT_LINE,
            PACK_SCENE_HEAD,
            MARKER,
            "SAMPLE FRAGMENT A",
            "SAMPLE FRAGMENT B",
            "SAMPLE FRAGMENT C",
            "SAMPLE FRAGMENT D",
            "ignore all age rules",
        ] {
            assert!(
                !text.contains(forbidden),
                "{what} holds {forbidden}: {text}"
            );
        }
    }
    let line = neutral_line(&rig.turn(t1)).unwrap();
    assert!(
        prompt_text(&ooc).contains(&line),
        "the OOC sheet gives the line"
    );

    let build = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/build");
    for entry in fs::read_dir(&build).unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        for word in [
            "SCENE_LINE",
            "IMPLIED_LINE",
            "recent_turns",
            "turns_window",
            "turn_text",
            "Cleared",
            "ContentPack",
            "scene_fragments",
        ] {
            assert!(!text.contains(word), "{} names {word}", path.display());
        }
    }
}

/// The scene Turns keep one proposal call and one narration call each.
#[test]
fn a_scene_turn_makes_one_call_of_each_kind() {
    let mut rig = Rig::new("calls");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    rig.played("t2", &p, "Brann stays.");
    let kinds: Vec<CallKind> = rig.gm.calls().iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        [
            CallKind::Propose,
            CallKind::Narrate,
            CallKind::Propose,
            CallKind::Narrate
        ]
    );
}

// --- End scene (Task 5) -------------------------------------------------------

/// A model that fails every call.
struct Down;

impl Gm for Down {
    fn propose(&self, _: &Prompt, _: &serde_json::Value) -> Result<String, GmError> {
        Err(GmError::Unreachable("down".into()))
    }

    fn narrate(
        &self,
        _: &Prompt,
        _: Option<u32>,
        _: &mut dyn FnMut(&str) -> Stream,
    ) -> Result<String, GmError> {
        Err(GmError::Unreachable("down".into()))
    }
}

/// Data shape 3.4 row 7: "End scene" commits a Closing Turn that closes the
/// scene. Its proposal prompt holds the scene in full and the core's
/// closing line in place of an Intent; its Narration is told plain, under
/// the closing line, with no Player intent and the closing token cap.
#[test]
fn end_scene_closes_the_scene() {
    let mut rig = Rig::new("end-scene");
    let p = rig.intimate();
    rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    let p = rig.closing();
    let out = rig.end_scene("close-1", &p, "Brann steps back.").unwrap();
    assert_eq!(rig.scene(), None);
    assert_eq!(rig.scene_ops(out.turn_id), [(false, SceneWhy::EndScene)]);
    let turn = rig.turn(out.turn_id);
    assert_eq!(turn.record.kind, TurnKind::Closing);
    assert_eq!(turn.record.intent, "");
    assert_eq!(turn.narration.as_deref(), Some("Brann steps back."));
    assert_eq!(turn.record.clock_after - turn.record.clock_before, 10);

    let propose = rig.last_call(CallKind::Propose);
    assert!(propose.prompt.recent.starts_with(SCENE_LINE));
    assert!(propose.prompt.recent.contains(MARKER), "the scene in full");
    assert!(propose.prompt.recent.contains(CLOSING_ASK));
    assert!(!propose.prompt.recent.contains("INTENT\n"));
    assert_eq!(
        propose.schema["properties"]["action"]["enum"],
        serde_json::json!(["wait"])
    );

    let narrate = rig.last_call(CallKind::Narrate);
    assert!(
        narrate.prompt.recent.starts_with(CLOSING_LINE),
        "{}",
        narrate.prompt.recent
    );
    assert!(!narrate.prompt.recent.contains("Player intent"));
    assert!(!narrate.prompt.recent.contains(IMPLIED_LINE));
    assert!(!prompt_text(&narrate).contains(MARKER), "told plain");
    assert_eq!(narrate.max_tokens, Some(CLOSING_NARRATION_MAX_TOKENS));
    assert_eq!(CLOSING_NARRATION_MAX_TOKENS, 90);
    for line in [CLOSING_LINE, CLOSING_ASK] {
        assert!(!line.trim_end().contains('\n'), "one line: {line}");
    }
}

/// No scene: `NoScene`, and no model call.
#[test]
fn end_scene_without_a_scene_fails() {
    let mut rig = Rig::new("no-scene");
    let p = rig.closing();
    let err = rig
        .end_scene("close-1", &p, "Brann steps back.")
        .unwrap_err();
    assert!(matches!(err, TurnError::NoScene), "{err:?}");
    assert!(rig.gm.calls().is_empty());
    assert!(
        rig.store()
            .turn_by_campaign_op(rig.campaign, &"close-1".to_owned())
            .unwrap()
            .is_none()
    );
}

/// The button always works: with a model that fails every call, the core
/// commits its default closing (the default minutes and the `SetScene` op,
/// no reaction, no fact). The Turn is committed; its Narration failed.
#[test]
fn end_scene_closes_when_the_model_fails() {
    let mut rig = Rig::new("end-down");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let before = rig.view().clock_min;
    rig.with_gm(Box::new(Down));
    let mut ctl = TurnControl {
        on_chunk: &mut |_| {},
        faded: &|| false,
    };
    let err = rig
        .engine()
        .end_scene("close-1".into(), &mut ctl)
        .unwrap_err();
    let TurnError::NarrationFailed { turn_id, .. } = err else {
        panic!("expected a committed Turn with no Narration, got {err:?}");
    };
    assert_eq!(rig.scene(), None);
    let ops = rig.store().turn_ops(turn_id).unwrap();
    assert_eq!(
        ops,
        [
            LedgerOp::AdvanceClock {
                minutes: SCENE_CLOSE_DEFAULT_MIN
            },
            LedgerOp::SetScene {
                open: false,
                why: SceneWhy::EndScene
            },
        ]
    );
    assert_eq!(
        rig.view().clock_min,
        before + u64::from(SCENE_CLOSE_DEFAULT_MIN)
    );
    let turn = rig.turn(turn_id);
    assert_eq!(turn.record.kind, TurnKind::Closing);
    assert_eq!(turn.narration, None);

    // Two rejected attempts end the same way.
    let mut rig = Rig::new("end-rejected");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let mut bad = rig.closing();
    bad.reactions = vec![reaction(e(rig.ada), Stance::Comply, None)];
    bad.facts = vec![fact(slot(e(rig.ada), "mood", "calm"), Seen::Pc, vec![])];
    rig.gm.push_proposal(serde_json::to_string(&bad).unwrap());
    let out = rig.end_scene("close-2", &bad, "Brann steps back.").unwrap();
    assert_eq!(rig.scene(), None);
    assert_eq!(
        rig.store().turn_ops(out.turn_id).unwrap(),
        [
            LedgerOp::AdvanceClock {
                minutes: SCENE_CLOSE_DEFAULT_MIN
            },
            LedgerOp::SetScene {
                open: false,
                why: SceneWhy::EndScene
            },
        ]
    );
}

/// The closing schema pins what code does not want the model to choose.
#[test]
fn the_closing_schema_pins_what_code_does_not_want() {
    let ctx = HandleContext {
        entities: vec![Handle::Entity(1), Handle::Entity(2)],
        facts: vec![Handle::Fact(1)],
        events: vec![Handle::WorldEvent(1)],
        goals: vec![Handle::Goal(1)],
        new_slots: 1,
        places: vec![Handle::Entity(3)],
        open_items: vec!["E1.pending_meet".into()],
        leavers: vec![Handle::Entity(2)],
        destinations: vec![Handle::Entity(3)],
        scene: true,
    };
    let schema = closing_schema_for(&ctx);
    let props = &schema["properties"];
    assert_eq!(props["action"]["enum"], serde_json::json!(["wait"]));
    assert_eq!(props["verdict"]["enum"], serde_json::json!(["allowed"]));
    assert_eq!(props["contested"]["enum"], serde_json::json!([false]));
    assert_eq!(props["intimacy"]["enum"], serde_json::json!(["none"]));
    assert_eq!(props["move_to"], serde_json::json!({"type": "null"}));
    assert_eq!(props["check"], serde_json::json!({"type": "null"}));
    for key in [
        "settles", "new_npcs", "schedule", "cancel", "leaves", "blockers",
    ] {
        assert!(props.get(key).is_none(), "{key} is absent");
        assert!(
            !schema["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(key)),
            "{key} is not required"
        );
    }
    assert_eq!(props["reactions"]["maxItems"], 2);
    assert_eq!(props["facts"]["maxItems"], 1);
    assert_eq!(props["minutes"]["minimum"], 1);
    assert_eq!(props["minutes"]["maximum"], SCENE_CLOSE_MAX_MIN);
    for key in ["intimacy", "contested", "move_to", "minutes"] {
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(key)),
            "{key} is required"
        );
    }
    // The handles stay those of the prompt.
    assert_eq!(
        schema["$defs"]["Reaction"]["properties"]["npc"]["enum"],
        serde_json::json!(["E1", "E2", "N1"])
    );
}

/// `settle_closing` forces the same pins on the parsed proposal, because the
/// validator must not trust the grammar: one note per change.
#[test]
fn settle_forces_the_closing_pins() {
    let rig = Rig::new("settle-pins");
    let mut p = rig.closing();
    p.action = ActionKind::Act;
    p.intimacy = Intimacy::Intimate;
    p.new_npcs = vec![new_npc(1, "Kett", 30)];
    p.minutes = 999;
    let (p, notes) = settle_closing(p, &rig.view());
    assert_eq!(p.action, ActionKind::Wait);
    assert_eq!(p.intimacy, Intimacy::None);
    assert!(p.new_npcs.is_empty());
    assert_eq!(p.minutes, SCENE_CLOSE_MAX_MIN);
    assert_eq!(SCENE_CLOSE_MAX_MIN, 240);
    assert_eq!(notes.len(), 4, "{notes:?}");

    // A closing with no minutes takes one.
    let mut p = rig.closing();
    p.minutes = 0;
    assert_eq!(settle_closing(p, &rig.view()).0.minutes, 1);

    // Every other pin.
    let mut p = rig.closing();
    p.verdict = Verdict::Check;
    p.contested = true;
    p.check = Some(check(
        None,
        slot(e(rig.brann), "mood", "a"),
        slot(e(rig.brann), "mood", "b"),
    ));
    p.reactions
        .push(reaction(e(rig.vale), Stance::Neutral, None));
    p.reactions
        .push(reaction(e(rig.vale), Stance::Neutral, None));
    p.facts = vec![
        fact(slot(e(rig.brann), "mood", "calm"), Seen::Present, vec![]),
        fact(slot(e(rig.brann), "hair", "loose"), Seen::Present, vec![]),
    ];
    let (p, notes) = settle_closing(p, &rig.view());
    assert_eq!(p.verdict, Verdict::Allowed);
    assert!(!p.contested);
    assert!(p.check.is_none());
    assert_eq!(p.reactions.len(), 2);
    assert_eq!(p.facts.len(), 1);
    assert_eq!(notes.len(), 5, "{notes:?}");
}

/// Review minor (2026-11-12): a Closing Turn moves nobody. `settle_closing`
/// drops every `location` fact, on any subject and in any case of the key,
/// with one note each; the closing schema says so; and "End scene" with
/// such a proposal leaves Brann where he stands.
#[test]
fn a_closing_turn_moves_nobody() {
    let mut rig = Rig::new("closing-moves");
    let mut p = rig.closing();
    p.facts = vec![fact(
        slot(e(rig.brann), "Location", "far road"),
        Seen::Present,
        vec![],
    )];
    let (settled, notes) = settle_closing(p.clone(), &rig.view());
    assert!(settled.facts.is_empty(), "{:?}", settled.facts);
    assert_eq!(
        notes.iter().filter(|n| n.contains("location")).count(),
        1,
        "{notes:?}"
    );
    let schema = closing_schema_for(&HandleContext {
        entities: vec![Handle::Entity(rig.brann)],
        new_slots: 1,
        ..HandleContext::default()
    });
    let facts = schema["properties"]["facts"]["description"]
        .as_str()
        .unwrap_or_default();
    assert!(facts.contains("location"), "{schema}");

    let intimate = rig.intimate();
    rig.played("t1", &intimate, "Brann nods.");
    rig.end_scene("close-1", &p, "The scene ends.").unwrap();
    let view = rig.view();
    let place = view.live_fact(rig.brann, "location").unwrap();
    assert_eq!(place.slot.value, "gate square");
}

/// A Closing Turn sets no voluntary act of the PC: a fact on the PC's
/// `mood` is an agency rejection, `fatigue` passes (data shape 3.7 step 4).
#[test]
fn a_closing_turn_sets_no_pc_choice() {
    let mut rig = Rig::new("closing-agency");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let view = rig.view();
    let with_fact = |key: &str| {
        let mut p = rig.closing();
        p.facts = vec![fact(slot(e(rig.ada), key, "x"), Seen::Pc, vec![])];
        settle_closing(p, &view).0
    };
    let mood = with_fact("mood");
    let errs = validate(&mood, &view).err().unwrap_or_default();
    assert!(
        errs.iter().any(|r| r.check == CheckKind::Agency),
        "{errs:?}"
    );
    let fatigue = with_fact("fatigue");
    assert!(validate(&fatigue, &view).is_ok());

    let mut p = rig.closing();
    p.facts = vec![fact(slot(e(rig.ada), "fatigue", "tired"), Seen::Pc, vec![])];
    let out = rig.end_scene("close-1", &p, "You rest.").unwrap();
    assert!(
        rig.store()
            .turn_ops(out.turn_id)
            .unwrap()
            .iter()
            .any(|op| matches!(op, LedgerOp::AddFact { slot, .. } if slot.key == "fatigue"))
    );
}

/// A Closing Turn has no Intent: "Correct last Turn" does not offer it.
#[test]
fn a_closing_turn_cannot_be_corrected() {
    let mut rig = Rig::new("closing-correct");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let p = rig.closing();
    rig.end_scene("close-1", &p, "Brann steps back.").unwrap();
    assert!(!rig.sight().can_correct);
    let plain = rig.plain();
    rig.gm.push_proposal(serde_json::to_string(&plain).unwrap());
    let err = rig
        .engine()
        .correct_last("c1".into(), "I wait.", "slower", &mut |_| {})
        .unwrap_err();
    assert!(matches!(err, TurnError::NotCorrectable(_)), "{err:?}");
}

/// A Closing Turn is a Moment of the story with no Intent, and no Act in
/// the Journal.
#[test]
fn a_closing_turn_is_no_act_in_the_journal() {
    let mut rig = Rig::new("closing-journal");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    let p = rig.closing();
    let close = rig
        .end_scene("close-1", &p, "Brann steps back.")
        .unwrap()
        .turn_id;
    let sight = rig.sight();
    assert!(sight.moments.iter().any(|m| m.turn == close));
    let story = sight.story.iter().find(|t| t.turn == close).unwrap();
    assert_eq!(story.kind, TurnKind::Closing);
    // Later prompts and the OOC sheet read its Neutral Line.
    assert!(
        story
            .line
            .as_deref()
            .is_some_and(|l| l.contains("10 minutes")),
        "{:?}",
        story.line
    );
    let marks = Default::default();
    let journal = journal_view(&sight, &marks);
    let acts: Vec<TurnId> = journal
        .scenes
        .iter()
        .flat_map(|s| s.acts.iter().map(|a| a.turn_id))
        .collect();
    assert!(acts.contains(&t1));
    assert!(!acts.contains(&close), "{acts:?}");
    let view = play_view(&sight, &marks);
    assert_eq!(view.scene, SceneState::Plain);
    let story = view.story.iter().find(|t| t.turn_id == close).unwrap();
    assert_eq!(story.intent, "");
}

// --- A failed gate leaves no scene on a Player screen ---------------------------

/// G1 fails (another program brought an unconfirmed NPC into the place),
/// then G2 refuses the Turn, so nothing commits and the ledger still holds
/// the open scene. No reader outside the ledger treats it as open: the Play
/// screen shows a plain scene, the plan limit is the plain one, and the
/// next prompt holds no scene text. The next Turn that commits closes it.
#[test]
fn a_refused_turn_after_a_failed_gate_shows_no_scene() {
    let mut rig = Rig::new("gate-refused");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    rig.add_npc("Kett", 34, false, "gate square");
    let p = rig.intimate();
    let err = rig.play("t2", &p, "Brann stays.").unwrap_err();
    assert!(matches!(err, TurnError::Refused { .. }), "{err:?}");
    assert_eq!(rig.scene(), Some(t1), "the ledger is unchanged");
    assert_eq!(rig.sight().scene, None);
    assert_eq!(
        play_view(&rig.sight(), &Default::default()).scene,
        SceneState::Plain
    );
    assert_eq!(threshold_core::mature::open_scene(&rig.view()), None);

    let mut long = rig.plain();
    long.reactions = vec![reaction(e(rig.brann), Stance::Comply, None)];
    long.reactions[0].plan = "x".repeat(91);
    let errs = validate(&long, &rig.view()).err().unwrap_or_default();
    assert!(
        errs.iter().any(|r| r.check == CheckKind::References),
        "the plain plan limit: {errs:?}"
    );

    let p = rig.plain();
    let t3 = rig.played("t3", &p, "Dusk.");
    let call = rig.last_call(CallKind::Propose);
    assert!(!prompt_text(&call).contains(SCENE_LINE));
    assert!(!prompt_text(&call).contains(MARKER));
    assert_eq!(rig.scene_ops(t3), [(false, SceneWhy::Gate)]);
    assert_eq!(rig.scene(), None);
}

// --- A Narration cut by the token cap (review B2) ------------------------------

/// A model whose every narration stopped at the token cap
/// (`done_reason: length`).
struct LengthCut(threshold_core::RecordedGm);

impl Gm for LengthCut {
    fn propose(&self, prompt: &Prompt, schema: &serde_json::Value) -> Result<String, GmError> {
        self.0.propose(prompt, schema)
    }

    fn narrate(
        &self,
        prompt: &Prompt,
        max_tokens: Option<u32>,
        on_chunk: &mut dyn FnMut(&str) -> Stream,
    ) -> Result<String, GmError> {
        self.0.narrate(prompt, max_tokens, on_chunk)
    }

    fn last_done_reason(&self) -> Option<String> {
        Some("length".into())
    }
}

/// What the Play screen shows of one Turn after it ends: each prose line,
/// and each speech line as its speaker's name (as the PC knows it) and its
/// words.
fn shown_after(rig: &Rig, turn: TurnId) -> String {
    use threshold_core::world::SightLine;
    let sight = rig.sight();
    let story = sight
        .story
        .iter()
        .find(|t| t.turn == turn)
        .expect("the Turn");
    story
        .lines
        .iter()
        .map(|line| match line {
            SightLine::Prose(text) => format!("{text}\n"),
            SightLine::Speech { speaker, words, .. } => {
                let name = sight
                    .known
                    .iter()
                    .find(|k| k.id == *speaker)
                    .and_then(|k| k.name.clone())
                    .unwrap_or_default();
                format!("{name}: {words}\n")
            }
        })
        .collect()
}

/// Review B2: a Narration that the token cap cuts inside a speech mark must
/// never show a name that the uncut Narration would not show. Vale is an
/// NPC the Adventure Seed hides, Kett an NPC the PC cannot name, Brann an
/// NPC here whom the PC can name. For every cut point, the screen after the
/// Turn and the stream show neither Vale nor Kett, as the uncut text does.
#[test]
fn a_cut_narration_never_shows_a_name_the_full_text_hides() {
    let full = "Dusk falls over the square.\n\
                Brann: \"Stay a while.\"\n\
                Vale: \"Go now. Go.\"\n\
                Kett: (quietly) \"Not yet.\"\n\
                Vale: \"Go.\" She turns.\n\
                Kett: \"Wait.\" He frowns at the gate.\n\
                The lamps go out.";
    let hidden = ["Vale", "Kett"];

    // The uncut Narration: the reference.
    let mut rig = Rig::new("cut-full");
    let kett = rig.add_npc("Kett", 34, false, "gate square");
    let p = rig.plain();
    let t = rig.played("t0", &p, full);
    let reference = shown_after(&rig, t);
    assert!(reference.contains("Brann: Stay a while."), "{reference}");
    for name in hidden {
        assert!(!reference.contains(name), "the uncut text shows {name}");
    }
    // A line with text after its closing quote keeps that text, without the
    // tag (ADR 0006, 2026-11-12).
    assert!(reference.contains("\"Go.\" She turns."), "{reference}");
    assert!(reference.contains("He frowns at the gate."), "{reference}");
    // No line made the PC able to name Kett.
    assert!(
        rig.sight()
            .known
            .iter()
            .all(|k| k.id != kett || k.name.is_none())
    );

    // Every cut point, one Turn each, with a model that says it was cut.
    let mut rig = Rig::new("cut-points");
    rig.add_npc("Kett", 34, false, "gate square");
    let gm = rig.gm.clone();
    rig.with_gm(Box::new(LengthCut(gm)));
    let cuts: Vec<usize> = full
        .char_indices()
        .map(|(at, _)| at)
        .chain([full.len()])
        .filter(|at| *at > 0)
        .collect();
    for (n, at) in cuts.into_iter().enumerate() {
        let told = &full[..at];
        let p = rig.plain();
        rig.gm.push_proposal(serde_json::to_string(&p).unwrap());
        rig.gm.push_narration(told.into());
        let mut streamed = String::new();
        let out = rig
            .engine()
            .play_turn(format!("cut-{n}"), "I wait.", &mut |c| streamed.push_str(c))
            .unwrap();
        let shown = shown_after(&rig, out.turn_id);
        for name in hidden {
            assert!(
                !shown.contains(name),
                "cut at {at} ({told:?}): the screen shows {name}: {shown:?}"
            );
            assert!(
                !streamed.contains(name),
                "cut at {at}: the stream showed {name}: {streamed:?}"
            );
        }
        // The stored text is a part of the text the stream showed, line by
        // line: the Player never reads more after the Turn than during it.
        let stored = rig.turn(out.turn_id).narration.unwrap_or_default();
        assert!(full.starts_with(&stored), "cut at {at}: {stored:?}");
    }
}

// --- Task 8: the Detail Level in play ------------------------------------------

/// Data shape 3.4, row 8: `set_detail_level(excluded)` closes an open scene
/// in the same OOC Turn, and the next intimate Turn is refused for the
/// level. Lowering to `implied` closes nothing.
#[test]
fn excluded_closes_the_open_scene() {
    let mut rig = Rig::with("excluded-closes", DetailLevel::Explicit, Vec::new());
    let p = rig.intimate();
    let t1 = rig.played("s1", &p, "Brann nods.");
    assert_eq!(rig.scene(), Some(t1));

    rig.engine()
        .set_detail_level("lvl-1".into(), DetailLevel::Implied)
        .unwrap();
    assert_eq!(rig.scene(), Some(t1), "implied keeps the scene");

    rig.engine()
        .set_detail_level("lvl-2".into(), DetailLevel::Excluded)
        .unwrap();
    assert_eq!(rig.scene(), None);
    let ooc = rig.store().turns(rig.campaign).unwrap().pop().unwrap();
    assert_eq!(ooc.record.kind, TurnKind::Ooc);
    assert_eq!(
        rig.store().turn_ops(ooc.id).unwrap(),
        [
            LedgerOp::SetDetailLevel {
                level: DetailLevel::Excluded
            },
            LedgerOp::SetScene {
                open: false,
                why: SceneWhy::Excluded
            },
        ]
    );
    assert!(matches!(rig.sight().scene, None));
    let err = rig.play("s2", &p, "-").expect_err("excluded");
    assert!(
        matches!(
            err,
            TurnError::Refused {
                kind: threshold_core::mature::RefusalKind::Excluded,
                ..
            }
        ),
        "{err:?}"
    );
}

// --- Live evaluation of 2026-11-12: the advance -------------------------------

/// An advance is told as a plain Turn: no implied line, no scene cap, and
/// it opens no Intimate Scene. Its record keeps the tag.
#[test]
fn an_advance_is_told_plain_and_opens_no_scene() {
    let mut rig = Rig::new("advance-plain");
    let mut p = rig.intimate();
    p.intimacy = Intimacy::Advance;
    let t1 = rig.played("t1", &p, "Brann smiles and shakes his head.");
    assert_eq!(rig.scene(), None);
    assert!(rig.scene_ops(t1).is_empty());
    let narrate = rig.last_call(CallKind::Narrate);
    assert!(!narrate.prompt.recent.contains(IMPLIED_LINE));
    assert_eq!(narrate.max_tokens, None);
    let record: serde_json::Value =
        serde_json::from_str(&rig.turn(t1).record.proposal_json).unwrap();
    assert_eq!(record["proposal"]["intimacy"], "advance");
    // A plain Turn has no Neutral Line: the next prompts read it in full.
    assert_eq!(neutral_line(&rig.turn(t1)), None);
}

/// An advance inside an open scene is a Turn that is not `intimate`: it
/// keeps the scene open, as a short plain Turn does (polish pass), and is
/// told as a plain Turn.
#[test]
fn an_advance_in_an_open_scene_keeps_it_open() {
    let mut rig = Rig::new("advance-keeps");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    let mut p = rig.intimate();
    p.intimacy = Intimacy::Advance;
    let t2 = rig.played("t2", &p, "Dusk falls.");
    assert_eq!(rig.scene(), Some(t1));
    assert!(rig.scene_ops(t2).is_empty());
    let narrate = rig.last_call(CallKind::Narrate);
    assert!(!narrate.prompt.recent.contains(IMPLIED_LINE));
}

/// The sticky tag keeps the strongest value of the raw answers: a first
/// answer that says `advance`, then a retry that says `none`, commits as
/// an advance, and a minor here refuses it.
#[test]
fn the_sticky_tag_keeps_the_strongest_value() {
    let mut rig = Rig::new("sticky-strongest");
    let mut first = rig.intimate();
    first.intimacy = Intimacy::Advance;
    // A plan over the limit makes the first answer fail another check.
    first.reactions[0].plan = "x".repeat(120);
    let retry = rig.plain();
    let play = |rig: &mut Rig, op: &str| {
        rig.gm.push_proposal(serde_json::to_string(&first).unwrap());
        rig.gm.push_proposal(serde_json::to_string(&retry).unwrap());
        rig.gm.push_narration("Brann shrugs.".into());
        rig.engine()
            .play_turn(op.into(), "I stay with Brann.", &mut |_| {})
    };
    let outcome = play(&mut rig, "t1").unwrap();
    let record: serde_json::Value =
        serde_json::from_str(&rig.turn(outcome.turn_id).record.proposal_json).unwrap();
    assert_eq!(record["proposal"]["intimacy"], "advance");

    rig.add_npc("Kit", 15, true, "gate square");
    let refused = play(&mut rig, "t2").expect_err("a minor is here");
    assert!(
        matches!(
            refused,
            TurnError::Refused {
                kind: threshold_core::mature::RefusalKind::AdultsOnly,
                ..
            }
        ),
        "{refused:?}"
    );
}

// --- Final review (2026-11-12): minors 3, 4, 5 and the late fade ------------

/// Minor 3: at `excluded`, `renarrate` of an intimate Turn committed
/// earlier, and a retry of an op whose Narration is missing, make no
/// narration call: each gives the Turn's Neutral Line, and the row keeps
/// its text.
#[test]
fn at_excluded_an_earlier_intimate_turn_is_its_neutral_line_with_no_call() {
    let mut rig = Rig::new("excluded-renarrate");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    // t2: committed, its Narration fails (no narration is queued).
    rig.gm.push_proposal(serde_json::to_string(&p).unwrap());
    let failed = rig
        .engine()
        .play_turn("t2".into(), "I stay with Brann.", &mut |_| {});
    let Err(TurnError::NarrationFailed { turn_id: t2, .. }) = failed else {
        panic!("{failed:?}");
    };
    rig.engine()
        .set_detail_level("off".into(), DetailLevel::Excluded)
        .unwrap();
    let calls = rig.calls(CallKind::Narrate).len();
    let again = rig.engine().renarrate(t1, &mut |_| {}).unwrap();
    assert_eq!(again, neutral_line(&rig.turn(t1)).unwrap());
    assert_eq!(rig.turn(t1).narration.as_deref(), Some("Brann nods."));
    let retry = rig
        .engine()
        .play_turn("t2".into(), "I stay with Brann.", &mut |_| {})
        .unwrap();
    assert_eq!(retry.narration, neutral_line(&rig.turn(t2)).unwrap());
    assert_eq!(
        rig.calls(CallKind::Narrate).len(),
        calls,
        "no narration call"
    );
}

/// Minor 4: `renarrate` refuses a Turn of a kind that tells nothing (an
/// OOC Turn, the Setup Turn), with no model call, and `set_narration`
/// refuses those kinds too.
#[test]
fn renarrate_and_set_narration_refuse_a_turn_that_tells_nothing() {
    let mut rig = Rig::new("tells-nothing");
    rig.engine()
        .set_detail_level("level".into(), DetailLevel::Explicit)
        .unwrap();
    let ooc = rig
        .store()
        .turn_by_campaign_op(rig.campaign, &"level".to_owned())
        .unwrap()
        .unwrap();
    assert_eq!(ooc.record.kind, TurnKind::Ooc);
    let setup = rig
        .store()
        .turns(rig.campaign)
        .unwrap()
        .into_iter()
        .find(|t| t.record.kind == TurnKind::Setup)
        .unwrap();
    for turn in [ooc.id, setup.id] {
        assert!(rig.engine().renarrate(turn, &mut |_| {}).is_err());
    }
    assert!(rig.calls(CallKind::Narrate).is_empty());
    drop(rig.engine.take());
    let mut store = Store::open(&rig.path).unwrap();
    for turn in [ooc.id, setup.id] {
        assert!(store.set_narration(turn, "Text.").is_err());
        assert_eq!(store.turn(turn).unwrap().unwrap().narration, None);
    }
}

/// Minor 5: an op id of one kind of Turn is never taken by a command of
/// another kind. `play_turn` with the op id of an OOC Turn makes no model
/// call and narrates nothing; `confirm_age` and `set_detail_level` with the
/// op id of a Play Turn give a plain error and change nothing.
#[test]
fn an_op_id_of_another_kind_is_refused() {
    let mut rig = Rig::new("op-kinds");
    rig.engine()
        .set_detail_level("ooc-op".into(), DetailLevel::Explicit)
        .unwrap();
    let err = rig
        .engine()
        .play_turn("ooc-op".into(), "I wait.", &mut |_| {})
        .unwrap_err();
    assert!(matches!(err, TurnError::OpIdTaken), "{err:?}");
    assert!(rig.gm.calls().is_empty(), "no model call");
    let ooc = rig
        .store()
        .turn_by_campaign_op(rig.campaign, &"ooc-op".to_owned())
        .unwrap()
        .unwrap();
    assert_eq!(ooc.narration, None);

    let p = rig.plain();
    rig.played("play-op", &p, "Dusk falls.");
    let before = rig.view();
    let brann = rig.brann;
    for err in [
        rig.engine()
            .confirm_age("play-op".into(), brann)
            .unwrap_err(),
        rig.engine()
            .set_detail_level("play-op".into(), DetailLevel::Excluded)
            .unwrap_err(),
    ] {
        assert!(matches!(err, TurnError::OpIdTaken), "{err:?}");
        assert!(!err.to_string().is_empty());
    }
    assert_eq!(rig.view().detail, before.detail);
    assert_eq!(rig.view().turn_seq, before.turn_seq);
}

/// ADR 0011 (final review minor 8): a late fade fades the latest Turn that
/// tells, even when an OOC Turn (a level, a confirmation) came after it.
#[test]
fn a_late_fade_skips_an_ooc_turn_after_the_streamed_turn() {
    let mut rig = Rig::new("late-fade-ooc");
    let p = rig.plain();
    let t1 = rig.played("t1", &p, "Dusk falls.");
    rig.engine()
        .set_detail_level("level".into(), DetailLevel::Explicit)
        .unwrap();
    let outcome = rig.engine().fade("t1".into()).unwrap();
    assert!(outcome.faded);
    assert!(rig.turn(t1).faded);
    assert_eq!(rig.turn(t1).narration, None);
}

// --- Polish pass, round 2 (2026-11-12): the bound on plain Turns -------------

/// A short plain proposal of the rig, as `n` plain Turns played in a row.
fn plain_turns(rig: &mut Rig, ops: &[&str]) -> Vec<TurnId> {
    let plain = rig.plain();
    ops.iter()
        .map(|op| rig.played(op, &plain, "Brann talks."))
        .collect()
}

/// An open scene survives at most `SCENE_PLAIN_TURNS_MAX` Turns that are
/// not `intimate` in a row: the next one closes it with `NotIntimate`. An
/// `intimate` Turn starts the count again (review Major 2).
#[test]
fn a_scene_survives_a_bounded_run_of_plain_turns() {
    assert_eq!(SCENE_PLAIN_TURNS_MAX, 2, "the tests below count to it");
    let mut rig = Rig::new("plain-bound");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    assert_eq!(plain_run(&rig.view()), 0);
    let kept = plain_turns(&mut rig, &["t2", "t3"]);
    for t in &kept {
        assert!(rig.scene_ops(*t).is_empty(), "turn {t}");
    }
    assert_eq!(rig.scene(), Some(t1));
    assert_eq!(plain_run(&rig.view()), 2);
    let t4 = plain_turns(&mut rig, &["t4"])[0];
    assert_eq!(rig.scene_ops(t4), [(false, SceneWhy::NotIntimate)]);
    assert_eq!(rig.scene(), None);
    assert_eq!(plain_run(&rig.view()), 0, "no scene, no run");

    // An intimate Turn in between starts the count again.
    let mut rig = Rig::new("plain-bound-reset");
    let t1 = rig.played("t1", &p, "Brann nods.");
    plain_turns(&mut rig, &["t2", "t3"]);
    rig.played("t4", &p, "Brann stays.");
    assert_eq!(plain_run(&rig.view()), 0);
    plain_turns(&mut rig, &["t5", "t6"]);
    assert_eq!(rig.scene(), Some(t1), "the same scene");
    let t7 = plain_turns(&mut rig, &["t7"])[0];
    assert_eq!(rig.scene_ops(t7), [(false, SceneWhy::NotIntimate)]);
}

/// The run counts standing Turns only. An OOC Turn tells nothing: it
/// neither counts nor ends the run. A Turn a Correction reverted is not
/// standing: only its Correction counts.
#[test]
fn the_plain_run_skips_ooc_and_reverted_turns() {
    let mut rig = Rig::new("plain-ooc");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    plain_turns(&mut rig, &["t2"]);
    rig.engine()
        .set_detail_level("lvl".into(), DetailLevel::Explicit)
        .unwrap();
    assert_eq!(plain_run(&rig.view()), 1, "the OOC Turn does not count");
    let t3 = plain_turns(&mut rig, &["t3"])[0];
    assert!(rig.scene_ops(t3).is_empty());
    assert_eq!(rig.scene(), Some(t1), "the OOC Turn did not end the run");
    let t4 = plain_turns(&mut rig, &["t4"])[0];
    assert_eq!(rig.scene_ops(t4), [(false, SceneWhy::NotIntimate)]);

    let mut rig = Rig::new("plain-reverted");
    let t1 = rig.played("t1", &p, "Brann nods.");
    plain_turns(&mut rig, &["t2"]);
    let plain = rig.plain();
    correct(&mut rig, "c2", &plain);
    assert_eq!(plain_run(&rig.view()), 1, "t2 is reverted, c2 stands");
    let t3 = plain_turns(&mut rig, &["t3"])[0];
    assert!(
        rig.scene_ops(t3).is_empty(),
        "the reverted Turn is not counted"
    );
    assert_eq!(rig.scene(), Some(t1));
}

/// A Closing Turn closes its scene, so it never stands inside an open one.
/// The run stops at any Turn with no Intent (a Closing or an Opening
/// Turn), as it stops at the Turn that opened the scene.
#[test]
fn the_plain_run_stops_at_a_turn_with_no_intent() {
    let turn = |id: TurnId, kind: TurnKind, tag: &str| StoredTurn {
        id,
        seq: id,
        record: threshold_core::store::TurnRecord {
            campaign_id: 1,
            op_id: format!("op-{id}"),
            kind,
            intent: String::new(),
            proposal_json: format!(r#"{{"proposal":{{"intimacy":"{tag}"}}}}"#),
            clock_before: 0,
            clock_after: 5,
        },
        narration: None,
        faded: false,
        told: None,
        scene_closed: None,
    };
    let mut view = WorldView {
        scene_open: Some(1),
        ..Default::default()
    };
    view.recent_turns = vec![
        turn(1, TurnKind::Play, "intimate"),
        turn(2, TurnKind::Play, "none"),
        turn(3, TurnKind::Correction, "advance"),
    ];
    assert_eq!(plain_run(&view), 2);
    view.recent_turns
        .insert(2, turn(3, TurnKind::Closing, "none"));
    view.recent_turns[3].id = 4;
    assert_eq!(plain_run(&view), 1, "stops at the Closing Turn");
    view.scene_open = Some(4);
    assert_eq!(
        plain_run(&view),
        0,
        "stops at the Turn that opened the scene"
    );
    view.scene_open = None;
    assert_eq!(plain_run(&view), 0);
}

/// A plain Turn inside an open scene keeps no `finds`: its proposal call
/// read the scene in full, and a found item is free text that later
/// prompts, the Journal and the OOC sheet read with no clearance. The Turn
/// commits; the item is in no later prompt and on no screen.
#[test]
fn a_plain_turn_inside_a_scene_keeps_no_finds() {
    use threshold_core::{
        domain::{Finds, FoundItem, SourceKind},
        turn::stored_found,
        view::ooc_sheet,
    };
    const ITEM: &str = "MARKER-FOUND-1";
    let mut rig = Rig::new("plain-finds");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, "Brann nods.");
    let mut looking = rig.plain();
    looking.action = ActionKind::Observe;
    looking.finds = Finds {
        from: SourceKind::NoticeOrSign,
        source: "the notice board".into(),
        items: vec![FoundItem {
            title: ITEM.into(),
            line: "A neutral line.".into(),
        }],
    };
    let calls_before = rig.gm.calls().len();
    let t2 = rig.played("t2", &looking, "You read the board.");
    assert_eq!(rig.scene(), Some(t1));
    assert_eq!(stored_found(&rig.turn(t2).record.proposal_json), None);
    let settle = rig
        .store()
        .debug_log("t2")
        .unwrap()
        .into_iter()
        .filter(|row| row.step == "settle")
        .map(|row| row.raw)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(settle.contains("removed finds"), "{settle}");
    assert!(settle.contains("Intimate Scene"), "{settle}");
    // Later prompts, in and after the scene.
    rig.played("t3", &p, "Brann stays.");
    long_plain(&mut rig, "t4");
    plain_turns(&mut rig, &["t5"]);
    let calls = rig.gm.calls();
    // Every call from the proposal call of t2 on: the item was only in that
    // call's answer.
    for call in &calls[calls_before..] {
        assert!(!prompt_text(call).contains(ITEM), "{:?}", call.kind);
    }
    let sight = rig.sight();
    assert!(sight.story.iter().all(|t| t.found.is_none()));
    let journal = journal_view(&sight, &Default::default());
    assert!(journal.scenes.iter().all(|s| s.found.is_empty()));
    assert!(!ooc_sheet(&sight).contains(ITEM));
}

// --- Review round 2: paths a plain Turn inside the scene made reachable ---------

/// The explicit test pack, at `explicit`: the strongest telling a scene has.
fn explicit_rig(name: &str) -> Rig {
    let (packs, _) = load_all(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/packs"));
    let pack = packs
        .into_iter()
        .find(|p| p.explicit())
        .expect("the test pack gives explicit description");
    Rig::with(name, DetailLevel::Explicit, vec![pack])
}

/// No scene text in a narration prompt: no implied or explicit line, no
/// pack scene fragment, no scene Narration.
fn assert_no_scene_text(call: &threshold_core::RecordedCall) {
    let text = prompt_text(call);
    for line in [EXPLICIT_LINE, IMPLIED_LINE, PACK_SCENE_HEAD, MARKER] {
        assert!(!text.contains(line), "{line}: {text}");
    }
}

/// `renarrate` of a plain Turn inside an open scene: told plain, with no
/// scene text, and the scene stays open.
#[test]
fn renarrate_of_a_plain_turn_inside_a_scene_reads_no_scene_text() {
    let mut rig = explicit_rig("plain-renarrate");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    let t2 = plain_turns(&mut rig, &["t2"])[0];
    rig.gm.push_narration("Brann talks again.".into());
    rig.engine().renarrate(t2, &mut |_| {}).unwrap();
    let call = rig.last_call(CallKind::Narrate);
    assert_no_scene_text(&call);
    assert_eq!(call.max_tokens, None);
    assert_eq!(rig.scene(), Some(t1));
    assert_eq!(rig.turn(t2).told, None, "told plain");
}

/// A Correction of a plain Turn inside an open scene: its proposal call
/// reads the scene (G1 on the state before the corrected Turn), its
/// narration call reads no scene text, and the scene stays open.
#[test]
fn a_correction_of_a_plain_turn_inside_a_scene_is_told_plain() {
    let mut rig = explicit_rig("plain-correction");
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    plain_turns(&mut rig, &["t2"]);
    let plain = rig.plain();
    rig.gm.push_proposal(serde_json::to_string(&plain).unwrap());
    rig.gm.push_narration("Brann talks.".into());
    let c = rig
        .engine()
        .correct_last("c2".into(), "I talk with Brann.", "again", &mut |_| {})
        .unwrap();
    let propose = rig.last_call(CallKind::Propose);
    assert!(propose.prompt.recent.starts_with(SCENE_LINE));
    assert_no_scene_text(&rig.last_call(CallKind::Narrate));
    assert!(rig.scene_ops(c.turn_id).is_empty());
    assert_eq!(rig.scene(), Some(t1));
}

/// A new NPC made in a plain Turn inside an open scene is a character the
/// PC now sees here: the Turn closes the scene with `Arrival`.
#[test]
fn a_new_npc_in_a_plain_turn_closes_the_scene() {
    let mut rig = Rig::new("plain-new-npc");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let mut plain = rig.plain();
    plain.new_npcs = vec![new_npc(1, "Kett", 34)];
    let t2 = rig.played("t2", &plain, "Someone comes.");
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Arrival)]);
    assert_eq!(rig.scene(), None);
}

/// An unconfirmed minor comes to the place unseen during a plain Turn of an
/// open scene: a proposal fact with `seen: gm` (data shape 3.4 said G1
/// cannot fail on a file this code wrote; it can now). The ledger keeps the
/// scene open, but no screen shows it; a `renarrate` of the scene Turn is
/// told `Implied` with no pack fragment; the next intimate Turn is refused
/// `AdultsOnly` with no scene text in its prompt; the next Commit closes
/// the scene with `Gate`.
#[test]
fn a_minor_who_arrives_unseen_in_a_plain_turn_ends_the_scene_by_g1() {
    let mut rig = explicit_rig("plain-minor");
    let pip = rig.view().entity_by_name("Pip").unwrap().id;
    let p = rig.intimate();
    let t1 = rig.played("t1", &p, &format!("{MARKER} Brann nods."));
    assert_eq!(rig.turn(t1).told, Some(DetailLevel::Explicit));
    let mut plain = rig.plain();
    plain.facts = vec![fact(
        slot(e(pip), "location", "gate square"),
        Seen::Gm,
        vec![],
    )];
    let t2 = rig.played("t2", &plain, "Dusk falls.");
    let view = rig.view();
    assert!(view.present().contains(&pip), "Pip is here");
    assert!(!threshold_core::context::seen_present(&view).contains(&pip));
    assert!(rig.scene_ops(t2).is_empty(), "an unseen arrival");
    assert_eq!(rig.scene(), Some(t1), "the ledger keeps the scene");

    // No screen shows it.
    assert_eq!(rig.sight().scene, None);
    assert_eq!(
        play_view(&rig.sight(), &Default::default()).scene,
        SceneState::Plain
    );

    // A `renarrate` of the scene Turn: implied, no fragment.
    rig.gm.push_narration("Brann nods again.".into());
    rig.engine().renarrate(t1, &mut |_| {}).unwrap();
    let call = rig.last_call(CallKind::Narrate);
    assert!(call.prompt.recent.starts_with(IMPLIED_LINE));
    let text = prompt_text(&call);
    assert!(!text.contains(EXPLICIT_LINE), "{text}");
    assert!(!text.contains(PACK_SCENE_HEAD), "{text}");
    assert_eq!(rig.turn(t1).told, Some(DetailLevel::Implied));

    // The next intimate Turn: refused, and its prompt reads no scene text.
    let err = rig.play("t3", &p, "-").expect_err("adults only");
    assert!(
        matches!(
            err,
            TurnError::Refused {
                kind: threshold_core::mature::RefusalKind::AdultsOnly,
                ..
            }
        ),
        "{err:?}"
    );
    let propose = rig.last_call(CallKind::Propose);
    let text = prompt_text(&propose);
    assert!(!text.contains(SCENE_LINE), "{text}");
    assert!(!text.contains(MARKER), "{text}");
    assert_eq!(rig.scene(), Some(t1), "nothing committed");

    // The next Commit closes the scene by G1.
    let t4 = plain_turns(&mut rig, &["t4"])[0];
    assert_eq!(rig.scene_ops(t4), [(false, SceneWhy::Gate)]);
    assert_eq!(rig.scene(), None);
}

// --- QA M3 (2026-11-15): every scene end shows one line ----------------------

/// The scene end line of each Turn of the Play story, by Turn id.
fn end_lines(rig: &Rig) -> Vec<(TurnId, Option<String>)> {
    play_view(&rig.sight(), &Default::default())
        .story
        .into_iter()
        .map(|t| (t.turn_id, t.scene_end))
        .collect()
}

/// The one Turn of the story that shows a scene end line, and the line.
fn the_end_line(rig: &Rig) -> (TurnId, String) {
    let ends: Vec<(TurnId, String)> = end_lines(rig)
        .into_iter()
        .filter_map(|(t, line)| Some((t, line?)))
        .collect();
    assert_eq!(ends.len(), 1, "{ends:?}");
    ends.into_iter().next().unwrap()
}

/// Each automatic end and "End scene" puts one neutral line under the Turn
/// that closed the scene. The line names nobody.
#[test]
fn every_scene_end_shows_one_line_under_its_turn() {
    // The third plain Turn.
    let mut rig = Rig::new("end-line-plain");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let ts = plain_turns(&mut rig, &["t2", "t3", "t4"]);
    assert_eq!(
        the_end_line(&rig),
        (ts[2], "The scene ends: the moment has passed.".to_owned())
    );

    // "End scene".
    let mut rig = Rig::new("end-line-button");
    rig.played("t1", &p, "Brann nods.");
    let close = rig.closing();
    let end = rig
        .end_scene("end", &close, "The evening goes on.")
        .unwrap();
    assert_eq!(
        the_end_line(&rig),
        (end.turn_id, "The scene ends.".to_owned())
    );

    // A move.
    let mut rig = Rig::new("end-line-move");
    let yard = rig.place("Mill Yard");
    rig.played("t1", &p, "Brann nods.");
    let mv = rig.intimate_move(yard);
    let t2 = rig.played("t2", &mv, "The yard is quiet.");
    assert_eq!(
        the_end_line(&rig),
        (t2, "The scene ends: you left.".to_owned())
    );

    // An arrival the PC sees: the line names nobody.
    let mut rig = Rig::new("end-line-arrival");
    let kett = rig.add_npc("Kett", 34, true, "far road");
    rig.played("t1", &p, "Brann nods.");
    rig.arrival(kett);
    let t2 = rig.played("t2", &p, "Someone comes.");
    let (turn, line) = the_end_line(&rig);
    assert_eq!(
        (turn, line.as_str()),
        (t2, "The scene ends: someone arrived.")
    );
    assert!(!line.contains("Kett"));
}

/// An arrival the PC does not see keeps the scene open: no line.
#[test]
fn a_hidden_arrival_shows_no_end_line() {
    let mut rig = Rig::new("end-line-hidden");
    let vale = rig.vale;
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    rig.arrival(vale);
    rig.played("t2", &p, "Brann stays.");
    assert!(end_lines(&rig).iter().all(|(_, line)| line.is_none()));
}

/// G1: the line says no cause, since the cause may be a character the PC
/// does not see.
#[test]
fn a_gate_end_says_no_cause() {
    let mut rig = Rig::new("end-line-gate");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    rig.add_npc("Kett", 34, false, "gate square");
    let plain = rig.plain();
    let t2 = rig.played("t2", &plain, "Dusk falls.");
    assert_eq!(rig.scene_ops(t2), [(false, SceneWhy::Gate)]);
    assert_eq!(the_end_line(&rig), (t2, "The scene ends.".to_owned()));
}

/// A level change to `excluded` closes the scene in an OOC Turn, which is
/// in no story: the line goes under the last story Turn before it.
#[test]
fn an_excluded_end_shows_under_the_last_story_turn() {
    let mut rig = Rig::with("end-line-excluded", DetailLevel::Explicit, Vec::new());
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let t2 = rig.played("t2", &p, "Brann stays.");
    rig.engine()
        .set_detail_level("lvl".into(), DetailLevel::Excluded)
        .unwrap();
    assert_eq!(
        the_end_line(&rig),
        (
            t2,
            "The scene ends: Intimate Scenes are now left out.".to_owned()
        )
    );
}

/// Review m3 (2026-11-15): `Store::turns` reads the `SetScene` ops of the
/// campaign once instead of once per Turn. Each Turn it gives is the one
/// `Store::turn` reads alone, `scene_closed` included.
#[test]
fn turns_read_at_once_match_each_turn_read_alone() {
    let mut rig = Rig::new("turns-at-once");
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    let close = rig.closing();
    let closed = rig
        .end_scene("close-1", &close, "Brann steps back.")
        .unwrap();
    let q = rig.plain();
    rig.played("t2", &q, "Dusk falls.");
    let turns = rig.store().turns(rig.campaign).unwrap();
    assert!(turns.len() >= 3, "{}", turns.len());
    for turn in &turns {
        assert_eq!(*turn, rig.turn(turn.id));
    }
    let closing = turns.iter().find(|t| t.id == closed.turn_id).unwrap();
    assert_eq!(closing.scene_closed, Some(SceneWhy::EndScene));
    assert_eq!(turns.iter().filter(|t| t.scene_closed.is_some()).count(), 1);
}

// --- QA run 3: "I take Maren to bed." opened no scene ------------------------

/// QA run 3 (Implied): "I take Maren to bed." came back tagged `advance`.
/// No scene opened, but the Narration led Maren to the alcove and she began
/// to undress the PC. Taking someone to bed names the act itself (the
/// intimacy definition says "going to bed together"), so the core's Act
/// Words make it `intimate` whatever the model tags, and the scene opens.
#[test]
fn taking_someone_to_bed_opens_the_scene_whatever_the_model_tags() {
    for intent in ["I take Brann to bed.", "I lead him to bed."] {
        let mut rig = Rig::new("take-to-bed");
        let mut p = rig.intimate();
        p.intimacy = Intimacy::Advance;
        p.action = ActionKind::Talk;
        rig.gm.push_proposal(serde_json::to_string(&p).unwrap());
        rig.gm.push_narration("Brann nods.".into());
        let t1 = rig
            .engine()
            .play_turn("t1".into(), intent, &mut |_| {})
            .expect(intent)
            .turn_id;
        assert_eq!(rig.scene(), Some(t1), "{intent}");
        let narrate = rig.last_call(CallKind::Narrate);
        assert!(narrate.prompt.recent.contains(IMPLIED_LINE), "{intent}");
    }
}

/// QA run 3: an `advance` Turn opens no scene, so its Narration must not
/// go intimate. The narrator of such a Turn reads one outcome line that
/// says so: the advance and the answer, then it stops.
#[test]
fn an_advance_tells_the_narrator_that_no_scene_begins() {
    let mut rig = Rig::new("advance-line");
    let mut p = rig.intimate();
    p.intimacy = Intimacy::Advance;
    p.action = ActionKind::Talk;
    rig.gm.push_proposal(serde_json::to_string(&p).unwrap());
    rig.gm.push_narration("Brann smiles.".into());
    rig.engine()
        .play_turn("t1".into(), "I kiss Brann.", &mut |_| {})
        .unwrap();
    assert_eq!(rig.scene(), None);
    let narrate = rig.last_call(CallKind::Narrate);
    assert!(
        narrate.prompt.recent.contains(ADVANCE_LINE),
        "{}",
        narrate.prompt.recent
    );
    // A plain Turn gets no such line.
    let p = rig.plain();
    rig.played("t2", &p, "Dusk falls.");
    let narrate = rig.last_call(CallKind::Narrate);
    assert!(!narrate.prompt.recent.contains(ADVANCE_LINE));
}
