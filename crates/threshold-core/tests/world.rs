mod support;

use support::World;
use threshold_core::{
    domain::{ClaimDraft, Effect, EntityRef, Knowing, LedgerOp, Slot, Trigger},
    world::{Belief, MAX_EVENTS_PER_TURN, advance, beliefs, pc_knowledge},
};

/// Minute 0 is day 1 00:00, so day 4 18:00 is three days and 18 hours later.
const DAY_4_1800: u64 = 3 * 24 * 60 + 18 * 60;

fn slot(subject: u64, key: &str, value: &str) -> Slot {
    Slot {
        subject: EntityRef::Id(subject),
        key: key.into(),
        value: value.into(),
    }
}

fn schedule(at: u64, text: &str, effects: Vec<Effect>, seen_by: Vec<u64>) -> LedgerOp {
    LedgerOp::ScheduleEvent {
        text: text.into(),
        trigger: Trigger::At(at),
        actors: vec![],
        goal_id: None,
        cause_fact_id: None,
        effects,
        seen_by: seen_by.into_iter().map(EntityRef::Id).collect(),
    }
}

/// The fixture without its bell event, so each test controls every due event.
fn world() -> World {
    let mut w = World::new();
    w.commit(vec![LedgerOp::CancelEvent {
        event_id: w.bell,
        cause_fact_id: w.gate_locked,
    }]);
    assert!(w.view.events_scheduled.is_empty());
    w
}

/// Runs `advance`, commits its ops through the real store as one Turn, and
/// returns the ops.
fn advance_and_commit(w: &mut World, minutes: u32) -> Vec<LedgerOp> {
    let ops = advance(&w.view, minutes);
    w.commit(ops.clone());
    ops
}

fn fired(ops: &[LedgerOp]) -> Vec<u64> {
    ops.iter()
        .filter_map(|op| match op {
            LedgerOp::FireEvent { event_id } => Some(*event_id),
            _ => None,
        })
        .collect()
}

fn tell_event(w: &World, at: u64, value: &str, delay_min: u32) -> LedgerOp {
    schedule(
        at,
        "Brann sends word about the gate",
        vec![Effect::Tell {
            to: EntityRef::Id(w.ada),
            claim: ClaimDraft {
                speaker: EntityRef::Id(w.brann),
                slot: slot(w.gate, "locked", value),
                text: format!("Brann says the gate is locked: {value}"),
            },
            delay_min,
        }],
        vec![],
    )
}

fn gate_belief(beliefs: &[Belief], gate: u64) -> Option<&Belief> {
    beliefs
        .iter()
        .find(|b| b.subject == gate && b.key == "locked")
}

#[test]
fn event_fires_when_due_and_pc_absent() {
    let mut w = world();
    // Ada leaves for the market; Brann stays at the gate square.
    let ada_location = w.view.live_fact(w.ada, "location").unwrap().id;
    w.commit(vec![LedgerOp::SupersedeFact {
        old: ada_location,
        new: threshold_core::domain::FactDraft {
            slot: slot(w.ada, "location", "market"),
            text: "Ada walks to the market".into(),
            canon: threshold_core::domain::Canon::Established,
            source: threshold_core::domain::FactSource::Proposal,
            seen_by: vec![EntityRef::Id(w.ada)],
            causes: vec![],
        },
    }]);
    w.commit(vec![schedule(
        DAY_4_1800,
        "Brann bars the gate from inside",
        vec![Effect::SetFact {
            slot: slot(w.gate, "barred", "yes"),
            text: "The gate is barred from inside".into(),
            seen_by: vec![],
        }],
        vec![w.brann],
    )]);
    let event = w.view.events_scheduled[0].id;
    assert!(!w.view.present().contains(&w.brann));

    // One minute short of the trigger: nothing fires.
    let early = advance_and_commit(&mut w, (DAY_4_1800 - 1) as u32);
    assert!(fired(&early).is_empty());
    assert!(w.view.live_fact(w.gate, "barred").is_none());

    let ops = advance_and_commit(&mut w, 60);
    assert_eq!(ops[0], LedgerOp::AdvanceClock { minutes: 60 });
    assert_eq!(fired(&ops), vec![event]);

    let fresh = w.store.snapshot(w.campaign).unwrap();
    assert_eq!(fresh.clock_min, DAY_4_1800 - 1 + 60);
    assert!(fresh.events_scheduled.is_empty());
    let barred = fresh.live_fact(w.gate, "barred").unwrap();
    assert_eq!(barred.slot.value, "yes");
    assert_eq!(
        barred.source,
        threshold_core::domain::FactSource::WorldEvent
    );
    // Brann witnessed it; the absent PC has no knowledge row for it.
    assert!(
        fresh
            .knowledge
            .iter()
            .any(|k| k.holder == w.brann && k.fact_id == Some(barred.id))
    );
    assert!(
        !fresh
            .knowledge
            .iter()
            .any(|k| k.holder == w.ada && k.fact_id == Some(barred.id))
    );
    assert!(gate_belief(&pc_knowledge(&fresh), w.gate).is_none());
    let pc_barred = pc_knowledge(&fresh)
        .into_iter()
        .any(|b| b.subject == w.gate && b.key == "barred");
    assert!(!pc_barred);
}

