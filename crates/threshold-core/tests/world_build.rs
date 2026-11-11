//! `build_world`: the four World Build stages and the checks of design
//! 6.5, replayed with `RecordedGm` (data shape 3.6, design 6.3).

use std::{fs, path::PathBuf};

use serde_json::{Value, json};
use threshold_core::{
    RecordedGm,
    build::{
        BuildCheck, BuildFailure, Control, DebugEntry, DraftState, KnownFact, PC_CENTRED_MAX_SHARE,
        Progress, SeedDraft, Stage, age_lines, approve, build_world, confirm_age,
    },
    domain::{
        AdventureSeed, EntityKind, FactEntry, OtherName, PcSeed, SeedEntity, StartSeed, WorldSetup,
    },
    setup::create_campaign_file,
    store::Store,
    world::pc_knowledge,
};

// ---- Fixture ---------------------------------------------------------------

fn entity(name: &str, kind: EntityKind, age: Option<u16>) -> SeedEntity {
    SeedEntity {
        name: name.into(),
        kind,
        age,
        other_names: vec![],
        hidden: false,
    }
}

/// An invented harbour: the PC Mira Holt (also "the Keeper"), two NPCs,
/// two places and a group.
fn seed() -> AdventureSeed {
    AdventureSeed {
        premise: "A lighthouse keeper faces the storm season.".into(),
        pc: PcSeed {
            name: "Mira Holt".into(),
            age: 31,
            other_names: vec![OtherName {
                name: "the Keeper".into(),
                known_to: vec!["Mira Holt".into(), "Tobin Reed".into()],
            }],
            concept: "A careful lighthouse keeper.".into(),
        },
        entities: vec![
            entity("Tobin Reed", EntityKind::Npc, Some(58)),
            entity("Ada Venn", EntityKind::Npc, Some(40)),
            entity("Gull Point", EntityKind::Place, None),
            entity("Tide Hall", EntityKind::Place, None),
            entity("Harbour Council", EntityKind::Faction, None),
        ],
        boundaries: vec![],
        decided: vec![],
        proposed: vec![],
        story_language: "en".into(),
        themes: vec!["duty".into()],
        gm_agreement: String::new(),
        resolution: String::new(),
        min_age: None,
        start: StartSeed {
            place: "Gull Point".into(),
            present: vec!["Tobin Reed".into()],
            situation: "Night falls on the point.".into(),
            when: "An autumn evening".into(),
            hour: None,
        },
        open_details: vec![],
        detail: Default::default(),
    }
}

/// A section of about 450 characters under a level-one heading.
fn section(title: &str, sentence: &str) -> String {
    let mut body = String::new();
    while body.chars().count() < 450 {
        body.push_str(sentence);
        body.push(' ');
    }
    format!("# {title}\n\n{body}\n\n")
}

/// Two headings, so two sections and two hidden-facts calls.
fn source() -> String {
    [
        section(
            "The harbour",
            "Mira Holt keeps the lighthouse at Gull Point.",
        ),
        section("The council", "The Harbour Council meets in the Tide Hall."),
    ]
    .concat()
}

fn known(subject: &str, key: &str, value: &str, open: bool) -> KnownFact {
    KnownFact {
        subject: subject.into(),
        key: key.into(),
        value: value.into(),
        text: format!("{subject}: {key} is {value}."),
        open,
    }
}

fn draft_of(seed: AdventureSeed, knowledge: Vec<KnownFact>) -> SeedDraft {
    let mut draft = SeedDraft::new(source(), seed);
    draft.knowledge = knowledge;
    // The Player confirms every age.
    for line in age_lines(&draft) {
        confirm_age(&mut draft, &line.entry()).unwrap();
    }
    approve(&mut draft).unwrap();
    assert_eq!(draft.state, DraftState::Approved);
    draft
}

fn draft() -> SeedDraft {
    draft_of(
        seed(),
        vec![
            known("Mira Holt", "occupation", "lighthouse keeper", false),
            known("Harbour Council", "vote", "new pier", true),
            known("Tobin Reed", "role", "harbour master", false),
        ],
    )
}

// ---- Stage outputs -----------------------------------------------------------

fn hf(subject: &str, key: &str, value: &str, open: bool, known_by: &[&str]) -> Value {
    json!({
        "subject": subject,
        "key": key,
        "value": value,
        "text": format!("{subject}: {key} is {value}."),
        "open": open,
        "known_by": known_by,
    })
}

fn hidden(facts: &[Value]) -> Value {
    json!({ "facts": facts })
}

fn npc_out(name: &str) -> Value {
    json!({
        "goals": [{"text": format!("{name} wants a quiet winter."), "against": null}],
        "secret": {
            "subject": name,
            "key": "savings",
            "value": "a tin box",
            "text": format!("{name} hides savings in a tin box."),
            "open": false
        },
        "attitudes": [],
        "voice_card": {
            "habits": format!("{name} speaks in short lines."),
            "vocabulary": "tide, rope, lamp",
            "samples": ["Mind the rope.", "The tide turns soon."]
        }
    })
}

fn ev(minutes: u64, actor: &str, effects: &[(&str, &str, &str)], seen_by: &[&str]) -> Value {
    let effects: Vec<Value> = effects
        .iter()
        .map(|(s, k, v)| json!({"subject": s, "key": k, "value": v}))
        .collect();
    json!({
        "in_minutes": minutes,
        "actor": actor,
        "text": format!("{actor} checks the harbour chains."),
        "effects": effects,
        "seen_by": seen_by,
        "public": false,
    })
}

fn events(list: &[Value]) -> Value {
    json!({ "events": list })
}

fn start(placements: &[(&str, &str)]) -> Value {
    let list: Vec<Value> = placements
        .iter()
        .map(|(npc, place)| json!({"npc": npc, "place": place}))
        .collect();
    json!({ "placements": list })
}

/// Every output of a full build of `draft()`, in call order.
fn plain() -> Vec<Value> {
    vec![
        hidden(&[]),
        hidden(&[hf("Ada Venn", "debt", "forty coins", false, &["Ada Venn"])]),
        npc_out("Tobin Reed"),
        npc_out("Ada Venn"),
        events(&[ev(60, "Tobin Reed", &[], &["Ada Venn"])]),
        start(&[("Ada Venn", "Tide Hall")]),
    ]
}

// ---- Runner ------------------------------------------------------------------

struct Run {
    result: Result<WorldSetup, BuildFailure>,
    gm: RecordedGm,
    progress: Vec<Progress>,
    log: Vec<DebugEntry>,
    /// How many outputs the test recorded.
    given: usize,
}

impl Run {
    /// The World Setup, once every recorded output was used: a trap that
    /// triggers no repair leaves its repair output unused.
    fn world(&self) -> WorldSetup {
        assert_eq!(self.gm.calls().len(), self.given, "unused outputs");
        self.result.clone().unwrap()
    }
}

fn run_raw(draft: &SeedDraft, outputs: Vec<String>, cancel_after: Option<usize>) -> Run {
    let given = outputs.len();
    let gm = RecordedGm::new(outputs, vec![]);
    let watcher = gm.clone();
    let cancelled = move || cancel_after.is_some_and(|n| watcher.calls().len() >= n);
    let mut progress = Vec::new();
    let mut log = Vec::new();
    let result = {
        let mut on_progress = |p| progress.push(p);
        let mut on_log = |e| log.push(e);
        let mut ctl = Control::new(&mut on_progress, &cancelled, &mut on_log);
        build_world(draft, &gm, &mut ctl)
    };
    Run {
        result,
        gm,
        progress,
        log,
        given,
    }
}

