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
        scenarios: SCENARIOS
            .iter()
            .filter(|s| **s != "replay")
            .map(ToString::to_string)
            .collect(),
        runs: 2,
        model: "recorded".into(),
        judge: "first-choice".into(),
        memory_turns: None,
        source: None,
        expected: None,
        raw_log: None,
        campaign: None,
        intents: None,
    };
    let make = || Box::new(gm.clone()) as Box<dyn Gm + Send>;
    let report = run(&config, &make, &make, &FirstChoice, &mut |_| {}).unwrap();

    for name in SCENARIOS.iter().filter(|s| **s != "replay") {
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
        "Mark rate",
        "Moves that reached the asked place",
        "False closes / negative steps",
        "False moves / negative steps",
        "Turns with a clock advance > 0",
        "Impossible acts narrated as failure (judge)",
        "Contests with a Roll",
        "Pending items closed",
        "Moves to an unknown place that left the PC in place",
        "Wrong closes in total",
        "Proposal prompt tokens",
        "Right answers in total",
        "Departures told with no recorded departure",
    ] {
        assert!(report.contains(metric), "missing {metric}:\n{report}");
    }
    assert!(report.contains("Runs: 2"), "{report}");
    // Per run: latency 10, memory 40, voices 9, shortcuts 4 traps x 2,
    // agency_leaks 12, offscreen 4, speech 10, play_basics 8, open_items 9,
    // departures 5 = 115 Turns. The OOC scenario plays no Turn.
    assert!(report.contains("- Turns played: 230 (0 failed"), "{report}");
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
            public: false,
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
        let place = id(&view, place);
        format!(
            r#"{{"action":"move","verdict":"allowed","move_to":"E{place}","check":null,"schedule":null,"minutes":{minutes}}}"#
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
        raw_log: None,
        campaign: None,
        intents: None,
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
        domain::{
            AdventureSeed, EffectEntry, EntityKind, EventEntry, FactEntry, GoalEntry, WorldSetup,
        },
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
            let pc = name == draft.seed.pc.name;
            let mark = AgeMark {
                name: name.into(),
                kind: if pc { EntityKind::Pc } else { EntityKind::Npc },
                drafted: None,
                stated: true,
                confirmed: None,
            };
            let row = draft.seed.entities.iter().position(|e| e.name == name);
            *draft.age_marks.slot(row) = Some(mark);
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
                    public: false,
                },
                EventEntry {
                    at: 600,
                    actor: "Osric Fell".into(),
                    text: "Osric argues with the cook".into(),
                    effects: vec![],
                    seen_by: vec![],
                    public: false,
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

// ---- speech -----------------------------------------------------------------

mod speech {
    use threshold_core::{
        Gm, RecordedGm,
        store::{Store, TurnId, TurnKind, TurnRecord},
    };
    use threshold_eval::{
        Config, run,
        speech::{Tally, quoted_spans, read_turn},
    };

    use super::{FirstChoice, PROPOSAL, root, town};

    /// Commits a Play Turn and stores `narration` as its Narration.
    fn narrated(store: &mut Store, campaign: u64, op: &str, narration: &str) -> TurnId {
        let clock = store.snapshot(campaign).unwrap().clock_min;
        let turn = store
            .commit(
                TurnRecord {
                    campaign_id: campaign,
                    op_id: op.into(),
                    kind: TurnKind::Play,
                    intent: "I listen.".into(),
                    proposal_json: "{}".into(),
                    clock_before: clock,
                    clock_after: clock + 1,
                },
                vec![threshold_core::domain::LedgerOp::AdvanceClock { minutes: 1 }],
            )
            .unwrap();
        store.set_narration(turn, narration).unwrap();
        turn
    }

    #[test]
    fn mark_rate_counts_resolved_marks_over_spoken_lines() {
        let (mut store, campaign) = town();
        let turn = narrated(
            &mut store,
            campaign,
            "speech-1",
            "Orsel: \"Sit down.\"\n\nNobody: \"Nobody is here.\"\n\nHessa mutters \"Ink is dear.\" and turns a page.",
        );
        let read = read_turn(&store, campaign, turn).unwrap();
        assert_eq!(read.tally.spoken, 3);
        assert_eq!(read.tally.marks, 2);
        assert_eq!(read.tally.resolved, 1);
        assert_eq!(read.tally.unresolved(), 1);
        assert_eq!(read.tally.no_entity, 1);
        assert_eq!(read.tally.rate(), Some((1, 3)));
        assert_eq!(read.bubbles.len(), 1);
        assert_eq!(read.bubbles[0].speaker, "Orsel");
        // The judge reads the Narration with every tag removed.
        assert!(
            !read.bubbles[0].context.contains("Orsel:"),
            "{:?}",
            read.bubbles[0]
        );
        assert!(read.bubbles[0].context.contains("\"Sit down.\""));
        let mut present = read.bubbles[0].present.clone();
        present.sort();
        assert_eq!(present, ["Doul", "Hessa", "Orsel"]);
    }

    #[test]
    fn marks_the_narrator_misuses_are_counted_apart() {
        let (mut store, campaign) = town();
        let turn = narrated(
            &mut store,
            campaign,
            "speech-2",
            "Wren: \"I am the scribe.\"\n\nOrsel: You are late.\n\nThunder: \"Boom.\"",
        );
        let read = read_turn(&store, campaign, turn).unwrap();
        assert_eq!(read.tally.marks, 2);
        assert_eq!(read.tally.resolved, 0);
        assert_eq!(read.tally.pc, 1);
        assert_eq!(read.tally.no_entity, 1);
        assert_eq!(read.tally.unquoted, 1);
        assert_eq!(read.tally.spoken, 2);
    }

    /// B11 (QA, 2026-11-10): a tag that is the first name of a character
    /// names an entity, as in the core (`world::tagged`). The eval matched
    /// full names only, so "Mara:" counted as a tag of no entity.
    #[test]
    fn a_first_name_tag_names_an_entity() {
        use threshold_core::domain::{Canon, EntityKind, EntityRef, FactSource, LedgerOp, Slot};
        let (mut store, campaign) = town();
        let clock = store.snapshot(campaign).unwrap().clock_min;
        store
            .commit(
                TurnRecord {
                    campaign_id: campaign,
                    op_id: "mara".into(),
                    kind: TurnKind::Play,
                    intent: "I ask about the mill.".into(),
                    proposal_json: "{}".into(),
                    clock_before: clock,
                    clock_after: clock,
                },
                vec![
                    LedgerOp::AddEntity {
                        local: Some(1),
                        kind: EntityKind::Npc,
                        name: "Mara Quell".into(),
                        age: Some(40),
                        other_names: vec![],
                    },
                    LedgerOp::AddFact {
                        slot: Slot {
                            subject: EntityRef::New(1),
                            key: "location".into(),
                            value: "varn mill".into(),
                        },
                        text: "Mara Quell is at the mill.".into(),
                        canon: Canon::Established,
                        source: FactSource::Proposal,
                        seen_by: vec![],
                        causes: vec![],
                    },
                ],
            )
            .unwrap();
        // A Narration names her, so the PC can name her; she is not here.
        narrated(
            &mut store,
            campaign,
            "named",
            "Hessa says Mara Quell runs the mill now.",
        );
        let turn = narrated(
            &mut store,
            campaign,
            "speech-mara",
            "Mara: \"Bring the sacks.\"\n\nMara: Bring them now.",
        );
        let read = read_turn(&store, campaign, turn).unwrap();
        assert_eq!(read.tally.marks, 1);
        assert_eq!(read.tally.no_entity, 0, "{:?}", read.tally);
        assert_eq!(read.tally.other, 1, "{:?}", read.tally);
        assert_eq!(read.tally.unquoted, 1, "{:?}", read.tally);
    }

    #[test]
    fn quoted_spans_are_counted_once_per_span() {
        assert_eq!(quoted_spans(r#""Late," he says, "very late.""#), 2);
        assert_eq!(quoted_spans("\u{201c}Yes.\u{201d} \u{ab}Oui.\u{bb}"), 2);
        assert_eq!(quoted_spans(r#"Orsel: "Late.""#), 1);
        assert_eq!(quoted_spans("Wren's quill isn't dry."), 0);
        assert_eq!(quoted_spans(r#"An open quote " with no end"#), 0);
        assert_eq!(quoted_spans(""), 0);
    }

    #[test]
    fn a_turn_with_no_speech_adds_nothing_to_the_rate() {
        let (mut store, campaign) = town();
        let quiet = narrated(
            &mut store,
            campaign,
            "speech-3",
            "The rain stops. Orsel reads.",
        );
        let read = read_turn(&store, campaign, quiet).unwrap();
        assert_eq!(read.tally, Tally::default());
        assert!(read.bubbles.is_empty());

        let said = narrated(&mut store, campaign, "speech-4", "Orsel: \"Again.\"");
        let mut total = read_turn(&store, campaign, said).unwrap().tally;
        let before = total.rate();
        total.add(&read.tally);
        assert_eq!(total.rate(), before);
        assert_eq!(before, Some((1, 1)));
        assert_eq!(Tally::default().rate(), None);
    }

    #[test]
    fn report_has_every_speech_metric() {
        let gm = RecordedGm::new(
            vec![PROPOSAL.to_owned(); 500],
            vec!["Orsel: \"Sit.\"\n\nHessa looks up. \"Morning,\" she says.".to_owned(); 500],
        );
        let config = Config {
            root: root(),
            scenarios: vec!["speech".into()],
            runs: 2,
            model: "recorded".into(),
            judge: "first-choice".into(),
            memory_turns: None,
            source: None,
            expected: None,
            raw_log: None,
            campaign: None,
            intents: None,
        };
        let make = || Box::new(gm.clone()) as Box<dyn Gm + Send>;
        let report = run(&config, &make, &make, &FirstChoice, &mut |_| {}).unwrap();
        assert_eq!(report.matches("\n## speech\n").count(), 1, "{report}");
        for metric in [
            "Turn time p50",
            "Turn time p90",
            "Turn time max",
            "Lines of speech (quoted spans): 40",
            "Mark rate: 20 / 40 (50%)",
            "Marks: 20; resolved 20; not resolved 0",
            "Marked lines without quotes: 0",
            "Wrong marks (judge):",
            "- Run 1:",
            "- Run 2:",
        ] {
            assert!(report.contains(metric), "missing {metric}:\n{report}");
        }
    }
}

/// B12 (QA, 2026-11-10): the play-basics counts, on recorded answers that
/// do each step right, then (review, 2026-11-10) on answers that close on
/// the negative step, move on the other one, and move to the wrong place.
#[test]
fn play_basics_counts_each_step() {
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
    let view = store.snapshot(campaign).unwrap();
    let id = |name: &str| view.entity_by_name(name).unwrap().id;
    let (wren, doul, hall) = (id("Wren"), id("Doul"), id("Varn town hall"));
    let mill = id("Varn mill");
    let contest = format!(
        r#"{{"action":"act","verdict":"check","check":{{"against":"E{doul}","win":{{"subject":"E{doul}","key":"mood","value":"beaten"}},"lose":{{"subject":"E{wren}","key":"fatigue","value":"sore_arm"}},"factors":[]}},"schedule":null,"minutes":5}}"#
    );
    let impossible =
        r#"{"action":"act","verdict":"impossible","check":null,"schedule":null,"minutes":1}"#
            .to_owned();
    let close = |minutes: u32| {
        format!(
            r#"{{"action":"talk","verdict":"allowed","settles":{{"item":"E{wren} pending_copy_tithe_roll","outcome":"accepted"}},"check":null,"schedule":null,"minutes":{minutes}}}"#
        )
    };
    // No `minutes`: the core's default for a talk.
    let talk = r#"{"action":"talk","verdict":"allowed","check":null,"schedule":null}"#.to_owned();
    let go = |place: u64| {
        format!(
            r#"{{"action":"move","verdict":"allowed","move_to":"E{place}","check":null,"schedule":null,"minutes":15}}"#
        )
    };
    let nowhere = r#"{"action":"move","verdict":"allowed","move_to":null,"check":null,"schedule":null,"minutes":10}"#
        .to_owned();
    let report_of = |proposals: Vec<String>| {
        let gm = RecordedGm::new(proposals, vec!["It happens.".to_owned(); 8]);
        let config = Config {
            root: root(),
            scenarios: vec!["play_basics".into()],
            runs: 1,
            model: "recorded".into(),
            judge: "first-choice".into(),
            memory_turns: None,
            source: None,
            expected: None,
            raw_log: None,
            campaign: None,
            intents: None,
        };
        let make = || Box::new(gm.clone()) as Box<dyn Gm + Send>;
        run(&config, &make, &make, &FirstChoice, &mut |_| {}).unwrap()
    };

    let right = report_of(vec![
        contest.clone(),
        impossible.clone(),
        talk.clone(),
        close(2),
        talk.clone(),
        go(mill),
        go(hall),
        nowhere.clone(),
    ]);
    for line in [
        "- Moves that reached the asked place: 2 / 2 (100%)",
        "- False closes / negative steps: 0 / 1 (0%)",
        "- False moves / negative steps: 0 / 1 (0%)",
        "- Turns with a clock advance > 0 (committed Turns): 8 / 8 (100%)",
        "- Impossible acts narrated as failure (judge): 1 / 1 (100%) (0 judge errors)",
        "- Contests with a Roll: 1 / 1 (100%)",
        "- Contests told with their result (judge): 1 / 1 (100%)",
        "- Narrations that open with the time (committed Turns): 0 / 8 (0%)",
        "- Pending items closed: 1 / 1 (100%)",
        "- Moves to an unknown place that left the PC in place: 1 / 1 (100%); told as not arriving (judge): 1 / 1 (100%)",
        "- Turns failed: 0 / 8 (0%)",
    ] {
        assert!(right.contains(line), "missing {line}:\n{right}");
    }

    // The model closes on "more time", moves on the question about the
    // mill, then "goes back" to the mill instead of the town hall.
    let wrong = report_of(vec![
        contest,
        impossible,
        close(2),
        talk,
        go(mill),
        go(mill),
        go(mill),
        nowhere,
    ]);
    for line in [
        // The PC already stands at the mill for the first move.
        "- Moves that reached the asked place: 1 / 2 (50%)",
        "- False closes / negative steps: 1 / 1 (100%)",
        "- False moves / negative steps: 1 / 1 (100%)",
        "- Pending items closed: 0 / 1 (0%)",
    ] {
        assert!(wrong.contains(line), "missing {line}:\n{wrong}");
    }
}

/// R9: the raw proposals go to a file the caller names outside the
/// repository, never to the log or the report; a path inside is refused.
#[test]
fn raw_log_goes_to_a_file_outside_the_repository_only() {
    // The live model writes newlines inside its JSON (R6).
    let spread = PROPOSAL.replace(",\"minutes\"", ",\n  \"minutes\"");
    let gm = RecordedGm::new(vec![spread; 20], vec!["Text.".to_owned(); 20]);
    let make = || Box::new(gm.clone()) as Box<dyn Gm + Send>;
    let config = |raw_log: PathBuf| Config {
        root: root(),
        scenarios: vec!["play_basics".into()],
        runs: 1,
        model: "recorded".into(),
        judge: "first-choice".into(),
        memory_turns: None,
        source: None,
        expected: None,
        raw_log: Some(raw_log),
        campaign: None,
        intents: None,
    };
    let inside = root().join("evals/reports/raw.log");
    let refused = run(
        &config(inside.clone()),
        &make,
        &make,
        &FirstChoice,
        &mut |_| {},
    );
    assert!(refused.unwrap_err().contains("inside the repository"));
    assert!(!inside.exists());

    let dir = std::env::temp_dir().join(format!("threshold-raw-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let outside = dir.join("raw.log");
    let mut lines = Vec::new();
    let report = run(
        &config(outside.clone()),
        &make,
        &make,
        &FirstChoice,
        &mut |l| lines.push(l.to_owned()),
    )
    .unwrap();
    let raw = std::fs::read_to_string(&outside).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(raw.contains("basics-r1-t1 propose: "), "{raw}");
    // One row per line, so a row never reads as a cut answer.
    assert!(raw.lines().all(|l| l.starts_with("basics-r1-t")), "{raw}");
    assert!(!report.contains("\"verdict\""), "{report}");
    assert!(
        lines.iter().all(|l| !l.contains("\"verdict\"")),
        "{lines:?}"
    );
}

/// Bug pass 3: the open-items counts, on recorded answers. The acceptance
/// closes its item; the refusal closes nothing; a keep step closes another
/// item it offers, which is a wrong close. A close of an item the step did
/// not offer is dropped by the core (review M1), so it is no close at all.
#[test]
fn open_items_counts_right_and_wrong_closes() {
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
    let view = store.snapshot(campaign).unwrap();
    let id = |name: &str| view.entity_by_name(name).unwrap().id;
    let (orsel, tobin) = (id("Orsel"), id("Tobin"));
    let wren = id("Wren");
    let talk = r#"{"action":"talk","verdict":"allowed","minutes":2}"#.to_owned();
    let settle = |item: String| {
        format!(
            r#"{{"action":"talk","verdict":"allowed","settles":{{"item":"{item}","outcome":"accepted"}},"minutes":2}}"#
        )
    };
    let gm = RecordedGm::new(
        vec![
            talk.clone(),
            // "I tell Orsel a joke.": nothing.
            talk.clone(),
            talk.clone(),
            settle(format!("E{orsel} pending_copy_tithe_roll")),
            // Not offered by "the walk down to the Varn mill": dropped.
            settle(format!("E{tobin} pending_millstone_sale")),
            talk.clone(),
            // "I ask Doul about his plans for the winter.": nothing.
            talk.clone(),
            // Offered by "the flour", and wrong on a keep step.
            settle(format!("E{wren} pending_flour_debt")),
            talk,
        ],
        vec!["It happens.".to_owned(); 9],
    );
    let config = Config {
        root: root(),
        scenarios: vec!["open_items".into()],
        runs: 1,
        model: "recorded".into(),
        judge: "first-choice".into(),
        memory_turns: None,
        source: None,
        expected: None,
        raw_log: None,
        campaign: None,
        intents: None,
    };
    let make = || Box::new(gm.clone()) as Box<dyn Gm + Send>;
    let report = run(&config, &make, &make, &FirstChoice, &mut |_| {}).unwrap();
    for line in [
        "| Accept | 1 | 1 / 1 | 0 | 0 |",
        "| Refuse | 1 | 0 / 1 | 0 | 0 |",
        "| Keep | 2 | (none asked) | 1 | 0 |",
        "| Unrelated | 5 | (none asked) | 0 | 0 |",
        "- Wrong closes in total: 1",
        "- Wrong closes by item: flour_debt 1",
    ] {
        assert!(report.contains(line), "missing {line}:\n{report}");
    }
}