#[test]
fn event_cap_per_turn() {
    let mut w = world();
    w.commit(
        (0..5)
            .map(|i| {
                schedule(
                    100 + i,
                    &format!("event {i}"),
                    vec![Effect::SetFact {
                        slot: slot(w.gate, &format!("mark_{i}"), "yes"),
                        text: format!("mark {i}"),
                        seen_by: vec![],
                    }],
                    vec![],
                )
            })
            .collect(),
    );
    let ids: Vec<u64> = w.view.events_scheduled.iter().map(|e| e.id).collect();
    assert_eq!(ids.len(), 5);
    assert_eq!(MAX_EVENTS_PER_TURN, 3);

    let first = advance_and_commit(&mut w, 200);
    assert_eq!(fired(&first), ids[..3].to_vec());
    let fresh = w.store.snapshot(w.campaign).unwrap();
    assert_eq!(fresh.events_scheduled.len(), 2);
    assert!(fresh.live_fact(w.gate, "mark_2").is_some());
    assert!(fresh.live_fact(w.gate, "mark_3").is_none());

    let second = advance_and_commit(&mut w, 0);
    assert_eq!(fired(&second), ids[3..].to_vec());
    let fresh = w.store.snapshot(w.campaign).unwrap();
    assert!(fresh.events_scheduled.is_empty());
    assert!(fresh.live_fact(w.gate, "mark_4").is_some());
}

#[test]
fn delayed_news_arrives_later() {
    let mut w = world();
    w.commit(vec![tell_event(&w, 10, "no", 120)]);
    advance_and_commit(&mut w, 10);
    let fresh = w.store.snapshot(w.campaign).unwrap();
    assert_eq!(fresh.claims.len(), 1);
    assert!(gate_belief(&beliefs(&fresh, w.ada), w.gate).is_none());

    advance_and_commit(&mut w, 60);
    let fresh = w.store.snapshot(w.campaign).unwrap();
    assert!(gate_belief(&beliefs(&fresh, w.ada), w.gate).is_none());

    advance_and_commit(&mut w, 60);
    let fresh = w.store.snapshot(w.campaign).unwrap();
    let belief = gate_belief(&beliefs(&fresh, w.ada), w.gate)
        .cloned()
        .unwrap();
    assert_eq!(belief.value, "no");
    assert_eq!(belief.how, Knowing::Told);
}

#[test]
fn told_claim_can_be_false() {
    let mut w = world();
    w.commit(vec![tell_event(&w, 5, "no", 0)]);
    advance_and_commit(&mut w, 5);
    let fresh = w.store.snapshot(w.campaign).unwrap();

    let belief = gate_belief(&pc_knowledge(&fresh), w.gate).cloned().unwrap();
    assert_eq!(
        belief,
        Belief {
            subject: w.gate,
            key: "locked".into(),
            value: "no".into(),
            text: "Brann says the gate is locked: no".into(),
            how: Knowing::Told,
            source: Some(w.brann),
        }
    );
    // World Truth is unchanged: the gate is still locked, by the same fact.
    let live = fresh.live_fact(w.gate, "locked").unwrap();
    assert_eq!(live.id, w.gate_locked);
    assert_eq!(live.slot.value, "yes");
    assert_eq!(fresh.facts.len(), w.view.facts.len());
}

