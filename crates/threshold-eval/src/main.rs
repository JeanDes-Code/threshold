//! `threshold-eval --scenario <name|all> --runs <n>`: runs the evaluation
//! scenarios against the live local model and writes a Markdown report.
//!
//! Options:
//! - `--scenario <name|all>` (default `all`); names: latency, memory,
//!   voices, shortcuts, agency_leaks, offscreen, world_build, speech,
//!   play_basics.
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
//! - `--raw-log <path>`: appends each Turn's raw proposals, core repairs
//!   and check results to this file. They can hold GM-only facts, so a path
//!   inside the repository is refused, and nothing of it reaches stderr or
//!   the report.

use std::{fs, path::PathBuf, process::ExitCode};

use threshold_core::{Gm, build::BUILD_TIMEOUT_S};
use threshold_eval::{Config, GmJudge, SCENARIOS, check_raw_log, find_root, run, today};
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
            "-h" | "--help" => {
                println!(
                    "threshold-eval --scenario <name|all> --runs <n> [--judge local] [--model <tag>] [--memory-turns <n>] [--source <path> --expected <path>] [--out <path>] [--raw-log <path outside the repository>]"
                );
                return Ok(());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if scenarios.is_empty() || scenarios.iter().any(|s| s == "all") {
        scenarios = SCENARIOS.iter().map(ToString::to_string).collect();
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
    let raw_log = raw_log.map(|p| cwd.join(p));
    if let Some(path) = &raw_log {
        check_raw_log(&root, path)?;
    }

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
    };
    let make = || Box::new(OllamaGm::new(ollama.clone())) as Box<dyn Gm + Send>;
    let mut slow = ollama.clone();
    slow.timeout_s = BUILD_TIMEOUT_S;
    let build = || Box::new(OllamaGm::new(slow.clone())) as Box<dyn Gm + Send>;
    let judge = GmJudge::new(OllamaGm::new(ollama.clone()));
    let report = run(&config, &make, &build, &judge, &mut |line| {
        eprintln!("{line}")
    })?;

    let path = out.unwrap_or_else(|| root.join("evals/reports").join(format!("{}.md", today())));
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, &report).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("{report}");
    eprintln!("report written to {}", path.display());
    Ok(())
}
