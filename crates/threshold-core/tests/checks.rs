mod support;

use support::*;
use threshold_core::{
    HandleContext,
    checks::{
        CheckKind, PC_INVOLUNTARY_KEYS, Rejection, STANCE_CEILINGS, ceiling, settle, to_ops,
        validate,
    },
    domain::{
        Cancel, Canon, Cause, EventDraft, Handle, ItemStatus, Knowing, LedgerOp, Seen, Stance,
        Verdict,
    },
    proposal_schema_for,
};

fn rejected<T: std::fmt::Debug>(
    result: Result<T, Vec<Rejection>>,
    kind: CheckKind,
) -> Vec<Rejection> {
    let errs = result.expect_err("proposal should be rejected");
    assert!(
        errs.iter().any(|r| r.check == kind),
        "expected {kind:?}, got {errs:?}"
    );
    errs
}

fn ok<T>(result: Result<T, Vec<Rejection>>) {
    if let Err(errs) = result {
        panic!("expected Ok, got {errs:?}");
    }
}

/// Runs `to_ops` and commits the ops through the real store.
fn apply(
    w: &mut World,
    p: &threshold_core::domain::DecisionProposal,
    roll: Option<threshold_core::rules::RollResult>,
) -> Vec<threshold_core::domain::Reaction> {
    let valid = validate(p, &w.view).expect("apply takes a valid proposal");
    let resolved = to_ops(valid, roll.as_ref(), &w.view);
    w.commit(resolved.ops);
    resolved.reactions
}

// --- Coincidence -----------------------------------------------------------

#[test]
fn coincidence_trap_rejected() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.gate), "locked", "no"), Seen::Pc, vec![])];
    let errs = rejected(validate(&p, &w.view), CheckKind::Coincidence);
    assert_eq!(errs[0].handle, Some(f(w.gate_locked)));
}

#[test]
fn coincidence_via_win_stake_ok() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Check);
    p.blockers = vec![f(w.gate_locked)];
    p.check = Some(check(
        None,
        slot(e(w.gate), "locked", "no"),
        slot(e(w.ada), "fatigue", "winded"),
    ));
    ok(validate(&p, &w.view));

    apply(&mut w, &p, Some(roll(true)));
    assert_eq!(w.view.live_fact(w.gate, "locked").unwrap().slot.value, "no");
    assert!(
        w.view.obstacles_open.is_empty(),
        "win stake clears the obstacle"
    );
}

#[test]
fn coincidence_with_valid_cause_ok() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(w.gate), "locked", "no"),
        Seen::Pc,
        vec![f(w.secret)],
    )];
    ok(validate(&p, &w.view));
}

#[test]
fn coincidence_on_blocker_declared_this_turn_rejected() {
    let mut w = World::new();
    let open = w.obstacle;
    w.commit(vec![LedgerOp::ClearObstacle { obstacle_id: open }]);
    let mut p = proposal(Verdict::Impossible);
    p.blockers = vec![f(w.gate_locked)];
    p.facts = vec![fact(slot(e(w.gate), "locked", "no"), Seen::Pc, vec![])];
    rejected(validate(&p, &w.view), CheckKind::Coincidence);
}

#[test]
fn cause_created_this_turn_rejected() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    // The first fact would get the next id; the second cites it.
    p.facts = vec![
        fact(slot(e(w.orsel), "mood", "sleepy"), Seen::Pc, vec![]),
        fact(
            slot(e(w.gate), "locked", "no"),
            Seen::Pc,
            vec![f(w.next_fact_id())],
        ),
    ];
    let errs = validate(&p, &w.view).expect_err("a cause from this Turn is never valid");
    assert!(
        errs.iter()
            .any(|r| r.check == CheckKind::References && r.handle == Some(f(w.next_fact_id()))),
        "{errs:?}"
    );
}

#[test]
fn possible_fact_is_not_a_valid_cause() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(w.gate), "locked", "no"),
        Seen::Pc,
        vec![f(w.rumor)],
    )];
    rejected(validate(&p, &w.view), CheckKind::Coincidence);
}

// --- NPC folds -------------------------------------------------------------

#[test]
fn stance_ceilings_follow_the_data_shape() {
    assert_eq!(STANCE_CEILINGS[0].1, Stance::Resist);
    let w = World::new();
    assert_eq!(ceiling(&w.view, w.orsel), Stance::Resist); // attitude -4 and goal
    assert_eq!(ceiling(&w.view, w.brann), Stance::Comply); // attitude 0
    let mut w = World::new();
    w.commit(vec![LedgerOp::ShiftAttitude {
        from: w.brann.into(),
        to: w.ada.into(),
        delta: 3,
    }]);
    assert_eq!(ceiling(&w.view, w.brann), Stance::Help);
    w.commit(vec![LedgerOp::ShiftAttitude {
        from: w.brann.into(),
        to: w.ada.into(),
        delta: -4,
    }]);
    assert_eq!(ceiling(&w.view, w.brann), Stance::Neutral); // -1
    w.commit(vec![LedgerOp::ShiftAttitude {
        from: w.orsel.into(),
        to: w.ada.into(),
        delta: 6,
    }]);
    assert_eq!(
        ceiling(&w.view, w.orsel),
        Stance::Resist,
        "goal caps at Resist"
    );
    w.goal_done();
    assert_eq!(ceiling(&w.view, w.orsel), Stance::Comply); // +2, goal done
}

#[test]
fn npc_fold_trap_rejected() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.reactions = vec![reaction(e(w.orsel), Stance::Comply, None)];
    let errs = rejected(validate(&p, &w.view), CheckKind::NpcFolds);
    assert_eq!(errs[0].handle, Some(e(w.orsel)));

    p.reactions = vec![reaction(e(w.orsel), Stance::Resist, None)];
    ok(validate(&p, &w.view));
}

#[test]
fn npc_fold_with_check_clamped_on_failure() {
    let mut p = proposal(Verdict::Check);
    let w = World::new();
    p.check = Some(check(
        Some(e(w.orsel)),
        slot(e(w.orsel), "convinced", "yes"),
        slot(e(w.ada), "fatigue", "winded"),
    ));
    p.reactions = vec![reaction(e(w.orsel), Stance::Comply, Some(Cause::Check))];
    ok(validate(&p, &w.view));

    let mut failed = World::new();
    let reactions = apply(&mut failed, &p, Some(roll(false)));
    assert_eq!(reactions[0].stance, Stance::Resist);
    assert_eq!(
        failed
            .view
            .live_fact(failed.ada, "fatigue")
            .unwrap()
            .slot
            .value,
        "winded"
    );

    let mut won = World::new();
    let reactions = apply(&mut won, &p, Some(roll(true)));
    assert_eq!(reactions[0].stance, Stance::Comply);
}

#[test]
fn npc_fold_check_against_someone_else_rejected() {
    let w = World::new();
    let mut p = proposal(Verdict::Check);
    p.check = Some(check(
        Some(e(w.brann)),
        slot(e(w.brann), "convinced", "yes"),
        slot(e(w.ada), "fatigue", "winded"),
    ));
    p.reactions = vec![reaction(e(w.orsel), Stance::Comply, Some(Cause::Check))];
    rejected(validate(&p, &w.view), CheckKind::NpcFolds);
}

#[test]
fn npc_fold_with_held_fact_ok_and_unheld_fact_rejected() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.reactions = vec![reaction(
        e(w.orsel),
        Stance::Comply,
        Some(Cause::Fact(f(w.debt))),
    )];
    ok(validate(&p, &w.view));

    p.reactions = vec![reaction(
        e(w.orsel),
        Stance::Comply,
        Some(Cause::Fact(f(w.secret))),
    )];
    rejected(validate(&p, &w.view), CheckKind::NpcFolds);
}

// --- Fact bends ------------------------------------------------------------

#[test]
fn fact_bend_rejected_even_with_cause() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(w.gate), "material", "wood"),
        Seen::Pc,
        vec![f(w.debt)],
    )];
    let errs = rejected(validate(&p, &w.view), CheckKind::FactBends);
    assert_eq!(errs[0].handle, Some(f(w.gate_material)));
}

