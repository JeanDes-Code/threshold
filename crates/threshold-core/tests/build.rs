//! `draft_from_text`: the three draft stages, their checks and the stage
//! runner, replayed with `RecordedGm` (data shape 2 and 3.5, design 6.2).

use serde_json::{Value, json};
use threshold_core::{
    RecordedGm,
    build::{
        BuildCheck, BuildFailure, Control, DebugEntry, DraftState, KnownFact, Progress,
        SOURCE_TEXT_MAX_CHARS, SeedDraft, Stage, draft_from_text, gaps,
    },
    domain::{EntityKind, OpenMode},
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
        let mut ctl = Control {
            progress: &mut on_progress,
            cancelled,
            log: &mut on_log,
        };
        draft_from_text(text, gm, &mut ctl)
    };
    Run {
        result,
        progress,
        log,
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
        "pc": {
            "name": "Mira Holt",
            "age": 31,
            "age_stated": true,
            "other_names": [{"name": "the Keeper", "known_to": ["Mira Holt", "Tobin Reed"]}],
            "concept": "A careful lighthouse keeper."
        },
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

fn npc(name: &str, age: u16, stated: bool) -> Value {
    json!({"name": name, "kind": "npc", "age": age, "age_stated": stated, "other_names": []})
}

fn thing(name: &str, kind: &str) -> Value {
    json!({"name": name, "kind": kind, "age": null, "age_stated": false, "other_names": []})
}

fn roster(entities: &[Value]) -> Value {
    json!({ "entities": entities })
}

fn roster_out() -> Value {
    roster(&[
        npc("Tobin Reed", 58, true),
        npc("Ada Venn", 40, false),
        thing("Gull Point", "place"),
        thing("Tide Hall", "place"),
        thing("Harbour Council", "faction"),
    ])
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
        seed_out(),
        roster_out(),
        knowledge(&[]),
        knowledge(&[]),
        knowledge(&[]),
    ]
}

#[test]
fn empty_or_long_source_is_refused_before_any_call() {
    assert_eq!(SOURCE_TEXT_MAX_CHARS, 60_000);
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
    // reaches the model, which has no recorded answer.
    let at_limit = "é".repeat(SOURCE_TEXT_MAX_CHARS);
    let gm = gm(&[]);
    let run = run(&at_limit, &gm);
    assert_eq!(gm.calls().len(), 1);
    assert_eq!(
        run.result.unwrap_err(),
        BuildFailure {
            stage: Stage::DraftSeed,
            check: BuildCheck::Model
        }
    );
}

#[test]
fn draft_holds_seed_roster_and_known_facts() {
    let text = source();
    let gm = gm(&[
        seed_out(),
        roster_out(),
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
    assert_eq!(draft.ages_to_confirm, vec!["Ada Venn"]);
    // The PC knows every other name that the draft lists.
    assert_eq!(
        seed.pc.other_names[0].known_to,
        vec!["Mira Holt", "Tobin Reed"]
    );

    // The draft passes its own review once the proposed age is confirmed.
    let mut confirmed = draft.clone();
    confirmed.ages_to_confirm.clear();
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
    let mut seed = seed_out();
    seed["pc"]["age_stated"] = json!(false);
    let gm = gm(&[
        seed,
        roster_out(),
        knowledge(&[]),
        knowledge(&[]),
        knowledge(&[]),
    ]);
    let draft = run(&source(), &gm).result.unwrap();
    assert_eq!(draft.ages_to_confirm, vec!["Mira Holt", "Ada Venn"]);
}

#[test]
fn draft_keeps_hidden_causes_out() {
    let gm = gm(&[
        seed_out(),
        roster_out(),
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
            seed_out().to_string(),
            "MARKER-7f3 this is not JSON".into(),
            "{\"entities\": \"MARKER-7f3 still wrong\"}".into(),
        ],
        vec![],
    );
    let run = run(&source(), &gm);
    let failure = run.result.unwrap_err();
    assert_eq!(
        failure,
        BuildFailure {
            stage: Stage::DraftRoster,
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
            let Progress { stage, done, total } = *p;
            (stage, done, total)
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            (Stage::DraftSeed, 0, 1),
            (Stage::DraftSeed, 1, 1),
            (Stage::DraftRoster, 0, 1),
            (Stage::DraftRoster, 1, 1),
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
            stage: Stage::DraftRoster,
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
            "draft_seed",
            "draft_roster",
            "draft_knowledge 1/3",
            "draft_knowledge 2/3",
            "draft_knowledge 3/3",
        ]
    );
}

#[test]
fn pending_prefix_becomes_an_open_fact() {
    let gm = gm(&[
        seed_out(),
        roster_out(),
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
        seed_out(),
        roster_out(),
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
        seed_out(),
        roster_out(),
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
        seed_out(),
        roster_out(),
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
            seed_out().to_string(),
            roster_out().to_string(),
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
    with_pc["entities"]
        .as_array_mut()
        .unwrap()
        .push(npc("The Keeper", 31, true));
    let mut outputs = plain_outputs();
    outputs.insert(1, with_pc);
    let gm = gm(&outputs);
    let draft = run(&source(), &gm).result.unwrap();
    assert_eq!(draft.seed.entities.len(), 5);
    let calls = gm.calls();
    assert_eq!(calls.len(), 6);
    assert!(calls[2].prompt.recent.contains("OnePc"));
}

#[test]
fn a_roster_check_that_fails_twice_fails_the_draft() {
    let ageless = roster(&[
        json!({"name": "Tobin Reed", "kind": "npc", "age": null, "age_stated": false, "other_names": []}),
        thing("Gull Point", "place"),
    ]);
    let kinds = gm(&[seed_out(), ageless.clone(), ageless]);
    assert_eq!(
        run(&source(), &kinds).result.unwrap_err(),
        BuildFailure {
            stage: Stage::DraftRoster,
            check: BuildCheck::Kinds
        }
    );

    // "Tobin Reed" is present at the start and in a `known_to`, but absent.
    let without_present = roster(&[thing("Gull Point", "place")]);
    let references = gm(&[seed_out(), without_present.clone(), without_present]);
    assert_eq!(
        run(&source(), &references).result.unwrap_err(),
        BuildFailure {
            stage: Stage::DraftRoster,
            check: BuildCheck::References
        }
    );
}

#[test]
fn the_later_section_wins_a_slot() {
    let gm = gm(&[
        seed_out(),
        roster_out(),
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
