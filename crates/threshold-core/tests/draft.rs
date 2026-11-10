//! The Seed Draft: its gaps, approval, and the draft store (data shape 3.5).

use std::{fs, path::PathBuf};

use threshold_core::{
    build::{
        BuildCheck, BuildFailure, DebugEntry, DraftLine, DraftState, DraftStore, Gap, GapKind,
        KnownFact, SeedDraft, Stage, apply_edit, approve, gaps,
    },
    context::ADVENTURE_BLOCK_MAX_CHARS,
    domain::{AdventureSeed, EntityKind, OtherName, PcSeed, SeedEntity, SeedFact, StartSeed},
};

/// An empty folder of its own under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("threshold-draft-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn entity(name: &str, kind: EntityKind, age: Option<u16>) -> SeedEntity {
    SeedEntity {
        name: name.into(),
        kind,
        age,
        other_names: vec![],
        hidden: false,
    }
}

fn other(name: &str) -> OtherName {
    OtherName {
        name: name.into(),
        known_to: vec![],
    }
}

fn known(subject: &str, key: &str, value: &str) -> KnownFact {
    KnownFact {
        subject: subject.into(),
        key: key.into(),
        value: value.into(),
        text: format!("{subject} {key} {value}."),
        open: false,
    }
}

/// A draft with no gap: entities[0] is Jean (an NPC), entities[1] the
/// landing stage (a place).
fn valid_draft() -> SeedDraft {
    let seed = AdventureSeed {
        premise: "A ferry crossing at night".into(),
        pc: PcSeed {
            name: "Wren".into(),
            age: 29,
            other_names: vec![other("Jay")],
            concept: "A courier".into(),
        },
        entities: vec![
            entity("Jean", EntityKind::Npc, Some(40)),
            entity("The landing stage", EntityKind::Place, None),
        ],
        boundaries: vec![],
        decided: vec![],
        proposed: vec![],
        story_language: "en".into(),
        themes: vec![],
        gm_agreement: String::new(),
        resolution: String::new(),
        min_age: Some(18),
        start: StartSeed {
            place: "The landing stage".into(),
            present: vec!["Jean".into()],
            situation: "The last ferry is late.".into(),
            when: "Midnight".into(),
        },
        open_details: vec![],
    };
    let mut draft = SeedDraft::new("A ferry, a courier, a late night.".into(), seed);
    draft.knowledge = vec![
        known("Jean", "job", "ferryman"),
        known("Jay", "debt", "ten"),
    ];
    draft
}

fn gap(kind: GapKind, at: &str) -> Gap {
    Gap {
        kind,
        at: at.into(),
    }
}

/// The gaps of `valid_draft` after `edit`.
fn gaps_after(edit: impl FnOnce(&mut SeedDraft)) -> Vec<Gap> {
    let mut draft = valid_draft();
    edit(&mut draft);
    gaps(&draft)
}

