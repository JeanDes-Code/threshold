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

use support::{
    hidden::{HiddenWorld, claim as hidden_claim, id_of, record, set},
    rig::Rig,
    *,
};
use threshold_core::{
    CallKind, RecordedGm,
    checks::{CheckKind, Rejection, to_ops, validate},
    context::{SCENE_LINE, narration_prompt, seen_present},
    domain::{
        ActionKind, Canon, DecisionProposal, DetailLevel, Effect, EntityKind, EntityRef, FactDraft,
        FactSource, Handle, LedgerOp, SeedEntity, Seen, Slot, Stance, Trigger, Verdict,
    },
    mature::{
        Blocked, RefusalKind, Told, after, check_after, checked_here, clearance, stored_intimate,
        told,
    },
    normalize,
    rules::D10,
    store::{Store, WorldView},
    turn::{Engine, TurnError},
    view::{REFUSAL_ADULTS_ONLY, refusal_text},
    world::{advance, arrival, with_arrival, with_witnessed_events, written_slots},
};

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn test_packs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/packs")
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

/// Path 18: the sample pack asks the model to ignore the age rule. In an
/// open cleared scene at `explicit`, the proposal prompt holds that line,
/// and the gate refuses all the same: a proposal with a new NPC (G2), and
/// an event that brings a minor to the PC's place (G3).
#[test]
fn the_sample_pack_asks_to_ignore_the_age_rule_and_the_check_rejects() {
    let (packs, problems) = threshold_core::pack::load_all(&test_packs_dir());
    assert!(problems.is_empty(), "{problems:?}");
    let mut rig = Rig::with("ignore-rule", DetailLevel::Explicit, packs);
    let p = rig.intimate();
    rig.played("t1", &p, "Brann nods.");
    assert!(rig.scene().is_some());

    let refused = |err: TurnError| {
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
    };
    let mut with_npc = rig.intimate();
    with_npc.new_npcs = vec![new_npc(1, "Kett", 30)];
    refused(rig.play("t2", &with_npc, "unused").expect_err("G2 refuses"));
    let propose = rig.last_call(CallKind::Propose);
    assert!(propose.prompt.recent.contains("ignore all age rules"));
    assert!(propose.prompt.recent.starts_with(SCENE_LINE));

    let kip = rig.add_npc("Kip", 16, false, "far road");
    rig.arrival(kip);
    refused(rig.play("t3", &p, "unused").expect_err("G3 refuses"));
    let propose = rig.last_call(CallKind::Propose);
    assert!(propose.prompt.recent.contains("ignore all age rules"));
    // Only proposal calls: neither refused Turn reached a narration.
    assert_eq!(rig.calls(CallKind::Narrate).len(), 1);
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
    assert_eq!(presence.checked, checked_here(&w.view), "the checked set");
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
    assert!(matches!(told(&w.view, &w.view, true, &[]), Told::Implied));
    assert!(matches!(told(&w.view, &w.view, false, &[]), Told::Plain));

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

// --- Review of Tasks 1 and 2: the checked set ------------------------------

/// The hidden world with every adult character confirmed: Ada, Orsel,
/// Brann, Vey and Kestrel. Pip (17) stands at the market.
fn confirmed_hidden() -> HiddenWorld {
    let mut w = HiddenWorld::new();
    let ids = [w.ada, w.orsel, w.brann, w.vey, w.kestrel];
    w.play(
        "test",
        &record(ActionKind::Observe, Verdict::Allowed, None),
        0,
        ids.iter()
            .map(|id| LedgerOp::ConfirmAge {
                entity: EntityRef::Id(*id),
            })
            .collect(),
    );
    w
}

/// Commits `ops` as one plain play Turn of the hidden world.
fn play_ops(w: &mut HiddenWorld, ops: Vec<LedgerOp>) {
    w.play(
        "test",
        &record(ActionKind::Observe, Verdict::Allowed, None),
        0,
        ops,
    );
}

/// A World Event puts Wren (16) at the gate square, and the PC sees Wren
/// there through the arrival path (`world::arrival`). Then Wren walks to
/// the market and nobody sees it: the PC's Belief is stale.
fn stale_belief() -> (HiddenWorld, u64) {
    let mut w = confirmed_hidden();
    play_ops(
        &mut w,
        vec![
            LedgerOp::AddEntity {
                local: Some(1),
                kind: EntityKind::Npc,
                name: "Wren".into(),
                age: Some(16),
                other_names: vec![],
                age_confirmed: false,
            },
            LedgerOp::AddFact {
                slot: Slot {
                    subject: EntityRef::New(1),
                    key: "location".into(),
                    value: "gate square".into(),
                },
                text: "Wren stands at the gate square.".into(),
                canon: Canon::Established,
                source: FactSource::WorldEvent,
                seen_by: vec![],
                causes: vec![],
            },
        ],
    );
    let view = w.view();
    let wren = id_of(&view, "Wren");
    let seen = arrival(&view, "gate square", view.clock_min);
    assert!(!seen.is_empty(), "the PC sees Wren arrive");
    play_ops(&mut w, seen);
    let view = w.view();
    assert!(seen_present(&view).contains(&wren));
    play_ops(
        &mut w,
        vec![set(
            &view,
            wren,
            "location",
            "market",
            "Wren walks to the market.",
            &[],
        )],
    );
    (w, wren)
}

/// Brann tells the PC that Pip is at the gate square. Pip is at the market.
fn claimed_here() -> HiddenWorld {
    let mut w = confirmed_hidden();
    let (brann, pip, ada) = (w.brann, w.pip, w.ada);
    play_ops(
        &mut w,
        vec![hidden_claim(
            brann,
            pip,
            "location",
            "gate square",
            &[ada],
            0,
        )],
    );
    w
}

/// The names the narrator's `Present:` line lists for `view`.
fn narrator_present(view: &WorldView) -> Vec<u64> {
    let prompt = narration_prompt(view, &[], &[], &[], &[], "", None, &Told::Plain);
    let text = format!("{}{}{}", prompt.stable, prompt.scene, prompt.recent);
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix("Present: "))
        .expect("a Present line");
    line.split(", ").map(|name| id_of(view, name)).collect()
}