fn run(draft: &SeedDraft, outputs: &[Value]) -> Run {
    run_raw(draft, outputs.iter().map(Value::to_string).collect(), None)
}

/// `plain()` with the output at `at` replaced by `trap` twice: the first
/// answer and the repair answer.
fn trapped(at: usize, trap: Value) -> Vec<Value> {
    let mut outputs = plain();
    outputs[at] = trap.clone();
    outputs.insert(at + 1, trap);
    outputs
}

fn world_of(run: &Run) -> WorldSetup {
    run.world()
}

fn facts_on<'a>(world: &'a WorldSetup, subject: &str, key: &str) -> Vec<&'a FactEntry> {
    world
        .facts
        .iter()
        .filter(|f| f.subject == subject && f.key == key)
        .collect()
}

fn repair_notes(gm: &RecordedGm) -> Vec<String> {
    gm.calls()
        .iter()
        .filter_map(|c| {
            c.prompt
                .recent
                .split_once("YOUR LAST ANSWER FAILED THESE CHECKS")
                .map(|(_, notes)| notes.to_owned())
        })
        .collect()
}

/// An empty folder of its own under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("threshold-world-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ---- The build ---------------------------------------------------------------

#[test]
fn full_build_commits_through_setup() {
    let draft = draft();
    let run = run(&draft, &plain());
    let world = run.world();
    assert_eq!(run.gm.calls().len(), 6);

    // Code adds the known facts with the PC as holder, and the location of
    // the PC and of each name present at the start.
    let pc = vec!["Mira Holt".to_owned()];
    let occupation = facts_on(&world, "Mira Holt", "occupation");
    assert_eq!(occupation.len(), 1);
    assert_eq!(occupation[0].known_by, pc);
    assert!(facts_on(&world, "Harbour Council", "vote")[0].open);
    for name in ["Mira Holt", "Tobin Reed"] {
        let location = facts_on(&world, name, "location");
        assert_eq!(location.len(), 1, "{name}");
        assert_eq!(location[0].value, "Gull Point");
        assert_eq!(location[0].known_by, pc);
    }
    // A placement puts an absent NPC somewhere, held by that NPC.
    let ada = facts_on(&world, "Ada Venn", "location");
    assert_eq!(ada[0].value, "Tide Hall");
    assert_eq!(ada[0].known_by, vec!["Ada Venn".to_owned()]);
    // Each NPC has a goal, a Voice Card and a secret held by that NPC alone.
    let npcs: Vec<&str> = world.voice_cards.iter().map(|v| v.npc.as_str()).collect();
    assert_eq!(npcs, ["Tobin Reed", "Ada Venn"]);
    assert_eq!(world.goals.len(), 2);
    for name in npcs {
        let secret = facts_on(&world, name, "savings");
        assert_eq!(secret[0].known_by, vec![name.to_owned()]);
    }
    assert_eq!(world.events.len(), 1);
    assert_eq!(world.events[0].at, 60);
    assert_eq!(world.events[0].seen_by, vec!["Ada Venn".to_owned()]);
    assert!(world.obstacles.is_empty());

    let dir = TempDir::new("commit");
    let path = dir.0.join("campaign-1.sqlite");
    create_campaign_file(&path, &draft.seed, &world, 7, None).unwrap();
    let store = Store::open_existing(&path).unwrap();
    let campaign = store.campaigns().unwrap()[0];
    let view = store.snapshot(campaign).unwrap();
    let keys: Vec<String> = pc_knowledge(&view)
        .into_iter()
        .map(|b| {
            let name = &view
                .entities
                .iter()
                .find(|e| e.id == b.subject)
                .unwrap()
                .name;
            format!("{name}.{}", b.key)
        })
        .collect();
    for key in [
        "Mira Holt.occupation",
        "Harbour Council.pending_vote",
        "Tobin Reed.role",
        "Mira Holt.location",
        "Tobin Reed.location",
    ] {
        assert!(keys.contains(&key.to_owned()), "{key} in {keys:?}");
    }
    for key in [
        "Ada Venn.debt",
        "Ada Venn.location",
        "Tobin Reed.savings",
        "Ada Venn.savings",
    ] {
        assert!(!keys.contains(&key.to_owned()), "{key} in {keys:?}");
    }
    // The open item has no outcome slot.
    assert!(!keys.contains(&"Harbour Council.vote".to_owned()));
}

#[test]
fn build_adds_no_character() {
    let draft = draft();
    let world = run(&draft, &plain()).world();
    let dir = TempDir::new("roster");
    let path = dir.0.join("campaign-1.sqlite");
    create_campaign_file(&path, &draft.seed, &world, 7, None).unwrap();
    let store = Store::open_existing(&path).unwrap();
    let view = store.snapshot(store.campaigns().unwrap()[0]).unwrap();
    let mut names: Vec<&str> = view.entities.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    let mut roster: Vec<&str> = std::iter::once(draft.seed.pc.name.as_str())
        .chain(draft.seed.entities.iter().map(|e| e.name.as_str()))
        .collect();
    roster.sort_unstable();
    assert_eq!(names, roster);
}

#[test]
fn build_needs_an_approved_draft() {
    let not_approved = BuildFailure {
        stage: Stage::HiddenFacts,
        check: BuildCheck::NotApproved,
    };
    for state in [DraftState::Draft, DraftState::Built] {
        let mut draft = draft();
        draft.state = state;
        let run = run(&draft, &plain());
        assert_eq!(run.result.unwrap_err(), not_approved);
        assert!(run.gm.calls().is_empty());
    }
}

/// A draft file from before the age marks says `approved` and holds no
/// confirmation. The state alone must not let it build.
#[test]
fn approved_draft_with_a_gap_does_not_build() {
    let mut legacy = draft();
    legacy.age_marks = Default::default();
    assert_eq!(legacy.state, DraftState::Approved);
    let run = run(&legacy, &plain());
    assert_eq!(
        run.result.unwrap_err(),
        BuildFailure {
            stage: Stage::HiddenFacts,
            check: BuildCheck::NotApproved,
        }
    );
    assert!(run.gm.calls().is_empty());

    // `approve` sees the gaps and puts the draft back to `Draft`.
    let found = approve(&mut legacy).unwrap_err();
    assert!(
        found
            .iter()
            .any(|g| g.kind == threshold_core::build::GapKind::AgeUnconfirmed)
    );
    assert_eq!(legacy.state, DraftState::Draft);
}

#[test]
fn roster_without_npc_builds() {
    let mut seed = seed();
    seed.entities.retain(|e| e.kind != EntityKind::Npc);
    seed.pc.other_names[0].known_to = vec!["Mira Holt".into()];
    seed.start.present.clear();
    let draft = draft_of(seed, vec![]);
    let run = run(&draft, &[hidden(&[]), hidden(&[])]);
    let world = run.world();
    // Two hidden-facts calls; no NPC, so no NpcState, event or placement call.
    assert_eq!(run.gm.calls().len(), 2);
    assert!(world.voice_cards.is_empty() && world.goals.is_empty() && world.events.is_empty());
    assert_eq!(facts_on(&world, "Mira Holt", "location").len(), 1);
    let dir = TempDir::new("no-npc");
    create_campaign_file(&dir.0.join("c.sqlite"), &draft.seed, &world, 7, None).unwrap();
}