#[test]
fn gaps_name_each_problem() {
    assert_eq!(gaps(&valid_draft()), vec![]);

    // Missing
    assert_eq!(
        gaps_after(|d| d.seed.premise = "  ".into()),
        vec![gap(GapKind::Missing, "premise")]
    );
    assert_eq!(
        gaps_after(|d| d.seed.pc.name = String::new()),
        vec![gap(GapKind::Missing, "pc.name")]
    );
    assert_eq!(
        gaps_after(|d| d.seed.pc.concept = String::new()),
        vec![gap(GapKind::Missing, "pc.concept")]
    );
    assert_eq!(
        gaps_after(|d| d.seed.story_language = String::new()),
        vec![gap(GapKind::Missing, "story_language")]
    );
    assert_eq!(
        gaps_after(|d| d.seed.start.place = String::new()),
        vec![gap(GapKind::Missing, "start.place")]
    );
    assert_eq!(
        gaps_after(|d| d.knowledge[0].text = String::new()),
        vec![gap(GapKind::Missing, "knowledge[0].text")]
    );

    // AgeMissing: a character with no age.
    assert_eq!(
        gaps_after(|d| d.seed.entities[0].age = None),
        vec![gap(GapKind::AgeMissing, "entities[0].age")]
    );

    // AgeUnconfirmed: a name in `ages_to_confirm`, matched as a name is; a
    // name no longer in the roster is dropped.
    assert_eq!(
        gaps_after(|d| d.ages_to_confirm = vec!["JEAN".into(), "Wren".into(), "Gone".into()]),
        vec![
            gap(GapKind::AgeUnconfirmed, "pc.age"),
            gap(GapKind::AgeUnconfirmed, "entities[0].age"),
        ]
    );

    // AgeBelowMin
    assert_eq!(
        gaps_after(|d| d.seed.min_age = Some(30)),
        vec![gap(GapKind::AgeBelowMin, "pc.age")]
    );
    assert_eq!(
        gaps_after(|d| d.seed.entities[0].age = Some(17)),
        vec![gap(GapKind::AgeBelowMin, "entities[0].age")]
    );

    // UnknownName: a known fact's subject, the start place, a present name.
    assert_eq!(
        gaps_after(|d| d.knowledge[1].subject = "Marlowe".into()),
        vec![gap(GapKind::UnknownName, "knowledge[1].subject")]
    );
    assert_eq!(
        gaps_after(|d| d.seed.start.place = "Jean".into()),
        vec![gap(GapKind::UnknownName, "start.place")]
    );
    assert_eq!(
        gaps_after(|d| d.seed.start.present.push("The landing stage".into())),
        vec![gap(GapKind::UnknownName, "start.present[1]")]
    );

    // DuplicateName: one normal name twice, counting other names and
    // accents; the same other name twice on one entity; the reserved "world".
    assert_eq!(
        gaps_after(|d| d
            .seed
            .entities
            .push(entity("JÉAN", EntityKind::Npc, Some(30)))),
        vec![gap(GapKind::DuplicateName, "entities[2].name")]
    );
    assert_eq!(
        gaps_after(|d| {
            let mut mara = entity("Mara", EntityKind::Npc, Some(30));
            mara.other_names = vec![other("jean")];
            d.seed.entities.push(mara);
        }),
        vec![gap(GapKind::DuplicateName, "entities[2].other_names[0]")]
    );
    assert_eq!(
        gaps_after(|d| d.seed.entities[0].other_names = vec![other("Old Jean"), other("old jean")]),
        vec![gap(GapKind::DuplicateName, "entities[0].other_names[1]")]
    );
    assert_eq!(
        gaps_after(|d| d.seed.entities[0].other_names = vec![other("Jean")]),
        vec![gap(GapKind::DuplicateName, "entities[0].other_names[0]")]
    );
    assert_eq!(
        gaps_after(|d| d
            .seed
            .entities
            .push(entity("World", EntityKind::Place, None))),
        vec![gap(GapKind::DuplicateName, "entities[2].name")]
    );

    // PcInRoster: the PC, or one of its names, in the roster.
    assert_eq!(
        gaps_after(|d| d
            .seed
            .entities
            .push(entity("wren", EntityKind::Npc, Some(29)))),
        vec![gap(GapKind::PcInRoster, "entities[2].name")]
    );
    assert_eq!(
        gaps_after(|d| {
            let mut mara = entity("Mara", EntityKind::Npc, Some(30));
            mara.other_names = vec![other("Jay")];
            d.seed.entities.push(mara);
        }),
        vec![gap(GapKind::PcInRoster, "entities[2].other_names[0]")]
    );

    // WrongKind: an age on a place, a second PC, a hand-written open key.
    assert_eq!(
        gaps_after(|d| d.seed.entities[1].age = Some(3)),
        vec![gap(GapKind::WrongKind, "entities[1].age")]
    );
    assert_eq!(
        gaps_after(|d| d
            .seed
            .entities
            .push(entity("Kit", EntityKind::Pc, Some(20)))),
        vec![gap(GapKind::WrongKind, "entities[2].kind")]
    );
    assert_eq!(
        gaps_after(|d| d.knowledge[0].key = "Pending_arrest".into()),
        vec![gap(GapKind::WrongKind, "knowledge[0].key")]
    );

    // SeedBlockTooLong: the stable block above its cap.
    assert_eq!(
        gaps_after(|d| d.seed.gm_agreement = "x".repeat(ADVENTURE_BLOCK_MAX_CHARS)),
        vec![gap(GapKind::SeedBlockTooLong, "seed")]
    );
}

fn open(mut fact: KnownFact) -> KnownFact {
    fact.open = true;
    fact
}

