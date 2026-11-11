mod support;

use std::panic::{AssertUnwindSafe, catch_unwind};

use support::*;
use threshold_core::{
    CallKind, Gm, GmError, Prompt, RecordedGm,
    checks::CheckKind,
    domain::{
        ActionKind, DecisionProposal, EntityRef, FactSource, Knowing, LedgerOp, Reaction,
        SeedEntity, SeedFact, Seen, Stance, Verdict,
    },
    rules::{D10, Ruleset, turn_seed},
    store::{Store, WorldView},
    turn::{Engine, MAX_INTENT_CHARS, TurnError, create_campaign, stored_action},
};

fn json(p: &DecisionProposal) -> String {
    serde_json::to_string(p).unwrap()
}

fn make_engine(store: Store, campaign: u64, gm: &RecordedGm) -> Engine {
    Engine::new(store, Box::new(gm.clone()), Box::new(D10), vec![], campaign)
}

/// The World fixture as an engine, the fixture's ids, and the shared Gm.
fn setup(proposals: Vec<String>, narrations: Vec<&str>) -> (Engine, World, RecordedGm) {
    let mut w = World::new();
    let gm = RecordedGm::new(proposals, narrations.into_iter().map(Into::into).collect());
    let store = std::mem::replace(&mut w.store, Store::open_in_memory().unwrap());
    (make_engine(store, w.campaign, &gm), w, gm)
}

fn snapshot(engine: &Engine, w: &World) -> WorldView {
    engine.store().snapshot(w.campaign).unwrap()
}

fn kinds(gm: &RecordedGm) -> Vec<CallKind> {
    gm.calls().into_iter().map(|c| c.kind).collect()
}

fn prompts(gm: &RecordedGm, kind: CallKind) -> Vec<Prompt> {
    gm.calls()
        .into_iter()
        .filter(|c| c.kind == kind)
        .map(|c| c.prompt)
        .collect()
}

fn fields(p: &Prompt) -> [&str; 4] {
    [&p.system, &p.stable, &p.scene, &p.recent]
}

/// A coincidence trap: the locked gate opens with no cause.
fn trap(w: &World) -> DecisionProposal {
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.gate), "locked", "no"), Seen::Pc, vec![])];
    p
}

/// The same opening, caused by an existing fact.
fn caused(w: &World) -> DecisionProposal {
    let mut p = trap(w);
    p.facts[0].because = vec![f(w.secret)];
    p
}

/// A Check against the locked gate.
fn gate_check(w: &World) -> DecisionProposal {
    let mut p = proposal(Verdict::Check);
    p.blockers = vec![f(w.gate_locked)];
    p.check = Some(check(
        None,
        slot(e(w.gate), "locked", "no"),
        slot(e(w.ada), "fatigue", "winded"),
    ));
    p
}

fn no_chunks() -> impl FnMut(&str) {
    |_: &str| {}
}

// --- Brief tests ------------------------------------------------------------

