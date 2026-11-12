//! `threshold-eval --scenario <name|all> --runs <n>`: runs the evaluation
//! scenarios against the live local model and writes a Markdown report.
//!
//! Options:
//! - `--scenario <name|all>` (default `all`); names: latency, memory,
//!   voices, shortcuts, agency_leaks, offscreen, world_build, speech,
//!   play_basics, open_items, ooc, departures, intimate_tag, scene_agency,
//!   npc_will, scene_latency, replay. `all` leaves out replay, which needs a
//!   Campaign file, and the four mature scenarios.
//!   Repeat or comma-separate.
//! - `--runs <n>` (default 3)
//! - `--judge local` (default): the same local model, with a short rubric.
//!   A frontier judge is not built in this slice.
//! - `--model <tag>`: the Ollama model (default from `OllamaConfig`).
//! - `--memory-turns <n>`: overrides the memory scenario's Turn count.
//! - `--source <path>`, `--expected <path>`: the world_build scenario's
//!   Source Text and expected list, instead of the scenario file's. A file
//!   outside the repository's `fixtures/` makes its report section print
//!   counts only.
//! - `--out <path>` (default `evals/reports/<date>.md`).
//! - `--campaign <path>`, `--intents <path>`: the replay scenario's
//!   Campaign file and Intents (one per line). Both must lie outside the
//!   repository; the report prints numbers only.
//! - `--raw-log <path>`: appends each Turn's raw proposals, core repairs
//!   and check results to this file. They can hold GM-only facts, so a path
//!   inside the repository is refused, and nothing of it reaches stderr or
//!   the report.
//! - `--packs <dir>`: the Content Packs every Engine of the run gets, from
//!   a folder laid out as `<data_dir>/packs`.
//! - `--scenarios-dir <dir>`: the folder the mature scenarios
//!   (`intimate_tag`, `scene_agency`, `npc_will`, `scene_latency`) are read
//!   from, in place of `evals/scenarios`. A file inside the repository must
//!   be at `detail = "implied"`.
//!
//!   When `--packs` or `--scenarios-dir` names a folder outside the
//!   repository, the run is private: `--out` is required and must lie
//!   outside the repository too (as `--raw-log`), the progress lines print
//!   no check reason and no error text, and the report prints counts only.
//!   `all` leaves the mature scenarios out: name them.

use std::{fs, path::PathBuf, process::ExitCode};

