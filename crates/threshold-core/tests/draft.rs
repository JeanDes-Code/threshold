//! The Seed Draft: its gaps, approval, and the draft store (data shape 3.5).

use std::{fs, path::PathBuf};

use threshold_core::{
    build::{
        AgeLine, AgeMark, AgeRows, AgeSource, BuildCheck, BuildFailure, DebugEntry, DraftLine,
        DraftState, DraftStore, EntryRef, Gap, GapKind, KnownFact, ReviewError, SeedDraft, Stage,
        age_lines, apply_edit, approve, confirm_age, confirmed_ages, gaps, rename,
    },
    context::ADVENTURE_BLOCK_MAX_CHARS,
    domain::{
        AdventureSeed, ConfirmedAges, EntityKind, OtherName, PcSeed, SeedEntity, SeedFact,
        StartSeed, WorldSetup,
    },
    mature::{Blocked, clearance},
    setup::create_campaign_file,
    store::{Store, WorldView},
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
        let _ = confirm_age(draft, &line.entry());
    }
}

/// The entry whose saved main name is `name`: the PC, or the first roster
/// row with that exact name.
fn entry(draft: &SeedDraft, name: &str) -> EntryRef {
    if draft.seed.pc.name == name {
        return EntryRef::pc(name);
    }
    let i = draft
        .seed
        .entities
        .iter()
        .position(|e| e.name == name)
        .unwrap_or_else(|| panic!("no entry {name}"));
    EntryRef::entity(i, name, draft.seed.entities[i].kind)
}

/// The Player confirms the age of the entry called `name`.
fn confirm(draft: &mut SeedDraft, name: &str) -> Result<(), ReviewError> {
    let entry = entry(draft, name);
    confirm_age(draft, &entry)
}

/// The Player renames the entry called `from`.
fn rename_named(draft: &mut SeedDraft, from: &str, to: &str) -> Result<(), ReviewError> {
    let entry = entry(draft, from);
    rename(draft, &entry, to)
}

/// A mark the draft stage made, not yet confirmed.
fn drafted(name: &str, kind: EntityKind, age: u16, stated: bool) -> AgeMark {
    AgeMark {
        name: name.into(),
        kind,
        drafted: Some(age),
        stated,
        confirmed: None,
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
            hour: None,
        },
        open_details: vec![],
        detail: Default::default(),
    };
    let mut draft = SeedDraft::new("A ferry, a courier, a late night.".into(), seed);
    draft.knowledge = vec![
        known("Jean", "job", "ferryman"),
        known("Jay", "debt", "ten"),
    ];
    draft.age_marks = AgeRows {
        pc: Some(drafted("Wren", EntityKind::Pc, 29, true)),
        entities: vec![Some(drafted("Jean", EntityKind::Npc, 40, false)), None],
    };
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
    // Each row has its own age mark, so both ages can be confirmed.
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
    let ages = draft.age_marks.clone();
    apply_edit(&mut draft, seed, knowledge);
    store.save(&draft).unwrap();

    let loaded = store.load(&draft.id).unwrap();
    assert_eq!(loaded.state, DraftState::Draft);
    assert_eq!(loaded.seed.premise, "A ferry crossing at dawn");
    // An edit that leaves the ages alone leaves their confirmations alone.
    assert_eq!(loaded.age_marks, ages);
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

    assert_eq!(
        rename_named(&mut draft, "Mira Holt", " Mira Vance "),
        Ok(())
    );
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
    let mark = draft.age_marks.entities[2].as_ref().unwrap();
    assert_eq!(mark.name, "Mira Vance");
    // Her age stays confirmed, and no gap is new.
    assert!(line(&draft, "Mira Vance").confirmed);
    assert_eq!(gaps(&draft), before);

    // A place: the start follows, and so do the texts.
    assert_eq!(rename_named(&mut draft, "Old Mill", "Grey Mill"), Ok(()));
    assert_eq!(draft.seed.entities[3].name, "Grey Mill");
    assert_eq!(draft.seed.start.place, "Grey Mill");
    assert_eq!(
        draft.knowledge[2].text,
        "Mira Vance grinds grain at the Grey Mill."
    );
    assert_eq!(gaps(&draft), before);

    // The PC too.
    assert_eq!(rename_named(&mut draft, "Wren", "Wren Ashby"), Ok(()));
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
    rename_named(&mut draft, "Jean", "Jean Marsh").unwrap();
    assert_eq!(draft.state, DraftState::Draft);
    assert_eq!(draft.last_failure, None);
}