#[test]
fn slot_conflicts_are_gaps() {
    let conflict = |at: &str| vec![gap(GapKind::SlotConflict, at)];

    // Two known facts on one slot, any values; names resolve first.
    assert_eq!(
        gaps_after(|d| d.knowledge.push(known("jean", "Job", "Ferryman"))),
        conflict("knowledge[2]")
    );
    assert_eq!(
        gaps_after(|d| d.knowledge.push(known("Jean", "job", "sailor"))),
        conflict("knowledge[2]")
    );
    assert_eq!(
        gaps_after(|d| d.knowledge.push(known("Wren", "debt", "twelve"))),
        conflict("knowledge[2]")
    );
    assert_eq!(
        gaps_after(|d| {
            d.knowledge.push(open(known("Jean", "move", "the city")));
            d.knowledge.push(open(known("Jean", "move", "the coast")));
        }),
        conflict("knowledge[3]")
    );

    // A known fact on the slot of a seed `decided` fact, or of an other name.
    let decided = SeedFact {
        subject: "Jean".into(),
        key: "job".into(),
        value: "ferryman".into(),
        text: "Jean runs the ferry.".into(),
    };
    assert_eq!(
        gaps_after(|d| d.seed.decided.push(decided.clone())),
        conflict("knowledge[0]")
    );
    assert_eq!(
        gaps_after(|d| {
            d.seed.decided.push(decided.clone());
            d.knowledge[0].open = true;
        }),
        conflict("knowledge[0]")
    );
    assert_eq!(
        gaps_after(|d| d.knowledge.push(known("Wren", "alias_jay", "Jay"))),
        conflict("knowledge[2]")
    );

    // A settled and an open known fact on one subject and key: the later
    // entry is named, in either order.
    assert_eq!(
        gaps_after(|d| d.knowledge.push(open(known("Jean", "job", "captain")))),
        conflict("knowledge[2]")
    );
    assert_eq!(
        gaps_after(|d| d.knowledge.insert(0, open(known("Jean", "job", "captain")))),
        conflict("knowledge[1]")
    );
}

#[test]
fn edited_draft_never_panics() {
    let mut draft = valid_draft();
    draft.ages_to_confirm = vec!["Jean".into()];
    draft.seed.entities.remove(0);

    let found = gaps(&draft);
    assert!(found.contains(&gap(GapKind::UnknownName, "knowledge[0].subject")));
    assert!(found.contains(&gap(GapKind::UnknownName, "start.present[0]")));
    assert!(!found.iter().any(|g| g.kind == GapKind::AgeUnconfirmed));
    assert_eq!(approve(&mut draft), Err(found));
    assert_eq!(draft.state, DraftState::Draft);
}

#[test]
fn approve_needs_zero_gaps_and_sets_the_state() {
    let mut draft = valid_draft();
    draft.seed.entities[0].age = None;
    assert_eq!(
        approve(&mut draft),
        Err(vec![gap(GapKind::AgeMissing, "entities[0].age")])
    );
    assert_eq!(draft.state, DraftState::Draft);

    draft.seed.entities[0].age = Some(40);
    assert_eq!(approve(&mut draft), Ok(()));
    assert_eq!(draft.state, DraftState::Approved);
}

#[test]
fn any_save_returns_the_draft_to_draft_state() {
    let dir = TempDir::new("edit");
    let store = DraftStore::open(&dir.0).unwrap();
    let mut draft = valid_draft();
    approve(&mut draft).unwrap();
    draft.last_failure = Some(BuildFailure {
        stage: Stage::NpcState,
        check: BuildCheck::Agency,
    });
    store.save(&draft).unwrap();

    let mut seed = draft.seed.clone();
    seed.premise = "A ferry crossing at dawn".into();
    let knowledge = draft.knowledge.clone();
    apply_edit(
        &mut draft,
        seed,
        knowledge,
        vec!["Jean".into()],
        vec!["Wren".into()],
    );
    store.save(&draft).unwrap();

    let loaded = store.load(&draft.id).unwrap();
    assert_eq!(loaded.state, DraftState::Draft);
    assert_eq!(loaded.seed.premise, "A ferry crossing at dawn");
    assert_eq!(loaded.ages_to_confirm, vec!["Jean".to_owned()]);
    // The model's claim follows the Player's renames, as the list to
    // confirm does.
    assert_eq!(loaded.ages_stated, vec!["Wren".to_owned()]);
    assert_eq!(loaded.last_failure, None);
}

