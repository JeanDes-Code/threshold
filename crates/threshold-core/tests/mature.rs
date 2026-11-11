//! The adults-only gate (mature content module, Tasks 1 and 2): the Detail
//! Level, the age rule over stored and confirmed ages, the namesake rule,
//! the stance rule, the final refusal, who is at the PC's place after the
//! ops of a Turn, the reserved `age` key and the telling of an intimate
//! Turn. Neutral fixtures only.

mod support;

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use support::*;
use threshold_core::{
    CallKind, RecordedGm,
    checks::{CheckKind, Rejection, to_ops, validate},
    context::seen_present,
    domain::{
        ActionKind, Canon, DecisionProposal, DetailLevel, Effect, EntityKind, EntityRef, FactDraft,
        FactSource, Handle, LedgerOp, SeedEntity, Seen, Slot, Stance, Trigger, Verdict,
    },
    mature::{Blocked, RefusalKind, Told, after, clearance, stored_intimate, told},
    normalize,
    pack::ContentPack,
    rules::D10,
    store::{Store, WorldView},
    turn::{Engine, TurnError},
    view::{REFUSAL_ADULTS_ONLY, refusal_text},
    world::{advance, with_arrival, with_witnessed_events, written_slots},
};

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn test_pack_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/packs/test-pack")
}

/// The gate scene at `implied`, with Ada, Orsel and Brann confirmed.
fn confirmed_gate() -> World {
    let mut w = World::with_detail(DetailLevel::Implied);
    let ids = [w.ada, w.orsel, w.brann];
    w.confirm(&ids);
    w
}

/// An intimate proposal: Brann reacts with `stance`.
fn intimate(w: &World, stance: Stance) -> DecisionProposal {
    let mut p = proposal(Verdict::Allowed);
    p.intimate = true;
    p.reactions = vec![reaction(e(w.brann), stance, None)];
    p
}

/// Adds a character with a raw op, at the place whose location value is
/// `place`, as a seed with a confirmed age would add it.
fn add_character(w: &mut World, name: &str, age: u16, confirmed: bool, place: &str) -> u64 {
    w.commit(vec![
        LedgerOp::AddEntity {
            local: Some(1),
            kind: EntityKind::Npc,
            name: name.into(),
            age: Some(age),
            other_names: vec![],
            age_confirmed: confirmed,
        },
        LedgerOp::AddFact {
            slot: Slot {
                subject: EntityRef::New(1),
                key: "location".into(),
                value: place.into(),
            },
            text: format!("{name} stands at {place}."),
            canon: Canon::Established,
            source: FactSource::Proposal,
            seen_by: vec![],
            causes: vec![],
        },
    ]);
    w.view
        .entities
        .iter()
        .filter(|e| e.name == name)
        .map(|e| e.id)
        .max()
        .unwrap()
}

/// Moves `id` to the place whose location value is `place`.
fn bring(w: &mut World, id: u64, place: &str) {
    let old = w.view.live_fact(id, "location").unwrap().id;
    w.commit(vec![LedgerOp::SupersedeFact {
        old,
        new: FactDraft {
            slot: Slot {
                subject: id.into(),
                key: "location".into(),
                value: place.into(),
            },
            text: format!("They came to {place}."),
            canon: Canon::Established,
            source: FactSource::Correction,
            seen_by: vec![],
            causes: vec![],
        },
    }]);
}

fn kinds(errs: &[Rejection]) -> Vec<CheckKind> {
    errs.iter().map(|r| r.check).collect()
}

fn adults_only(errs: &[Rejection]) -> Vec<Option<Handle>> {
    errs.iter()
        .filter(|r| r.check == CheckKind::AdultsOnly)
        .map(|r| r.handle)
        .collect()
}

/// A view where `id` is a confirmed character of age `age`.
fn with_age(view: &WorldView, id: u64, age: Option<u16>) -> WorldView {
    let mut view = view.clone();
    let entity = view.entities.iter_mut().find(|e| e.id == id).unwrap();
    entity.age = age;
    entity.age_confirmed = true;
    view
}

