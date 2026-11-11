//! Bug pass 5, M1: an NPC who leaves the PC's place leaves through a
//! structured field (`leaves`, an event's `moves_actor_to`), the core writes
//! the location, and the narrator reads who is here after the Turn.

mod support;

use support::*;
use threshold_core::{
    CallKind, RecordedGm,
    context::seen_present,
    domain::{
        AWAY, ActionKind, DecisionProposal, EntityKind, EventDraft, Leave, LedgerOp, Seen, Trigger,
        Verdict, WorldSetup,
    },
    domain::{EffectEntry, EventEntry},
    rules::D10,
    setup::commit_world,
    store::WorldView,
    turn::Engine,
    world::pc_knowledge,
};

fn json(p: &DecisionProposal) -> String {
    serde_json::to_string(p).unwrap()
}

/// The gate world, where the PC sees Orsel and Brann stand with it.
fn gate() -> World {
    let mut w = World::new();
    let ada = w.ada;
    let know: Vec<LedgerOp> = [w.orsel, w.brann]
        .into_iter()
        .map(|npc| LedgerOp::Know {
            holder: threshold_core::domain::EntityRef::Id(ada),
            fact_id: Some(w.view.live_fact(npc, "location").unwrap().id),
            claim_id: None,
            how: threshold_core::domain::Knowing::Witnessed,
            arrives_at: 0,
        })
        .collect();
    w.commit(know);
    w
}

fn leave(npc: u64, to: &str) -> Leave {
    Leave {
        npc: e(npc),
        to: to.into(),
    }
}

/// Plays `p` with `intent` on `w`, then a plain look around; returns the
/// view after both and the narrator's two prompts.
fn play(w: World, p: &DecisionProposal, intent: &str) -> (WorldView, Vec<String>) {
    let gm = RecordedGm::new(
        vec![json(p), json(&proposal(Verdict::Allowed))],
        vec!["One.".into(), "Two.".into()],
    );
    let campaign = w.campaign;
    let mut engine = Engine::new(
        w.store,
        Box::new(gm.clone()),
        Box::new(D10),
        vec![],
        campaign,
    );
    engine
        .play_turn("t1".into(), intent, &mut |_: &str| {})
        .unwrap_or_else(|e| panic!("{e}"));
    let after = engine.store().snapshot(campaign).unwrap();
    engine
        .play_turn("t2".into(), "I look around.", &mut |_: &str| {})
        .unwrap();
    let prompts = gm
        .calls()
        .into_iter()
        .filter(|c| c.kind == CallKind::Narrate)
        .map(|c| {
            format!(
                "{}{}{}{}",
                c.prompt.system, c.prompt.stable, c.prompt.scene, c.prompt.recent
            )
        })
        .collect();
    (after, prompts)
}

/// QA: Ludo Brenn strode to the door in the story, and Here kept him in the
/// room. A `leaves` entry moves him: the world, the PC's sight, and the
/// narrator agree.
#[test]
fn a_leave_moves_the_npc_out_for_everyone_present() {
    let w = gate();
    let (ada, orsel, brann) = (w.ada, w.orsel, w.brann);
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Talk;
    p.leaves = vec![leave(brann, AWAY)];
    let (view, narrations) = play(w, &p, "I tell Brann his cart is loose.");
    let at = view.live_fact(brann, "location").unwrap();
    assert_eq!(at.slot.value, "");
    assert_eq!(at.text, "Brann left.");
    assert!(at.seen_by.contains(&ada) && at.seen_by.contains(&orsel));
    assert!(!seen_present(&view).contains(&brann));
    assert!(
        narrations[0].contains("- Brann leaves.\n"),
        "{}",
        narrations[0]
    );
    assert!(
        narrations[0]
            .contains("- Here after this Turn besides the PC: Orsel. Nobody else comes or goes.\n"),
        "{}",
        narrations[0]
    );
    // The next Narration has Brann neither present nor leaving again.
    assert!(!narrations[1].contains("Brann leaves"), "{}", narrations[1]);
    let present = narrations[1]
        .lines()
        .find(|l| l.starts_with("Present:"))
        .unwrap_or_default();
    assert!(!present.contains("Brann"), "{}", narrations[1]);
}

