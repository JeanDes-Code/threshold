//! Presence drift of QA run 5 (2026-11-16), from the shapes of its debug
//! log. The story and the committed state disagreed about the child:
//! - Turn 38: "Tobin bolts out the back door into the dark rain and is
//!   gone." The retry sent no `leaves`, only a `schedule` with
//!   `in_minutes: 0` and `moves_actor_to: away`. The event fired one Turn
//!   later, so HERE, the card and Ask the GM kept Tobin in the room while
//!   the Narration had him gone;
//! - Turn 44: "I call Tobin's name ... until he answers and comes home."
//!   The proposal was a talk with no way to bring Tobin back, and the
//!   narrator told him stumbling in;
//! - the next Intent, "I kneel by the fire and ask Tobin where he ran off
//!   to.", failed twice on `reactions.npc: N1`: Tobin had no handle in the
//!   prompt, so the model made one up;
//! - Turn 35: "Ilse and I walk to the Cooper house." came with no reaction
//!   for Ilse. She stayed, and the Player got no word.

mod support;

use support::*;
use threshold_core::{
    CallKind, Prompt, RecordedGm,
    domain::{EntityRef, Knowing, LedgerOp, Verdict},
    store::{Store, WorldView},
    turn::{Engine, TurnError},
};

/// The gate world with the Coopers: Ada (the PC) stands at the gate square
/// with Marta Cooper (41), Ilse Vale (34) and, when `tobin_here`, Tobin
/// Cooper (10). Otherwise Tobin is at the market. The PC sees everyone who
/// stands with it.
struct Coopers {
    w: World,
    tobin: u64,
    marta: u64,
}

fn coopers(tobin_here: bool) -> Coopers {
    let mut seed = adventure_seed();
    for (name, age) in [
        ("Tobin Cooper", 10),
        ("Marta Cooper", 41),
        ("Ilse Vale", 34),
    ] {
        seed.entities.push(npc(name, age));
    }
    let tobin_at = if tobin_here { "gate square" } else { "market" };
    for (name, at) in [
        ("Tobin Cooper", tobin_at),
        ("Marta Cooper", "gate square"),
        ("Ilse Vale", "gate square"),
    ] {
        seed.decided.push(seed_fact(name, "location", at));
    }
    let mut w = World::with_seed(seed);
    let id = |w: &World, name: &str| w.view.entity_by_name(name).unwrap().id;
    let (tobin, marta) = (id(&w, "Tobin Cooper"), id(&w, "Marta Cooper"));
    let ada = w.ada;
    let ilse = id(&w, "Ilse Vale");
    let mut seen = vec![w.orsel, w.brann, marta, ilse];
    if tobin_here {
        seen.push(tobin);
    }
    let know: Vec<LedgerOp> = seen
        .into_iter()
        .map(|npc| LedgerOp::Know {
            holder: EntityRef::Id(ada),
            fact_id: Some(w.view.live_fact(npc, "location").unwrap().id),
            claim_id: None,
            how: Knowing::Witnessed,
            arrives_at: 0,
        })
        .collect();
    w.commit(know);
    Coopers { w, tobin, marta }
}

fn engine_of(w: &mut World, gm: &RecordedGm) -> Engine {
    let store = std::mem::replace(&mut w.store, Store::open_in_memory().unwrap());
    Engine::new(
        store,
        Box::new(gm.clone()),
        Box::new(threshold_core::rules::D10),
        vec![],
        w.campaign,
    )
}

fn snapshot(engine: &Engine, w: &World) -> WorldView {
    engine.store().snapshot(w.campaign).unwrap()
}

fn prompts(gm: &RecordedGm, kind: CallKind) -> Vec<Prompt> {
    gm.calls()
        .into_iter()
        .filter(|c| c.kind == kind)
        .map(|c| c.prompt)
        .collect()
}

fn play(engine: &mut Engine, intent: &str) -> Result<(), TurnError> {
    engine
        .play_turn("t1".into(), intent, &mut |_: &str| {})
        .map(|_| ())
}

/// The narrator's outcome block of the last narration call.
fn narration(gm: &RecordedGm) -> String {
    prompts(gm, CallKind::Narrate).pop().unwrap().recent
}