#[test]
fn an_intimate_proposal_is_refused_at_excluded() {
    let mut w = World::new();
    let ids = [w.ada, w.orsel, w.brann];
    w.confirm(&ids);
    assert_eq!(w.view.detail, DetailLevel::Excluded);

    let errs = validate(&intimate(&w, Stance::Comply), &w.view).expect_err("excluded");
    assert_eq!(kinds(&errs), [CheckKind::DetailExcluded]);
    assert!(errs[0].check.is_final());
    // A plain proposal at the same level passes.
    let mut p = intimate(&w, Stance::Comply);
    p.intimate = false;
    assert!(validate(&p, &w.view).is_ok());
}

#[test]
fn a_minor_in_the_place_refuses() {
    let mut w = confirmed_gate();
    // A confirmed age of 17 is still under the floor.
    let wren = add_character(&mut w, "Wren", 17, true, "gate square");
    let errs = validate(&intimate(&w, Stance::Comply), &w.view).expect_err("a minor");
    assert_eq!(adults_only(&errs), [Some(e(wren))]);
    assert!(CheckKind::AdultsOnly.is_final());

    // Pip, the seed's minor, blocks too once Pip stands here.
    let mut w = confirmed_gate();
    let pip = w.pip;
    bring(&mut w, pip, "Gate Square");
    let errs = validate(&intimate(&w, Stance::Comply), &w.view).expect_err("Pip");
    assert_eq!(adults_only(&errs), [Some(e(w.pip))]);
}

#[test]
fn an_unconfirmed_adult_refuses() {
    let mut w = World::with_detail(DetailLevel::Implied);
    let ids = [w.ada, w.orsel];
    w.confirm(&ids);
    let errs = validate(&intimate(&w, Stance::Comply), &w.view).expect_err("Brann");
    assert_eq!(adults_only(&errs), [Some(e(w.brann))]);
    assert_eq!(
        clearance(&w.view, &[]).unwrap_err(),
        Blocked { who: vec![w.brann] }
    );
}

#[test]
fn a_hidden_minor_refuses_and_names_nobody() {
    let mut seed = adventure_seed();
    seed.detail = DetailLevel::Implied;
    seed.entities.push(SeedEntity {
        name: "Moth".into(),
        kind: EntityKind::Npc,
        age: Some(16),
        other_names: vec![],
        hidden: true,
    });
    seed.decided
        .push(seed_fact("Moth", "location", "gate square"));
    let mut w = World::with_seed(seed);
    let ids = [w.ada, w.orsel, w.brann];
    w.confirm(&ids);
    let moth = w.view.entity_by_name("Moth").unwrap().id;
    assert!(w.view.present().contains(&moth));

    let gm = RecordedGm::new(
        vec![serde_json::to_string(&intimate(&w, Stance::Comply)).unwrap()],
        vec![],
    );
    let store = std::mem::replace(&mut w.store, Store::open_in_memory().unwrap());
    let mut engine = Engine::new(
        store,
        Box::new(gm.clone()),
        Box::new(D10),
        vec![],
        w.campaign,
    );
    let err = engine
        .play_turn("t1".into(), "I lean closer.", &mut |_| {})
        .expect_err("refused");
    let TurnError::Refused { kind, blocked } = err else {
        panic!("expected a refusal, got {err:?}");
    };
    assert_eq!(kind, RefusalKind::AdultsOnly);
    assert_eq!(blocked, [moth]);
    let text = refusal_text(kind);
    assert_eq!(text, REFUSAL_ADULTS_ONLY);
    assert!(!text.contains("Moth"));
    assert!(!text.chars().any(|c| c.is_ascii_digit()));
}

#[test]
fn a_new_npc_in_an_intimate_turn_refuses() {
    let w = confirmed_gate();
    let mut p = intimate(&w, Stance::Comply);
    p.new_npcs = vec![new_npc(1, "Kett", 30)];
    let errs = validate(&p, &w.view).expect_err("a new NPC");
    assert_eq!(adults_only(&errs), [Some(Handle::NewNpc(1))]);
}