#[test]
fn malformed_twice_commits_nothing() {
    let (mut engine, w, gm) = setup(
        vec!["not json".into(), r#"{"action":"act"}"#.into()],
        vec!["never"],
    );
    let before = snapshot(&engine, &w);
    let result = engine.play_turn("turn-1".into(), "I open the gate.", &mut no_chunks());
    let Err(TurnError::Rejected(errs)) = result else {
        panic!("expected Rejected, got {result:?}");
    };
    assert!(
        errs.iter().all(|r| r.check == CheckKind::Schema),
        "{errs:?}"
    );
    assert_eq!(snapshot(&engine, &w), before);
    assert_eq!(kinds(&gm), [CallKind::Propose, CallKind::Propose]);
    // The failed Turn stays visible in the debug log.
    let steps: Vec<String> = engine
        .store()
        .debug_log("turn-1")
        .unwrap()
        .into_iter()
        .map(|e| e.step)
        .collect();
    assert_eq!(steps, ["propose", "validate", "propose_retry", "validate"]);
}

#[test]
fn rejected_then_valid_commits_once() {
    let w0 = World::new();
    let (mut engine, w, gm) = setup(
        vec![json(&trap(&w0)), json(&caused(&w0))],
        vec!["The gate swings open."],
    );
    let before = snapshot(&engine, &w);
    let outcome = engine
        .play_turn("turn-1".into(), "I open the gate.", &mut no_chunks())
        .unwrap();
    let after = snapshot(&engine, &w);
    assert_eq!(after.turn_seq, before.turn_seq + 1, "exactly one Turn row");
    assert_eq!(after.live_fact(w.gate, "locked").unwrap().slot.value, "no");
    assert_eq!(outcome.narration, "The gate swings open.");
    assert_eq!(
        kinds(&gm),
        [CallKind::Propose, CallKind::Propose, CallKind::Narrate]
    );
    // The retry carries the rejection lines in `recent`.
    let retry = &prompts(&gm, CallKind::Propose)[1];
    assert!(retry.recent.contains("coincidence"), "{}", retry.recent);
    let first = &prompts(&gm, CallKind::Propose)[0];
    assert_eq!(
        (&first.system, &first.stable, &first.scene),
        (&retry.system, &retry.stable, &retry.scene)
    );
}

#[test]
fn retry_with_same_op_id_returns_stored_turn() {
    let mut p = proposal(Verdict::Allowed);
    p.minutes = 5;
    let (mut engine, w, gm) = setup(vec![json(&p)], vec!["You wait."]);
    let before = snapshot(&engine, &w);
    let first = engine
        .play_turn("turn-1".into(), "I wait.", &mut no_chunks())
        .unwrap();
    let second = engine
        .play_turn("turn-1".into(), "I wait.", &mut no_chunks())
        .unwrap();
    assert_eq!(second.turn_id, first.turn_id);
    assert_eq!(second.narration, "You wait.");
    let after = snapshot(&engine, &w);
    assert_eq!(after.turn_seq, before.turn_seq + 1);
    assert_eq!(gm.calls().len(), 2, "the retry makes no model call");
    // The clock moves once, not once per op list.
    assert_eq!(after.clock_min, before.clock_min + 5);
}

#[test]
fn seeded_check_is_reproducible() {
    let rolls: Vec<_> = (0..2)
        .map(|_| {
            let w0 = World::new();
            let (mut engine, _w, _gm) = setup(vec![json(&gate_check(&w0))], vec!["n"]);
            engine
                .play_turn("turn-1".into(), "I force the gate.", &mut no_chunks())
                .unwrap()
                .roll
                .expect("a Check rolls")
        })
        .collect();
    assert_eq!(rolls[0], rolls[1]);
    let w = World::new();
    assert_eq!(rolls[0].seed, turn_seed(7, w.view.turn_seq + 1));
    assert_eq!(rolls[0], D10.roll(rolls[0].difficulty, rolls[0].seed));
}

#[test]
fn correction_keeps_seed() {
    let w0 = World::new();
    let mut clarified = gate_check(&w0);
    clarified.check.as_mut().unwrap().lose = Some(slot(e(w0.ada), "injury", "bruised hand"));
    let (mut engine, w, gm) = setup(
        vec![json(&gate_check(&w0)), json(&clarified)],
        vec!["first", "second"],
    );
    let before = snapshot(&engine, &w);
    let played = engine
        .play_turn("turn-1".into(), "I force the gate.", &mut no_chunks())
        .unwrap();
    let corrected = engine
        .correct_last(
            "turn-2".into(),
            "I pick the lock.",
            "I meant picking, not forcing",
            &mut no_chunks(),
        )
        .unwrap();
    let (a, b) = (played.roll.unwrap(), corrected.roll.unwrap());
    assert_eq!((a.seed, a.value), (b.seed, b.value));
    assert_eq!(corrected.narration, "second");

    // The Correction re-plans from the state before the corrected Turn.
    let retry = &prompts(&gm, CallKind::Propose)[1];
    assert!(retry.recent.contains("I pick the lock."));
    assert!(!retry.recent.contains("I force the gate."));

    let ops = engine.store().turn_ops(corrected.turn_id).unwrap();
    assert_eq!(
        ops[0],
        LedgerOp::RevertTurn {
            turn_id: played.turn_id
        }
    );
    assert!(ops.iter().any(|op| matches!(
        op,
        LedgerOp::RecordCorrection { target_turn, why, .. }
            if *target_turn == played.turn_id && why == "I meant picking, not forcing"
    )));
    let after = snapshot(&engine, &w);
    assert_eq!(after.turn_seq, before.turn_seq + 2, "history is kept");
    assert!(
        after.recent_turns.iter().all(|t| t.id != played.turn_id),
        "the reverted Turn leaves the recent Turns"
    );
    // Only one Roll is live: the reverted one is gone from the projections.
    // The fixed seed decides the branch; both branches are checked.
    let seq = before.turn_seq + 1;
    assert_eq!(b, D10.roll(b.difficulty, turn_seed(7, seq)));
    assert!(
        after.live_fact(w.ada, "fatigue").is_none(),
        "old stake reverted"
    );
    if b.success {
        assert_eq!(after.live_fact(w.gate, "locked").unwrap().slot.value, "no");
        assert!(after.live_fact(w.ada, "injury").is_none());
    } else {
        assert_eq!(
            after.live_fact(w.ada, "injury").unwrap().slot.value,
            "bruised hand"
        );
        assert_eq!(after.live_fact(w.gate, "locked").unwrap().slot.value, "yes");
    }
}

#[test]
fn only_the_last_play_turn_can_be_corrected() {
    let p = proposal(Verdict::Allowed);
    let (mut engine, w, gm) = setup(vec![json(&p), json(&p)], vec!["a", "b"]);
    engine
        .play_turn("turn-1".into(), "I wait.", &mut no_chunks())
        .unwrap();
    engine
        .correct_last("turn-2".into(), "I sit.", "misread", &mut no_chunks())
        .unwrap();
    let before = snapshot(&engine, &w);
    let result = engine.correct_last("turn-3".into(), "I stand.", "again", &mut no_chunks());
    assert!(
        matches!(result, Err(TurnError::NotCorrectable(_))),
        "{result:?}"
    );
    assert_eq!(snapshot(&engine, &w), before);
    assert_eq!(gm.calls().len(), 4);
}

#[test]
fn empty_and_long_intents_refused() {
    let (mut engine, w, gm) = setup(vec![], vec![]);
    let before = snapshot(&engine, &w);
    assert!(matches!(
        engine.play_turn("turn-1".into(), "   ", &mut no_chunks()),
        Err(TurnError::EmptyIntent)
    ));
    let long = "a".repeat(MAX_INTENT_CHARS + 1);
    assert_eq!(long.chars().count(), 2_001);
    assert!(matches!(
        engine.play_turn("turn-2".into(), &long, &mut no_chunks()),
        Err(TurnError::IntentTooLong)
    ));
    assert!(matches!(
        engine.correct_last("turn-3".into(), "", "why", &mut no_chunks()),
        Err(TurnError::EmptyIntent)
    ));
    assert!(matches!(
        engine.ooc("ooc-0".into(), " "),
        Err(TurnError::EmptyIntent)
    ));
    assert!(gm.calls().is_empty(), "no model call for a refused Intent");
    assert_eq!(snapshot(&engine, &w), before);
}

#[test]
fn interrupted_narration_recoverable() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("turn-interrupted-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut store = Store::open(&path).unwrap();
    let campaign = create_campaign(&mut store, &adventure_seed(), 7).unwrap();
    let gm = RecordedGm::new(
        vec![json(&proposal(Verdict::Allowed))],
        vec!["You look around.".into()],
    );
    let mut engine = make_engine(store, campaign, &gm);
    let result = catch_unwind(AssertUnwindSafe(|| {
        engine.play_turn("turn-1".into(), "I look around.", &mut |_| {
            panic!("the window closed")
        })
    }));
    assert!(result.is_err(), "on_chunk panicked");
    drop(engine);

    let store = Store::open(&path).unwrap();
    let turn = store
        .turn_by_campaign_op(campaign, &"turn-1".to_owned())
        .unwrap()
        .expect("the Turn was committed before narration");
    assert_eq!(turn.narration, None);
    let gm = RecordedGm::new(vec![], vec!["Again, you look around.".into()]);
    let mut engine = make_engine(store, campaign, &gm);
    let mut chunks = String::new();
    let narration = engine
        .renarrate(turn.id, &mut |c| chunks.push_str(c))
        .unwrap();
    assert_eq!(narration, "Again, you look around.");
    assert_eq!(chunks, narration);
    let stored = engine.store().turn(turn.id).unwrap().unwrap();
    assert_eq!(stored.narration.as_deref(), Some("Again, you look around."));
    let narrate = &prompts(&gm, CallKind::Narrate)[0];
    assert!(narrate.recent.contains("I look around."));
    drop(engine);
    std::fs::remove_file(&path).unwrap();
}

struct DownGm;

impl Gm for DownGm {
    fn propose(&self, _: &Prompt, _: &serde_json::Value) -> Result<String, GmError> {
        Err(GmError::Unreachable("connection refused".into()))
    }
    fn narrate(&self, _: &Prompt, _: &mut dyn FnMut(&str)) -> Result<String, GmError> {
        Err(GmError::Unreachable("connection refused".into()))
    }
}

#[test]
fn model_down_commits_nothing() {
    let w = World::new();
    let before = w.view.clone();
    let mut engine = Engine::new(w.store, Box::new(DownGm), Box::new(D10), vec![], w.campaign);
    let result = engine.play_turn("turn-1".into(), "I wait.", &mut no_chunks());
    assert!(
        matches!(result, Err(TurnError::Model(GmError::Unreachable(_)))),
        "{result:?}"
    );
    assert_eq!(engine.store().snapshot(w.campaign).unwrap(), before);
}

#[test]
fn ooc_never_sees_secrets() {
    let mut w = World::new();
    // One fact the PC witnessed: PC Knowledge reaches the OOC prompt.
    w.commit(vec![LedgerOp::AddFact {
        slot: threshold_core::domain::Slot {
            subject: EntityRef::Id(w.gate),
            key: "color".into(),
            value: "red".into(),
        },
        text: "The gate is painted red.".into(),
        canon: threshold_core::domain::Canon::Established,
        source: threshold_core::domain::FactSource::Proposal,
        seen_by: vec![EntityRef::Id(w.ada)],
        causes: vec![],
    }]);
    let before = w.view.clone();
    let gm = RecordedGm::new(vec![], vec!["You are Ada, a courier.".into()]);
    let mut engine = make_engine(w.store, w.campaign, &gm);
    let answer = engine
        .ooc("ooc-1".into(), "Who am I, and what do I know?")
        .unwrap();
    assert_eq!(answer, "You are Ada, a courier.");
    assert_eq!(kinds(&gm), [CallKind::Narrate]);
    let prompt = &gm.calls()[0].prompt;
    let all = fields(prompt).concat();
    assert!(all.contains("The gate is painted red."), "{all}");
    assert!(all.contains("A town gate at dusk"), "{all}");
    // GM-only and NPC-only facts, and unconfirmed seed rumors, stay out.
    for secret in [
        "gate key",
        "owes",
        "bribable",
        "Keep Ada out",
        "rings the bell",
    ] {
        assert!(!all.contains(secret), "{secret} leaked:\n{all}");
    }
    assert_eq!(engine.store().snapshot(w.campaign).unwrap(), before);

    // Each OOC question has its own debug rows.
    engine.ooc("ooc-2".into(), "And now?").unwrap_err();
    let steps = |op: &str| -> Vec<String> {
        engine
            .store()
            .debug_log(op)
            .unwrap()
            .into_iter()
            .map(|e| e.step)
            .collect()
    };
    assert_eq!(steps("ooc-1"), ["ooc"]);
    assert_eq!(steps("ooc-2"), ["ooc"]);
}

/// ADR 0005: the OOC prompt lists PC Knowledge, so an open item there
/// carries the marker too.
#[test]
fn ooc_prompt_marks_open_items() {
    let mut w = World::new();
    w.commit(vec![LedgerOp::AddFact {
        slot: threshold_core::domain::Slot {
            subject: EntityRef::Id(w.gate),
            key: "pending_repair".into(),
            value: "next week".into(),
        },
        text: "The gate is to be repaired next week.".into(),
        canon: threshold_core::domain::Canon::Established,
        source: threshold_core::domain::FactSource::Proposal,
        seen_by: vec![EntityRef::Id(w.ada)],
        causes: vec![],
    }]);
    let gm = RecordedGm::new(vec![], vec!["Not yet.".into()]);
    let mut engine = make_engine(w.store, w.campaign, &gm);
    engine.ooc("ooc-1".into(), "Is the gate repaired?").unwrap();
    let all = fields(&gm.calls()[0].prompt).concat();
    assert!(
        all.contains("(open, not done) The gate is to be repaired next week."),
        "{all}"
    );
}

// --- Controller rulings -----------------------------------------------------

/// Ruling 2 and 3: final reactions are stored with the Turn, and only the
/// reactions of NPCs the PC knows are here reach the narrator.
#[test]
fn narrator_gets_only_reactions_of_seen_npcs() {
    let mut w = World::new();
    // The PC has seen Brann at the gate, but not Orsel.
    let brann_location = w.view.live_fact(w.brann, "location").unwrap().id;
    w.commit(vec![LedgerOp::Know {
        holder: EntityRef::Id(w.ada),
        fact_id: Some(brann_location),
        claim_id: None,
        how: Knowing::Witnessed,
        arrives_at: 0,
    }]);
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Talk;
    p.reactions = vec![
        Reaction {
            npc: e(w.brann),
            stance: Stance::Neutral,
            plan: "shrugs_and_listens".into(),
            cause: None,
        },
        Reaction {
            npc: e(w.orsel),
            stance: Stance::Resist,
            plan: "watches_from_the_dark".into(),
            cause: None,
        },
    ];
    let gm = RecordedGm::new(vec![json(&p)], vec!["Brann shrugs.".into()]);
    let mut engine = make_engine(w.store, w.campaign, &gm);
    let outcome = engine
        .play_turn("turn-1".into(), "I greet everyone.", &mut no_chunks())
        .unwrap();
    let narration = &prompts(&gm, CallKind::Narrate)[0];
    let all = fields(narration).concat();
    assert!(all.contains("shrugs_and_listens"), "{all}");
    assert!(!all.contains("watches_from_the_dark"), "{all}");
    assert!(!all.contains("Orsel"), "{all}");

    let stored = engine.store().turn(outcome.turn_id).unwrap().unwrap();
    let record: serde_json::Value = serde_json::from_str(&stored.record.proposal_json).unwrap();
    let reactions: Vec<Reaction> = serde_json::from_value(record["reactions"].clone()).unwrap();
    assert_eq!(reactions, p.reactions);
    let stored_proposal: DecisionProposal =
        serde_json::from_value(record["proposal"].clone()).unwrap();
    // C9: a talk with no minutes takes the Ruleset default.
    let mut expected = p.clone();
    expected.minutes = threshold_core::rules::TALK_DEFAULT_MIN;
    assert_eq!(stored_proposal, expected);
}

/// Ruling 4: at setup and on arrival, the PC sees who openly stands there,
/// never a hidden entity.
#[test]
fn arrival_shows_open_npcs_and_keeps_hidden_out() {
    let mut seed = adventure_seed();
    seed.entities.push(SeedEntity {
        name: "Vell".into(),
        kind: threshold_core::domain::EntityKind::Npc,
        age: Some(40),
        other_names: vec![],
        hidden: true,
    });
    seed.decided.push(SeedFact {
        subject: "Vell".into(),
        key: "location".into(),
        value: "market".into(),
        text: "Vell lurks in the market.".into(),
    });
    seed.entities.push(place("Market"));
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, &seed, 7).unwrap();
    let view = store.snapshot(campaign).unwrap();
    let id = |name: &str| view.entities.iter().find(|e| e.name == name).unwrap().id;
    let ada = id("Ada");

    let look = proposal(Verdict::Allowed);
    let mut walk = proposal(Verdict::Allowed);
    walk.action = ActionKind::Move;
    walk.minutes = 10;
    walk.move_to = Some(e(id("Market")));
    let gm = RecordedGm::new(
        vec![json(&look), json(&walk)],
        vec!["At the gate.".into(), "At the market.".into()],
    );
    let mut engine = make_engine(store, campaign, &gm);
    engine
        .play_turn("turn-1".into(), "I look around.", &mut no_chunks())
        .unwrap();
    engine
        .play_turn("turn-2".into(), "I walk to the market.", &mut no_chunks())
        .unwrap();
    let narrations = prompts(&gm, CallKind::Narrate);
    // Setup: Orsel and Brann stand openly at the gate.
    assert!(
        narrations[0].scene.contains("Orsel"),
        "{}",
        narrations[0].scene
    );
    assert!(narrations[0].scene.contains("Brann"));
    // Arrival: Pip stands openly at the market; Vell hides there.
    let market = &narrations[1];
    assert!(market.scene.contains("Pip"), "{}", market.scene);
    for field in fields(market) {
        assert!(!field.contains("Vell"), "Vell in {field}");
    }
    assert!(!market.scene.contains("Orsel"));
    let after = engine.store().snapshot(campaign).unwrap();
    let pip_location = after.live_fact(id("Pip"), "location").unwrap().id;
    let pip_known = after
        .knowledge
        .iter()
        .find(|k| k.holder == ada && k.fact_id == Some(pip_location))
        .expect("the PC witnessed Pip's location on arrival");
    // C9: the look with no minutes takes the act default, then the walk 10.
    let at = u64::from(threshold_core::rules::ACT_DEFAULT_MIN) + 10;
    assert_eq!((pip_known.learned_at, pip_known.arrives_at), (at, at));
}

