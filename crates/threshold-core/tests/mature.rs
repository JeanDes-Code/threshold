//! The adults-only gate (mature content module, Task 1): the Detail Level,
//! the age rule over stored and confirmed ages, the namesake rule, the
//! stance rule and the final refusal. Neutral fixtures only.

mod support;

use std::path::{Path, PathBuf};

use support::*;
use threshold_core::{
    CallKind, RecordedGm,
    checks::{CheckKind, Rejection, to_ops, validate},
    domain::{
        Canon, DecisionProposal, DetailLevel, EntityKind, EntityRef, FactDraft, FactSource, Handle,
        LedgerOp, SeedEntity, Slot, Stance, Verdict,
    },
    mature::{Blocked, RefusalKind, clearance},
    pack::ContentPack,
    rules::D10,
    store::{Store, WorldView},
    turn::{Engine, TurnError},
    view::{REFUSAL_ADULTS_ONLY, refusal_text},
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