#[test]
fn restating_a_persistent_fact_is_not_a_bend() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.gate), "material", "Iron"), Seen::Pc, vec![])];
    ok(validate(&p, &w.view));
}

// --- Tension evaporates ----------------------------------------------------

#[test]
fn cancel_without_cause_rejected() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.cancel = vec![Cancel {
        event: Handle::WorldEvent(w.bell),
        because: f(w.rumor),
    }];
    let errs = rejected(validate(&p, &w.view), CheckKind::TensionEvaporates);
    assert_eq!(errs[0].handle, Some(Handle::WorldEvent(w.bell)));

    p.cancel[0].because = f(w.debt);
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    assert!(w.view.events_scheduled.is_empty());
}

#[test]
fn actor_death_counts_as_cancel() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.orsel), "alive", "no"), Seen::Pc, vec![])];
    rejected(validate(&p, &w.view), CheckKind::TensionEvaporates);

    // Orsel's location is a state key, so it is not Persistent: only the
    // actor-cancel rule can reject this write.
    p.facts = vec![fact(
        slot(e(w.orsel), "location", "tavern"),
        Seen::Gm,
        vec![],
    )];
    let errs = rejected(validate(&p, &w.view), CheckKind::TensionEvaporates);
    assert!(
        errs.iter().all(|r| r.check != CheckKind::FactBends),
        "{errs:?}"
    );

    p.facts = vec![fact(
        slot(e(w.orsel), "alive", "no"),
        Seen::Pc,
        vec![f(w.debt)],
    )];
    ok(validate(&p, &w.view));
}

#[test]
fn write_to_when_slot_counts_as_cancel() {
    let mut w = World::new();
    let gate = w.gate;
    w.commit(vec![LedgerOp::ScheduleEvent {
        text: "The guard opens up".into(),
        trigger: threshold_core::domain::Trigger::When(threshold_core::domain::Slot {
            subject: gate.into(),
            key: "bell".into(),
            value: "rung".into(),
        }),
        actors: vec![],
        goal_id: None,
        cause_fact_id: None,
        effects: vec![],
        seen_by: vec![],
    }]);
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.gate), "bell", "broken"), Seen::Pc, vec![])];
    rejected(validate(&p, &w.view), CheckKind::TensionEvaporates);
}

type Edit = dyn Fn(&mut threshold_core::domain::DecisionProposal);

// --- References ------------------------------------------------------------

#[test]
fn bad_handles_rejected() {
    let w = World::new();
    let cases: Vec<Box<Edit>> = vec![
        Box::new(|p| p.reactions = vec![reaction(Handle::Entity(999), Stance::Neutral, None)]),
        Box::new(move |p| p.reactions = vec![reaction(f(w.debt), Stance::Neutral, None)]),
        Box::new(|p| p.new_npcs = vec![new_npc(1, "Kett", 34), new_npc(1, "Lio", 34)]),
        Box::new(|p| p.reactions = vec![reaction(Handle::NewNpc(2), Stance::Neutral, None)]),
        Box::new(|p| p.new_npcs = vec![new_npc(300, "Kett", 34)]),
        Box::new(move |p| p.blockers = vec![e(w.gate)]),
        Box::new(move |p| {
            p.cancel = vec![Cancel {
                event: Handle::WorldEvent(999),
                because: f(w.debt),
            }]
        }),
        Box::new(move |p| {
            p.schedule = Some(EventDraft {
                in_minutes: 5,
                seen_by: vec![],
                actor: e(w.orsel),
                goal: Some(Handle::Goal(999)),
                text: "x".into(),
                effects: vec![],
            })
        }),
        Box::new(move |p| {
            p.facts = vec![
                fact(slot(e(w.orsel), "mood", "a"), Seen::Gm, vec![]),
                fact(slot(e(w.orsel), "mood", "b"), Seen::Gm, vec![]),
                fact(slot(e(w.orsel), "hat", "c"), Seen::Gm, vec![]),
            ]
        }),
    ];
    for (i, case) in cases.iter().enumerate() {
        let mut p = proposal(Verdict::Allowed);
        case(&mut p);
        let errs = validate(&p, &w.view).expect_err(&format!("case {i} should fail"));
        assert!(
            errs.iter().all(|r| r.check == CheckKind::References),
            "case {i}: {errs:?}"
        );
    }
}

#[test]
fn two_writes_to_one_slot_rejected() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![
        fact(slot(e(w.orsel), "mood", "calm"), Seen::Gm, vec![]),
        fact(slot(e(w.orsel), "Mood", "angry"), Seen::Gm, vec![]),
    ];
    rejected(validate(&p, &w.view), CheckKind::References);
}

#[test]
fn event_effects_writing_one_slot_twice_rejected() {
    let w = World::new();
    let draft = |second_key: &str| EventDraft {
        in_minutes: 5,
        seen_by: vec![],
        actor: e(w.orsel),
        goal: None,
        text: "Orsel paces".into(),
        effects: vec![
            slot(e(w.orsel), "mood", "calm"),
            slot(e(w.orsel), second_key, "angry"),
        ],
    };
    let mut p = proposal(Verdict::Allowed);
    p.schedule = Some(draft("Mood"));
    let errs = rejected(validate(&p, &w.view), CheckKind::References);
    assert!(
        errs.iter()
            .any(|r| r.check == CheckKind::References && r.reason.contains("schedule.effects"))
    );

    p.schedule = Some(draft("fatigue"));
    if let Err(errs) = validate(&p, &w.view) {
        assert!(
            !errs.iter().any(|r| r.check == CheckKind::References),
            "{errs:?}"
        );
    }
}

#[test]
fn shape_mismatch_rejected() {
    let w = World::new();
    let p = proposal(Verdict::Check);
    rejected(validate(&p, &w.view), CheckKind::Schema);
}

// --- Agency ----------------------------------------------------------------

#[test]
fn pc_voluntary_fact_rejected() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.ada), "intends", "flee"), Seen::Pc, vec![])];
    rejected(validate(&p, &w.view), CheckKind::Agency);

    assert!(PC_INVOLUNTARY_KEYS.contains(&"injury"));
    p.facts = vec![fact(slot(e(w.ada), "Injury", "cut hand"), Seen::Pc, vec![])];
    ok(validate(&p, &w.view));
}

#[test]
fn pc_cannot_react_act_or_speak() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.reactions = vec![reaction(e(w.ada), Stance::Neutral, None)];
    rejected(validate(&p, &w.view), CheckKind::Agency);

    let mut p = proposal(Verdict::Allowed);
    p.schedule = Some(EventDraft {
        in_minutes: 5,
        seen_by: vec![],
        actor: e(w.ada),
        goal: None,
        text: "Ada leaves".into(),
        effects: vec![],
    });
    rejected(validate(&p, &w.view), CheckKind::Agency);

    let mut p = proposal(Verdict::Allowed);
    let mut said = fact(slot(e(w.orsel), "guilty", "yes"), Seen::Present, vec![]);
    said.said_by = Some(e(w.ada));
    p.facts = vec![said];
    rejected(validate(&p, &w.view), CheckKind::Agency);
}

// --- Content Boundary ------------------------------------------------------