/// Ruling 5: in one Scene, with no present entity's fact changed, the
/// stable and scene blocks of two proposal prompts are byte-identical.
#[test]
fn proposal_prefix_is_stable_within_a_scene() {
    let p = proposal(Verdict::Allowed);
    let (mut engine, _w, gm) = setup(vec![json(&p), json(&p)], vec!["a", "b"]);
    let first = engine
        .play_turn("turn-1".into(), "I wait.", &mut no_chunks())
        .unwrap();
    engine
        .play_turn("turn-2".into(), "I wait some more.", &mut no_chunks())
        .unwrap();
    let proposals = prompts(&gm, CallKind::Propose);
    let prefix = |p: &Prompt| format!("{}{}{}", p.system, p.stable, p.scene);
    assert_eq!(prefix(&proposals[0]), prefix(&proposals[1]));
    assert_ne!(proposals[0].recent, proposals[1].recent);
    assert_eq!(
        first.timings.proposal_prompt_tokens, None,
        "RecordedGm reports none"
    );
    assert_eq!(first.timings.narration_prompt_tokens, None);
    let steps: Vec<String> = engine
        .store()
        .debug_log("turn-1")
        .unwrap()
        .into_iter()
        .map(|e| e.step)
        .collect();
    assert_eq!(
        steps,
        ["propose", "validate", "roll", "world", "commit", "narrate"]
    );
}

// --- Fix round 1 -----------------------------------------------------------

/// Schedules a World Event due at `at` that sets `subject.key = value`,
/// with no witnesses.
fn schedule_write(w: &mut World, at: u64, subject: u64, key: &str, value: &str) {
    w.commit(vec![LedgerOp::ScheduleEvent {
        text: format!("{key} becomes {value}"),
        trigger: threshold_core::domain::Trigger::At(at),
        actors: vec![EntityRef::Id(w.orsel)],
        goal_id: None,
        cause_fact_id: None,
        effects: vec![threshold_core::domain::Effect::SetFact {
            slot: threshold_core::domain::Slot {
                subject: EntityRef::Id(subject),
                key: key.into(),
                value: value.into(),
            },
            text: format!("{key} {value}"),
            seen_by: vec![],
        }],
        seen_by: vec![],
        public: false,
    }]);
}

/// A due World Event that writes a slot the proposal also writes waits for
/// the next Turn instead of breaking the Commit.
#[test]
fn colliding_event_waits_for_the_next_turn() {
    let mut w = World::new();
    let brann = w.brann;
    schedule_write(&mut w, 0, brann, "mood", "angry");
    let event = w
        .view
        .events_scheduled
        .iter()
        .find(|e| e.text == "mood becomes angry")
        .unwrap()
        .id;
    let mut calm = proposal(Verdict::Allowed);
    calm.facts = vec![fact(
        slot(e(w.brann), "mood", "calm"),
        Seen::Present,
        vec![],
    )];
    let wait = proposal(Verdict::Allowed);
    let gm = RecordedGm::new(vec![json(&calm), json(&wait)], vec!["a".into(), "b".into()]);
    let campaign = w.campaign;
    let mut engine = make_engine(w.store, campaign, &gm);
    engine
        .play_turn("turn-1".into(), "I calm Brann.", &mut no_chunks())
        .unwrap();
    let view = engine.store().snapshot(campaign).unwrap();
    assert_eq!(view.live_fact(brann, "mood").unwrap().slot.value, "calm");
    assert!(view.events_scheduled.iter().any(|e| e.id == event));

    engine
        .play_turn("turn-2".into(), "I wait.", &mut no_chunks())
        .unwrap();
    let view = engine.store().snapshot(campaign).unwrap();
    assert_eq!(view.live_fact(brann, "mood").unwrap().slot.value, "angry");
    assert!(view.events_scheduled.iter().all(|e| e.id != event));
}

/// Amended ruling 4(b): presence placed by a World Event is open. Event
/// `seen_by` decides who witnesses the event itself; on arrival the PC sees
/// the NPC it moved. `same_turn` fires the event in the arrival Turn.
fn event_moved_npc_is_seen_on_arrival(same_turn: bool) {
    let mut w = World::new();
    let brann = w.brann;
    schedule_write(&mut w, 5, brann, "location", "market");
    let mut wait = proposal(Verdict::Allowed);
    wait.minutes = 10;
    let mut walk = proposal(Verdict::Allowed);
    walk.action = ActionKind::Move;
    walk.minutes = 10;
    walk.move_to = Some(e(w.add_place("Market")));
    let mut proposals = vec![json(&walk)];
    if !same_turn {
        proposals.insert(0, json(&wait));
    }
    let gm = RecordedGm::new(proposals, vec!["a".into(), "b".into()]);
    let (ada, campaign) = (w.ada, w.campaign);
    let mut engine = make_engine(w.store, campaign, &gm);
    if !same_turn {
        engine
            .play_turn("turn-1".into(), "I wait.", &mut no_chunks())
            .unwrap();
        let view = engine.store().snapshot(campaign).unwrap();
        let moved = view.live_fact(brann, "location").unwrap();
        assert_eq!(moved.slot.value, "market");
        assert!(!moved.seen_by.contains(&ada), "the absent PC saw nothing");
    }
    engine
        .play_turn("turn-2".into(), "I walk to the market.", &mut no_chunks())
        .unwrap();
    let narration = prompts(&gm, CallKind::Narrate).pop().unwrap();
    assert!(narration.scene.contains("Brann"), "{}", narration.scene);
    assert!(narration.scene.contains("Pip"), "{}", narration.scene);
}

