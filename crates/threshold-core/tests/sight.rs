//! `PcSight`: the one door from the world to the Player screens. Each test
//! runs on the hidden world of `support::hidden`.

mod support;

use std::collections::{BTreeMap, HashSet};

use support::{hidden::*, *};
use threshold_core::{
    RecordedGm,
    context::seen_present,
    domain::{
        ActionKind, DecisionProposal, EntityKind, EntityRef, FactSource, Knowing, LedgerOp,
        SeedEntity, Verdict,
    },
    normalize,
    player::Marks,
    store::{Store, TurnId, TurnKind, TurnRecord},
    turn::{BEGIN_OP_ID, create_campaign},
    view::{entity_card, play_view},
    world::{Known, Moment, OpenItem, PcSight, SCENE_GAP_MIN, SightLine, unnameable_names},
};

fn json(p: &DecisionProposal) -> String {
    serde_json::to_string(p).unwrap()
}

fn no_chunks() -> impl FnMut(&str) {
    |_| {}
}

fn known(sight: &PcSight, id: u64) -> Option<&Known> {
    sight.known.iter().find(|k| k.id == id)
}

/// The name the PC knows for `id`: `None` when the entity is not in
/// `known`, `Some(None)` when it is there but unnamed.
fn name(sight: &PcSight, id: u64) -> Option<Option<String>> {
    known(sight, id).map(|k| k.name.clone())
}

fn moment(sight: &PcSight, turn: TurnId) -> Option<&Moment> {
    sight.moments.iter().find(|m| m.turn == turn)
}

fn meet(sight: &PcSight, ada: u64) -> OpenItem {
    sight
        .open_items
        .iter()
        .find(|i| i.subject == ada && i.key == "pending_meet")
        .cloned()
        .unwrap_or_else(|| panic!("no open item: {:?}", sight.open_items))
}

fn action(kind: ActionKind) -> String {
    record(kind, Verdict::Allowed, None)
}

#[test]
fn pc_sight_holds_no_hidden_world() {
    let w = HiddenWorld::new();
    let sight = w.sight();
    let text = format!("{sight:?}");
    for hidden in HIDDEN {
        assert!(!text.contains(hidden), "{hidden} leaks: {text}");
    }
    // Not vacuous: the stranger is perceived, the hidden entry is not.
    assert!(sight.present.contains(&w.kestrel), "{:?}", sight.present);
    assert!(sight.present.contains(&w.orsel));
    assert!(!sight.present.contains(&w.vey));
    assert_eq!(name(&sight, w.kestrel), Some(None));
    assert!(text.contains("Orsel") && text.contains("Brann at the mill"));
}

#[test]
fn moment_matches_seen_present_of_snapshot_before() {
    let mut w = HiddenWorld::new();
    let place = |name: &str| LedgerOp::AddEntity {
        local: None,
        kind: threshold_core::domain::EntityKind::Place,
        name: name.into(),
        age: None,
        other_names: vec![],
    };
    // The places the moves go to (`move_to`, ADR 0007).
    w.commit(
        TurnKind::Setup,
        "",
        "{}",
        0,
        vec![place("Market"), place("Gate square")],
    );
    // The PC can name both places (C7: `move_to` takes only those).
    let places = w.view();
    let ada = w.ada;
    let known = |name: &str| LedgerOp::AddFact {
        slot: threshold_core::domain::Slot {
            subject: threshold_core::domain::EntityRef::Id(id_of(&places, name)),
            key: "kind".into(),
            value: "square".into(),
        },
        text: format!("{name} is a square."),
        canon: threshold_core::domain::Canon::Established,
        source: threshold_core::domain::FactSource::Proposal,
        seen_by: vec![threshold_core::domain::EntityRef::Id(ada)],
        causes: vec![],
    };
    w.commit(
        TurnKind::Setup,
        "",
        "{}",
        0,
        vec![known("Market"), known("Gate square")],
    );
    let mut look = proposal(Verdict::Allowed);
    look.action = ActionKind::Observe;
    look.minutes = 5;
    let mut walk = proposal(Verdict::Allowed);
    walk.action = ActionKind::Move;
    walk.minutes = 10;
    walk.move_to = Some(e(id_of(&places, "Market")));
    let mut arrive = proposal(Verdict::Allowed);
    arrive.new_npcs = vec![new_npc(1, "Hale", 40)];
    arrive.minutes = 5;
    let mut back = walk.clone();
    back.move_to = Some(e(id_of(&places, "Gate square")));
    let proposals = [&look, &walk, &arrive, &look, &back, &look]
        .into_iter()
        .map(json)
        .collect();
    let narrations = (1..=6).map(|n| format!("Turn {n}.")).collect();
    let gm = RecordedGm::new(proposals, narrations);
    let (mut engine, campaign) = w.engine(&gm);
    for n in 1..=6 {
        engine
            .play_turn(format!("oracle-{n}"), "I go on.", &mut no_chunks())
            .unwrap();
    }
    let store = engine.store();
    let seq: BTreeMap<TurnId, u64> = store
        .turns(campaign)
        .unwrap()
        .into_iter()
        .map(|t| (t.id, t.seq))
        .collect();
    let sight = sight_of(store, campaign);
    assert_eq!(sight.moments.len(), 9, "Opening, two fixture Turns, six");
    for m in &sight.moments {
        let before = store.snapshot_before(campaign, seq[&m.turn] + 1).unwrap();
        assert_eq!(m.present, seen_present(&before), "Turn {}", m.turn);
    }
    // The move and the arrival show in the Moments.
    let view = store.snapshot(campaign).unwrap();
    let (pip, hale) = (id_of(&view, "Pip"), id_of(&view, "Hale"));
    assert!(sight.moments.iter().any(|m| m.present.contains(&pip)));
    assert!(sight.moments.iter().any(|m| m.present.contains(&hale)));
    assert!(
        sight
            .moments
            .iter()
            .any(|m| m.place.as_deref() == Some("market"))
    );
}

