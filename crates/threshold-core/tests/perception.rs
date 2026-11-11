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
/// `(subject, key, value)`. Not public: only Brann notices what it changes.
fn schedule(w: &mut World, text: &str, effects: &[(u64, &str, &str)]) {
    schedule_event(w, text, effects, false);
}

/// `schedule`, for a public event: everyone at the place sees or hears
/// what it changes.
fn schedule_public(w: &mut World, text: &str, effects: &[(u64, &str, &str)]) {
    schedule_event(w, text, effects, true);
}

fn schedule_event(w: &mut World, text: &str, effects: &[(u64, &str, &str)], public: bool) {
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
        public,
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

/// Review of bug pass 4, m2: the departure record named its entity by the
/// view's last six Turns, the card by every standing Turn. Kestrel, whom
/// only an old Narration names, left as "Someone" on her own card. The
/// record now names her, as the card does.
#[test]
fn a_departure_names_an_npc_whom_an_old_narration_named() {
    let mut w = World::new();
    add_unnamed_place(&mut w, "Smugglers Cellar");
    let ada = w.ada;
    w.commit(vec![LedgerOp::AddEntity {
        local: None,
        kind: EntityKind::Npc,
        name: "Kestrel".into(),
        age: Some(40),
        other_names: vec![],
    }]);
    let kestrel = w.view.entity_by_name("Kestrel").unwrap().id;
    w.commit(vec![LedgerOp::AddFact {
        slot: Slot {
            subject: EntityRef::Id(kestrel),
            key: "location".into(),
            value: "gate square".into(),
        },
        text: "Kestrel is at the gate square.".into(),
        canon: Canon::Established,
        source: threshold_core::domain::FactSource::Proposal,
        seen_by: vec![EntityRef::Id(ada)],
        causes: vec![],
    }]);
    let narrate = |w: &mut World, text: &str| {
        let turn = w.store.turns(w.campaign).unwrap().last().unwrap().id;
        w.store.set_narration(turn, text).unwrap();
    };
    narrate(&mut w, "Kestrel, a drover, tips her hat to you.");
    for _ in 0..6 {
        w.commit(vec![]);
        narrate(&mut w, "The wind turns.");
    }
    schedule(
        &mut w,
        "Orsel sends the drover off",
        &[(kestrel, "location", "Smugglers Cellar")],
    );
    let (engine, campaign, _) = wait(w);
    let view = view_of(&engine, campaign);
    let record = view
        .live_facts
        .iter()
        .find(|f| f.slot.subject == kestrel && f.slot.value.is_empty())
        .expect("the departure record");
    assert!(record.text.starts_with("Kestrel "), "{}", record.text);
    let card = entity_card(&sight(engine.store(), campaign), &Marks::default(), kestrel).unwrap();
    assert_eq!(card.who.name.as_deref(), Some("Kestrel"));
}

/// Schedules a World Event at `at` whose actor is Orsel, seen by Brann,
/// that moves Orsel to `place`.
fn move_orsel(w: &mut World, at: u64, text: &str, place: &str) {
    let (brann, orsel) = (w.brann, w.orsel);
    w.commit(vec![LedgerOp::ScheduleEvent {
        text: text.into(),
        trigger: Trigger::At(at),
        actors: vec![EntityRef::Id(orsel)],
        goal_id: None,
        cause_fact_id: None,
        effects: vec![Effect::SetFact {
            slot: Slot {
                subject: EntityRef::Id(orsel),
                key: "location".into(),
                value: place.into(),
            },
            text: format!("Orsel is at {place}."),
            seen_by: vec![],
        }],
        seen_by: vec![EntityRef::Id(brann)],
        public: false,
    }]);
}

/// The live departure records of `entity`: Possible location facts with
/// no place.
fn departure_records(view: &WorldView, entity: u64) -> usize {
    view.live_facts
        .iter()
        .filter(|f| f.slot.subject == entity && f.slot.key == "location" && f.slot.value.is_empty())
        .count()
}

/// Review of bug pass 4, M2: Orsel leaves for a place the PC cannot name,
/// walks back in alone, and leaves again. His return is perceptible: the
/// wait stops at it, the PC's Belief puts him here again, the departure
/// record is superseded and Here lists him. His second departure leaves
/// one live departure record, never two.
#[test]
fn an_npc_who_leaves_and_walks_back_in_is_seen_both_ways() {
    let mut w = World::new();
    add_unnamed_place(&mut w, "Smugglers Cellar");
    let orsel = w.orsel;
    move_orsel(&mut w, 10, "Orsel slips away", "Smugglers Cellar");
    move_orsel(&mut w, 20, "Orsel comes back in", "gate square");
    move_orsel(&mut w, 40, "Orsel slips away again", "Smugglers Cellar");
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Wait;
    let gm = RecordedGm::new(vec![], vec![]);
    for _ in 0..3 {
        gm.push_proposal(json(&p));
        gm.push_narration("You wait.".into());
    }
    let campaign = w.campaign;
    let mut engine = Engine::new(
        w.store,
        Box::new(gm.clone()),
        Box::new(D10),
        vec![],
        campaign,
    );
    let mut wait = |n: u32| {
        engine
            .play_turn(format!("wait-{n}"), "I wait an hour.", &mut |_| {})
            .unwrap();
    };
    wait(1);
    wait(2);
    let view = view_of(&engine, campaign);
    assert_eq!(view.clock_min, 20, "the wait stops at the return");
    assert!(
        pc_knowledge(&view)
            .iter()
            .any(|b| b.subject == orsel && b.key == "location" && b.value == "gate square"),
        "{:?}",
        pc_knowledge(&view)
    );
    assert!(seen_present(&view).contains(&orsel));
    assert_eq!(
        departure_records(&view, orsel),
        0,
        "the record is superseded"
    );
    let sight = sight(engine.store(), campaign);
    let marks = Marks::default();
    assert!(
        play_view(&sight, &marks)
            .here
            .iter()
            .any(|who| who.entity_id == orsel)
    );
    let card = entity_card(&sight, &marks, orsel).unwrap();
    assert!(
        card.facts.iter().all(|f| !f.text.contains("Orsel le")),
        "{card:?}"
    );
    let mut engine = engine;
    engine
        .play_turn("wait-3".into(), "I wait an hour.", &mut |_| {})
        .unwrap();
    let view = view_of(&engine, campaign);
    assert_eq!(view.clock_min, 40);
    assert!(!seen_present(&view).contains(&orsel));
    assert_eq!(departure_records(&view, orsel), 1, "one live record");
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

/// Review of bug pass 4, blocker: the text rule of 9459297 read meaning
/// from a bag of words. Each event below "states" its effect's value word
/// for word, and none of them makes the effect true in front of the PC.
/// A private event (not `public`) teaches the PC none of them: no Belief,
/// no card line, no narrator outcome line, and the value stays a secret,
/// so the guard drops the text that holds it.
#[test]
fn a_private_event_teaches_no_effect_its_text_seems_to_state() {
    // (text, subject, key, value); the subject is Orsel, Brann or Pip.
    let cases: [(&str, &str, &str, &str); 11] = [
        (
            "Orsel paid no attention to the crowd",
            "Orsel",
            "debt_status",
            "paid",
        ),
        (
            "Orsel has not paid the reeve",
            "Orsel",
            "debt_status",
            "paid",
        ),
        ("Orsel says yes to another ale", "Orsel", "guilty", "yes"),
        ("Orsel stares with dead eyes", "Pip", "status", "dead"),
        (
            "Orsel swears he will never close the gate",
            "Orsel",
            "plan",
            "close the gate",
        ),
        (
            "Orsel asks Brann whether the toll is raised",
            "Orsel",
            "toll",
            "raised",
        ),
        (
            "Orsel denies that Brann is guilty",
            "Brann",
            "guilt",
            "guilty",
        ),
        (
            "Orsel laughs at the rumor that Brann is married",
            "Brann",
            "marital",
            "married",
        ),
        (
            "Orsel dreams of being free one day",
            "Orsel",
            "status",
            "free",
        ),
        ("Orsel jokes that Pip is lost", "Pip", "fate", "lost"),
        (
            "Orsel forgets whether he sold the barge",
            "Orsel",
            "barge",
            "sold",
        ),
    ];
    for (text, subject, key, value) in cases {
        let mut w = World::new();
        let subject = w.view.entity_by_name(subject).unwrap().id;
        schedule(&mut w, text, &[(subject, key, value)]);
        let (engine, campaign, recent) = wait(w);
        let view = view_of(&engine, campaign);
        assert!(
            !pc_knowledge(&view)
                .iter()
                .any(|b| b.subject == subject && b.key == key),
            "{text}: the PC learned {key}"
        );
        let line = format!("{key} is now {value}.");
        assert!(!recent.contains(&line), "{text}: {recent}");
        assert!(
            !recent.contains(text),
            "{text}: the guard told it: {recent}"
        );
        let all = screens(engine.store(), campaign, subject);
        assert!(!all.contains(&line), "{text}: {all}");
        assert_eq!(
            view.live_fact(subject, key).unwrap().slot.value,
            value,
            "{text}: the event fired"
        );
    }
}

/// A public event: everyone at the place sees or hears what it changes.
/// "Orsel announces his plan to close the gate" is learned and told. The
/// flag decides, never the text: a public event whose text says nothing of
/// the value is learned too, and the same announcement not public is not.
/// The fact is Persistent: the PC witnessed it, and its key is no state
/// key (the store keeps state keys Established).
#[test]
fn a_public_announcement_is_learned_and_told() {
    let mut w = World::new();
    let orsel = w.orsel;
    schedule_public(
        &mut w,
        "Orsel announces his plan to close the gate",
        &[(orsel, "plan", "close the gate")],
    );
    let (engine, campaign, recent) = wait(w);
    let view = view_of(&engine, campaign);
    assert!(
        pc_knowledge(&view)
            .iter()
            .any(|b| b.subject == orsel && b.key == "plan" && b.value == "close the gate"),
        "{:?}",
        pc_knowledge(&view)
    );
    assert!(
        recent.contains("- Orsel announces his plan to close the gate\n"),
        "{recent}"
    );
    assert!(
        recent.contains("- plan is now close the gate.\n"),
        "{recent}"
    );
    let plan = view.live_fact(orsel, "plan").unwrap();
    assert_eq!(plan.canon, Canon::Persistent);
    // The flag decides, not the words of the text.
    let mut w = World::new();
    schedule_public(
        &mut w,
        "Orsel speaks to the whole square",
        &[(orsel, "plan", "close the gate")],
    );
    let (engine, campaign, _) = wait(w);
    assert!(
        pc_knowledge(&view_of(&engine, campaign))
            .iter()
            .any(|b| b.subject == orsel && b.key == "plan")
    );
    let mut w = World::new();
    schedule(
        &mut w,
        "Orsel announces his plan to close the gate",
        &[(orsel, "plan", "close the gate")],
    );
    let (engine, campaign, _) = wait(w);
    assert!(
        !pc_knowledge(&view_of(&engine, campaign))
            .iter()
            .any(|b| b.subject == orsel && b.key == "plan")
    );
}

/// The leak that bug pass 4 closed stays closed: the text does not state
/// the value, so the PC does not learn it. The text itself is told: it
/// holds no secret value ("debt" is the key, not the value).
#[test]
fn an_effect_the_text_does_not_state_stays_hidden_and_the_text_is_told() {
    for text in [
        "Orsel retreats to a dark corner to count his remaining silver",
        "Orsel counts the silver for his debt",
    ] {
        let mut w = World::new();
        let orsel = w.orsel;
        schedule(&mut w, text, &[(orsel, "debt_status", "partially paid")]);
        let (engine, campaign, recent) = wait(w);
        let view = view_of(&engine, campaign);
        assert!(
            !pc_knowledge(&view)
                .iter()
                .any(|b| b.subject == orsel && b.key == "debt_status"),
            "{text}"
        );
        assert!(recent.contains(&format!("- {text}\n")), "{recent}");
        assert!(!recent.contains("partially paid"), "{recent}");
        let all = screens(engine.store(), campaign, orsel);
        assert!(!all.contains("partially paid"), "{all}");
    }
    // Only part of the value is stated: not learned.
    let mut w = World::new();
    let orsel = w.orsel;
    schedule(
        &mut w,
        "Orsel says the debt is paid",
        &[(orsel, "debt_status", "not paid")],
    );
    let (engine, campaign, _) = wait(w);
    assert!(
        !pc_knowledge(&view_of(&engine, campaign))
            .iter()
            .any(|b| b.key == "debt_status")
    );
}

/// A public event gives the PC no feeling: an effect on the PC's own mood
/// stays hidden. And it names nobody the PC must not know: an effect on a
/// hidden NPC, or on someone the PC cannot name, stays hidden.
#[test]
fn a_public_effect_on_the_pc_s_feelings_a_hidden_npc_or_a_stranger_stays_hidden() {
    let mut w = World::new();
    let ada = w.ada;
    schedule_public(
        &mut w,
        "Orsel scares Ada, who is afraid",
        &[(ada, "mood", "afraid")],
    );
    let (engine, campaign, _) = wait(w);
    assert!(
        !pc_knowledge(&view_of(&engine, campaign))
            .iter()
            .any(|b| b.subject == ada && b.key == "mood")
    );
    let mut w = World::new();
    w.commit(vec![LedgerOp::AddEntity {
        local: None,
        kind: EntityKind::Npc,
        name: "Kestrel".into(),
        age: Some(40),
        other_names: vec![],
    }]);
    let kestrel = w.view.entity_by_name("Kestrel").unwrap().id;
    schedule_public(
        &mut w,
        "Orsel announces a plan to close the gate",
        &[(kestrel, "plan", "close the gate")],
    );
    let (engine, campaign, _) = wait(w);
    let view = view_of(&engine, campaign);
    assert!(!pc_knowledge(&view).iter().any(|b| b.subject == kestrel));
    let mut w = World::with_seed(seed_with_maren());
    let maren = w.view.entity_by_name("Maren Voss").unwrap().id;
    schedule_public(
        &mut w,
        "Orsel announces a plan to close the gate",
        &[(maren, "plan", "close the gate")],
    );
    let (engine, campaign, _) = wait(w);
    let view = view_of(&engine, campaign);
    assert!(!pc_knowledge(&view).iter().any(|b| b.subject == maren));
}

/// The gate seed with the hidden NPC Maren Voss ("the Grey Lady"), who
/// stands at the gate square unseen.
fn seed_with_maren() -> threshold_core::domain::AdventureSeed {
    let mut seed = adventure_seed();
    let mut maren = npc("Maren Voss", 40);
    maren.hidden = true;
    maren.other_names = vec![threshold_core::domain::OtherName {
        name: "the Grey Lady".into(),
        known_to: vec![],
    }];
    seed.entities.push(maren);
    seed.decided
        .push(seed_fact("Maren Voss", "location", "gate square"));
    seed
}

/// When every part of a witnessed event is guarded away, the wait still
/// stopped for it: the narrator gets a neutral line instead of an empty
/// outcome.
#[test]
fn an_event_guarded_away_gives_the_narrator_a_neutral_line() {
    let mut w = World::new();
    let orsel = w.orsel;
    // `Ada holds = gate key` is GM only: the text quotes a secret.
    schedule(
        &mut w,
        "Orsel eyes the gate key",
        &[(orsel, "plan", "flee")],
    );
    let (_, _, recent) = wait(w);
    assert!(!recent.contains("gate key"), "{recent}");
    assert!(
        recent.contains(&format!(
            "- {}\n",
            threshold_core::context::QUIET_EVENT_LINE
        )),
        "{recent}"
    );
    // A told event gets no neutral line.
    let mut w = World::new();
    schedule(&mut w, "Orsel yawns", &[(orsel, "plan", "flee")]);
    let (_, _, recent) = wait(w);
    assert!(
        !recent.contains(threshold_core::context::QUIET_EVENT_LINE),
        "{recent}"
    );
}

/// Review M2: a public event that settles an item closes it for the PC in
/// that Turn, on an NPC's item and on the PC's own (its outcome slot).
#[test]
fn an_announced_ruling_closes_the_item_in_that_turn() {
    let mut w = World::new();
    let (ada, orsel) = (w.ada, w.orsel);
    let pending = |subject: u64, key: &str, text: &str| LedgerOp::AddFact {
        slot: Slot {
            subject: EntityRef::Id(subject),
            key: key.into(),
            value: "pending".into(),
        },
        text: text.into(),
        canon: Canon::Established,
        source: threshold_core::domain::FactSource::Proposal,
        seen_by: vec![EntityRef::Id(ada), EntityRef::Id(orsel)],
        causes: vec![],
    };
    w.commit(vec![
        pending(orsel, "pending_toll", "Orsel will rule on the toll."),
        pending(ada, "pending_meet", "Ada is to meet Orsel's clerk."),
    ]);
    schedule_public(
        &mut w,
        "Orsel reads out his ruling: the toll is raised, and the meeting is cancelled",
        &[(orsel, "toll", "raised"), (ada, "meet", "cancelled")],
    );
    let (engine, campaign, recent) = wait(w);
    let view = view_of(&engine, campaign);
    assert!(
        threshold_core::world::pc_open_items(&view).is_empty(),
        "{:?}",
        threshold_core::world::pc_open_items(&view)
    );
    let sight = sight(engine.store(), campaign);
    let last = sight.story.last().unwrap().turn;
    for key in ["pending_toll", "pending_meet"] {
        let item = sight.open_items.iter().find(|i| i.key == key).unwrap();
        assert_eq!(item.closed_in, Some(last), "{key}");
    }
    assert!(recent.contains("toll is now raised"), "{recent}");
}