/// B1: a minor the PC believes is here, through a stale Belief or a Claim,
/// is checked like one who is truly here, before the Turn (G2) and after
/// its ops (G3).
#[test]
fn the_gate_checks_who_the_pc_believes_is_here() {
    let (w, wren) = stale_belief();
    let view = w.view();
    assert!(!view.present().contains(&wren), "world truth: Wren left");
    assert_eq!(
        clearance(&view, &[]).unwrap_err(),
        Blocked { who: vec![wren] }
    );

    let w = claimed_here();
    let view = w.view();
    assert!(!view.present().contains(&w.pip));
    assert_eq!(
        clearance(&view, &[]).unwrap_err(),
        Blocked { who: vec![w.pip] }
    );

    // G3: the Claim comes in this Turn's ops.
    let w = confirmed_hidden();
    let view = w.view();
    assert!(clearance(&view, &[]).is_ok());
    let ops = vec![hidden_claim(
        w.brann,
        w.pip,
        "location",
        "gate square",
        &[w.ada],
        0,
    )];
    let mut p = proposal(Verdict::Allowed);
    p.intimate = true;
    assert_eq!(
        check_after(&p, &view, &after(&view, &ops)).unwrap_err(),
        Blocked { who: vec![w.pip] }
    );
}

/// B1: the narrator's Present line, built from the same view, never names
/// anyone the gate does not check.
#[test]
fn the_narrator_present_line_is_a_subset_of_the_checked_set() {
    let (stale, wren) = stale_belief();
    let claimed = claimed_here();
    for (view, believed) in [(stale.view(), wren), (claimed.view(), claimed.pip)] {
        let checked = checked_here(&view);
        let listed = narrator_present(&view);
        assert!(listed.contains(&believed), "{listed:?}");
        for id in &listed {
            assert!(checked.contains(id), "{id} is told but not checked");
        }
        for id in seen_present(&view) {
            assert!(checked.contains(&id), "{id} is seen but not checked");
        }
        for id in view.present() {
            assert!(checked.contains(&id), "{id} is here but not checked");
        }
    }
}

// --- Review of Tasks 1 and 2: the place rule of the gate --------------------

