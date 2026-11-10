//! Begin: the opening Narration of the start state, stored as a Turn of kind
//! `Opening` (data shape 3.7).

mod support;

use support::*;
use threshold_core::{
    CallKind, Prompt, RecordedGm,
    context::opening_prompt,
    domain::{AdventureSeed, Canon, EntityRef, FactSource, LedgerOp, Slot, StartSeed, Trigger},
    rules::D10,
    store::{Store, TurnKind, TurnRecord},
    turn::{BEGIN_OP_ID, Engine, TurnError, create_campaign},
};

const OPENING: &str = "Dusk falls on the gate square.";
const SITUATION: &str = "The guards close the gate for the night.";
const WHEN: &str = "Late autumn, at dusk.";

fn seed_with_start() -> AdventureSeed {
    let mut seed = adventure_seed();
    seed.start = StartSeed {
        place: "gate square".into(),
        present: vec!["Orsel".into(), "Brann".into()],
        situation: SITUATION.into(),
        when: WHEN.into(),
    };
    seed
}

fn engine_on(store: Store, campaign: u64, gm: &RecordedGm) -> Engine {
    Engine::new(store, Box::new(gm.clone()), Box::new(D10), vec![], campaign)
}

/// A new Campaign: only its Setup Turn, no Play Turn yet.
fn fresh(seed: &AdventureSeed, gm: &RecordedGm) -> (Engine, u64) {
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, seed, 7).unwrap();
    (engine_on(store, campaign, gm), campaign)
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

fn allowed() -> String {
    serde_json::to_string(&proposal(threshold_core::domain::Verdict::Allowed)).unwrap()
}

#[test]
fn begin_commits_an_opening_turn_with_no_op() {
    let gm = RecordedGm::new(vec![], vec![OPENING.into()]);
    let (mut engine, campaign) = fresh(&seed_with_start(), &gm);
    let before = engine.store().snapshot(campaign).unwrap();
    let mut chunks = String::new();
    let outcome = engine.begin(&mut |c| chunks.push_str(c)).unwrap();
    assert_eq!(outcome.narration, OPENING);
    assert_eq!(chunks, OPENING);
    assert_eq!(outcome.roll, None);

    let turn = engine.store().turn(outcome.turn_id).unwrap().unwrap();
    assert_eq!(turn.record.kind, TurnKind::Opening);
    assert_eq!(turn.record.op_id, BEGIN_OP_ID);
    assert_eq!(turn.record.intent, "");
    assert_eq!(turn.narration.as_deref(), Some(OPENING));
    assert!(
        engine.store().turn_ops(outcome.turn_id).unwrap().is_empty(),
        "an Opening Turn commits no op"
    );
    let after = engine.store().snapshot(campaign).unwrap();
    assert_eq!(after.turn_seq, before.turn_seq + 1, "exactly one Turn row");
    assert_eq!(after.clock_min, before.clock_min);
    assert_eq!(turn.record.clock_before, before.clock_min);
    assert_eq!(turn.record.clock_after, before.clock_min);
    assert_eq!(after.live_facts, before.live_facts);
    assert_eq!(kinds(&gm), [CallKind::Narrate]);

    // The opening prompt: the narration rules, then the opening lines; its
    // last block holds the situation and the time.
    let prompt = &prompts(&gm, CallKind::Narrate)[0];
    assert!(prompt.system.contains("Never decide the PC's actions"));
    assert!(prompt.system.contains("Do not move time forward"));
    assert!(
        prompt.scene.contains("Place: gate square"),
        "{}",
        prompt.scene
    );
    assert!(prompt.recent.contains(SITUATION), "{}", prompt.recent);
    assert!(prompt.recent.contains(WHEN), "{}", prompt.recent);
    let steps: Vec<String> = engine
        .store()
        .debug_log(BEGIN_OP_ID)
        .unwrap()
        .into_iter()
        .map(|e| e.step)
        .collect();
    assert_eq!(steps, ["commit", "narrate"]);
}

#[test]
fn second_begin_returns_the_stored_turn() {
    let gm = RecordedGm::new(vec![], vec![OPENING.into()]);
    let (mut engine, campaign) = fresh(&seed_with_start(), &gm);
    let first = engine.begin(&mut |_| {}).unwrap();
    let before = engine.store().snapshot(campaign).unwrap();
    let second = engine.begin(&mut |_| {}).unwrap();
    assert_eq!(second.turn_id, first.turn_id);
    assert_eq!(second.narration, OPENING);
    assert_eq!(engine.store().snapshot(campaign).unwrap(), before);
    assert_eq!(kinds(&gm), [CallKind::Narrate], "one narration call");
}