#[test]
fn pc_names_by_seed_belief_or_narration() {
    let mut w = HiddenWorld::new();
    let (ada, brann) = (w.ada, w.brann);
    let sight = w.sight();
    assert_eq!(name(&sight, ada), Some(Some("Ada".into())));
    assert_eq!(name(&sight, w.orsel), Some(Some("Orsel".into())), "seed");
    assert_eq!(name(&sight, w.kestrel), Some(None), "perceived, not named");

    // A hidden seed entry the PC comes to see is perceived, not named.
    let vey_location = w.view().live_fact(w.vey, "location").unwrap().id;
    let clock = w.view().clock_min;
    w.play(
        "I look into the shadow.",
        &action(ActionKind::Observe),
        0,
        vec![LedgerOp::Know {
            holder: EntityRef::Id(ada),
            fact_id: Some(vey_location),
            claim_id: None,
            how: Knowing::Witnessed,
            arrives_at: clock,
        }],
    );
    assert!(w.sight().present.contains(&w.vey));
    assert_eq!(name(&w.sight(), w.vey), Some(None), "hidden entry");

    // A Belief subject: an NPC the PC only hears about.
    w.play(
        "I wait.",
        &action(ActionKind::Observe),
        0,
        newcomer("Marlo", "market", &[]),
    );
    let marlo = id_of(&w.view(), "Marlo");
    assert_eq!(name(&w.sight(), marlo), None, "not perceived, not told");
    w.play(
        "I ask Brann about the market.",
        &action(ActionKind::Talk),
        0,
        vec![claim(brann, marlo, "mood", "angry", &[ada], 0)],
    );
    assert_eq!(name(&w.sight(), marlo), Some(Some("Marlo".into())));

    // Whole words after `normalize::name`.
    w.play(
        "I wait.",
        &action(ActionKind::Observe),
        0,
        newcomer("Rose", "gate square", &[ada]),
    );
    w.play(
        "I wait.",
        &action(ActionKind::Observe),
        0,
        newcomer("Chloé", "gate square", &[ada]),
    );
    let view = w.view();
    let (rose, chloe) = (id_of(&view, "Rose"), id_of(&view, "Chloé"));
    let said = w.play("I look around.", &action(ActionKind::Observe), 0, vec![]);
    w.narrate(said, "Roseanne waves from the wall. Chloe nods.");
    let sight = w.sight();
    assert_eq!(name(&sight, rose), Some(None), "Rose is not Roseanne");
    assert_eq!(name(&sight, chloe), Some(Some("Chloé".into())));

    // A reverted Turn's Narration does not count.
    let undone = w.play(
        "I call to the stranger.",
        &action(ActionKind::Talk),
        0,
        vec![],
    );
    w.narrate(undone, "Kestrel answers from under the hood.");
    w.revert(undone, "I stay quiet.", 0);
    assert_eq!(name(&w.sight(), w.kestrel), Some(None));
    let told = w.play(
        "I ask the stranger's name.",
        &action(ActionKind::Talk),
        0,
        vec![],
    );
    w.narrate(told, "\"Kestrel,\" the stranger says.");
    assert_eq!(name(&w.sight(), w.kestrel), Some(Some("Kestrel".into())));
}

#[test]
fn other_names_come_from_alias_beliefs_only() {
    let mut w = HiddenWorld::new();
    let orsel = w.orsel;
    let other = |s: &PcSight| known(s, orsel).unwrap().other_names.clone();
    assert!(other(&w.sight()).is_empty());
    let (ada, alias) = (w.ada, w.alias);
    let clock = w.view().clock_min;
    w.play(
        "I listen to Brann.",
        &action(ActionKind::Talk),
        0,
        vec![LedgerOp::Know {
            holder: EntityRef::Id(ada),
            fact_id: Some(alias),
            claim_id: None,
            how: Knowing::Witnessed,
            arrives_at: clock,
        }],
    );
    assert_eq!(other(&w.sight()), ["the Toll-Wolf"]);
}

