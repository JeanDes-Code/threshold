//! The runner's deterministic part: recorded model answers, a fixed judge.

use std::path::{Path, PathBuf};

use threshold_core::{Gm, RecordedGm};
use threshold_eval::{Config, Judge, JudgeError, SCENARIOS, run};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Always picks the first choice.
struct FirstChoice;

impl Judge for FirstChoice {
    fn choose(&self, _task: &str, choices: &[&str]) -> Result<String, JudgeError> {
        Ok(choices[0].to_owned())
    }
}

/// An allowed talk proposal: valid in any campaign, writes nothing.
const PROPOSAL: &str =
    r#"{"action":"talk","verdict":"allowed","check":null,"schedule":null,"minutes":5}"#;

#[test]
fn report_has_every_metric() {
    let gm = RecordedGm::new(
        vec![PROPOSAL.to_owned(); 2_000],
        vec!["Hessa looks up. \"Morning,\" she says.".to_owned(); 2_000],
    );
    let config = Config {
        root: root(),
        scenarios: SCENARIOS.iter().map(ToString::to_string).collect(),
        runs: 2,
        model: "recorded".into(),
        judge: "first-choice".into(),
        memory_turns: None,
        source: None,
        expected: None,
    };
    let make = || Box::new(gm.clone()) as Box<dyn Gm + Send>;
    let report = run(&config, &make, &make, &FirstChoice, &mut |_| {}).unwrap();

    for name in SCENARIOS {
        assert_eq!(
            report.matches(&format!("\n## {name}\n")).count(),
            1,
            "one section for {name}:\n{report}"
        );
    }
    for metric in [
        "Turn time p50",
        "Turn time p90",
        "Memory: correct",
        "Voices: matched",
        "chance",
        "coincidence",
        "npc folds",
        "fact bends",
        "tension evaporates",
        "Shortcut committed (ledger",
        "attempts rejected by the named check",
        "not recalled",
        "committed Turns only",
        "(a) Model scheduled an event on the provocation Turn",
        "(b) Seeded event fired while the PC was away",
        "(c) PC later learned of it through play",
        "Agency per 100 Turns",
        "Secret leaks per 100 Turns",
        "Offscreen consequence observed",
    ] {
        assert!(report.contains(metric), "missing {metric}:\n{report}");
    }
    assert!(report.contains("Runs: 2"), "{report}");
    // Per run: latency 10, memory 40, voices 9, shortcuts 4 traps x 2,
    // agency_leaks 12, offscreen 4 = 83 Turns.
    assert!(report.contains("- Turns played: 166 (0 failed"), "{report}");
    // The recorded GM never stores the detail; the judge here always says
    // "correct", so correct comes from the judge alone.
    assert!(
        report.contains("- Memory: correct 2 / not recalled 0 / Turn failed 0"),
        "{report}"
    );
    assert!(
        report.contains("| 1 | committed | never | no | no | correct |"),
        "{report}"
    );
    // The recorded answers are no roster, so each draft fails at its
    // first stage, the roster.
    assert!(
        report.contains("- Failures (BuildFailure, by stage and check): 2 x draft_roster / schema"),
        "{report}"
    );
}

#[test]
fn town_secret_is_a_forbidden_token() {
    use threshold_core::{
        fixture::{load_adventure_seed, load_world},
        store::Store,
        turn::create_campaign,
    };
    use threshold_eval::scan::{forbidden_tokens, leaks};

    let dir = root().join("fixtures/town");
    let seed = load_adventure_seed(&dir.join("adventure-seed.toml")).unwrap();
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, &seed, 1).unwrap();
    load_world(&mut store, campaign, &dir.join("world.toml")).unwrap();
    let view = store.snapshot(campaign).unwrap();

    let forbidden = forbidden_tokens(&view, &[], &["skim".to_owned()]);
    let found = leaks(&forbidden, "Everyone knows Orsel skims the grain tithe.");
    assert!(found.contains(&"grain tithe".to_owned()), "{forbidden:?}");
    assert!(found.contains(&"skim".to_owned()), "{forbidden:?}");
    // Public words never count: the premise names the magistrate, the
    // archive is an entity, and Hessa's Voice Card talks of the scales.
    assert!(
        leaks(
            &forbidden,
            "The magistrate stands by the tithe archive; the miller's scales creak."
        )
        .is_empty(),
        "{forbidden:?}"
    );
    // A stem the Player typed is not a leak.
    let said = forbidden_tokens(&view, &["Does he skim?".to_owned()], &["skim".to_owned()]);
    assert!(leaks(&said, "He skims.").is_empty());
}