#[test]
fn progress_and_log_name_stages_and_counts_only() {
    let draft = draft();
    let run = run(&draft, &plain());
    run.world();
    let seen: Vec<(Stage, u32, u32)> = run
        .progress
        .iter()
        .map(|p| {
            let Progress {
                stage,
                done,
                total,
                attempt: _,
                attempts: _,
                retry_after_timeout: _,
            } = *p;
            (stage, done, total)
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            (Stage::HiddenFacts, 0, 2),
            (Stage::HiddenFacts, 1, 2),
            (Stage::HiddenFacts, 2, 2),
            (Stage::NpcState, 0, 2),
            (Stage::NpcState, 1, 2),
            (Stage::NpcState, 2, 2),
            (Stage::WorldEvents, 0, 1),
            (Stage::WorldEvents, 1, 1),
            (Stage::StartState, 0, 1),
            (Stage::StartState, 1, 1),
        ]
    );
    let steps: Vec<&str> = run.log.iter().map(|e| e.step.as_str()).collect();
    assert_eq!(
        steps,
        [
            "hidden_facts 1/2",
            "hidden_facts 2/2",
            "npc_state 1/2",
            "npc_state 2/2",
            "world_events",
            "start_state",
        ]
    );
    assert!(run.log.iter().all(|e| e.op_id == draft.id));
}

#[test]
fn cancel_stops_the_build_between_two_calls() {
    let outputs = plain().iter().map(Value::to_string).collect();
    let run = run_raw(&draft(), outputs, Some(3));
    assert_eq!(
        run.result.unwrap_err(),
        BuildFailure {
            stage: Stage::NpcState,
            check: BuildCheck::Cancelled
        }
    );
    assert_eq!(run.gm.calls().len(), 3);
}

#[test]
fn each_call_sees_what_it_needs() {
    let run = run(&draft(), &plain());
    run.world();
    let calls = run.gm.calls();
    let hidden = &calls[0].prompt.recent;
    assert!(hidden.contains("# The harbour") && !hidden.contains("# The council"));
    for name in ["Mira Holt", "the Keeper", "Tobin Reed", "Harbour Council"] {
        assert!(hidden.contains(name), "{name}: {hidden}");
    }
    // Known facts are listed so that the call does not repeat them, and an
    // open item always carries its marker.
    assert!(hidden.contains("lighthouse keeper"));
    assert!(hidden.contains("(open, not done)"));
    assert!(!hidden.contains("pending_"));
    // The second NPC call sees the Voice Card written before it.
    assert!(
        !calls[2]
            .prompt
            .recent
            .contains("Tobin Reed speaks in short lines")
    );
    assert!(
        calls[3]
            .prompt
            .recent
            .contains("Tobin Reed speaks in short lines")
    );
    // The NpcState prompt asks for a life that does not turn around the PC.
    assert!(calls[2].prompt.system.contains("own life"));
}

#[test]
fn every_world_call_sees_the_content_boundaries() {
    let mut seed = seed();
    seed.boundaries = vec!["No harm comes to an animal.".into()];
    seed.themes = vec!["duty".into(), "the sea".into()];
    seed.min_age = Some(21);
    let draft = draft_of(seed, vec![]);
    let run = run(&draft, &plain());
    run.world();
    let calls = run.gm.calls();
    let stages: Vec<&str> = run.log.iter().map(|e| e.step.as_str()).collect();
    assert_eq!(calls.len(), 6, "{stages:?}");
    for (call, step) in calls.iter().zip(&stages) {
        let recent = &call.prompt.recent;
        assert!(
            recent.contains("- No harm comes to an animal."),
            "{step}: {recent}"
        );
        assert!(
            recent.contains("Nothing you write may break a boundary."),
            "{step}: {recent}"
        );
        assert!(
            recent.contains("Every character is at least 21 years old."),
            "{step}: {recent}"
        );
        assert!(recent.contains("duty, the sea"), "{step}: {recent}");
    }
    // Hidden facts, NPC state, World Events and start state: four stages.
    for stage in ["hidden_facts", "npc_state", "world_events", "start_state"] {
        assert!(stages.iter().any(|s| s.starts_with(stage)), "{stages:?}");
    }
}

#[test]
fn the_start_state_is_the_latest_location() {
    let draft = draft_of(
        seed(),
        vec![
            known("Tobin Reed", "role", "harbour master", false),
            known("Tobin Reed", "location", "Tide Hall", false),
        ],
    );
    let world = run(&draft, &plain()).world();
    // Tobin Reed is present at the start, so the start place replaces the
    // location the PC knew from the Source Text.
    let location = facts_on(&world, "Tobin Reed", "location");
    assert_eq!(location.len(), 1);
    assert_eq!(location[0].value, "Gull Point");
    let dir = TempDir::new("slots");
    create_campaign_file(&dir.0.join("c.sqlite"), &draft.seed, &world, 7, None).unwrap();
}

#[test]
fn a_hidden_npc_present_at_the_start_gets_no_pc_holder() {
    let mut seed = seed();
    seed.entities[0].hidden = true;
    let draft = draft_of(seed, vec![]);
    let world = run(&draft, &plain()).world();
    assert!(
        facts_on(&world, "Tobin Reed", "location")[0]
            .known_by
            .is_empty()
    );
}

// ---- The checks of design 6.5 ------------------------------------------------

#[test]
fn references_trap() {
    let bad_hidden = hidden(&[
        hf("Ada Venn", "debt", "forty coins", false, &["Ada Venn"]),
        hf("Nobody Known", "role", "stranger", false, &[]),
        hf("Tobin Reed", "fear", "deep water", false, &["Someone Else"]),
    ]);
    let mut bad_npc = npc_out("Tobin Reed");
    bad_npc["attitudes"] = json!([
        {"to": "Ada Venn", "attitude": 2},
        {"to": "Nobody Known", "attitude": -1}
    ]);
    let bad_events = events(&[
        ev(60, "Tobin Reed", &[], &["Ada Venn"]),
        ev(70, "Nobody Known", &[], &[]),
        ev(80, "Ada Venn", &[], &["Someone Else"]),
        ev(90, "Ada Venn", &[("Nowhere Inn", "state", "closed")], &[]),
    ]);
    let outputs = [
        bad_hidden.clone(),
        bad_hidden,
        hidden(&[]),
        bad_npc.clone(),
        bad_npc,
        npc_out("Ada Venn"),
        bad_events.clone(),
        bad_events,
        start(&[("Ada Venn", "Nowhere Inn")]),
        start(&[("Ada Venn", "Nowhere Inn")]),
    ];
    let run = run(&draft(), &outputs);
    let world = run.world();
    assert_eq!(run.gm.calls().len(), 10);
    let notes = repair_notes(&run.gm);
    assert_eq!(notes.len(), 4);
    assert!(notes.iter().all(|n| n.contains("References")));

    assert_eq!(facts_on(&world, "Ada Venn", "debt").len(), 1);
    assert!(facts_on(&world, "Tobin Reed", "fear").is_empty());
    assert!(world.facts.iter().all(|f| f.subject != "Nobody Known"));
    let attitudes: Vec<(&str, &str)> = world
        .attitudes
        .iter()
        .map(|a| (a.from.as_str(), a.to.as_str()))
        .collect();
    assert_eq!(attitudes, [("Tobin Reed", "Ada Venn")]);
    assert_eq!(world.events.len(), 1);
    assert_eq!(world.events[0].at, 60);
    assert!(facts_on(&world, "Ada Venn", "location").is_empty());
}