#[test]
fn an_npc_made_in_play_blocks_until_confirmed() {
    let mut w = confirmed_gate();
    let mut p = proposal(Verdict::Allowed);
    p.new_npcs = vec![new_npc(1, "Kett", 34)];
    p.reactions = vec![reaction(Handle::NewNpc(1), Stance::Comply, None)];
    let valid = validate(&p, &w.view).unwrap();
    let ops = to_ops(valid, None, &w.view).ops;
    w.commit(ops);
    let kett = w.view.entity_by_name("Kett").unwrap();
    assert!(!kett.age_confirmed, "a model-stated age is unconfirmed");
    let kett = kett.id;
    assert!(w.view.present().contains(&kett));

    let errs = validate(&intimate(&w, Stance::Comply), &w.view).expect_err("Kett");
    assert_eq!(adults_only(&errs), [Some(e(kett))]);

    w.confirm(&[kett]);
    assert!(validate(&intimate(&w, Stance::Comply), &w.view).is_ok());
}

#[test]
fn a_namesake_of_a_minor_is_not_cleared() {
    // A confirmed adult who shares the name of the seed's minor, Pip.
    let mut w = confirmed_gate();
    let adult_pip = add_character(&mut w, "Pip", 30, true, "gate square");
    assert_ne!(adult_pip, w.pip);
    assert_eq!(
        clearance(&w.view, &[]).unwrap_err(),
        Blocked {
            who: vec![adult_pip]
        }
    );
    let errs = validate(&intimate(&w, Stance::Comply), &w.view).expect_err("namesake");
    assert_eq!(adults_only(&errs), [Some(e(adult_pip))]);

    // An accented twin: "Léa" is a confirmed adult, "lea" a minor elsewhere.
    let mut w = confirmed_gate();
    let lea = add_character(&mut w, "Léa", 25, true, "gate square");
    add_character(&mut w, "lea", 16, true, "market");
    assert_eq!(
        clearance(&w.view, &[]).unwrap_err(),
        Blocked { who: vec![lea] }
    );

    // The same through another name.
    let mut w = confirmed_gate();
    w.commit(vec![LedgerOp::AddEntity {
        local: Some(1),
        kind: EntityKind::Npc,
        name: "Marla".into(),
        age: Some(40),
        other_names: vec!["PIP".into()],
        age_confirmed: true,
    }]);
    let marla = w.view.entity_by_name("Marla").unwrap().id;
    assert_eq!(
        clearance(&w.view, &[marla]).unwrap_err(),
        Blocked { who: vec![marla] }
    );
}

#[test]
fn the_age_floor_is_eighteen_exactly() {
    let w = confirmed_gate();
    let at = |age| clearance(&with_age(&w.view, w.brann, age), &[]);
    assert_eq!(at(Some(17)).unwrap_err(), Blocked { who: vec![w.brann] });
    assert!(at(Some(18)).is_ok());
    assert!(at(Some(u16::MAX)).is_ok());
    assert_eq!(at(None).unwrap_err(), Blocked { who: vec![w.brann] });
}

#[test]
fn clearing_min_age_changes_no_clearance() {
    let w = confirmed_gate();
    for age in [17, 18] {
        let base = with_age(&w.view, w.brann, Some(age));
        let results: Vec<bool> = [None, Some(16), Some(21)]
            .into_iter()
            .map(|min_age| {
                let mut view = base.clone();
                view.adventure_seed.as_mut().unwrap().min_age = min_age;
                clearance(&view, &[]).is_ok()
            })
            .collect();
        assert_eq!(results, [age >= 18; 3], "age {age}");
    }
}

#[test]
fn detail_explicit_clears_nobody() {
    let mut w = World::with_detail(DetailLevel::Explicit);
    let ids = [w.ada, w.orsel];
    w.confirm(&ids);
    let errs = validate(&intimate(&w, Stance::Comply), &w.view).expect_err("Brann");
    assert_eq!(adults_only(&errs), [Some(e(w.brann))]);
}

#[test]
fn an_intimate_turn_with_a_resisting_npc_is_rejected() {
    let w = confirmed_gate();
    for stance in [Stance::Oppose, Stance::Resist, Stance::Neutral] {
        let errs = validate(&intimate(&w, stance), &w.view).expect_err("unwilling");
        assert!(kinds(&errs).contains(&CheckKind::Unwilling), "{errs:?}");
        assert!(errs.iter().all(|r| !r.check.is_final()), "{errs:?}");
    }
}