#[test]
fn rename_refuses_a_name_in_use_and_changes_nothing() {
    let draft = mira_draft();
    let refused = |from: EntryRef, to: &str| {
        let mut changed = draft.clone();
        let result = rename(&mut changed, &from, to);
        assert_eq!(changed, draft, "{from:?} -> {to}");
        result
    };
    let named = |name: &str| entry(&draft, name);
    // Another main name, in any spelling; another entry's other name; the
    // PC's name; the reserved "world".
    let in_use = Err(ReviewError::NameInUse);
    assert_eq!(refused(named("Mira Holt"), "JEAN"), in_use);
    assert_eq!(refused(named("Mira Holt"), "jay"), in_use);
    assert_eq!(refused(named("Jean"), "Wren"), in_use);
    assert_eq!(refused(named("Old Mill"), "World"), in_use);
    // An empty name.
    assert_eq!(refused(named("Mira Holt"), "  "), Err(ReviewError::Empty));
    // An entry its row does not hold: no name, another name, another
    // spelling, another kind, another row.
    let unknown = Err(ReviewError::UnknownName);
    let npc = EntityKind::Npc;
    for from in [
        EntryRef::entity(0, "Nobody", npc),
        EntryRef::entity(0, "", npc),
        EntryRef::pc(""),
        EntryRef::entity(2, "mira holt", npc),
        EntryRef::entity(2, "Mira Holt", EntityKind::Place),
        EntryRef::entity(0, "Mira Holt", npc),
        EntryRef::entity(9, "Mira Holt", npc),
    ] {
        assert_eq!(refused(from, "Somebody"), unknown);
    }
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
    rename_named(&mut draft, "Rose", "Iris").unwrap();
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
    assert_eq!(rename_named(&mut draft, "chloé", "Chloé"), Ok(()));
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
    let marks = draft.age_marks.clone();
    // No input of `apply_edit` writes a confirmation.
    let (seed, knowledge) = (draft.seed.clone(), draft.knowledge.clone());
    apply_edit(&mut draft, seed, knowledge);
    assert_eq!(draft.age_marks, marks);

    let unconfirmed = |d: &SeedDraft| -> Vec<String> {
        gaps(d)
            .into_iter()
            .filter(|g| g.kind == GapKind::AgeUnconfirmed)
            .map(|g| g.at)
            .collect()
    };
    assert_eq!(unconfirmed(&draft), ["pc.age", "entities[0].age"]);
    assert!(approve(&mut draft).is_err());
    assert_eq!(confirm(&mut draft, "Wren"), Ok(()));
    assert_eq!(unconfirmed(&draft), ["entities[0].age"]);
    assert!(approve(&mut draft).is_err());
    assert_eq!(draft.state, DraftState::Draft);
    // The entry must match its row exactly: another spelling is refused.
    let npc = EntityKind::Npc;
    assert_eq!(
        confirm_age(&mut draft, &EntryRef::entity(0, "JEAN", npc)),
        Err(ReviewError::UnknownName)
    );
    assert_eq!(confirm(&mut draft, "Jean"), Ok(()));
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
    confirm(&mut draft, "Jean").unwrap();
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
    let npc = EntityKind::Npc;
    for entry in [
        EntryRef::entity(0, "Nobody", npc),
        EntryRef::entity(1, "The landing stage", EntityKind::Place),
        EntryRef::pc("Jay"),
        EntryRef::pc(""),
        EntryRef::entity(1, "Jean", npc),
        EntryRef::entity(0, "Jean", EntityKind::Place),
        EntryRef::entity(0, "Jean", EntityKind::Pc),
    ] {
        assert_eq!(
            confirm_age(&mut draft, &entry),
            Err(ReviewError::UnknownName),
            "{entry:?}"
        );
    }
    // A character with no age has no age to confirm.
    assert_eq!(confirm(&mut draft, "Kit"), Err(ReviewError::Empty));
    assert_eq!(draft, before);
}