/// M1: for the gate, a place written another way is the PC's place: case,
/// accents, punctuation, a word-prefix, or another name of the same place
/// entity. Ada stands at "gate square".
#[test]
fn a_near_place_name_counts_as_here() {
    for place in [
        "Gate Square.",
        "gate-square",
        "Gâte Square",
        "gate square, by the well",
        "Gate",
        "the gate square",
    ] {
        let mut w = confirmed_gate();
        let wren = add_character(&mut w, "Wren", 16, true, place);
        assert_eq!(
            clearance(&w.view, &[]).unwrap_err(),
            Blocked { who: vec![wren] },
            "{place}"
        );
        let errs = validate(&intimate(&w, Stance::Comply), &w.view).expect_err(place);
        assert_eq!(adults_only(&errs), [Some(e(wren))], "{place}");
    }

    // Another name of the place entity the PC stands at.
    let mut w = confirmed_gate();
    w.commit(vec![LedgerOp::AddEntity {
        local: None,
        kind: EntityKind::Place,
        name: "Gate Square".into(),
        age: None,
        other_names: vec!["Old Yard".into()],
        age_confirmed: false,
    }]);
    let wren = add_character(&mut w, "Wren", 16, true, "old yard");
    assert_eq!(
        clearance(&w.view, &[]).unwrap_err(),
        Blocked { who: vec![wren] }
    );
}

/// M1: unknown is not cleared. A character with no live location counts
/// as here, and so does every character when the PC's own place is
/// unknown.
#[test]
fn a_character_with_no_place_counts_as_here() {
    let mut w = confirmed_gate();
    w.commit(vec![LedgerOp::AddEntity {
        local: None,
        kind: EntityKind::Npc,
        name: "Wren".into(),
        age: Some(16),
        other_names: vec![],
        age_confirmed: true,
    }]);
    let wren = w.view.entity_by_name("Wren").unwrap().id;
    assert_eq!(
        clearance(&w.view, &[]).unwrap_err(),
        Blocked { who: vec![wren] }
    );

    // The PC's place unknown: Pip, at the market, counts too.
    let mut w = confirmed_gate();
    let mut view = w.view.clone();
    view.live_facts
        .retain(|f| !(f.slot.subject == w.ada && f.slot.key == "location"));
    assert_eq!(
        clearance(&view, &[]).unwrap_err(),
        Blocked { who: vec![w.pip] }
    );
    let pip = w.pip;
    bring(&mut w, pip, "far road");
    assert!(clearance(&w.view, &[]).is_ok(), "far road is not here");
}

/// M1: a place that shares no first words with the PC's place does not
/// block, even when one word is the same.
#[test]
fn another_place_does_not_block() {
    for place in ["market", "far road", "square of the mill", "town gate"] {
        let mut w = confirmed_gate();
        add_character(&mut w, "Wren", 16, true, place);
        assert!(clearance(&w.view, &[]).is_ok(), "{place}");
        assert!(
            validate(&intimate(&w, Stance::Comply), &w.view).is_ok(),
            "{place}"
        );
    }
}

// --- Review of Tasks 1 and 2: every character a tagged proposal names -------