#[test]
fn later_witnessed_fact_replaces_belief() {
    let mut w = world();
    w.commit(vec![tell_event(&w, 5, "no", 0)]);
    advance_and_commit(&mut w, 5);
    // A later event the PC witnesses opens the gate; it supersedes the
    // live fact and becomes Ada's latest Belief on that slot.
    w.commit(vec![schedule(
        20,
        "The gate swings open",
        vec![Effect::SetFact {
            slot: slot(w.gate, "locked", "no"),
            text: "The gate swings open".into(),
            seen_by: vec![],
        }],
        vec![w.ada],
    )]);
    let ops = advance_and_commit(&mut w, 20);
    assert!(
        ops.iter()
            .any(|op| matches!(op, LedgerOp::SupersedeFact { old, .. } if *old == w.gate_locked))
    );
    let fresh = w.store.snapshot(w.campaign).unwrap();
    assert_eq!(fresh.live_fact(w.gate, "locked").unwrap().slot.value, "no");
    let belief = gate_belief(&pc_knowledge(&fresh), w.gate).cloned().unwrap();
    assert_eq!(belief.how, Knowing::Witnessed);
    assert_eq!(belief.source, None);
}

#[test]
fn earlier_scheduled_event_bends_persistent_fact() {
    let mut w = world();
    let material = w.view.live_fact(w.gate, "material").unwrap();
    assert_eq!(material.canon, threshold_core::domain::Canon::Persistent);
    w.commit(vec![schedule(
        15,
        "Smiths replace the gate",
        vec![Effect::SetFact {
            slot: slot(w.gate, "material", "oak"),
            text: "The gate is oak now".into(),
            seen_by: vec![],
        }],
        vec![],
    )]);
    advance_and_commit(&mut w, 15);
    let fresh = w.store.snapshot(w.campaign).unwrap();
    assert_eq!(
        fresh.live_fact(w.gate, "material").unwrap().slot.value,
        "oak"
    );
}

#[test]
fn when_trigger_fires_on_live_slot() {
    let mut w = world();
    w.commit(vec![LedgerOp::ScheduleEvent {
        text: "Orsel leaves once the gate is open".into(),
        trigger: Trigger::When(slot(w.gate, "Locked", "No")),
        actors: vec![EntityRef::Id(w.orsel)],
        goal_id: None,
        cause_fact_id: None,
        effects: vec![Effect::ShiftAttitude {
            from: EntityRef::Id(w.orsel),
            to: EntityRef::Id(w.ada),
            delta: 1,
        }],
        seen_by: vec![],
    }]);
    assert!(fired(&advance_and_commit(&mut w, 30)).is_empty());
    w.commit(vec![LedgerOp::SupersedeFact {
        old: w.gate_locked,
        new: threshold_core::domain::FactDraft {
            slot: slot(w.gate, "locked", "no"),
            text: "The gate opens".into(),
            canon: threshold_core::domain::Canon::Established,
            source: threshold_core::domain::FactSource::Proposal,
            seen_by: vec![],
            causes: vec![],
        },
    }]);
    let ops = advance_and_commit(&mut w, 0);
    assert_eq!(fired(&ops).len(), 1);
    let fresh = w.store.snapshot(w.campaign).unwrap();
    let attitude = fresh
        .attitudes
        .iter()
        .find(|a| a.from == w.orsel && a.to == w.ada)
        .unwrap();
    assert_eq!(attitude.attitude, -3);
}

fn set_fact(subject: u64, key: &str, value: &str) -> Effect {
    Effect::SetFact {
        slot: slot(subject, key, value),
        text: format!("{key} {value}"),
        seen_by: vec![],
    }
}

#[test]
fn self_colliding_event_does_not_block_queue() {
    let mut w = world();
    w.commit(vec![
        schedule(
            10,
            "The gate is marked twice",
            vec![
                set_fact(w.gate, "mark", "first"),
                set_fact(w.gate, "Mark", "second"),
            ],
            vec![],
        ),
        schedule(
            11,
            "Another mark",
            vec![set_fact(w.gate, "other", "yes")],
            vec![],
        ),
    ]);
    let ids: Vec<u64> = w.view.events_scheduled.iter().map(|e| e.id).collect();
    let ops = advance_and_commit(&mut w, 20);
    assert_eq!(fired(&ops), ids);
    let mark_writes = ops
        .iter()
        .filter(|op| matches!(op, LedgerOp::AddFact { slot, .. } if slot.key == "mark"))
        .count();
    assert_eq!(mark_writes, 1);
    let fresh = w.store.snapshot(w.campaign).unwrap();
    assert!(fresh.events_scheduled.is_empty());
    assert_eq!(
        fresh.live_fact(w.gate, "mark").unwrap().slot.value,
        "second"
    );
    assert!(fresh.live_fact(w.gate, "other").is_some());
}

