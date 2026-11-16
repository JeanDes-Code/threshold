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
    coopers_at(tobin_here, DetailLevel::default())
}

/// `coopers` at Detail Level `level`.
fn coopers_at(tobin_here: bool, level: DetailLevel) -> Coopers {
    let mut seed = adventure_seed();
    seed.detail = level;
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
        // Review r5 (m5): kept is not enough. The gated Turn is then
        // rejected on that handle; the plain one passes.
        let result = threshold_core::checks::validate(&p, &c.w.view);
        if kept == 0 {
            assert!(result.is_ok(), "{intimacy:?}: {:?}", result.err());
        } else {
            let errs = result.err().expect("the N handle is rejected");
            assert!(
                errs.iter()
                    .any(|r| r.handle == Some(threshold_core::domain::Handle::NewNpc(1))),
                "{errs:?}"
            );
        }
    }
}

/// The same N reaction, through the engine, on a Turn the Intent's words
/// gate: both attempts keep it, so the Turn fails and nothing is committed.
#[test]
fn an_undeclared_n_handle_fails_a_gated_turn() {
    let mut c = coopers(false);
    let n1 = || {
        let mut p = serde_json::to_value(proposal(Verdict::Allowed)).unwrap();
        p["intimacy"] = "advance".into();
        p["reactions"] = serde_json::json!([
            {"npc": "N1", "plan": "lean in", "stance": "comply", "willing": "yes", "cause": null}
        ]);
        p.to_string()
    };
    let gm = RecordedGm::new(vec![n1(), n1()], vec!["You kiss.".into()]);
    let mut engine = engine_of(&mut c.w, &gm);
    let before = engine.store().turns(c.w.campaign).unwrap().len();
    let result = play(&mut engine, "I kiss the stranger by the fire.");
    assert!(result.is_err(), "{result:?}");
    assert!(prompts(&gm, CallKind::Narrate).is_empty());
    assert_eq!(engine.store().turns(c.w.campaign).unwrap().len(), before);
}

// --- the age gate reads an arrival -------------------------------------------

/// `result` is the final `AdultsOnly` Refusal.
fn adults_only(result: &Result<(), TurnError>) -> bool {
    matches!(
        result,
        Err(TurnError::Refused {
            kind: threshold_core::mature::RefusalKind::AdultsOnly,
            ..
        })
    )
}

/// The confirmed gate world at Detail Level `implied` (Pip, 17, is at the
/// market), with `raw` as the only proposal: one answer for both attempts.
fn gated_engine(raw: &str) -> (World, RecordedGm, Engine) {
    let mut w = World::with_detail(DetailLevel::Implied);
    let ids = [w.ada, w.orsel, w.brann];
    w.confirm(&ids);
    let gm = RecordedGm::new(
        vec![raw.to_owned(), raw.to_owned()],
        vec!["They kiss.".into()],
    );
    let engine = engine_of(&mut w, &gm);
    (w, gm, engine)
}

/// A proposal on Brann tagged `intimacy`, with `arrives` as given.
fn on_brann(w: &World, intimacy: &str, arrives: &[u64]) -> String {
    let mut p = proposal(Verdict::Allowed);
    p.reactions = vec![reaction(e(w.brann), Stance::Comply, None)];
    let mut raw = serde_json::to_value(&p).unwrap();
    raw["intimacy"] = intimacy.into();
    let handles: Vec<String> = arrives.iter().map(|id| format!("E{id}")).collect();
    raw["arrives"] = serde_json::json!(handles);
    raw.to_string()
}

/// A minor the Intent calls in refuses an intimate Turn: the gate reads
/// `arrives` as names, and the arrival puts the minor in the room. Review
/// r5 re-check (B1): "make love" gates the Intent, so the schema offers
/// nobody for `arrives`; `settle` used to drop Pip as not offered, and the
/// Turn committed. On a gated Turn `settle` now keeps every `arrives`
/// entry, so the gate sees Pip.
#[test]
fn a_minor_who_arrives_refuses_an_intimate_turn() {
    let w = World::with_detail(DetailLevel::Implied);
    let raw = on_brann(&w, "intimate", &[w.pip]);
    let (_w, gm, mut engine) = gated_engine(&raw);
    let result = play(&mut engine, "I call Pip over, then make love with Brann.");
    assert!(adults_only(&result), "{result:?}");
    let call = gm.calls().into_iter().next().unwrap();
    assert!(call.schema["properties"].get("arrives").is_none());
    assert!(prompts(&gm, CallKind::Narrate).is_empty());
}

