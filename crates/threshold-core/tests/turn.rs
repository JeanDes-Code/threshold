mod support;

use std::panic::{AssertUnwindSafe, catch_unwind};

use support::*;
use threshold_core::{
    CallKind, Gm, GmError, Prompt, RecordedGm,
    checks::CheckKind,
    domain::{
        ActionKind, DecisionProposal, EntityRef, Knowing, LedgerOp, Reaction, SeedEntity, SeedFact,
        Seen, Stance, Verdict,
    },
    rules::{D10, Ruleset, turn_seed},
    store::{Store, WorldView},
    turn::{Engine, MAX_INTENT_CHARS, TurnError, create_campaign},
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
    clarified.check.as_mut().unwrap().lose = slot(e(w0.ada), "injury", "bruised hand");
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
    assert_eq!(stored_proposal, p);
}

/// Ruling 4: at setup and on arrival, the PC sees who openly stands there,
/// never a hidden entity.
#[test]
fn arrival_shows_open_npcs_and_keeps_hidden_out() {
    let mut seed = adventure_seed();
    seed.entities.push(SeedEntity {
        name: "Vell".into(),
        kind: threshold_core::domain::EntityKind::Npc,
        adult: Some(true),
        hidden: true,
    });
    seed.decided.push(SeedFact {
        subject: "Vell".into(),
        key: "location".into(),
        value: "market".into(),
        text: "Vell lurks in the market.".into(),
    });
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, &seed, 7).unwrap();
    let view = store.snapshot(campaign).unwrap();
    let id = |name: &str| view.entities.iter().find(|e| e.name == name).unwrap().id;
    let ada = id("Ada");

    let look = proposal(Verdict::Allowed);
    let mut walk = proposal(Verdict::Allowed);
    walk.action = ActionKind::Move;
    walk.minutes = 10;
    walk.facts = vec![fact(slot(e(ada), "location", "market"), Seen::Pc, vec![])];
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
    assert_eq!((pip_known.learned_at, pip_known.arrives_at), (10, 10));
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
    walk.facts = vec![fact(slot(e(w.ada), "location", "market"), Seen::Pc, vec![])];
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