#[test]
fn one_pc_trap() {
    let trap = start(&[("the Keeper", "Tide Hall"), ("Ada Venn", "Tide Hall")]);
    let run = run(&draft(), &trapped(5, trap));
    let world = run.world();
    assert!(repair_notes(&run.gm)[0].contains("OnePc"));
    let pc = facts_on(&world, "Mira Holt", "location");
    assert_eq!(pc.len(), 1);
    assert_eq!(pc[0].value, "Gull Point");
    assert_eq!(
        facts_on(&world, "Ada Venn", "location")[0].value,
        "Tide Hall"
    );
}

#[test]
fn kinds_trap() {
    let bad_hidden = hidden(&[hf(
        "Ada Venn",
        "debt",
        "forty coins",
        false,
        &["Harbour Council"],
    )]);
    let bad_events = events(&[
        ev(60, "Harbour Council", &[], &[]),
        ev(70, "Tobin Reed", &[], &["Tide Hall"]),
    ]);
    let bad_start = start(&[
        ("Harbour Council", "Tide Hall"),
        ("Ada Venn", "Harbour Council"),
    ]);
    let outputs = [
        hidden(&[]),
        bad_hidden.clone(),
        bad_hidden,
        npc_out("Tobin Reed"),
        npc_out("Ada Venn"),
        bad_events.clone(),
        bad_events,
        bad_start.clone(),
        bad_start,
    ];
    let run = run(&draft(), &outputs);
    let world = run.world();
    let notes = repair_notes(&run.gm);
    assert_eq!(notes.len(), 3);
    assert!(notes.iter().all(|n| n.contains("Kinds")));
    // Only an NPC gets a NpcState call, so only an NPC has a Voice Card or
    // a goal: none for a place or a group.
    let npc_calls = run.log.iter().filter(|e| e.step.starts_with("npc_state"));
    assert_eq!(npc_calls.count(), 2);
    for list in [
        world
            .voice_cards
            .iter()
            .map(|v| v.npc.as_str())
            .collect::<Vec<_>>(),
        world.goals.iter().map(|g| g.npc.as_str()).collect(),
    ] {
        assert_eq!(list, ["Tobin Reed", "Ada Venn"]);
    }
    assert!(facts_on(&world, "Ada Venn", "debt").is_empty());
    assert!(world.events.is_empty());
    assert!(facts_on(&world, "Ada Venn", "location").is_empty());
    assert!(facts_on(&world, "Harbour Council", "location").is_empty());
}

#[test]
fn completeness_trap() {
    let mut no_goal = npc_out("Tobin Reed");
    no_goal["goals"] = json!([]);
    let run1 = run(&draft(), &trapped(2, no_goal));
    assert_eq!(
        run1.result.unwrap_err(),
        BuildFailure {
            stage: Stage::NpcState,
            check: BuildCheck::Completeness
        }
    );

    // A Voice Card with one sample line, then a repair with two.
    let mut one_sample = npc_out("Tobin Reed");
    one_sample["voice_card"]["samples"] = json!(["Mind the rope."]);
    let mut outputs = plain();
    outputs.insert(2, one_sample);
    let run2 = run(&draft(), &outputs);
    let world = run2.world();
    assert!(repair_notes(&run2.gm)[0].contains("Completeness"));
    assert_eq!(world.voice_cards[0].samples.len(), 2);
}

#[test]
fn agency_trap() {
    let bad_hidden = hidden(&[
        hf("the Keeper", "plan", "sail north", false, &["Tobin Reed"]),
        hf(
            "Mira Holt",
            "reputation",
            "reliable",
            false,
            &["Tobin Reed"],
        ),
    ]);
    let bad_events = events(&[
        ev(60, "Tobin Reed", &[("Mira Holt", "mood", "afraid")], &[]),
        ev(
            70,
            "Tobin Reed",
            &[("the Keeper", "standing", "owed a favour")],
            &[],
        ),
        ev(80, "Mira Holt", &[], &[]),
        ev(90, "Tobin Reed", &[], &["Mira Holt"]),
    ]);
    let outputs = [
        bad_hidden.clone(),
        bad_hidden,
        hidden(&[]),
        npc_out("Tobin Reed"),
        npc_out("Ada Venn"),
        bad_events.clone(),
        bad_events,
        start(&[("Ada Venn", "Tide Hall")]),
    ];
    let run = run(&draft(), &outputs);
    let world = run.world();
    let notes = repair_notes(&run.gm);
    assert_eq!(notes.len(), 2);
    assert!(notes.iter().all(|n| n.contains("Agency")));
    assert!(facts_on(&world, "Mira Holt", "plan").is_empty());
    assert_eq!(facts_on(&world, "Mira Holt", "reputation").len(), 1);
    assert_eq!(world.events.len(), 1);
    assert_eq!(world.events[0].effects[0].subject, "Mira Holt");
    assert_eq!(world.events[0].effects[0].key, "standing");
}

#[test]
fn settled_facts_trap() {
    let bad_hidden = hidden(&[
        hf("Tobin Reed", "role", "smuggler", false, &["Ada Venn"]),
        hf("tobin reed", "Role", "Harbour Master", false, &["Ada Venn"]),
    ]);
    let mut bad_secret = npc_out("Tobin Reed");
    bad_secret["secret"] = json!({
        "subject": "Ada Venn", "key": "debt", "value": "ten coins",
        "text": "Ada owes ten coins.", "open": false
    });
    let outputs = [
        bad_hidden.clone(),
        bad_hidden,
        hidden(&[hf("Ada Venn", "debt", "forty coins", false, &["Ada Venn"])]),
        bad_secret.clone(),
        bad_secret,
        npc_out("Ada Venn"),
        events(&[]),
        start(&[]),
    ];
    let run = run(&draft(), &outputs);
    let world = run.world();
    let notes = repair_notes(&run.gm);
    assert_eq!(notes.len(), 2);
    assert!(notes.iter().all(|n| n.contains("SettledFacts")));
    // Another value on the slot of an accepted fact is dropped; the same
    // value adds its holders and adds no fact.
    let role = facts_on(&world, "Tobin Reed", "role");
    assert_eq!(role.len(), 1);
    assert_eq!(role[0].value, "harbour master");
    assert_eq!(role[0].known_by, ["Mira Holt", "Ada Venn"]);
    let debt = facts_on(&world, "Ada Venn", "debt");
    assert_eq!(debt.len(), 1);
    assert_eq!(debt[0].value, "forty coins");
    let dir = TempDir::new("settled");
    create_campaign_file(&dir.0.join("c.sqlite"), &draft().seed, &world, 7, None).unwrap();
}