/// The PC's sight now: what HERE, the cards and Ask the GM read.
fn sight(engine: &Engine, w: &World) -> threshold_core::world::PcSight {
    let store = engine.store();
    let turns = store.turns(w.campaign).unwrap();
    let reverted = store
        .reverted_turns(w.campaign)
        .unwrap()
        .into_iter()
        .collect();
    threshold_core::world::pc_sight(&snapshot(engine, w), &turns, &reverted).unwrap()
}

/// True when `who` is here in every reader of presence: world truth, what
/// the PC sees (HERE, the narrator), the PC's sight (cards, Ask the GM) and
/// the set the age gate checks.
fn here_everywhere(engine: &Engine, w: &World, who: u64) -> [bool; 4] {
    let view = snapshot(engine, w);
    [
        view.present().contains(&who),
        threshold_core::context::seen_present(&view).contains(&who),
        sight(engine, w).present.contains(&who),
        threshold_core::world::room(&view).checked().contains(&who),
    ]
}

/// The Turn 38 retry of QA run 5, as the model sent it: a talk, a Check
/// against Tobin with no stakes, Tobin's reaction, and a schedule due at
/// once that sends Tobin away.
fn bolts(c: &Coopers) -> String {
    let mut p = serde_json::to_value(proposal(Verdict::Check)).unwrap();
    let tobin = format!("E{}", c.tobin);
    p["action"] = "talk".into();
    p["minutes"] = 5.into();
    p["check"] = serde_json::json!({"against": tobin, "factors": [], "win": null, "lose": null});
    p["reactions"] = serde_json::json!([
        {"npc": tobin, "plan": "run_to_riverbank", "stance": "resist", "willing": "no", "cause": null}
    ]);
    p["schedule"] = serde_json::json!({
        "actor": tobin,
        "effects": [{"key": "location", "subject": tobin, "value": "unknown_place"}],
        "goal": null,
        "in_minutes": 0,
        "moves_actor_to": "away",
        "public": false,
        "text": "Tobin bolts into the rain and vanishes into the gloom.",
        "seen_by": [format!("E{}", c.marta)],
    });
    p.to_string()
}

// --- 1: the story removes the child, the state keeps him -------------------

/// QA run 5, Turn 38: a schedule due at once (`in_minutes: 0`) that sends a
/// present NPC away is a departure of THIS Turn. Every reader of presence
/// loses Tobin at the Commit, and the narrator hears that he leaves.
#[test]
fn a_schedule_due_now_that_sends_a_present_npc_away_moves_it_this_turn() {
    let mut c = coopers(true);
    let gm = RecordedGm::new(
        vec![bolts(&c)],
        vec![
            "Tobin bolts.".into(),
            "Tobin Cooper is not here with you.".into(),
        ],
    );
    let mut engine = engine_of(&mut c.w, &gm);
    play(
        &mut engine,
        "Tobin bolts out the back door into the dark rain and is gone. Marta and I run to the doorway, but nobody can see where he went.",
    )
    .unwrap();
    // The age gate still counts him: `away` places him nowhere, and the
    // gate takes doubt as "here" (it fails closed).
    assert_eq!(
        here_everywhere(&engine, &c.w, c.tobin),
        [false, false, false, true]
    );
    let n = narration(&gm);
    assert!(n.contains("- Tobin Cooper leaves.\n"), "{n}");
    let here = n
        .lines()
        .find(|l| l.starts_with(threshold_core::context::HERE_AFTER_LINE))
        .unwrap();
    assert!(here.contains("Marta Cooper"), "{here}");
    assert!(!here.contains("Tobin"), "{here}");
    // No event is left to move him again a Turn later.
    let view = snapshot(&engine, &c.w);
    assert!(
        !view
            .events_scheduled
            .iter()
            .any(|w| w.actors.contains(&c.tobin)),
        "{:?}",
        view.events_scheduled
    );
    // Ask the GM reads the same committed presence.
    engine
        .ooc("ooc-1".into(), "Where is Tobin Cooper right now?")
        .unwrap();
    let ooc = prompts(&gm, CallKind::Narrate).pop().unwrap();
    assert!(
        ooc.recent.contains("Tobin Cooper: not here with you"),
        "{}",
        ooc.recent
    );
    assert!(!ooc.scene.contains("Here with you: Tobin"), "{}", ooc.scene);
}
