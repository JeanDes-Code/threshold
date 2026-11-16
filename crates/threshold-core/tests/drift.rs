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
    domain::{ActionKind, DetailLevel, EntityRef, Intimacy, Knowing, LedgerOp, Stance, Verdict},
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
    ilse: u64,
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
    let (tobin, marta, ilse) = (
        id(&w, "Tobin Cooper"),
        id(&w, "Marta Cooper"),
        id(&w, "Ilse Vale"),
    );
    let ada = w.ada;
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
    Coopers {
        w,
        tobin,
        marta,
        ilse,
    }
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

/// The Turn 44 proposal of QA run 5: a talk, Marta's reaction, nothing
/// else. `arrives` adds the field the fix gives the model.
fn calls_home(c: &Coopers, arrives: &[u64]) -> String {
    let mut p = serde_json::to_value(proposal(Verdict::Allowed)).unwrap();
    p["action"] = "talk".into();
    p["minutes"] = 5.into();
    p["reactions"] = serde_json::json!([
        {"npc": format!("E{}", c.marta), "plan": "Continue watching the dark lane for Tobin",
         "stance": "neutral", "willing": "yes", "cause": null}
    ]);
    if !arrives.is_empty() {
        let handles: Vec<String> = arrives.iter().map(|id| format!("E{id}")).collect();
        p["arrives"] = serde_json::json!(handles);
    }
    p.to_string()
}

const CALL_HOME: &str =
    "I call Tobin's name loudly down the lane, again and again, until he answers and comes home.";

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

// --- 2: the story brings the child back, the state does not ----------------

/// QA run 5, Turn 44: an NPC the Intent names, who is not here, can come
/// back through `arrives`. The schema offers him, the prompt shows his
/// handle, and the core writes his `location`: HERE, the narrator, the card
/// and Ask the GM all have him here.
#[test]
fn an_npc_the_intent_calls_home_arrives_through_the_proposal() {
    let mut c = coopers(false);
    let gm = RecordedGm::new(
        vec![calls_home(&c, &[c.tobin])],
        vec![
            "Tobin stumbles in.".into(),
            "Tobin Cooper is here with you.".into(),
        ],
    );
    let mut engine = engine_of(&mut c.w, &gm);
    assert_eq!(here_everywhere(&engine, &c.w, c.tobin)[..3], [false; 3]);
    play(&mut engine, CALL_HOME).unwrap();
    let call = gm
        .calls()
        .into_iter()
        .find(|c| c.kind == CallKind::Propose)
        .unwrap();
    let tobin = format!("E{}", c.tobin);
    assert_eq!(
        call.schema["properties"]["arrives"]["items"]["enum"],
        serde_json::json!([tobin]),
        "{}",
        call.schema["properties"]
    );
    assert!(
        call.prompt
            .recent
            .contains(&format!("{tobin} Tobin Cooper")),
        "{}",
        call.prompt.recent
    );
    assert_eq!(here_everywhere(&engine, &c.w, c.tobin), [true; 4]);
    let n = narration(&gm);
    assert!(n.contains("Tobin Cooper"), "{n}");
    assert!(!n.contains("Tobin Cooper is not here"), "{n}");
    engine
        .ooc("ooc-1".into(), "Where is Tobin Cooper right now?")
        .unwrap();
    let ooc = prompts(&gm, CallKind::Narrate).pop().unwrap();
    assert!(
        ooc.recent.contains("Tobin Cooper: here with you."),
        "{}",
        ooc.recent
    );
}

/// The same Turn when the model sends no arrival (QA run 5's own proposal):
/// Tobin stays away, and the narrator is told so by name, so the story
/// does not bring him in.
#[test]
fn the_narrator_hears_that_a_named_npc_who_did_not_come_is_not_here() {
    let mut c = coopers(false);
    let gm = RecordedGm::new(vec![calls_home(&c, &[])], vec!["You call.".into()]);
    let mut engine = engine_of(&mut c.w, &gm);
    play(&mut engine, CALL_HOME).unwrap();
    assert_eq!(here_everywhere(&engine, &c.w, c.tobin)[..3], [false; 3]);
    let n = narration(&gm);
    assert!(
        n.contains("- Tobin Cooper is not here and does not come in this Turn: do not show Tobin Cooper here.\n"),
        "{n}"
    );
}

/// An arrival applies only when the act happens: a lost Check or an
/// impossible verdict brings nobody, as for `move_to`.
#[test]
fn an_arrival_needs_the_act_to_happen() {
    let mut c = coopers(false);
    let mut p: serde_json::Value = serde_json::from_str(&calls_home(&c, &[c.tobin])).unwrap();
    p["verdict"] = "impossible".into();
    let gm = RecordedGm::new(vec![p.to_string()], vec!["Nobody comes.".into()]);
    let mut engine = engine_of(&mut c.w, &gm);
    play(&mut engine, CALL_HOME).unwrap();
    assert_eq!(here_everywhere(&engine, &c.w, c.tobin)[..3], [false; 3]);
}