#[test]
fn intimate_with_minor_rejected() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.intimate = true;
    p.reactions = vec![reaction(e(w.pip), Stance::Neutral, None)];
    rejected(validate(&p, &w.view), CheckKind::ContentBoundary);

    // Pip is 17: one year short of `ADULT_AGE`.
    assert_eq!(
        w.view.entities.iter().find(|e| e.id == w.pip).unwrap().age,
        Some(17)
    );

    let mut p = proposal(Verdict::Allowed);
    p.intimate = true;
    p.new_npcs = vec![new_npc(1, "Kett", 17)];
    rejected(validate(&p, &w.view), CheckKind::ContentBoundary);

    // A minor who is merely present also blocks an intimate scene.
    let mut w = World::new();
    let pip_location = w.view.live_fact(w.pip, "location").unwrap().id;
    let pip = w.pip;
    w.commit(vec![LedgerOp::SupersedeFact {
        old: pip_location,
        new: threshold_core::domain::FactDraft {
            slot: threshold_core::domain::Slot {
                subject: pip.into(),
                key: "location".into(),
                value: "Gate Square".into(),
            },
            text: "Pip came to the gate".into(),
            canon: Canon::Established,
            source: threshold_core::domain::FactSource::Correction,
            seen_by: vec![],
            causes: vec![],
        },
    }]);
    let mut p = proposal(Verdict::Allowed);
    p.intimate = true;
    p.reactions = vec![reaction(e(w.brann), Stance::Neutral, None)];
    let errs = rejected(validate(&p, &w.view), CheckKind::ContentBoundary);
    assert_eq!(errs[0].handle, Some(e(w.pip)));
}

#[test]
fn intimate_with_adults_ok() {
    let w = World::new();
    // Brann is 18, exactly `ADULT_AGE`; so is the new NPC.
    assert_eq!(
        w.view
            .entities
            .iter()
            .find(|e| e.id == w.brann)
            .unwrap()
            .age,
        Some(18)
    );
    let mut p = proposal(Verdict::Allowed);
    p.intimate = true;
    p.reactions = vec![reaction(e(w.brann), Stance::Comply, None)];
    p.new_npcs = vec![new_npc(1, "Kett", 18)];
    ok(validate(&p, &w.view));
}

// --- to_ops ----------------------------------------------------------------

#[test]
fn new_npc_commits_with_voice_card_location_and_witnessed_fact() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.new_npcs = vec![new_npc(1, "Kett", 34)];
    p.reactions = vec![reaction(Handle::NewNpc(1), Stance::Comply, None)];
    p.facts = vec![fact(
        slot(Handle::NewNpc(1), "mood", "wary"),
        Seen::Present,
        vec![],
    )];
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);

    let kett = w.view.entities.iter().find(|e| e.name == "Kett").unwrap();
    assert_eq!(kett.age, Some(34));
    let kett = kett.id;
    assert!(w.view.present().contains(&kett));
    assert!(w.store.voice_card(kett).unwrap().is_some());
    let mood = w.view.live_fact(kett, "mood").unwrap();
    assert_eq!(mood.canon, Canon::Established, "mood is a state key");
    let holders: Vec<u64> = w
        .view
        .knowledge
        .iter()
        .filter(|k| k.fact_id == Some(mood.id) && k.how == Knowing::Witnessed)
        .map(|k| k.holder)
        .collect();
    for holder in [w.ada, w.orsel, w.brann, kett] {
        assert!(holders.contains(&holder), "{holder} should witness");
    }
    assert!(!holders.contains(&w.pip), "Pip is not present");
}

#[test]
fn seen_controls_canon_and_existing_slot_is_superseded() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![
        fact(slot(e(w.brann), "mood", "bored"), Seen::Gm, vec![]),
        fact(slot(e(w.brann), "scar", "left cheek"), Seen::Pc, vec![]),
    ];
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    assert_eq!(
        w.view.live_fact(w.brann, "mood").unwrap().canon,
        Canon::Established
    );
    assert_eq!(
        w.view.live_fact(w.brann, "scar").unwrap().canon,
        Canon::Persistent
    );

    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.brann), "mood", "alert"), Seen::Gm, vec![])];
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    assert_eq!(
        w.view.live_fact(w.brann, "mood").unwrap().slot.value,
        "alert"
    );
}

#[test]
fn cited_possible_fact_is_established() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(w.brann), "mood", "suspicious"),
        Seen::Gm,
        vec![f(w.rumor)],
    )];
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    assert_eq!(w.view.fact(f(w.rumor)).unwrap().canon, Canon::Established);
}

#[test]
fn pc_perceived_possible_fact_is_established() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.orsel), "bribable", "yes"), Seen::Pc, vec![])];
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    let rumor = w.view.fact(f(w.rumor)).unwrap();
    assert_eq!(rumor.canon, Canon::Established);
    assert!(rumor.superseded_by.is_none());
}

#[test]
fn claims_blockers_rolls_schedule_and_clock_commit() {
    let mut w = World::new();
    let open = w.obstacle;
    w.commit(vec![LedgerOp::ClearObstacle { obstacle_id: open }]);
    let mut p = proposal(Verdict::Check);
    p.blockers = vec![f(w.gate_locked)];
    p.check = Some(check(
        None,
        slot(e(w.gate), "locked", "no"),
        slot(e(w.ada), "injury", "bruised"),
    ));
    let mut claim = fact(slot(e(w.ada), "holds", "nothing"), Seen::Present, vec![]);
    claim.said_by = Some(e(w.orsel));
    p.facts = vec![claim];
    p.schedule = Some(EventDraft {
        in_minutes: 10,
        seen_by: vec![],
        actor: e(w.brann),
        goal: None,
        text: "Brann fetches the captain".into(),
        effects: vec![slot(e(w.brann), "errand", "captain")],
    });
    p.minutes = 5;
    ok(validate(&p, &w.view));

    let valid = validate(&p, &w.view).unwrap();
    let resolved = to_ops(valid, Some(&roll(false)), &w.view);
    assert!(resolved.ops.iter().any(|op| matches!(
        op,
        LedgerOp::RecordRoll { outcome, .. } if outcome == "lose"
    )));
    w.commit(resolved.ops);
    assert_eq!(w.view.obstacles_open.len(), 1, "blocker opened");
    assert_eq!(
        w.view.live_fact(w.gate, "locked").unwrap().slot.value,
        "yes"
    );
    assert_eq!(
        w.view.live_fact(w.ada, "injury").unwrap().slot.value,
        "bruised"
    );
    assert_eq!(w.view.claims.len(), 1);
    assert_eq!(w.view.clock_min, 5);
    let event = w
        .view
        .events_scheduled
        .iter()
        .find(|ev| ev.text == "Brann fetches the captain")
        .unwrap();
    assert_eq!(event.trigger, threshold_core::domain::Trigger::At(15));
}

#[test]
fn rejection_display_is_one_line() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.reactions = vec![reaction(e(w.orsel), Stance::Help, None)];
    p.facts = vec![fact(slot(e(w.ada), "intends", "flee"), Seen::Pc, vec![])];
    let errs = validate(&p, &w.view).unwrap_err();
    assert!(errs.len() >= 2);
    for r in errs {
        let line = r.to_string();
        assert!(!line.contains('\n'), "{line}");
        assert!(!line.is_empty());
    }
}

// --- Per-Turn schema -------------------------------------------------------

