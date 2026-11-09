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
    };
    let make = || Box::new(gm.clone()) as Box<dyn Gm + Send>;
    let report = run(&config, &make, &FirstChoice, &mut |_| {}).unwrap();

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
    let event = pending.events_scheduled[0].id;
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
    };
    let make = || Box::new(gm.clone()) as Box<dyn Gm + Send>;
    let report = run(&config, &make, &FirstChoice, &mut |_| {}).unwrap();
    for line in [
        "- (a) Model scheduled an event on the provocation Turn: 0 of 1",
        "- (b) Seeded event fired while the PC was away: 1 of 1 (PC away after Turn 2: 1 of 1)",
        "- (c) PC later learned of it through play: 1 of 1",
        "- Offscreen consequence observed (b and c in the same run): 1 of 1",
    ] {
        assert!(report.contains(line), "missing {line}:\n{report}");
    }
}