/// A destination the PC can name is told and learned.
#[test]
fn a_leave_to_a_place_the_pc_can_name_tells_the_place() {
    let mut w = gate();
    let (ada, brann) = (w.ada, w.brann);
    let market = w.add_place("Market");
    let mut p = proposal(Verdict::Allowed);
    p.leaves = vec![leave(brann, &format!("E{market}"))];
    let (view, narrations) = play(w, &p, "I send Brann to the market.");
    let at = view.live_fact(brann, "location").unwrap();
    assert_eq!(at.slot.value, "market");
    assert_eq!(at.text, "Brann went to Market.");
    assert!(at.seen_by.contains(&ada));
    assert!(
        narrations[0].contains("- Brann leaves for Market.\n"),
        "{}",
        narrations[0]
    );
}

/// A destination the PC cannot name stays the world's: the PC sees Brann
/// leave, as for a World Event (data shape 13, B1).
#[test]
fn a_leave_to_a_place_the_pc_cannot_name_shows_a_departure_only() {
    let mut w = gate();
    let (ada, orsel, brann) = (w.ada, w.orsel, w.brann);
    w.commit(vec![LedgerOp::AddEntity {
        local: None,
        kind: EntityKind::Place,
        name: "Smugglers Cove".into(),
        age: None,
        other_names: vec![],
        age_confirmed: false,
    }]);
    let cove = w.view.entity_by_name("Smugglers Cove").unwrap().id;
    let mut p = proposal(Verdict::Allowed);
    p.leaves = vec![leave(brann, &format!("E{cove}"))];
    let (view, narrations) = play(w, &p, "I tell Brann to hide.");
    let at = view.live_fact(brann, "location").unwrap();
    assert_eq!(at.slot.value, "smugglers cove");
    assert!(at.seen_by.contains(&orsel) && !at.seen_by.contains(&ada));
    let belief = pc_knowledge(&view)
        .into_iter()
        .find(|b| b.subject == brann && b.key == "location")
        .unwrap();
    assert_eq!(belief.value, "");
    assert!(!seen_present(&view).contains(&brann));
    assert!(
        narrations[0].contains("- Brann leaves.\n"),
        "{}",
        narrations[0]
    );
    assert!(!narrations[0].contains("Smugglers"), "{}", narrations[0]);
}

/// In QA every NPC had a scheduled event, so a departure that counted as a
/// cancel would fail every Turn. The event still fires wherever its actor
/// is.
#[test]
fn a_leave_of_an_actor_of_a_scheduled_event_is_no_cancel() {
    let w = gate();
    let orsel = w.orsel;
    let mut p = proposal(Verdict::Allowed);
    p.leaves = vec![leave(orsel, AWAY)];
    let (view, _) = play(w, &p, "I tell Orsel the mayor wants him.");
    assert_eq!(view.live_fact(orsel, "location").unwrap().slot.value, "");
    assert!(view.events_scheduled.iter().any(|ev| ev.actors == [orsel]));
}

/// The core takes model output away and adds none: a leave of an NPC who
/// is not here, of the PC, twice the same NPC, or to the NPC's own place
/// goes, with a note.
#[test]
fn settle_drops_a_leave_the_pc_cannot_see_or_that_changes_nothing() {
    let mut w = gate();
    let (ada, pip, brann, orsel) = (w.ada, w.pip, w.brann, w.orsel);
    let here = w.add_place("gate square");
    let mut p = proposal(Verdict::Allowed);
    p.leaves = vec![
        leave(pip, AWAY),
        leave(ada, AWAY),
        leave(orsel, &format!("E{here}")),
        leave(brann, AWAY),
        leave(brann, "E999"),
    ];
    let (settled, notes) = threshold_core::checks::settle(p, &w.view);
    assert_eq!(settled.leaves, [leave(brann, AWAY)], "{notes:?}");
    assert_eq!(notes.len(), 4, "{notes:?}");
}