/// The model may only bring an NPC the Intent names: an arrival of anyone
/// else is dropped (here Pip, whom the Intent does not name).
#[test]
fn an_arrival_the_intent_does_not_name_is_dropped() {
    let mut c = coopers(false);
    let pip = c.w.pip;
    let gm = RecordedGm::new(vec![calls_home(&c, &[pip])], vec!["You call.".into()]);
    let mut engine = engine_of(&mut c.w, &gm);
    play(&mut engine, CALL_HOME).unwrap();
    assert!(!snapshot(&engine, &c.w).present().contains(&pip));
}

// --- 3: the Player cannot talk to him ---------------------------------------

/// QA run 5, the failed Turn after Turn 45: "I kneel by the fire and ask
/// Tobin where he ran off to." Tobin is away and the Intent names him by
/// his first name only. His handle now reaches the prompt, and a reaction
/// on an N handle that `new_npcs` never declares is dropped instead of
/// failing the Turn.
#[test]
fn a_first_name_in_the_intent_shows_the_npc_and_an_undeclared_n_handle_does_not_fail_the_turn() {
    let mut c = coopers(false);
    let n1 = || {
        let mut p = serde_json::to_value(proposal(Verdict::Allowed)).unwrap();
        p["action"] = "talk".into();
        p["minutes"] = 5.into();
        p["reactions"] = serde_json::json!([
            {"npc": "N1", "plan": "check on the boy", "stance": "comply", "willing": "yes", "cause": null}
        ]);
        p.to_string()
    };
    let gm = RecordedGm::new(vec![n1(), n1()], vec!["You kneel.".into()]);
    let mut engine = engine_of(&mut c.w, &gm);
    play(
        &mut engine,
        "I kneel by the fire and ask Tobin where he ran off to.",
    )
    .expect("the Turn commits");
    let first = prompts(&gm, CallKind::Propose).remove(0);
    assert!(
        first.recent.contains(&format!("E{} Tobin Cooper", c.tobin)),
        "{}",
        first.recent
    );
}

/// The age gate reads handles: on a Turn it runs for, the reaction on an
/// undeclared N handle stays, so the references check still rejects it
/// (it fails closed). On a plain Turn it goes.
#[test]
fn an_undeclared_n_handle_stays_on_a_gated_turn() {
    let c = coopers(false);
    for (intimacy, kept) in [(Intimacy::None, 0), (Intimacy::Advance, 1)] {
        let mut p = proposal(Verdict::Allowed);
        p.intimacy = intimacy;
        p.reactions = vec![reaction(
            threshold_core::domain::Handle::NewNpc(1),
            Stance::Comply,
            None,
        )];
        let (p, _) = threshold_core::checks::settle(p, &c.w.view);
        assert_eq!(p.reactions.len(), kept, "{intimacy:?}");
    }
}

// --- the age gate reads an arrival -------------------------------------------

/// A minor the Intent calls in refuses an intimate Turn: the gate reads
/// `arrives` as names, and the arrival puts the minor in the room.
#[test]
fn a_minor_who_arrives_refuses_an_intimate_turn() {
    let mut w = World::with_detail(DetailLevel::Implied);
    let ids = [w.ada, w.orsel, w.brann];
    w.confirm(&ids);
    let mut p = proposal(Verdict::Allowed);
    p.intimacy = Intimacy::Intimate;
    p.reactions = vec![reaction(e(w.brann), Stance::Comply, None)];
    let mut raw = serde_json::to_value(&p).unwrap();
    raw["arrives"] = serde_json::json!([format!("E{}", w.pip)]);
    let gm = RecordedGm::new(vec![raw.to_string()], vec!["They kiss.".into()]);
    let mut engine = engine_of(&mut w, &gm);
    let result = play(&mut engine, "I call Pip over, then make love with Brann.");
    assert!(
        matches!(result, Err(TurnError::Refused { .. })),
        "{result:?}"
    );
}

// --- 4: a companion missed ----------------------------------------------------

/// QA run 5, Turn 35: "Ilse and I walk to the Cooper house." came with no
/// reaction for Ilse, so she stayed (ADR 0007). The Player now reads it
/// under the Turn.
#[test]
fn a_named_companion_who_stays_behind_gets_a_note_for_the_player() {
    let mut c = coopers(false);
    let ferry = c.w.add_place("ferry house");
    let mut p = proposal(Verdict::Allowed);
    p.action = ActionKind::Move;
    p.minutes = 15;
    p.move_to = Some(e(ferry));
    let gm = RecordedGm::new(
        vec![serde_json::to_string(&p).unwrap()],
        vec!["You walk.".into()],
    );
    let mut engine = engine_of(&mut c.w, &gm);
    play(&mut engine, "Ilse and I walk to the ferry house.").unwrap();
    assert!(!sight(&engine, &c.w).present.contains(&c.ilse));
    let play = threshold_core::view::play_view(&sight(&engine, &c.w), &Default::default());
    let story = serde_json::to_value(&play).unwrap()["story"].clone();
    let last = story.as_array().unwrap().last().unwrap().clone();
    assert_eq!(
        last["move_note"],
        serde_json::json!("Ilse Vale stays behind."),
        "{last}"
    );
}
