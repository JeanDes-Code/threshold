//! Moves of QA run 4 (2026-11-16), from the shapes of its debug log:
//! - a known place the model called `unknown_place` ("Ilse and I ... walk
//!   together to the Cooper house", "I walk down to the ferry house"): the
//!   PC stayed, with the note "you do not know the place you named";
//! - a place the PC does not know ("I walk to the reeve's house.") that the
//!   model sent to a known place (`move_to` the Cooper house): the PC moved
//!   there, with no note;
//! - a companion named in the Intent ("Ilse and I walk to ...") who agreed
//!   to come along: only the PC moved.

mod support;

use support::*;
use threshold_core::{
    CallKind, Prompt, RecordedGm,
    domain::{
        ActionKind, DecisionProposal, EntityRef, Knowing, LedgerOp, Stance, Verdict, Willing,
    },
    store::{Store, WorldView},
    turn::{Engine, TurnError},
};

fn make_engine(store: Store, campaign: u64, gm: &RecordedGm) -> Engine {
    Engine::new(
        store,
        Box::new(gm.clone()),
        Box::new(threshold_core::rules::D10),
        vec![],
        campaign,
    )
}

/// The gate world with two more places the PC knows: the Cooper house and
/// the ferry house, as QA run 4's PEOPLE AND PLACES listed them. The PC
/// sees Orsel and Brann stand with it.
struct Places {
    w: World,
    cooper: u64,
    ferry: u64,
}

fn places() -> Places {
    let mut w = World::new();
    let ada = w.ada;
    let know: Vec<LedgerOp> = [w.orsel, w.brann]
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
    let cooper = w.add_place("Cooper house");
    let ferry = w.add_place("ferry house");
    Places { w, cooper, ferry }
}

fn engine(mut p: Places, proposals: Vec<String>) -> (Engine, Places, RecordedGm) {
    let gm = RecordedGm::new(proposals, vec!["You go.".into(), "You go again.".into()]);
    let store = std::mem::replace(&mut p.w.store, Store::open_in_memory().unwrap());
    (make_engine(store, p.w.campaign, &gm), p, gm)
}

fn snapshot(engine: &Engine, w: &World) -> WorldView {
    engine.store().snapshot(w.campaign).unwrap()
}

/// The `location` of `who`, in its normal form.
fn location(engine: &Engine, w: &World, who: u64) -> String {
    threshold_core::normalize::value(
        &snapshot(engine, w)
            .live_fact(who, "location")
            .unwrap()
            .slot
            .value,
    )
}

fn prompts(gm: &RecordedGm, kind: CallKind) -> Vec<Prompt> {
    gm.calls()
        .into_iter()
        .filter(|c| c.kind == kind)
        .map(|c| c.prompt)
        .collect()
}

fn calls(gm: &RecordedGm) -> Vec<CallKind> {
    gm.calls().into_iter().map(|c| c.kind).collect()
}

fn a_move(verdict: Verdict) -> DecisionProposal {
    let mut p = proposal(verdict);
    p.action = ActionKind::Move;
    p.minutes = 15;
    p
}

/// A move to `place`, as the model sent it.
fn move_to(place: u64) -> DecisionProposal {
    let mut p = a_move(Verdict::Allowed);
    p.move_to = Some(e(place));
    p
}

/// A move with `move_to: "unknown_place"`, as the model sent it in QA run 4.
fn unknown(p: &DecisionProposal) -> String {
    let mut raw = serde_json::to_value(p).unwrap();
    raw["move_to"] = serde_json::json!(threshold_core::domain::UNKNOWN_PLACE);
    raw.to_string()
}

fn json(p: &DecisionProposal) -> String {
    serde_json::to_string(p).unwrap()
}

fn play(engine: &mut Engine, intent: &str) -> Result<(), TurnError> {
    engine
        .play_turn("t1".into(), intent, &mut |_: &str| {})
        .map(|_| ())
}

fn narration(gm: &RecordedGm) -> String {
    prompts(gm, CallKind::Narrate).pop().unwrap().recent
}

// --- Bug 1: a known place the model called unknown ---------------------------