/// B2: one tagged proposal per field that can name a character, each naming
/// Pip (17, at the market, not here), with its label.
fn proposals_naming_pip(w: &World) -> Vec<(&'static str, DecisionProposal)> {
    use threshold_core::domain::{Cancel, Cause, EventDraft, Factor, ItemStatus, Leave, Settle};
    let pip = e(w.pip);
    let pip_fact = f(w.view.live_fact(w.pip, "location").unwrap().id);
    let base = || intimate(w, Stance::Comply);
    let mut out = Vec::new();
    let mut with = |label: &'static str, change: &dyn Fn(&mut DecisionProposal)| {
        let mut p = base();
        change(&mut p);
        out.push((label, p));
    };
    with("facts.slot.subject", &|p| {
        p.facts = vec![fact(slot(pip, "mood", "calm"), Seen::Gm, vec![])];
    });
    with("facts.said_by", &|p| {
        let mut said = fact(slot(e(w.orsel), "mood", "calm"), Seen::Pc, vec![]);
        said.said_by = Some(pip);
        p.facts = vec![said];
    });
    with("facts.because", &|p| {
        p.facts = vec![fact(
            slot(e(w.orsel), "mood", "calm"),
            Seen::Gm,
            vec![pip_fact],
        )];
    });
    with("check.against", &|p| {
        p.verdict = Verdict::Check;
        p.check = Some(check(
            Some(pip),
            slot(e(w.orsel), "mood", "calm"),
            slot(e(w.orsel), "mood", "cross"),
        ));
    });
    with("check.win.subject", &|p| {
        p.verdict = Verdict::Check;
        p.check = Some(check(
            None,
            slot(pip, "mood", "calm"),
            slot(e(w.orsel), "mood", "cross"),
        ));
    });
    with("check.lose.subject", &|p| {
        p.verdict = Verdict::Check;
        p.check = Some(check(
            None,
            slot(e(w.orsel), "mood", "calm"),
            slot(pip, "mood", "cross"),
        ));
    });
    with("check.factors.fact", &|p| {
        p.verdict = Verdict::Check;
        let mut c = check(
            None,
            slot(e(w.orsel), "mood", "calm"),
            slot(e(w.orsel), "mood", "cross"),
        );
        c.factors = vec![Factor {
            fact: pip_fact,
            helps: true,
        }];
        p.check = Some(c);
    });
    with("blockers", &|p| p.blockers = vec![pip_fact]);
    with("reactions.npc", &|p| {
        p.reactions.push(reaction(pip, Stance::Comply, None));
    });
    with("reactions.cause", &|p| {
        p.reactions[0].cause = Some(Cause::Fact(pip_fact));
    });
    with("leaves.npc", &|p| {
        p.leaves = vec![Leave {
            npc: pip,
            to: "away".into(),
        }];
    });
    with("settles.item", &|p| {
        p.settles = Some(Settle {
            item: format!("E{} pending_meet", w.pip),
            outcome: ItemStatus::Done,
        });
    });
    let event = |actor: Handle| EventDraft {
        in_minutes: 30,
        actor,
        goal: None,
        text: "Someone files a record.".into(),
        effects: vec![],
        seen_by: vec![],
        public: false,
        moves_actor_to: None,
    };
    with("schedule.actor", &|p| p.schedule = Some(event(pip)));
    with("schedule.seen_by", &|p| {
        let mut s = event(e(w.orsel));
        s.seen_by = vec![pip];
        p.schedule = Some(s);
    });
    with("schedule.effects.subject", &|p| {
        let mut s = event(e(w.orsel));
        s.effects = vec![slot(pip, "mood", "busy")];
        p.schedule = Some(s);
    });
    with("cancel.because", &|p| {
        p.cancel = vec![Cancel {
            event: threshold_core::domain::Handle::WorldEvent(w.bell),
            because: pip_fact,
        }];
    });
    out
}

/// B2: a tagged proposal that names a character anywhere, here or not,
/// needs that character cleared. Pip (17) stands at the market.
#[test]
fn a_tagged_proposal_checks_every_character_it_names() {
    let w = confirmed_gate();
    assert!(validate(&intimate(&w, Stance::Comply), &w.view).is_ok());
    for (label, p) in proposals_naming_pip(&w) {
        let errs = validate(&p, &w.view).expect_err(label);
        assert!(
            adults_only(&errs).contains(&Some(e(w.pip))),
            "{label}: {errs:?}"
        );
    }
    // The same proposals, untagged, are not refused for Pip.
    for (label, mut p) in proposals_naming_pip(&w) {
        p.intimate = false;
        if let Err(errs) = validate(&p, &w.view) {
            assert!(adults_only(&errs).is_empty(), "{label}: {errs:?}");
        }
    }
}

/// B2: a handle that names no known entity, fact, event or goal is not
/// cleared: the gate fails closed.
#[test]
fn a_tagged_proposal_that_names_an_unknown_handle_is_refused() {
    let w = confirmed_gate();
    let mut p = intimate(&w, Stance::Comply);
    p.facts = vec![fact(slot(e(999), "mood", "calm"), Seen::Gm, vec![])];
    let errs = validate(&p, &w.view).expect_err("unknown");
    assert!(adults_only(&errs).contains(&Some(e(999))), "{errs:?}");

    let mut p = intimate(&w, Stance::Comply);
    p.blockers = vec![f(999)];
    let errs = validate(&p, &w.view).expect_err("unknown fact");
    assert!(adults_only(&errs).contains(&Some(f(999))), "{errs:?}");
}