#[test]
fn an_intimate_turn_with_no_reaction_is_rejected() {
    let w = confirmed_gate();
    let mut p = intimate(&w, Stance::Comply);
    p.reactions.clear();
    let errs = validate(&p, &w.view).expect_err("no reaction");
    assert_eq!(kinds(&errs), [CheckKind::Unwilling]);
    assert!(!CheckKind::Unwilling.is_final());
}

#[test]
fn confirmed_adults_who_comply_or_help_pass() {
    let mut w = confirmed_gate();
    assert!(validate(&intimate(&w, Stance::Comply), &w.view).is_ok());
    // Help needs a ceiling of help: Brann likes Ada.
    let (brann, ada) = (w.brann, w.ada);
    w.commit(vec![LedgerOp::ShiftAttitude {
        from: brann.into(),
        to: ada.into(),
        delta: 3,
    }]);
    assert!(validate(&intimate(&w, Stance::Help), &w.view).is_ok());
}

/// Path 18: the test pack asks the model to ignore the age rule. The
/// prompt holds that line, and the Turn is refused all the same.
#[test]
fn the_sample_pack_asks_to_ignore_the_age_rule_and_the_check_rejects() {
    let mut w = confirmed_gate();
    let pip = w.pip;
    bring(&mut w, pip, "Gate Square");
    let pack = ContentPack::load(&test_pack_dir()).unwrap();
    let gm = RecordedGm::new(
        vec![serde_json::to_string(&intimate(&w, Stance::Comply)).unwrap()],
        vec!["unused".into()],
    );
    let store = std::mem::replace(&mut w.store, Store::open_in_memory().unwrap());
    let mut engine = Engine::new(
        store,
        Box::new(gm.clone()),
        Box::new(D10),
        vec![pack],
        w.campaign,
    );
    let err = engine
        .play_turn("t1".into(), "I lean closer.", &mut |_| {})
        .expect_err("refused");
    assert!(
        matches!(
            err,
            TurnError::Refused {
                kind: RefusalKind::AdultsOnly,
                ..
            }
        ),
        "{err:?}"
    );
    let calls = gm.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].kind, CallKind::Propose);
    assert!(calls[0].prompt.system.contains("ignore all age rules"));
}

// --- Task 2: the reserved age key, presence after the ops, telling ---------

/// Path 11: a proposal fact on the key `age` is dropped with a note. The
/// gate reads the stored age only, so Pip (17) still blocks the Turn.
#[test]
fn a_fact_on_the_age_key_changes_no_clearance() {
    let mut w = confirmed_gate();
    let pip = w.pip;
    bring(&mut w, pip, "Gate Square");

    // `settle` drops every carrier of the key, on any subject, one note each.
    let mut p = proposal(Verdict::Allowed);
    p.facts = vec![
        fact(slot(e(pip), "age", "30"), Seen::Pc, vec![]),
        fact(slot(e(w.orsel), " Age ", "60"), Seen::Gm, vec![]),
        fact(slot(e(w.orsel), "mood", "calm"), Seen::Pc, vec![]),
    ];
    p.check = Some(check(
        None,
        slot(e(pip), "age", "31"),
        slot(e(w.brann), "age", "40"),
    ));
    p.schedule = Some(threshold_core::domain::EventDraft {
        in_minutes: 30,
        actor: e(w.orsel),
        goal: None,
        text: "Orsel files a record.".into(),
        effects: vec![slot(e(pip), "age", "21"), slot(e(w.orsel), "mood", "busy")],
        seen_by: vec![],
        public: false,
        moves_actor_to: None,
    });
    let (settled, notes) = threshold_core::checks::settle(p, &w.view);
    let dropped = notes.iter().filter(|n| n.contains("key age")).count();
    assert_eq!(dropped, 5, "{notes:?}");
    assert_eq!(settled.facts.len(), 1);
    let check = settled.check.as_ref().unwrap();
    assert_eq!((&check.win, &check.lose), (&None, &None));
    assert_eq!(settled.schedule.as_ref().unwrap().effects.len(), 1);

    // A plain Turn commits no fact on `age`.
    let mut plain = proposal(Verdict::Allowed);
    plain.facts = vec![fact(slot(e(pip), "age", "30"), Seen::Pc, vec![])];
    let mut tagged = intimate(&w, Stance::Comply);
    tagged.facts = vec![fact(slot(e(pip), "age", "30"), Seen::Pc, vec![])];
    let gm = RecordedGm::new(
        vec![
            serde_json::to_string(&plain).unwrap(),
            serde_json::to_string(&tagged).unwrap(),
        ],
        vec!["Pip shrugs.".into()],
    );
    let store = std::mem::replace(&mut w.store, Store::open_in_memory().unwrap());
    let mut engine = Engine::new(
        store,
        Box::new(gm.clone()),
        Box::new(D10),
        vec![],
        w.campaign,
    );
    engine
        .play_turn("t1".into(), "I ask Pip a question.", &mut |_| {})
        .unwrap();
    let view = engine.store().snapshot(w.campaign).unwrap();
    assert!(
        view.facts.iter().all(|f| f.slot.key != "age"),
        "{:?}",
        view.facts
    );

    // An intimate Turn that says Pip is 30 is still refused for Pip.
    let err = engine
        .play_turn("t2".into(), "I lean closer.", &mut |_| {})
        .expect_err("refused");
    let TurnError::Refused { kind, blocked } = err else {
        panic!("expected a refusal, got {err:?}");
    };
    assert_eq!((kind, blocked), (RefusalKind::AdultsOnly, vec![pip]));
    let view = engine.store().snapshot(w.campaign).unwrap();
    let pip_row = view.entities.iter().find(|e| e.id == pip).unwrap();
    assert_eq!((pip_row.age, pip_row.age_confirmed), (Some(17), false));
}

