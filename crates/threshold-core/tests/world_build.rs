//! `build_world`: the four World Build stages and the checks of design
//! 6.5, replayed with `RecordedGm` (data shape 3.6, design 6.3).

use std::{fs, path::PathBuf};

use serde_json::{Value, json};
use threshold_core::{
    RecordedGm,
    build::{
        BuildCheck, BuildFailure, Control, DebugEntry, DraftState, KnownFact, PC_CENTRED_MAX_SHARE,
        Progress, SeedDraft, Stage, approve, build_world,
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
        },
        open_details: vec![],
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
        let mut ctl = Control {
            progress: &mut on_progress,
            cancelled: &cancelled,
            log: &mut on_log,
        };
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

    fn is_empty(&self) -> bool {
        fs::read_dir(&self.0).unwrap().next().is_none()
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
    create_campaign_file(&path, &draft.seed, &world, 7).unwrap();
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
    create_campaign_file(&path, &draft.seed, &world, 7).unwrap();
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
fn build_writes_no_file() {
    let dir = TempDir::new("nofile");
    run(&draft(), &plain()).world();
    assert!(dir.is_empty());
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
    create_campaign_file(&dir.0.join("c.sqlite"), &draft.seed, &world, 7).unwrap();
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
            let Progress { stage, done, total } = *p;
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
fn known_facts_keep_one_entry_per_slot() {
    let draft = draft_of(
        seed(),
        vec![
            known("Tobin Reed", "role", "harbour master", false),
            known("Tobin Reed", "location", "Tide Hall", false),
            known("tobin reed", "Role", "retired pilot", false),
        ],
    );
    let world = run(&draft, &plain()).world();
    // A later fact on the same slot is the later state; the start state is
    // the latest of all.
    let role = facts_on(&world, "Tobin Reed", "role");
    assert_eq!(role.len(), 1);
    assert_eq!(role[0].value, "retired pilot");
    let location = facts_on(&world, "Tobin Reed", "location");
    assert_eq!(location.len(), 1);
    assert_eq!(location[0].value, "Gull Point");
    let dir = TempDir::new("slots");
    create_campaign_file(&dir.0.join("c.sqlite"), &draft.seed, &world, 7).unwrap();
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
    create_campaign_file(&dir.0.join("c.sqlite"), &draft().seed, &world, 7).unwrap();
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
    create_campaign_file(&dir.0.join("c.sqlite"), &draft().seed, &world, 7).unwrap();
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
    ]);
    let run = run(&draft(), &trapped(0, bad_hidden));
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
    let tobin: Vec<&str> = world
        .goals
        .iter()
        .filter(|g| g.npc == "Tobin Reed")
        .map(|g| g.text.as_str())
        .collect();
    assert_eq!(tobin, ["Tobin Reed wants a quiet winter."]);
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

    // A start place that is not a place fails before any call.
    let mut draft = draft();
    draft.seed.start.place = "Harbour Council".into();
    let run3 = run(&draft, &plain());
    assert_eq!(
        run3.result.unwrap_err(),
        BuildFailure {
            stage: Stage::StartState,
            check: BuildCheck::Kinds
        }
    );
    assert!(run3.gm.calls().is_empty());
}

#[test]
fn event_rules() {
    let mut long = ev(60, "Tobin Reed", &[], &[]);
    long["text"] = json!("x".repeat(81));
    let bad_events = events(&[
        ev(5, "Tobin Reed", &[], &[]),
        ev(20_161, "Tobin Reed", &[], &[]),
        ev(60, "Tobin Reed", &[], &["Tobin Reed"]),
        ev(
            60,
            "Tobin Reed",
            &[
                ("Ada Venn", "a", "1"),
                ("Ada Venn", "b", "2"),
                ("Ada Venn", "c", "3"),
            ],
            &[],
        ),
        long,
        ev(
            10,
            "Ada Venn",
            &[("Gull Point", "lamp", "dark")],
            &["Tobin Reed"],
        ),
        ev(20_160, "Ada Venn", &[], &[]),
    ]);
    let world = run(&draft(), &trapped(4, bad_events)).world();
    let at: Vec<u64> = world.events.iter().map(|e| e.at).collect();
    assert_eq!(at, [10, 20_160]);
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