#[test]
fn per_turn_schema_accepts_known_handles_and_rejects_unknown() {
    let ctx = HandleContext {
        entities: vec![Handle::Entity(1), Handle::Entity(2)],
        facts: vec![Handle::Fact(40)],
        events: vec![Handle::WorldEvent(3)],
        goals: vec![Handle::Goal(2)],
        new_slots: 1,
        places: vec![Handle::Entity(7)],
        open_items: vec!["E1 pending_trip".into()],
    };
    let schema = proposal_schema_for(&ctx);
    let validator = jsonschema::validator_for(&schema).unwrap();
    let good = serde_json::json!({
        "action": "talk",
        "verdict": "allowed",
        "contested": false,
        "move_to": null,
        "open_items": {"E1 pending_trip": "open"},
        "reactions": [{"npc": "E2", "stance": "neutral", "plan": "listens", "cause": {"fact": "F40"}}],
        "facts": [{"slot": {"subject": "N1", "key": "mood", "value": "calm"}, "seen": "pc", "because": ["F40"]}],
        "new_npcs": [{"id": "N1", "name": "Kett", "age": 34, "voice": "terse"}]
    });
    assert!(validator.is_valid(&good), "{:?}", validator.validate(&good));
    let mut bad = good.clone();
    bad["reactions"][0]["npc"] = "E999".into();
    assert!(!validator.is_valid(&bad));
    let mut ageless = good.clone();
    ageless["new_npcs"][0]
        .as_object_mut()
        .unwrap()
        .remove("age");
    assert!(!validator.is_valid(&ageless));
    let mut too_old = good.clone();
    too_old["new_npcs"][0]["age"] = 1000.into();
    assert!(!validator.is_valid(&too_old));
    // ADR 0007: `move_to` is required, a known place or null.
    let mut moving = good.clone();
    moving["move_to"] = "E7".into();
    assert!(
        validator.is_valid(&moving),
        "{:?}",
        validator.validate(&moving)
    );
    moving["move_to"] = "E2".into();
    assert!(!validator.is_valid(&moving));
    let mut silent = good.clone();
    silent.as_object_mut().unwrap().remove("move_to");
    assert!(!validator.is_valid(&silent));
    let mut unsaid = good.clone();
    unsaid.as_object_mut().unwrap().remove("contested");
    assert!(!validator.is_valid(&unsaid));
    // `open_items` is required: one status for each open item the PC
    // holds, `open` included, and no other key.
    let mut closing = good.clone();
    closing["open_items"]["E1 pending_trip"] = "accepted".into();
    assert!(
        validator.is_valid(&closing),
        "{:?}",
        validator.validate(&closing)
    );
    let mut unknown = good.clone();
    unknown["open_items"] = serde_json::json!({"E1 pending_feast": "open"});
    assert!(!validator.is_valid(&unknown));
    unknown["open_items"]["E1 pending_trip"] = "open".into();
    assert!(!validator.is_valid(&unknown), "an extra key");
    let mut unstated = good.clone();
    unstated["open_items"] = serde_json::json!({});
    assert!(
        !validator.is_valid(&unstated),
        "a held item without a status"
    );
    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("open_items");
    assert!(!validator.is_valid(&missing));
    // With no open item held, the schema has no `open_items`.
    let none = proposal_schema_for(&HandleContext {
        open_items: vec![],
        ..ctx.clone()
    });
    let none = jsonschema::validator_for(&none).unwrap();
    assert!(none.is_valid(&missing), "{:?}", none.validate(&missing));
    assert!(!none.is_valid(&good));
}

// --- Fix round 1 -----------------------------------------------------------

#[test]
fn agency_applies_to_stakes_and_effects() {
    let w = World::new();
    let mut p = proposal(Verdict::Check);
    p.check = Some(check(
        None,
        slot(e(w.orsel), "convinced", "yes"),
        slot(e(w.ada), "intends", "give up"),
    ));
    rejected(validate(&p, &w.view), CheckKind::Agency);

    let mut p = proposal(Verdict::Allowed);
    p.schedule = Some(EventDraft {
        in_minutes: 5,
        seen_by: vec![],
        actor: e(w.brann),
        goal: None,
        text: "Brann shouts".into(),
        effects: vec![slot(e(w.ada), "intends", "flee")],
    });
    rejected(validate(&p, &w.view), CheckKind::Agency);
}

#[test]
fn lose_stake_is_not_exempt_from_coincidence_or_tension() {
    let w = World::new();
    let mut p = proposal(Verdict::Check);
    p.check = Some(check(
        None,
        slot(e(w.ada), "fatigue", "winded"),
        slot(e(w.gate), "locked", "no"),
    ));
    rejected(validate(&p, &w.view), CheckKind::Coincidence);

    let mut p = proposal(Verdict::Check);
    p.check = Some(check(
        None,
        slot(e(w.ada), "fatigue", "winded"),
        slot(e(w.orsel), "alive", "no"),
    ));
    rejected(validate(&p, &w.view), CheckKind::TensionEvaporates);

    let mut p = proposal(Verdict::Check);
    p.check = Some(check(
        None,
        slot(e(w.orsel), "alive", "no"),
        slot(e(w.ada), "fatigue", "winded"),
    ));
    rejected(validate(&p, &w.view), CheckKind::TensionEvaporates);
}

#[test]
fn state_keys_never_persistent() {
    use threshold_core::domain::STATE_KEYS;
    assert!(STATE_KEYS.contains(&"location") && !STATE_KEYS.contains(&"alive"));
    let mut w = World::new();
    assert_eq!(
        w.view.live_fact(w.ada, "location").unwrap().canon,
        Canon::Established,
        "seed-decided location"
    );
    assert_eq!(
        w.view.live_fact(w.gate, "material").unwrap().canon,
        Canon::Persistent
    );

    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.ada), "fatigue", "tired"), Seen::Pc, vec![])];
    apply(&mut w, &p, None);
    assert_eq!(
        w.view.live_fact(w.ada, "fatigue").unwrap().canon,
        Canon::Established,
        "PC-witnessed fatigue"
    );

    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![
        fact(slot(e(w.ada), "fatigue", "rested"), Seen::Pc, vec![]),
        fact(slot(e(w.brann), "location", "tavern"), Seen::Pc, vec![]),
    ];
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    assert_eq!(
        w.view.live_fact(w.ada, "fatigue").unwrap().slot.value,
        "rested"
    );
    assert_eq!(
        w.view.live_fact(w.brann, "location").unwrap().slot.value,
        "tavern"
    );
    w.store.rebuild_projections(w.campaign).unwrap();
    assert_eq!(w.store.snapshot(w.campaign).unwrap(), w.view);
}

#[test]
fn move_to_writes_the_pc_location_when_the_action_happens() {
    let mut w = World::new();
    // A PC location fact is still an agency error when it reaches the
    // checks directly; `settle` drops it first (ADR 0007).
    let mut p = proposal(Verdict::Allowed);
    p.action = threshold_core::domain::ActionKind::Move;
    p.facts = vec![fact(slot(e(w.ada), "location", "tavern"), Seen::Pc, vec![])];
    rejected(validate(&p, &w.view), CheckKind::Agency);

    let mut p = proposal(Verdict::Allowed);
    p.action = threshold_core::domain::ActionKind::Move;
    p.move_to = Some(e(w.gate));
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    let location = w.view.live_fact(w.ada, "location").unwrap();
    assert_eq!(location.slot.value, "gate");
    assert!(location.seen_by.contains(&w.ada));
    assert!(
        !w.view.present().contains(&w.orsel),
        "Orsel stayed at the gate square"
    );
}

#[test]
fn a_check_moves_the_pc_on_a_success_only() {
    for (success, moved) in [(false, false), (true, true)] {
        let mut w = World::new();
        let mut p = proposal(Verdict::Check);
        p.action = threshold_core::domain::ActionKind::Move;
        p.move_to = Some(e(w.gate));
        p.check = Some(check(
            None,
            slot(e(w.brann), "mood", "impressed"),
            slot(e(w.ada), "injury", "sprain"),
        ));
        apply(&mut w, &p, Some(roll(success)));
        let at = w
            .view
            .live_fact(w.ada, "location")
            .unwrap()
            .slot
            .value
            .clone();
        assert_eq!(at == "gate", moved, "success {success}: {at}");
    }
    // An impossible verdict moves nobody.
    let mut w = World::new();
    let mut p = proposal(Verdict::Impossible);
    p.action = threshold_core::domain::ActionKind::Move;
    p.move_to = Some(e(w.gate));
    apply(&mut w, &p, None);
    assert_eq!(
        w.view.live_fact(w.ada, "location").unwrap().slot.value,
        "gate square"
    );
}

/// R1: a move with `move_to: null` is not rejected: the PC stays.
#[test]
fn move_with_no_destination_is_not_rejected() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.action = threshold_core::domain::ActionKind::Move;
    p.minutes = 10;
    ok(validate(&p, &w.view));
}

#[test]
fn move_to_names_a_place_the_pc_knows() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.action = threshold_core::domain::ActionKind::Move;
    p.move_to = Some(e(w.orsel));
    rejected(validate(&p, &w.view), CheckKind::References);
    let (settled, notes) = settle(p, &w.view);
    assert_eq!(settled.move_to, None);
    assert_eq!(notes.len(), 1, "{notes:?}");
    ok(validate(&settled, &w.view));
}