#[test]
fn open_items_trap() {
    let bad_hidden = hidden(&[
        hf("Harbour Council", "vote", "new pier", false, &["Ada Venn"]),
        hf("Tobin Reed", "role", "harbour master", true, &["Ada Venn"]),
        hf("Ada Venn", "trip", "the islands", true, &["Ada Venn"]),
        hf("Ada Venn", "pending_move", "the city", false, &["Ada Venn"]),
    ]);
    let mut outputs = trapped(0, bad_hidden);
    outputs[2] = hidden(&[]);
    let run = run(&draft(), &outputs);
    let world = run.world();
    assert!(repair_notes(&run.gm)[0].contains("OpenItems"));
    let vote = facts_on(&world, "Harbour Council", "vote");
    assert_eq!(vote.len(), 1);
    assert!(vote[0].open);
    let role = facts_on(&world, "Tobin Reed", "role");
    assert_eq!(role.len(), 1);
    assert!(!role[0].open);
    assert!(facts_on(&world, "Ada Venn", "trip")[0].open);
    // A `pending_` prefix that the model wrote marks an open item; code
    // alone writes the prefix.
    let moving = facts_on(&world, "Ada Venn", "move");
    assert!(moving[0].open);
    let dir = TempDir::new("open");
    create_campaign_file(&dir.0.join("c.sqlite"), &draft().seed, &world, 7, None).unwrap();
}

/// Bug pass 3: the QA World Build wrote 7 Open Items of the draft again,
/// with the same sentence on a new key (6) or another subject (1), held by
/// no one but the GM. The proposal prompt then showed an item the PC had
/// settled as still open. An open fact whose subject and sentence are
/// those of an accepted open fact is that fact: it adds its holders and no
/// fact (changed after review B2: the subject must match).
#[test]
fn an_open_item_the_build_restates_is_merged() {
    let restated = |subject: &str, key: &str| {
        json!({
            "subject": subject, "key": key, "value": "pier",
            "text": "Harbour Council: vote is new pier.",
            "open": true, "known_by": ["Tobin Reed"],
        })
    };
    let mut outputs = plain();
    outputs[0] = hidden(&[
        restated("Harbour Council", "pier_vote"),
        restated("Tobin Reed", "council_vote"),
    ]);
    let first = run(&draft(), &outputs);
    let world = first.world();
    assert!(facts_on(&world, "Harbour Council", "pier_vote").is_empty());
    let vote = facts_on(&world, "Harbour Council", "vote");
    assert_eq!(vote.len(), 1);
    assert!(vote[0].open);
    assert!(
        vote[0].known_by.iter().any(|h| h == "Tobin Reed"),
        "{:?}",
        vote[0].known_by
    );
    // Review B2: on another subject the same sentence is another matter.
    // It stays a fact of its own, and no holder moves.
    let other = facts_on(&world, "Tobin Reed", "council_vote");
    assert_eq!(other.len(), 1);
    assert_eq!(other[0].known_by, ["Tobin Reed"]);
    // A settled fact with the sentence of an open one is no duplicate.
    let mut settled = plain();
    settled[0] = hidden(&[json!({
        "subject": "Tobin Reed", "key": "council_vote", "value": "pier",
        "text": "Harbour Council: vote is new pier.",
        "open": false, "known_by": ["Tobin Reed"],
    })]);
    let second = run(&draft(), &settled);
    assert_eq!(
        facts_on(&second.world(), "Tobin Reed", "council_vote").len(),
        1
    );
}

#[test]
fn secrecy_trap() {
    let bad_hidden = hidden(&[
        hf(
            "Ada Venn",
            "debt",
            "forty coins",
            false,
            &["Ada Venn", "the Keeper"],
        ),
        hf("Tobin Reed", "fear", "deep water", false, &["Tobin Reed"]),
        hf(
            "Tobin Reed",
            "route",
            "the north channel",
            false,
            &["Tobin Reed", "MIRA HOLT"],
        ),
        hf(
            "Ada Venn",
            "plan",
            "leave in spring",
            false,
            &["Ada Venn", "Mïra Hólt"],
        ),
    ]);
    let run = run(&draft(), &trapped(0, bad_hidden));
    // The PC under any spelling of its names: case and accents do not count.
    assert!(facts_on(&world_of(&run), "Tobin Reed", "route").is_empty());
    assert!(facts_on(&world_of(&run), "Ada Venn", "plan").is_empty());
    let world = run.world();
    assert!(repair_notes(&run.gm)[0].contains("Secrecy"));
    assert!(facts_on(&world, "Ada Venn", "debt").len() == 1);
    assert_eq!(
        facts_on(&world, "Ada Venn", "debt")[0].known_by,
        ["Ada Venn"]
    );
    assert_eq!(
        facts_on(&world, "Tobin Reed", "fear")[0].known_by,
        ["Tobin Reed"]
    );
    // A secret is held by its NPC alone.
    assert_eq!(
        facts_on(&world, "Tobin Reed", "savings")[0].known_by,
        ["Tobin Reed"]
    );
    // The PC holds no generated fact: only what it knew, and where it is.
    let draft = draft();
    for fact in world
        .facts
        .iter()
        .filter(|f| f.known_by.contains(&"Mira Holt".to_owned()))
    {
        let known = draft
            .knowledge
            .iter()
            .any(|k| k.subject == fact.subject && k.key == fact.key);
        assert!(known || fact.key == "location", "{fact:?}");
    }
}

#[test]
fn world_autonomy_trap() {
    assert_eq!(PC_CENTRED_MAX_SHARE, 0.5);
    let centred = |name: &str| {
        let mut out = npc_out(name);
        out["goals"] =
            json!([{"text": format!("{name} watches Mira Holt closely."), "against": null}]);
        out["secret"]["text"] = json!(format!("{name} fears the Keeper."));
        out
    };
    let mut outputs = plain();
    outputs[2] = centred("Tobin Reed");
    outputs[3] = centred("Ada Venn");

    // Every goal and secret names the PC: one more call per NPC, and the
    // share stays above the limit.
    let mut failing = outputs.clone();
    failing.insert(4, centred("Tobin Reed"));
    failing.insert(5, centred("Ada Venn"));
    let run1 = run(&draft(), &failing);
    assert_eq!(
        run1.result.unwrap_err(),
        BuildFailure {
            stage: Stage::NpcState,
            check: BuildCheck::WorldAutonomy
        }
    );
    assert_eq!(run1.gm.calls().len(), 6);
    let again = &run1.gm.calls()[4].prompt.recent;
    assert!(again.contains("WorldAutonomy"), "{again}");
    // The note names items by index, never by their text.
    assert!(!again.contains("watches Mira Holt closely"), "{again}");

    // One NPC repaired brings the share to the limit: the build goes on.
    let mut passing = outputs;
    passing.insert(4, npc_out("Tobin Reed"));
    let run2 = run(&draft(), &passing);
    let world = run2.world();
    assert_eq!(run2.gm.calls().len(), 7);
    assert_eq!(world.goals[0].text, "Tobin Reed wants a quiet winter.");
}

