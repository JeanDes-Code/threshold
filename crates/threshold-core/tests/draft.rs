//! The Seed Draft: its gaps, approval, and the draft store (data shape 3.5).

use std::{fs, path::PathBuf};

use threshold_core::{
    build::{
        AgeLine, AgeMark, AgeSource, BuildCheck, BuildFailure, DebugEntry, DraftLine, DraftState,
        DraftStore, Gap, GapKind, KnownFact, ReviewError, SeedDraft, Stage, age_lines, apply_edit,
        approve, confirm_age, gaps, rename,
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

/// Confirms the current age of every character, as the Player would one by
/// one. A character with no age stays as it is.
fn confirm_all(draft: &mut SeedDraft) {
    for line in age_lines(draft) {
        let _ = confirm_age(draft, &line.name);
    }
}

/// A draft with no gap: entities[0] is Jean (an NPC), entities[1] the
/// landing stage (a place). Every age is confirmed.
fn valid_draft() -> SeedDraft {
    let mut draft = unconfirmed_draft();
    confirm_all(&mut draft);
    draft
}

/// `valid_draft` before the Player confirmed any age: Wren's age is from
/// the text, Jean's is proposed.
fn unconfirmed_draft() -> SeedDraft {
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
    for (name, age, stated) in [("Wren", 29, true), ("Jean", 40, false)] {
        let mark = AgeMark {
            drafted: Some(age),
            stated,
            confirmed: None,
        };
        draft.ages.insert(name.to_lowercase(), mark);
    }
    draft
}

fn gap(kind: GapKind, at: &str) -> Gap {
    Gap {
        kind,
        at: at.into(),
    }
}

/// The gaps of `valid_draft` after `edit`, once the Player confirmed every
/// age again: each gap kind but `AgeUnconfirmed` shows alone.
fn gaps_after(edit: impl FnOnce(&mut SeedDraft)) -> Vec<Gap> {
    let mut draft = valid_draft();
    edit(&mut draft);
    confirm_all(&mut draft);
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

    // AgeUnconfirmed: an age the Player has not confirmed through the core,
    // or one that changed after the confirmation.
    assert_eq!(
        gaps(&unconfirmed_draft()),
        vec![
            gap(GapKind::AgeUnconfirmed, "pc.age"),
            gap(GapKind::AgeUnconfirmed, "entities[0].age"),
        ]
    );
    let mut draft = valid_draft();
    draft.seed.entities[0].age = Some(41);
    assert_eq!(
        gaps(&draft),
        vec![gap(GapKind::AgeUnconfirmed, "entities[0].age")]
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
    // Two characters with one normal name share one age mark, so the second
    // age cannot be confirmed until one of them is renamed.
    assert_eq!(
        gaps_after(|d| d
            .seed
            .entities
            .push(entity("JÉAN", EntityKind::Npc, Some(30)))),
        vec![
            gap(GapKind::DuplicateName, "entities[2].name"),
            gap(GapKind::AgeUnconfirmed, "entities[2].age"),
        ]
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
    let ages = draft.ages.clone();
    apply_edit(&mut draft, seed, knowledge);
    store.save(&draft).unwrap();

    let loaded = store.load(&draft.id).unwrap();
    assert_eq!(loaded.state, DraftState::Draft);
    assert_eq!(loaded.seed.premise, "A ferry crossing at dawn");
    // An edit that leaves the ages alone leaves their confirmations alone.
    assert_eq!(loaded.ages, ages);
    assert_eq!(gaps(&loaded), vec![]);
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

// ---- Review: renames, ages, old files ---------------------------------------

fn seed_fact(subject: &str, key: &str, value: &str) -> SeedFact {
    SeedFact {
        subject: subject.into(),
        key: key.into(),
        value: value.into(),
        text: format!("{subject} {key} {value}."),
    }
}

/// A draft that names "Mira Holt" (an NPC) everywhere a name can be, and
/// "Old Mill" (a place) as the start. It has one gap: an empty premise.
fn mira_draft() -> SeedDraft {
    let mut draft = valid_draft();
    let seed = &mut draft.seed;
    seed.premise = String::new();
    seed.pc.other_names = vec![OtherName {
        name: "Jay".into(),
        known_to: vec!["Wren".into(), "Mira Holt".into()],
    }];
    seed.entities
        .push(entity("Mira Holt", EntityKind::Npc, Some(35)));
    seed.entities
        .push(entity("Old Mill", EntityKind::Place, None));
    seed.start.place = "Old Mill".into();
    seed.start.present = vec!["Jean".into(), "mira holt".into()];
    seed.start.situation = "Mira Holt waits at the Old Mill; Mira Holt's dog sleeps.".into();
    seed.decided.push(seed_fact("Mira Holt", "trade", "miller"));
    seed.proposed
        .push(seed_fact("Mira Holt", "plan", "sell the mill"));
    draft.knowledge.push(KnownFact {
        text: "Mira Holt grinds grain at the Old Mill.".into(),
        ..known("Mira Holt", "skill", "grinding")
    });
    confirm_all(&mut draft);
    draft
}

fn line<'a>(draft: &SeedDraft, name: &str) -> AgeLine {
    age_lines(draft)
        .into_iter()
        .find(|l| l.name == name)
        .unwrap_or_else(|| panic!("no age line for {name}"))
}

#[test]
fn rename_updates_every_reference_and_adds_no_gap() {
    let mut draft = mira_draft();
    let before = gaps(&draft);
    assert_eq!(before, vec![gap(GapKind::Missing, "premise")]);
    assert!(line(&draft, "Mira Holt").confirmed);

    assert_eq!(rename(&mut draft, "Mira Holt", " Mira Vance "), Ok(()));
    let seed = &draft.seed;
    assert_eq!(seed.entities[2].name, "Mira Vance");
    assert_eq!(seed.pc.other_names[0].known_to, ["Wren", "Mira Vance"]);
    assert_eq!(seed.start.present, ["Jean", "Mira Vance"]);
    assert_eq!(seed.decided[0].subject, "Mira Vance");
    assert_eq!(seed.proposed[0].subject, "Mira Vance");
    assert_eq!(draft.knowledge[2].subject, "Mira Vance");
    assert_eq!(
        draft.knowledge[2].text,
        "Mira Vance grinds grain at the Old Mill."
    );
    assert_eq!(
        seed.start.situation,
        "Mira Vance waits at the Old Mill; Mira Vance's dog sleeps."
    );
    assert!(draft.ages.contains_key("mira vance"));
    assert!(!draft.ages.contains_key("mira holt"));
    // Her age stays confirmed, and no gap is new.
    assert!(line(&draft, "Mira Vance").confirmed);
    assert_eq!(gaps(&draft), before);

    // A place: the start follows, and so do the texts.
    assert_eq!(rename(&mut draft, "Old Mill", "Grey Mill"), Ok(()));
    assert_eq!(draft.seed.entities[3].name, "Grey Mill");
    assert_eq!(draft.seed.start.place, "Grey Mill");
    assert_eq!(
        draft.knowledge[2].text,
        "Mira Vance grinds grain at the Grey Mill."
    );
    assert_eq!(gaps(&draft), before);

    // The PC too.
    assert_eq!(rename(&mut draft, "Wren", "Wren Ashby"), Ok(()));
    assert_eq!(draft.seed.pc.name, "Wren Ashby");
    assert_eq!(draft.seed.pc.other_names[0].known_to[0], "Wren Ashby");
    assert!(line(&draft, "Wren Ashby").confirmed);
    assert_eq!(gaps(&draft), before);
}

#[test]
fn rename_returns_an_approved_draft_to_draft() {
    let mut draft = valid_draft();
    approve(&mut draft).unwrap();
    draft.last_failure = Some(BuildFailure {
        stage: Stage::NpcState,
        check: BuildCheck::Agency,
    });
    rename(&mut draft, "Jean", "Jean Marsh").unwrap();
    assert_eq!(draft.state, DraftState::Draft);
    assert_eq!(draft.last_failure, None);
}

#[test]
fn rename_refuses_a_name_in_use_and_changes_nothing() {
    let draft = mira_draft();
    let refused = |from: &str, to: &str| {
        let mut changed = draft.clone();
        let result = rename(&mut changed, from, to);
        assert_eq!(changed, draft, "{from} -> {to}");
        result
    };
    // Another main name, in any spelling; another entry's other name; the
    // PC's name; the reserved "world".
    assert_eq!(refused("Mira Holt", "JEAN"), Err(ReviewError::NameInUse));
    assert_eq!(refused("Mira Holt", "jay"), Err(ReviewError::NameInUse));
    assert_eq!(refused("Jean", "Wren"), Err(ReviewError::NameInUse));
    assert_eq!(refused("Old Mill", "World"), Err(ReviewError::NameInUse));
    // An empty name, and a name no entry has.
    assert_eq!(refused("Mira Holt", "  "), Err(ReviewError::Empty));
    assert_eq!(refused("Nobody", "Somebody"), Err(ReviewError::UnknownName));
    assert_eq!(refused("", "Somebody"), Err(ReviewError::UnknownName));
}

#[test]
fn rename_changes_whole_words_only() {
    let mut draft = valid_draft();
    draft
        .seed
        .entities
        .push(entity("Rose", EntityKind::Npc, Some(30)));
    draft.knowledge.push(KnownFact {
        text: "Rose gave Roseanne a rosewood box (Rose's own).".into(),
        ..known("Rose", "gift", "a box")
    });
    draft.seed.start.situation = "Rose, Roses and ROSE.".into();
    rename(&mut draft, "Rose", "Iris").unwrap();
    assert_eq!(
        draft.knowledge[2].text,
        "Iris gave Roseanne a rosewood box (Iris's own)."
    );
    // The same spelling only: "ROSE" stays.
    assert_eq!(draft.seed.start.situation, "Iris, Roses and ROSE.");
}

#[test]
fn rename_to_the_same_name_in_other_case_is_allowed() {
    let mut draft = valid_draft();
    draft
        .seed
        .entities
        .push(entity("chloé", EntityKind::Npc, Some(25)));
    confirm_all(&mut draft);
    assert_eq!(rename(&mut draft, "chloé", "Chloé"), Ok(()));
    assert_eq!(draft.seed.entities[2].name, "Chloé");
    assert!(line(&draft, "Chloé").confirmed);
    assert_eq!(gaps(&draft), vec![]);
}

#[test]
fn edited_age_is_labelled_edited() {
    let mut draft = unconfirmed_draft();
    assert_eq!(line(&draft, "Wren").source, AgeSource::FromText);
    assert_eq!(line(&draft, "Jean").source, AgeSource::Proposed);

    // The Player changes Jean's age and adds Mara.
    let mut seed = draft.seed.clone();
    seed.entities[0].age = Some(41);
    seed.entities
        .push(entity("Mara", EntityKind::Npc, Some(30)));
    let knowledge = draft.knowledge.clone();
    apply_edit(&mut draft, seed.clone(), knowledge.clone());
    assert_eq!(line(&draft, "Wren").source, AgeSource::FromText);
    assert_eq!(line(&draft, "Jean").source, AgeSource::Edited);
    assert_eq!(line(&draft, "Mara").source, AgeSource::Edited);

    // Back to the drafted value: untouched again.
    seed.entities[0].age = Some(40);
    apply_edit(&mut draft, seed, knowledge);
    assert_eq!(line(&draft, "Jean").source, AgeSource::Proposed);
    assert_eq!(
        serde_json::to_value(AgeSource::FromText).unwrap(),
        serde_json::json!("from_text")
    );
}

#[test]
fn approval_needs_every_age_confirmed_through_the_core() {
    let mut draft = unconfirmed_draft();
    let marks = draft.ages.clone();
    // No input of `apply_edit` writes a confirmation.
    let (seed, knowledge) = (draft.seed.clone(), draft.knowledge.clone());
    apply_edit(&mut draft, seed, knowledge);
    assert_eq!(draft.ages, marks);

    let unconfirmed = |d: &SeedDraft| -> Vec<String> {
        gaps(d)
            .into_iter()
            .filter(|g| g.kind == GapKind::AgeUnconfirmed)
            .map(|g| g.at)
            .collect()
    };
    assert_eq!(unconfirmed(&draft), ["pc.age", "entities[0].age"]);
    assert!(approve(&mut draft).is_err());
    assert_eq!(confirm_age(&mut draft, "Wren"), Ok(()));
    assert_eq!(unconfirmed(&draft), ["entities[0].age"]);
    assert!(approve(&mut draft).is_err());
    assert_eq!(draft.state, DraftState::Draft);
    // A name matches as the core matches names.
    assert_eq!(confirm_age(&mut draft, "JEAN"), Ok(()));
    assert_eq!(approve(&mut draft), Ok(()));
    assert_eq!(draft.state, DraftState::Approved);

    // An edit of a confirmed age makes it unconfirmed again.
    let mut seed = draft.seed.clone();
    seed.entities[0].age = Some(45);
    let knowledge = draft.knowledge.clone();
    apply_edit(&mut draft, seed, knowledge);
    assert_eq!(draft.state, DraftState::Draft);
    assert_eq!(unconfirmed(&draft), ["entities[0].age"]);
    assert!(!line(&draft, "Jean").confirmed);
    assert!(approve(&mut draft).is_err());
    confirm_age(&mut draft, "Jean").unwrap();
    assert_eq!(approve(&mut draft), Ok(()));
}

#[test]
fn a_name_that_leaves_the_roster_loses_its_confirmation() {
    let mut draft = valid_draft();
    let mut seed = draft.seed.clone();
    seed.entities[0].name = "Jeanne".into();
    let knowledge = draft.knowledge.clone();
    apply_edit(&mut draft, seed.clone(), knowledge.clone());
    // A name changed through `save_draft` is a new entry for the core.
    assert!(!line(&draft, "Jeanne").confirmed);
    assert_eq!(line(&draft, "Jeanne").source, AgeSource::Edited);
    // The old name back is a new entry too: its mark was dropped.
    seed.entities[0].name = "Jean".into();
    apply_edit(&mut draft, seed, knowledge);
    assert!(!line(&draft, "Jean").confirmed);
}

#[test]
fn confirm_age_refuses_an_unknown_name() {
    let mut draft = unconfirmed_draft();
    draft
        .seed
        .entities
        .push(entity("Kit", EntityKind::Npc, None));
    let before = draft.clone();
    for name in ["Nobody", "The landing stage", "Jay", ""] {
        assert_eq!(
            confirm_age(&mut draft, name),
            Err(ReviewError::UnknownName),
            "{name}"
        );
    }
    // A character with no age has no age to confirm.
    assert_eq!(confirm_age(&mut draft, "Kit"), Err(ReviewError::Empty));
    assert_eq!(draft, before);
}

#[test]
fn old_draft_file_without_ages_loads_unconfirmed() {
    let dir = TempDir::new("old");
    let store = DraftStore::open(&dir.0).unwrap();
    let draft = valid_draft();
    let mut json = serde_json::to_value(&draft).unwrap();
    let fields = json.as_object_mut().unwrap();
    fields.remove("ages");
    fields.remove("cut");
    fields.insert("ages_to_confirm".into(), serde_json::json!([]));
    fields.insert("ages_stated".into(), serde_json::json!(["Wren"]));
    fs::create_dir_all(&dir.0).unwrap();
    fs::write(
        dir.0.join(format!("{}.json", draft.id)),
        serde_json::to_vec(&json).unwrap(),
    )
    .unwrap();

    let loaded = store.load(&draft.id).unwrap();
    assert!(loaded.ages.is_empty());
    assert!(loaded.cut.is_empty());
    assert_eq!(
        gaps(&loaded),
        vec![
            gap(GapKind::AgeUnconfirmed, "pc.age"),
            gap(GapKind::AgeUnconfirmed, "entities[0].age"),
        ]
    );
    assert!(
        age_lines(&loaded)
            .iter()
            .all(|l| l.source == AgeSource::Edited)
    );
}