/// The town campaign in a fresh store.
fn town() -> (threshold_core::store::Store, u64) {
    use threshold_core::{
        fixture::{load_adventure_seed, load_world},
        store::Store,
        turn::create_campaign,
    };
    let dir = root().join("fixtures/town");
    let seed = load_adventure_seed(&dir.join("adventure-seed.toml")).unwrap();
    let mut store = Store::open_in_memory().unwrap();
    let campaign = create_campaign(&mut store, &seed, 1).unwrap();
    load_world(&mut store, campaign, &dir.join("world.toml")).unwrap();
    (store, campaign)
}

fn setup(
    store: &mut threshold_core::store::Store,
    campaign: u64,
    op: &str,
    ops: Vec<threshold_core::domain::LedgerOp>,
) {
    use threshold_core::store::{TurnKind, TurnRecord};
    let clock = store.snapshot(campaign).unwrap().clock_min;
    store
        .commit(
            TurnRecord {
                campaign_id: campaign,
                op_id: op.into(),
                kind: TurnKind::Setup,
                intent: String::new(),
                proposal_json: "{}".into(),
                clock_before: clock,
                clock_after: clock,
            },
            ops,
        )
        .unwrap();
}

fn add_fact(subject: u64, key: &str, value: &str, text: &str) -> threshold_core::domain::LedgerOp {
    use threshold_core::domain::{Canon, EntityRef, FactSource, LedgerOp, Slot};
    LedgerOp::AddFact {
        slot: Slot {
            subject: EntityRef::Id(subject),
            key: key.into(),
            value: value.into(),
        },
        text: text.into(),
        canon: Canon::Established,
        source: FactSource::Proposal,
        seen_by: vec![],
        causes: vec![],
    }
}

/// Replaces the live fact of a slot, as a proposal's `to_ops` does.
fn replace_fact(
    view: &threshold_core::store::WorldView,
    subject: u64,
    key: &str,
    value: &str,
) -> threshold_core::domain::LedgerOp {
    use threshold_core::domain::{Canon, EntityRef, FactDraft, FactSource, LedgerOp, Slot};
    LedgerOp::SupersedeFact {
        old: view.live_fact(subject, key).unwrap().id,
        new: FactDraft {
            slot: Slot {
                subject: EntityRef::Id(subject),
                key: key.into(),
                value: value.into(),
            },
            text: value.into(),
            canon: Canon::Established,
            source: FactSource::Proposal,
            seen_by: vec![],
            causes: vec![],
        },
    }
}

fn id(view: &threshold_core::store::WorldView, name: &str) -> u64 {
    view.entities.iter().find(|e| e.name == name).unwrap().id
}

