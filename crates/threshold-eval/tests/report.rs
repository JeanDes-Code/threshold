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
        "proposed",
        "caught",
        "Agency per 100 Turns",
        "Secret leaks per 100 Turns",
        "Offscreen consequence observed",
    ] {
        assert!(report.contains(metric), "missing {metric}:\n{report}");
    }
    assert!(report.contains("Runs: 2"), "{report}");
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