/// The ops one Turn commits for `p`, built as `turn::resolve` builds them:
/// `to_ops`, the World Clock and its due World Events, perception on
/// arrival and of the witnessed events.
fn turn_ops(view: &WorldView, p: &DecisionProposal) -> Vec<LedgerOp> {
    let valid = validate(p, view).unwrap();
    let mut ops = to_ops(valid, None, view).ops;
    let written = written_slots(&ops);
    ops.extend(
        advance(view, p.minutes, &written)
            .into_iter()
            .filter(|op| !matches!(op, LedgerOp::AdvanceClock { .. })),
    );
    let ops = with_arrival(view, ops, view.clock_min + u64::from(p.minutes));
    with_witnessed_events(view, ops).0
}

/// Plays `p` as one committed Turn and checks `mature::after` against the
/// snapshot after the Commit: world truth, what the PC knows is here, and
/// the PC's place. Returns the presence it computed.
fn step(w: &mut World, p: &DecisionProposal) -> threshold_core::mature::Presence {
    let ops = turn_ops(&w.view, p);
    let presence = after(&w.view, &ops);
    w.commit(ops);
    let truth: BTreeSet<u64> = w.view.present().into_iter().collect();
    let seen: BTreeSet<u64> = seen_present(&w.view).into_iter().collect();
    let place = w
        .view
        .live_fact(w.ada, "location")
        .map(|f| normalize::value(&f.slot.value));
    assert_eq!(presence.truth, truth, "world truth after the Commit");
    assert_eq!(presence.seen, seen, "what the PC knows is here");
    assert_eq!(presence.pc_place, place, "the PC's place");
    presence
}

/// A World Event due now that sets `subject`'s location to `place`. A
/// `witnessed` event is public and has `subject` among its witnesses, so a
/// PC who stands with `subject` sees it happen.
fn schedule_move(w: &mut World, subject: u64, place: &str, witnessed: bool) {
    let at = w.view.clock_min;
    w.commit(vec![LedgerOp::ScheduleEvent {
        text: format!("Someone goes to {place}."),
        trigger: Trigger::At(at),
        actors: vec![EntityRef::Id(subject)],
        goal_id: None,
        cause_fact_id: None,
        effects: vec![Effect::SetFact {
            slot: Slot {
                subject: EntityRef::Id(subject),
                key: "location".into(),
                value: place.into(),
            },
            text: format!("They are at {place}."),
            seen_by: vec![],
        }],
        seen_by: if witnessed {
            vec![EntityRef::Id(subject)]
        } else {
            vec![]
        },
        public: witnessed,
    }]);
}