use threshold_core::{Gm, build::BUILD_TIMEOUT_S};
use threshold_eval::{
    Config, GmJudge, NOT_IN_ALL, SCENARIOS, check_private_out, check_raw_log, check_target,
    find_root, resolve, run, today,
};
use threshold_ollama::{OllamaConfig, OllamaGm};

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<(), String> {
    let mut scenarios: Vec<String> = Vec::new();
    let mut runs = 3;
    let mut judge = "local".to_owned();
    let mut ollama = OllamaConfig::default();
    let mut memory_turns = None;
    let mut out: Option<PathBuf> = None;
    let mut raw_log: Option<PathBuf> = None;
    let (mut source, mut expected): (Option<PathBuf>, Option<PathBuf>) = (None, None);
    let (mut campaign, mut intents): (Option<PathBuf>, Option<PathBuf>) = (None, None);
    let (mut packs_dir, mut scenarios_dir): (Option<PathBuf>, Option<PathBuf>) = (None, None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--scenario" => scenarios.extend(value()?.split(',').map(str::to_owned)),
            "--runs" => runs = value()?.parse().map_err(|e| format!("--runs: {e}"))?,
            "--judge" => judge = value()?,
            "--model" => ollama.model = value()?,
            "--memory-turns" => {
                memory_turns = Some(
                    value()?
                        .parse()
                        .map_err(|e| format!("--memory-turns: {e}"))?,
                );
            }
            "--out" => out = Some(PathBuf::from(value()?)),
            "--raw-log" => raw_log = Some(PathBuf::from(value()?)),
            "--source" => source = Some(PathBuf::from(value()?)),
            "--expected" => expected = Some(PathBuf::from(value()?)),
            "--campaign" => campaign = Some(PathBuf::from(value()?)),
            "--intents" => intents = Some(PathBuf::from(value()?)),
            "--packs" => packs_dir = Some(PathBuf::from(value()?)),
            "--scenarios-dir" => scenarios_dir = Some(PathBuf::from(value()?)),
            "-h" | "--help" => {
                println!(
                    "threshold-eval --scenario <name|all> --runs <n> [--judge local] [--model <tag>] [--memory-turns <n>] [--source <path> --expected <path>] [--out <path>] [--raw-log <path outside the repository>] [--packs <dir>] [--scenarios-dir <dir>]\n\n--packs <dir>          Content Packs for every Engine of the run\n--scenarios-dir <dir>  the folder of the mature scenario files (intimate_tag, scene_agency, npc_will, scene_latency)\nA --packs or --scenarios-dir folder outside the repository makes the run private: --out is then required, outside the repository, and the report prints counts only."
                );
                return Ok(());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if scenarios.is_empty() || scenarios.iter().any(|s| s == "all") {
        scenarios = SCENARIOS
            .iter()
            .filter(|s| !NOT_IN_ALL.contains(s))
            .map(ToString::to_string)
            .collect();
    }
    if runs == 0 {
        return Err("--runs must be at least 1".into());
    }
    if judge != "local" {
        return Err(format!(
            "judge {judge} is not available: only the local judge is built in this slice"
        ));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = find_root(&cwd).ok_or("run from the repository: evals/scenarios not found")?;
    // Every path a run writes is resolved and checked before anything is
    // created (review B1): no folder of it exists until the checks pass.
    let raw_log = match raw_log.map(|p| cwd.join(p)) {
        Some(path) => Some(check_raw_log(&root, &path)?),
        None => None,
    };
    let packs_dir = packs_dir.map(|p| cwd.join(p));
    let scenarios_dir = scenarios_dir.map(|p| cwd.join(p));
    let out = out.map(|p| cwd.join(p));
    let private = check_private_out(
        &root,
        packs_dir.as_deref(),
        scenarios_dir.as_deref(),
        out.as_deref(),
    )?;
    // The report: the checked path of a private run; else the path given,
    // or the default under `evals/reports`, never through a link.
    let out = match out {
        Some(path) if private => check_target(&root, &path, "the report of a private run")?,
        Some(path) => resolve(&path)?,
        None => resolve(&root.join("evals/reports").join(format!("{}.md", today())))?,
    };

    let warm = OllamaGm::new(ollama.clone());
    warm.warm_up().map_err(|e| format!("warm-up: {e}"))?;
    eprintln!("warm-up ok: {}", ollama.model);

    let config = Config {
        root: root.clone(),
        scenarios,
        runs,
        model: format!("{} (num_ctx {})", ollama.model, ollama.num_ctx),
        judge: format!("local: {} with a short rubric prompt", ollama.model),
        memory_turns,
        source: source.map(|p| cwd.join(p)),
        expected: expected.map(|p| cwd.join(p)),
        raw_log,
        campaign: campaign.map(|p| cwd.join(p)),
        intents: intents.map(|p| cwd.join(p)),
        packs_dir,
        scenarios_dir,
    };
    let make = || Box::new(OllamaGm::new(ollama.clone())) as Box<dyn Gm + Send>;
    let mut slow = ollama.clone();
    slow.timeout_s = BUILD_TIMEOUT_S;
    let build = || Box::new(OllamaGm::new(slow.clone())) as Box<dyn Gm + Send>;
    let judge = GmJudge::new(OllamaGm::new(ollama.clone()));
    let report = run(&config, &make, &build, &judge, &mut |line| {
        eprintln!("{line}")
    })?;

    let path = out;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, &report).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("{report}");
    eprintln!("report written to {}", path.display());
    Ok(())
}