#[test]
fn event_moved_npc_is_seen_on_later_arrival() {
    event_moved_npc_is_seen_on_arrival(false);
}

#[test]
fn event_moved_npc_is_seen_when_arriving_in_the_same_turn() {
    event_moved_npc_is_seen_on_arrival(true);
}

/// Regression: the model sent `minutes: 0` on every `wait`, so the World
/// Clock never moved and the player was stuck. The engine owns the length of
/// a wait.
#[test]
fn wait_without_minutes_moves_the_clock() {
    let mut wait = proposal(Verdict::Allowed);
    wait.action = ActionKind::Wait;
    wait.minutes = 0;
    let (mut engine, w, _gm) = setup(vec![json(&wait)], vec!["Time passes."]);
    let before = snapshot(&engine, &w);
    engine
        .play_turn("turn-1".into(), "I wait.", &mut no_chunks())
        .unwrap();
    let after = snapshot(&engine, &w);
    let turn = engine
        .store()
        .turn_by_campaign_op(w.campaign, &"turn-1".to_owned())
        .unwrap()
        .unwrap();
    assert!(turn.record.clock_after > turn.record.clock_before);
    assert!(after.clock_min > before.clock_min);
}

/// Schedules a World Event at `at` that sets `subject.key = value`, done by
/// `actor` and witnessed by `seen_by`.
fn schedule_seen(
    w: &mut World,
    at: u64,
    actor: u64,
    seen_by: Vec<u64>,
    (subject, key, value): (u64, &str, &str),
) {
    w.commit(vec![LedgerOp::ScheduleEvent {
        text: format!("{key} becomes {value}"),
        trigger: threshold_core::domain::Trigger::At(at),
        actors: vec![EntityRef::Id(actor)],
        goal_id: None,
        cause_fact_id: None,
        effects: vec![threshold_core::domain::Effect::SetFact {
            slot: threshold_core::domain::Slot {
                subject: EntityRef::Id(subject),
                key: key.into(),
                value: value.into(),
            },
            text: format!("{key} {value}"),
            seen_by: vec![],
        }],
        seen_by: seen_by.into_iter().map(EntityRef::Id).collect(),
        public: false,
    }]);
}

/// Plays one 60-minute wait from minute 40 (past the fixture's GM-only
/// bell) over the World Event that `schedule` adds at minute 70. Returns
/// the minute after the Turn, whether the event fired, and the narration's
/// `recent` block.
fn wait_an_hour_over(schedule: impl FnOnce(&mut World, u64)) -> (u64, bool, String) {
    let mut w = World::new();
    w.commit(vec![LedgerOp::AdvanceClock { minutes: 40 }]);
    schedule(&mut w, 70);
    let event = w
        .view
        .events_scheduled
        .iter()
        .find(|e| e.trigger == threshold_core::domain::Trigger::At(70))
        .unwrap()
        .id;
    let mut wait = proposal(Verdict::Allowed);
    wait.action = ActionKind::Wait;
    wait.minutes = 60;
    let gm = RecordedGm::new(vec![json(&wait)], vec!["a".into()]);
    let campaign = w.campaign;
    let mut engine = make_engine(w.store, campaign, &gm);
    let outcome = engine
        .play_turn("turn-1".into(), "I wait an hour.", &mut no_chunks())
        .unwrap();
    let view = engine.store().snapshot(campaign).unwrap();
    let ops = engine.store().turn_ops(outcome.turn_id).unwrap();
    let fired = ops.contains(&LedgerOp::FireEvent { event_id: event });
    let recent = prompts(&gm, CallKind::Narrate).pop().unwrap().recent;
    (view.clock_min, fired, recent)
}

/// A wait ends when something happens that the PC sees: the next timed
/// World Event whose actor stands with the PC.
#[test]
fn wait_ends_at_the_next_event_and_fires_it() {
    let (clock, fired, recent) = wait_an_hour_over(|w, at| {
        let (orsel, brann) = (w.orsel, w.brann);
        schedule_seen(w, at, orsel, vec![brann], (brann, "mood", "angry"));
    });
    assert_eq!(clock, 70);
    assert!(fired);
    assert!(
        recent.contains("Time this Turn takes: 30 minutes."),
        "{recent}"
    );
    assert!(recent.contains("- mood becomes angry\n"), "{recent}");
}

/// A GM-only event does not cut the wait. It still fires, offscreen.
#[test]
fn gm_only_event_does_not_cut_a_wait() {
    let (clock, fired, recent) = wait_an_hour_over(|w, at| {
        let (orsel, brann) = (w.orsel, w.brann);
        schedule_seen(w, at, orsel, vec![], (brann, "mood", "angry"));
    });
    assert_eq!(clock, 100);
    assert!(fired);
    assert!(
        recent.contains("Time this Turn takes: 60 minutes."),
        "{recent}"
    );
    assert!(!recent.contains("mood becomes angry"), "{recent}");
}

/// An event whose actor is elsewhere does not cut the wait.
#[test]
fn event_elsewhere_does_not_cut_a_wait() {
    let (clock, fired, recent) = wait_an_hour_over(|w, at| {
        let (pip, brann) = (w.pip, w.brann);
        schedule_seen(w, at, pip, vec![brann], (pip, "mood", "angry"));
    });
    assert_eq!(clock, 100);
    assert!(fired);
    assert!(!recent.contains("mood becomes angry"), "{recent}");
}

/// A `Gm` that reports prompt tokens: 100 for a proposal, 40 for narration.
struct TokenGm {
    inner: RecordedGm,
    last: std::sync::Mutex<Option<u64>>,
}

impl Gm for TokenGm {
    fn propose(&self, prompt: &Prompt, schema: &serde_json::Value) -> Result<String, GmError> {
        *self.last.lock().unwrap() = Some(100);
        self.inner.propose(prompt, schema)
    }
    fn narrate(&self, prompt: &Prompt, on_chunk: &mut dyn FnMut(&str)) -> Result<String, GmError> {
        *self.last.lock().unwrap() = Some(40);
        self.inner.narrate(prompt, on_chunk)
    }
    fn last_prompt_tokens(&self) -> Option<u64> {
        *self.last.lock().unwrap()
    }
}

#[test]
fn timings_record_prompt_tokens_of_both_calls() {
    let w = World::new();
    let gm = TokenGm {
        inner: RecordedGm::new(vec![json(&proposal(Verdict::Allowed))], vec!["n".into()]),
        last: std::sync::Mutex::new(None),
    };
    let mut engine = Engine::new(w.store, Box::new(gm), Box::new(D10), vec![], w.campaign);
    let outcome = engine
        .play_turn("turn-1".into(), "I wait.", &mut no_chunks())
        .unwrap();
    assert_eq!(outcome.timings.proposal_prompt_tokens, Some(100));
    assert_eq!(outcome.timings.narration_prompt_tokens, Some(40));
}

/// Final fix 1: a narration that fails after the Commit reports the
/// committed Turn. A retry under the same op_id narrates it again and adds
/// no second Turn row.
#[test]
fn narration_failure_after_commit_reports_the_turn() {
    let (mut engine, w, gm) = setup(vec![json(&proposal(Verdict::Allowed))], vec![]);
    let before = snapshot(&engine, &w);
    let result = engine.play_turn("turn-1".into(), "I look around.", &mut no_chunks());
    let Err(TurnError::NarrationFailed { turn_id, .. }) = result else {
        panic!("expected NarrationFailed, got {result:?}");
    };
    let stored = engine.store().turn(turn_id).unwrap().unwrap();
    assert_eq!(stored.record.op_id, "turn-1");
    assert_eq!(stored.narration, None);
    assert_eq!(snapshot(&engine, &w).turn_seq, before.turn_seq + 1);

    gm.push_narration("You look around.".into());
    let outcome = engine
        .play_turn("turn-1".into(), "I look around.", &mut no_chunks())
        .unwrap();
    assert_eq!(outcome.turn_id, turn_id);
    assert_eq!(outcome.narration, "You look around.");
    let after = snapshot(&engine, &w);
    assert_eq!(after.turn_seq, before.turn_seq + 1, "exactly one Turn row");
    let stored = engine.store().turn(turn_id).unwrap().unwrap();
    assert_eq!(stored.narration.as_deref(), Some("You look around."));
    assert_eq!(
        kinds(&gm),
        [CallKind::Propose, CallKind::Narrate, CallKind::Narrate]
    );
}