/// Oracle: over a move, an arrival event, a departure event, a new NPC and
/// a hidden arrival, `after` gives the presence the next snapshot holds.
#[test]
fn presence_after_matches_the_next_snapshot() {
    let mut seed = adventure_seed();
    seed.entities.push(SeedEntity {
        name: "Moth".into(),
        kind: EntityKind::Npc,
        age: Some(16),
        other_names: vec![],
        hidden: true,
    });
    seed.decided.push(seed_fact("Moth", "location", "far road"));
    let mut w = World::with_seed(seed);
    let moth = w.view.entity_by_name("Moth").unwrap().id;
    let market = w.add_place("Market");
    let mut wait = proposal(Verdict::Allowed);
    wait.minutes = 5;

    // A plain Turn changes nobody's place.
    let start = step(&mut w, &wait);
    assert!(start.truth.contains(&w.orsel));

    // The PC moves to the market, where Pip stands.
    let mut walk = proposal(Verdict::Allowed);
    walk.action = ActionKind::Move;
    walk.move_to = Some(e(market));
    walk.minutes = 10;
    let moved = step(&mut w, &walk);
    assert!(moved.truth.contains(&w.pip) && moved.seen.contains(&w.pip));
    assert!(!moved.truth.contains(&w.orsel));
    assert_eq!(moved.pc_place.as_deref(), Some("market"));

    // A public World Event brings Orsel to the market.
    let orsel = w.orsel;
    schedule_move(&mut w, orsel, "Market", true);
    let arrived = step(&mut w, &wait);
    assert!(arrived.truth.contains(&orsel));

    // A World Event takes Pip away, before the PC's eyes, to a place the
    // PC cannot name: a departure record.
    let pip = w.pip;
    schedule_move(&mut w, pip, "gate square", true);
    let left = step(&mut w, &wait);
    assert!(!left.truth.contains(&pip) && !left.seen.contains(&pip));
    assert!(
        w.view
            .live_facts
            .iter()
            .any(|f| f.slot.subject == pip && f.slot.key == "location" && f.slot.value.is_empty()),
        "a departure record"
    );

    // A new NPC joins the PC.
    let mut meet = wait.clone();
    meet.new_npcs = vec![new_npc(1, "Kett", 34)];
    let met = step(&mut w, &meet);
    let kett = w.view.entity_by_name("Kett").unwrap().id;
    assert!(met.truth.contains(&kett));

    // A hidden character arrives, unseen.
    schedule_move(&mut w, moth, "Market", false);
    let hidden = step(&mut w, &wait);
    assert!(hidden.truth.contains(&moth));
    assert!(!hidden.seen.contains(&moth));
}

/// G4 in Task 2: an intimate Turn is told by implication, any other Turn
/// plainly. The stored proposal keeps the tag for a later `renarrate`.
#[test]
fn an_intimate_turn_is_told_by_implication_only() {
    let w = confirmed_gate();
    assert_eq!(told(&w.view, &w.view, true), Told::Implied);
    assert_eq!(told(&w.view, &w.view, false), Told::Plain);

    let record = |intimate: bool| {
        let mut p = proposal(Verdict::Allowed);
        p.intimate = intimate;
        serde_json::json!({ "proposal": p, "reactions": [], "events": [] }).to_string()
    };
    assert!(stored_intimate(&record(true)));
    assert!(!stored_intimate(&record(false)));
    assert!(!stored_intimate("{}"));
    assert!(!stored_intimate("not json"));
}

/// Every Rust file under `dir`, recursively.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
    out
}

#[test]
fn no_check_names_a_pack() {
    let mut files = rust_files(&src().join("checks"));
    files.push(src().join("mature").join("gate.rs"));
    assert!(files.len() > 2);
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap().to_lowercase();
        assert!(!text.contains("pack"), "{} names a pack", file.display());
    }
}

#[test]
fn only_mature_calls_is_adult() {
    let mature = src().join("mature");
    let domain = src().join("domain").join("mod.rs");
    let mut callers = Vec::new();
    for file in rust_files(&src()) {
        let text = std::fs::read_to_string(&file).unwrap();
        if text.contains("is_adult(") && file != domain {
            callers.push(file);
        }
    }
    assert!(!callers.is_empty(), "mature calls is_adult");
    for file in callers {
        assert!(
            file.starts_with(&mature),
            "{} calls is_adult outside mature",
            file.display()
        );
    }
}
