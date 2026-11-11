//! `draft_from_text`: the three draft stages, their checks and the stage
//! runner, replayed with `RecordedGm` (data shape 2 and 3.5, design 6.2).

use serde_json::{Value, json};
use threshold_core::{
    RecordedGm,
    build::{
        AgeSource, BuildCheck, BuildFailure, Control, Cut, DebugEntry, DraftState, GapKind,
        KnownFact, Progress, SOURCE_TEXT_MAX_CHARS, SOURCE_TEXT_MIN_CHARS, SeedDraft,
        SourceTextProblem, Stage, age_lines, apply_edit, approve, build_world, check_source_text,
        confirm_age, draft_from_text, gaps,
    },
    domain::{EntityKind, OpenMode, SeedEntity},
};

/// What one draft returned, reported and logged.
struct Run {
    result: Result<SeedDraft, BuildFailure>,
    progress: Vec<Progress>,
    log: Vec<DebugEntry>,
}

fn run(text: &str, gm: &RecordedGm) -> Run {
    run_until(text, gm, &|| false)
}

fn run_until(text: &str, gm: &RecordedGm, cancelled: &dyn Fn() -> bool) -> Run {
    let mut progress = Vec::new();
    let mut log = Vec::new();
    let result = {
        let mut on_progress = |p| progress.push(p);
        let mut on_log = |e| log.push(e);
        let mut ctl = Control::new(&mut on_progress, cancelled, &mut on_log);
        draft_from_text(text, gm, &mut ctl)
    };
    Run {
        result,
        progress,
        log,
    }
}

/// Each character's name and where its age comes from, the PC first.
fn sources(draft: &SeedDraft) -> Vec<(String, AgeSource)> {
    age_lines(draft)
        .into_iter()
        .map(|l| (l.name, l.source))
        .collect()
}

/// The names whose age is not confirmed.
fn waiting(draft: &SeedDraft) -> Vec<String> {
    age_lines(draft)
        .into_iter()
        .filter(|l| !l.confirmed)
        .map(|l| l.name)
        .collect()
}

/// Confirms every age, as the Player would one by one.
fn confirm_all(draft: &mut SeedDraft) {
    for line in age_lines(draft) {
        confirm_age(draft, &line.entry()).unwrap();
    }
}