#[test]
fn learned_rows_land_in_their_moment() {
    let mut w = HiddenWorld::new();
    let (ada, brann, orsel) = (w.ada, w.brann, w.orsel);
    let told = w.play(
        "I ask Brann about Orsel.",
        &action(ActionKind::Talk),
        0,
        vec![claim(brann, orsel, "mood", "tired", &[ada], 90)],
    );
    let wait = w.play("I wait.", &action(ActionKind::Observe), 60, vec![]);
    let view = w.view();
    let climb = w.play(
        "I wait more.",
        &action(ActionKind::Observe),
        60,
        vec![set(
            &view,
            orsel,
            "location",
            "gate tower",
            "Orsel climbs the gate tower.",
            &[ada],
        )],
    );
    let sight = w.sight();
    let heard = |turn| {
        moment(&sight, turn)
            .unwrap()
            .learned
            .iter()
            .any(|b| b.subject == orsel && b.key == "mood")
    };
    // Said at minute 5, arrives at 95: the Turn that ends at 125.
    assert!(!heard(told) && !heard(wait) && heard(climb));

    // A Setup row is known before the Campaign, in no Moment.
    assert!(sight.before.iter().any(|b| b.key == "pending_meet"));
    assert!(
        sight
            .moments
            .iter()
            .all(|m| m.learned.iter().all(|b| b.key != "pending_meet"))
    );

    // A witnessed location of another entity is presence, not learning.
    assert!(
        sight
            .beliefs
            .iter()
            .any(|b| b.subject == orsel && b.key == "location" && b.value == "gate tower"),
        "the PC saw Orsel climb"
    );
    for m in &sight.moments {
        assert!(
            m.learned
                .iter()
                .all(|b| b.key != "location" || b.subject == ada),
            "{m:?}"
        );
    }
}

#[test]
fn scenes_break_on_place_change_and_long_gaps() {
    let mut w = HiddenWorld::new();
    let ada = w.ada;
    let short = w.play("I wait.", &action(ActionKind::Observe), 10, vec![]);
    let view = w.view();
    let walk = w.play(
        "I walk to the market.",
        &action(ActionKind::Move),
        10,
        vec![set(
            &view,
            ada,
            "location",
            "market",
            "Ada walks to the market.",
            &[ada],
        )],
    );
    let long = w.play(
        "I wait for the night.",
        &action(ActionKind::Wait),
        SCENE_GAP_MIN as u32 + 1,
        vec![],
    );
    let even = w.play(
        "I wait for dawn.",
        &action(ActionKind::Wait),
        SCENE_GAP_MIN as u32,
        vec![],
    );
    assert_eq!(SCENE_GAP_MIN, 240);
    let sight = w.sight();
    let scenes = sight.scenes();
    let first: Vec<TurnId> = scenes.iter().map(|scene| scene[0].turn).collect();
    assert_eq!(first[0], w.opening);
    assert!(first.contains(&walk), "{first:?}");
    assert!(first.contains(&long), "{first:?}");
    assert!(!first.contains(&even), "{first:?}");
    assert!(!first.contains(&short), "{first:?}");
    let flat: Vec<TurnId> = scenes
        .iter()
        .flat_map(|s| s.iter().map(|m| m.turn))
        .collect();
    let all: Vec<TurnId> = sight.moments.iter().map(|m| m.turn).collect();
    assert_eq!(flat, all);
}

#[test]
fn open_item_closes_on_a_fact_not_a_claim() {
    let mut w = HiddenWorld::new();
    let (ada, brann) = (w.ada, w.brann);
    assert_eq!(meet(&w.sight(), ada).closed_in, None);
    w.play(
        "I ask Brann about the meeting.",
        &action(ActionKind::Talk),
        0,
        vec![claim(brann, ada, "meet", "done", &[ada], 0)],
    );
    assert_eq!(
        meet(&w.sight(), ada).closed_in,
        None,
        "a Claim can be false"
    );
    let view = w.view();
    w.play(
        "We pick another place.",
        &action(ActionKind::Talk),
        0,
        vec![set(
            &view,
            ada,
            "pending_meet",
            "brann at the well",
            "Ada plans to meet Brann at the well.",
            &[ada],
        )],
    );
    let item = meet(&w.sight(), ada);
    assert_eq!(item.closed_in, None);
    assert!(item.text.contains("well"), "{item:?}");
    let view = w.view();
    let met = w.play(
        "I meet Brann at the well.",
        &action(ActionKind::Act),
        10,
        vec![set(
            &view,
            ada,
            "meet",
            "brann",
            "Ada met Brann at the well.",
            &[ada],
        )],
    );
    assert_eq!(meet(&w.sight(), ada).closed_in, Some(met));
}