#[test]
fn begin_narrates_again_when_the_narration_is_missing() {
    let gm = RecordedGm::new(vec![], vec![]);
    let (mut engine, campaign) = fresh(&seed_with_start(), &gm);
    let before = engine.store().snapshot(campaign).unwrap();
    let first = engine.begin(&mut |_| {});
    let Err(TurnError::NarrationFailed { turn_id, .. }) = first else {
        panic!("expected NarrationFailed, got {first:?}");
    };
    let stored = engine.store().turn(turn_id).unwrap().unwrap();
    assert_eq!(stored.record.kind, TurnKind::Opening);
    assert_eq!(stored.narration, None);

    gm.push_narration(OPENING.into());
    let mut chunks = String::new();
    let again = engine.begin(&mut |c| chunks.push_str(c)).unwrap();
    assert_eq!(again.turn_id, turn_id);
    assert_eq!(again.narration, OPENING);
    assert_eq!(chunks, OPENING);
    let stored = engine.store().turn(turn_id).unwrap().unwrap();
    assert_eq!(stored.narration.as_deref(), Some(OPENING));
    let after = engine.store().snapshot(campaign).unwrap();
    assert_eq!(after.turn_seq, before.turn_seq + 1, "exactly one Turn row");
    assert_eq!(kinds(&gm), [CallKind::Narrate, CallKind::Narrate]);
    // The second call narrates the opening again, not a Play Turn.
    let retry = &prompts(&gm, CallKind::Narrate)[1];
    assert!(retry.recent.contains(SITUATION), "{}", retry.recent);
    assert!(!retry.recent.contains("Player intent"), "{}", retry.recent);
}

#[test]
fn begin_fails_after_a_play_turn() {
    let gm = RecordedGm::new(vec![allowed()], vec!["You wait.".into()]);
    let (mut engine, campaign) = fresh(&seed_with_start(), &gm);
    engine
        .play_turn("turn-1".into(), "I wait.", &mut |_| {})
        .unwrap();
    let before = engine.store().snapshot(campaign).unwrap();
    let result = engine.begin(&mut |_| {});
    assert!(matches!(result, Err(TurnError::PlayStarted)), "{result:?}");
    assert_eq!(engine.store().snapshot(campaign).unwrap(), before);
    assert_eq!(gm.calls().len(), 2, "no model call for a refused Begin");
}

#[test]
fn opening_prompt_has_no_hidden_fact() {
    let mut w = World::new();
    let (ada, orsel) = (w.ada, w.orsel);
    w.commit(vec![
        LedgerOp::AddFact {
            slot: Slot {
                subject: EntityRef::Id(orsel),
                key: "skims".into(),
                value: "grain tithe".into(),
            },
            text: "Orsel skims the grain tithe".into(),
            canon: Canon::Established,
            source: FactSource::Proposal,
            seen_by: vec![],
            causes: vec![],
        },
        LedgerOp::AddFact {
            slot: Slot {
                subject: EntityRef::Id(orsel),
                key: "mood".into(),
                value: "annoyed".into(),
            },
            text: "Orsel looks annoyed.".into(),
            canon: Canon::Established,
            source: FactSource::Proposal,
            seen_by: vec![EntityRef::Id(ada), EntityRef::Id(orsel)],
            causes: vec![],
        },
    ]);
    let mut view = w.view.clone();
    view.adventure_seed = Some(seed_with_start());

    let prompt = opening_prompt(&view, &[]);
    for field in fields(&prompt) {
        assert!(!field.contains("Orsel skims the grain tithe"), "{field}");
        assert!(!field.contains("grain tithe"), "{field}");
        assert!(!field.contains("skims"), "{field}");
        // The fixture's other GM-only fact.
        assert!(!field.contains("gate key"), "{field}");
        // Goals and scheduled World Events stay out too.
        assert!(!field.contains("rings the bell"), "{field}");
        assert!(!field.contains("Keep Ada out of town"), "{field}");
        // No fact by id, no handle.
        assert!(!field.contains(&f(w.secret).to_string()), "{field}");
        assert!(!field.contains(&e(orsel).to_string()), "{field}");
    }
    assert!(prompt.recent.contains(SITUATION));
    assert!(prompt.recent.contains(WHEN));
}