/// R1: the model's own PC location, in a fact or a stake, is dropped and
/// logged; the rest of the proposal stays.
#[test]
fn settle_drops_a_pc_location_fact_and_stake() {
    let w = World::new();
    let mut p = proposal(Verdict::Check);
    p.action = threshold_core::domain::ActionKind::Move;
    p.facts = vec![
        fact(slot(e(w.ada), "location", "tavern"), Seen::Pc, vec![]),
        fact(slot(e(w.brann), "mood", "wary"), Seen::Pc, vec![]),
    ];
    p.check = Some(check(
        None,
        slot(e(w.ada), "location", "roof"),
        slot(e(w.ada), "injury", "sprain"),
    ));
    let (settled, notes) = settle(p, &w.view);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(settled.facts.len(), 1);
    assert_eq!(settled.facts[0].slot.subject, e(w.brann));
    let c = settled.check.as_ref().unwrap();
    assert!(c.win.is_none());
    assert!(c.lose.is_some());
    ok(validate(&settled, &w.view));
}

#[test]
fn alive_stays_protected() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.brann), "alive", "no"), Seen::Pc, vec![])];
    apply(&mut w, &p, None);
    let dead = w.view.live_fact(w.brann, "alive").unwrap();
    assert_eq!(dead.canon, Canon::Persistent);
    let dead = dead.id;

    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(w.brann), "alive", "yes"),
        Seen::Pc,
        vec![f(w.debt)],
    )];
    let errs = rejected(validate(&p, &w.view), CheckKind::FactBends);
    assert_eq!(errs[0].handle, Some(f(dead)));
}

/// ADR 0005: an open key is a state key. A PC-witnessed open item stays
/// Established, and a later Turn may write the same `pending_` slot.
#[test]
fn open_fact_can_be_replaced_by_a_turn() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(w.orsel), "pending_inspection", "tonight"),
        Seen::Pc,
        vec![],
    )];
    apply(&mut w, &p, None);
    assert_eq!(
        w.view
            .live_fact(w.orsel, "pending_inspection")
            .unwrap()
            .canon,
        Canon::Established
    );

    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(w.orsel), "pending_inspection", "tomorrow"),
        Seen::Pc,
        vec![],
    )];
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    assert_eq!(
        w.view
            .live_fact(w.orsel, "pending_inspection")
            .unwrap()
            .slot
            .value,
        "tomorrow"
    );
}

#[test]
fn schedule_effect_may_set_pc_reputation() {
    let w = World::new();
    for key in ["reputation", "standing", "debt"] {
        assert!(PC_INVOLUNTARY_KEYS.contains(&key), "{key}");
        assert!(threshold_core::domain::is_state_key(key), "{key}");
    }
    let mut p = proposal(Verdict::Allowed);
    p.schedule = Some(EventDraft {
        in_minutes: 1440,
        seen_by: vec![],
        actor: e(w.orsel),
        goal: None,
        text: "Orsel files a complaint".into(),
        effects: vec![slot(e(w.ada), "reputation", "under_suspicion")],
    });
    ok(validate(&p, &w.view));
}

#[test]
fn witnessed_pc_reputation_changes_next_turn() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(w.ada), "reputation", "trusted"),
        Seen::Pc,
        vec![],
    )];
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    assert_eq!(
        w.view.live_fact(w.ada, "reputation").unwrap().canon,
        Canon::Established
    );
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(w.ada), "reputation", "disgraced"),
        Seen::Pc,
        vec![],
    )];
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    assert_eq!(
        w.view.live_fact(w.ada, "reputation").unwrap().slot.value,
        "disgraced"
    );
}

/// A scheduled event names its NPC witnesses; they reach the stored event.
/// The PC is never among them, and unknown handles are refused.
#[test]
fn schedule_seen_by_lists_npc_witnesses_only() {
    let mut w = World::new();
    let draft = |seen_by: Vec<Handle>| {
        let mut p = proposal(Verdict::Allowed);
        p.schedule = Some(EventDraft {
            in_minutes: 60,
            seen_by,
            actor: e(w.orsel),
            goal: None,
            text: "Orsel files a complaint".into(),
            effects: vec![slot(e(w.ada), "reputation", "under_suspicion")],
        });
        p
    };
    rejected(validate(&draft(vec![e(w.ada)]), &w.view), CheckKind::Agency);
    rejected(
        validate(&draft(vec![e(999)]), &w.view),
        CheckKind::References,
    );
    rejected(
        validate(&draft(vec![e(w.brann), e(w.pip), e(w.orsel)]), &w.view),
        CheckKind::References,
    );
    let p = draft(vec![e(w.brann)]);
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    let event = w.view.events_scheduled.iter().find(|ev| ev.id != w.bell);
    assert_eq!(event.unwrap().seen_by, [w.brann]);
}

// --- Canon per writer (final fix 2) ------------------------------------------
//
// One rule for every fact a Turn writes: Persistent when the PC witnesses
// it, Established otherwise, and a state key is never Persistent.

#[test]
fn canon_of_a_proposal_fact() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![
        fact(slot(e(w.brann), "scar", "left cheek"), Seen::Pc, vec![]),
        fact(slot(e(w.brann), "oath", "silence"), Seen::Gm, vec![]),
    ];
    apply(&mut w, &p, None);
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.brann), "mood", "calm"), Seen::Pc, vec![])];
    apply(&mut w, &p, None);
    let canon = |key: &str| w.view.live_fact(w.brann, key).unwrap().canon;
    assert_eq!(canon("scar"), Canon::Persistent);
    assert_eq!(canon("oath"), Canon::Established);
    assert_eq!(canon("mood"), Canon::Established, "state key");
}

fn stake_check(w: &World) -> threshold_core::domain::DecisionProposal {
    let mut p = proposal(Verdict::Check);
    p.blockers = vec![f(w.gate_locked)];
    p.check = Some(check(
        None,
        slot(e(w.gate), "locked", "no"),
        slot(e(w.ada), "fatigue", "winded"),
    ));
    p
}

#[test]
fn canon_of_a_stake_fact() {
    // Won: `locked` is not a state key, and the present PC witnesses it.
    let mut w = World::new();
    let p = stake_check(&w);
    apply(&mut w, &p, Some(roll(true)));
    let won = w.view.live_fact(w.gate, "locked").unwrap();
    assert_eq!(won.slot.value, "no");
    assert_eq!(won.source, threshold_core::domain::FactSource::Stake);
    assert_eq!(won.canon, Canon::Persistent);

    // Lost: `fatigue` is a state key, so it stays Established.
    let mut w = World::new();
    let p = stake_check(&w);
    apply(&mut w, &p, Some(roll(false)));
    let lost = w.view.live_fact(w.ada, "fatigue").unwrap();
    assert_eq!(lost.source, threshold_core::domain::FactSource::Stake);
    assert_eq!(lost.canon, Canon::Established);
}

#[test]
fn canon_of_an_adventure_seed_fact() {
    let w = World::new();
    let material = w.view.live_fact(w.gate, "material").unwrap();
    assert_eq!(
        material.source,
        threshold_core::domain::FactSource::AdventureSeed
    );
    assert_eq!(material.canon, Canon::Persistent, "decided");
    assert_eq!(
        w.view.live_fact(w.ada, "location").unwrap().canon,
        Canon::Established,
        "decided state key"
    );
    assert_eq!(w.view.fact(f(w.rumor)).unwrap().canon, Canon::Possible);
}

/// Final fix 4: a new NPC on a move Turn stands where the PC arrives.
#[test]
fn new_npc_on_a_move_turn_stands_at_the_new_place() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.action = threshold_core::domain::ActionKind::Move;
    p.move_to = Some(e(w.gate));
    p.new_npcs = vec![new_npc(1, "Kett", 34)];
    apply(&mut w, &p, None);
    let kett = w
        .view
        .entities
        .iter()
        .find(|x| x.name == "Kett")
        .unwrap()
        .id;
    assert_eq!(
        w.view.live_fact(kett, "location").unwrap().slot.value,
        "gate"
    );
    assert!(w.view.present().contains(&kett));
}