/// A proposal that restates a live fact with the same value writes one more
/// PC row on it (`checks::ops` `witness`). That row must not re-open an
/// item that an outcome closed, and the closing Turn stays a decision.
#[test]
fn closed_item_stays_closed_when_the_pc_learns_the_plan_again() {
    let mut w = HiddenWorld::new();
    let ada = w.ada;
    let view = w.view();
    let met = w.play(
        "I meet Brann at the mill.",
        &action(ActionKind::Act),
        10,
        vec![set(
            &view,
            ada,
            "meet",
            "brann",
            "Ada met Brann at the mill.",
            &[ada],
        )],
    );
    assert_eq!(meet(&w.sight(), ada).closed_in, Some(met));
    let clock = w.view().clock_min;
    w.play(
        "I think back on the plan.",
        &action(ActionKind::Observe),
        0,
        vec![LedgerOp::Know {
            holder: EntityRef::Id(ada),
            fact_id: Some(w.meet),
            claim_id: None,
            how: Knowing::Witnessed,
            arrives_at: clock,
        }],
    );
    let sight = w.sight();
    assert_eq!(meet(&sight, ada).closed_in, Some(met));
    assert!(moment(&sight, met).unwrap().decision);
}

/// Only an outcome learned after the plan closes it: an older row on the
/// outcome slot does not.
#[test]
fn outcome_learned_before_the_plan_keeps_it_open() {
    let mut w = HiddenWorld::new();
    let ada = w.ada;
    let view = w.view();
    w.play(
        "I walk to the mill and back.",
        &action(ActionKind::Act),
        10,
        vec![set(
            &view,
            ada,
            "trip",
            "the mill",
            "Ada went to the mill.",
            &[ada],
        )],
    );
    let view = w.view();
    w.play(
        "I plan another trip.",
        &action(ActionKind::Talk),
        0,
        vec![set(
            &view,
            ada,
            "pending_trip",
            "the mill",
            "Ada plans a trip to the mill.",
            &[ada],
        )],
    );
    let sight = w.sight();
    let trip = sight
        .open_items
        .iter()
        .find(|i| i.key == "pending_trip")
        .unwrap();
    assert_eq!(trip.closed_in, None, "{trip:?}");
}

/// `op` with its fact source set to `source`.
fn sourced(mut op: LedgerOp, source: FactSource) -> LedgerOp {
    match &mut op {
        LedgerOp::AddFact { source: s, .. } => *s = source,
        LedgerOp::SupersedeFact { new, .. } => new.source = source,
        other => panic!("not a fact op: {other:?}"),
    }
    op
}

/// The clauses of the decision rule that `decision_rule_marks_lasting_
/// witnessed_acts` does not reach alone: a Persistent fact from a Check
/// stake or a Correction, and a Turn whose only mark is that it closed an
/// Open Item the PC knows.
#[test]
fn decision_rule_counts_stakes_corrections_and_closed_items() {
    let mut w = HiddenWorld::new();
    let (ada, gate) = (w.ada, w.gate);
    let view = w.view();
    let mark = |key: &str, source: FactSource| {
        sourced(
            set(
                &view,
                gate,
                key,
                "chalk",
                "A chalk mark is on the gate.",
                &[ada],
            ),
            source,
        )
    };
    let stake = w.play(
        "I bet my coin on the lock.",
        &action(ActionKind::Act),
        5,
        vec![mark("bet", FactSource::Stake)],
    );
    let fixed = w.play(
        "I scratch the gate.",
        &action(ActionKind::Act),
        5,
        vec![mark("scratch", FactSource::Correction)],
    );
    let event = w.play(
        "I watch the gate.",
        &action(ActionKind::Act),
        5,
        vec![mark("stain", FactSource::WorldEvent)],
    );
    // `mood` is a state key: the outcome fact is never Persistent, so the
    // only mark of this Turn is that it closes the Open Item.
    let view = w.view();
    w.play(
        "I promise to keep calm.",
        &action(ActionKind::Observe),
        0,
        vec![set(
            &view,
            ada,
            "pending_mood",
            "calm",
            "Ada means to stay calm.",
            &[ada],
        )],
    );
    let view = w.view();
    let calm = w.play(
        "I breathe slowly.",
        &action(ActionKind::Act),
        0,
        vec![set(&view, ada, "mood", "calm", "Ada is calm.", &[ada])],
    );
    let sight = w.sight();
    let decision = |turn| moment(&sight, turn).is_some_and(|m| m.decision);
    assert!(decision(stake), "a Persistent fact from a stake");
    assert!(decision(fixed), "a Persistent fact from a Correction");
    assert!(!decision(event), "a World Event fact is not the PC's act");
    let item = sight
        .open_items
        .iter()
        .find(|i| i.key == "pending_mood")
        .unwrap();
    assert_eq!(item.closed_in, Some(calm));
    assert!(decision(calm), "it closed an Open Item the PC knows");
}

