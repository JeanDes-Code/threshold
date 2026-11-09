//! The town demo (design §9): an offscreen consequence that the Player learns
//! of only through what the PC observes.

mod support;

use std::path::{Path, PathBuf};

use support::*;
use threshold_core::{
    CallKind, RecordedGm,
    domain::{
        ActionKind, Canon, DecisionProposal, EntityKind, EventDraft, Handle, Knowing, LedgerOp,
        NewFact, Reaction, Seen, Stance, Verdict,
    },
    fixture::{load_adventure_seed, load_world},
    rules::D10,
    store::{Store, TurnKind, WorldView},
    turn::{Engine, create_campaign},
    world::{beliefs, pc_knowledge},
};

const DAY: u32 = 24 * 60;

fn town_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/town")
}

/// A fresh store with the town campaign: the Adventure Seed, then the world.
fn town() -> (Store, u64) {
    let dir = town_dir();
    let seed = load_adventure_seed(&dir.join("adventure-seed.toml")).unwrap();
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, &seed, 11).unwrap();
    load_world(&mut store, campaign, &dir.join("world.toml")).unwrap();
    (store, campaign)
}

fn id(view: &WorldView, name: &str) -> u64 {
    view.entities
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("no entity {name}"))
        .id
}

fn json(p: &DecisionProposal) -> String {
    serde_json::to_string(p).unwrap()
}

fn knows_reputation(view: &WorldView) -> bool {
    pc_knowledge(view).iter().any(|b| b.key == "reputation")
}

#[test]
fn town_fixture_loads() {
    let (store, campaign) = town();
    let view = store.snapshot(campaign).unwrap();
    let seed = view.adventure_seed.as_ref().unwrap();
    assert!(
        seed.boundaries
            .iter()
            .any(|b| b == "adult participants only")
    );

    let pc = id(&view, "Wren");
    let orsel = id(&view, "Orsel");
    let doul = id(&view, "Doul");
    let hessa = id(&view, "Hessa");
    let magistracy = id(&view, "Magistracy");
    for person in [pc, orsel, doul, hessa] {
        let e = view.entities.iter().find(|e| e.id == person).unwrap();
        assert_eq!(e.adult, Some(true), "{} is adult", e.name);
    }
    let kind = |id: u64| view.entities.iter().find(|e| e.id == id).unwrap().kind;
    assert_eq!(kind(pc), EntityKind::Pc);
    assert_eq!(kind(magistracy), EntityKind::Faction);

    // The GM-only secret: Persistent, and nobody holds it.
    let skims = view.live_fact(orsel, "skims").unwrap();
    assert_eq!(skims.canon, Canon::Persistent);
    assert!(view.knowledge.iter().all(|k| k.fact_id != Some(skims.id)));
    // The rumor stays Possible.
    let miller = id(&view, "Tobin");
    assert_eq!(
        view.live_fact(miller, "cheats").unwrap().canon,
        Canon::Possible
    );

    // world.toml: goals, Voice Cards, attitudes and the obstacle.
    let goal = |npc: u64| view.goals_active.iter().find(|g| g.npc_id == npc).unwrap();
    assert_eq!(goal(orsel).text, "Protect his reputation");
    assert_eq!(goal(doul).text, "Serve Orsel");
    let card = view.voice_card(orsel).unwrap();
    assert_eq!(card.samples.len(), 3);
    assert!(card.vocabulary.contains("statute"));
    assert!(view.voice_card(doul).is_some() && view.voice_card(hessa).is_some());
    let attitude = |from: u64| {
        view.attitudes
            .iter()
            .find(|a| a.from == from && a.to == pc)
            .unwrap()
            .attitude
    };
    assert_eq!(attitude(orsel), -1);
    assert_eq!(attitude(hessa), 1);
    let archive = id(&view, "Tithe archive");
    let locked = view.live_fact(archive, "condition").unwrap();
    assert!(view.obstacles_open.iter().any(|o| o.fact_id == locked.id));

    // The world arrives as one Setup Turn after the campaign's own.
    assert_eq!(view.turn_seq, 2);
    let world_turn = store.turn_by_campaign_op(campaign, &"__world__".into());
    assert_eq!(world_turn.unwrap().unwrap().record.kind, TurnKind::Setup);
    // The PC witnessed who stands in the town hall at the start.
    assert!(pc_knowledge(&view).iter().any(|b| b.subject == orsel));
}