/// QA run 4, Turn 6: `move_to: "unknown_place"`, verdict allowed, for a
/// place listed under PLACES. The Intent names the place, whatever the case
/// or the article: the first proposal goes back with a note that names the
/// place, and a retry that goes there moves the PC. The core never sets the
/// place itself (review M2, 2026-11-16).
#[test]
fn a_known_place_the_model_called_unknown_is_asked_again() {
    for (intent, to) in [
        (
            "I get dressed and walk to the Cooper house on the lane behind the green, to ask Marta Cooper for dry kindling.",
            "cooper house",
        ),
        ("I walk down to the ferry house.", "ferry house"),
        ("I walk down to ferry house.", "ferry house"),
        ("I walk down to The Ferry House.", "ferry house"),
        ("i walk to the cooper house", "cooper house"),
    ] {
        let p0 = places();
        let target = if to == "ferry house" {
            p0.ferry
        } else {
            p0.cooper
        };
        let (mut engine, p, gm) = engine(
            p0,
            vec![unknown(&a_move(Verdict::Allowed)), json(&move_to(target))],
        );
        play(&mut engine, intent).unwrap();
        assert_eq!(location(&engine, &p.w, p.w.ada), to, "{intent}");
        assert_eq!(
            calls(&gm),
            [CallKind::Propose, CallKind::Propose, CallKind::Narrate],
            "{intent}"
        );
        let retry = &prompts(&gm, CallKind::Propose)[1];
        assert!(retry.recent.contains(&format!("E{target}")), "{intent}");
        assert!(
            !narration(&gm).contains(threshold_core::context::UNKNOWN_PLACE_LINE),
            "{intent}"
        );
        let stored = engine.store().turns(p.w.campaign).unwrap();
        let last = stored.last().unwrap();
        assert!(
            !threshold_core::turn::stored_unknown_place(&last.record.proposal_json),
            "{intent}"
        );
    }
}

/// QA run 4, Turn 8: `unknown_place` with the verdict impossible and no
/// blocker, for the ferry house. The verdict was judged for a place the
/// model thought unknown, so the first attempt goes back with a note that
/// names the place; the retry moves the PC.
#[test]
fn an_impossible_move_to_a_known_place_called_unknown_is_asked_again() {
    let p0 = places();
    let ferry = p0.ferry;
    let (mut engine, p, gm) = engine(
        p0,
        vec![unknown(&a_move(Verdict::Impossible)), json(&move_to(ferry))],
    );
    play(&mut engine, "I walk down to the ferry house.").unwrap();
    assert_eq!(location(&engine, &p.w, p.w.ada), "ferry house");
    assert_eq!(
        calls(&gm),
        [CallKind::Propose, CallKind::Propose, CallKind::Narrate]
    );
    let retry = &prompts(&gm, CallKind::Propose)[1];
    assert!(
        retry.recent.contains(&format!("E{ferry}")),
        "{}",
        retry.recent
    );
    assert!(retry.recent.contains("ferry house"), "{}", retry.recent);
}

/// The stored proposal of the last Turn.
fn stored_proposal(engine: &Engine, w: &World) -> serde_json::Value {
    let stored = engine.store().turns(w.campaign).unwrap();
    let record: serde_json::Value =
        serde_json::from_str(&stored.last().unwrap().record.proposal_json).unwrap();
    record["proposal"].clone()
}

/// The narrator's line for a known place the PC does not reach.
fn not_reached(place: &str) -> String {
    format!(
        "{}{place}{}",
        threshold_core::context::NOT_REACHED_LINE,
        threshold_core::context::NOT_REACHED_END
    )
}

/// Final review (2026-11-16): a retry that still sends another known place
/// stays put. The ferry house is known, so the PC does not reach it:
/// never "you do not know the place you named". The verdict and blockers
/// were judged for the other place, so they are not kept.
#[test]
fn a_retry_that_still_goes_elsewhere_does_not_reach_the_known_place() {
    let p0 = places();
    let cooper = p0.cooper;
    let impossible_there = {
        let mut p = move_to(cooper);
        p.verdict = Verdict::Impossible;
        p
    };
    for (first, retry) in [
        (json(&move_to(cooper)), json(&move_to(cooper))),
        (json(&impossible_there), json(&impossible_there)),
        (
            unknown(&a_move(Verdict::Impossible)),
            json(&move_to(cooper)),
        ),
    ] {
        let (mut engine, p, gm) = engine(places(), vec![first, retry.clone()]);
        let before = location(&engine, &p.w, p.w.ada);
        play(&mut engine, "I walk down to the ferry house.").unwrap();
        assert_eq!(location(&engine, &p.w, p.w.ada), before, "{retry}");
        let told = narration(&gm);
        assert!(
            told.contains(&not_reached("ferry house")),
            "{retry}: {told}"
        );
        assert!(
            !told.contains(threshold_core::context::UNKNOWN_PLACE_LINE),
            "{retry}"
        );
        assert!(
            !told.contains(threshold_core::context::IMPOSSIBLE_LINE),
            "{retry}"
        );
        let stored = stored_proposal(&engine, &p.w);
        assert_eq!(stored["verdict"], "allowed", "{retry}");
        assert_eq!(stored["not_reached"], format!("E{}", p.ferry), "{retry}");
        assert!(stored.get("unknown_place").is_none(), "{retry}");
    }
}