/// A fact the model writes on the location of an NPC who leaves goes: the
/// core writes that slot from `leaves`.
#[test]
fn a_location_fact_of_a_leaving_npc_goes() {
    let w = gate();
    let brann = w.brann;
    let mut p = proposal(Verdict::Allowed);
    p.leaves = vec![leave(brann, AWAY)];
    p.facts = vec![fact(
        slot(e(brann), "location", "tavern"),
        Seen::Present,
        vec![],
    )];
    let (settled, notes) = threshold_core::checks::settle(p, &w.view);
    assert!(settled.facts.is_empty(), "{notes:?}");
}

/// A scheduled event says where its actor is when it happens; the core
/// adds the actor's location effect.
#[test]
fn a_scheduled_event_carries_its_actors_place() {
    let w = gate();
    let (orsel, brann) = (w.orsel, w.brann);
    let mut p = proposal(Verdict::Allowed);
    p.schedule = Some(EventDraft {
        in_minutes: 30,
        actor: e(brann),
        goal: None,
        text: "Brann walks off to fetch the guard".into(),
        effects: vec![],
        seen_by: vec![e(orsel)],
        public: false,
        moves_actor_to: Some(AWAY.into()),
    });
    let (view, _) = play(w, &p, "I insult Brann.");
    let event = view
        .events_scheduled
        .iter()
        .find(|ev| ev.text.starts_with("Brann walks off"))
        .unwrap();
    let effect = event.effects.last().unwrap();
    let threshold_core::domain::Effect::SetFact { slot, text, .. } = effect else {
        panic!("{effect:?}");
    };
    assert_eq!(
        (
            slot.subject,
            slot.key.as_str(),
            slot.value.as_str(),
            text.as_str()
        ),
        (brann, "location", "", "Brann left.")
    );
}

/// The World Build's `moves_actor_to` becomes the actor's location effect
/// in the world file's setup, last, so it wins over another write of the
/// slot.
#[test]
fn a_world_event_moves_its_actor() {
    let w = gate();
    let mut store = w.store;
    let campaign = w.campaign;
    let setup = WorldSetup {
        events: vec![
            EventEntry {
                at: 20,
                actor: "Brann".into(),
                text: "Brann walks the towpath".into(),
                effects: vec![EffectEntry {
                    subject: "Brann".into(),
                    key: "fatigue".into(),
                    value: "tired".into(),
                }],
                seen_by: vec!["Orsel".into()],
                public: false,
                moves_actor_to: Some("Gate".into()),
            },
            EventEntry {
                at: 40,
                actor: "Orsel".into(),
                text: "Orsel slips away".into(),
                effects: vec![],
                seen_by: vec!["Brann".into()],
                public: false,
                moves_actor_to: Some(AWAY.into()),
            },
        ],
        ..WorldSetup::default()
    };
    commit_world(&mut store, campaign, &setup).unwrap();
    let view = store.snapshot(campaign).unwrap();
    let moves: Vec<(String, String)> = view
        .events_scheduled
        .iter()
        .filter(|ev| matches!(ev.trigger, Trigger::At(at) if at == 20 || at == 40))
        .filter_map(|ev| match ev.effects.last()? {
            threshold_core::domain::Effect::SetFact { slot, text, .. }
                if slot.key == "location" =>
            {
                Some((slot.value.clone(), text.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        moves,
        [
            ("gate".to_owned(), "Brann went to Gate.".to_owned()),
            (String::new(), "Orsel left.".to_owned()),
        ]
    );
}