#[test]
fn a_goal_against_the_pc_or_a_secret_about_the_pc_counts_for_world_autonomy() {
    // No text names the PC. A goal against the PC and a secret about the PC
    // (under an other name) centre on the PC all the same.
    let against = json!({"text": "Ruin him.", "against": "Mira Holt"});
    let about_pc = json!({
        "subject": "the Keeper",
        "key": "reputation",
        "value": "a thief",
        "text": "The harbour calls that one a thief.",
        "open": false
    });
    let fails = |tobin: Value, ada: Value| {
        let mut outputs = plain();
        outputs[2] = tobin.clone();
        outputs[3] = ada.clone();
        outputs.insert(4, tobin);
        outputs.insert(5, ada);
        let run = run(&draft(), &outputs);
        assert_eq!(
            run.result.unwrap_err(),
            BuildFailure {
                stage: Stage::NpcState,
                check: BuildCheck::WorldAutonomy
            }
        );
        run.gm.calls()[4].prompt.recent.clone()
    };

    // Goals against the PC alone: four of six items.
    let two_against = |name: &str| {
        let mut out = npc_out(name);
        out["goals"] = json!([against, against]);
        out
    };
    let again = fails(two_against("Tobin Reed"), two_against("Ada Venn"));
    assert!(
        again.contains("goals[0]") && again.contains("goals[1]"),
        "{again}"
    );

    // One goal against the PC and one secret about the PC each: four of
    // four items. The goals alone would be two of four, at the limit.
    let one_each = |name: &str| {
        let mut out = npc_out(name);
        out["goals"] = json!([against]);
        out["secret"] = about_pc.clone();
        out
    };
    let again = fails(one_each("Tobin Reed"), one_each("Ada Venn"));
    assert!(again.contains("secret"), "{again}");
}

#[test]
fn a_goal_above_its_cap_is_dropped_and_a_long_list_is_cut() {
    let mut npc = npc_out("Tobin Reed");
    npc["goals"] = json!([
        {"text": "W".repeat(161), "against": null},
        {"text": "Tobin Reed wants a quiet winter.", "against": null}
    ]);
    // Six samples: the schema allows four.
    npc["voice_card"]["samples"] = json!(["One.", "Two.", "Three.", "Four.", "Five.", "Six."]);
    let mut outputs = plain();
    outputs[2] = npc.clone();
    outputs.insert(3, npc);
    let world = run(&draft(), &outputs).world();
    assert_eq!(
        goals_of(&world, "Tobin Reed"),
        [("Tobin Reed wants a quiet winter.", None)]
    );
    let card = world
        .voice_cards
        .iter()
        .find(|c| c.npc == "Tobin Reed")
        .unwrap();
    assert_eq!(card.samples, ["One.", "Two.", "Three.", "Four."]);
}

#[test]
fn optional_item_is_dropped_and_the_build_goes_on() {
    let mut bad_npc = npc_out("Tobin Reed");
    bad_npc["secret"]["subject"] = json!("Nobody Known");
    bad_npc["attitudes"] = json!([{"to": "Nobody Known", "attitude": 1}]);
    bad_npc["goals"] = json!([
        {"text": "Tobin Reed wants a quiet winter.", "against": null},
        {"text": "Tobin Reed wants a new boat.", "against": "Nobody Known"}
    ]);
    let bad_events = events(&[ev(60, "Nobody Known", &[], &[])]);
    let outputs = [
        hidden(&[]),
        hidden(&[]),
        bad_npc.clone(),
        bad_npc,
        npc_out("Ada Venn"),
        bad_events.clone(),
        bad_events,
        start(&[("Ada Venn", "Tide Hall")]),
    ];
    let world = run(&draft(), &outputs).world();
    assert!(facts_on(&world, "Nobody Known", "savings").is_empty());
    assert!(world.attitudes.is_empty());
    // A bad `against` drops only the `against`.
    assert_eq!(
        goals_of(&world, "Tobin Reed"),
        [
            ("Tobin Reed wants a quiet winter.", None),
            ("Tobin Reed wants a new boat.", None)
        ]
    );
    assert!(world.events.is_empty());
}

#[test]
fn essential_item_fails_the_build() {
    let mut one_sample = npc_out("Ada Venn");
    one_sample["voice_card"]["samples"] = json!(["Mind the rope."]);
    let run1 = run(&draft(), &trapped(3, one_sample));
    assert_eq!(
        run1.result.unwrap_err(),
        BuildFailure {
            stage: Stage::NpcState,
            check: BuildCheck::Completeness
        }
    );

    let mut outputs: Vec<String> = plain().iter().map(Value::to_string).collect();
    outputs[5] = "not JSON".into();
    outputs.push("still not JSON".into());
    let run2 = run_raw(&draft(), outputs, None);
    assert_eq!(
        run2.result.unwrap_err(),
        BuildFailure {
            stage: Stage::StartState,
            check: BuildCheck::Schema
        }
    );

    // A start place that is not a place is a gap: it fails before any call.
    let mut draft = draft();
    draft.seed.start.place = "Harbour Council".into();
    let run3 = run(&draft, &plain());
    assert_eq!(
        run3.result.unwrap_err(),
        BuildFailure {
            stage: Stage::HiddenFacts,
            check: BuildCheck::NotApproved
        }
    );
    assert!(run3.gm.calls().is_empty());
    assert!(run3.gm.calls().is_empty());
}

#[test]
fn event_rules() {
    let mut long = ev(60, "Tobin Reed", &[], &[]);
    long["text"] = json!("x".repeat(81));
    // Six events, the schema's cap. Three effects are above the cap of two:
    // the list is cut to two and the event stays.
    let bad_events = events(&[
        ev(5, "Tobin Reed", &[], &[]),
        ev(20_161, "Tobin Reed", &[], &[]),
        ev(60, "Tobin Reed", &[], &["Tobin Reed"]),
        long,
        ev(
            10,
            "Ada Venn",
            &[("Gull Point", "lamp", "dark")],
            &["Tobin Reed"],
        ),
        ev(
            20_160,
            "Ada Venn",
            &[
                ("Ada Venn", "a", "1"),
                ("Ada Venn", "b", "2"),
                ("Ada Venn", "c", "3"),
            ],
            &[],
        ),
    ]);
    let world = run(&draft(), &trapped(4, bad_events)).world();
    let at: Vec<u64> = world.events.iter().map(|e| e.at).collect();
    assert_eq!(at, [10, 20_160]);
    let keys: Vec<&str> = world.events[1]
        .effects
        .iter()
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(keys, ["a", "b"]);
}

#[test]
fn an_event_effect_never_writes_an_open_key() {
    // An open item is written by `setup` alone (ADR 0005): an effect on a
    // `pending_` key drops its event after the repair.
    let trap = events(&[
        ev(
            30,
            "Tobin Reed",
            &[("Harbour Council", "pending_vote", "new pier")],
            &[],
        ),
        ev(
            60,
            "Tobin Reed",
            &[("Harbour Council", "vote", "new pier")],
            &[],
        ),
    ]);
    let run = run(&draft(), &trapped(4, trap));
    let repair = run
        .gm
        .calls()
        .iter()
        .map(|c| c.prompt.recent.clone())
        .find(|r| r.contains("YOUR LAST ANSWER FAILED"))
        .unwrap();
    assert!(repair.contains("events[0].effects[0].key"), "{repair}");
    let world = run.world();
    let at: Vec<u64> = world.events.iter().map(|e| e.at).collect();
    assert_eq!(at, [60]);
}