/// Final review (2026-11-16): a retry that calls the named place
/// `unknown_place` with `impossible` and a blocker judged the Intent's own
/// words. The PC stays, the verdict and the blocker are kept for the
/// narrator, and the place is still told as known.
#[test]
fn a_retry_that_calls_the_named_place_unknown_keeps_its_obstacle() {
    let p0 = places();
    let gate_locked = p0.w.gate_locked;
    let blocked = {
        let mut p = a_move(Verdict::Impossible);
        p.blockers = vec![f(gate_locked)];
        p
    };
    let (mut engine, p, gm) = engine(places(), vec![unknown(&blocked), unknown(&blocked)]);
    let before = location(&engine, &p.w, p.w.ada);
    play(&mut engine, "I walk down to the ferry house.").unwrap();
    assert_eq!(location(&engine, &p.w, p.w.ada), before);
    let told = narration(&gm);
    assert!(
        told.contains(threshold_core::context::IMPOSSIBLE_LINE),
        "{told}"
    );
    assert!(told.contains(&not_reached("ferry house")), "{told}");
    assert!(
        !told.contains(threshold_core::context::UNKNOWN_PLACE_LINE),
        "{told}"
    );
    let stored = stored_proposal(&engine, &p.w);
    assert_eq!(stored["verdict"], "impossible");
    assert_eq!(
        stored["blockers"],
        serde_json::json!([format!("F{gate_locked}")])
    );
    assert!(stored.get("unknown_place").is_none());
}

/// Review M2, case (a), from the QA view: "I walk to the Copper Kettle."
/// came with `move_to` another known place, verdict allowed. That verdict
/// was judged for the other place (a storm or a high river on the way to
/// the named one is skipped), so the first proposal goes back; the PC
/// moves only on a retry that judged the named place.
#[test]
fn a_move_to_another_known_place_is_asked_again_even_when_allowed() {
    let p0 = places();
    let (cooper, ferry) = (p0.cooper, p0.ferry);
    let (mut engine, p, gm) = engine(p0, vec![json(&move_to(cooper)), json(&move_to(ferry))]);
    play(&mut engine, "I walk down to the ferry house.").unwrap();
    assert_eq!(location(&engine, &p.w, p.w.ada), "ferry house");
    assert_eq!(
        calls(&gm),
        [CallKind::Propose, CallKind::Propose, CallKind::Narrate]
    );
    let retry = &prompts(&gm, CallKind::Propose)[1];
    assert!(
        retry.recent.contains(&format!("E{ferry}")),
        "{}",
        retry.recent
    );
}

/// Review M2, case (b): "to the window" is a place phrase that names no
/// known place; the Cooper house named after it is no destination. The PC
/// stays, whatever the model sent, and no retry is asked.
#[test]
fn a_place_named_outside_the_place_phrase_is_no_destination() {
    let p0 = places();
    let cooper = p0.cooper;
    for first in [unknown(&a_move(Verdict::Allowed)), json(&move_to(cooper))] {
        let (mut engine, p, gm) = engine(places(), vec![first.clone()]);
        let before = location(&engine, &p.w, p.w.ada);
        play(
            &mut engine,
            "I walk to the window and look out at the Cooper house.",
        )
        .unwrap();
        assert_eq!(location(&engine, &p.w, p.w.ada), before, "{first}");
        assert_eq!(
            calls(&gm),
            [CallKind::Propose, CallKind::Narrate],
            "{first}"
        );
    }
}