/// The same with an advance (review r5 re-check, B1): "kiss" gates the
/// Intent, and the model brings Pip in anyway.
#[test]
fn a_minor_who_arrives_refuses_an_advance() {
    let w = World::with_detail(DetailLevel::Implied);
    let raw = on_brann(&w, "advance", &[w.pip]);
    let (_w, gm, mut engine) = gated_engine(&raw);
    let result = play(&mut engine, "I wave Pip in and kiss Brann.");
    assert!(adults_only(&result), "{result:?}");
    assert!(prompts(&gm, CallKind::Narrate).is_empty());
}

/// No word of the Intent gates the Turn, so the schema offers Pip; the
/// model alone tags it intimate, and the gate reads the offered arrival.
#[test]
fn a_minor_offered_for_arrives_refuses_a_turn_the_model_tags_intimate() {
    let w = World::with_detail(DetailLevel::Implied);
    let raw = on_brann(&w, "intimate", &[w.pip]);
    let (w, gm, mut engine) = gated_engine(&raw);
    let result = play(&mut engine, "I call Pip over, then lie down with Brann.");
    assert!(adults_only(&result), "{result:?}");
    let call = gm.calls().into_iter().next().unwrap();
    assert_eq!(
        call.schema["properties"]["arrives"]["items"]["enum"],
        serde_json::json!([format!("E{}", w.pip)])
    );
}

/// A confirmed adult named in `arrives` on a gated Turn (review r5
/// re-check, B1): nobody may arrive on a gated Turn, so the references
/// check rejects the entry on both attempts. The Turn fails; it does not
/// commit with the arrival silently taken out.
#[test]
fn an_adult_who_arrives_on_a_gated_turn_fails_the_turn() {
    let mut seed = adventure_seed();
    seed.detail = DetailLevel::Implied;
    seed.entities.push(npc("Hal Reed", 40));
    seed.decided
        .push(seed_fact("Hal Reed", "location", "market"));
    let mut w = World::with_seed(seed);
    let hal = w.view.entity_by_name("Hal Reed").unwrap().id;
    let know = LedgerOp::Know {
        holder: EntityRef::Id(w.ada),
        fact_id: Some(w.view.live_fact(hal, "location").unwrap().id),
        claim_id: None,
        how: Knowing::Witnessed,
        arrives_at: 0,
    };
    w.commit(vec![know]);
    let ids = [w.ada, w.orsel, w.brann, hal];
    w.confirm(&ids);
    let raw = on_brann(&w, "advance", &[hal]);
    let gm = RecordedGm::new(vec![raw.clone(), raw], vec!["You kiss.".into()]);
    let mut engine = engine_of(&mut w, &gm);
    let before = engine.store().turns(w.campaign).unwrap().len();
    let result = play(&mut engine, "I call Hal over and kiss Brann.");
    assert!(matches!(result, Err(TurnError::Rejected(_))), "{result:?}");
    assert!(prompts(&gm, CallKind::Narrate).is_empty());
    assert_eq!(engine.store().turns(w.campaign).unwrap().len(), before);
    assert!(!snapshot(&engine, &w).present().contains(&hal));
}

/// The confirmed gate world at Detail Level `implied`, and an intimate
/// proposal on Brann that brings Pip (17, at the market) in.
fn pip_arrives() -> (World, threshold_core::domain::DecisionProposal) {
    let mut w = World::with_detail(DetailLevel::Implied);
    let ids = [w.ada, w.orsel, w.brann];
    w.confirm(&ids);
    let mut p = proposal(Verdict::Allowed);
    p.intimacy = Intimacy::Intimate;
    p.reactions = vec![reaction(e(w.brann), Stance::Comply, None)];
    p.arrives = vec![e(w.pip)];
    p.may_arrive = vec![e(w.pip)];
    (w, p)
}

/// G2 reads `arrives` as named (review r5, m5): Pip is away before the
/// Turn, so only the name refuses. Without `arrives` in the gate's list,
/// this passes.
#[test]
fn the_gate_before_the_turn_reads_arrives_as_named() {
    let (w, p) = pip_arrives();
    let mut errs = Vec::new();
    threshold_core::mature::check(&p, &w.view, &mut errs);
    assert!(errs.iter().any(|r| r.handle == Some(e(w.pip))), "{errs:?}");
}