#[test]
fn a_repair_note_never_quotes_hidden_text() {
    const MARK: &str = "zqxmark";
    let bad_hidden = hidden(&[
        hf(
            &format!("{MARK} Person"),
            &format!("{MARK}_key"),
            MARK,
            false,
            &[],
        ),
        hf(
            "Ada Venn",
            &format!("{MARK}_key"),
            MARK,
            false,
            &["Mira Holt", &format!("{MARK} Two")],
        ),
        hf("Mira Holt", &format!("{MARK}_plan"), MARK, false, &[]),
        hf("Tobin Reed", "role", MARK, false, &[]),
    ]);
    let mut bad_npc = npc_out("Tobin Reed");
    bad_npc["secret"] = json!({
        "subject": format!("{MARK} Person"), "key": MARK, "value": MARK,
        "text": MARK, "open": false
    });
    bad_npc["attitudes"] = json!([{"to": MARK, "attitude": 1}]);
    bad_npc["goals"] = json!([
        {"text": "Tobin Reed wants a quiet winter.", "against": null},
        {"text": MARK, "against": MARK}
    ]);
    let mut bad_event = ev(60, MARK, &[(MARK, MARK, MARK)], &[MARK]);
    bad_event["text"] = json!(MARK);
    let bad_events = events(&[bad_event]);
    let bad_start = start(&[(MARK, MARK)]);
    let outputs = [
        bad_hidden.clone(),
        bad_hidden,
        hidden(&[]),
        bad_npc.clone(),
        bad_npc,
        npc_out("Ada Venn"),
        bad_events.clone(),
        bad_events,
        bad_start.clone(),
        bad_start,
    ];
    let run = run(&draft(), &outputs);
    run.world();
    let notes = repair_notes(&run.gm);
    assert_eq!(notes.len(), 4);
    for note in notes {
        assert!(!note.to_lowercase().contains(MARK), "{note}");
    }
}

// ---- Fix round 1 -------------------------------------------------------------

fn goals_of<'a>(world: &'a WorldSetup, npc: &str) -> Vec<(&'a str, Option<&'a str>)> {
    world
        .goals
        .iter()
        .filter(|g| g.npc == npc)
        .map(|g| (g.text.as_str(), g.against.as_deref()))
        .collect()
}

fn with_goals(goals: Value) -> Value {
    let mut out = npc_out("Tobin Reed");
    out["goals"] = goals;
    out
}

#[test]
fn a_bad_against_drops_only_the_against() {
    let trap = with_goals(json!([
        {"text": "Tobin Reed wants a new boat.", "against": "Nobody Known"},
        {"text": "Tobin Reed wants a quiet winter.", "against": "Ada Venn"}
    ]));
    let run = run(&draft(), &trapped(2, trap));
    let world = run.world();
    assert!(repair_notes(&run.gm)[0].contains("References"));
    assert_eq!(
        goals_of(&world, "Tobin Reed"),
        [
            ("Tobin Reed wants a new boat.", None),
            ("Tobin Reed wants a quiet winter.", Some("Ada Venn"))
        ]
    );
}

#[test]
fn a_bad_first_goal_is_dropped_when_another_survives() {
    let trap = with_goals(json!([
        {"text": "  ", "against": null},
        {"text": "Tobin Reed wants a quiet winter.", "against": null}
    ]));
    let run = run(&draft(), &trapped(2, trap));
    let world = run.world();
    assert_eq!(
        goals_of(&world, "Tobin Reed"),
        [("Tobin Reed wants a quiet winter.", None)]
    );
}

#[test]
fn an_npc_with_no_surviving_goal_fails_completeness() {
    let trap = with_goals(json!([
        {"text": "", "against": null},
        {"text": " ", "against": "Ada Venn"}
    ]));
    let run = run(&draft(), &trapped(2, trap));
    assert_eq!(
        run.result.unwrap_err(),
        BuildFailure {
            stage: Stage::NpcState,
            check: BuildCheck::Completeness
        }
    );
    assert_eq!(run.gm.calls().len(), 4);
}

#[test]
fn a_slot_conflict_in_an_approved_draft_fails_the_build() {
    // `approve` refuses each of these drafts (`SlotConflict`). A draft
    // approved by hand fails too: `build_world` checks the gaps again before
    // any call, so it never reaches `setup` with two facts on a slot.
    let decided = threshold_core::domain::SeedFact {
        subject: "Tobin Reed".into(),
        key: "role".into(),
        value: "harbour master".into(),
        text: "Tobin Reed is the harbour master.".into(),
    };
    let mut retired = known("tobin reed", "Role", "retired pilot", false);
    retired.text = "Tobin Reed retired.".into();
    let cases = [
        (
            vec![],
            vec![
                known("Tobin Reed", "role", "harbour master", false),
                retired,
            ],
        ),
        (
            vec![decided],
            vec![known("Tobin Reed", "role", "harbour master", false)],
        ),
        (
            vec![],
            vec![
                known("Tobin Reed", "role", "harbour master", false),
                known("Tobin Reed", "role", "harbour pilot", true),
            ],
        ),
    ];
    for (decided, knowledge) in cases {
        let mut seed = seed();
        seed.decided = decided;
        let mut draft = SeedDraft::new(source(), seed);
        draft.knowledge = knowledge;
        assert!(
            threshold_core::build::gaps(&draft)
                .iter()
                .any(|g| g.kind == threshold_core::build::GapKind::SlotConflict)
        );
        draft.state = DraftState::Approved;
        let run = run(&draft, &plain());
        assert_eq!(
            run.result.unwrap_err(),
            BuildFailure {
                stage: Stage::HiddenFacts,
                check: BuildCheck::NotApproved
            }
        );
        assert!(run.gm.calls().is_empty());
    }
}

#[test]
fn an_absent_npc_is_never_placed_at_the_start_place() {
    let trap = start(&[("Ada Venn", "Gull Point")]);
    let run = run(&draft(), &trapped(5, trap));
    let world = run.world();
    // Ada Venn is not in `start.present`: the approved start state says she
    // is not at Gull Point.
    assert!(repair_notes(&run.gm)[0].contains("SettledFacts"));
    assert!(facts_on(&world, "Ada Venn", "location").is_empty());
}

// ---- Names by schema -----------------------------------------------------------

/// The schema node at `path` of `schema`: each step is a property name, or
/// `[]` for the items of a list. A `$ref` is followed, and a nullable
/// `anyOf` gives its other branch.
fn node<'a>(schema: &'a Value, path: &[&str]) -> &'a Value {
    let mut at = resolve(schema, schema);
    for step in path {
        at = match *step {
            "[]" => &at["items"],
            name => &at["properties"][name],
        };
        at = resolve(schema, at);
    }
    at
}

fn resolve<'a>(schema: &'a Value, mut at: &'a Value) -> &'a Value {
    loop {
        if let Some(path) = at.get("$ref").and_then(Value::as_str) {
            at = schema.pointer(&path[1..]).expect("a $ref resolves");
        } else if let Some(branch) = at
            .get("anyOf")
            .and_then(Value::as_array)
            .and_then(|b| b.iter().find(|b| b["type"] != json!("null")))
        {
            at = branch;
        } else {
            return at;
        }
    }
}

/// The values of the `enum` at `path`; `null` as "null".
fn names_at<'a>(schema: &'a Value, path: &[&str]) -> Vec<&'a str> {
    node(schema, path)["enum"]
        .as_array()
        .unwrap_or_else(|| panic!("no enum at {path:?}: {}", node(schema, path)))
        .iter()
        .map(|v| v.as_str().unwrap_or("null"))
        .collect()
}

const ALL: [&str; 6] = [
    "Mira Holt",
    "Tobin Reed",
    "Ada Venn",
    "Gull Point",
    "Tide Hall",
    "Harbour Council",
];
const NPCS: [&str; 2] = ["Tobin Reed", "Ada Venn"];