/// Review of the eval counts (2026-11-16): a place the Intent names is not
/// always where the PC goes. The place in the "to ..." phrase wins over a
/// place named on the way, and a phrase to the PC's own place makes no
/// move.
#[test]
fn the_place_after_to_is_where_the_pc_goes() {
    let p0 = places();
    let (cooper, ferry) = (p0.cooper, p0.ferry);
    let (mut past, p, gm) = engine(p0, vec![json(&move_to(ferry)), json(&move_to(cooper))]);
    play(
        &mut past,
        "I walk to the Cooper house, past the ferry house.",
    )
    .unwrap();
    assert_eq!(location(&past, &p.w, p.w.ada), "cooper house");
    let retry = &prompts(&gm, CallKind::Propose)[1];
    assert!(
        retry.recent.contains(&format!("E{cooper}")),
        "{}",
        retry.recent
    );

    let p0 = places();
    let cooper = p0.cooper;
    let (mut back, p, _gm) = engine(p0, vec![json(&move_to(cooper))]);
    play(
        &mut back,
        "I turn away from the Cooper house and walk back to the gate square.",
    )
    .unwrap();
    assert_eq!(location(&back, &p.w, p.w.ada), "gate square");
}

/// The model kept the PC where it is (`move_to: null`, not unknown): the
/// core does not invent a move from a place name in the Intent ("I watch
/// from the door until Tobin reaches the Cooper house" names a place, and
/// is no move).
#[test]
fn a_move_the_model_kept_in_place_is_not_made_from_a_name() {
    let (mut engine, p, _gm) = engine(places(), vec![json(&a_move(Verdict::Allowed))]);
    let before = location(&engine, &p.w, p.w.ada);
    play(
        &mut engine,
        "I watch from the door to make sure Pip reaches the Cooper house.",
    )
    .unwrap();
    assert_eq!(location(&engine, &p.w, p.w.ada), before);
}

// --- Bug 2: a place the PC does not know -------------------------------------

/// QA run 4, Turn 16 (and QA run 3): "I walk to the reeve's house." There
/// is no reeve's house; the model sent the Cooper house. The PC stays, and
/// the narrator and the Player are told why.
#[test]
fn a_place_the_pc_does_not_know_never_lands_in_a_known_place() {
    let p0 = places();
    let cooper = p0.cooper;
    let (mut engine, p, gm) = engine(p0, vec![json(&move_to(cooper))]);
    let before = location(&engine, &p.w, p.w.ada);
    play(&mut engine, "I walk to the reeve's house.").unwrap();
    assert_eq!(location(&engine, &p.w, p.w.ada), before);
    assert!(narration(&gm).contains(threshold_core::context::UNKNOWN_PLACE_LINE));
    let stored = engine.store().turns(p.w.campaign).unwrap();
    assert!(threshold_core::turn::stored_unknown_place(
        &stored.last().unwrap().record.proposal_json
    ));
}

/// The same, when the first proposal was rejected for another reason and
/// the retry picks a known place the Intent does not name (QA run 3's
/// path: a rejection, then a retry to the Copper Kettle).
#[test]
fn a_retry_is_never_sent_to_a_known_place_the_intent_does_not_name() {
    let p0 = places();
    let cooper = p0.cooper;
    // A coincidence trap: the locked gate opens with no cause.
    let mut trap = move_to(cooper);
    trap.facts = vec![fact(
        slot(e(p0.w.gate), "locked", "no"),
        threshold_core::domain::Seen::Pc,
        vec![],
    )];
    let (mut engine, p, gm) = engine(p0, vec![json(&trap), json(&move_to(cooper))]);
    let before = location(&engine, &p.w, p.w.ada);
    play(&mut engine, "I walk to the reeve's house and knock.").unwrap();
    assert_eq!(
        calls(&gm),
        [CallKind::Propose, CallKind::Propose, CallKind::Narrate]
    );
    assert_eq!(location(&engine, &p.w, p.w.ada), before);
    assert!(narration(&gm).contains(threshold_core::context::UNKNOWN_PLACE_LINE));
}

/// A short name of a known place still moves the PC: "the inn" for the
/// Lantern Inn. An Intent with no place phrase ("I go home") keeps the
/// model's place.
#[test]
fn a_short_name_or_no_place_phrase_keeps_the_model_place() {
    for intent in [
        "I head back to the inn.",
        "I go home.",
        "I walk back to my inn.",
    ] {
        let mut p0 = places();
        let inn = p0.w.add_place("Lantern Inn");
        let (mut engine, p, _gm) = engine(p0, vec![json(&move_to(inn))]);
        play(&mut engine, intent).unwrap();
        assert_eq!(location(&engine, &p.w, p.w.ada), "lantern inn", "{intent}");
    }
}

/// A place phrase that names a character the PC knows is no unknown
/// place: "I walk over to Brann" keeps the model's place.
#[test]
fn a_phrase_that_names_a_character_keeps_the_model_place() {
    let p0 = places();
    let cooper = p0.cooper;
    let (mut engine, p, _gm) = engine(p0, vec![json(&move_to(cooper))]);
    play(&mut engine, "I run over to Pip.").unwrap();
    assert_eq!(location(&engine, &p.w, p.w.ada), "cooper house");
}