#[test]
fn old_draft_file_without_ages_loads_unconfirmed() {
    let dir = TempDir::new("old");
    let store = DraftStore::open(&dir.0).unwrap();
    let draft = valid_draft();
    let mut json = serde_json::to_value(&draft).unwrap();
    let fields = json.as_object_mut().unwrap();
    fields.remove("age_marks");
    fields.remove("cut");
    // The marks of the first version were keyed by name. They are ignored.
    fields.insert(
        "ages".into(),
        serde_json::json!({"wren": {"drafted": 29, "stated": true, "confirmed": 29}}),
    );
    fields.insert("ages_to_confirm".into(), serde_json::json!([]));
    fields.insert("ages_stated".into(), serde_json::json!(["Wren"]));
    fs::create_dir_all(&dir.0).unwrap();
    fs::write(
        dir.0.join(format!("{}.json", draft.id)),
        serde_json::to_vec(&json).unwrap(),
    )
    .unwrap();

    let loaded = store.load(&draft.id).unwrap();
    assert_eq!(loaded.age_marks, AgeRows::default());
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

// ---- Review: an age mark follows its exact entry ----------------------------

/// Saves `seed` as the Player's edit, with the knowledge unchanged.
fn save(draft: &mut SeedDraft, seed: AdventureSeed) {
    let knowledge = draft.knowledge.clone();
    apply_edit(draft, seed, knowledge);
}

fn confirmed(draft: &SeedDraft, name: &str) -> bool {
    line(draft, name).confirmed
}

#[test]
fn an_accented_twin_does_not_take_a_confirmation() {
    // "jéan" is added beside the confirmed "Jean", then "Jean" is deleted.
    let mut draft = valid_draft();
    assert!(confirmed(&draft, "Jean"));
    let mut seed = draft.seed.clone();
    seed.entities
        .push(entity("jéan", EntityKind::Npc, Some(40)));
    save(&mut draft, seed.clone());
    assert!(!confirmed(&draft, "jéan"));
    seed.entities.remove(0);
    save(&mut draft, seed);
    assert!(!confirmed(&draft, "jéan"));
    assert!(gaps(&draft).contains(&gap(GapKind::AgeUnconfirmed, "entities[1].age")));

    // The same in one row: "Jean" spelt "jéan" is a new entry.
    let mut draft = valid_draft();
    let mut seed = draft.seed.clone();
    seed.entities[0].name = "jéan".into();
    save(&mut draft, seed);
    assert!(!confirmed(&draft, "jéan"));
}

#[test]
fn a_name_deleted_and_added_again_in_one_save_is_unconfirmed() {
    let mut draft = valid_draft();
    draft
        .seed
        .entities
        .push(entity("Mara", EntityKind::Npc, Some(30)));
    confirm_all(&mut draft);
    assert!(confirmed(&draft, "Jean") && confirmed(&draft, "Mara"));

    // Jean leaves row 0 and comes back at the end, with the same age.
    let mut seed = draft.seed.clone();
    let jean = seed.entities.remove(0);
    seed.entities.push(jean);
    save(&mut draft, seed);
    assert!(!confirmed(&draft, "Jean"));
    // Mara moved too, in the same save: the safe side, a new confirmation.
    assert!(!confirmed(&draft, "Mara"));
}

#[test]
fn removing_one_row_keeps_the_marks_of_the_others() {
    let mut draft = valid_draft();
    draft
        .seed
        .entities
        .push(entity("Mara", EntityKind::Npc, Some(30)));
    confirm_all(&mut draft);
    let mut seed = draft.seed.clone();
    seed.entities.remove(0);
    save(&mut draft, seed);
    assert!(confirmed(&draft, "Mara"));
    assert_eq!(draft.age_marks.entities.len(), 2);
    assert_eq!(
        draft.age_marks.entities[1]
            .as_ref()
            .map(|m| m.name.as_str()),
        Some("Mara")
    );
}

#[test]
fn an_entry_that_becomes_a_place_and_a_character_again_is_unconfirmed() {
    let mut draft = valid_draft();
    let mut seed = draft.seed.clone();
    seed.entities[0].kind = EntityKind::Place;
    seed.entities[0].age = None;
    save(&mut draft, seed.clone());
    // The prune keeps no mark for an entry that became a place.
    assert_eq!(draft.age_marks.entities[0], None);

    seed.entities[0].kind = EntityKind::Npc;
    seed.entities[0].age = Some(40);
    save(&mut draft, seed);
    assert!(!confirmed(&draft, "Jean"));
    assert_eq!(line(&draft, "Jean").source, AgeSource::Edited);
}

#[test]
fn an_age_set_back_to_the_confirmed_value_is_confirmed_again() {
    let mut draft = valid_draft();
    let mut seed = draft.seed.clone();
    seed.entities[0].age = Some(41);
    save(&mut draft, seed.clone());
    assert!(!confirmed(&draft, "Jean"));
    seed.entities[0].age = Some(40);
    save(&mut draft, seed);
    assert!(confirmed(&draft, "Jean"));
    assert_eq!(gaps(&draft), vec![]);
}

#[test]
fn a_mark_counts_only_for_its_exact_name_and_kind() {
    // A mark in the slot of a row whose entry is another one does not count.
    let mut draft = valid_draft();
    let mark = draft.age_marks.entities[0].clone().unwrap();
    assert_eq!(mark.confirmed, Some(40));
    for (name, kind) in [
        ("JEAN", EntityKind::Npc),
        ("Jean ", EntityKind::Npc),
        ("Jean", EntityKind::Pc),
    ] {
        draft.seed.entities[0].name = name.into();
        draft.seed.entities[0].kind = kind;
        assert!(
            gaps(&draft).contains(&gap(GapKind::AgeUnconfirmed, "entities[0].age")),
            "{name} {kind:?}"
        );
    }
}

#[test]
fn rename_moves_only_the_mark_of_its_own_row() {
    // A place and a character share the name "Ash".
    let mut draft = valid_draft();
    let seed = &mut draft.seed;
    seed.entities.push(entity("Ash", EntityKind::Place, None));
    seed.entities.push(entity("Ash", EntityKind::Npc, Some(30)));
    confirm_all(&mut draft);
    assert!(confirmed(&draft, "Ash"));

    let place = EntryRef::entity(2, "Ash", EntityKind::Place);
    assert_eq!(rename(&mut draft, &place, "Ash Hill"), Ok(()));
    assert_eq!(draft.seed.entities[2].name, "Ash Hill");
    assert_eq!(draft.seed.entities[3].name, "Ash");
    assert_eq!(draft.age_marks.entities[2], None);
    assert_eq!(
        draft.age_marks.entities[3]
            .as_ref()
            .map(|m| m.name.as_str()),
        Some("Ash")
    );
    assert!(confirmed(&draft, "Ash"));

    // The character: its own mark follows it.
    let npc = EntryRef::entity(3, "Ash", EntityKind::Npc);
    assert_eq!(rename(&mut draft, &npc, "Ash Reed"), Ok(()));
    assert!(confirmed(&draft, "Ash Reed"));
    assert_eq!(draft.seed.entities[2].name, "Ash Hill");
}

/// B8 (QA, 2026-11-10): Pim had age 12 on the entry, "11" on the known age
/// fact, and "twelve" in its text. Every operation that sets an age now
/// rewrites the known `age` facts of that entry in the same step.
fn age_fact<'a>(draft: &'a SeedDraft, subject: &str) -> &'a KnownFact {
    draft
        .knowledge
        .iter()
        .find(|f| f.subject == subject && f.key.eq_ignore_ascii_case("age"))
        .unwrap_or_else(|| panic!("no age fact of {subject}: {:?}", draft.knowledge))
}