/// A retry whose narration fails again still reports the committed Turn.
#[test]
fn narration_failure_on_retry_reports_the_turn() {
    let (mut engine, _w, _gm) = setup(vec![json(&proposal(Verdict::Allowed))], vec![]);
    let first = engine.play_turn("turn-1".into(), "I wait.", &mut no_chunks());
    let Err(TurnError::NarrationFailed { turn_id, .. }) = first else {
        panic!("expected NarrationFailed, got {first:?}");
    };
    let second = engine.play_turn("turn-1".into(), "I wait.", &mut no_chunks());
    assert!(
        matches!(second, Err(TurnError::NarrationFailed { turn_id: t, .. }) if t == turn_id),
        "{second:?}"
    );
}

/// Final fix 4: a Correction commits its facts with `FactSource::Correction`,
/// and the store reports the Turn it reverted.
#[test]
fn correction_facts_have_the_correction_source() {
    let w0 = World::new();
    let mut first = proposal(Verdict::Allowed);
    first.facts = vec![fact(slot(e(w0.gate), "color", "red"), Seen::Pc, vec![])];
    let mut clarified = proposal(Verdict::Allowed);
    clarified.facts = vec![fact(slot(e(w0.gate), "color", "blue"), Seen::Pc, vec![])];
    let (mut engine, w, _gm) = setup(vec![json(&first), json(&clarified)], vec!["a", "b"]);
    let played = engine
        .play_turn("turn-1".into(), "I paint the gate red.", &mut no_chunks())
        .unwrap();
    assert!(
        engine
            .store()
            .reverted_turns(w.campaign)
            .unwrap()
            .is_empty()
    );
    let corrected = engine
        .correct_last(
            "turn-2".into(),
            "I paint the gate blue.",
            "blue, not red",
            &mut no_chunks(),
        )
        .unwrap();
    let after = snapshot(&engine, &w);
    let color = after.live_fact(w.gate, "color").unwrap();
    assert_eq!(color.slot.value, "blue");
    assert_eq!(color.source, FactSource::Correction);
    assert_eq!(color.canon, threshold_core::domain::Canon::Persistent);
    assert_eq!(color.created_turn, corrected.turn_id);
    let reverted = engine.store().reverted_turns(w.campaign).unwrap();
    assert_eq!(reverted.into_iter().collect::<Vec<_>>(), [played.turn_id]);
}

/// Narrates one wait over a World Event at minute 10 with a witness, and
/// returns the narration's `recent` block and the PC's Beliefs on its slot.
/// The event moves Orsel to the market: a change the PC sees.
fn wait_over_witnessed_event(actor: fn(&World) -> u64) -> (String, bool) {
    wait_over_event_with_text(actor, "Orsel waves the courier over")
}

/// Like `wait_over_witnessed_event`, with the event's own text.
fn wait_over_event_with_text(actor: fn(&World) -> u64, text: &str) -> (String, bool) {
    wait_over_event(actor, text, |w| (w.orsel, "location", "market"))
}

/// One wait over a World Event at minute 10, seen by Brann and Pip, whose
/// actor is `actor` and whose one effect writes `effect`. Returns the
/// narration's `recent` block and whether the PC holds a Belief on the
/// effect's slot.
fn wait_over_event(
    actor: fn(&World) -> u64,
    text: &str,
    effect: impl Fn(&World) -> (u64, &'static str, &'static str),
) -> (String, bool) {
    let mut w = World::new();
    let (brann, pip) = (w.brann, w.pip);
    let (subject, key, value) = effect(&w);
    w.commit(vec![LedgerOp::ScheduleEvent {
        text: text.into(),
        trigger: threshold_core::domain::Trigger::At(10),
        actors: vec![EntityRef::Id(actor(&w))],
        goal_id: None,
        cause_fact_id: None,
        effects: vec![threshold_core::domain::Effect::SetFact {
            slot: threshold_core::domain::Slot {
                subject: EntityRef::Id(subject),
                key: key.into(),
                value: value.into(),
            },
            text: format!("{key} = {value}"),
            seen_by: vec![],
        }],
        seen_by: vec![EntityRef::Id(brann), EntityRef::Id(pip)],
        public: false,
    }]);
    let mut wait = proposal(Verdict::Allowed);
    wait.action = ActionKind::Wait;
    let gm = RecordedGm::new(vec![json(&wait)], vec!["a".into()]);
    let campaign = w.campaign;
    let mut engine = make_engine(w.store, campaign, &gm);
    engine
        .play_turn("turn-1".into(), "I wait.", &mut no_chunks())
        .unwrap();
    let view = engine.store().snapshot(campaign).unwrap();
    let knows = threshold_core::world::pc_knowledge(&view)
        .iter()
        .any(|b| b.subject == subject && b.key == key);
    (prompts(&gm, CallKind::Narrate).pop().unwrap().recent, knows)
}

/// The PC perceives a World Event whose actor stands with the PC.
#[test]
fn witnessed_event_reaches_the_narrator() {
    let (recent, knows) = wait_over_witnessed_event(|w| w.orsel);
    assert!(
        recent.contains("- Orsel waves the courier over\n"),
        "{recent}"
    );
    assert!(
        recent.contains("Time this Turn takes: 10 minutes."),
        "{recent}"
    );
    assert!(knows);
}

/// An event whose actor is elsewhere stays offscreen, even with witnesses.
#[test]
fn event_of_an_absent_actor_stays_offscreen() {
    let (recent, knows) = wait_over_witnessed_event(|w| w.pip);
    assert!(!recent.contains("waves the courier"), "{recent}");
    assert!(!recent.contains("market"), "{recent}");
    assert!(!knows);
}

/// Event text is model text, like a reaction plan. A witnessed event whose
/// text quotes a GM-only fact never reaches the narrator; the PC still
/// witnesses what the event visibly does.
#[test]
fn witnessed_event_text_that_quotes_a_secret_is_dropped() {
    // `Ada holds = gate key` is GM only.
    let (recent, knows) = wait_over_event_with_text(|w| w.orsel, "Orsel eyes the gate key");
    assert!(!recent.contains("gate key"), "{recent}");
    assert!(!recent.contains("Orsel eyes"), "{recent}");
    assert!(knows);
}

/// Bug pass 4, B1: "Ludo counts his silver", seen by Agathe only, gave the
/// PC "Ludo's debt is now partially paid". The PC who witnesses an event
/// sees its text, never its hidden effects: a debt, a plan, a standing.
#[test]
fn a_witnessed_event_keeps_its_hidden_effects_from_the_pc() {
    for (key, value) in [
        ("debt", "partially paid"),
        ("plan", "flee"),
        ("standing", "summoned"),
    ] {
        let (recent, knows) = wait_over_event(
            |w| w.orsel,
            "Orsel counts his silver",
            |w| (w.orsel, key, value),
        );
        assert!(recent.contains("- Orsel counts his silver\n"), "{recent}");
        assert!(!recent.contains(value), "{key}: {recent}");
        assert!(!knows, "the PC learned Orsel's {key}");
    }
    // A slot of the PC that is no involuntary key (a feeling) stays hidden.
    let (_, knows) = wait_over_event(
        |w| w.orsel,
        "Orsel points at Ada",
        |w| (w.ada, "mood", "afraid"),
    );
    assert!(!knows);
}

/// What the PC does perceive of a witnessed event: an NPC that leaves or
/// comes, and what happens to the PC itself.
#[test]
fn a_witnessed_event_shows_a_move_and_what_happens_to_the_pc() {
    for (key, value) in [("injury", "bruised arm"), ("standing", "summoned")] {
        let (_, knows) = wait_over_event(|w| w.orsel, "Orsel shoves Ada", |w| (w.ada, key, value));
        assert!(knows, "{key}");
    }
    // Pip, at the market, comes to the gate square: the PC sees him come.
    let (_, knows) = wait_over_event(
        |w| w.orsel,
        "Orsel whistles",
        |w| (w.pip, "location", "gate square"),
    );
    assert!(knows);
    // Pip goes from the market to the mill: the PC sees neither place.
    let (_, knows) = wait_over_event(
        |w| w.orsel,
        "Orsel whistles",
        |w| (w.pip, "location", "mill"),
    );
    assert!(!knows);
}