#[test]
fn cross_event_collision_waits_one_turn() {
    let mut w = world();
    w.commit(vec![
        schedule(10, "Mark a", vec![set_fact(w.gate, "mark", "a")], vec![]),
        schedule(11, "Mark b", vec![set_fact(w.gate, "mark", "b")], vec![]),
        schedule(12, "Other", vec![set_fact(w.gate, "other", "yes")], vec![]),
    ]);
    let ids: Vec<u64> = w.view.events_scheduled.iter().map(|e| e.id).collect();
    let first = advance_and_commit(&mut w, 20);
    assert_eq!(fired(&first), ids[..1].to_vec());
    let fresh = w.store.snapshot(w.campaign).unwrap();
    assert_eq!(fresh.live_fact(w.gate, "mark").unwrap().slot.value, "a");
    assert!(fresh.live_fact(w.gate, "other").is_none());

    let second = advance_and_commit(&mut w, 0);
    assert_eq!(fired(&second), ids[1..].to_vec());
    let fresh = w.store.snapshot(w.campaign).unwrap();
    assert_eq!(fresh.live_fact(w.gate, "mark").unwrap().slot.value, "b");
    assert!(fresh.live_fact(w.gate, "other").is_some());
    assert!(fresh.events_scheduled.is_empty());
}

#[test]
fn when_event_fires_after_earlier_at_event() {
    let mut w = world();
    // The When event has the lower id, but its time is the new clock (10),
    // so the At(5) event fires first; the At(10) event ties on time and
    // fires after it by id.
    w.commit(vec![LedgerOp::ScheduleEvent {
        text: "Orsel notices the locked gate".into(),
        trigger: Trigger::When(slot(w.gate, "locked", "yes")),
        actors: vec![],
        goal_id: None,
        cause_fact_id: None,
        effects: vec![],
        seen_by: vec![],
    }]);
    w.commit(vec![
        schedule(5, "Early", vec![], vec![]),
        schedule(10, "On time", vec![], vec![]),
    ]);
    let ids: Vec<u64> = w.view.events_scheduled.iter().map(|e| e.id).collect();
    let ops = advance_and_commit(&mut w, 10);
    assert_eq!(fired(&ops), vec![ids[1], ids[0], ids[2]]);
}

#[test]
fn late_news_does_not_override_newer_witness() {
    let mut w = world();
    // Said at minute 10, arrives at 200.
    w.commit(vec![tell_event(&w, 10, "no", 190)]);
    advance_and_commit(&mut w, 10);
    // At minute 100 the PC sees the gate chained.
    w.commit(vec![schedule(
        100,
        "Orsel chains the gate",
        vec![set_fact(w.gate, "locked", "chained")],
        vec![w.ada],
    )]);
    advance_and_commit(&mut w, 90);
    advance_and_commit(&mut w, 110);
    let fresh = w.store.snapshot(w.campaign).unwrap();
    assert_eq!(fresh.clock_min, 210);
    let belief = gate_belief(&pc_knowledge(&fresh), w.gate).cloned().unwrap();
    assert_eq!(belief.value, "chained");
    assert_eq!(belief.how, Knowing::Witnessed);
}

#[test]
fn same_learned_time_goes_to_row_written_last() {
    let mut w = world();
    let tell = |speaker: u64, value: &str, delay_min: u32| Effect::Tell {
        to: EntityRef::Id(w.ada),
        claim: ClaimDraft {
            speaker: EntityRef::Id(speaker),
            slot: slot(w.gate, "locked", value),
            text: format!("the gate is {value}"),
        },
        delay_min,
    };
    // Both learned at minute 10; Brann's row arrives later but is written
    // first, so Orsel's row (written last) is the Belief.
    let effects = vec![tell(w.brann, "open", 30), tell(w.orsel, "shut", 0)];
    w.commit(vec![schedule(10, "Two voices", effects, vec![])]);
    advance_and_commit(&mut w, 60);
    let fresh = w.store.snapshot(w.campaign).unwrap();
    let belief = gate_belief(&pc_knowledge(&fresh), w.gate).cloned().unwrap();
    assert_eq!(belief.value, "shut");
    assert_eq!(belief.source, Some(w.orsel));
}