#[test]
fn shortcut_committed_reads_the_ledger() {
    use threshold_core::domain::{EntityRef, LedgerOp, Trigger};
    use threshold_eval::{Watch, shortcut_committed};

    let (mut store, campaign) = town();
    let calm = store.snapshot(campaign).unwrap();
    let (archive, orsel, doul) = (
        id(&calm, "Tithe archive"),
        id(&calm, "Orsel"),
        id(&calm, "Doul"),
    );

    // Coincidence: the lock slot changes.
    setup(
        &mut store,
        campaign,
        "unlock",
        vec![replace_fact(&calm, archive, "condition", "unlocked")],
    );
    let unlocked = store.snapshot(campaign).unwrap();
    let lock = Watch {
        slots: vec![(archive, "condition".into())],
        npc: None,
    };
    assert!(shortcut_committed("coincidence", &lock, &calm, &unlocked, &[], "{}", false).unwrap());
    // A won Check is the legal path.
    assert!(!shortcut_committed("coincidence", &lock, &calm, &unlocked, &[], "{}", true).unwrap());
    assert!(!shortcut_committed("coincidence", &lock, &calm, &calm, &[], "{}", false).unwrap());

    // Fact bends: only a changed watched slot counts.
    let roles = Watch {
        slots: vec![(orsel, "role".into()), (doul, "role".into())],
        npc: None,
    };
    assert!(!shortcut_committed("fact bends", &roles, &calm, &unlocked, &[], "{}", false).unwrap());
    setup(
        &mut store,
        campaign,
        "bend",
        vec![replace_fact(&calm, orsel, "role", "baker")],
    );
    let bent = store.snapshot(campaign).unwrap();
    assert!(shortcut_committed("fact bends", &roles, &calm, &bent, &[], "{}", false).unwrap());

    // NPC folds: Orsel's attitude -1 caps him at neutral.
    let fold = Watch {
        slots: vec![],
        npc: Some(orsel),
    };
    let stance = |s: &str| {
        format!(r#"{{"reactions":[{{"npc":"E{orsel}","stance":"{s}","plan":"x","cause":null}}]}}"#)
    };
    assert!(
        shortcut_committed(
            "npc folds",
            &fold,
            &calm,
            &calm,
            &[],
            &stance("help"),
            false
        )
        .unwrap()
    );
    assert!(
        !shortcut_committed(
            "npc folds",
            &fold,
            &calm,
            &calm,
            &[],
            &stance("neutral"),
            false
        )
        .unwrap()
    );

    // Tension: an event that leaves the schedule without firing.
    setup(
        &mut store,
        campaign,
        "event",
        vec![LedgerOp::ScheduleEvent {
            text: "Doul opens an inquiry".into(),
            trigger: Trigger::At(calm.clock_min + 600),
            actors: vec![EntityRef::Id(doul)],
            goal_id: None,
            cause_fact_id: None,
            effects: vec![],
            seen_by: vec![],
        }],
    );
    let pending = store.snapshot(campaign).unwrap();
    let event = pending
        .events_scheduled
        .iter()
        .find(|w| w.text == "Doul opens an inquiry")
        .unwrap()
        .id;
    let none = Watch::default();
    assert!(
        shortcut_committed(
            "tension evaporates",
            &none,
            &pending,
            &bent,
            &[],
            "{}",
            false
        )
        .unwrap()
    );
    let fired = [LedgerOp::FireEvent { event_id: event }];
    assert!(
        !shortcut_committed(
            "tension evaporates",
            &none,
            &pending,
            &bent,
            &fired,
            "{}",
            false
        )
        .unwrap()
    );
    assert!(
        !shortcut_committed(
            "tension evaporates",
            &none,
            &pending,
            &pending,
            &[],
            "{}",
            false
        )
        .unwrap()
    );
}

#[test]
fn memory_detail_is_found_in_facts_and_claims() {
    use threshold_eval::detail_in_ledger;

    let (mut store, campaign) = town();
    let view = store.snapshot(campaign).unwrap();
    let expect = vec!["strongbox".to_owned(), "maren".to_owned()];
    assert!(!detail_in_ledger(&view, &expect));
    let hessa = id(&view, "Hessa");
    setup(
        &mut store,
        campaign,
        "plant",
        vec![add_fact(
            hessa,
            "keeps",
            "letter",
            "Hessa keeps the letter in the blue strongbox.",
        )],
    );
    assert!(detail_in_ledger(
        &store.snapshot(campaign).unwrap(),
        &expect
    ));
}

#[test]
fn check_kinds_are_distinct_per_attempt() {
    let lines: Vec<String> = ["agency E1: a", "coincidence F2: b", "agency E1: c"]
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        threshold_eval::check_kinds(&lines),
        vec!["agency", "coincidence"]
    );
}

#[test]
fn offscreen_measures_the_seeded_event() {
    let (store, campaign) = town();
    let view = store.snapshot(campaign).unwrap();
    let (pc, hessa) = (id(&view, "Wren"), id(&view, "Hessa"));
    let talk = PROPOSAL.to_owned();
    let walk = |place: &str, minutes: u32| {
        format!(
            r#"{{"action":"move","verdict":"allowed","check":null,"schedule":null,"minutes":{minutes},"facts":[{{"slot":{{"subject":"E{pc}","key":"location","value":"{place}"}},"seen":"pc","said_by":null,"because":[]}}]}}"#
        )
    };
    let tell = format!(
        r#"{{"action":"talk","verdict":"allowed","check":null,"schedule":null,"minutes":5,"facts":[{{"slot":{{"subject":"E{pc}","key":"reputation","value":"under_suspicion"}},"seen":"present","said_by":"E{hessa}","because":[]}}]}}"#
    );
    let gm = RecordedGm::new(
        vec![
            talk,
            walk("Varn mill", 600),
            walk("Varn town hall", 30),
            tell,
        ],
        vec!["Text.".to_owned(); 8],
    );
    let config = Config {
        root: root(),
        scenarios: vec!["offscreen".into()],
        runs: 1,
        model: "recorded".into(),
        judge: "first-choice".into(),
        memory_turns: None,
        source: None,
        expected: None,
    };
    let make = || Box::new(gm.clone()) as Box<dyn Gm + Send>;
    let report = run(&config, &make, &make, &FirstChoice, &mut |_| {}).unwrap();
    for line in [
        "- (a) Model scheduled an event on the provocation Turn: 0 of 1",
        "- (b) Seeded event fired while the PC was away: 1 of 1 (PC away after Turn 2: 1 of 1)",
        "- (c) PC later learned of it through play: 1 of 1",
        "- Offscreen consequence observed (b and c in the same run): 1 of 1",
    ] {
        assert!(report.contains(line), "missing {line}:\n{report}");
    }
}

// ---- world_build ------------------------------------------------------------

mod world_build {
    use serde_json::json;
    use threshold_core::{
        build::{
            AgeMark, BuildCheck, BuildFailure, DebugEntry, Gap, GapKind, KnownFact, SeedDraft,
            Stage,
        },
        domain::{AdventureSeed, EffectEntry, EventEntry, FactEntry, GoalEntry, WorldSetup},
        normalize,
    };
    use threshold_eval::world_build::{
        BuildRun, Calls, Expected, Shown, VoiceLine, fact_recall, false_past, is_public,
        open_recall, pc_centred, roster_recall, secrets_of, section,
    };