#[test]
fn town_demo_offscreen_consequence() {
    let (store, campaign) = town();
    let view = store.snapshot(campaign).unwrap();
    let (pc, orsel, doul, hessa) = (
        id(&view, "Wren"),
        id(&view, "Orsel"),
        id(&view, "Doul"),
        id(&view, "Hessa"),
    );
    let orsel_goal = view
        .goals_active
        .iter()
        .find(|g| g.npc_id == orsel)
        .unwrap()
        .id;

    // Turn 1: the PC insults Orsel in public. He stays polite and has his
    // investigator look into the scribe three days later. Hessa, the clerk,
    // will see the inquiry; the PC will not.
    let mut insult = proposal(Verdict::Allowed);
    insult.action = ActionKind::Talk;
    insult.reactions = vec![Reaction {
        npc: e(orsel),
        stance: Stance::Resist,
        plan: "smiles, will have the scribe investigated".into(),
        cause: None,
    }];
    insult.schedule = Some(EventDraft {
        in_minutes: 3 * DAY,
        actor: e(doul),
        goal: Some(Handle::Goal(orsel_goal)),
        text: "Doul opens an inquiry into the scribe".into(),
        effects: vec![slot(e(pc), "reputation", "under_suspicion")],
        seen_by: vec![e(hessa)],
    });
    insult.minutes = 5;

    // Turns 2-3: four days of travel, to Ostry and back.
    let mut away = proposal(Verdict::Allowed);
    away.action = ActionKind::Move;
    away.facts = vec![fact(slot(e(pc), "location", "Ostry"), Seen::Pc, vec![])];
    away.minutes = 3 * DAY + 12 * 60;
    let mut back = proposal(Verdict::Allowed);
    back.action = ActionKind::Move;
    back.facts = vec![fact(
        slot(e(pc), "location", "Varn town hall"),
        Seen::Pc,
        vec![],
    )];
    back.minutes = 12 * 60;

    let gm = RecordedGm::new(
        vec![json(&insult), json(&away), json(&back)],
        vec![
            "Orsel smiles thinly.".into(),
            "The road to Ostry is long.".into(),
            "You are back in Varn.".into(),
            "Hessa leans close.".into(),
        ],
    );
    let mut engine = Engine::new(store, Box::new(gm.clone()), Box::new(D10), vec![], campaign);
    let play = |engine: &mut Engine, op: &str, intent: &str| {
        engine
            .play_turn(op.into(), intent, &mut |_: &str| {})
            .unwrap_or_else(|err| panic!("{op}: {err}"))
    };

    play(
        &mut engine,
        "t1",
        "I call Orsel a thief in front of everyone.",
    );
    let view = engine.store().snapshot(campaign).unwrap();
    let event = view.events_scheduled.iter().find(|w| w.actors == [doul]);
    let event = event.expect("Doul's inquiry is scheduled").id;
    assert!(!knows_reputation(&view));

    let t2 = play(&mut engine, "t2", "I travel to Ostry.");
    let view = engine.store().snapshot(campaign).unwrap();
    // The inquiry fired during Turn 2, while the PC was in Ostry.
    let fired = engine.store().turn_ops(t2.turn_id).unwrap();
    assert!(fired.contains(&LedgerOp::FireEvent { event_id: event }));
    assert!(view.events_scheduled.iter().all(|w| w.id != event));
    let pc_at = view.live_fact(pc, "location").unwrap().slot.value.clone();
    let doul_at = view.live_fact(doul, "location").unwrap().slot.value.clone();
    assert_eq!(pc_at, "ostry");
    assert_ne!(pc_at, doul_at);
    let rumor = view.live_fact(pc, "reputation").unwrap().clone();
    assert_eq!(rumor.slot.value, "under_suspicion");
    // Hessa witnessed the inquiry; the PC holds nothing about it.
    assert!(beliefs(&view, hessa).iter().any(|b| b.key == "reputation"
        && b.value == "under_suspicion"
        && b.how == Knowing::Witnessed));
    assert!(!knows_reputation(&view));

    play(&mut engine, "t3", "I ride back to Varn.");
    let view = engine.store().snapshot(campaign).unwrap();
    assert!(!knows_reputation(&view), "coming home tells the PC nothing");
    assert!(view.present().contains(&hessa));

    // Turn 4: the PC meets Hessa, who tells what she saw. The Claim cites
    // the fact the inquiry wrote.
    let mut gossip = proposal(Verdict::Allowed);
    gossip.action = ActionKind::Talk;
    gossip.reactions = vec![Reaction {
        npc: e(hessa),
        stance: Stance::Neutral,
        plan: "whispers what Doul did".into(),
        cause: None,
    }];
    gossip.facts = vec![NewFact {
        slot: slot(e(pc), "reputation", "under_suspicion"),
        seen: Seen::Pc,
        said_by: Some(e(hessa)),
        because: vec![f(rumor.id)],
    }];
    gossip.minutes = 10;
    gm.push_proposal(json(&gossip));
    play(
        &mut engine,
        "t4",
        "I ask Hessa what happened while I was away.",
    );
    let view = engine.store().snapshot(campaign).unwrap();
    let told = pc_knowledge(&view)
        .into_iter()
        .find(|b| b.key == "reputation")
        .expect("the PC now knows");
    assert_eq!(told.subject, pc);
    assert_eq!(told.value, "under_suspicion");
    assert_eq!((told.how, told.source), (Knowing::Told, Some(hessa)));

    // The narrator never heard of the inquiry before Turn 4, and hears it then.
    let narrations: Vec<String> = gm
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
    assert_eq!(narrations.len(), 4);
    for prompt in &narrations[..3] {
        assert!(!prompt.contains("under_suspicion"), "{prompt}");
    }
    assert!(
        narrations[3].contains("under_suspicion"),
        "{}",
        narrations[3]
    );
}