fn with_stale_age_facts() -> SeedDraft {
    let mut draft = unconfirmed_draft();
    draft.knowledge.push(KnownFact {
        subject: "Jean".into(),
        key: "age".into(),
        value: "39".into(),
        text: "Jean is forty.".into(),
        open: false,
    });
    draft.knowledge.push(KnownFact {
        subject: "Jay".into(),
        key: "Age".into(),
        value: "28".into(),
        text: "Jay is twenty-eight.".into(),
        open: false,
    });
    draft
}

#[test]
fn confirming_an_age_rewrites_the_known_age_fact() {
    let mut draft = with_stale_age_facts();
    confirm(&mut draft, "Jean").unwrap();
    let fact = age_fact(&draft, "Jean");
    assert_eq!(
        (fact.value.as_str(), fact.text.as_str()),
        ("40", "Jean is 40 years old.")
    );
    // The PC's fact under an other name follows the PC's age.
    confirm(&mut draft, "Wren").unwrap();
    let fact = age_fact(&draft, "Jay");
    assert_eq!(
        (fact.value.as_str(), fact.text.as_str()),
        ("29", "Wren is 29 years old.")
    );
    // Other facts do not change.
    assert!(
        draft
            .knowledge
            .iter()
            .any(|f| f.key == "job" && f.text == "Jean job ferryman.")
    );
}