// --- Settle: the core's repairs (QA, 2026-11-10) ----------------------------

#[test]
fn settle_removes_the_three_pc_shapes_and_keeps_the_rest() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.action = threshold_core::domain::ActionKind::Talk;
    p.reactions = vec![
        reaction(e(w.ada), Stance::Comply, None),
        reaction(e(w.brann), Stance::Neutral, None),
    ];
    p.schedule = Some(EventDraft {
        in_minutes: 30,
        actor: e(w.orsel),
        goal: None,
        text: "Orsel reports to the guard".into(),
        effects: vec![],
        seen_by: vec![e(w.ada), e(w.brann)],
    });
    rejected(validate(&p, &w.view), CheckKind::Agency);

    let (settled, notes) = settle(p, &w.view);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(settled.reactions.len(), 1);
    assert_eq!(settled.reactions[0].npc, e(w.brann));
    assert_eq!(settled.schedule.as_ref().unwrap().seen_by, [e(w.brann)]);
    ok(validate(&settled, &w.view));

    // An event whose actor is the PC goes whole.
    let mut p = proposal(Verdict::Allowed);
    p.schedule = Some(EventDraft {
        in_minutes: 30,
        actor: e(w.ada),
        goal: None,
        text: "Ada comes back".into(),
        effects: vec![],
        seen_by: vec![],
    });
    let (settled, notes) = settle(p, &w.view);
    assert!(settled.schedule.is_none());
    assert_eq!(notes.len(), 1);
    ok(validate(&settled, &w.view));
}

#[test]
fn settle_never_repairs_other_agency_cases() {
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![threshold_core::domain::NewFact {
        slot: slot(e(w.orsel), "told", "yes"),
        seen: Seen::Pc,
        said_by: Some(e(w.ada)),
        because: vec![],
    }];
    let (settled, notes) = settle(p.clone(), &w.view);
    assert!(notes.is_empty());
    assert_eq!(settled, p);
    rejected(validate(&settled, &w.view), CheckKind::Agency);

    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.ada), "mood", "angry"), Seen::Pc, vec![])];
    let (settled, notes) = settle(p, &w.view);
    assert!(notes.is_empty());
    rejected(validate(&settled, &w.view), CheckKind::Agency);
}

#[test]
fn settle_spells_a_known_place_by_its_name() {
    let w = World::new();
    for value in [format!("E{}", w.gate), "the gate.".into(), "GATE".into()] {
        let mut p = proposal(Verdict::Allowed);
        p.facts = vec![fact(slot(e(w.brann), "location", &value), Seen::Pc, vec![])];
        let (settled, _) = settle(p, &w.view);
        assert_eq!(settled.facts[0].slot.value, "Gate", "{value}");
    }
    // A place that is not an entity stays as written.
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(w.brann), "location", "tavern"),
        Seen::Pc,
        vec![],
    )];
    let (settled, notes) = settle(p, &w.view);
    assert_eq!(settled.facts[0].slot.value, "tavern");
    assert!(notes.is_empty());
}

// --- Open Items the PC settles (QA, 2026-11-10) ------------------------------

fn pending(w: &mut World, subject: u64, key: &str, seen_by: Vec<u64>) {
    w.commit(vec![LedgerOp::AddFact {
        slot: threshold_core::domain::Slot {
            subject: threshold_core::domain::EntityRef::Id(subject),
            key: key.into(),
            value: "agathe asked".into(),
        },
        text: "Agathe asked for help with the winter post.".into(),
        canon: Canon::Established,
        source: threshold_core::domain::FactSource::Proposal,
        seen_by: seen_by
            .into_iter()
            .map(threshold_core::domain::EntityRef::Id)
            .collect(),
        causes: vec![],
    }]);
}

/// ADR 0007: the PC's own outcome fact is refused even when the PC holds
/// the item; `open_items` gives the item a closing status and the core
/// writes the outcome.
#[test]
fn a_closing_status_writes_the_outcome_of_an_open_item_the_pc_holds() {
    let mut w = World::new();
    let ada = w.ada;
    let item = format!("E{ada} pending_winter_post");
    pending(&mut w, ada, "pending_winter_post", vec![]);
    // An open item the PC does not hold cannot be closed.
    let mut p = proposal(Verdict::Allowed);
    p.action = threshold_core::domain::ActionKind::Talk;
    p.open_items = [(item.clone(), ItemStatus::Accepted)].into();
    rejected(validate(&p, &w.view), CheckKind::References);
    let (settled, notes) = settle(p.clone(), &w.view);
    assert!(settled.open_items.is_empty());
    assert_eq!(notes.len(), 1, "{notes:?}");

    // The PC learns of it.
    let fact_id = w.view.live_fact(ada, "pending_winter_post").unwrap().id;
    let now = w.view.clock_min;
    w.commit(vec![LedgerOp::Know {
        holder: threshold_core::domain::EntityRef::Id(ada),
        fact_id: Some(fact_id),
        claim_id: None,
        how: Knowing::Witnessed,
        arrives_at: now,
    }]);
    assert_eq!(
        threshold_core::world::pc_open_items(&w.view),
        [(ada, "pending_winter_post".to_owned())]
    );
    // The model's own outcome fact on the PC is still refused.
    let mut own = proposal(Verdict::Allowed);
    own.facts = vec![fact(
        slot(e(ada), "winter_post", "accepted"),
        Seen::Pc,
        vec![],
    )];
    rejected(validate(&own, &w.view), CheckKind::Agency);
    // With a closing status, a fact on the same slot goes: the core
    // writes it.
    p.facts = own.facts.clone();
    let (settled, notes) = settle(p, &w.view);
    assert!(settled.facts.is_empty());
    assert_eq!(notes.len(), 1, "{notes:?}");
    ok(validate(&settled, &w.view));
    apply(&mut w, &settled, None);
    let outcome = w.view.live_fact(ada, "winter_post").unwrap();
    assert_eq!(outcome.slot.value, "accepted");
    for holder in [ada, w.orsel, w.brann] {
        assert!(outcome.seen_by.contains(&holder), "{holder}");
    }
    // A closed item is no longer open.
    assert!(threshold_core::world::pc_open_items(&w.view).is_empty());
}

#[test]
fn outcome_of_an_npc_open_item_needs_no_unlock() {
    let mut w = World::new();
    let brann = w.brann;
    pending(&mut w, brann, "pending_winter_post", vec![]);
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(
        slot(e(w.brann), "winter_post", "done"),
        Seen::Pc,
        vec![],
    )];
    ok(validate(&p, &w.view));
}

// --- Contests (ADR 0007) -------------------------------------------------------

/// The recorded shape from the live run: a contest whose win stake writes
/// a PC key the agency rule refuses. `settle` drops that stake and logs it;
/// the Check stays valid and still rolls.
#[test]
fn illegal_pc_stake_is_dropped_and_the_check_still_rolls() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Check);
    p.contested = true;
    p.check = Some(check(
        Some(e(w.brann)),
        slot(e(w.ada), &format!("E{}_strength", w.ada), "proven"),
        slot(e(w.ada), "fatigue", "sore_arm"),
    ));
    rejected(validate(&p, &w.view), CheckKind::Agency);
    let (settled, notes) = settle(p, &w.view);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("win stake"), "{notes:?}");
    let c = settled.check.as_ref().unwrap();
    assert!(c.win.is_none() && c.lose.is_some());
    ok(validate(&settled, &w.view));
    let facts = w.view.facts.len();
    apply(&mut w, &settled, Some(roll(true)));
    assert_eq!(
        w.view.facts.len(),
        facts,
        "a won Check with no win stake writes nothing"
    );
    let rolls = w
        .store
        .turn_ops(w.view.recent_turns.last().unwrap().id)
        .unwrap()
        .into_iter()
        .filter(|op| matches!(op, LedgerOp::RecordRoll { .. }))
        .count();
    assert_eq!(rolls, 1);
}

