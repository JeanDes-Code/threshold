//! The replay scenario (bug pass 5, M4): plays a list of Intents on a copy
//! of a Campaign file, and reports Turn time with the model server's numbers
//! for each call: the prompt's tokens (Ollama's `prompt_eval_count`; Ollama
//! 0.32.15 counts the whole prompt, cached part included), the time to read
//! the prompt (`prompt_eval_duration`, which shows the cache), and the
//! tokens generated.
//!
//! The Campaign file and the Intents file are given on the command line
//! (`--campaign`, `--intents`) and must lie outside the repository: they
//! can hold a Player's own world. The report prints numbers only, never a
//! name or a line of the Campaign.

use std::{fmt::Write as _, fs, path::Path};

use threshold_core::{
    CallStats,
    rules::D10,
    store::Store,
    turn::{Engine, METRICS_STEP},
};

use super::{Ctx, Session, check_raw_log, scan::percentile, secs};

/// The numbers of one committed Turn.
#[derive(Default)]
struct TurnNumbers {
    total_ms: u64,
    /// Total minus the narration's generation: about when the first word
    /// of the Narration reaches the screen.
    first_word_ms: u64,
    proposal: Vec<CallStats>,
    narration: Option<CallStats>,
}

/// Read faster than this, in microseconds per prompt token, a prompt came
/// mostly from the cache: a cold read takes about 1,000 µs per token on the
/// target machine, a cached prefix almost nothing.
const REUSED_US_PER_TOKEN: u64 = 400;

fn reused(c: &CallStats) -> bool {
    c.prompt_tokens > 0 && c.prompt_ms * 1_000 < c.prompt_tokens * REUSED_US_PER_TOKEN
}

/// The `metrics` rows of one operation, as `(call, stats)`.
fn call_stats(s: &Session, op_id: &str) -> Vec<(String, CallStats)> {
    s.engine
        .store()
        .debug_log(op_id)
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.step == METRICS_STEP)
        .filter_map(|row| {
            let call = row.raw.split_whitespace().next()?.strip_prefix("call=")?;
            Some((call.to_owned(), CallStats::parse(&row.raw)?))
        })
        .collect()
}

fn line(label: &str, values: &[u64], unit: &str) -> String {
    let show = |v: Option<u64>| v.map_or_else(|| "n/a".into(), |v| format!("{v}{unit}"));
    format!(
        "| {label} | {} | {} | {} |",
        show(percentile(values, 50)),
        show(percentile(values, 90)),
        show(values.iter().max().copied())
    )
}