#[test]
fn editing_an_age_rewrites_the_known_age_fact() {
    let mut draft = with_stale_age_facts();
    let mut seed = draft.seed.clone();
    seed.entities[0].age = Some(41);
    let knowledge = draft.knowledge.clone();
    apply_edit(&mut draft, seed, knowledge);
    let fact = age_fact(&draft, "Jean");
    assert_eq!(
        (fact.value.as_str(), fact.text.as_str()),
        ("41", "Jean is 41 years old.")
    );
}

#[test]
fn renaming_an_entry_rewrites_its_known_age_fact() {
    let mut draft = with_stale_age_facts();
    rename_named(&mut draft, "Jean", "Joan").unwrap();
    let fact = age_fact(&draft, "Joan");
    assert_eq!(
        (fact.value.as_str(), fact.text.as_str()),
        ("40", "Joan is 40 years old.")
    );
}

/// C11 (review, 2026-11-10): an edit rewrites the known age facts of a row
/// only when that row's age changed, or when its age is confirmed. An edit
/// of another field never puts an unconfirmed age over an age fact that
/// the Source Text states.
#[test]
fn an_edit_rewrites_only_the_age_facts_of_a_changed_row() {
    let mut draft = with_stale_age_facts();
    // Jean's entry says 40, unconfirmed; the text's fact says 39.
    let mut seed = draft.seed.clone();
    seed.premise = "A ferry crossing in the fog".into();
    let knowledge = draft.knowledge.clone();
    apply_edit(&mut draft, seed, knowledge);
    let fact = age_fact(&draft, "Jean");
    assert_eq!(
        (fact.value.as_str(), fact.text.as_str()),
        ("39", "Jean is forty.")
    );
    // The PC's age is unchanged too: its fact stays.
    let fact = age_fact(&draft, "Jay");
    assert_eq!(fact.value, "28");
    // Once the age is confirmed, a stale fact sent back by an edit follows it.
    confirm(&mut draft, "Jean").unwrap();
    let mut knowledge = draft.knowledge.clone();
    for f in knowledge
        .iter_mut()
        .filter(|f| f.subject == "Jean" && f.key == "age")
    {
        f.value = "39".into();
        f.text = "Jean is forty.".into();
    }
    let seed = draft.seed.clone();
    apply_edit(&mut draft, seed, knowledge);
    let fact = age_fact(&draft, "Jean");
    assert_eq!(
        (fact.value.as_str(), fact.text.as_str()),
        ("40", "Jean is 40 years old.")
    );
}

// --- Mature content module, Task 2: confirmed ages reach the Campaign ------

/// Builds a Campaign file from `draft` with no World Build content and the
/// confirmed ages `ages`, and returns its world.
fn built(draft: &SeedDraft, ages: Option<&ConfirmedAges>, name: &str) -> WorldView {
    let dir = TempDir::new(name);
    fs::create_dir_all(&dir.0).unwrap();
    let path = dir.0.join("campaign.sqlite");
    create_campaign_file(&path, &draft.seed, &WorldSetup::default(), 7, ages).unwrap();
    let store = Store::open_existing(&path).unwrap();
    store.snapshot(store.campaigns().unwrap()[0]).unwrap()
}

/// Each character's main name and its Age Confirmation in the Campaign.
fn marks(view: &WorldView) -> Vec<(String, bool)> {
    view.entities
        .iter()
        .filter(|e| matches!(e.kind, EntityKind::Pc | EntityKind::Npc))
        .map(|e| (e.name.clone(), e.age_confirmed))
        .collect()
}

/// Path 1: a draft with every age confirmed marks every PC and NPC of the
/// Campaign it builds. A place has no mark. With no `ConfirmedAges`, as on
/// the fixture path, nothing is marked.
#[test]
fn a_built_campaign_marks_only_confirmed_ages() {
    let draft = valid_draft();
    let ages = confirmed_ages(&draft).expect("no gap");
    assert!(ages.holds(EntityKind::Pc, "Wren", 29));
    assert!(ages.holds(EntityKind::Npc, "Jean", 40));
    assert!(!ages.holds(EntityKind::Npc, "jean", 40), "exact main name");
    assert!(!ages.holds(EntityKind::Pc, "Jean", 40), "same kind");

    let view = built(&draft, Some(&ages), "marked");
    assert_eq!(
        marks(&view),
        [("Wren".to_owned(), true), ("Jean".to_owned(), true)]
    );
    assert!(view.entities.iter().all(|e| match e.kind {
        EntityKind::Pc | EntityKind::Npc => true,
        _ => !e.age_confirmed,
    }));

    let view = built(&draft, None, "unmarked");
    assert_eq!(
        marks(&view),
        [("Wren".to_owned(), false), ("Jean".to_owned(), false)]
    );
}