/// The sibling path: an event a proposal schedules fires through the same
/// rule. Its hidden effect stays with the GM, and the stored fact is not
/// Persistent.
#[test]
fn a_scheduled_event_from_play_keeps_its_hidden_effect_from_the_pc() {
    let w0 = World::new();
    let mut first = proposal(Verdict::Allowed);
    first.schedule = Some(threshold_core::domain::EventDraft {
        text: "Orsel counts his silver".into(),
        in_minutes: 5,
        actor: e(w0.orsel),
        goal: None,
        effects: vec![slot(e(w0.orsel), "debt", "partially paid")],
        seen_by: vec![e(w0.brann)],
        public: false,
    });
    let mut wait = proposal(Verdict::Allowed);
    wait.action = ActionKind::Wait;
    let (mut engine, w, _gm) = setup(vec![json(&first), json(&wait)], vec!["a", "b"]);
    engine
        .play_turn("turn-1".into(), "I watch Orsel.", &mut no_chunks())
        .unwrap();
    engine
        .play_turn("turn-2".into(), "I wait an hour.", &mut no_chunks())
        .unwrap();
    let view = snapshot(&engine, &w);
    let debt = view.live_fact(w.orsel, "debt").expect("the event fired");
    assert_eq!(debt.slot.value, "partially paid");
    assert!(!debt.seen_by.contains(&w.ada), "{debt:?}");
    assert_ne!(debt.canon, threshold_core::domain::Canon::Persistent);
    assert!(
        !threshold_core::world::pc_knowledge(&view)
            .iter()
            .any(|b| b.subject == w.orsel && b.key == "debt")
    );
}

/// The sibling path of a public event (review of bug pass 4, blocker): an
/// event a proposal schedules with `public: true` fires through the same
/// rule, and the PC who witnesses it learns its effect.
#[test]
fn a_public_event_from_play_teaches_the_pc_its_effect() {
    let w0 = World::new();
    let mut first = proposal(Verdict::Allowed);
    first.schedule = Some(threshold_core::domain::EventDraft {
        text: "Orsel announces his plan".into(),
        in_minutes: 5,
        actor: e(w0.orsel),
        goal: None,
        effects: vec![slot(e(w0.orsel), "plan", "close the gate")],
        seen_by: vec![e(w0.brann)],
        public: true,
    });
    let mut wait = proposal(Verdict::Allowed);
    wait.action = ActionKind::Wait;
    let (mut engine, w, _gm) = setup(vec![json(&first), json(&wait)], vec!["a", "b"]);
    engine
        .play_turn("turn-1".into(), "I watch Orsel.", &mut no_chunks())
        .unwrap();
    engine
        .play_turn("turn-2".into(), "I wait an hour.", &mut no_chunks())
        .unwrap();
    let view = snapshot(&engine, &w);
    let plan = view.live_fact(w.orsel, "plan").expect("the event fired");
    assert!(plan.seen_by.contains(&w.ada), "{plan:?}");
    assert!(
        threshold_core::world::pc_knowledge(&view)
            .iter()
            .any(|b| b.subject == w.orsel && b.key == "plan")
    );
}

/// Shape 3.2: the decision rule reads the stored action and verdict.
#[test]
fn stored_action_reads_the_proposal_record() {
    let mut talk = proposal(Verdict::Allowed);
    talk.action = ActionKind::Talk;
    let (mut engine, _w, _gm) = setup(vec![json(&talk)], vec!["Brann nods."]);
    let turn = engine
        .play_turn("turn-1".into(), "I greet Brann.", &mut no_chunks())
        .unwrap()
        .turn_id;
    let stored = engine.store().turn(turn).unwrap().unwrap();
    assert_eq!(
        stored_action(&stored.record.proposal_json),
        Some((ActionKind::Talk, Verdict::Allowed))
    );
    assert_eq!(stored_action("{}"), None);
}

/// Design 11, "Turn time does not grow": the speech mark adds no model call.
/// A Turn asks for one proposal and one Narration; Begin for one Narration.
#[test]
fn turn_makes_the_same_model_calls() {
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, &adventure_seed(), 7).unwrap();
    let gm = RecordedGm::new(
        vec![json(&proposal(Verdict::Allowed))],
        vec![
            "Dusk falls on the gate square.".into(),
            "Orsel: \"You are late.\"".into(),
        ],
    );
    let mut engine = make_engine(store, campaign, &gm);
    engine.begin(&mut |_| {}).unwrap();
    assert_eq!(kinds(&gm), [CallKind::Narrate]);
    engine
        .play_turn("t1".into(), "I greet Orsel.", &mut |_| {})
        .unwrap();
    assert_eq!(
        kinds(&gm),
        [CallKind::Narrate, CallKind::Propose, CallKind::Narrate]
    );
}

// --- QA, 2026-11-10 -----------------------------------------------------------

fn steps(engine: &Engine, op_id: &str) -> Vec<(String, String)> {
    engine
        .store()
        .debug_log(op_id)
        .unwrap()
        .into_iter()
        .map(|e| (e.step, e.raw))
        .collect()
}

/// ADR 0007: the model names the destination in `move_to`, and the core
/// writes the PC's location. A PC location fact the model writes itself is
/// dropped, not rejected, and logged. The narrator is told where the PC is.
#[test]
fn move_to_moves_the_pc_and_a_model_location_fact_is_dropped() {
    let w0 = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Move;
    p.minutes = 10;
    p.move_to = Some(e(w0.gate));
    p.facts = vec![fact(
        slot(e(w0.ada), "location", "tavern"),
        Seen::Pc,
        vec![],
    )];
    let (mut engine, w, gm) = setup(vec![json(&p)], vec!["You walk to the gate."]);
    engine
        .play_turn("t1".into(), "I walk to the gate.", &mut no_chunks())
        .unwrap();
    assert_eq!(kinds(&gm), [CallKind::Propose, CallKind::Narrate]);
    let view = snapshot(&engine, &w);
    assert_eq!(
        view.live_fact(w.ada, "location").unwrap().slot.value,
        "gate"
    );
    assert!(!view.present().contains(&w.orsel));
    let log = steps(&engine, "t1");
    let settle = log
        .iter()
        .find(|(step, _)| step == "settle")
        .expect("a settle row");
    assert!(settle.1.contains("location fact"), "{}", settle.1);
    let narration = prompts(&gm, CallKind::Narrate).pop().unwrap();
    assert!(
        narration
            .recent
            .contains(&format!("{}Gate.", threshold_core::context::PC_PLACE_LINE)),
        "{}",
        narration.recent
    );
}

/// R4: a move with no known destination commits; the PC stays, and the
/// narrator is told that the move does not happen and where the PC still is.
#[test]
fn move_with_no_destination_commits_and_the_pc_stays() {
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Move;
    p.minutes = 10;
    let (mut engine, w, gm) = setup(vec![json(&p)], vec!["You do not find the way."]);
    let before = snapshot(&engine, &w)
        .live_fact(w.ada, "location")
        .unwrap()
        .id;
    engine
        .play_turn("t1".into(), "I go to the tavern.", &mut no_chunks())
        .unwrap();
    assert_eq!(kinds(&gm), [CallKind::Propose, CallKind::Narrate]);
    let view = snapshot(&engine, &w);
    assert_eq!(view.live_fact(w.ada, "location").unwrap().id, before);
    let narration = prompts(&gm, CallKind::Narrate).pop().unwrap();
    assert!(
        narration.recent.contains(&format!(
            "{}gate square.",
            threshold_core::context::NOT_MOVED_LINE
        )),
        "{}",
        narration.recent
    );
}

/// B4: a PC reaction, the PC in an event's seen_by, and an event whose
/// actor is the PC are removed, not rejected. The debug log records it.
#[test]
fn pc_reaction_and_pc_witness_are_removed_not_rejected() {
    let w0 = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Talk;
    p.reactions = vec![
        Reaction {
            npc: e(w0.ada),
            stance: Stance::Comply,
            plan: "agrees".into(),
            cause: None,
        },
        Reaction {
            npc: e(w0.brann),
            stance: Stance::Neutral,
            plan: "shrugs".into(),
            cause: None,
        },
    ];
    p.schedule = Some(threshold_core::domain::EventDraft {
        in_minutes: 60,
        actor: e(w0.brann),
        goal: None,
        text: "Brann fetches the smith".into(),
        effects: vec![],
        seen_by: vec![e(w0.ada)],
        public: false,
    });
    p.minutes = 2;
    let (mut engine, w, gm) = setup(vec![json(&p)], vec!["Brann shrugs."]);
    engine
        .play_turn("t1".into(), "I ask Brann for help.", &mut no_chunks())
        .unwrap();
    assert_eq!(kinds(&gm), [CallKind::Propose, CallKind::Narrate]);
    let view = snapshot(&engine, &w);
    let event = view
        .events_scheduled
        .iter()
        .find(|ev| ev.text == "Brann fetches the smith")
        .expect("the event stays");
    assert!(event.seen_by.is_empty());
    let log = steps(&engine, "t1");
    let settle = log
        .iter()
        .find(|(step, _)| step == "settle")
        .expect("a settle row");
    assert!(settle.1.contains("reaction of the PC"), "{}", settle.1);
    assert!(settle.1.contains("seen_by"), "{}", settle.1);
    let stored = engine.store().turn_by_campaign_op(w.campaign, &"t1".into());
    let record = stored.unwrap().unwrap().record.proposal_json;
    assert!(
        !record.contains(&format!("\"npc\":\"E{}\"", w.ada)),
        "{record}"
    );
}

