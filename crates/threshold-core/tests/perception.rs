//! What the PC perceives of a World Event that fires in front of it
//! (review of bug pass 4: B1, M1, M2).

mod support;

use std::collections::HashSet;

use support::*;
use threshold_core::{
    CallKind, RecordedGm,
    context::seen_present,
    domain::{
        ActionKind, Canon, DecisionProposal, Effect, EntityKind, EntityRef, LedgerOp, Slot,
        Trigger, Verdict,
    },
    player::Marks,
    rules::D10,
    store::{Store, TurnId, WorldView},
    turn::Engine,
    view::{entity_card, journal_view, ooc_sheet, play_view},
    world::{PcSight, pc_knowledge, pc_sight},
};

fn json(p: &DecisionProposal) -> String {
    serde_json::to_string(p).unwrap()
}

/// Adds a place that no Adventure Seed entry, Belief or Narration names:
/// the PC cannot name it.
fn add_unnamed_place(w: &mut World, name: &str) {
    w.commit(vec![LedgerOp::AddEntity {
        local: None,
        kind: EntityKind::Place,
        name: name.into(),
        age: None,
        other_names: vec![],
    }]);
}

/// Schedules a World Event at minute 10 whose actor is Orsel, who stands
/// with the PC, seen by Brann, with `text` and one effect per
/// `(subject, key, value)`.
fn schedule(w: &mut World, text: &str, effects: &[(u64, &str, &str)]) {
    let brann = w.brann;
    let orsel = w.orsel;
    w.commit(vec![LedgerOp::ScheduleEvent {
        text: text.into(),
        trigger: Trigger::At(10),
        actors: vec![EntityRef::Id(orsel)],
        goal_id: None,
        cause_fact_id: None,
        effects: effects
            .iter()
            .map(|(subject, key, value)| Effect::SetFact {
                slot: Slot {
                    subject: EntityRef::Id(*subject),
                    key: (*key).into(),
                    value: (*value).into(),
                },
                text: format!("{key} is now {value}."),
                seen_by: vec![],
            })
            .collect(),
        seen_by: vec![EntityRef::Id(brann)],
    }]);
}

/// The engine after one wait over the scheduled events, and the narrator's
/// `recent` block.
fn wait(w: World) -> (Engine, u64, String) {
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Wait;
    let gm = RecordedGm::new(vec![json(&p)], vec!["You wait.".into()]);
    let campaign = w.campaign;
    let mut engine = Engine::new(
        w.store,
        Box::new(gm.clone()),
        Box::new(D10),
        vec![],
        campaign,
    );
    engine
        .play_turn("wait".into(), "I wait an hour.", &mut |_| {})
        .unwrap();
    let recent = gm
        .calls()
        .into_iter()
        .filter(|c| c.kind == CallKind::Narrate)
        .map(|c| c.prompt.recent)
        .next_back()
        .expect("a narration call");
    (engine, campaign, recent)
}

fn sight(store: &Store, campaign: u64) -> PcSight {
    let view = store.snapshot(campaign).unwrap();
    let turns = store.turns(campaign).unwrap();
    let reverted: HashSet<TurnId> = store
        .reverted_turns(campaign)
        .unwrap()
        .into_iter()
        .collect();
    pc_sight(&view, &turns, &reverted).unwrap()
}

/// Every Player screen and the OOC sheet, as text.
fn screens(store: &Store, campaign: u64, entity: u64) -> String {
    let sight = sight(store, campaign);
    let marks = Marks::default();
    [
        serde_json::to_string(&play_view(&sight, &marks)).unwrap(),
        serde_json::to_string(&journal_view(&sight, &marks)).unwrap(),
        serde_json::to_string(&entity_card(&sight, &marks, entity)).unwrap(),
        ooc_sheet(&sight),
    ]
    .concat()
}

fn view_of(engine: &Engine, campaign: u64) -> WorldView {
    engine.store().snapshot(campaign).unwrap()
}

/// Review B1: an event moves Orsel, in front of the PC, to a place the PC
/// cannot name. The PC sees Orsel leave and learns no destination: no
/// Belief holds the place, no screen and no prompt of the narrator names
/// it, and Orsel is no longer here.
#[test]
fn a_witnessed_move_to_a_place_the_pc_cannot_name_shows_a_departure_only() {
    let mut w = World::new();
    add_unnamed_place(&mut w, "Smugglers Cellar");
    let orsel = w.orsel;
    schedule(
        &mut w,
        "Orsel slips down to the Smugglers Cellar",
        &[(orsel, "location", "Smugglers Cellar")],
    );
    let (engine, campaign, recent) = wait(w);
    let view = view_of(&engine, campaign);
    assert_eq!(
        view.live_fact(orsel, "location").unwrap().slot.value,
        "smugglers cellar",
        "the event fired"
    );
    assert!(
        !pc_knowledge(&view)
            .iter()
            .any(|b| b.subject == orsel && b.value.contains("cellar")),
        "the PC learned the destination"
    );
    assert!(!seen_present(&view).contains(&orsel), "Orsel is still here");
    assert!(recent.contains("- Orsel leaves.\n"), "{recent}");
    assert!(!recent.to_lowercase().contains("cellar"), "{recent}");
    let all = screens(engine.store(), campaign, orsel);
    assert!(!all.to_lowercase().contains("cellar"), "{all}");
    let sight = sight(engine.store(), campaign);
    let here = play_view(&sight, &Marks::default()).here;
    assert!(here.iter().all(|who| who.entity_id != orsel), "{here:?}");
    let card = entity_card(&sight, &Marks::default(), orsel).unwrap();
    assert!(
        card.facts.iter().any(|f| f.text == "Orsel leaves."),
        "{card:?}"
    );
}

/// A destination the PC can name is seen: the Adventure Seed place Gate.
#[test]
fn a_witnessed_move_to_a_place_the_pc_can_name_shows_the_destination() {
    let mut w = World::new();
    let orsel = w.orsel;
    schedule(
        &mut w,
        "Orsel walks to the Gate",
        &[(orsel, "location", "Gate")],
    );
    let (engine, campaign, recent) = wait(w);
    let view = view_of(&engine, campaign);
    assert!(
        pc_knowledge(&view)
            .iter()
            .any(|b| b.subject == orsel && b.key == "location" && b.value == "gate"),
        "{:?}",
        pc_knowledge(&view)
    );
    assert!(!seen_present(&view).contains(&orsel));
    assert!(!recent.contains("Orsel leaves."), "{recent}");
}

/// The departure record is no world truth: the GM's prompt never shows it,
/// and the world keeps Orsel at the cellar.
#[test]
fn the_departure_record_stays_out_of_the_gm_prompt() {
    let mut w = World::new();
    add_unnamed_place(&mut w, "Smugglers Cellar");
    let orsel = w.orsel;
    schedule(
        &mut w,
        "Orsel slips away",
        &[(orsel, "location", "Smugglers Cellar")],
    );
    let (engine, campaign, _) = wait(w);
    let view = view_of(&engine, campaign);
    let record = view
        .live_facts
        .iter()
        .find(|f| f.slot.subject == orsel && f.text == "Orsel leaves.")
        .expect("the departure record");
    assert_eq!(record.canon, Canon::Possible);
    let prompt = threshold_core::context::proposal_prompt(&view, &[], "I ask Orsel where he went.");
    let text = [
        &prompt.system,
        &prompt.stable,
        &prompt.scene,
        &prompt.recent,
    ]
    .map(|s| s.as_str())
    .concat();
    assert!(!text.contains("Orsel leaves."), "{text}");
    assert!(text.to_lowercase().contains("smugglers cellar"), "{text}");
}