/// A PC row counts only once it has arrived: not in "Known before the
/// Campaign", and not to close an Open Item.
#[test]
fn rows_that_have_not_arrived_count_nowhere() {
    let mut w = HiddenWorld::new();
    let (ada, brann) = (w.ada, w.brann);
    w.commit(
        TurnKind::Setup,
        "",
        "{}",
        0,
        vec![claim(brann, brann, "rumour", "a toll rise", &[ada], 60)],
    );
    // An outcome nobody saw, that the PC hears of in an hour.
    let view = w.view();
    w.play(
        "I wait.",
        &action(ActionKind::Observe),
        0,
        vec![set(
            &view,
            ada,
            "meet",
            "missed",
            "Brann waited at the mill in vain.",
            &[],
        )],
    );
    let outcome = w.view().live_fact(ada, "meet").unwrap().id;
    let clock = w.view().clock_min;
    let heard = w.play(
        "I wait more.",
        &action(ActionKind::Observe),
        0,
        vec![LedgerOp::Know {
            holder: EntityRef::Id(ada),
            fact_id: Some(outcome),
            claim_id: None,
            how: Knowing::Told,
            arrives_at: clock + 60,
        }],
    );
    let sight = w.sight();
    assert!(
        sight.before.iter().all(|b| b.key != "rumour"),
        "{:?}",
        sight.before
    );
    assert_eq!(meet(&sight, ada).closed_in, None);

    w.play("I wait an hour.", &action(ActionKind::Observe), 60, vec![]);
    let sight = w.sight();
    assert!(sight.before.iter().any(|b| b.key == "rumour"));
    assert_eq!(meet(&sight, ada).closed_in, Some(heard));
}

#[test]
fn offscreen_outcome_keeps_the_item_open() {
    let mut w = HiddenWorld::new();
    let (ada, brann) = (w.ada, w.brann);
    let view = w.view();
    w.play(
        "I wait.",
        &action(ActionKind::Observe),
        10,
        vec![set(
            &view,
            ada,
            "meet",
            "missed",
            "Brann waited at the mill in vain.",
            &[brann],
        )],
    );
    assert_eq!(
        w.view().live_fact(ada, "meet").unwrap().slot.value,
        "missed"
    );
    assert_eq!(meet(&w.sight(), ada).closed_in, None);
}

#[test]
fn decision_rule_marks_lasting_witnessed_acts() {
    let mut w = HiddenWorld::new();
    let (ada, gate) = (w.ada, w.gate);
    let talk = w.play(
        "I haggle with Brann.",
        &record(ActionKind::Talk, Verdict::Check, Some(roll(true))),
        5,
        vec![],
    );
    let view = w.view();
    let mark = |key: &str| {
        set(
            &view,
            gate,
            key,
            "chalk",
            "A chalk mark is on the gate.",
            &[ada],
        )
    };
    let act = w.play(
        "I chalk a sign on the gate.",
        &action(ActionKind::Act),
        5,
        vec![mark("sign")],
    );
    let plan = w.play(
        "I plan a trip to the mill.",
        &action(ActionKind::Act),
        0,
        vec![set(
            &view,
            ada,
            "pending_trip",
            "the mill",
            "Ada plans a trip to the mill.",
            &[ada],
        )],
    );
    let walk = w.play(
        "I walk to the market.",
        &action(ActionKind::Move),
        10,
        vec![set(
            &view,
            ada,
            "location",
            "market",
            "Ada walks to the market.",
            &[ada],
        )],
    );
    let impossible = w.play(
        "I fly over the wall.",
        &record(ActionKind::Act, Verdict::Impossible, Some(roll(false))),
        0,
        vec![mark("scratch")],
    );
    let look = w.play(
        "I study the gate.",
        &record(ActionKind::Observe, Verdict::Check, Some(roll(true))),
        5,
        vec![mark("smudge")],
    );
    let undone = w.play(
        "I carve my name on the gate.",
        &action(ActionKind::Act),
        5,
        vec![mark("carving")],
    );
    let fix = w.revert(undone, "I leave the gate alone.", 0);
    let sight = w.sight();
    let decision = |turn| moment(&sight, turn).is_some_and(|m| m.decision);
    assert!(decision(talk), "talk with a Roll");
    assert!(decision(act), "a lasting witnessed fact");
    assert!(decision(plan), "an Open Item the PC knows");
    assert!(!decision(walk), "a move writes state only");
    assert!(!decision(impossible), "impossible");
    assert!(!decision(look), "observe");
    assert!(!decision(undone), "reverted");
    assert!(!decision(fix), "a Correction that only reverts");
    assert!(!decision(w.opening), "the Opening Turn");
    assert!(moment(&sight, w.opening).is_some());
}