#[test]
fn event_with_the_pc_as_actor_is_removed_not_rejected() {
    let w0 = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.schedule = Some(threshold_core::domain::EventDraft {
        in_minutes: 60,
        actor: e(w0.ada),
        goal: None,
        text: "Ada returns".into(),
        effects: vec![],
        seen_by: vec![],
        public: false,
    });
    let (mut engine, w, _gm) = setup(vec![json(&p)], vec!["Time passes."]);
    let before = snapshot(&engine, &w).events_scheduled.len();
    engine
        .play_turn("t1".into(), "I plan to come back.", &mut no_chunks())
        .unwrap();
    assert_eq!(snapshot(&engine, &w).events_scheduled.len(), before);
    assert!(steps(&engine, "t1").iter().any(|(s, _)| s == "settle"));
}

/// B2: the narrator gets the verdict of an impossible attempt, never the
/// blocker's GM-only text, on the first Narration and on `renarrate`.
#[test]
fn impossible_attempt_is_told_as_not_happening_without_the_blocker() {
    let w0 = World::new();
    let mut p = proposal(Verdict::Impossible);
    p.blockers = vec![f(w0.secret)];
    p.minutes = 1;
    let (mut engine, _w, gm) = setup(vec![json(&p)], vec!["You fail.", "You fail again."]);
    let outcome = engine
        .play_turn("t1".into(), "I fly over the gate.", &mut no_chunks())
        .unwrap();
    engine.renarrate(outcome.turn_id, &mut no_chunks()).unwrap();
    let narrations = prompts(&gm, CallKind::Narrate);
    assert_eq!(narrations.len(), 2);
    for prompt in &narrations {
        assert!(
            prompt
                .recent
                .contains(threshold_core::context::IMPOSSIBLE_LINE),
            "{}",
            prompt.recent
        );
        for field in fields(prompt) {
            for secret in ["gate key", "holds"] {
                assert!(!field.contains(secret), "{field}");
            }
        }
    }
    // An allowed Turn has no such line.
    let mut ok = proposal(Verdict::Allowed);
    ok.minutes = 1;
    gm.push_proposal(json(&ok));
    gm.push_narration("You wait.".into());
    engine
        .play_turn("t2".into(), "I wait a moment.", &mut no_chunks())
        .unwrap();
    let last = prompts(&gm, CallKind::Narrate).pop().unwrap();
    assert!(!last.recent.contains("does not happen"), "{}", last.recent);
}

/// ADR 0007: a contested act with no Check gets one repair note; when the
/// retry still has none, the core builds a default Check and the Turn
/// commits with a Roll. The narrator gets the result.
#[test]
fn contested_act_without_a_check_rolls_on_the_retry() {
    let w0 = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Act;
    p.contested = true;
    p.minutes = 5;
    p.reactions = vec![Reaction {
        npc: e(w0.brann),
        stance: Stance::Resist,
        plan: "pushes back".into(),
        cause: None,
    }];
    let (mut engine, w, gm) = setup(vec![json(&p), json(&p)], vec!["You strain."]);
    let outcome = engine
        .play_turn("t1".into(), "I arm-wrestle Brann.", &mut no_chunks())
        .unwrap();
    let roll = outcome.roll.expect("the default Check rolls");
    assert_eq!(roll.difficulty, 6, "the Ruleset's normal difficulty");
    let retry = &prompts(&gm, CallKind::Propose)[1];
    assert!(
        retry.recent.contains("contest: contested is true"),
        "{}",
        retry.recent
    );
    let log = steps(&engine, "t1");
    assert!(
        log.iter()
            .any(|(step, raw)| step == "settle" && raw.contains("default Check")),
        "{log:?}"
    );
    let narration = prompts(&gm, CallKind::Narrate).pop().unwrap();
    assert!(
        narration.recent.contains("- Check: the PC "),
        "{}",
        narration.recent
    );
    let stored = engine.store().turn_by_campaign_op(w.campaign, &"t1".into());
    assert!(stored.unwrap().is_some());
}

/// The recorded shape from the live run (cdefde8): the first attempt is
/// contested with verdict `check` but no Check. The verdict already says
/// "roll", so the core builds the default Check on that first attempt and
/// spends no retry. The facts apply on a win only.
#[test]
fn contested_check_verdict_without_a_check_rolls_on_the_first_attempt() {
    // The hidden world, where the PC sees Brann here: the default Check
    // opposes the NPC the Intent names (review M3).
    let w0 = support::hidden::HiddenWorld::new();
    let mut p = proposal(Verdict::Check);
    p.action = ActionKind::Wait;
    p.contested = true;
    p.reactions = vec![Reaction {
        npc: e(w0.brann),
        stance: Stance::Neutral,
        plan: "stares at the hands".into(),
        cause: None,
    }];
    let brann = w0.brann;
    let gm = RecordedGm::new(vec![json(&p)], vec!["You strain.".into()]);
    let (mut engine, _) = w0.engine(&gm);
    let outcome = engine
        .play_turn("t1".into(), "I arm-wrestle Brann.", &mut no_chunks())
        .unwrap();
    let roll = outcome.roll.expect("the default Check rolls");
    assert_eq!(roll.difficulty, 6, "the Ruleset's normal difficulty");
    assert_eq!(
        kinds(&gm),
        [CallKind::Propose, CallKind::Narrate],
        "no retry"
    );
    let log = steps(&engine, "t1");
    assert!(
        log.iter().any(|(step, raw)| step == "settle"
            && raw.contains(&format!("default Check against E{brann}"))),
        "{log:?}"
    );
    // A Check verdict without `contested` keeps the repair round: the core
    // builds a Check on the first attempt only for a contest.
    let mut plain = proposal(Verdict::Check);
    plain.action = ActionKind::Act;
    let (mut engine, _w, gm) = setup(vec![json(&plain), json(&plain)], vec!["You try."]);
    engine
        .play_turn("t1".into(), "I climb the wall.", &mut no_chunks())
        .unwrap();
    assert_eq!(kinds(&gm).len(), 3, "propose, retry, narrate");
}

/// The recorded shape from the live run (ADR 0007): the impossible act
/// cites the PC's own location as its blocker. The core drops it, opens no
/// obstacle, and a legal move in the next Turn commits.
#[test]
fn impossible_act_citing_the_pc_location_does_not_block_a_later_move() {
    let w0 = World::new();
    let location = w0.view.live_fact(w0.ada, "location").unwrap().id;
    let mut fly = proposal(Verdict::Impossible);
    fly.blockers = vec![f(location)];
    fly.minutes = 1;
    let mut walk = proposal(Verdict::Allowed);
    walk.action = ActionKind::Move;
    walk.move_to = Some(e(w0.gate));
    walk.minutes = 5;
    let (mut engine, w, gm) = setup(vec![json(&fly), json(&walk)], vec!["No.", "You go."]);
    let obstacles = snapshot(&engine, &w).obstacles_open.len();
    engine
        .play_turn("t1".into(), "I fly over the roofs.", &mut no_chunks())
        .unwrap();
    assert_eq!(snapshot(&engine, &w).obstacles_open.len(), obstacles);
    assert!(
        steps(&engine, "t1")
            .iter()
            .any(|(step, raw)| step == "settle" && raw.contains("never blocks"))
    );
    engine
        .play_turn("t2".into(), "I walk to the gate.", &mut no_chunks())
        .unwrap();
    assert_eq!(kinds(&gm).len(), 4, "no retry");
    let view = snapshot(&engine, &w);
    assert_eq!(
        view.live_fact(w.ada, "location").unwrap().slot.value,
        "gate"
    );
}

/// R6: a proposal the model stopped for a reason other than `stop` keeps
/// that reason in its debug row, so a cut answer shows its cause.
#[test]
fn a_cut_proposal_logs_why_the_model_stopped() {
    struct Cut(RecordedGm);
    impl Gm for Cut {
        fn propose(&self, prompt: &Prompt, schema: &serde_json::Value) -> Result<String, GmError> {
            self.0.propose(prompt, schema)
        }
        fn narrate(
            &self,
            prompt: &Prompt,
            on_chunk: &mut dyn FnMut(&str),
        ) -> Result<String, GmError> {
            self.0.narrate(prompt, on_chunk)
        }
        fn last_done_reason(&self) -> Option<String> {
            Some("length".into())
        }
    }
    let w = World::new();
    let gm = RecordedGm::new(
        vec![
            r#"{"action":"act","verdict":"allowed","factors":["#.into(),
            json(&proposal(Verdict::Allowed)),
        ],
        vec!["Done.".into()],
    );
    let mut engine = Engine::new(
        w.store,
        Box::new(Cut(gm)),
        Box::new(D10),
        vec![],
        w.campaign,
    );
    engine
        .play_turn("t1".into(), "I wait.", &mut no_chunks())
        .unwrap();
    let first = steps(&engine, "t1")
        .into_iter()
        .find(|(step, _)| step == "propose")
        .unwrap();
    assert!(
        first.1.ends_with("[generation stopped: length]"),
        "{}",
        first.1
    );
}