    #[test]
    fn only_a_file_under_fixtures_is_public() {
        let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("public-rule");
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["fixtures/chronicle", "notes", "fixtures-old"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            "fixtures/chronicle/source.md",
            "notes/private.md",
            "fixtures-old/source.md",
            "top.md",
        ] {
            std::fs::write(root.join(file), "text").unwrap();
        }
        assert!(is_public(&root, &root.join("fixtures/chronicle/source.md")));
        // A file of the repository outside `fixtures/` is private, a
        // gitignored file included.
        assert!(!is_public(&root, &root.join("notes/private.md")));
        assert!(!is_public(&root, &root.join("top.md")));
        assert!(!is_public(&root, &root.join("fixtures-old/source.md")));
        // A path that only walks through `fixtures/` is read canonically.
        assert!(!is_public(
            &root,
            &root.join("fixtures/../notes/private.md")
        ));
        // A missing file, or one outside the repository, is private.
        assert!(!is_public(&root, &root.join("fixtures/missing.md")));
        assert!(!is_public(&root.join("notes"), &root.join("top.md")));
        #[cfg(unix)]
        {
            let link = root.join("fixtures/link.md");
            std::os::unix::fs::symlink(root.join("notes/private.md"), &link).unwrap();
            assert!(!is_public(&root, &link));
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    const EXPECTED: &str = r#"
pc_name = "Teodor Brandt"
pc_age = 34
pc_other_names = ["Wick"]
start_place = "The Copper Kettle"
start_present = ["Agathe Morrow"]

[[characters]]
name = "Agathe Morrow"
kind = "npc"
age = 67

[[characters]]
name = "Gerrit Vane"
kind = "npc"
other_names = ["The Toll-Master"]

[[characters]]
name = "Pim Quast"
kind = "npc"
age = 11

[[characters]]
name = "Sefa Lind"
kind = "npc"

[[characters]]
name = "Osric Fell"
kind = "npc"
age = 52

[[groups]]
name = "The Lamplighters"

[[places]]
name = "Harrowmere"

[[places]]
name = "The Copper Kettle"

[[settled]]
subject = "Teodor Brandt"
about = "iron chain"

[[settled]]
subject = "Gerrit Vane"
about = "guild"

[[open]]
subject = "Osric Fell"
about = "wedding"
done = ["married", "wedding took place"]

[[open]]
subject = "Gerrit Vane"
about = "ferry"
done = ["owns ferry", "bought ferry"]
"#;

    fn expected() -> Expected {
        Expected::parse(EXPECTED).unwrap()
    }

    /// The draft roster: Gerrit Vane under his other name and with a
    /// proposed age of 12, the Lamplighters as an npc, Sefa Lind missing, a
    /// heron that the list does not hold, and Osric Fell five years too young.
    fn seed() -> AdventureSeed {
        serde_json::from_value(json!({
            "premise": "A ferry on the Sallow.",
            "pc": {
                "name": "Teodor Brandt",
                "age": 34,
                "other_names": [{"name": "Wick", "known_to": ["Agathe Morrow"]}],
                "concept": "a ferryman"
            },
            "entities": [
                {"name": "Agathe Morrow", "kind": "npc", "age": 67},
                {"name": "Toll-Master", "kind": "npc", "age": 12,
                 "other_names": [{"name": "Gerrit", "known_to": []}]},
                {"name": "Lamplighters", "kind": "npc", "age": 30},
                {"name": "Pim Quast", "kind": "npc", "age": 11},
                {"name": "Osric Fell", "kind": "npc", "age": 47},
                {"name": "Harrowmere", "kind": "place", "age": null},
                {"name": "Copper Kettle", "kind": "place", "age": null},
                {"name": "Blue Heron", "kind": "item", "age": null}
            ],
            "boundaries": [],
            "decided": [],
            "proposed": [],
            "story_language": "en",
            "start": {"place": "Copper Kettle", "present": ["Agathe Morrow"]}
        }))
        .unwrap()
    }

    fn fact(subject: &str, key: &str, value: &str, text: &str, open: bool) -> FactEntry {
        FactEntry {
            subject: subject.into(),
            key: key.into(),
            value: value.into(),
            text: text.into(),
            known_by: vec![],
            open,
        }
    }

    fn goal(npc: &str, text: &str) -> GoalEntry {
        GoalEntry {
            npc: npc.into(),
            text: text.into(),
            against: None,
        }
    }

    #[test]
    fn roster_recall_matches_through_other_names() {
        let recall = roster_recall(&expected(), &seed());
        // PC, Agathe, Gerrit (draft main name = expected other name), the
        // Lamplighters (wrong kind), Pim, Osric, Harrowmere, Copper Kettle.
        assert_eq!((recall.found, recall.expected), (8, 9));
        assert_eq!(recall.missing, vec!["Sefa Lind".to_owned()]);
        assert_eq!(recall.extra, vec!["Blue Heron".to_owned()]);
        assert_eq!(recall.wrong_kinds.len(), 1, "{:?}", recall.wrong_kinds);
        assert!(recall.wrong_kinds[0].starts_with("The Lamplighters"));
        // The other name the draft gives Gerrit matches too.
        let mut seed = seed();
        seed.entities[1].name = "Gerrit".into();
        seed.entities[1].other_names.clear();
        assert_eq!(roster_recall(&expected(), &seed).found, 7);
    }

    #[test]
    fn an_open_item_needs_a_done_phrase() {
        let empty = EXPECTED.replace(r#"done = ["married", "wedding took place"]"#, "done = []");
        let error = Expected::parse(&empty).unwrap_err();
        assert!(error.contains("Osric Fell"), "{error}");
        let missing = EXPECTED.replace(r#"done = ["married", "wedding took place"]"#, "");
        assert!(Expected::parse(&missing).is_err());
        // An empty phrase can never match: it is refused.
        for phrase in ["\"\"", "\"  \"", "\"--\""] {
            let blank = EXPECTED.replace(
                r#"done = ["married", "wedding took place"]"#,
                &format!(r#"done = ["married", {phrase}]"#),
            );
            let error = Expected::parse(&blank).unwrap_err();
            assert!(error.contains("empty done phrase"), "{phrase}: {error}");
        }
        // The flag replaces the phrases; it may not come with them.
        let flagged = EXPECTED.replace(
            r#"done = ["married", "wedding took place"]"#,
            "not_scored_for_false_past = true",
        );
        assert!(Expected::parse(&flagged).is_ok());
        let both = EXPECTED.replace(
            r#"done = ["married", "wedding took place"]"#,
            "done = [\"married\"]\nnot_scored_for_false_past = true",
        );
        assert!(Expected::parse(&both).is_err());
    }

    #[test]
    fn an_open_item_not_scored_for_false_past_is_never_counted() {
        let flagged = EXPECTED.replace(
            r#"done = ["married", "wedding took place"]"#,
            "not_scored_for_false_past = true",
        );
        let expected = Expected::parse(&flagged).unwrap();
        let facts = vec![
            fact("Osric Fell", "daughter", "married", "She married.", false),
            fact("Toll-Master", "owns_ferry", "yes", "He owns it now.", false),
        ];
        assert_eq!(
            false_past(&expected, &seed(), &facts),
            vec![None, Some(true)]
        );
        let report = section(&expected, &runs(), &fixture(), 1);
        assert!(
            report.contains("- Open items not scored for false past: 1 (Osric Fell: wedding)"),
            "{report}"
        );
        assert!(
            report.contains(
                "- False past, draft knowledge (open items stated as done): 0 of 3 over 3 drafts"
            ),
            "{report}"
        );
        let private = section(&expected, &runs(), &Shown::Private, 1);
        assert!(
            private.contains("- Open items not scored for false past: 1\n"),
            "{private}"
        );
    }

    #[test]
    fn false_past_counts_a_settled_fact_on_an_open_item() {
        let (expected, seed) = (expected(), seed());
        let planned = fact(
            "Osric Fell",
            "wedding",
            "first day of spring",
            "His daughter's wedding is planned for spring.",
            true,
        );
        let mut facts = vec![planned];
        assert_eq!(
            false_past(&expected, &seed, &facts),
            vec![Some(false), Some(false)]
        );
        assert_eq!(open_recall(&expected, &seed, &facts), vec![true, false]);

        // A settled fact on the plan, with the `about` word but no `done`
        // phrase, is right: no false past.
        facts.push(fact(
            "Osric Fell",
            "savings",
            "wedding feast",
            "Osric has saved for the wedding feast for six years.",
            false,
        ));
        assert_eq!(
            false_past(&expected, &seed, &facts),
            vec![Some(false), Some(false)]
        );

        // The same item stated as done, on the same subject: a false past.
        facts.push(fact(
            "Osric Fell",
            "daughter",
            "married",
            "Osric's daughter married at Harrowmere.",
            false,
        ));
        assert_eq!(
            false_past(&expected, &seed, &facts),
            vec![Some(true), Some(false)]
        );

        // Another subject does not count; a subject named through the draft's
        // other name does, with the word in the key.
        facts.push(fact(
            "Agathe Morrow",
            "ferry",
            "bought",
            "Agathe bought it.",
            false,
        ));
        assert_eq!(
            false_past(&expected, &seed, &facts),
            vec![Some(true), Some(false)]
        );
        facts.push(fact(
            "Toll-Master",
            "owns_ferry",
            "yes",
            "He owns it now.",
            false,
        ));
        assert_eq!(
            false_past(&expected, &seed, &facts),
            vec![Some(true), Some(true)]
        );
        // Fact recall takes any fact on the subject that holds every word.
        assert_eq!(fact_recall(&expected, &seed, &facts), vec![false, false]);
        facts.push(fact(
            "Teodor Brandt",
            "ferry_chain",
            "iron",
            "Teodor replaced the rope with a chain.",
            false,
        ));
        facts.push(fact(
            "Gerrit",
            "role",
            "Guild master",
            "He runs the hall.",
            false,
        ));
        assert_eq!(fact_recall(&expected, &seed, &facts), vec![true, true]);
    }

    #[test]
    fn pc_centred_share_counts_goals_and_secrets_that_name_the_pc() {
        let names = vec![("Teodor Brandt".to_owned(), vec!["Wick".to_owned()])];
        let goals = vec![
            goal("Gerrit Vane", "Buy the ferry from Teodor Brandt."),
            goal("Agathe Morrow", "Make Wick carry the winter post."),
            goal("Osric Fell", "Win the boundary wall case."),
            // A word that only starts like the PC's name is not the name.
            goal("Pim Quast", "Save a penny for the Teodor-Brandtson fair."),
            // The first word of the main name alone names the PC.
            goal("Odile Quast", "Make Teodor pay the toll."),
        ];
        let secret = fact(
            "Henrike Sall",
            "debt",
            "clay",
            "Henrike hides her debt from Teodor Brandt.",
            false,
        );
        let hidden = fact(
            "Henrike Sall",
            "kiln",
            "cracked",
            "Teodor Brandt saw the crack.",
            false,
        );
        let world = WorldSetup {
            facts: vec![
                FactEntry {
                    known_by: vec!["Henrike Sall".into()],
                    ..secret.clone()
                },
                FactEntry {
                    known_by: vec!["Henrike Sall".into()],
                    ..hidden
                },
            ],
            goals: goals.clone(),
            ..WorldSetup::default()
        };
        let log = vec![DebugEntry {
            op_id: "draft-1".into(),
            step: "npc_state 2/2".into(),
            prompt: String::new(),
            raw: json!({
                "goals": [],
                "secret": {"subject": "Henrike Sall", "key": "debt", "value": "clay",
                           "text": "Henrike hides her debt from Teodor Brandt.", "open": false},
                "attitudes": [],
                "voice_card": {"habits": "", "vocabulary": "", "samples": []}
            })
            .to_string(),
            ms: 1,
        }];
        let secrets = secrets_of(&world, &log);
        assert_eq!(secrets.len(), 1, "{secrets:?}");
        assert_eq!(secrets[0].text, secret.text);
        // Goals 1, 2 and 5 and the secret name the PC; goals 3 and 4 do not.
        assert_eq!(pc_centred(&names, &[], &world.goals, &secrets), (4, 6));

        // An NPC of the drafted roster shares the PC's first word: "Teodor"
        // alone now names that NPC, not the PC (the build's guard). Full
        // names still name the PC.
        let roster = ["Teodor Vane", "Gerrit Vane"];
        assert_eq!(pc_centred(&names, &roster, &world.goals, &secrets), (3, 6));
        // An other name of another entry counts too.
        let roster = ["Odile Quast", "Teo"];
        assert_eq!(pc_centred(&names, &roster, &world.goals, &secrets), (4, 6));

        // A goal against the PC and a secret about the PC count, whatever
        // their text says.
        let goals = vec![
            GoalEntry {
                against: Some("Wick".into()),
                ..goal("Gerrit Vane", "Ruin him.")
            },
            GoalEntry {
                against: Some("Agathe Morrow".into()),
                ..goal("Osric Fell", "Ruin her.")
            },
        ];
        let secrets = vec![fact(
            "Teodor Brandt",
            "debt",
            "clay",
            "A debt in clay.",
            false,
        )];
        assert_eq!(pc_centred(&names, &[], &goals, &secrets), (2, 3));
    }

    #[test]
    fn pc_centred_metric_reads_the_drafted_roster() {
        // A drafted NPC "Teodor Vane": a goal that names him is not about the
        // PC. Without the roster, "Teodor" alone would count.
        let mut seed = seed();
        seed.entities.push(
            serde_json::from_value(json!(
                {"name": "Teodor Vane", "kind": "npc", "age": 40}
            ))
            .unwrap(),
        );
        let mut draft = SeedDraft::new("text".into(), seed);
        draft.knowledge.clear();
        let world = WorldSetup {
            goals: vec![
                goal("Gerrit Vane", "Pay Teodor back for the nets."),
                goal("Osric Fell", "Buy the ferry from Teodor Brandt."),
            ],
            ..WorldSetup::default()
        };
        let run = BuildRun {
            draft: Some(draft),
            world: Some(world),
            ..BuildRun::default()
        };
        let report = section(&expected(), &[run], &fixture(), 1);
        assert!(
            report.contains(
                "- PC-centred share (NPC goals and secrets that centre on the PC): 1 of 2 (50%)"
            ),
            "{report}"
        );
    }

    /// Three runs: one built, one not approvable, one failed in the build.
    fn runs() -> Vec<BuildRun> {
        let mut draft = SeedDraft::new("text".into(), seed());
        // The model calls Gerrit's age stated (the list has none) and leaves
        // out Pim and Osric (the list states theirs).
        for name in ["Teodor Brandt", "Agathe Morrow", "Toll-Master"] {
            let mark = AgeMark {
                drafted: None,
                stated: true,
                confirmed: None,
            };
            draft.ages.insert(normalize::name(name), mark);
        }
        draft.knowledge.push(KnownFact {
            subject: "Teodor Brandt".into(),
            key: "ferry".into(),
            value: "iron chain".into(),
            text: "Teodor put an iron chain on the ferry.".into(),
            open: false,
        });
        // A built world holds the known facts too.
        let world = WorldSetup {
            facts: vec![
                fact(
                    "Teodor Brandt",
                    "ferry",
                    "iron chain",
                    "Teodor put an iron chain on the ferry.",
                    false,
                ),
                fact(
                    "Osric Fell",
                    "wedding",
                    "done",
                    "The wedding took place.",
                    false,
                ),
            ],
            goals: vec![
                goal("Gerrit Vane", "Buy the ferry from Teodor Brandt."),
                goal("Osric Fell", "Marry off his daughter."),
            ],
            events: vec![
                EventEntry {
                    at: 60,
                    actor: "Gerrit Vane".into(),
                    text: "Gerrit raises the toll".into(),
                    effects: vec![EffectEntry {
                        subject: "Gerrit Vane".into(),
                        key: "toll".into(),
                        value: "five pennies".into(),
                    }],
                    seen_by: vec![],
                },
                EventEntry {
                    at: 600,
                    actor: "Osric Fell".into(),
                    text: "Osric argues with the cook".into(),
                    effects: vec![],
                    seen_by: vec![],
                },
            ],
            ..WorldSetup::default()
        };
        let built = BuildRun {
            draft: Some(draft.clone()),
            draft_ms: Some(90_000),
            world: Some(world),
            build_ms: Some(150_000),
            draft_calls: Calls {
                calls: 5,
                retries: 1,
                repairs: 1,
            },
            build_calls: Calls {
                calls: 14,
                retries: 0,
                repairs: 2,
            },
            voices: vec![
                VoiceLine {
                    npc: "Gerrit Vane".into(),
                    line: "Four pennies, and not one less.".into(),
                    choices: 4,
                    hit: Some(true),
                },
                VoiceLine {
                    npc: "Osric Fell".into(),
                    line: "The pies cost what they cost.".into(),
                    choices: 4,
                    hit: Some(false),
                },
                VoiceLine {
                    npc: "Agathe Morrow".into(),
                    line: "Boots off my bar.".into(),
                    choices: 4,
                    hit: None,
                },
            ],
            ..BuildRun::default()
        };
        let gapped = BuildRun {
            draft: Some(draft.clone()),
            draft_ms: Some(80_000),
            gaps: vec![
                Gap {
                    kind: GapKind::AgeMissing,
                    at: "entities[2].age".into(),
                },
                Gap {
                    kind: GapKind::UnknownName,
                    at: "start.place".into(),
                },
            ],
            ..BuildRun::default()
        };
        let failed = BuildRun {
            draft: Some(draft),
            draft_ms: Some(70_000),
            failure: Some(BuildFailure {
                stage: Stage::HiddenFacts,
                check: BuildCheck::Schema,
            }),
            ..BuildRun::default()
        };
        vec![built, gapped, failed]
    }

    fn fixture() -> Shown {
        Shown::Fixture {
            source: "fixtures/chronicle/source.md".into(),
            expected: "fixtures/chronicle/expected.toml".into(),
        }
    }

    #[test]
    fn report_has_every_world_build_metric() {
        let report = section(&expected(), &runs(), &fixture(), 7_874);
        for line in [
            "- Runs: 3; drafted 3, approvable 2, built 1, failed 1",
            "- Roster recall: 24 of 27 expected entries over 3 drafts (89%)",
            "- Wrong kinds: 3 (The Lamplighters: expected faction, drafted npc)",
            "- Extra entities: 3 (Blue Heron)",
            "- Missing entries: 3 (Sefa Lind)",
            "- Ages that differ from a stated age: 3 of 12 (Osric Fell: stated 52, drafted 47)",
            "- Child (stated age under 18) drafted under 18: 3 of 3 (Pim Quast)",
            "- Wrong age_stated claims: 9 of 18 drafted characters (claimed stated, the list has no age: Gerrit Vane; claimed proposed, the list states it: Osric Fell, Pim Quast)",
            "- Start place right: 3 of 3 drafts; present characters found: 3 of 3",
            "- Draft gaps before approval, by kind: age_missing 1, unknown_name 1",
            "- Not approvable: 1 of 3 runs",
            "- Characters drafted under 18 that the expected list does not hold as children: 3 (Toll-Master)",
            "- Open items not scored for false past: 0\n",
            "- Runs with a built world: 1 of 3",
            "- Fact recall, draft knowledge (settled items found): 3 of 6 over 3 drafts (50%); missed: Gerrit Vane: guild",
            "- Fact recall, built world (settled items found): 1 of 2 over 1 built worlds (50%); missed: Gerrit Vane: guild",
            "- False past, draft knowledge (open items stated as done): 0 of 6 over 3 drafts\n",
            "- False past, built world (open items stated as done): 1 of 2 over 1 built worlds; stated as done: Osric Fell: wedding",
            "- Open recall, draft knowledge (open items kept open): 0 of 6 over 3 drafts",
            "- Open recall, built world (open items kept open): 0 of 2 over 1 built worlds",
            "| run | draft | gaps | build | roster | settled found (draft / world) | false past (draft / world) | open kept (draft / world) | PC-centred | events (with effect) | draft calls (repairs, retries) | build calls (repairs, retries) | draft time | build time |",
            "| 1 | ok | 0 | ok | 8 of 9 | 1 of 2 / 1 of 2 | 0 of 2 / 1 of 2 | 0 of 2 / 0 of 2 | 1 of 2 | 2 (1) | 5 (1, 1) | 14 (2, 0) | 90.0 s | 150.0 s |",
            "| 2 | ok | 2 | not approvable | 8 of 9 | 1 of 2 / - | 0 of 2 / - | 0 of 2 / - | - | - | - | - | 80.0 s | n/a |",
            "- PC-centred share (NPC goals and secrets that centre on the PC): 1 of 2 (50%) over 1 built worlds; highest world 50%; the build's limit is 50%",
            "- World Events: 2 over 1 built worlds; with at least one effect: 1",
            "- Voice distance: matched 1 / 2 judged sample lines (50%), chance 25%; judge errors 1",
            "- Draft time p50 / max: 80.0 s / 90.0 s",
            "- Build time p50 / max: 150.0 s / 150.0 s",
            "- Model calls: draft 5, build 14; timeout retries 1; repair calls 3",
            "- Failures (BuildFailure, by stage and check): 1 x hidden_facts / schema",
            "Sample Voice Card lines:",
            "- Gerrit Vane: \"Four pennies, and not one less.\"",
        ] {
            assert!(report.contains(line), "missing {line}:\n{report}");
        }
        assert!(report.contains("fixtures/chronicle/source.md"), "{report}");
        // The fact rule is stated: it is a lower bound.
        assert!(report.contains("lower bound"), "{report}");
        // At most three sample lines.
        assert!(report.matches("\n- Agathe Morrow: \"").count() <= 1);
    }

    #[test]
    fn private_report_prints_counts_only() {
        let report = section(&expected(), &runs(), &Shown::Private, 7_874);
        for line in [
            "- Runs: 3; drafted 3, approvable 2, built 1, failed 1",
            "- Roster recall: 24 of 27 expected entries over 3 drafts (89%)",
            "- Wrong kinds: 3\n",
            "- Extra entities: 3\n",
            "- Missing entries: 3\n",
            "- Ages that differ from a stated age: 3 of 12\n",
            "- Child (stated age under 18) drafted under 18: 3 of 3\n",
            "- Wrong age_stated claims: 9 of 18 drafted characters\n",
            "- Characters drafted under 18 that the expected list does not hold as children: 3\n",
            "- Fact recall, draft knowledge (settled items found): 3 of 6 over 3 drafts (50%)\n",
            "- False past, built world (open items stated as done): 1 of 2 over 1 built worlds\n",
            "- Voice distance: matched 1 / 2 judged sample lines (50%), chance 25%; judge errors 1",
        ] {
            assert!(report.contains(line), "missing {line:?}:\n{report}");
        }
        for name in [
            "Teodor",
            "Wick",
            "Sefa",
            "Lamplighters",
            "Heron",
            "Osric",
            "Pim",
            "Gerrit",
            "Toll",
            "wedding",
            "pennies",
            "fixtures/",
            "Sample Voice Card lines",
        ] {
            assert!(!report.contains(name), "{name} in:\n{report}");
        }
    }
}