/// B2, explicit: a Check against a minor who is elsewhere, in a tagged
/// Turn, is a final Refusal after one proposal call.
#[test]
fn a_check_against_a_minor_in_a_tagged_turn_is_refused() {
    let mut w = confirmed_gate();
    let mut p = intimate(&w, Stance::Comply);
    p.verdict = Verdict::Check;
    p.contested = true;
    p.check = Some(check(
        Some(e(w.pip)),
        slot(e(w.orsel), "mood", "calm"),
        slot(e(w.orsel), "mood", "cross"),
    ));
    let gm = RecordedGm::new(
        vec![
            serde_json::to_string(&p).unwrap(),
            serde_json::to_string(&p).unwrap(),
        ],
        vec!["unused".into()],
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
    assert_eq!((kind, blocked), (RefusalKind::AdultsOnly, vec![w.pip]));
    assert_eq!(gm.calls().len(), 1, "no retry after a refusal");
}

/// Every property path of a `DecisionProposal` as its JSON schema spells
/// it, `[]` for an array item. A `Handle` is a leaf.
fn schema_paths(
    schema: &serde_json::Value,
    defs: &serde_json::Value,
    at: &str,
    out: &mut BTreeSet<String>,
) {
    if let Some(name) = schema
        .get("$ref")
        .and_then(|r| r.as_str())
        .and_then(|r| r.strip_prefix("#/$defs/"))
    {
        if name == "Handle" {
            out.insert(at.to_owned());
        } else {
            schema_paths(&defs[name], defs, at, out);
        }
        return;
    }
    let mut inner = false;
    for key in ["anyOf", "oneOf", "allOf"] {
        for part in schema
            .get(key)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            // The `null` of an `Option` adds no field.
            if part.get("type").and_then(|t| t.as_str()) == Some("null") {
                continue;
            }
            inner = true;
            schema_paths(part, defs, at, out);
        }
    }
    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        inner = true;
        for (key, value) in props {
            let path = if at.is_empty() {
                key.clone()
            } else {
                format!("{at}.{key}")
            };
            schema_paths(value, defs, &path, out);
        }
    }
    if let Some(items) = schema.get("items") {
        inner = true;
        schema_paths(items, defs, &format!("{at}[]"), out);
    }
    if !inner && !at.is_empty() {
        out.insert(at.to_owned());
    }
}

/// Every field of a Decision Proposal, and whether it can name a
/// character, directly or through a fact, an event or a goal. The gate's
/// collector (`mature::check`, `named` in `gate.rs`) reads each `true`
/// field. A new field fails this test until its author adds it here, and
/// to the collector when it can name a character: the collector also
/// destructures the proposal with no `..`, so the compiler asks too.
const PROPOSAL_FIELDS: &[(&str, bool)] = &[
    ("action", false),
    ("blockers[]", true),
    ("cancel[].because", true),
    ("cancel[].event", true),
    ("check.against", true),
    ("check.factors[].fact", true),
    ("check.factors[].helps", false),
    ("check.lose.key", false),
    ("check.lose.subject", true),
    ("check.lose.value", false),
    ("check.win.key", false),
    ("check.win.subject", true),
    ("check.win.value", false),
    ("contested", false),
    ("facts[].because[]", true),
    ("facts[].said_by", true),
    ("facts[].seen", false),
    ("facts[].slot.key", false),
    ("facts[].slot.subject", true),
    ("facts[].slot.value", false),
    ("intimate", false),
    ("leaves[].npc", true),
    ("leaves[].to", false),
    ("minutes", false),
    ("move_to", true),
    ("new_npcs[].age", false),
    ("new_npcs[].id", true),
    ("new_npcs[].name", false),
    ("new_npcs[].voice", false),
    ("reactions[].cause", false),
    ("reactions[].cause.fact", true),
    ("reactions[].npc", true),
    ("reactions[].plan", false),
    ("reactions[].stance", false),
    ("schedule.actor", true),
    ("schedule.effects[].key", false),
    ("schedule.effects[].subject", true),
    ("schedule.effects[].value", false),
    ("schedule.goal", true),
    ("schedule.in_minutes", false),
    ("schedule.moves_actor_to", false),
    ("schedule.public", false),
    ("schedule.seen_by[]", true),
    ("schedule.text", false),
    ("settles.item", true),
    ("settles.outcome", false),
    ("verdict", false),
];