fn plan(npc: u64, stance: Stance, plan: &str) -> Reaction {
    Reaction {
        npc: e(npc),
        stance,
        plan: plan.into(),
        cause: None,
    }
}

/// Final fix 3: a reaction plan that names a live fact the PC holds no
/// Belief about (its key or value, as a whole word) is dropped before the
/// narrator sees it. The NPC and the stance stay.
#[test]
fn narrator_never_gets_a_plan_that_quotes_a_secret() {
    let (store, campaign) = town();
    let view = store.snapshot(campaign).unwrap();
    let (orsel, doul, hessa) = (id(&view, "Orsel"), id(&view, "Doul"), id(&view, "Hessa"));
    let mut talk = proposal(Verdict::Allowed);
    talk.action = ActionKind::Talk;
    talk.reactions = vec![
        plan(orsel, Stance::Resist, "hides the Grain  Tithe ledger"),
        plan(doul, Stance::Neutral, "knows_his_master_skims"),
    ];
    let mut later = proposal(Verdict::Allowed);
    later.action = ActionKind::Talk;
    later.reactions = vec![plan(
        hessa,
        Stance::Comply,
        "glances at the door, then smiles",
    )];
    let gm = RecordedGm::new(
        vec![json(&talk), json(&later)],
        vec!["They watch you.".into(), "Hessa smiles.".into()],
    );
    let mut engine = Engine::new(store, Box::new(gm.clone()), Box::new(D10), vec![], campaign);
    for (op, intent) in [("t1", "I greet the room."), ("t2", "I greet Hessa.")] {
        engine
            .play_turn(op.into(), intent, &mut |_: &str| {})
            .unwrap_or_else(|err| panic!("{op}: {err}"));
    }
    let narrations: Vec<String> = gm
        .calls()
        .into_iter()
        .filter(|c| c.kind == CallKind::Narrate)
        .map(|c| {
            let p = c.prompt;
            format!("{}{}{}{}", p.system, p.stable, p.scene, p.recent)
        })
        .collect();
    let all = &narrations[0];
    for secret in ["skims", "grain tithe", "Grain  Tithe", "his_master"] {
        assert!(!all.contains(secret), "{secret} leaked:\n{all}");
    }
    assert!(all.contains("- Orsel (resist)\n"), "{all}");
    assert!(all.contains("- Doul (neutral)\n"), "{all}");
    assert!(
        narrations[1].contains("- Hessa (comply): glances at the door, then smiles\n"),
        "{}",
        narrations[1]
    );
    // The stored Turn keeps the model's reactions as they were.
    let stored = engine
        .store()
        .turn_by_campaign_op(campaign, &"t1".to_owned())
        .unwrap()
        .unwrap();
    assert!(stored.record.proposal_json.contains("Grain  Tithe"));
}

/// Words that only look like a secret do not count: a part of a word, a
/// state key, an entity name. A secret the PC knows is no longer one.
#[test]
fn plan_guard_matches_whole_words_of_unknown_facts_only() {
    let (mut store, campaign) = town();
    let view = store.snapshot(campaign).unwrap();
    let (pc, orsel) = (id(&view, "Wren"), id(&view, "Orsel"));
    let keep = [
        // "skimsy" and "tithes" are not the words "skims" and "tithe".
        "skimsy grain tithes",
        // `condition` is a state key.
        "checks the condition of the archive",
        // The Magistracy's `seat` value is the entity name "Varn town hall".
        "walks back to the Varn town hall",
    ];
    let reactions: Vec<Reaction> = keep
        .iter()
        .map(|p| plan(orsel, Stance::Neutral, p))
        .collect();
    assert_eq!(
        threshold_core::context::guard_plans(&view, &reactions),
        reactions
    );
    let secret = vec![plan(orsel, Stance::Neutral, "pockets the grain tithe")];
    assert_eq!(
        threshold_core::context::guard_plans(&view, &secret)[0].plan,
        ""
    );

    // Once the PC witnessed the secret, a plan may name it.
    let skims = view.live_fact(orsel, "skims").unwrap().id;
    store
        .commit(
            threshold_core::store::TurnRecord {
                campaign_id: campaign,
                op_id: "learn".into(),
                kind: TurnKind::Play,
                intent: "test".into(),
                proposal_json: "{}".into(),
                clock_before: view.clock_min,
                clock_after: view.clock_min,
            },
            vec![LedgerOp::Know {
                holder: threshold_core::domain::EntityRef::Id(pc),
                fact_id: Some(skims),
                claim_id: None,
                how: Knowing::Witnessed,
                arrives_at: view.clock_min,
            }],
        )
        .unwrap();
    let view = store.snapshot(campaign).unwrap();
    assert_eq!(threshold_core::context::guard_plans(&view, &secret), secret);
}