// --- Time after review (C9) ---------------------------------------------------

/// The minutes one Turn of `p` moves the clock, from the fixture's start.
fn minutes_of(p: &DecisionProposal) -> u64 {
    let (mut engine, w, _gm) = setup(vec![json(p)], vec!["It happens."]);
    let before = snapshot(&engine, &w).clock_min;
    engine
        .play_turn("t1".into(), "I do it.", &mut no_chunks())
        .unwrap();
    snapshot(&engine, &w).clock_min - before
}

/// The model leaves out `minutes` on most Turns. With 0, the core uses a
/// fixed default by action from the Ruleset module.
#[test]
fn a_turn_without_minutes_takes_the_default_of_its_action() {
    use threshold_core::rules::{ACT_DEFAULT_MIN, MOVE_DEFAULT_MIN, TALK_DEFAULT_MIN};
    assert_eq!(
        (TALK_DEFAULT_MIN, ACT_DEFAULT_MIN, MOVE_DEFAULT_MIN),
        (5, 10, 15)
    );
    for (action, minutes) in [
        (ActionKind::Talk, 5),
        (ActionKind::Act, 10),
        (ActionKind::Move, 15),
    ] {
        let mut p = proposal(Verdict::Allowed);
        p.action = action;
        p.minutes = 0;
        assert_eq!(minutes_of(&p), minutes, "{action:?}");
    }
    // A stated duration stays.
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Talk;
    p.minutes = 3;
    assert_eq!(minutes_of(&p), 3);
}

/// A contested `wait` is an act, never the engine-owned wait that runs the
/// clock forward to the next event.
#[test]
fn a_contested_wait_is_an_act() {
    let mut p = proposal(Verdict::Check);
    p.action = ActionKind::Wait;
    p.contested = true;
    p.minutes = 0;
    p.check = Some(check(
        Some(e(World::new().brann)),
        slot(e(World::new().brann), "mood", "beaten"),
        slot(e(World::new().brann), "mood", "smug"),
    ));
    let (mut engine, w, _gm) = setup(vec![json(&p)], vec!["You stare him down."]);
    let before = snapshot(&engine, &w).clock_min;
    engine
        .play_turn("t1".into(), "I outwait Brann.", &mut no_chunks())
        .unwrap();
    assert_eq!(
        snapshot(&engine, &w).clock_min - before,
        u64::from(threshold_core::rules::ACT_DEFAULT_MIN)
    );
    let stored = engine
        .store()
        .turn_by_campaign_op(w.campaign, &"t1".to_owned())
        .unwrap()
        .unwrap();
    assert_eq!(
        stored_action(&stored.record.proposal_json),
        Some((ActionKind::Act, Verdict::Check))
    );
}

/// Bug pass 3, M3: the QA arm-wrestle. The model sent action `move` with
/// `move_to: null`, `contested: true`, a Check with no `against`, and a
/// lose stake on the PC's injury. The narrator then got "The Check result:
/// lose", the raw line "Teodor Brandt: injury = bruised_wrist", "15
/// minutes pass." first, and "The move does not happen", and told the PC
/// sitting by the fire. Now: a contested act with no destination is an
/// act; the Check opposes the NPC who resists; the narrator gets the
/// opponent, the result and the stake as a sentence; the time is no
/// outcome bullet.
#[test]
fn a_contest_reaches_the_narrator_with_its_opponent_and_result() {
    // The hidden world: the PC sees Brann at the gate square.
    let w = support::hidden::HiddenWorld::new();
    let (ada, brann) = (w.ada, w.brann);
    let mut p = proposal(Verdict::Check);
    p.action = ActionKind::Move;
    p.contested = true;
    p.minutes = 15;
    p.check = Some(threshold_core::domain::CheckDraft {
        against: None,
        win: Some(slot(e(ada), "reputation", "stronger")),
        lose: Some(slot(e(ada), "injury", "bruised_wrist")),
        factors: vec![],
    });
    p.reactions = vec![Reaction {
        npc: e(brann),
        stance: Stance::Resist,
        plan: "laugh and try to win".into(),
        cause: None,
    }];
    let gm = RecordedGm::new(vec![json(&p)], vec!["Brann slams your hand down.".into()]);
    let (mut engine, campaign) = w.engine(&gm);
    let outcome = engine
        .play_turn(
            "t1".into(),
            "I walk over to Brann and challenge him to arm-wrestle.",
            &mut no_chunks(),
        )
        .unwrap();
    let roll = outcome.roll.expect("the Check rolls");
    let stored = engine.store().turn(outcome.turn_id).unwrap().unwrap();
    assert_eq!(
        stored_action(&stored.record.proposal_json),
        Some((ActionKind::Act, Verdict::Check))
    );
    let recent = prompts(&gm, CallKind::Narrate).pop().unwrap().recent;
    let (result, stake) = if roll.success {
        (
            "- Check against Brann: the PC wins.",
            "- Ada's reputation is now stronger.\n",
        )
    } else {
        (
            "- Check against Brann: the PC loses.",
            "- Ada is injured (bruised wrist).\n",
        )
    };
    assert!(recent.contains(result), "{recent}");
    assert!(recent.contains(stake), "{recent}");
    assert!(!recent.contains(" = "), "{recent}");
    assert!(!recent.contains("does not happen"), "{recent}");
    assert!(!recent.contains("minutes pass"), "{recent}");
    assert!(
        recent.contains("Time this Turn takes: 15 minutes."),
        "{recent}"
    );
    let view = engine.store().snapshot(campaign).unwrap();
    assert_eq!(
        view.live_fact(ada, "location").unwrap().slot.value,
        "gate square"
    );
}

/// Bug pass 3, M3: "I nod to Sefa and warm my hands at the fire" came back
/// as `wait` with no minutes and took 60 minutes. An Intent that asks for
/// no wait is an act: the act default when the model gives no minutes.
#[test]
fn a_wait_the_intent_does_not_ask_for_is_an_act() {
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Wait;
    let (mut engine, w, _) = setup(vec![json(&p), json(&p)], vec!["a", "b"]);
    let start = snapshot(&engine, &w).clock_min;
    let warm = engine
        .play_turn(
            "t1".into(),
            "I warm my hands at the fire.",
            &mut no_chunks(),
        )
        .unwrap();
    let stored = engine.store().turn(warm.turn_id).unwrap().unwrap();
    assert_eq!(
        stored_action(&stored.record.proposal_json).map(|(a, _)| a),
        Some(ActionKind::Act)
    );
    let after = snapshot(&engine, &w).clock_min;
    assert_eq!(
        after - start,
        u64::from(threshold_core::rules::ACT_DEFAULT_MIN)
    );
    engine
        .play_turn("t2".into(), "I wait.", &mut no_chunks())
        .unwrap();
    assert!(
        snapshot(&engine, &w).clock_min - after > u64::from(threshold_core::rules::ACT_DEFAULT_MIN)
    );
}

/// Review minor: `engine_time` ran before `settle`. A contested move whose
/// `move_to` `settle` drops (not a place the PC knows) stayed a move, and
/// the narrator heard "the move does not happen". Now `settle` runs first,
/// so the act is an act.
#[test]
fn a_contested_move_whose_destination_is_dropped_is_an_act() {
    let w0 = World::new();
    let mut p = proposal(Verdict::Check);
    p.action = ActionKind::Move;
    p.contested = true;
    p.move_to = Some(e(w0.orsel));
    p.check = Some(check(
        Some(e(w0.brann)),
        slot(e(w0.brann), "mood", "beaten"),
        slot(e(w0.brann), "mood", "smug"),
    ));
    let (mut engine, _w, gm) = setup(vec![json(&p)], vec!["You push past."]);
    let outcome = engine
        .play_turn("t1".into(), "I push past Brann.", &mut no_chunks())
        .unwrap();
    let stored = engine.store().turn(outcome.turn_id).unwrap().unwrap();
    assert_eq!(
        stored_action(&stored.record.proposal_json),
        Some((ActionKind::Act, Verdict::Check))
    );
    let recent = prompts(&gm, CallKind::Narrate).pop().unwrap().recent;
    assert!(!recent.contains("does not happen"), "{recent}");
    assert!(
        steps(&engine, "t1")
            .iter()
            .any(|(step, raw)| step == "settle" && raw.contains("move_to")),
    );
}