#[test]
fn correction_replaces_the_reverted_turn() {
    let w = HiddenWorld::new();
    let mut greet = proposal(Verdict::Allowed);
    greet.action = ActionKind::Talk;
    let gm = RecordedGm::new(
        vec![json(&greet), json(&greet)],
        vec!["Brann nods.".into(), "Brann laughs.".into()],
    );
    let (mut engine, campaign) = w.engine(&gm);
    let old = engine
        .play_turn("fix-1".into(), "I greet Brann.", &mut no_chunks())
        .unwrap()
        .turn_id;
    assert!(sight_of(engine.store(), campaign).can_correct);
    let new = engine
        .correct_last(
            "fix-2".into(),
            "I greet Brann warmly.",
            "too cold",
            &mut no_chunks(),
        )
        .unwrap()
        .turn_id;
    let sight = sight_of(engine.store(), campaign);
    assert_eq!(sight.replaced_by.get(&old), Some(&new));
    assert!(!sight.can_correct, "a Correction reaches back one Turn");
    assert!(sight.story.iter().any(|t| t.turn == old && t.reverted));
    assert!(sight.story.iter().any(|t| t.turn == new && !t.reverted));
    assert!(moment(&sight, old).is_none());
    assert!(moment(&sight, new).is_some());
    assert!(!sight.needs_begin);
}

// ---- The stream filter's names (data shape 3.1, changed after review) -------

/// The set the stream filter reads, from the store as it is now.
fn unnameable(store: &threshold_core::store::Store, campaign: u64) -> HashSet<String> {
    let view = store.snapshot(campaign).unwrap();
    let turns = store.turns(campaign).unwrap();
    let reverted: HashSet<TurnId> = store
        .reverted_turns(campaign)
        .unwrap()
        .into_iter()
        .collect();
    unnameable_names(&view, &turns, &reverted)
}

/// A tag is in the stream filter's set exactly when the commit-time rule
/// removes it: one tagged line for every name and other name of the world.
#[test]
fn unnameable_names_match_the_commit_time_rule() {
    let mut w = HiddenWorld::new();
    w.play(
        "I wait.",
        &observe_record(),
        0,
        newcomer("Brann", "market", &[]),
    );
    let said = w.play("I listen.", &observe_record(), 0, vec![]);
    let view = w.view();
    let names: Vec<String> = view
        .entities
        .iter()
        .flat_map(|e| std::iter::once(e.name.clone()).chain(e.other_names.clone()))
        .chain(["Thunder".to_owned()])
        .collect();
    let lines: Vec<String> = names.iter().map(|n| format!("{n}: \"Hi.\"")).collect();
    w.narrate(said, &lines.join("\n"));

    let set = unnameable(&w.store, w.campaign);
    assert!(set.contains("kestrel") && set.contains("toll-wolf") && set.contains("vey"));
    assert!(!set.contains("orsel") && !set.contains("thunder"));
    let sight = w.sight();
    let turn = sight.story.iter().find(|t| t.turn == said).unwrap();
    assert_eq!(turn.lines.len(), names.len());
    for (name, line) in names.iter().zip(&turn.lines) {
        let dropped = *line == SightLine::Prose("\"Hi.\"".into());
        assert_eq!(dropped, set.contains(&normalize::name(name)), "{name}");
    }
}

fn observe_record() -> String {
    record(ActionKind::Observe, Verdict::Allowed, None)
}

/// The Turn path: the Player never sees the tag of a name the PC does not
/// know, not even while the Narration streams. The stored Narration keeps
/// it, and the screens remove it after the Commit as before.
#[test]
fn a_streamed_narration_never_shows_an_unknown_tag() {
    let w = HiddenWorld::new();
    let opening = w.opening;
    let raw =
        "Kestrel: \"Nobody knows my name.\"\n\nOrsel: \"You are late.\"\nThe Toll-Wolf: \"Pay.\"";
    let shown = "\"Nobody knows my name.\"\n\nOrsel: \"You are late.\"\n\"Pay.\"";
    let gm = RecordedGm::new(
        vec![json(&proposal(Verdict::Allowed))],
        vec![raw.to_owned(), raw.to_owned(), raw.to_owned()],
    );
    let (mut engine, _) = w.engine(&gm);
    let mut seen = String::new();
    let outcome = engine
        .play_turn("stream-1".into(), "I listen.", &mut |c| seen.push_str(c))
        .unwrap();
    assert_eq!(seen, shown);
    assert_eq!(outcome.narration, raw);
    let stored = engine.store().turn(outcome.turn_id).unwrap().unwrap();
    assert_eq!(stored.narration.as_deref(), Some(raw));

    // `renarrate`, of a play Turn and of the Opening Turn.
    let mut seen = String::new();
    engine
        .renarrate(outcome.turn_id, &mut |c| seen.push_str(c))
        .unwrap();
    assert_eq!(seen, shown);
    let mut seen = String::new();
    engine
        .renarrate(opening, &mut |c| seen.push_str(c))
        .unwrap();
    assert_eq!(seen, shown);
    // One proposal and three Narrations: the filter calls no model.
    assert_eq!(gm.calls().len(), 4);
}