#[test]
fn contested_act_without_a_check_gets_a_note_then_a_default_check() {
    use threshold_core::checks::{contest_note, default_check};
    let w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.contested = true;
    p.reactions = vec![
        reaction(e(w.orsel), Stance::Resist, None),
        reaction(e(w.brann), Stance::Neutral, None),
    ];
    let note = contest_note(&p).expect("a contested act with no check");
    assert_eq!(note.check, CheckKind::Contest);
    let mut built = p.clone();
    let log = default_check(&mut built, &w.view).expect("the core builds the Check");
    assert!(log.contains(&format!("E{}", w.orsel)), "{log}");
    assert_eq!(built.verdict, Verdict::Check);
    let c = built.check.as_ref().unwrap();
    assert_eq!(c.against, Some(e(w.orsel)));
    assert!(c.win.is_none() && c.lose.is_none() && c.factors.is_empty());
    assert!(contest_note(&built).is_none());
    ok(validate(&built, &w.view));
    // Not contested, or impossible: nothing to do.
    let mut calm = p.clone();
    calm.contested = false;
    assert!(contest_note(&calm).is_none());
    assert!(default_check(&mut calm, &w.view).is_none());
    let mut impossible = p;
    impossible.verdict = Verdict::Impossible;
    assert!(contest_note(&impossible).is_none());
    assert!(default_check(&mut impossible, &w.view).is_none());
}

// --- Blockers (ADR 0007) -------------------------------------------------------

/// Changed after review: an impossible verdict opens its real blockers
/// again, as before the bug pass. Only the PC's own identity facts are
/// never blockers (`a_fact_about_the_pc_itself_is_never_a_blocker`). A
/// GM-only obstacle that an impossible act left must still need a cause.
#[test]
fn an_impossible_verdict_and_a_check_open_their_real_blockers() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Impossible);
    p.blockers = vec![f(w.gate_material)];
    apply(&mut w, &p, None);
    assert!(
        w.view
            .obstacles_open
            .iter()
            .any(|o| o.fact_id == w.gate_material)
    );
    // The next Turn cannot change that slot with no cause.
    let mut next = proposal(Verdict::Allowed);
    next.facts = vec![fact(
        slot(e(w.gate), "material", "wood"),
        Seen::Present,
        vec![],
    )];
    rejected(validate(&next, &w.view), CheckKind::Coincidence);

    let mut w = World::new();

    let mut p = proposal(Verdict::Check);
    p.blockers = vec![f(w.gate_material)];
    p.check = Some(check(
        None,
        slot(e(w.brann), "mood", "impressed"),
        slot(e(w.ada), "fatigue", "winded"),
    ));
    apply(&mut w, &p, Some(roll(false)));
    assert!(
        w.view
            .obstacles_open
            .iter()
            .any(|o| o.fact_id == w.gate_material)
    );
}

#[test]
fn a_fact_about_the_pc_itself_is_never_a_blocker() {
    let mut w = World::new();
    let location = w.view.live_fact(w.ada, "location").unwrap().id;
    let ada = w.ada;
    w.commit(vec![LedgerOp::AddFact {
        slot: threshold_core::domain::Slot {
            subject: threshold_core::domain::EntityRef::Id(ada),
            key: "injury".into(),
            value: "broken arm".into(),
        },
        text: "Ada's arm is broken.".into(),
        canon: Canon::Established,
        source: threshold_core::domain::FactSource::Proposal,
        seen_by: vec![threshold_core::domain::EntityRef::Id(ada)],
        causes: vec![],
    }]);
    let injury = w.view.live_fact(ada, "injury").unwrap().id;
    let mut p = proposal(Verdict::Impossible);
    p.blockers = vec![f(location), f(injury)];
    let (settled, notes) = settle(p, &w.view);
    assert_eq!(settled.blockers, [f(injury)], "an injury may block");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains(&format!("F{location}")), "{notes:?}");
}

// --- open_items after review (ADR 0007) --------------------------------------

/// Gives the PC an Open Item on `key` that it holds: the pending fact, then
/// the PC's knowledge of it.
fn held_item(w: &mut World, key: &str) -> String {
    let ada = w.ada;
    pending(w, ada, key, vec![]);
    let fact_id = w.view.live_fact(ada, key).unwrap().id;
    let now = w.view.clock_min;
    w.commit(vec![LedgerOp::Know {
        holder: threshold_core::domain::EntityRef::Id(ada),
        fact_id: Some(fact_id),
        claim_id: None,
        how: Knowing::Witnessed,
        arrives_at: now,
    }]);
    format!("E{ada} {key}")
}

fn closing(item: &str, verdict: Verdict) -> threshold_core::domain::DecisionProposal {
    let mut p = proposal(verdict);
    p.action = threshold_core::domain::ActionKind::Talk;
    p.open_items = [(item.to_owned(), ItemStatus::Accepted)].into();
    p
}

/// C1: a status is a closed list. No free text parses, and the per-Turn
/// schema refuses any other word. `open` closes nothing; every other
/// status is the outcome word of the fact the core writes.
#[test]
fn item_status_is_a_closed_list() {
    use threshold_core::domain::CloseOutcome;
    type Items = std::collections::BTreeMap<String, ItemStatus>;
    for (word, status, outcome) in [
        ("open", ItemStatus::Open, None),
        (
            "accepted",
            ItemStatus::Accepted,
            Some(CloseOutcome::Accepted),
        ),
        ("refused", ItemStatus::Refused, Some(CloseOutcome::Refused)),
        ("done", ItemStatus::Done, Some(CloseOutcome::Done)),
        ("failed", ItemStatus::Failed, Some(CloseOutcome::Failed)),
        ("dropped", ItemStatus::Dropped, Some(CloseOutcome::Dropped)),
    ] {
        let items: Items =
            serde_json::from_value(serde_json::json!({"E1 pending_x": word})).unwrap();
        assert_eq!(items["E1 pending_x"], status);
        assert_eq!(status.outcome(), outcome);
        if let Some(outcome) = outcome {
            assert_eq!(outcome.word(), word);
        }
    }
    for free in ["agreed", "I swear it gladly", "Accepted", "postponed", ""] {
        let parsed: Result<Items, _> =
            serde_json::from_value(serde_json::json!({"E1 pending_x": free}));
        assert!(parsed.is_err(), "{free:?} parsed");
    }
    let schema = proposal_schema_for(&HandleContext {
        open_items: vec!["E1 pending_trip".into()],
        ..HandleContext::default()
    });
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut p = serde_json::json!({
        "action": "talk", "verdict": "allowed", "contested": false, "move_to": null,
        "open_items": {"E1 pending_trip": "refused"}
    });
    assert!(validator.is_valid(&p), "{:?}", validator.validate(&p));
    p["open_items"]["E1 pending_trip"] = "open".into();
    assert!(validator.is_valid(&p), "{:?}", validator.validate(&p));
    p["open_items"]["E1 pending_trip"] = "gladly agreed".into();
    assert!(!validator.is_valid(&p));
}

/// The live bug (play_basics, cdefde8): "I ask Orsel for more time" came
/// back as a close with `refused`, because the model had no way to say that
/// the Intent touched the item and left it open. Status `open` is that
/// way: it writes no outcome, and the PC still holds the item.
#[test]
fn an_open_status_keeps_the_item_open() {
    let mut w = World::new();
    let item = held_item(&mut w, "pending_winter_post");
    let mut p = proposal(Verdict::Allowed);
    p.action = threshold_core::domain::ActionKind::Talk;
    p.open_items = [(item.clone(), ItemStatus::Open)].into();
    let (settled, notes) = settle(p, &w.view);
    assert!(notes.is_empty(), "{notes:?}");
    apply(&mut w, &settled, None);
    assert!(w.view.live_fact(w.ada, "winter_post").is_none());
    assert_eq!(
        threshold_core::world::pc_open_items(&w.view),
        [(w.ada, "pending_winter_post".to_owned())]
    );
    // A held item the proposal leaves out stays open too: the schema
    // demands its status, but the core creates no close.
    let mut silent = proposal(Verdict::Allowed);
    silent.action = threshold_core::domain::ActionKind::Talk;
    apply(&mut w, &silent, None);
    assert_eq!(threshold_core::world::pc_open_items(&w.view).len(), 1);
    // With `open`, the outcome slot is not the core's: the model's own
    // outcome fact on the PC is still an agency error.
    let mut own = proposal(Verdict::Allowed);
    own.open_items = [(item, ItemStatus::Open)].into();
    own.facts = vec![fact(
        slot(e(w.ada), "winter_post", "accepted"),
        Seen::Pc,
        vec![],
    )];
    let (own, _) = settle(own, &w.view);
    assert_eq!(own.facts.len(), 1);
    rejected(validate(&own, &w.view), CheckKind::Agency);
}

