//! `PcSight`: the one door from the world to the Player screens. Each test
//! runs on the hidden world of `support::hidden`.

mod support;

use std::collections::BTreeMap;

use support::{hidden::*, *};
use threshold_core::{
    RecordedGm,
    context::seen_present,
    domain::{ActionKind, DecisionProposal, EntityRef, Knowing, LedgerOp, Seen, Verdict},
    store::TurnId,
    world::{Known, Moment, OpenItem, PcSight, SCENE_GAP_MIN},
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
    let w = HiddenWorld::new();
    let ada = w.ada;
    let mut look = proposal(Verdict::Allowed);
    look.action = ActionKind::Observe;
    look.minutes = 5;
    let mut walk = proposal(Verdict::Allowed);
    walk.action = ActionKind::Move;
    walk.minutes = 10;
    walk.facts = vec![fact(slot(e(ada), "location", "market"), Seen::Pc, vec![])];
    let mut arrive = proposal(Verdict::Allowed);
    arrive.new_npcs = vec![new_npc(1, "Hale", 40)];
    arrive.minutes = 5;
    let mut back = walk.clone();
    back.facts = vec![fact(
        slot(e(ada), "location", "gate square"),
        Seen::Pc,
        vec![],
    )];
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
    w.narrate(said, "Roseanne waves from the wall. chloe nods.");
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