/// Path 2: an age edited after its confirmation is not confirmed; once the
/// Player confirms it again, the mark holds the new age only.
#[test]
fn an_edited_age_is_unconfirmed_in_the_campaign() {
    let mut draft = valid_draft();
    let mut seed = draft.seed.clone();
    seed.entities[0].age = Some(41);
    let knowledge = draft.knowledge.clone();
    apply_edit(&mut draft, seed, knowledge);
    let gaps = confirmed_ages(&draft).expect_err("Jean's new age waits");
    assert_eq!(gaps, [gap(GapKind::AgeUnconfirmed, "entities[0].age")]);

    confirm(&mut draft, "Jean").unwrap();
    let ages = confirmed_ages(&draft).unwrap();
    assert!(ages.holds(EntityKind::Npc, "Jean", 41));
    assert!(!ages.holds(EntityKind::Npc, "Jean", 40));
    let view = built(&draft, Some(&ages), "edited");
    let jean = view.entities.iter().find(|e| e.name == "Jean").unwrap();
    assert_eq!((jean.age, jean.age_confirmed), (Some(41), true));
}

/// Path 6: the state a draft file says counts for nothing. The gaps are
/// computed again.
#[test]
fn an_approved_draft_with_a_gap_gives_no_confirmed_ages() {
    let mut draft = unconfirmed_draft();
    draft.state = DraftState::Approved;
    let gaps = confirmed_ages(&draft).expect_err("ages wait");
    assert_eq!(
        gaps,
        [
            gap(GapKind::AgeUnconfirmed, "pc.age"),
            gap(GapKind::AgeUnconfirmed, "entities[0].age"),
        ]
    );
    let mut built_state = valid_draft();
    built_state.state = DraftState::Built;
    built_state.seed.pc.age = 30;
    assert!(confirmed_ages(&built_state).is_err());
}

/// Path 3 and the review note of the campaign-from-document shape: clearing
/// or lowering `min_age` removes the `AgeBelowMin` gap of a minor, and the
/// Player may confirm that age. The Campaign then holds a confirmed age of
/// 16, and the adults-only rule still blocks that character: its floor is
/// `ADULT_AGE`, never `min_age`.
#[test]
fn a_cleared_min_age_never_clears_a_minor() {
    for min_age in [None, Some(10), Some(16)] {
        let mut draft = valid_draft();
        let mut seed = draft.seed.clone();
        seed.min_age = min_age;
        seed.entities.push(entity("Tam", EntityKind::Npc, Some(16)));
        let knowledge = draft.knowledge.clone();
        apply_edit(&mut draft, seed, knowledge);
        confirm_all(&mut draft);
        let found = gaps(&draft);
        assert!(found.is_empty(), "min_age {min_age:?}: {found:?}");
        let ages = confirmed_ages(&draft).unwrap();
        assert!(ages.holds(EntityKind::Npc, "Tam", 16));

        let view = built(&draft, Some(&ages), "minor");
        let tam = view.entities.iter().find(|e| e.name == "Tam").unwrap();
        assert_eq!((tam.age, tam.age_confirmed), (Some(16), true));
        assert_eq!(
            clearance(&view, &[tam.id]).unwrap_err(),
            Blocked { who: vec![tam.id] },
            "min_age {min_age:?}"
        );
        // Jean is cleared. Tam has no location, so the gate counts Tam as
        // here (unknown is not cleared, review M1), and Tam alone blocks.
        let jean = view.entities.iter().find(|e| e.name == "Jean").unwrap();
        assert_eq!(
            clearance(&view, &[jean.id]).unwrap_err(),
            Blocked { who: vec![tam.id] },
            "min_age {min_age:?}"
        );
    }

    // A `min_age` above the minor's age keeps the gap: no confirmed ages.
    let mut draft = valid_draft();
    let mut seed = draft.seed.clone();
    seed.entities.push(entity("Tam", EntityKind::Npc, Some(16)));
    let knowledge = draft.knowledge.clone();
    apply_edit(&mut draft, seed, knowledge);
    confirm_all(&mut draft);
    let gaps = confirmed_ages(&draft).expect_err("below min_age");
    assert_eq!(gaps, [gap(GapKind::AgeBelowMin, "entities[2].age")]);
}