fn gm(outputs: &[Value]) -> RecordedGm {
    RecordedGm::new(outputs.iter().map(Value::to_string).collect(), vec![])
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

/// An invented Source Text with three headings, so three sections.
fn source() -> String {
    [
        section(
            "The harbour",
            "Mira Holt keeps the lighthouse at Gull Point.",
        ),
        section("The council", "The Harbour Council meets in the Tide Hall."),
        section("The storm", "A storm is coming and the boats stay in port."),
    ]
    .concat()
}

fn seed_out() -> Value {
    json!({
        "premise": "A lighthouse keeper faces the storm season.",
        "story_language": "en",
        "themes": ["duty", "weather"],
        "gm_agreement": "Keep the pace slow.",
        "resolution": "Roll when the outcome is uncertain.",
        "min_age": null,
        "start": {
            "place": "Gull Point",
            "present": ["Tobin Reed"],
            "situation": "Night falls on the point.",
            "when": "An autumn evening"
        },
        "open_details": [{"text": "Who sent the letter.", "mode": "discover"}]
    })
}

fn pc_out() -> Value {
    json!({
        "name": "Mira Holt",
        "age": 31,
        "age_stated": true,
        "other_names": [{"name": "the Keeper", "known_to": ["Mira Holt", "Tobin Reed"]}],
        "concept": "A careful lighthouse keeper."
    })
}

fn npc(name: &str, age: u16, stated: bool) -> Value {
    json!({"name": name, "age": age, "age_stated": stated, "other_names": []})
}

fn thing(name: &str, kind: &str) -> Value {
    json!({"name": name, "kind": kind, "other_names": []})
}

fn roster(characters: &[Value], others: &[Value]) -> Value {
    json!({ "pc": pc_out(), "characters": characters, "others": others })
}

fn roster_out() -> Value {
    roster(
        &[npc("Tobin Reed", 58, true), npc("Ada Venn", 40, false)],
        &[
            thing("Gull Point", "place"),
            thing("Tide Hall", "place"),
            thing("Harbour Council", "faction"),
        ],
    )
}

fn fact(subject: &str, key: &str, value: &str, open: bool, pc_knows: bool) -> Value {
    json!({
        "subject": subject,
        "key": key,
        "value": value,
        "text": format!("{subject}: {key} is {value}."),
        "open": open,
        "pc_knows": pc_knows
    })
}

fn knowledge(facts: &[Value]) -> Value {
    json!({ "facts": facts })
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

/// Seed, roster and one empty knowledge output per section of `source()`.
fn plain_outputs() -> Vec<Value> {
    vec![
        roster_out(),
        seed_out(),
        knowledge(&[]),
        knowledge(&[]),
        knowledge(&[]),
    ]
}

#[test]
fn empty_or_long_source_is_refused_before_any_call() {
    assert_eq!(SOURCE_TEXT_MAX_CHARS, 40_000);
    let too_long = "a".repeat(SOURCE_TEXT_MAX_CHARS + 1);
    for text in ["", "   \n\t ", too_long.as_str()] {
        let gm = gm(&[]);
        let run = run(text, &gm);
        assert_eq!(
            run.result.unwrap_err(),
            BuildFailure {
                stage: Stage::DraftSeed,
                check: BuildCheck::Completeness
            }
        );
        assert!(gm.calls().is_empty());
        assert!(run.log.is_empty());
    }

    // The limit counts characters, not bytes, and is inclusive: this text
    // reaches the model, which has no recorded answer. The roster is the
    // first call.
    let at_limit = "é".repeat(SOURCE_TEXT_MAX_CHARS);
    let gm = gm(&[]);
    let run = run(&at_limit, &gm);
    assert_eq!(gm.calls().len(), 1);
    assert_eq!(
        run.result.unwrap_err(),
        BuildFailure {
            stage: Stage::DraftRoster,
            check: BuildCheck::Model
        }
    );
}

#[test]
fn draft_holds_seed_roster_and_known_facts() {
    let text = source();
    let gm = gm(&[
        roster_out(),
        seed_out(),
        knowledge(&[
            fact("Mira Holt", "occupation", "lighthouse keeper", false, true),
            fact("Gull Point", "owner", "Harbour Council", false, true),
        ]),
        knowledge(&[fact("Harbour Council", "vote", "new pier", true, true)]),
        knowledge(&[fact("Tobin Reed", "location", "Gull Point", false, true)]),
    ]);
    let draft = run(&text, &gm).result.unwrap();

    assert!(draft.id.starts_with("draft-"));
    assert_eq!(draft.state, DraftState::Draft);
    assert_eq!(draft.source_text, text);
    assert_eq!(draft.last_failure, None);
    assert_eq!(draft.campaign_file, None);

    let seed = &draft.seed;
    assert_eq!(seed.premise, "A lighthouse keeper faces the storm season.");
    assert_eq!(seed.story_language, "en");
    assert_eq!(seed.pc.name, "Mira Holt");
    assert_eq!(seed.pc.age, 31);
    assert_eq!(seed.pc.concept, "A careful lighthouse keeper.");
    assert_eq!(seed.pc.other_names.len(), 1);
    assert_eq!(seed.pc.other_names[0].name, "the Keeper");
    assert_eq!(seed.themes, vec!["duty", "weather"]);
    assert_eq!(seed.gm_agreement, "Keep the pace slow.");
    assert_eq!(seed.resolution, "Roll when the outcome is uncertain.");
    assert_eq!(seed.min_age, None);
    assert_eq!(seed.start.place, "Gull Point");
    assert_eq!(seed.start.present, vec!["Tobin Reed"]);
    assert_eq!(seed.open_details.len(), 1);
    assert_eq!(seed.open_details[0].mode, OpenMode::Discover);
    assert!(seed.boundaries.is_empty());
    assert!(seed.decided.is_empty());
    assert!(seed.proposed.is_empty());

    let roster: Vec<(&str, EntityKind, Option<u16>)> = seed
        .entities
        .iter()
        .map(|e| (e.name.as_str(), e.kind, e.age))
        .collect();
    assert_eq!(
        roster,
        vec![
            ("Tobin Reed", EntityKind::Npc, Some(58)),
            ("Ada Venn", EntityKind::Npc, Some(40)),
            ("Gull Point", EntityKind::Place, None),
            ("Tide Hall", EntityKind::Place, None),
            ("Harbour Council", EntityKind::Faction, None),
        ]
    );
    assert!(seed.entities.iter().all(|e| !e.hidden));

    assert_eq!(
        draft.knowledge,
        vec![
            known("Mira Holt", "occupation", "lighthouse keeper", false),
            known("Gull Point", "owner", "Harbour Council", false),
            known("Harbour Council", "vote", "new pier", true),
            known("Tobin Reed", "location", "Gull Point", false),
        ]
    );
    // Every age waits for the Player, the PC's too; the model's claim that
    // the text states an age is kept as information only.
    assert_eq!(waiting(&draft), vec!["Mira Holt", "Tobin Reed", "Ada Venn"]);
    assert_eq!(
        sources(&draft),
        vec![
            ("Mira Holt".to_owned(), AgeSource::FromText),
            ("Tobin Reed".to_owned(), AgeSource::FromText),
            ("Ada Venn".to_owned(), AgeSource::Proposed),
        ]
    );
    assert!(draft.cut.is_empty());
    // The PC knows every other name that the draft lists.
    assert_eq!(
        seed.pc.other_names[0].known_to,
        vec!["Mira Holt", "Tobin Reed"]
    );

    // The draft passes its own review once every age is confirmed.
    let mut confirmed = draft.clone();
    confirm_all(&mut confirmed);
    assert_eq!(gaps(&confirmed), vec![]);
}

#[test]
fn knowledge_runs_one_call_per_section() {
    let text = source();
    let gm = gm(&plain_outputs());
    run(&text, &gm).result.unwrap();

    let calls = gm.calls();
    assert_eq!(calls.len(), 5);
    let titles = ["The harbour", "The council", "The storm"];
    for (call, title) in calls[2..].iter().zip(titles) {
        let recent = &call.prompt.recent;
        assert!(recent.contains(&format!("# {title}")), "{recent}");
        let others = titles.iter().filter(|t| **t != title);
        for other in others {
            assert!(!recent.contains(&format!("# {other}")), "{recent}");
        }
    }
}

#[test]
fn proposed_ages_are_listed_for_confirmation() {
    let mut roster = roster_out();
    roster["pc"]["age_stated"] = json!(false);
    let gm = gm(&[
        roster,
        seed_out(),
        knowledge(&[]),
        knowledge(&[]),
        knowledge(&[]),
    ]);
    let draft = run(&source(), &gm).result.unwrap();
    assert_eq!(waiting(&draft), vec!["Mira Holt", "Tobin Reed", "Ada Venn"]);
    assert_eq!(
        sources(&draft),
        vec![
            ("Mira Holt".to_owned(), AgeSource::Proposed),
            ("Tobin Reed".to_owned(), AgeSource::FromText),
            ("Ada Venn".to_owned(), AgeSource::Proposed),
        ]
    );
}

#[test]
fn an_age_the_model_calls_stated_still_needs_the_player() {
    // The model says the text states every age: no age passes on its word.
    let mut roster = roster_out();
    roster["characters"][1]["age_stated"] = json!(true);
    let gm = gm(&[
        roster,
        seed_out(),
        knowledge(&[]),
        knowledge(&[]),
        knowledge(&[]),
    ]);
    let mut draft = run(&source(), &gm).result.unwrap();
    assert!(
        sources(&draft)
            .iter()
            .all(|(_, s)| *s == AgeSource::FromText)
    );
    let unconfirmed = |d: &SeedDraft| -> Vec<String> {
        gaps(d)
            .into_iter()
            .filter(|g| g.kind == GapKind::AgeUnconfirmed)
            .map(|g| g.at)
            .collect()
    };
    assert_eq!(
        unconfirmed(&draft),
        vec!["pc.age", "entities[0].age", "entities[1].age"]
    );

    // Approval waits for every confirmation, the PC's included.
    let mut lines = age_lines(&draft);
    lines.rotate_left(1);
    for line in lines {
        assert!(approve(&mut draft).is_err());
        assert_eq!(draft.state, DraftState::Draft);
        confirm_age(&mut draft, &line.entry()).unwrap();
    }
    assert_eq!(unconfirmed(&draft), Vec::<String>::new());
    assert_eq!(approve(&mut draft), Ok(()));
    assert_eq!(draft.state, DraftState::Approved);
}

#[test]
fn a_roster_text_above_its_cap_fails_the_roster() {
    // A roster entry is essential: a name above its cap is never dropped
    // in silence, and the roster fails after one repair.
    let mut roster = roster_out();
    roster["characters"][1]["name"] = json!("A".repeat(61));
    let gm = gm(&[roster.clone(), roster]);
    let run = run(&source(), &gm);
    assert_eq!(
        run.result.unwrap_err(),
        BuildFailure {
            stage: Stage::DraftRoster,
            check: BuildCheck::Schema
        }
    );
    let repair = &gm.calls()[1].prompt.recent;
    assert!(
        repair.contains("characters[1].name is longer than 60 characters"),
        "{repair}"
    );
}

#[test]
fn draft_keeps_hidden_causes_out() {
    let gm = gm(&[
        roster_out(),
        seed_out(),
        knowledge(&[
            fact("Tobin Reed", "role", "harbour master", false, true),
            fact("Tobin Reed", "fear", "the sea", false, false),
        ]),
        knowledge(&[]),
        knowledge(&[]),
    ]);
    let draft = run(&source(), &gm).result.unwrap();
    assert_eq!(
        draft.knowledge,
        vec![known("Tobin Reed", "role", "harbour master", false)]
    );
    // A fact the PC does not know is not an error: no repair call.
    assert_eq!(gm.calls().len(), 5);
}

#[test]
fn one_repair_then_failure_names_stage_and_check() {
    let gm = RecordedGm::new(
        vec![
            roster_out().to_string(),
            "MARKER-7f3 this is not JSON".into(),
            "{\"start\": \"MARKER-7f3 still wrong\"}".into(),
        ],
        vec![],
    );
    let run = run(&source(), &gm);
    let failure = run.result.unwrap_err();
    assert_eq!(
        failure,
        BuildFailure {
            stage: Stage::DraftSeed,
            check: BuildCheck::Schema
        }
    );
    assert!(!format!("{failure:?}").contains("MARKER"));
    assert!(!serde_json::to_string(&failure).unwrap().contains("MARKER"));

    let calls = gm.calls();
    assert_eq!(calls.len(), 3);
    let repair = &calls[2].prompt.recent;
    assert!(repair.contains("Schema"), "{repair}");
    assert!(!repair.contains("MARKER"), "{repair}");
    // The repair call is logged like any other.
    assert_eq!(run.log.len(), 3);
}

#[test]
fn progress_reports_stage_and_counts_only() {
    let gm = gm(&plain_outputs());
    let run = run(&source(), &gm);
    run.result.unwrap();
    let seen: Vec<(Stage, u32, u32)> = run
        .progress
        .iter()
        .map(|p| {
            // Exhaustive: a new field on `Progress` breaks this line.
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
            (Stage::DraftRoster, 0, 1),
            (Stage::DraftRoster, 1, 1),
            (Stage::DraftSeed, 0, 1),
            (Stage::DraftSeed, 1, 1),
            (Stage::DraftKnowledge, 0, 3),
            (Stage::DraftKnowledge, 1, 3),
            (Stage::DraftKnowledge, 2, 3),
            (Stage::DraftKnowledge, 3, 3),
        ]
    );
}

#[test]
fn cancel_stops_between_two_calls() {
    let gm = gm(&plain_outputs());
    let watcher = gm.clone();
    let cancelled = move || !watcher.calls().is_empty();
    let run = run_until(&source(), &gm, &cancelled);
    assert_eq!(
        run.result.unwrap_err(),
        BuildFailure {
            stage: Stage::DraftSeed,
            check: BuildCheck::Cancelled
        }
    );
    assert_eq!(gm.calls().len(), 1);
}

#[test]
fn every_call_is_logged() {
    let outputs = plain_outputs();
    let gm = gm(&outputs);
    let run = run(&source(), &gm);
    let draft = run.result.unwrap();

    let calls = gm.calls();
    assert_eq!(run.log.len(), calls.len());
    for ((entry, call), output) in run.log.iter().zip(&calls).zip(&outputs) {
        assert_eq!(entry.op_id, draft.id);
        assert_eq!(entry.raw, output.to_string());
        assert!(entry.prompt.contains(&call.prompt.system));
        assert!(entry.prompt.contains(&call.prompt.recent));
    }
    let steps: Vec<&str> = run.log.iter().map(|e| e.step.as_str()).collect();
    assert_eq!(
        steps,
        vec![
            "draft_roster",
            "draft_seed",
            "draft_knowledge 1/3",
            "draft_knowledge 2/3",
            "draft_knowledge 3/3",
        ]
    );
}

#[test]
fn pending_prefix_becomes_an_open_fact() {
    let gm = gm(&[
        roster_out(),
        seed_out(),
        knowledge(&[fact(
            "Harbour Council",
            "pending_vote",
            "new pier",
            false,
            true,
        )]),
        knowledge(&[]),
        knowledge(&[]),
    ]);
    let draft = run(&source(), &gm).result.unwrap();
    assert_eq!(
        draft.knowledge,
        vec![KnownFact {
            text: "Harbour Council: pending_vote is new pier.".into(),
            ..known("Harbour Council", "vote", "new pier", true)
        }]
    );
}

#[test]
fn a_fact_stated_in_two_sections_is_kept_once() {
    let twice = knowledge(&[fact("Tobin Reed", "role", "harbour master", false, true)]);
    let gm = gm(&[
        roster_out(),
        seed_out(),
        twice.clone(),
        twice,
        knowledge(&[fact("tobin reed", "Role", "Harbour Master", false, true)]),
    ]);
    let draft = run(&source(), &gm).result.unwrap();
    assert_eq!(
        draft.knowledge,
        vec![known("Tobin Reed", "role", "harbour master", false)]
    );
}

#[test]
fn a_fact_on_another_name_of_the_pc_names_the_pc() {
    let gm = gm(&[
        roster_out(),
        seed_out(),
        knowledge(&[fact("the keeper", "duty", "the lamp", false, true)]),
        knowledge(&[]),
        knowledge(&[]),
    ]);
    let draft = run(&source(), &gm).result.unwrap();
    assert_eq!(draft.knowledge.len(), 1);
    assert_eq!(draft.knowledge[0].subject, "Mira Holt");
}

#[test]
fn a_fact_on_an_unknown_name_is_dropped_after_one_repair() {
    let section_out = knowledge(&[
        fact("Tobin Reed", "role", "harbour master", false, true),
        fact("Nobody Known", "role", "stranger", false, true),
    ]);
    let gm = gm(&[
        roster_out(),
        seed_out(),
        section_out.clone(),
        section_out,
        knowledge(&[]),
        knowledge(&[]),
    ]);
    let draft = run(&source(), &gm).result.unwrap();
    assert_eq!(
        draft.knowledge,
        vec![known("Tobin Reed", "role", "harbour master", false)]
    );
    let calls = gm.calls();
    assert_eq!(calls.len(), 6);
    assert!(calls[3].prompt.recent.contains("References"));
}

#[test]
fn a_section_that_stays_unreadable_gives_no_fact() {
    let gm = RecordedGm::new(
        vec![
            roster_out().to_string(),
            seed_out().to_string(),
            "not JSON".into(),
            "still not JSON".into(),
            knowledge(&[fact("Tobin Reed", "role", "harbour master", false, true)]).to_string(),
            knowledge(&[]).to_string(),
        ],
        vec![],
    );
    let draft = run(&source(), &gm).result.unwrap();
    assert_eq!(
        draft.knowledge,
        vec![known("Tobin Reed", "role", "harbour master", false)]
    );
}

#[test]
fn a_roster_that_lists_the_pc_is_repaired() {
    let mut with_pc = roster_out();
    with_pc["characters"]
        .as_array_mut()
        .unwrap()
        .push(npc("The Keeper", 31, true));
    let mut outputs = plain_outputs();
    outputs.insert(0, with_pc);
    let gm = gm(&outputs);
    let draft = run(&source(), &gm).result.unwrap();
    assert_eq!(draft.seed.entities.len(), 5);
    let calls = gm.calls();
    assert_eq!(calls.len(), 6);
    assert!(calls[1].prompt.recent.contains("OnePc"));
}

#[test]
fn a_roster_check_that_fails_twice_fails_the_draft() {
    // A character with no age no longer fits the roster shape: `age` is a
    // required integer, so a null age fails as Schema, not as Kinds.
    let mut ageless = roster_out();
    ageless["characters"][0]["age"] = Value::Null;
    let schema = gm(&[ageless.clone(), ageless]);
    assert_eq!(
        run(&source(), &schema).result.unwrap_err(),
        BuildFailure {
            stage: Stage::DraftRoster,
            check: BuildCheck::Schema
        }
    );

    // "Tobin Reed" knows another name of the PC, but is not in the roster.
    let without_holder = roster(
        &[npc("Ada Venn", 40, true)],
        &[thing("Gull Point", "place")],
    );
    let references = gm(&[without_holder.clone(), without_holder]);
    assert_eq!(
        run(&source(), &references).result.unwrap_err(),
        BuildFailure {
            stage: Stage::DraftRoster,
            check: BuildCheck::References
        }
    );
}

#[test]
fn a_seed_start_outside_the_roster_fails_after_one_repair() {
    // The schema allows only roster names; these checks guard a model that
    // ignores it.
    let mut elsewhere = seed_out();
    elsewhere["start"]["place"] = json!("The lighthouse stairs");
    let references = gm(&[roster_out(), elsewhere.clone(), elsewhere]);
    assert_eq!(
        run(&source(), &references).result.unwrap_err(),
        BuildFailure {
            stage: Stage::DraftSeed,
            check: BuildCheck::References
        }
    );

    let mut on_a_person = seed_out();
    on_a_person["start"]["place"] = json!("Ada Venn");
    let kinds = gm(&[roster_out(), on_a_person.clone(), on_a_person]);
    assert_eq!(
        run(&source(), &kinds).result.unwrap_err(),
        BuildFailure {
            stage: Stage::DraftSeed,
            check: BuildCheck::Kinds
        }
    );
}

#[test]
fn the_later_section_wins_a_slot() {
    let gm = gm(&[
        roster_out(),
        seed_out(),
        knowledge(&[
            fact("Tobin Reed", "location", "Gull Point", false, true),
            fact("Harbour Council", "vote", "new pier", true, true),
        ]),
        knowledge(&[
            fact("Ada Venn", "role", "clerk", false, true),
            fact("ada venn", "Role", "harbour clerk", false, true),
        ]),
        knowledge(&[
            fact("Tobin Reed", "location", "Tide Hall", false, true),
            fact("Harbour Council", "vote", "old pier", false, true),
        ]),
    ]);
    let draft = run(&source(), &gm).result.unwrap();
    // One fact per subject, key and open state: the later one, in its
    // place. An open and a settled fact are two slots (a gap later).
    assert_eq!(
        draft.knowledge,
        vec![
            known("Tobin Reed", "location", "Tide Hall", false),
            known("Harbour Council", "vote", "new pier", true),
            KnownFact {
                text: "ada venn: Role is harbour clerk.".into(),
                ..known("Ada Venn", "role", "harbour clerk", false)
            },
            known("Harbour Council", "vote", "old pier", false),
        ]
    );
}

// ---- Names by schema -------------------------------------------------------

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

/// The string values of the `enum` at `path`.
fn names_at<'a>(schema: &'a Value, path: &[&str]) -> Vec<&'a str> {
    node(schema, path)["enum"]
        .as_array()
        .unwrap_or_else(|| panic!("no enum at {path:?}: {}", node(schema, path)))
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

#[test]
fn roster_runs_before_seed() {
    let gm = gm(&plain_outputs());
    run(&source(), &gm).result.unwrap();
    let calls = gm.calls();
    let fields = |i: usize| -> Vec<String> {
        let mut keys: Vec<String> = calls[i].schema["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(fields(0), vec!["characters", "others", "pc"]);
    assert!(fields(1).contains(&"start".to_owned()), "{:?}", fields(1));
    assert!(!fields(1).contains(&"pc".to_owned()), "{:?}", fields(1));
    // The seed call receives the PC of the roster; it does not draft one.
    let seed = &calls[1].prompt.recent;
    assert!(seed.contains("Mira Holt"), "{seed}");
    assert!(seed.contains("the Keeper"), "{seed}");
}

#[test]
fn start_place_is_a_roster_enum_in_the_seed_schema() {
    let gm = gm(&plain_outputs());
    run(&source(), &gm).result.unwrap();
    let seed = &gm.calls()[1].schema;
    assert_eq!(
        names_at(seed, &["start", "place"]),
        vec!["Gull Point", "Tide Hall"]
    );
    assert_eq!(
        names_at(seed, &["start", "present", "[]"]),
        vec!["Tobin Reed", "Ada Venn"]
    );
}

#[test]
fn character_age_is_required_by_the_roster_schema() {
    let gm = gm(&plain_outputs());
    run(&source(), &gm).result.unwrap();
    let roster = &gm.calls()[0].schema;

    let character = node(roster, &["characters", "[]"]);
    assert!(
        character["required"]
            .as_array()
            .unwrap()
            .contains(&json!("age")),
        "{character}"
    );
    let age = node(roster, &["characters", "[]", "age"]);
    assert_eq!(age["type"], json!("integer"), "{age}");
    assert_eq!(age["minimum"], json!(0));
    assert_eq!(age["maximum"], json!(999));
    assert!(node(roster, &["characters", "[]", "age_stated"])["type"] == json!("boolean"));

    let other = node(roster, &["others", "[]"]);
    assert!(other["properties"].get("age").is_none(), "{other}");
    assert_eq!(
        names_at(roster, &["others", "[]", "kind"]),
        vec!["place", "faction", "item"]
    );

    let pc_age = node(roster, &["pc", "age"]);
    assert_eq!(pc_age["type"], json!("integer"), "{pc_age}");
    assert!(
        node(roster, &["pc"])["required"]
            .as_array()
            .unwrap()
            .contains(&json!("age"))
    );
}

#[test]
fn knowledge_subjects_are_a_roster_enum() {
    let gm = gm(&plain_outputs());
    run(&source(), &gm).result.unwrap();
    for call in &gm.calls()[2..] {
        assert_eq!(
            names_at(&call.schema, &["facts", "[]", "subject"]),
            vec![
                "Mira Holt",
                "Tobin Reed",
                "Ada Venn",
                "Gull Point",
                "Tide Hall",
                "Harbour Council"
            ]
        );
    }
}

/// A roster in the shape the live model gives for the fixture chronicle:
/// proposed ages, an article in a place name, objects as items.
fn chronicle_roster() -> Value {
    json!({
        "pc": {
            "name": "Teodor Brandt",
            "age": 34,
            "age_stated": true,
            "other_names": [{"name": "Wick", "known_to": ["Agathe Morrow", "Sefa Lind"]}],
            "concept": "The ferryman of Ostley Crossing."
        },
        "characters": [
            npc("Agathe Morrow", 67, true),
            npc("Sefa Lind", 19, false),
            npc("Odile Quast", 45, true),
            npc("Pim Quast", 11, true),
            {"name": "Gerrit Vane", "age": 50, "age_stated": false,
             "other_names": [{"name": "the Toll-Master", "known_to": []}]},
            npc("Ludo Brenn", 40, false),
            npc("Henrike Sall", 38, false),
            npc("Osric Fell", 52, true)
        ],
        "others": [
            thing("Ostley Crossing", "place"),
            thing("The Copper Kettle", "place"),
            thing("Harrowmere", "place"),
            thing("The Bargemen's Guild", "faction"),
            thing("The Lamplighters", "faction"),
            thing("the ferry rope", "item")
        ]
    })
}

#[test]
fn a_live_shaped_answer_with_a_start_place_phrase_still_drafts() {
    let text = include_str!("../../../fixtures/chronicle/source.md");
    // The source says "in the common room of the Copper Kettle." The seed
    // answer writes the place with an article and a final period, as the
    // live model did before the enum: it still names the roster place.
    let seed = json!({
        "premise": "A ferryman holds a sealed letter while the Guild wants his ferry.",
        "story_language": "en",
        "themes": ["debt", "trust"],
        "gm_agreement": "Slow, grounded scenes.",
        "resolution": "The dice decide uncertain actions.",
        "min_age": null,
        "start": {
            "place": "the Copper Kettle.",
            "present": ["Agathe Morrow", "Sefa Lind.", "Pim Quast"],
            "situation": "Teodor comes in wet from the last crossing.",
            "when": "The evening of the third day of the thaw"
        },
        "open_details": []
    });
    let mut outputs = vec![chronicle_roster(), seed];
    outputs.extend(std::iter::repeat_n(knowledge(&[]), 20));
    let gm = gm(&outputs);
    let mut draft = run(text, &gm).result.unwrap();

    let calls = gm.calls();
    assert!(names_at(&calls[1].schema, &["start", "place"]).contains(&"The Copper Kettle"));
    // No repair call: two draft calls, then one call per section.
    assert!(
        calls[2..]
            .iter()
            .all(|c| c.schema["properties"].get("facts").is_some())
    );
    assert_eq!(draft.seed.start.place, "The Copper Kettle");
    assert_eq!(
        draft.seed.start.present,
        vec!["Agathe Morrow", "Sefa Lind", "Pim Quast"]
    );
    assert_eq!(
        waiting(&draft),
        vec![
            "Teodor Brandt",
            "Agathe Morrow",
            "Sefa Lind",
            "Odile Quast",
            "Pim Quast",
            "Gerrit Vane",
            "Ludo Brenn",
            "Henrike Sall",
            "Osric Fell"
        ]
    );
    let stated: Vec<String> = sources(&draft)
        .into_iter()
        .filter(|(_, s)| *s == AgeSource::FromText)
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        stated,
        vec![
            "Teodor Brandt",
            "Agathe Morrow",
            "Odile Quast",
            "Pim Quast",
            "Osric Fell"
        ]
    );
    confirm_all(&mut draft);
    assert_eq!(gaps(&draft), vec![]);
}

#[test]
fn the_seed_prompt_shows_other_names_of_places_and_characters() {
    let mut roster = roster_out();
    roster["characters"][0]["other_names"] =
        json!([{"name": "the Harbour Master", "known_to": []}]);
    roster["others"][0]["other_names"] = json!([{"name": "the Point", "known_to": []}]);
    let mut outputs = plain_outputs();
    outputs[0] = roster;
    let gm = gm(&outputs);
    run(&source(), &gm).result.unwrap();
    let call = &gm.calls()[1];
    let recent = &call.prompt.recent;
    assert!(
        recent.contains("- Tobin Reed; also called the Harbour Master\n"),
        "{recent}"
    );
    assert!(recent.contains("- Ada Venn\n"), "{recent}");
    assert!(
        recent.contains("- Gull Point; also called the Point\n"),
        "{recent}"
    );
    // The enums still hold main names only.
    assert_eq!(
        names_at(&call.schema, &["start", "place"]),
        vec!["Gull Point", "Tide Hall"]
    );
    assert_eq!(
        names_at(&call.schema, &["start", "present", "[]"]),
        vec!["Tobin Reed", "Ada Venn"]
    );
}

// ---- Review: the Source Text length, cut lists, an added character ---------

#[test]
fn short_source_text_is_refused_before_any_call() {
    assert_eq!(SOURCE_TEXT_MIN_CHARS, 400);
    let short = "a".repeat(SOURCE_TEXT_MIN_CHARS - 1);
    assert_eq!(
        check_source_text(&short),
        Err(SourceTextProblem::TooShort { min: 400 })
    );
    // Blanks at the ends do not count.
    let padded = format!("  {short}\n\n  ");
    assert_eq!(
        check_source_text(&padded),
        Err(SourceTextProblem::TooShort { min: 400 })
    );
    assert_eq!(check_source_text(""), Err(SourceTextProblem::Empty));
    assert_eq!(check_source_text(" \n\t"), Err(SourceTextProblem::Empty));
    assert_eq!(
        check_source_text("hello"),
        Err(SourceTextProblem::TooShort { min: 400 })
    );
    let long = "a".repeat(SOURCE_TEXT_MAX_CHARS + 1);
    assert_eq!(
        check_source_text(&long),
        Err(SourceTextProblem::TooLong { max: 40_000 })
    );

    for text in [short.as_str(), "", "hello"] {
        let gm = gm(&[]);
        let run = run(text, &gm);
        assert_eq!(
            run.result.unwrap_err(),
            BuildFailure {
                stage: Stage::DraftSeed,
                check: BuildCheck::Completeness
            }
        );
        assert!(gm.calls().is_empty(), "{text}");
        assert!(run.log.is_empty());
    }

    // 400 characters pass the check and reach the model, which has no
    // recorded answer here.
    let at_limit = "é".repeat(SOURCE_TEXT_MIN_CHARS);
    assert_eq!(check_source_text(&at_limit), Ok(()));
    let gm = gm(&[]);
    run(&at_limit, &gm).result.unwrap_err();
    assert_eq!(gm.calls().len(), 1);
}

/// Both limits count the text without the blanks at its ends.
#[test]
fn source_text_limits_count_the_trimmed_text() {
    let chars = |n: usize| "é".repeat(n);
    let too_short = Err(SourceTextProblem::TooShort { min: 400 });
    let too_long = Err(SourceTextProblem::TooLong { max: 40_000 });
    assert_eq!(check_source_text(&chars(399)), too_short);
    assert_eq!(check_source_text(&chars(400)), Ok(()));
    assert_eq!(check_source_text(&chars(40_000)), Ok(()));
    assert_eq!(check_source_text(&chars(40_001)), too_long);
    // Two trailing newlines do not push a full text over the limit.
    assert_eq!(check_source_text(&format!("{}\n\n", chars(40_000))), Ok(()));
    assert_eq!(
        check_source_text(&format!("  {}  ", chars(40_001))),
        too_long
    );
}

#[test]
fn cut_lists_are_reported() {
    // 31 characters: the roster schema caps the list at 30.
    let mut characters = vec![npc("Tobin Reed", 58, true)];
    characters.extend((1..=30).map(|n| npc(&format!("Sailor {n}"), 30, false)));
    let mut outputs = plain_outputs();
    outputs[0] = roster(
        &characters,
        &[thing("Gull Point", "place"), thing("Tide Hall", "place")],
    );
    let first = gm(&outputs);
    let draft = run(&source(), &first).result.unwrap();
    assert_eq!(
        draft.cut,
        vec![Cut {
            at: "entities".into(),
            cap: 30
        }]
    );
    assert_eq!(
        draft
            .seed
            .entities
            .iter()
            .filter(|e| e.kind == EntityKind::Npc)
            .count(),
        30
    );

    // A list of other names and a section's facts, in the path form of
    // `Gap.at`; an entry of `others` comes after the characters.
    let mut outputs = plain_outputs();
    outputs[0]["others"][1]["other_names"] = json!(
        (1..=5)
            .map(|n| json!({"name": format!("the Hall {n}"), "known_to": []}))
            .collect::<Vec<_>>()
    );
    let facts: Vec<Value> = (0..41)
        .map(|n| fact("Gull Point", &format!("detail_{n}"), "stone", false, true))
        .collect();
    outputs[3] = knowledge(&facts);
    let gm = gm(&outputs);
    let draft = run(&source(), &gm).result.unwrap();
    assert_eq!(
        draft.cut,
        vec![
            Cut {
                at: "entities[3].other_names".into(),
                cap: 4
            },
            Cut {
                at: "knowledge".into(),
                cap: 40
            },
        ]
    );
    // The review keeps the report.
    let mut edited = draft.clone();
    apply_edit(&mut edited, draft.seed.clone(), vec![]);
    assert_eq!(edited.cut, draft.cut);
}

fn hidden_out() -> Value {
    json!({ "facts": [] })
}

fn npc_out(name: &str) -> Value {
    json!({
        "goals": [{"text": format!("{name} wants a quiet winter."), "against": null}],
        "secret": null,
        "attitudes": [],
        "voice_card": {
            "habits": format!("{name} speaks in short lines."),
            "vocabulary": "tide, rope, lamp",
            "samples": ["Mind the rope.", "The tide turns soon."]
        }
    })
}

#[test]
fn added_character_reaches_the_world_build() {
    let text = source();
    let gm_draft = gm(&plain_outputs());
    let mut draft = run(&text, &gm_draft).result.unwrap();

    // The Player adds a character the draft missed, and saves.
    let mut seed = draft.seed.clone();
    seed.entities.push(SeedEntity {
        name: "Nell Quay".into(),
        kind: EntityKind::Npc,
        age: Some(44),
        other_names: vec![],
        hidden: false,
    });
    let knowledge = draft.knowledge.clone();
    apply_edit(&mut draft, seed, knowledge);
    let added = age_lines(&draft).pop().unwrap();
    assert_eq!(added.name, "Nell Quay");
    assert_eq!(added.source, AgeSource::Edited);
    assert!(!added.confirmed);
    assert!(approve(&mut draft).is_err());
    confirm_all(&mut draft);
    assert_eq!(approve(&mut draft), Ok(()));

    let outputs = [
        hidden_out(),
        hidden_out(),
        hidden_out(),
        npc_out("Tobin Reed"),
        npc_out("Ada Venn"),
        npc_out("Nell Quay"),
        json!({"events": []}),
        json!({"placements": [
            {"npc": "Ada Venn", "place": "Tide Hall"},
            {"npc": "Nell Quay", "place": "Tide Hall"}
        ]}),
    ];
    let gm_build = gm(&outputs);
    let mut progress = |_| {};
    let mut log = |_| {};
    let mut ctl = Control::new(&mut progress, &|| false, &mut log);
    let world = build_world(&draft, &gm_build, &mut ctl).unwrap();
    let calls = gm_build.calls();
    let named: Vec<_> = calls
        .iter()
        .filter(|c| c.prompt.recent.contains("NPC TO PREPARE\nNell Quay"))
        .collect();
    assert_eq!(named.len(), 1);
    let voices: Vec<&str> = world.voice_cards.iter().map(|v| v.npc.as_str()).collect();
    assert_eq!(voices, ["Tobin Reed", "Ada Venn", "Nell Quay"]);
}

/// B6 (QA, 2026-11-10): the seed stage asks for the start hour, and the
/// draft keeps it. An hour outside 0 to 23 is dropped.
#[test]
fn the_seed_stage_reads_the_start_hour() {
    for (hour, kept) in [
        (json!(19), Some(19)),
        (json!(null), None),
        (json!(30), None),
    ] {
        let mut outputs = plain_outputs();
        outputs[1]["start"]["hour"] = hour;
        let gm = gm(&outputs);
        let draft = run(&source(), &gm).result.unwrap();
        assert_eq!(draft.seed.start.hour, kept);
        let call = &gm.calls()[1];
        let schema = node(&call.schema, &["start", "hour"]);
        assert_eq!(schema["maximum"], json!(23), "{schema}");
        let prompt = format!(
            "{}{}{}{}",
            call.prompt.system, call.prompt.stable, call.prompt.scene, call.prompt.recent
        );
        assert!(prompt.contains("hour: the hour of the day"), "{prompt}");
    }
}

/// C12 (review, 2026-11-10): one known sentence shows once. QA saw "Agathe
/// Morrow and Sefa Lind know that Wick is Teodor." twice, once per subject.
/// The draft keeps the first fact whose sentence, normalized, is the same.
#[test]
fn a_known_sentence_shows_once_in_the_draft() {
    let shared = |subject: &str, key: &str| {
        json!({
            "subject": subject,
            "key": key,
            "value": "knows the Keeper is Mira",
            "text": "Tobin Reed and Ada Venn know that the Keeper is Mira.",
            "open": false,
            "pc_knows": true
        })
    };
    let mut outputs = plain_outputs();
    outputs[2] = knowledge(&[
        shared("Tobin Reed", "knows_keeper"),
        shared("Ada Venn", "knows_keeper"),
        fact("Tobin Reed", "role", "boatman", false, true),
    ]);
    // The same sentence again in a later section, spelled with other case
    // and spaces, about the same subject on another key.
    let mut again = shared("Tobin Reed", "secret");
    again["text"] = json!("tobin reed and  Ada Venn know that the keeper is Mira.");
    outputs[3] = knowledge(&[again]);
    let gm = gm(&outputs);
    let draft = run(&source(), &gm).result.unwrap();
    let texts: Vec<&str> = draft.knowledge.iter().map(|f| f.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Tobin Reed and Ada Venn know that the Keeper is Mira.",
            "Tobin Reed: role is boatman."
        ]
    );
}

/// C12: the knowledge prompt says that a statement about the document
/// itself is not a fact.
#[test]
fn the_knowledge_prompt_rules_out_the_document_itself() {
    let gm = gm(&plain_outputs());
    run(&source(), &gm).result.unwrap();
    let calls = gm.calls();
    let knowledge = calls
        .iter()
        .find(|c| c.prompt.recent.contains("SECTION 1 OF"))
        .unwrap();
    assert!(
        knowledge
            .prompt
            .system
            .contains("A statement about the document itself is not a fact"),
        "{}",
        knowledge.prompt.system
    );
}

/// Bug pass 4, B2: a fact the PC knows, filed under a place whose name its
/// text never says, gets one repair. QA's draft filed "The reeve's inquiry
/// into the missing toll money is pending." under the place "the stone
/// quay", and the place's card showed that line alone. A person's fact may
/// say "she"; a place's, a Faction's or a thing's names it.
#[test]
fn a_place_fact_whose_text_does_not_name_the_place_is_repaired() {
    let filed = |subject: &str, text: &str| {
        json!({
            "subject": subject, "key": "vote", "value": "pending",
            "text": text, "open": true, "pc_knows": true
        })
    };
    let first = knowledge(&[
        filed("Tide Hall", "The vote on the new pier is pending."),
        // Named, so it stays: the article and the case do not matter.
        filed("Gull Point", "The lamp of gull point is to be repaired."),
        // A pronoun that opens the sentence stands for the place.
        filed("Tide Hall", "Its doors stay shut until the vote."),
    ]);
    let repaired = knowledge(&[
        filed(
            "Harbour Council",
            "The Harbour Council's vote on the new pier is pending.",
        ),
        filed("Gull Point", "The lamp of gull point is to be repaired."),
        filed("Tide Hall", "Its doors stay shut until the vote."),
    ]);
    let gm = gm(&[
        roster_out(),
        seed_out(),
        first,
        repaired,
        knowledge(&[]),
        knowledge(&[]),
    ]);
    let draft = run(&source(), &gm).result.unwrap();
    let subjects: Vec<&str> = draft.knowledge.iter().map(|k| k.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["Harbour Council", "Gull Point", "Tide Hall"],
        "{:?}",
        draft.knowledge
    );
    let calls = gm.calls();
    let repair = &calls[3].prompt.recent;
    assert!(
        repair.contains("facts[0].text does not name its subject \"Tide Hall\""),
        "{repair}"
    );
    assert!(!repair.contains("facts[2]"), "{repair}");
}