/// The PC accepts a pending proposal (B3, then ADR 0007). The proposal
/// settles the Open Item with outcome `accepted`, the core
/// writes its outcome on the PC, and the item closes in a Turn that is a
/// decision.
#[test]
fn pc_settles_its_open_item_and_the_turn_is_a_decision() {
    let w = HiddenWorld::new();
    let ada = w.ada;
    let mut accept = proposal(Verdict::Allowed);
    accept.action = ActionKind::Talk;
    accept.minutes = 2;
    accept.settles = Some(threshold_core::domain::Settle {
        item: format!("E{ada} pending_meet"),
        outcome: threshold_core::domain::ItemStatus::Accepted,
    });
    let gm = RecordedGm::new(vec![json(&accept)], vec!["You give your word.".into()]);
    let (mut engine, campaign) = w.engine(&gm);
    let handles = {
        let view = engine.store().snapshot(campaign).unwrap();
        threshold_core::context::handle_context(&view, "I will meet Brann.")
    };
    assert_eq!(handles.open_items, [format!("E{ada} pending_meet")]);
    let outcome = engine
        .play_turn(
            "t-accept".into(),
            "Yes, I will meet Brann.",
            &mut no_chunks(),
        )
        .unwrap();
    let sight = sight_of(engine.store(), campaign);
    assert_eq!(meet(&sight, ada).closed_in, Some(outcome.turn_id));
    assert!(moment(&sight, outcome.turn_id).unwrap().decision);
}

/// Bug pass 3, M1 step 3: "Correct last Turn" re-opens what the corrected
/// Turn closed. The Correction reverts the close; its own proposal settles
/// nothing, so the item is open again, and the reverted Turn is no
/// decision.
#[test]
fn a_correction_reopens_the_item_its_turn_closed() {
    let w = HiddenWorld::new();
    let ada = w.ada;
    let mut accept = proposal(Verdict::Allowed);
    accept.action = ActionKind::Talk;
    accept.settles = Some(threshold_core::domain::Settle {
        item: format!("E{ada} pending_meet"),
        outcome: threshold_core::domain::ItemStatus::Accepted,
    });
    let mut talk = proposal(Verdict::Allowed);
    talk.action = ActionKind::Talk;
    let gm = RecordedGm::new(
        vec![json(&accept), json(&talk)],
        vec!["You give your word.".into(), "Brann waits.".into()],
    );
    let (mut engine, campaign) = w.engine(&gm);
    let old = engine
        .play_turn("t-yes".into(), "Yes, I will meet Brann.", &mut no_chunks())
        .unwrap()
        .turn_id;
    assert_eq!(
        meet(&sight_of(engine.store(), campaign), ada).closed_in,
        Some(old)
    );
    let new = engine
        .correct_last(
            "t-fix".into(),
            "I ask Brann where the mill is.",
            "I did not accept",
            &mut no_chunks(),
        )
        .unwrap()
        .turn_id;
    let sight = sight_of(engine.store(), campaign);
    assert_eq!(meet(&sight, ada).closed_in, None, "{:?}", sight.open_items);
    assert!(moment(&sight, old).is_none());
    assert!(moment(&sight, new).is_some_and(|m| !m.decision));
    let view = engine.store().snapshot(campaign).unwrap();
    assert!(view.live_fact(ada, "meet").is_none());
    assert_eq!(
        threshold_core::world::pc_open_items(&view),
        [(ada, "pending_meet".to_owned())]
    );
}

/// The live bug (play_basics, cdefde8), in the new shape: the Intent asks
/// for more time, and the proposal states the item `open`. The Turn
/// commits on the first attempt and the item stays open.
#[test]
fn an_intent_that_asks_for_time_leaves_the_open_item_open() {
    let w = HiddenWorld::new();
    let ada = w.ada;
    let raw = format!(
        r#"{{"action":"talk","contested":false,"move_to":null,"settles":{{"item":"E{ada} pending_meet","outcome":"open"}},"verdict":"allowed","minutes":3}}"#
    );
    let gm = RecordedGm::new(vec![raw], vec!["Brann nods and waits.".into()]);
    let (mut engine, campaign) = w.engine(&gm);
    let outcome = engine
        .play_turn(
            "t-later".into(),
            "I ask Brann for more time to think about the meeting.",
            &mut no_chunks(),
        )
        .unwrap();
    let sight = sight_of(engine.store(), campaign);
    assert_eq!(meet(&sight, ada).closed_in, None);
    assert!(moment(&sight, outcome.turn_id).is_none_or(|m| !m.decision));
    assert_eq!(gm.calls().len(), 2, "one proposal, one Narration");
}