#[test]
fn begin_works_with_an_empty_start() {
    let gm = RecordedGm::new(vec![], vec![OPENING.into()]);
    let (mut engine, _) = fresh(&adventure_seed(), &gm);
    let outcome = engine.begin(&mut |_| {}).unwrap();
    assert_eq!(outcome.narration, OPENING);
    let prompt = &prompts(&gm, CallKind::Narrate)[0];
    assert!(
        prompt.scene.contains("Place: gate square"),
        "{}",
        prompt.scene
    );
    assert!(!prompt.recent.contains("Situation"), "{}", prompt.recent);
    assert!(!prompt.recent.contains("When"), "{}", prompt.recent);
}

#[test]
fn begin_moves_no_clock_and_fires_no_event() {
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, &seed_with_start(), 7).unwrap();
    let view = store.snapshot(campaign).unwrap();
    let orsel = view.entities.iter().find(|e| e.name == "Orsel").unwrap().id;
    // A World Event already due: only a Turn that moves the clock fires it.
    store
        .commit(
            TurnRecord {
                campaign_id: campaign,
                op_id: "world".into(),
                kind: TurnKind::Setup,
                intent: String::new(),
                proposal_json: "{}".into(),
                clock_before: 0,
                clock_after: 0,
            },
            vec![LedgerOp::ScheduleEvent {
                text: "Orsel rings the bell".into(),
                trigger: Trigger::At(0),
                actors: vec![EntityRef::Id(orsel)],
                goal_id: None,
                cause_fact_id: None,
                effects: vec![],
                seen_by: vec![],
            }],
        )
        .unwrap();
    let gm = RecordedGm::new(vec![], vec![OPENING.into()]);
    let mut engine = engine_on(store, campaign, &gm);
    let before = engine.store().snapshot(campaign).unwrap();
    engine.begin(&mut |_| {}).unwrap();
    let after = engine.store().snapshot(campaign).unwrap();
    assert_eq!(after.clock_min, before.clock_min);
    assert_eq!(after.events_scheduled, before.events_scheduled);
    assert_eq!(after.events_scheduled.len(), 1);
    let prompt = &prompts(&gm, CallKind::Narrate)[0];
    for field in fields(prompt) {
        assert!(!field.contains("rings the bell"), "{field}");
    }
}

#[test]
fn first_play_turn_reads_the_opening_narration() {
    let gm = RecordedGm::new(vec![allowed()], vec![OPENING.into(), "You wait.".into()]);
    let (mut engine, campaign) = fresh(&seed_with_start(), &gm);
    let opening = engine.begin(&mut |_| {}).unwrap();
    let view = engine.store().snapshot(campaign).unwrap();
    assert_eq!(
        view.recent_turns
            .iter()
            .map(|t| (t.id, t.record.kind))
            .collect::<Vec<_>>(),
        [(opening.turn_id, TurnKind::Opening)]
    );
    engine
        .play_turn("turn-1".into(), "I wait.", &mut |_| {})
        .unwrap();
    for prompt in [
        &prompts(&gm, CallKind::Propose)[0],
        &prompts(&gm, CallKind::Narrate)[1],
    ] {
        assert!(prompt.recent.contains(OPENING), "{}", prompt.recent);
        // The opening has no Player line: the PC did nothing yet.
        assert!(!prompt.recent.contains("Player: \n"), "{}", prompt.recent);
    }
}

#[test]
fn correct_last_after_begin_is_refused() {
    let gm = RecordedGm::new(vec![allowed()], vec![OPENING.into()]);
    let (mut engine, campaign) = fresh(&seed_with_start(), &gm);
    engine.begin(&mut |_| {}).unwrap();
    let before = engine.store().snapshot(campaign).unwrap();
    let result = engine.correct_last("turn-1".into(), "I sit.", "misread", &mut |_| {});
    assert!(
        matches!(result, Err(TurnError::NotCorrectable(_))),
        "{result:?}"
    );
    assert_eq!(engine.store().snapshot(campaign).unwrap(), before);
    assert_eq!(kinds(&gm), [CallKind::Narrate]);
}
