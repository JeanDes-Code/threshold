mod support;

use support::*;
use threshold_core::{
    HandleContext,
    checks::{
        CheckKind, PC_INVOLUNTARY_KEYS, Rejection, STANCE_CEILINGS, ceiling, to_ops, validate,
    },
    domain::{Cancel, Canon, Cause, EventDraft, Handle, Knowing, LedgerOp, Seen, Stance, Verdict},
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
    let resolved = to_ops(valid, roll.as_ref(), &w.view, w.view.turn_seq + 1);
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
        Box::new(|p| p.new_npcs = vec![new_npc(1, "Kett", true), new_npc(1, "Lio", true)]),
        Box::new(|p| p.reactions = vec![reaction(Handle::NewNpc(2), Stance::Neutral, None)]),
        Box::new(|p| p.new_npcs = vec![new_npc(300, "Kett", true)]),
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

    let mut p = proposal(Verdict::Allowed);
    p.intimate = true;
    p.new_npcs = vec![new_npc(1, "Kett", false)];
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
    let mut p = proposal(Verdict::Allowed);
    p.intimate = true;
    p.reactions = vec![reaction(e(w.brann), Stance::Comply, None)];
    p.new_npcs = vec![new_npc(1, "Kett", true)];
    ok(validate(&p, &w.view));
}

// --- to_ops ----------------------------------------------------------------

#[test]
fn new_npc_commits_with_voice_card_location_and_witnessed_fact() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.new_npcs = vec![new_npc(1, "Kett", true)];
    p.reactions = vec![reaction(Handle::NewNpc(1), Stance::Comply, None)];
    p.facts = vec![fact(
        slot(Handle::NewNpc(1), "mood", "wary"),
        Seen::Present,
        vec![],
    )];
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);

    let kett = w.view.entities.iter().find(|e| e.name == "Kett").unwrap();
    assert_eq!(kett.adult, Some(true));
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
        actor: e(w.brann),
        goal: None,
        text: "Brann fetches the captain".into(),
        effects: vec![slot(e(w.brann), "errand", "captain")],
    });
    p.minutes = 5;
    ok(validate(&p, &w.view));

    let valid = validate(&p, &w.view).unwrap();
    let resolved = to_ops(valid, Some(&roll(false)), &w.view, w.view.turn_seq + 1);
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
    };
    let schema = proposal_schema_for(&ctx);
    let validator = jsonschema::validator_for(&schema).unwrap();
    let good = serde_json::json!({
        "action": "talk",
        "verdict": "allowed",
        "reactions": [{"npc": "E2", "stance": "neutral", "plan": "listens", "cause": {"fact": "F40"}}],
        "facts": [{"slot": {"subject": "N1", "key": "mood", "value": "calm"}, "seen": "pc", "because": ["F40"]}],
        "new_npcs": [{"id": "N1", "name": "Kett", "adult": true, "voice": "terse"}]
    });
    assert!(validator.is_valid(&good), "{:?}", validator.validate(&good));
    let mut bad = good.clone();
    bad["reactions"][0]["npc"] = "E999".into();
    assert!(!validator.is_valid(&bad));
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
fn pc_move_sets_location_only_for_move_action() {
    let mut w = World::new();
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![fact(slot(e(w.ada), "location", "tavern"), Seen::Pc, vec![])];
    rejected(validate(&p, &w.view), CheckKind::Agency);

    p.action = threshold_core::domain::ActionKind::Move;
    ok(validate(&p, &w.view));
    apply(&mut w, &p, None);
    assert_eq!(
        w.view.live_fact(w.ada, "location").unwrap().slot.value,
        "tavern"
    );
    assert!(
        !w.view.present().contains(&w.orsel),
        "Orsel stayed at the gate"
    );

    // A stake still cannot move the PC.
    let mut p = proposal(Verdict::Check);
    p.action = threshold_core::domain::ActionKind::Move;
    p.check = Some(check(
        None,
        slot(e(w.ada), "location", "roof"),
        slot(e(w.ada), "injury", "sprain"),
    ));
    rejected(validate(&p, &w.view), CheckKind::Agency);
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