/// The Opening Turn of a new Campaign made from `seed`, with `narration`.
fn opened(seed: &threshold_core::domain::AdventureSeed, narration: &str) -> (Store, u64) {
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, seed, 7).unwrap();
    let opening = store
        .commit(
            TurnRecord {
                campaign_id: campaign,
                op_id: BEGIN_OP_ID.to_owned(),
                kind: TurnKind::Opening,
                intent: String::new(),
                proposal_json: "{}".into(),
                clock_before: 0,
                clock_after: 0,
            },
            vec![],
        )
        .unwrap();
    store.set_narration(opening, narration).unwrap();
    (store, campaign)
}

/// Review B1 (T3): a hidden Adventure Seed Faction whose name is a common
/// word. The Opening Narration says "A circle of lamplight", and even "the
/// Circle": a hidden entry is never named through a Narration word. It is
/// on no screen, has no card and no link. A PC Belief names it.
#[test]
fn a_hidden_seed_entry_is_never_named_by_a_narration_word() {
    let mut seed = adventure_seed();
    seed.entities.push(SeedEntity {
        name: "The Circle".into(),
        kind: EntityKind::Faction,
        age: None,
        other_names: vec![],
        hidden: true,
    });
    let (mut store, campaign) = opened(
        &seed,
        "Dusk falls. A circle of lamplight lies on the gate square. The Circle meets tonight, they say.",
    );
    let view = store.snapshot(campaign).unwrap();
    let circle = id_of(&view, "The Circle");
    let ada = id_of(&view, "Ada");
    let sight = sight_of(&store, campaign);
    assert!(known(&sight, circle).is_none(), "{:?}", sight.known);
    let play = play_view(&sight, &Marks::default());
    assert!(!play.known.iter().any(|k| k.who.entity_id == circle));
    assert!(entity_card(&sight, &Marks::default(), circle).is_none());
    let story = serde_json::to_string(&play.story).unwrap();
    assert!(
        !story.contains(&format!("\"entity_id\":{circle}")),
        "{story}"
    );
    let turns = store.turns(campaign).unwrap();
    let standing: Vec<_> = turns.iter().collect();
    assert!(!threshold_core::world::pc_can_name(&view, &standing).contains(&circle));
    assert!(unnameable_names(&view, &turns, &HashSet::new()).contains("circle"));

    // A PC Belief about it names it.
    store
        .commit(
            TurnRecord {
                campaign_id: campaign,
                op_id: "told".into(),
                kind: TurnKind::Play,
                intent: "I ask about the lamps.".into(),
                proposal_json: action(ActionKind::Talk),
                clock_before: 0,
                clock_after: 0,
            },
            vec![set(
                &view,
                circle,
                "meets",
                "at dusk",
                "The Circle meets at dusk.",
                &[ada],
            )],
        )
        .unwrap();
    let sight = sight_of(&store, campaign);
    assert_eq!(name(&sight, circle), Some(Some("The Circle".into())));
}

/// Review B1: a Narration names an entity only with the name's own
/// capitals; a leading article may be dropped or lower case. "a mill" or
/// "the mill wheel" does not name the place Mill; "the Mill" and "MILL" do.
/// `known_places` follows the same rule.
#[test]
fn a_narration_names_an_entity_with_its_own_capitals() {
    let mut w = HiddenWorld::new();
    w.play(
        "I wait.",
        &action(ActionKind::Observe),
        0,
        vec![LedgerOp::AddEntity {
            local: None,
            kind: EntityKind::Place,
            name: "Mill".into(),
            age: None,
            other_names: vec![],
        }],
    );
    let mill = id_of(&w.view(), "Mill");
    let quiet = w.play("I listen.", &action(ActionKind::Observe), 0, vec![]);
    w.narrate(quiet, "The mill wheel turns somewhere. A mill, perhaps.");
    assert!(known(&w.sight(), mill).is_none());
    assert!(!threshold_core::world::known_places(&w.view()).contains(&mill));
    for text in ["Brann points to the Mill.", "MILL, says the sign."] {
        let mut w = HiddenWorld::new();
        w.play(
            "I wait.",
            &action(ActionKind::Observe),
            0,
            vec![LedgerOp::AddEntity {
                local: None,
                kind: EntityKind::Place,
                name: "Mill".into(),
                age: None,
                other_names: vec![],
            }],
        );
        let mill = id_of(&w.view(), "Mill");
        let said = w.play("I listen.", &action(ActionKind::Observe), 0, vec![]);
        w.narrate(said, text);
        assert_eq!(name(&w.sight(), mill), Some(Some("Mill".into())), "{text}");
        assert!(
            threshold_core::world::known_places(&w.view()).contains(&mill),
            "{text}"
        );
    }
}