/// B2: the list above is every field of the proposal schema.
#[test]
fn every_proposal_field_is_sorted_for_the_gate() {
    let schema = serde_json::to_value(schemars::schema_for!(DecisionProposal)).unwrap();
    let mut found = BTreeSet::new();
    schema_paths(&schema, &schema["$defs"], "", &mut found);
    let listed: BTreeSet<String> = PROPOSAL_FIELDS
        .iter()
        .map(|(p, _)| (*p).to_owned())
        .collect();
    assert_eq!(
        found, listed,
        "a proposal field changed: decide whether it names a character, then update the gate's collector and this list"
    );
}

// --- Review of Tasks 1 and 2: the sticky tag reads the raw text -------------

/// Plays one Turn on `w` whose first answer is `first` and whose retry is
/// an untagged proposal with Brann complying.
fn play_with_first(mut w: World, first: String) -> Result<(), TurnError> {
    let mut retry = intimate(&w, Stance::Comply);
    retry.intimate = false;
    let gm = RecordedGm::new(
        vec![first, serde_json::to_string(&retry).unwrap()],
        vec!["Brann nods.".into()],
    );
    let store = std::mem::replace(&mut w.store, Store::open_in_memory().unwrap());
    let mut engine = Engine::new(store, Box::new(gm), Box::new(D10), vec![], w.campaign);
    engine
        .play_turn("t1".into(), "I lean closer.", &mut |_| {})
        .map(|_| ())
}

/// M2: a first answer that says `intimate` is true keeps the tag on the
/// retry even when it is not valid JSON: cut, fenced, with trailing text,
/// or with the key twice. Pip (17) stands here, so the tagged retry is
/// refused; an untagged retry would commit.
#[test]
fn a_malformed_first_answer_keeps_the_tag() {
    let gate = || {
        let mut w = confirmed_gate();
        let pip = w.pip;
        bring(&mut w, pip, "Gate Square");
        w
    };
    let tagged = serde_json::to_string(&intimate(&gate(), Stance::Comply)).unwrap();
    let mut untagged = intimate(&gate(), Stance::Comply);
    untagged.intimate = false;
    let untagged = serde_json::to_string(&untagged).unwrap();
    let cut = tagged[..tagged.find("\"intimate\":true").unwrap() + 15].to_owned();
    let shapes = [
        ("cut", cut),
        ("fenced", format!("```json\n{tagged}\n```")),
        ("trailing text", format!("{tagged} That is the answer.")),
        (
            "duplicate key",
            untagged.replacen('{', "{\"intimate\":true,", 1),
        ),
        (
            "spaces and quotes",
            "{ \"intimate\" : \"true\", \"action\": ".to_owned(),
        ),
    ];
    for (shape, first) in shapes {
        let err = play_with_first(gate(), first).expect_err(shape);
        assert!(
            matches!(
                err,
                TurnError::Refused {
                    kind: RefusalKind::AdultsOnly,
                    ..
                }
            ),
            "{shape}: {err:?}"
        );
    }

    // A malformed first answer that says false does not tag the retry.
    let first = format!("```json\n{untagged}\n```");
    play_with_first(gate(), first).expect("an untagged Turn commits");
}

// --- Review of Tasks 1 and 2: minors ----------------------------------------

/// One Turn of `w` whose two answers are `p`: the result and the number of
/// proposal calls.
fn play_twice(mut w: World, p: &DecisionProposal) -> (Result<(), TurnError>, usize) {
    let raw = serde_json::to_string(p).unwrap();
    let gm = RecordedGm::new(vec![raw.clone(), raw], vec!["unused".into()]);
    let store = std::mem::replace(&mut w.store, Store::open_in_memory().unwrap());
    let mut engine = Engine::new(
        store,
        Box::new(gm.clone()),
        Box::new(D10),
        vec![],
        w.campaign,
    );
    let result = engine
        .play_turn("t1".into(), "I lean closer.", &mut |_| {})
        .map(|_| ());
    let calls = gm
        .calls()
        .iter()
        .filter(|c| c.kind == CallKind::Propose)
        .count();
    (result, calls)
}