#[test]
fn store_round_trip_and_atomic_save() {
    let dir = TempDir::new("store");
    let store = DraftStore::open(&dir.0).unwrap();
    let mut draft = valid_draft();
    draft.seed.premise = format!("A ferry\ncrossing   at night, {}", "and on ".repeat(20));
    draft.last_failure = Some(BuildFailure {
        stage: Stage::HiddenFacts,
        check: BuildCheck::Secrecy,
    });
    store.save(&draft).unwrap();
    store.save(&draft).unwrap();

    assert_eq!(store.load(&draft.id).unwrap(), draft);
    assert_eq!(dir.files(), vec![format!("{}.json", draft.id)]);
    let title: String = format!("A ferry crossing at night, {}", "and on ".repeat(20))
        .chars()
        .take(60)
        .collect();
    assert_eq!(
        store.list().unwrap(),
        vec![DraftLine {
            id: draft.id.clone(),
            title,
            state: DraftState::Draft,
        }]
    );

    // An id is a file stem, never a path.
    assert!(store.load("../escape").is_err());
    let mut bad = draft.clone();
    bad.id = "draft-1/../../x".into();
    assert!(store.save(&bad).is_err());
}

#[test]
fn debug_file_is_replaced_per_run_and_deleted_with_the_draft() {
    let dir = TempDir::new("debug");
    let store = DraftStore::open(&dir.0).unwrap();
    let draft = valid_draft();
    store.save(&draft).unwrap();
    let entry = |step: &str| DebugEntry {
        op_id: draft.id.clone(),
        step: step.into(),
        prompt: "the prompt".into(),
        raw: "{\"raw\": true}\nsecond line".into(),
        ms: 12,
    };
    assert_eq!(store.read_debug(&draft.id).unwrap(), vec![]);

    store.clear_debug(&draft.id).unwrap();
    store.append_debug(&draft.id, &entry("draft_seed")).unwrap();
    store
        .append_debug(&draft.id, &entry("draft_roster"))
        .unwrap();
    assert_eq!(
        store.read_debug(&draft.id).unwrap(),
        vec![entry("draft_seed"), entry("draft_roster")]
    );

    store.clear_debug(&draft.id).unwrap();
    store
        .append_debug(&draft.id, &entry("hidden_facts 1"))
        .unwrap();
    assert_eq!(
        store.read_debug(&draft.id).unwrap(),
        vec![entry("hidden_facts 1")]
    );

    store.delete(&draft.id).unwrap();
    assert_eq!(dir.files(), Vec::<String>::new());
    assert_eq!(store.list().unwrap(), vec![]);
    assert!(store.load(&draft.id).is_err());
}

#[test]
fn failed_build_keeps_the_approved_draft() {
    let dir = TempDir::new("failed");
    let store = DraftStore::open(&dir.0).unwrap();
    let mut draft = valid_draft();
    approve(&mut draft).unwrap();
    store.save(&draft).unwrap();

    draft.last_failure = Some(BuildFailure {
        stage: Stage::WorldEvents,
        check: BuildCheck::Cancelled,
    });
    store.save(&draft).unwrap();

    let loaded = store.load(&draft.id).unwrap();
    assert_eq!(loaded.state, DraftState::Approved);
    assert_eq!(loaded.last_failure, draft.last_failure);
    assert_eq!(gaps(&loaded), vec![]);
}

#[test]
fn build_types_use_snake_case_on_the_wire() {
    let failure = BuildFailure {
        stage: Stage::DraftKnowledge,
        check: BuildCheck::NotApproved,
    };
    assert_eq!(
        serde_json::to_value(failure).unwrap(),
        serde_json::json!({"stage": "draft_knowledge", "check": "not_approved"})
    );
    assert_eq!(
        serde_json::to_value(gap(GapKind::SeedBlockTooLong, "seed")).unwrap(),
        serde_json::json!({"kind": "seed_block_too_long", "at": "seed"})
    );
    assert_eq!(
        serde_json::to_value(gap(GapKind::SlotConflict, "knowledge[1]")).unwrap(),
        serde_json::json!({"kind": "slot_conflict", "at": "knowledge[1]"})
    );
    assert_eq!(
        serde_json::to_value(DraftState::Approved).unwrap(),
        serde_json::json!("approved")
    );
}