/// C1: the core builds the outcome fact from the item's own text and the
/// outcome word; the value is the word.
#[test]
fn the_outcome_fact_is_built_from_the_item_text() {
    let mut w = World::new();
    let item = held_item(&mut w, "pending_winter_post");
    let p = closing(&item, Verdict::Allowed);
    apply(&mut w, &p, None);
    let outcome = w.view.live_fact(w.ada, "winter_post").unwrap();
    assert_eq!(outcome.slot.value, "accepted");
    assert_eq!(
        outcome.text,
        "Accepted: Agathe asked for help with the winter post."
    );
}

/// C2: a closing status obeys the verdict, like `move_to`: an impossible verdict
/// and a lost Check close nothing; a won Check closes.
#[test]
fn a_close_applies_only_when_the_action_happens() {
    for (verdict, roll_result, closed) in [
        (Verdict::Impossible, None, false),
        (Verdict::Check, Some(false), false),
        (Verdict::Check, Some(true), true),
        (Verdict::Allowed, None, true),
    ] {
        let mut w = World::new();
        let item = held_item(&mut w, "pending_winter_post");
        let mut p = closing(&item, verdict);
        if verdict == Verdict::Check {
            p.check = Some(check(
                Some(e(w.brann)),
                slot(e(w.brann), "mood", "impressed"),
                slot(e(w.brann), "mood", "smug"),
            ));
        }
        apply(&mut w, &p, roll_result.map(roll));
        assert_eq!(
            w.view.live_fact(w.ada, "winter_post").is_some(),
            closed,
            "{verdict:?} {roll_result:?}"
        );
        assert_eq!(
            threshold_core::world::pc_open_items(&w.view).is_empty(),
            closed,
            "{verdict:?} {roll_result:?}"
        );
    }
}

/// C3: the outcome fact is Established, though the PC witnesses it, so play
/// can change it later without a Correction.
#[test]
fn the_outcome_fact_is_established_and_play_can_change_it() {
    let mut w = World::new();
    let item = held_item(&mut w, "pending_winter_post");
    apply(&mut w, &closing(&item, Verdict::Allowed), None);
    let outcome = w.view.live_fact(w.ada, "winter_post").unwrap();
    assert!(outcome.seen_by.contains(&w.ada));
    assert_eq!(outcome.canon, Canon::Established);
    // A later write on the slot is not a Persistent fact bending: only the
    // agency rule, which is about the PC, speaks.
    let mut later = proposal(Verdict::Allowed);
    later.facts = vec![fact(
        slot(e(w.ada), "winter_post", "refused"),
        Seen::Present,
        vec![],
    )];
    let errs = validate(&later, &w.view).unwrap_err();
    assert!(
        errs.iter().all(|r| r.check != CheckKind::FactBends),
        "{errs:?}"
    );
}

/// C4: at most `MAX_CLOSES` items close per proposal. `validate` rejects a
/// third closing status; `settle` keeps the first two closes and sets the
/// rest back to `open`, with a note. The schema cannot count the closing
/// statuses, so the core alone holds this rule.
#[test]
fn a_proposal_closes_at_most_two_items() {
    use threshold_core::domain::MAX_CLOSES;
    assert_eq!(MAX_CLOSES, 2);
    let mut w = World::new();
    let items: Vec<String> = ["pending_a", "pending_b", "pending_c"]
        .into_iter()
        .map(|key| held_item(&mut w, key))
        .collect();
    let mut p = proposal(Verdict::Allowed);
    p.open_items = items
        .iter()
        .map(|item| (item.clone(), ItemStatus::Done))
        .collect();
    rejected(validate(&p, &w.view), CheckKind::References);
    let (settled, notes) = settle(p.clone(), &w.view);
    let closing = |p: &threshold_core::domain::DecisionProposal| {
        p.open_items
            .values()
            .filter(|s| s.outcome().is_some())
            .count()
    };
    assert_eq!(closing(&settled), MAX_CLOSES);
    assert_eq!(settled.open_items.len(), 3, "the third stays, as open");
    assert_eq!(notes.len(), 1, "{notes:?}");
    ok(validate(&settled, &w.view));
    // Three statuses of which only two close are fine.
    let mut two = p.clone();
    two.open_items.insert(items[2].clone(), ItemStatus::Open);
    ok(validate(&two, &w.view));
    let (two, notes) = settle(two, &w.view);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(closing(&two), MAX_CLOSES);
    apply(&mut w, &settled, None);
    assert_eq!(threshold_core::world::pc_open_items(&w.view).len(), 1);
}

// --- move_to and the default Check after review (ADR 0007) -------------------

/// Restored non-move case (C5): `move_to` moves the PC only with the move
/// action. `settle` drops it otherwise, and `to_ops` ignores it.
#[test]
fn pc_move_sets_location_only_for_move_action() {
    use threshold_core::domain::ActionKind;
    for action in [ActionKind::Talk, ActionKind::Act, ActionKind::Observe] {
        let mut w = World::new();
        let mut p = proposal(Verdict::Allowed);
        p.action = action;
        p.move_to = Some(e(w.gate));
        let (settled, notes) = settle(p.clone(), &w.view);
        assert_eq!(settled.move_to, None, "{action:?}");
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("move_to"), "{notes:?}");
        // Even unsettled, the Commit does not move the PC.
        apply(&mut w, &p, None);
        assert_eq!(
            w.view.live_fact(w.ada, "location").unwrap().slot.value,
            "gate square",
            "{action:?}"
        );
    }
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Move;
    p.move_to = Some(e(w.gate));
    let (settled, notes) = settle(p, &w.view);
    assert_eq!(settled.move_to, Some(e(w.gate)));
    assert!(notes.is_empty(), "{notes:?}");
    apply(&mut w, &settled, None);
    assert_eq!(
        w.view.live_fact(w.ada, "location").unwrap().slot.value,
        "gate"
    );
}

/// C8: when the core builds the default Check, the proposal's facts were
/// written for a success: they commit on a won Roll only.
#[test]
fn facts_of_a_default_check_apply_on_a_win_only() {
    use threshold_core::checks::default_check;
    for (success, applied) in [(false, false), (true, true)] {
        let mut w = World::new();
        let mut p = proposal(Verdict::Allowed);
        p.contested = true;
        p.reactions = vec![reaction(e(w.brann), Stance::Resist, None)];
        p.facts = vec![fact(
            slot(e(w.brann), "mood", "beaten"),
            Seen::Present,
            vec![],
        )];
        assert!(default_check(&mut p, &w.view).is_some());
        apply(&mut w, &p, Some(roll(success)));
        assert_eq!(
            w.view.live_fact(w.brann, "mood").is_some(),
            applied,
            "success {success}"
        );
    }
    // A Check the model wrote keeps its facts on a lost Roll, as before.
    let mut w = World::new();
    let mut p = proposal(Verdict::Check);
    p.check = Some(check(
        None,
        slot(e(w.orsel), "mood", "impressed"),
        slot(e(w.ada), "fatigue", "winded"),
    ));
    p.facts = vec![fact(
        slot(e(w.brann), "mood", "watchful"),
        Seen::Present,
        vec![],
    )];
    apply(&mut w, &p, Some(roll(false)));
    assert!(w.view.live_fact(w.brann, "mood").is_some());
}