/// `CheckKind::is_final` is the one list of final kinds: `turn::plan`
/// reads it through `mature::final_refusal`. Each final kind ends the Turn
/// after one proposal call; a kind made non-final would get the retry.
#[test]
fn every_final_kind_ends_the_turn_after_one_call() {
    // `DetailExcluded`: the default level.
    let mut w = World::new();
    let ids = [w.ada, w.orsel, w.brann];
    w.confirm(&ids);
    let p = intimate(&w, Stance::Comply);
    let (result, calls) = play_twice(w, &p);
    assert!(CheckKind::DetailExcluded.is_final());
    assert!(
        matches!(
            result,
            Err(TurnError::Refused {
                kind: RefusalKind::Excluded,
                ..
            })
        ),
        "{result:?}"
    );
    assert_eq!(calls, 1, "excluded");

    // `AdultsOnly`: Brann is unconfirmed.
    let mut w = World::with_detail(DetailLevel::Implied);
    let ids = [w.ada, w.orsel];
    w.confirm(&ids);
    let p = intimate(&w, Stance::Comply);
    let (result, calls) = play_twice(w, &p);
    assert!(CheckKind::AdultsOnly.is_final());
    assert!(
        matches!(
            result,
            Err(TurnError::Refused {
                kind: RefusalKind::AdultsOnly,
                ..
            })
        ),
        "{result:?}"
    );
    assert_eq!(calls, 1, "adults only");
}

/// G2 alone, with no G3 behind it: a hidden minor at the PC's place, whom
/// the PC does not see, refuses the tagged proposal in `mature::check`.
#[test]
fn g2_alone_refuses_a_hidden_minor() {
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
    assert!(
        !seen_present(&w.view).contains(&moth),
        "the PC does not see Moth"
    );

    let mut errs = Vec::new();
    threshold_core::mature::check(&intimate(&w, Stance::Comply), &w.view, &mut errs);
    assert_eq!(adults_only(&errs), [Some(e(moth))]);
    assert!(
        errs.iter().all(|r| r.check != CheckKind::Unwilling),
        "{errs:?}"
    );
}

/// OOC: the stable neutral rule line holds for an out-of-character answer
/// too, so "off" does not wait for the scene tasks.
#[test]
fn the_ooc_rules_hold_the_intimate_rule() {
    use threshold_core::{context::INTIMATE_RULE, turn::OOC_RULES};
    assert!(OOC_RULES.contains(INTIMATE_RULE));

    let mut w = confirmed_gate();
    let gm = RecordedGm::new(vec![], vec!["You stand at the gate.".into()]);
    let store = std::mem::replace(&mut w.store, Store::open_in_memory().unwrap());
    let mut engine = Engine::new(
        store,
        Box::new(gm.clone()),
        Box::new(D10),
        vec![],
        w.campaign,
    );
    engine.ooc("q1".into(), "Where am I?").unwrap();
    let calls = gm.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].prompt.system.contains(INTIMATE_RULE));
}

// --- Review of Tasks 1 and 2: a Refusal is not a failed Turn ----------------

/// M3: the same `op_id` after a Refusal gets the same Refusal, with no
/// model call, so a resend cannot run the model until it answers
/// otherwise. A new Intent is a new op and is judged afresh.
#[test]
fn the_same_op_after_a_refusal_makes_no_model_call() {
    let mut w = confirmed_gate();
    let pip = w.pip;
    bring(&mut w, pip, "Gate Square");
    let raw = serde_json::to_string(&intimate(&w, Stance::Comply)).unwrap();
    let gm = RecordedGm::new(vec![raw.clone(), raw.clone(), raw], vec![]);
    let store = std::mem::replace(&mut w.store, Store::open_in_memory().unwrap());
    let mut engine = Engine::new(
        store,
        Box::new(gm.clone()),
        Box::new(D10),
        vec![],
        w.campaign,
    );
    let refused = |r: Result<threshold_core::turn::TurnOutcome, TurnError>| match r {
        Err(TurnError::Refused { kind, blocked }) => (kind, blocked),
        other => panic!("expected a refusal, got {other:?}"),
    };
    let first = refused(engine.play_turn("t1".into(), "I lean closer.", &mut |_| {}));
    assert_eq!(first, (RefusalKind::AdultsOnly, vec![pip]));
    assert_eq!(gm.calls().len(), 1);

    let again = refused(engine.play_turn("t1".into(), "I lean closer.", &mut |_| {}));
    assert_eq!(again, first);
    assert_eq!(gm.calls().len(), 1, "no model call for the same op");

    let fresh = refused(engine.play_turn("t2".into(), "I lean closer.", &mut |_| {}));
    assert_eq!(fresh, first);
    assert_eq!(gm.calls().len(), 2, "a new op runs the model");
}