/// G3 reads the arrival's `location` op (review r5, m5): the ops of the
/// proposal put Pip at the PC's place, so the gate after the Turn's ops
/// blocks him. Without the arrival in `to_ops`, Pip stays at the market
/// and this passes.
#[test]
fn the_gate_after_the_ops_reads_the_arrival() {
    let (w, gated) = pip_arrives();
    // `validate` runs G2, which already refuses Pip: build the ops from
    // the same proposal untagged, then ask G3 with the tag.
    let mut plain = gated.clone();
    plain.intimacy = Intimacy::None;
    let valid = threshold_core::checks::validate(&plain, &w.view).unwrap();
    let ops = threshold_core::checks::to_ops(valid, None, &w.view).ops;
    let after = threshold_core::mature::after(&w.view, &ops);
    assert!(after.checked.contains(&w.pip), "{:?}", after.checked);
    let blocked = threshold_core::mature::check_after(&gated, &w.view, &after)
        .expect_err("Pip is here after the ops");
    assert_eq!(blocked.who, vec![w.pip]);
}

/// A minor who leaves through a schedule due now (`leave_now`, QA run 5)
/// on an intimate Turn is still counted: G2 reads the place before the
/// Turn, so the Turn is refused (review r5, m5).
#[test]
fn a_minor_who_leaves_now_still_refuses_an_intimate_turn() {
    let mut c = coopers_at(true, DetailLevel::Implied);
    let ids = [c.w.ada, c.w.orsel, c.w.brann, c.marta, c.ilse];
    c.w.confirm(&ids);
    let mut p: serde_json::Value = serde_json::from_str(&bolts(&c)).unwrap();
    p["intimacy"] = "intimate".into();
    p["check"] = serde_json::Value::Null;
    p["verdict"] = "allowed".into();
    p["reactions"] = serde_json::json!([
        {"npc": format!("E{}", c.w.brann), "plan": "go along", "stance": "comply", "willing": "yes", "cause": null}
    ]);
    // The settled proposal moves Tobin out in THIS Turn.
    let parsed: threshold_core::domain::DecisionProposal =
        serde_json::from_value(p.clone()).unwrap();
    let (settled, _) = threshold_core::checks::settle(parsed, &c.w.view);
    assert!(
        settled.leaves.iter().any(|l| l.npc == e(c.tobin)),
        "{:?}",
        settled.leaves
    );
    let gm = RecordedGm::new(vec![p.to_string()], vec!["They kiss.".into()]);
    let mut engine = engine_of(&mut c.w, &gm);
    let result = play(&mut engine, "I make love with Brann while Tobin runs off.");
    assert!(
        matches!(
            &result,
            Err(TurnError::Refused {
                kind: threshold_core::mature::RefusalKind::AdultsOnly,
                ..
            })
        ),
        "{result:?}"
    );
    assert!(prompts(&gm, CallKind::Narrate).is_empty());
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

// --- review r5, M1: a first word is not a name on a gated Turn ---------------

/// The gate world with Little Wren (9), a child at the market whose place
/// the PC knows, so the PC can name her. Ada, Orsel and Brann are
/// confirmed adults, at Detail Level `implied`.
fn wren_world() -> (World, u64) {
    let mut seed = adventure_seed();
    seed.detail = DetailLevel::Implied;
    seed.entities.push(npc("Little Wren", 9));
    seed.decided
        .push(seed_fact("Little Wren", "location", "market"));
    let mut w = World::with_seed(seed);
    let wren = w.view.entity_by_name("Little Wren").unwrap().id;
    let ada = w.ada;
    let know = LedgerOp::Know {
        holder: EntityRef::Id(ada),
        fact_id: Some(w.view.live_fact(wren, "location").unwrap().id),
        claim_id: None,
        how: Knowing::Witnessed,
        arrives_at: 0,
    };
    w.commit(vec![know]);
    let ids = [w.ada, w.orsel, w.brann];
    w.confirm(&ids);
    (w, wren)
}

/// An advance on Brann, with `arrives` as given.
fn kiss_brann(w: &World, arrives: &[u64]) -> String {
    let mut p = proposal(Verdict::Allowed);
    p.intimacy = Intimacy::Advance;
    p.reactions = vec![reaction(e(w.brann), Stance::Comply, None)];
    let mut raw = serde_json::to_value(&p).unwrap();
    let handles: Vec<String> = arrives.iter().map(|id| format!("E{id}")).collect();
    raw["arrives"] = serde_json::json!(handles);
    raw.to_string()
}

const KISS: &str = "I take a little time to lean in and kiss Brann.";

/// Review r5 (M1): "a little time" holds the first word of Little Wren.
/// On a Turn the age rule runs for, the prompt shows no Little Wren, the
/// schema offers no `arrives`, the narrator gets no "is not here" line for
/// her, so the Turn is not refused for a child the Intent never named.
#[test]
fn a_first_word_does_not_bring_a_minor_into_a_gated_turn() {
    let (mut w, wren) = wren_world();
    let gm = RecordedGm::new(vec![kiss_brann(&w, &[])], vec!["You kiss.".into()]);
    let mut engine = engine_of(&mut w, &gm);
    play(&mut engine, KISS).expect("the advance commits");
    let call = gm
        .calls()
        .into_iter()
        .find(|c| c.kind == CallKind::Propose)
        .unwrap();
    assert!(
        call.schema["properties"].get("arrives").is_none(),
        "{}",
        call.schema["properties"]["arrives"]
    );
    for prompt in prompts(&gm, CallKind::Propose)
        .into_iter()
        .chain(prompts(&gm, CallKind::Narrate))
    {
        for block in [&prompt.scene, &prompt.recent] {
            assert!(!block.contains("Wren"), "{block}");
            assert!(!block.contains(&format!("E{wren}")), "{block}");
        }
    }
    assert!(!snapshot(&engine, &w).present().contains(&wren));
}

/// A model that names Little Wren in `arrives` anyway, on that gated
/// Turn, ends it in the final `AdultsOnly` Refusal (review r5 re-check,
/// B1): the gate fails closed, so `settle` keeps the entry for it to read.
#[test]
fn a_minor_the_model_brings_into_a_gated_turn_refuses_it() {
    let (mut w, wren) = wren_world();
    let raw = kiss_brann(&w, &[wren]);
    let gm = RecordedGm::new(vec![raw.clone(), raw], vec!["You kiss.".into()]);
    let mut engine = engine_of(&mut w, &gm);
    let result = play(&mut engine, KISS);
    assert!(adults_only(&result), "{result:?}");
    assert!(prompts(&gm, CallKind::Narrate).is_empty());
}

/// The same first word on a plain Turn still offers the NPC for `arrives`
/// (ADR 0007, QA run 5): the age rule does not run, so the cost is one
/// more offered handle.
#[test]
fn a_first_word_still_offers_the_arriver_on_a_plain_turn() {
    let (w, wren) = wren_world();
    let handles = threshold_core::context::handle_context(&w.view, "I call Little home.");
    assert_eq!(handles.arrivers, vec![e(wren)]);
    let schema = threshold_core::proposal_schema_for(&handles);
    let arrives = &schema["properties"]["arrives"];
    assert_eq!(
        arrives["items"]["enum"],
        serde_json::json!([format!("E{wren}")])
    );
    // One offered handle is never forced: an empty list is a valid answer.
    assert!(arrives.get("minItems").is_none_or(|m| m == 0), "{arrives}");
}

/// The prompt helpers read the Intent's words as the core does: on a
/// gated Intent (a Romance Word here), `handle_context` offers nobody.
#[test]
fn a_gated_intent_offers_no_arriver() {
    let (w, _) = wren_world();
    let handles = threshold_core::context::handle_context(&w.view, KISS);
    assert!(handles.arrivers.is_empty(), "{:?}", handles.arrivers);
    let schema = threshold_core::proposal_schema_for(&handles);
    assert!(schema["properties"].get("arrives").is_none());
}

// --- review r5 re-check, MJ1: an open Intimate Scene gates the prompt ------

/// Plays an intimate Turn on Brann in `w` ("t1"), which opens the Intimate
/// Scene, then the plain `intent` ("t2") with a plain proposal. Returns
/// the recorder and the result of t2.
fn in_scene(w: &mut World, intent: &str) -> (RecordedGm, Result<(), TurnError>) {
    let gm = RecordedGm::new(
        vec![on_brann(w, "intimate", &[]), on_brann(w, "none", &[])],
        vec!["You lie together.".into(), "You stay.".into()],
    );
    let mut engine = engine_of(w, &gm);
    play(&mut engine, "I make love with Brann.").expect("the scene opens");
    assert!(snapshot(&engine, w).scene_open.is_some());
    let result = engine
        .play_turn("t2".into(), intent, &mut |_: &str| {})
        .map(|_| ());
    (gm, result)
}

/// The schema of the last proposal call.
fn last_schema(gm: &RecordedGm) -> serde_json::Value {
    gm.calls()
        .into_iter()
        .filter(|c| c.kind == CallKind::Propose)
        .next_back()
        .unwrap()
        .schema
}

/// Review r5 re-check (MJ1): inside an open Intimate Scene, an Intent with
/// no Romance Word ("think of little Pip") offered Pip, a minor away at
/// the market, for `arrives`. A model that took him ended the Turn in a
/// final Refusal. A Turn of an open scene is gated before the model
/// answers: nobody is offered. The Intent names Pip by his full name, so
/// the narrator still hears that he is not here (r5 second re-check, N1).
#[test]
fn an_open_scene_offers_no_arriver() {
    let mut w = World::with_detail(DetailLevel::Implied);
    let ids = [w.ada, w.orsel, w.brann];
    w.confirm(&ids);
    let (gm, result) = in_scene(&mut w, "I stay with Brann and think of little Pip.");
    result.expect("the plain Turn commits");
    let schema = last_schema(&gm);
    assert!(
        schema["properties"].get("arrives").is_none(),
        "{}",
        schema["properties"]["arrives"]
    );
    let n = narration(&gm);
    assert!(n.contains(PIP_NOT_HERE), "{n}");
}

/// Review r5 re-check (MJ1): inside an open Intimate Scene, a first word
/// is no name. "a little longer" holds the first word of Little Wren (9):
/// no prompt of that Turn shows her, and the schema offers nobody.
#[test]
fn a_first_word_is_no_name_in_an_open_scene() {
    let (mut w, wren) = wren_world();
    let (gm, result) = in_scene(&mut w, "I stay with Brann a little longer.");
    result.expect("the plain Turn commits");
    assert!(last_schema(&gm)["properties"].get("arrives").is_none());
    let calls = gm.calls();
    for call in &calls[calls.len() - 2..] {
        for block in [&call.prompt.scene, &call.prompt.recent] {
            assert!(!block.contains("Wren"), "{block}");
            assert!(!block.contains(&format!("E{wren}")), "{block}");
        }
    }
}

// --- review r5 second re-check, N1: an open scene drops no arrival --------

/// The narrator's line for Pip, away at the market, when the Intent names
/// him and he does not come.
const PIP_NOT_HERE: &str =
    "- Pip is not here and does not come in this Turn: do not show Pip here.";

/// The confirmed gate world at Detail Level `implied` (Pip, 17, at the
/// market). The recorder answers an intimate t1 on Brann (it opens the
/// Intimate Scene), then each proposal of `t2`, then an intimate t3 on
/// Brann; it holds `narrated` narrations. t1 is played.
fn scene_rig(t2: impl Fn(&World) -> Vec<String>, narrated: usize) -> (World, RecordedGm, Engine) {
    let mut w = World::with_detail(DetailLevel::Implied);
    let ids = [w.ada, w.orsel, w.brann];
    w.confirm(&ids);
    let mut proposals = vec![on_brann(&w, "intimate", &[])];
    proposals.extend(t2(&w));
    proposals.push(on_brann(&w, "intimate", &[]));
    let narrations = (0..narrated).map(|i| format!("Narration {i}.")).collect();
    let gm = RecordedGm::new(proposals, narrations);
    let mut engine = engine_of(&mut w, &gm);
    play(&mut engine, "I make love with Brann.").expect("the scene opens");
    assert!(snapshot(&engine, &w).scene_open.is_some());
    (w, gm, engine)
}

/// Plays `intent` as Turn `id`.
fn turn(engine: &mut Engine, id: &str, intent: &str) -> Result<(), TurnError> {
    engine
        .play_turn(id.into(), intent, &mut |_: &str| {})
        .map(|_| ())
}

/// Review r5 second re-check (N1), the probe: in an open scene the Intent
/// "I call Pip over." comes back `none` with `arrives: [Pip]`. The schema
/// offered nobody, and `settle` dropped Pip as not offered: the Turn
/// committed, Pip stayed away, and the narrator heard nothing about him.
/// Now `settle` keeps the entry in an open scene, so the references check
/// rejects it on both attempts: the Turn fails and nothing is committed.
/// The next intimate Turn reads no Turn that called Pip.
#[test]
fn an_arrival_in_an_open_scene_is_never_dropped_silently() {
    let (w, gm, mut engine) = scene_rig(
        |w| {
            let raw = on_brann(w, "none", &[w.pip]);
            vec![raw.clone(), raw]
        },
        2,
    );
    let before = engine.store().turns(w.campaign).unwrap().len();
    let t2 = turn(&mut engine, "t2", "I call Pip over.");
    assert!(matches!(t2, Err(TurnError::Rejected(_))), "{t2:?}");
    assert_eq!(prompts(&gm, CallKind::Narrate).len(), 1);
    assert_eq!(engine.store().turns(w.campaign).unwrap().len(), before);
    let now = snapshot(&engine, &w);
    assert!(now.scene_open.is_some());
    assert!(!now.present().contains(&w.pip));
    turn(&mut engine, "t3", "I make love with Brann again.").expect("t3 commits");
    let n = narration(&gm);
    assert!(!n.contains("call Pip"), "{n}");
    assert!(!snapshot(&engine, &w).present().contains(&w.pip));
}

/// Review r5 second re-check (N1): the same Intent, and the model sends no
/// `arrives`. Pip stays at the market, and the narrator of t2 hears it, as
/// outside a scene: the Intent names him by his full name. The next
/// intimate Turn commits, and Pip is still away.
#[test]
fn a_full_name_in_an_open_scene_gets_the_not_here_line() {
    let (w, gm, mut engine) = scene_rig(|w| vec![on_brann(w, "none", &[])], 3);
    turn(&mut engine, "t2", "I call Pip over.").expect("t2 commits");
    let n = narration(&gm);
    assert!(n.contains(PIP_NOT_HERE), "{n}");
    let now = snapshot(&engine, &w);
    assert!(now.scene_open.is_some());
    assert!(!now.present().contains(&w.pip));
    turn(&mut engine, "t3", "I make love with Brann again.").expect("t3 commits");
    assert!(!snapshot(&engine, &w).present().contains(&w.pip));
}

/// Review r5 second re-check (N1): the "is not here" line in an open scene
/// reads full names only. "a little longer" holds the first word of Little
/// Wren (9): the narrator gets no line for her (review r5, M1).
#[test]
fn a_first_word_gets_no_not_here_line_in_an_open_scene() {
    let (mut w, _) = wren_world();
    let (gm, result) = in_scene(&mut w, "I stay with Brann a little longer.");
    result.expect("the plain Turn commits");
    let n = narration(&gm);
    assert!(!n.contains("Wren"), "{n}");
    assert!(!n.contains("is not here"), "{n}");
}

// --- MJ-a: a gated Turn outside a scene hears who does not come -----------

/// MJ-a, the probe: "I wave Pip in and kiss Brann." outside a scene. The
/// model tags it `advance` and sends no `arrives`. Pip stays at the
/// market, and the narrator hears it: the Intent names him by his full
/// name. Before, a gated Turn outside a scene got no line, so the story
/// could bring Pip in while the state kept him away.
#[test]
fn a_full_name_on_a_gated_turn_outside_a_scene_gets_the_not_here_line() {
    let w = World::with_detail(DetailLevel::Implied);
    let (w, gm, mut engine) = gated_engine(&on_brann(&w, "advance", &[]));
    play(&mut engine, "I wave Pip in and kiss Brann.").expect("the advance commits");
    let now = snapshot(&engine, &w);
    assert!(now.scene_open.is_none());
    assert!(!now.present().contains(&w.pip));
    let n = narration(&gm);
    assert!(n.contains(PIP_NOT_HERE), "{n}");
}

/// MJ-a: on a gated Turn outside a scene the line reads full names only.
/// "a little time" holds the first word of Little Wren (9): the narrator
/// gets no line for her (review r5, M1).
#[test]
fn a_first_word_gets_no_not_here_line_on_a_gated_turn() {
    let (mut w, _) = wren_world();
    let gm = RecordedGm::new(vec![kiss_brann(&w, &[])], vec!["You kiss.".into()]);
    let mut engine = engine_of(&mut w, &gm);
    play(&mut engine, KISS).expect("the advance commits");
    assert!(snapshot(&engine, &w).scene_open.is_none());
    let n = narration(&gm);
    assert!(!n.contains("Wren"), "{n}");
    assert!(!n.contains("is not here"), "{n}");
}

/// The gate world at Detail Level `implied` where the PC holds the
/// `alias_sparrow` Belief: Pip is also "Sparrow".
fn sparrow_world() -> World {
    let mut w = World::with_detail(DetailLevel::Implied);
    let (ada, pip) = (w.ada, w.pip);
    w.commit(vec![LedgerOp::AddFact {
        slot: threshold_core::domain::Slot {
            subject: EntityRef::Id(pip),
            key: threshold_core::domain::alias_key("Sparrow"),
            value: "Sparrow".into(),
        },
        text: "Pip is also called Sparrow".into(),
        canon: threshold_core::domain::Canon::Established,
        source: threshold_core::domain::FactSource::Proposal,
        seen_by: vec![EntityRef::Id(ada)],
        causes: vec![],
    }]);
    let alias = w.view.live_fact(pip, "alias_sparrow").unwrap().id;
    w.commit(vec![LedgerOp::Know {
        holder: EntityRef::Id(ada),
        fact_id: Some(alias),
        claim_id: None,
        how: Knowing::Witnessed,
        arrives_at: 0,
    }]);
    let ids = [w.ada, w.orsel, w.brann];
    w.confirm(&ids);
    w
}

/// `characters_named_in_full` matches an other name the PC holds (an
/// `alias_` Belief) as a full name, and the "is not here" line of a gated
/// Turn outside a scene reads that match. The line names Pip by his main
/// name.
#[test]
fn an_alias_on_a_gated_turn_gets_the_not_here_line() {
    let mut w = sparrow_world();
    let intent = "I wave Sparrow in and kiss Brann.";
    assert_eq!(
        threshold_core::checks::characters_named_in_full(&w.view, intent),
        vec![w.brann, w.pip]
    );
    let gm = RecordedGm::new(vec![on_brann(&w, "advance", &[])], vec!["You kiss.".into()]);
    let mut engine = engine_of(&mut w, &gm);
    play(&mut engine, intent).expect("the advance commits");
    assert!(!snapshot(&engine, &w).present().contains(&w.pip));
    let n = narration(&gm);
    assert!(n.contains(PIP_NOT_HERE), "{n}");
}

// --- mn-a: a Closing Turn moves nobody, `arrives` included ----------------

/// mn-a: "End scene" whose model sends `arrives: [Pip]`. A Closing Turn
/// moves nobody, so `settle_closing` clears `arrives`. Before, the entry
/// stayed (in an open scene `settle_arrivals` keeps every entry), the
/// references check rejected both attempts, and the core fell back to its
/// default closing: the model's closing was lost. Now the first answer
/// commits: the scene closes, with the model's minutes, and Pip does not
/// arrive.
#[test]
fn a_closing_turn_with_arrives_closes_the_scene_and_brings_nobody() {
    let (w, gm, mut engine) = scene_rig(
        |w| {
            let mut raw: serde_json::Value =
                serde_json::from_str(&on_brann(w, "none", &[w.pip])).unwrap();
            raw["action"] = "wait".into();
            raw["minutes"] = 7.into();
            vec![raw.to_string(), raw.to_string()]
        },
        2,
    );
    let proposals_before = prompts(&gm, CallKind::Propose).len();
    let clock = snapshot(&engine, &w).clock_min;
    let mut ctl = threshold_core::turn::TurnControl {
        on_chunk: &mut |_| {},
        faded: &|| false,
    };
    engine
        .end_scene("close".into(), &mut ctl)
        .expect("End scene commits");
    assert_eq!(prompts(&gm, CallKind::Propose).len(), proposals_before + 1);
    let now = snapshot(&engine, &w);
    assert!(now.scene_open.is_none());
    assert!(!now.present().contains(&w.pip));
    assert_eq!(now.clock_min, clock + 7);
}