#[test]
fn every_name_field_of_each_world_stage_is_an_enum() {
    let run = run(&draft(), &plain());
    run.world();
    let calls = run.gm.calls();
    let steps: Vec<&str> = run.log.iter().map(|e| e.step.as_str()).collect();
    assert_eq!(
        steps,
        vec![
            "hidden_facts 1/2",
            "hidden_facts 2/2",
            "npc_state 1/2",
            "npc_state 2/2",
            "world_events",
            "start_state",
        ]
    );

    for hidden in &calls[0..2] {
        assert_eq!(names_at(&hidden.schema, &["facts", "[]", "subject"]), ALL);
        assert_eq!(
            names_at(&hidden.schema, &["facts", "[]", "known_by", "[]"]),
            NPCS
        );
    }

    // Tobin Reed's state: never toward Tobin Reed himself. An attitude may
    // target a group, never a place.
    let npc = &calls[2].schema;
    assert_eq!(
        names_at(npc, &["attitudes", "[]", "to"]),
        vec!["Mira Holt", "Ada Venn", "Harbour Council"]
    );
    assert_eq!(
        names_at(npc, &["goals", "[]", "against"]),
        vec!["Mira Holt", "Ada Venn", "null"]
    );
    assert_eq!(names_at(npc, &["secret", "subject"]), ALL);
    assert_eq!(
        names_at(&calls[3].schema, &["attitudes", "[]", "to"]),
        vec!["Mira Holt", "Tobin Reed", "Harbour Council"]
    );

    let events = &calls[4].schema;
    assert_eq!(names_at(events, &["events", "[]", "actor"]), NPCS);
    // Review of bug pass 4, blocker: an event says whether everyone at the
    // place perceives what it changes, as a required boolean.
    let event = node(events, &["events", "[]"]);
    assert!(
        event["required"]
            .as_array()
            .unwrap()
            .contains(&json!("public")),
        "{event}"
    );
    assert_eq!(event["properties"]["public"]["type"], json!("boolean"));
    assert!(
        event["properties"]["public"]["description"]
            .as_str()
            .unwrap()
            .contains("everyone at the place")
    );
    assert_eq!(names_at(events, &["events", "[]", "seen_by", "[]"]), NPCS);
    assert_eq!(
        names_at(events, &["events", "[]", "effects", "[]", "subject"]),
        ALL
    );

    // Tobin Reed is present at the start, so only Ada Venn is placed, and
    // never at the start place, Gull Point.
    let start = &calls[5].schema;
    assert_eq!(
        names_at(start, &["placements", "[]", "npc"]),
        vec!["Ada Venn"]
    );
    assert_eq!(
        names_at(start, &["placements", "[]", "place"]),
        vec!["Tide Hall"]
    );
}

#[test]
fn hidden_fact_holders_cannot_be_the_pc_by_schema() {
    let run = run(&draft(), &plain());
    run.world();
    for call in &run.gm.calls()[0..2] {
        let holders = names_at(&call.schema, &["facts", "[]", "known_by", "[]"]);
        for pc in ["Mira Holt", "the Keeper"] {
            assert!(!holders.contains(&pc), "{holders:?}");
        }
    }
}

#[test]
fn an_attitude_toward_a_group_is_built_and_committed() {
    let draft = draft();
    let mut outputs = plain();
    outputs[2]["attitudes"] = json!([{"to": "Harbour Council", "attitude": -2}]);
    let run = run(&draft, &outputs);
    let world = run.world();
    assert_eq!(world.attitudes.len(), 1);
    assert_eq!(world.attitudes[0].from, "Tobin Reed");
    assert_eq!(world.attitudes[0].to, "Harbour Council");
    assert_eq!(world.attitudes[0].attitude, -2);

    let dir = TempDir::new("group-attitude");
    let path = dir.0.join("campaign-1.sqlite");
    create_campaign_file(&path, &draft.seed, &world, 7, None).unwrap();
    let store = Store::open_existing(&path).unwrap();
    let view = store.snapshot(store.campaigns().unwrap()[0]).unwrap();
    let id = |name: &str| view.entity_by_name(name).unwrap().id;
    let attitude = view
        .attitudes
        .iter()
        .find(|a| a.from == id("Tobin Reed") && a.to == id("Harbour Council"))
        .expect("the attitude toward the group is committed");
    assert_eq!(attitude.attitude, -2);
}

#[test]
fn with_no_place_but_the_start_place_the_start_state_is_not_called() {
    let mut seed = seed();
    seed.entities.retain(|e| e.name != "Tide Hall");
    let draft = draft_of(seed, vec![]);
    let run = run(&draft, &plain()[..5]);
    let world = run.world();
    // Hidden facts, two NPC states, events: no StartState call, and the
    // absent NPC gets no placement.
    assert_eq!(run.gm.calls().len(), 5);
    assert!(!run.log.iter().any(|e| e.step.starts_with("start_state")));
    assert!(facts_on(&world, "Ada Venn", "location").is_empty());
    assert_eq!(facts_on(&world, "Tobin Reed", "location").len(), 1);
}

#[test]
fn a_shared_first_name_does_not_trigger_world_autonomy() {
    // Ada Venn is now Mira Venn: she shares the PC's first name, Mira.
    let mut seed = seed();
    seed.entities[1].name = "Mira Venn".into();
    let draft = draft_of(seed, vec![]);
    let outputs: Vec<Value> = plain()
        .iter()
        .map(|o| serde_json::from_str(&o.to_string().replace("Ada Venn", "Mira Venn")).unwrap())
        .collect();
    let mut outputs = outputs;
    // Three of the four goals and secrets name Mira Venn; none names the PC.
    outputs[2]["goals"] = json!([{"text": "Help Mira Venn mend the nets.", "against": null}]);
    let run = run(&draft, &outputs);
    let world = run.world();
    // No WorldAutonomy call: six calls, as in a plain build.
    assert_eq!(run.gm.calls().len(), 6);
    assert!(!run.log.iter().any(|e| e.step.contains("autonomy")));
    assert_eq!(world.goals.len(), 2);
}

/// Review B2 (T7): two NPCs owe different sums, and the World Build writes
/// both with one sentence. They are two matters: each fact stays, with its
/// own value and its own holder. Merging on the sentence alone gave Ada
/// Venn's holders to Tobin Reed's debt and lost hers.
#[test]
fn two_matters_with_one_sentence_stay_two_facts() {
    let debt = |subject: &str, value: &str| {
        json!({
            "subject": subject, "key": "levy", "value": value,
            "text": "A debt to the Harbour Council is due at midwinter.",
            "open": true, "known_by": [subject],
        })
    };
    let mut outputs = plain();
    outputs[0] = hidden(&[
        debt("Tobin Reed", "40 marks"),
        debt("Ada Venn", "900 marks"),
    ]);
    let world = run(&draft(), &outputs).world();
    let tobin = facts_on(&world, "Tobin Reed", "levy");
    assert_eq!(tobin.len(), 1);
    assert_eq!(tobin[0].value, "40 marks");
    assert_eq!(tobin[0].known_by, ["Tobin Reed"]);
    let ada = facts_on(&world, "Ada Venn", "levy");
    assert_eq!(ada.len(), 1, "{:?}", world.facts);
    assert_eq!(ada[0].value, "900 marks");
    assert_eq!(ada[0].known_by, ["Ada Venn"]);
}