// --- Bug 3: companions -------------------------------------------------------

fn willing(npc: u64, yes: bool) -> threshold_core::domain::Reaction {
    let mut r = reaction(e(npc), Stance::Comply, None);
    r.willing = Some(if yes { Willing::Yes } else { Willing::No });
    r
}

/// QA run 4, Turns 15 and 18: "Ilse and I walk to the ferry house" and "I
/// take Ilse by the hand and we walk together to the ferry house", with
/// Ilse's reaction `comply`, willing `yes`. She comes along: her
/// `location` is the PC's new place, and the narrator lists her as here.
#[test]
fn a_willing_companion_named_in_the_intent_moves_with_the_pc() {
    for intent in [
        "Brann and I walk to the ferry house to ask about the crossing.",
        "I take Brann by the hand and we walk together to the ferry house.",
        "I walk to the ferry house with Brann.",
    ] {
        let p0 = places();
        let mut p = move_to(p0.ferry);
        p.reactions = vec![willing(p0.w.brann, true)];
        let (mut engine, pl, gm) = engine(p0, vec![json(&p)]);
        play(&mut engine, intent).unwrap();
        assert_eq!(
            location(&engine, &pl.w, pl.w.ada),
            "ferry house",
            "{intent}"
        );
        assert_eq!(
            location(&engine, &pl.w, pl.w.brann),
            "ferry house",
            "{intent}"
        );
        // Orsel, not named, stays at the gate square.
        assert_eq!(
            location(&engine, &pl.w, pl.w.orsel),
            "gate square",
            "{intent}"
        );
        let n = narration(&gm);
        assert!(
            n.contains(&format!(
                "{}Brann.",
                threshold_core::context::HERE_AFTER_LINE
            )),
            "{intent}: {n}"
        );
        let view = snapshot(&engine, &pl.w);
        assert!(view.present().contains(&pl.w.brann), "{intent}");
    }
}

/// A companion who does not agree, or who has no reaction (QA run 4, Turn
/// 29), stays; the narrator is told so, so the story does not walk them
/// along.
#[test]
fn a_named_companion_who_does_not_agree_stays_and_the_narrator_knows() {
    for reactions in [vec![false], vec![]] {
        let p0 = places();
        let mut p = move_to(p0.ferry);
        p.reactions = reactions.iter().map(|y| willing(p0.w.brann, *y)).collect();
        let (mut engine, pl, gm) = engine(p0, vec![json(&p)]);
        play(&mut engine, "Brann and I walk to the ferry house.").unwrap();
        assert_eq!(location(&engine, &pl.w, pl.w.ada), "ferry house");
        assert_eq!(location(&engine, &pl.w, pl.w.brann), "gate square");
        let n = narration(&gm);
        assert!(
            n.contains("- Brann does not come along: Brann stays behind."),
            "{n}"
        );
    }
}

/// An NPC the Intent names, but not as coming along, stays even when the
/// reaction agrees: "I tell Brann goodbye and walk to the ferry house".
#[test]
fn a_named_npc_who_is_not_taken_along_stays() {
    let p0 = places();
    let mut p = move_to(p0.ferry);
    p.reactions = vec![willing(p0.w.brann, true)];
    let (mut engine, pl, _gm) = engine(p0, vec![json(&p)]);
    play(
        &mut engine,
        "I tell Brann goodbye and walk to the ferry house.",
    )
    .unwrap();
    assert_eq!(location(&engine, &pl.w, pl.w.ada), "ferry house");
    assert_eq!(location(&engine, &pl.w, pl.w.brann), "gate square");
}

/// A move that does not happen moves no companion.
#[test]
fn an_impossible_move_takes_no_companion() {
    let p0 = places();
    let mut p = move_to(p0.ferry);
    p.verdict = Verdict::Impossible;
    p.reactions = vec![willing(p0.w.brann, true)];
    let (mut engine, pl, gm) = engine(p0, vec![json(&p)]);
    play(&mut engine, "Brann and I walk to the ferry house.").unwrap();
    assert_eq!(location(&engine, &pl.w, pl.w.ada), "gate square");
    assert_eq!(location(&engine, &pl.w, pl.w.brann), "gate square");
    assert!(!narration(&gm).contains("does not come along"));
}