pub(crate) fn scenario(ctx: &mut Ctx) -> Result<String, String> {
    let (Some(campaign), Some(intents)) = (&ctx.config.campaign, &ctx.config.intents) else {
        return Err("replay needs --campaign <file> and --intents <file>".into());
    };
    check_raw_log(&ctx.config.root, campaign)?;
    check_raw_log(&ctx.config.root, intents)?;
    let intents: Vec<String> = fs::read_to_string(intents)
        .map_err(|e| format!("{}: {e}", intents.display()))?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect();
    let mut turns: Vec<TurnNumbers> = Vec::new();
    let mut failed = 0;
    for run in 1..=ctx.config.runs {
        let copy = copy_path(campaign, run);
        fs::copy(campaign, &copy).map_err(|e| format!("{}: {e}", copy.display()))?;
        let store = Store::open(&copy).map_err(|e| e.to_string())?;
        let id = *store
            .campaigns()
            .map_err(|e| e.to_string())?
            .first()
            .ok_or("the Campaign file holds no Campaign")?;
        let engine = Engine::new(store, (ctx.gm)(), Box::new(D10), vec![], id);
        let mut s = Session {
            engine,
            campaign: id,
            label: format!("replay-r{run}"),
            n: 0,
            intents: Vec::new(),
        };
        for intent in &intents {
            let played = ctx.play(&mut s, intent);
            let Ok(outcome) = &played.outcome else {
                failed += 1;
                continue;
            };
            let calls = call_stats(&s, &played.op_id);
            let narration = calls
                .iter()
                .find(|(call, _)| call == "narrate")
                .map(|(_, stats)| *stats);
            let numbers = TurnNumbers {
                total_ms: outcome.timings.total_ms,
                first_word_ms: outcome
                    .timings
                    .total_ms
                    .saturating_sub(narration.map_or(0, |n| n.eval_ms)),
                proposal: calls
                    .iter()
                    .filter(|(call, _)| call.starts_with("propose"))
                    .map(|(_, stats)| *stats)
                    .collect(),
                narration,
            };
            (ctx.log)(&format!(
                "{} replay: total {} ms, first word {} ms, proposal read {:?} ms of {:?} tok, narration read {} ms of {} tok",
                played.op_id,
                numbers.total_ms,
                numbers.first_word_ms,
                numbers
                    .proposal
                    .iter()
                    .map(|p| p.prompt_ms)
                    .collect::<Vec<_>>(),
                numbers
                    .proposal
                    .iter()
                    .map(|p| p.prompt_tokens)
                    .collect::<Vec<_>>(),
                narration.map_or(0, |n| n.prompt_ms),
                narration.map_or(0, |n| n.prompt_tokens)
            ));
            turns.push(numbers);
        }
        drop(s);
        // `THRESHOLD_KEEP_REPLAY` keeps the copy, with its debug log, for a
        // person to read.
        if std::env::var_os("THRESHOLD_KEEP_REPLAY").is_none() {
            let _ = fs::remove_file(&copy);
        }
    }
    let total: Vec<u64> = turns.iter().map(|t| t.total_ms).collect();
    let first: Vec<u64> = turns.iter().map(|t| t.first_word_ms).collect();
    let first_proposals: Vec<&CallStats> =
        turns.iter().filter_map(|t| t.proposal.first()).collect();
    let pick = |f: fn(&CallStats) -> u64, calls: &[&CallStats]| -> Vec<u64> {
        calls.iter().map(|c| f(c)).collect()
    };
    let narrations: Vec<&CallStats> = turns.iter().filter_map(|t| t.narration.as_ref()).collect();
    let over = total.iter().filter(|ms| **ms > 15_000).count();
    let retries = turns.iter().filter(|t| t.proposal.len() > 1).count();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} Intents per run on a copy of a Campaign file outside the repository, {} run(s). Prompt tokens are Ollama's `prompt_eval_count`, which counts the whole prompt; the prompt read time shows the cache. A call counts as reused when its prompt read takes under {REUSED_US_PER_TOKEN} µs per token. Only the first proposal call of a Turn is counted below; a retry is counted apart.\n",
        intents.len(),
        ctx.config.runs
    );
    let _ = writeln!(out, "| Measure | p50 | p90 | max |");
    let _ = writeln!(out, "|---|---|---|---|");
    let _ = writeln!(out, "{}", line("Turn time, ms", &total, ""));
    let _ = writeln!(
        out,
        "{}",
        line("First word of the Narration, ms", &first, "")
    );
    let _ = writeln!(
        out,
        "{}",
        line(
            "Proposal: prompt tokens",
            &pick(|c| c.prompt_tokens, &first_proposals),
            ""
        )
    );
    let _ = writeln!(
        out,
        "{}",
        line(
            "Proposal: prompt read, ms",
            &pick(|c| c.prompt_ms, &first_proposals),
            ""
        )
    );
    let _ = writeln!(
        out,
        "{}",
        line(
            "Proposal: tokens generated",
            &pick(|c| c.eval_tokens, &first_proposals),
            ""
        )
    );
    let _ = writeln!(
        out,
        "{}",
        line(
            "Proposal: generation, ms",
            &pick(|c| c.eval_ms, &first_proposals),
            ""
        )
    );
    let _ = writeln!(
        out,
        "{}",
        line(
            "Narration: prompt tokens",
            &pick(|c| c.prompt_tokens, &narrations),
            ""
        )
    );
    let _ = writeln!(
        out,
        "{}",
        line(
            "Narration: prompt read, ms",
            &pick(|c| c.prompt_ms, &narrations),
            ""
        )
    );
    let _ = writeln!(
        out,
        "{}",
        line(
            "Narration: tokens generated",
            &pick(|c| c.eval_tokens, &narrations),
            ""
        )
    );
    let _ = writeln!(
        out,
        "{}",
        line(
            "Narration: generation, ms",
            &pick(|c| c.eval_ms, &narrations),
            ""
        )
    );
    let _ = writeln!(
        out,
        "\n- Calls that reused the cache: proposals {} of {}, narrations {} of {}",
        first_proposals.iter().filter(|c| reused(c)).count(),
        first_proposals.len(),
        narrations.iter().filter(|c| reused(c)).count(),
        narrations.len()
    );
    let _ = writeln!(
        out,
        "- Turns committed: {} of {}; failed: {failed}; Turns with a retry: {retries}",
        turns.len(),
        intents.len() * ctx.config.runs
    );
    let _ = writeln!(
        out,
        "- Turns over 15 s: {over}; Turn time p50 {}, p90 {}, max {}",
        secs(percentile(&total, 50)),
        secs(percentile(&total, 90)),
        secs(total.iter().max().copied())
    );
    Ok(out)
}

/// Where run `run` plays: a copy next to the Campaign file.
fn copy_path(campaign: &Path, run: usize) -> std::path::PathBuf {
    let stem = campaign
        .file_stem()
        .map_or_else(|| "campaign".into(), |s| s.to_string_lossy().into_owned());
    campaign.with_file_name(format!("{stem}.replay-r{run}.sqlite"))
}
