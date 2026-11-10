//! The evaluation runner (design §10, plan Task 11).
//!
//! Each scenario plays scripted Player Intents through the real `Engine`
//! against a `Gm` (the live model, or recorded answers in tests) and turns
//! what it observes into numbers: Turn times, the Commit-check rejections in
//! the `debug_log`, the ledger state, the Narration text, and a `Judge` for
//! what code cannot read. The model is not deterministic, so the report
//! gives counts, never a pass/fail, except the latency pass line.

mod judge;
mod play_basics;
pub mod scan;
mod scenario;
pub mod speech;
pub mod world_build;

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub use judge::{GmJudge, JUDGE_RULES, Judge, JudgeError};
use threshold_core::{
    Gm,
    checks::ceiling,
    domain::{Cause, Effect, EntityKind, EntityRef, Handle, LedgerOp, Reaction, Slot, Trigger},
    fixture::{load_adventure_seed, load_world},
    normalize,
    rules::D10,
    store::{CampaignId, Store, TurnKind, TurnRecord, WorldView},
    turn::{Engine, TurnOutcome, create_campaign},
    world::{beliefs, pc_knowledge},
};

use scan::{first_quote, forbidden_tokens, leaks, mentions_any, percentile, without_names};

/// Every scenario, in report order.
pub const SCENARIOS: [&str; 9] = [
    "latency",
    "memory",
    "voices",
    "shortcuts",
    "agency_leaks",
    "offscreen",
    "world_build",
    "speech",
    "play_basics",
];

/// The latency pass line (design §10): p90 at or under 15 s, warm.
pub const P90_PASS_MS: u64 = 15_000;

/// The Commit checks, as `CheckKind` prints them at the start of a rejection
/// line in the debug log.
const CHECK_NAMES: [&str; 9] = [
    "schema",
    "references",
    "agency",
    "contest",
    "coincidence",
    "npc folds",
    "fact bends",
    "tension evaporates",
    "content boundary",
];

pub struct Config {
    /// The repository root: `evals/scenarios` and `fixtures` live under it.
    pub root: PathBuf,
    pub scenarios: Vec<String>,
    pub runs: usize,
    /// Labels for the report.
    pub model: String,
    pub judge: String,
    /// Overrides the memory scenario's `total_turns`.
    pub memory_turns: Option<usize>,
    /// Override the world_build scenario's Source Text and expected list.
    /// A file outside `<root>/fixtures/` makes the report print counts only.
    pub source: Option<PathBuf>,
    pub expected: Option<PathBuf>,
}

pub type GmFactory<'a> = &'a dyn Fn() -> Box<dyn Gm + Send>;

/// Runs the scenarios and returns the Markdown report. `gm` plays Turns;
/// `build` drafts and builds worlds, with a longer timeout.
pub fn run(
    config: &Config,
    gm: GmFactory<'_>,
    build: GmFactory<'_>,
    judge: &dyn Judge,
    log: &mut dyn FnMut(&str),
) -> Result<String, String> {
    for name in &config.scenarios {
        if !SCENARIOS.contains(&name.as_str()) {
            return Err(format!("unknown scenario {name}"));
        }
    }
    let declared = if config
        .root
        .join("evals/scenarios/agency_leaks.toml")
        .exists()
    {
        scenario::load::<scenario::AgencyLeaks>(&config.root, "agency_leaks")?.secret_tokens
    } else {
        Vec::new()
    };
    let start = Instant::now();
    let mut ctx = Ctx {
        config,
        gm,
        build,
        judge,
        log,
        declared,
        totals: Totals::default(),
    };
    let mut sections = BTreeMap::new();
    // agency_leaks reads the totals of every other scenario, so it runs last.
    let mut order: Vec<&str> = SCENARIOS
        .iter()
        .copied()
        .filter(|s| config.scenarios.iter().any(|c| c == s))
        .collect();
    order.sort_by_key(|s| *s == "agency_leaks");
    for name in order {
        (ctx.log)(&format!("== scenario {name}"));
        let section = match name {
            "latency" => latency(&mut ctx)?,
            "memory" => memory(&mut ctx)?,
            "voices" => voices(&mut ctx)?,
            "shortcuts" => shortcuts(&mut ctx)?,
            "agency_leaks" => agency_leaks(&mut ctx)?,
            "offscreen" => offscreen(&mut ctx)?,
            "world_build" => world_build::scenario(&mut ctx)?,
            "speech" => speech::scenario(&mut ctx)?,
            "play_basics" => play_basics::scenario(&mut ctx)?,
            _ => unreachable!("checked above"),
        };
        sections.insert(name, section);
    }

    let t = &ctx.totals;
    let mut out = String::new();
    let _ = writeln!(out, "# Threshold evaluation report\n");
    let _ = writeln!(out, "- Date: {}", today());
    let _ = writeln!(out, "- Model: {}", config.model);
    let _ = writeln!(out, "- Judge: {}", config.judge);
    let _ = writeln!(out, "- Runs: {}", config.runs);
    let _ = writeln!(out, "- Scenarios: {}", config.scenarios.join(", "));
    let _ = writeln!(
        out,
        "- Wall time: {:.1} min",
        start.elapsed().as_secs_f64() / 60.0
    );
    let _ = writeln!(
        out,
        "- Turns played: {} ({} failed: both proposal attempts rejected, or a model error)",
        t.turns, t.failed
    );
    let _ = writeln!(
        out,
        "- Turn time over every scenario, over committed Turns only: p50 {}, p90 {} ({} committed Turns; not timed: {} failed Turns, of which {} timed out)",
        secs(percentile(&t.total_ms, 50)),
        secs(percentile(&t.total_ms, 90)),
        t.total_ms.len(),
        t.failed,
        t.timed_out
    );
    let _ = writeln!(
        out,
        "- Turns that needed the retry: {} of {}",
        t.retried, t.turns
    );
    for name in SCENARIOS {
        if let Some(section) = sections.get(name) {
            let _ = write!(out, "\n## {name}\n\n{section}");
        }
    }
    if !t.errors.is_empty() {
        let _ = writeln!(
            out,
            "\n## Turn failures\n\nGrouped by cause; a rejected Turn is grouped by the checks that rejected its last attempt.\n"
        );
        for (error, n) in &t.errors {
            let _ = writeln!(out, "- {n} x {error}");
        }
    }
    Ok(out)
}

/// One evaluation run's shared state.
struct Ctx<'a> {
    config: &'a Config,
    gm: GmFactory<'a>,
    /// Drafts and World Builds.
    build: GmFactory<'a>,
    judge: &'a dyn Judge,
    log: &'a mut dyn FnMut(&str),
    /// The scenario-declared secret stems; every Narration is scanned for them.
    declared: Vec<String>,
    totals: Totals,
}

#[derive(Default)]
struct Totals {
    turns: usize,
    failed: usize,
    /// Failed Turns whose model request timed out.
    timed_out: usize,
    retried: usize,
    total_ms: Vec<u64>,
    /// Turns with at least one proposal attempt the agency check rejected.
    agency_turns: usize,
    /// Committed Turns whose Narration holds a forbidden token.
    leak_turns: usize,
    committed: usize,
    leak_tokens: BTreeMap<String, usize>,
    errors: BTreeMap<String, usize>,
}

/// One played Turn.
struct Played {
    op_id: String,
    outcome: Result<TurnOutcome, String>,
    /// The rejection lines of each proposal attempt; empty when it passed.
    attempts: Vec<Vec<String>>,
}

impl Played {
    fn narration(&self) -> Option<&str> {
        self.outcome.as_ref().ok().map(|o| o.narration.as_str())
    }

    /// The checks named in the rejections of each attempt.
    fn rejected_by(&self) -> Vec<Vec<&'static str>> {
        self.attempts
            .iter()
            .map(|lines| check_kinds(lines))
            .collect()
    }
}

/// The distinct checks named in one attempt's rejection lines, in
/// `CHECK_NAMES` order.
pub fn check_kinds(lines: &[String]) -> Vec<&'static str> {
    CHECK_NAMES
        .iter()
        .copied()
        .filter(|k| lines.iter().any(|l| l.starts_with(k)))
        .collect()
}

struct Session {
    engine: Engine,
    campaign: CampaignId,
    label: String,
    n: usize,
    intents: Vec<String>,
}

impl Ctx<'_> {
    /// A fresh campaign from `fixtures/<fixture>`, then `setup` on its store.
    fn session(
        &mut self,
        fixture: &str,
        label: String,
        rng_seed: u64,
        setup: impl FnOnce(&mut Store, CampaignId) -> Result<(), String>,
    ) -> Result<Session, String> {
        let dir = self.config.root.join("fixtures").join(fixture);
        let seed =
            load_adventure_seed(&dir.join("adventure-seed.toml")).map_err(|e| e.to_string())?;
        let mut store = Store::open_in_memory().map_err(|e| e.to_string())?;
        let campaign = create_campaign(&mut store, &seed, rng_seed).map_err(|e| e.to_string())?;
        load_world(&mut store, campaign, &dir.join("world.toml")).map_err(|e| e.to_string())?;
        setup(&mut store, campaign)?;
        let engine = Engine::new(store, (self.gm)(), Box::new(D10), vec![], campaign);
        Ok(Session {
            engine,
            campaign,
            label,
            n: 0,
            intents: Vec::new(),
        })
    }

    fn play(&mut self, s: &mut Session, intent: &str) -> Played {
        s.n += 1;
        let op_id = format!("{}-t{}", s.label, s.n);
        s.intents.push(intent.to_owned());
        let outcome = s
            .engine
            .play_turn(op_id.clone(), intent, &mut |_: &str| {})
            .map_err(|e| e.to_string());
        if std::env::var_os("THRESHOLD_EVAL_RAW").is_some() {
            for row in s.engine.store().debug_log(&op_id).unwrap_or_default() {
                if row.step.starts_with("propose") || row.step == "settle" {
                    (self.log)(&format!("RAW {op_id} {}: {}", row.step, row.raw));
                }
            }
        }
        let attempts: Vec<Vec<String>> = s
            .engine
            .store()
            .debug_log(&op_id)
            .unwrap_or_default()
            .into_iter()
            .filter(|row| row.step == "validate")
            .map(|row| {
                if row.raw == "ok" {
                    Vec::new()
                } else {
                    row.raw.lines().map(str::to_owned).collect()
                }
            })
            .collect();
        let played = Played {
            op_id,
            outcome,
            attempts,
        };

        let t = &mut self.totals;
        t.turns += 1;
        t.retried += usize::from(played.attempts.len() > 1);
        if played
            .rejected_by()
            .iter()
            .any(|kinds| kinds.contains(&"agency"))
        {
            t.agency_turns += 1;
        }
        let status = match &played.outcome {
            Ok(o) => {
                t.committed += 1;
                t.total_ms.push(o.timings.total_ms);
                let view = s.engine.store().snapshot(s.campaign).unwrap_or_default();
                let found = leaks(
                    &forbidden_tokens(&view, &s.intents, &self.declared),
                    &o.narration,
                );
                if !found.is_empty() {
                    t.leak_turns += 1;
                    for token in &found {
                        *t.leak_tokens.entry(token.clone()).or_default() += 1;
                    }
                }
                format!(
                    "ok {} ms (propose {} ms, narrate {} ms){}",
                    o.timings.total_ms,
                    o.timings.propose_ms,
                    o.timings.narrate_ms,
                    if found.is_empty() {
                        String::new()
                    } else {
                        format!(" LEAK {found:?}")
                    }
                )
            }
            Err(e) => {
                t.failed += 1;
                let rejected = played.rejected_by();
                let key = match rejected.last() {
                    Some(kinds)
                        if !kinds.is_empty() && e.starts_with("the proposal was rejected") =>
                    {
                        format!(
                            "proposal rejected twice; last attempt by: {}",
                            kinds.join(", ")
                        )
                    }
                    _ if e.contains("timed out") => {
                        t.timed_out += 1;
                        "game model request timed out".to_owned()
                    }
                    _ => e.lines().next().unwrap_or("").chars().take(160).collect(),
                };
                *t.errors.entry(key).or_default() += 1;
                format!("FAILED {}", e.replace('\n', " | "))
            }
        };
        let rejected: Vec<String> = played
            .attempts
            .iter()
            .filter(|a| !a.is_empty())
            .map(|a| a.join(" | "))
            .collect();
        (self.log)(&format!(
            "{} {status}{}",
            played.op_id,
            if rejected.is_empty() {
                String::new()
            } else {
                format!(" rejected: {}", rejected.join(" || "))
            }
        ));
        played
    }

    /// Asks the judge; a failure counts as `None` and is logged.
    fn ask(&mut self, task: &str, choices: &[&str]) -> Option<String> {
        match self.judge.choose(task, choices) {
            Ok(answer) => Some(answer),
            Err(e) => {
                (self.log)(&format!("judge error: {e}"));
                None
            }
        }
    }
}

fn secs(ms: Option<u64>) -> String {
    ms.map_or_else(|| "n/a".into(), |ms| format!("{:.1} s", ms as f64 / 1000.0))
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        "n/a".into()
    } else {
        format!("{:.0}%", 100.0 * n as f64 / d as f64)
    }
}

fn per_100(n: usize, d: usize) -> String {
    if d == 0 {
        "n/a".into()
    } else {
        format!("{:.1}", 100.0 * n as f64 / d as f64)
    }
}

fn entity_id(view: &WorldView, name: &str) -> Result<u64, String> {
    view.entity_by_name(name)
        .map(|e| e.id)
        .ok_or_else(|| format!("no entity {}", normalize::name(name)))
}

fn pc_location(view: &WorldView) -> Option<String> {
    let pc = view.entities.iter().find(|e| e.kind == EntityKind::Pc)?;
    view.live_fact(pc.id, "location")
        .map(|f| f.slot.value.clone())
}

fn latency(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::Latency = scenario::load(&ctx.config.root, "latency")?;
    let (mut total, mut propose, mut narrate) = (Vec::new(), Vec::new(), Vec::new());
    let (mut failed, mut timed_out) = (0, 0);
    for run in 1..=ctx.config.runs {
        let mut s = ctx.session(
            &sc.fixture,
            format!("latency-r{run}"),
            1_000 + run as u64,
            |_, _| Ok(()),
        )?;
        for intent in &sc.intents {
            match ctx.play(&mut s, intent).outcome {
                Ok(o) => {
                    total.push(o.timings.total_ms);
                    propose.push(o.timings.propose_ms);
                    narrate.push(o.timings.narrate_ms);
                }
                Err(e) => {
                    failed += 1;
                    timed_out += usize::from(e.contains("timed out"));
                }
            }
        }
    }
    let p90 = percentile(&total, 90);
    let verdict = match p90 {
        Some(ms) if ms <= P90_PASS_MS => "PASS",
        Some(_) => "FAIL",
        None => "not measured",
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} Intents per run, {} committed Turns, {failed} failed ({timed_out} timed out). Turn time = proposal (with the retry when it runs), checks, Commit and the full streamed Narration. The percentiles cover committed Turns only: a failed Turn has no time, so a timeout (60 s) is not in them.\n",
        sc.intents.len(),
        total.len()
    );
    let _ = writeln!(
        out,
        "- Turn time p50 (committed Turns only): {}",
        secs(percentile(&total, 50))
    );
    let _ = writeln!(
        out,
        "- Turn time p90 (committed Turns only): {} (pass line <= 15 s warm: {verdict}); failed Turns not timed: {failed}, of which {timed_out} timed out",
        secs(p90)
    );
    let _ = writeln!(
        out,
        "- Turn time max: {}",
        secs(total.iter().max().copied())
    );
    let _ = writeln!(
        out,
        "- Proposal step p50 / p90: {} / {}",
        secs(percentile(&propose, 50)),
        secs(percentile(&propose, 90))
    );
    let _ = writeln!(
        out,
        "- Narration step p50 / p90: {} / {}",
        secs(percentile(&narrate, 50)),
        secs(percentile(&narrate, 90))
    );
    Ok(out)
}

/// True when the ledger state names the detail: an entity name, a fact
/// (value or text), a Claim, or any holder's Knowledge mentions one of the
/// `expect` stems. Turn records (Intents, Narrations) do not count.
pub fn detail_in_ledger(view: &WorldView, expect: &[String]) -> bool {
    view.entities.iter().any(|e| mentions_any(&e.name, expect))
        || view
            .facts
            .iter()
            .any(|f| mentions_any(&format!("{} {}", f.slot.value, f.text), expect))
        || view
            .claims
            .iter()
            .any(|c| mentions_any(&format!("{} {}", c.claim.slot.value, c.claim.text), expect))
        || view.entities.iter().any(|e| {
            beliefs(view, e.id)
                .iter()
                .any(|b| mentions_any(&format!("{} {}", b.value, b.text), expect))
        })
}

fn memory(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::Memory = scenario::load(&ctx.config.root, "memory")?;
    let total = ctx.config.memory_turns.unwrap_or(sc.total_turns);
    if total <= sc.plant_turn || sc.filler.is_empty() {
        return Err(
            "memory: total_turns must exceed plant_turn, and filler must not be empty".into(),
        );
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut stored = 0;
    let mut rows = Vec::new();
    for run in 1..=ctx.config.runs {
        let mut s = ctx.session(
            &sc.fixture,
            format!("memory-r{run}"),
            2_000 + run as u64,
            |_, _| Ok(()),
        )?;
        let mut filler = sc.filler.iter().cycle();
        let mut first_seen: Option<usize> = None;
        let mut at_probe = false;
        let mut plant_failed = false;
        let mut probe = None;
        for t in 1..=total {
            let intent = if t == sc.plant_turn {
                sc.plant.as_str()
            } else if t == total {
                sc.probe.as_str()
            } else {
                filler.next().map_or("", String::as_str)
            };
            let played = ctx.play(&mut s, intent);
            if t == sc.plant_turn {
                plant_failed = played.outcome.is_err();
            }
            if t >= sc.plant_turn {
                let view = s.engine.store().snapshot(s.campaign).unwrap_or_default();
                let present = detail_in_ledger(&view, &sc.expect);
                if present && first_seen.is_none() {
                    first_seen = Some(t);
                }
                if t + 1 == total {
                    at_probe = present;
                }
            }
            if t == total {
                probe = Some(played);
            }
        }
        stored += usize::from(first_seen.is_some());
        let verdict = match probe.as_ref().and_then(Played::narration) {
            None => "failed",
            Some(text) => {
                let task = format!(
                    "Earlier in the game this detail was established: {}\n\nMuch later, the player asked: {}\n\nThe game master narrated:\n{text}\n\nDoes the narration use the detail correctly (correct), state a different detail that contradicts it (wrong), or not use it at all (missing)?",
                    sc.detail, sc.probe
                );
                match ctx.ask(&task, &["correct", "wrong", "missing"]).as_deref() {
                    Some("correct") => "correct",
                    Some(_) => "not recalled",
                    None => "judge error",
                }
            }
        };
        let keyword = probe
            .as_ref()
            .and_then(Played::narration)
            .is_some_and(|text| mentions_any(text, &sc.expect));
        *counts.entry(verdict).or_default() += 1;
        rows.push(format!(
            "| {run} | {} | {} | {} | {} | {verdict} |",
            if plant_failed { "failed" } else { "committed" },
            first_seen.map_or_else(|| "never".to_owned(), |t| format!("Turn {t}")),
            if at_probe { "yes" } else { "no" },
            if keyword { "yes" } else { "no" },
        ));
    }
    let n = |k: &str| counts.get(k).copied().unwrap_or(0);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Detail planted on Turn {}, probed on Turn {total} (the scenario file asks for {}). After every Turn from the plant on, the runner searches the ledger state (entity names, facts, Claims, every holder's Knowledge) for the detail. Correct = the judge confirms the probe Narration uses the detail correctly; a keyword alone is not enough. Wrong and missing are one bucket, not recalled.\n",
        sc.plant_turn, sc.total_turns
    );
    let _ = writeln!(
        out,
        "- Memory: correct {} / not recalled {} / Turn failed {} / judge error {} (of {} runs)",
        n("correct"),
        n("not recalled"),
        n("failed"),
        n("judge error"),
        ctx.config.runs
    );
    let _ = writeln!(
        out,
        "- Detail found in the ledger on some Turn after the plant: {stored} of {} runs\n",
        ctx.config.runs
    );
    let _ = writeln!(
        out,
        "| run | plant Turn | first in ledger | in ledger before the probe | keyword in probe Narration | verdict |\n|---|---|---|---|---|---|"
    );
    for row in rows {
        let _ = writeln!(out, "{row}");
    }
    Ok(out)
}

fn voices(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::Voices = scenario::load(&ctx.config.root, "voices")?;
    let mut speakers: Vec<String> = Vec::new();
    for line in &sc.lines {
        if !speakers.contains(&line.speaker) {
            speakers.push(line.speaker.clone());
        }
    }
    const LABELS: [&str; 6] = ["A", "B", "C", "D", "E", "F"];
    if speakers.len() > LABELS.len() {
        return Err("voices: at most 6 speakers".into());
    }
    let (mut judged, mut matched, mut no_dialogue, mut failed, mut judge_errors) = (0, 0, 0, 0, 0);
    let mut per_speaker: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut samples = Vec::new();
    for run in 1..=ctx.config.runs {
        let mut s = ctx.session(
            &sc.fixture,
            format!("voices-r{run}"),
            3_000 + run as u64,
            |_, _| Ok(()),
        )?;
        let view = s
            .engine
            .store()
            .snapshot(s.campaign)
            .map_err(|e| e.to_string())?;
        let mut cards = Vec::new();
        for name in &speakers {
            let card = view
                .voice_card(entity_id(&view, name)?)
                .ok_or_else(|| format!("voices: {name} has no Voice Card"))?;
            cards.push(format!(
                "habits: {}; vocabulary: {}; sample lines: {}",
                card.habits,
                card.vocabulary,
                card.samples.join(" / ")
            ));
        }
        let names: Vec<String> = view
            .entities
            .iter()
            .filter(|e| matches!(e.kind, EntityKind::Npc | EntityKind::Pc))
            .map(|e| e.name.clone())
            .collect();
        for (i, line) in sc.lines.iter().enumerate() {
            let played = ctx.play(&mut s, &line.intent);
            let Some(text) = played.narration() else {
                failed += 1;
                continue;
            };
            let Some(quote) = first_quote(text) else {
                no_dialogue += 1;
                continue;
            };
            let quote = without_names(&quote, &names);
            let k = speakers.len();
            let order: Vec<usize> = (0..k).map(|j| (j + i + run) % k).collect();
            let mut task = String::from(
                "Below are the voice descriptions of several characters, then one line of dialogue with names removed. Which character most likely spoke the line?\n\n",
            );
            for (label, card) in LABELS.iter().zip(&order) {
                let _ = writeln!(task, "{label}: {}", cards[*card]);
            }
            let _ = write!(task, "\nLine: \"{quote}\"");
            let Some(answer) = ctx.ask(&task, &LABELS[..k]) else {
                judge_errors += 1;
                continue;
            };
            let picked = LABELS[..k]
                .iter()
                .position(|l| *l == answer)
                .map(|p| speakers[order[p]].clone());
            judged += 1;
            let hit = picked.as_deref() == Some(line.speaker.as_str());
            matched += usize::from(hit);
            let entry = per_speaker.entry(line.speaker.clone()).or_default();
            entry.0 += 1;
            entry.1 += usize::from(hit);
            if samples.len() < 9 {
                samples.push(format!(
                    "- {} -> judged {}: \"{}\"",
                    line.speaker,
                    picked.unwrap_or_default(),
                    quote.chars().take(120).collect::<String>()
                ));
            }
        }
    }
    let chance = 1.0 / speakers.len().max(1) as f64;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "The truth is the NPC the Intent addresses; the judge sees the first quoted line of the Narration, names removed, and the {} Voice Cards in a rotating order. A Narration where someone else speaks first counts against the score, so this is a lower bound.\n",
        speakers.len()
    );
    let _ = writeln!(
        out,
        "- Voices: matched {matched} / {judged} judged lines ({}), chance {:.0}%",
        pct(matched, judged),
        chance * 100.0
    );
    let _ = writeln!(
        out,
        "- Turns without a quoted line: {no_dialogue}; Turns failed: {failed}; judge errors: {judge_errors}"
    );
    for (speaker, (n, hit)) in &per_speaker {
        let _ = writeln!(out, "- {speaker}: {hit} / {n}");
    }
    if !samples.is_empty() {
        let _ = writeln!(out, "\nSample lines:\n");
        for s in samples {
            let _ = writeln!(out, "{s}");
        }
    }
    Ok(out)
}

/// The Setup Turn that schedules a seeded World Event. Returns its id.
fn schedule_event(
    store: &mut Store,
    campaign: CampaignId,
    event: &scenario::TrapEvent,
) -> Result<u64, String> {
    let view = store.snapshot(campaign).map_err(|e| e.to_string())?;
    let actor = entity_id(&view, &event.actor)?;
    let subject = entity_id(&view, &event.subject)?;
    let seen_by = event
        .seen_by
        .iter()
        .map(|n| entity_id(&view, n).map(EntityRef::Id))
        .collect::<Result<Vec<_>, _>>()?;
    let record = TurnRecord {
        campaign_id: campaign,
        op_id: "__seeded_event__".into(),
        kind: TurnKind::Setup,
        intent: String::new(),
        proposal_json: "{}".into(),
        clock_before: view.clock_min,
        clock_after: view.clock_min,
    };
    let ops = vec![LedgerOp::ScheduleEvent {
        text: event.text.clone(),
        trigger: Trigger::At(view.clock_min + event.in_minutes),
        actors: vec![EntityRef::Id(actor)],
        goal_id: None,
        cause_fact_id: None,
        effects: vec![Effect::SetFact {
            slot: Slot {
                subject: EntityRef::Id(subject),
                key: normalize::key(&event.key),
                value: normalize::value(&event.value),
            },
            text: format!(
                "{}: {} {} {}",
                event.text, event.subject, event.key, event.value
            ),
            seen_by: seen_by.clone(),
        }],
        seen_by,
    }];
    store.commit(record, ops).map_err(|e| format!("{e:?}"))?;
    let after = store.snapshot(campaign).map_err(|e| e.to_string())?;
    after
        .events_scheduled
        .iter()
        .filter(|w| w.text == event.text)
        .map(|w| w.id)
        .max()
        .ok_or_else(|| "the seeded event was not scheduled".to_owned())
}

/// What a trap watches in the ledger, as entity ids.
#[derive(Clone, Debug, Default)]
pub struct Watch {
    /// `(subject, key)` slots that must keep their value.
    pub slots: Vec<(u64, String)>,
    /// The NPC whose stance must stay within its ceiling.
    pub npc: Option<u64>,
}

/// Did the committed Turn take the Shortcut? Read from the ledger only:
/// - `coincidence`: a watched slot changed, or its open obstacle closed,
///   and no Check succeeded this Turn (a won Check is the legal path);
/// - `fact bends`: a watched slot changed;
/// - `npc folds`: the stored reaction of the watched NPC is above its
///   ceiling (computed before the Turn), unless it cites a Check that
///   succeeded;
/// - `tension evaporates`: an event scheduled before the Turn is gone
///   without a `FireEvent` in this Turn's ops.
pub fn shortcut_committed(
    shortcut: &str,
    watch: &Watch,
    before: &WorldView,
    after: &WorldView,
    turn_ops: &[LedgerOp<u64>],
    proposal_json: &str,
    roll_success: bool,
) -> Result<bool, String> {
    let value = |view: &WorldView, (subject, key): &(u64, String)| {
        view.live_fact(*subject, key).map(|f| f.slot.value.clone())
    };
    let slot_changed = || {
        watch
            .slots
            .iter()
            .any(|s| value(before, s) != value(after, s))
    };
    let obstacle_on = |view: &WorldView, (subject, key): &(u64, String)| {
        view.obstacles_open.iter().any(|o| {
            view.facts.iter().any(|f| {
                f.id == o.fact_id && f.slot.subject == *subject && f.slot.key == normalize::key(key)
            })
        })
    };
    Ok(match shortcut {
        "coincidence" => {
            let closed = watch
                .slots
                .iter()
                .any(|s| obstacle_on(before, s) && !obstacle_on(after, s));
            (slot_changed() || closed) && !roll_success
        }
        "fact bends" => slot_changed(),
        "npc folds" => {
            let npc = watch.npc.ok_or("npc folds needs an npc to watch")?;
            let record: serde_json::Value =
                serde_json::from_str(proposal_json).map_err(|e| e.to_string())?;
            let reactions: Vec<Reaction> = record
                .get("reactions")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            let limit = ceiling(before, npc);
            reactions.iter().any(|r| {
                r.npc == Handle::Entity(npc)
                    && r.stance > limit
                    && !(r.cause == Some(Cause::Check) && roll_success)
            })
        }
        "tension evaporates" => before.events_scheduled.iter().any(|w| {
            !after.events_scheduled.iter().any(|a| a.id == w.id)
                && !turn_ops.contains(&LedgerOp::FireEvent { event_id: w.id })
        }),
        other => return Err(format!("no ledger rule for {other}")),
    })
}

#[derive(Default)]
struct TrapStats {
    turns: usize,
    failed: usize,
    committed: usize,
    shortcut: usize,
    attempts: usize,
    caught: usize,
    judge_taken: usize,
    judge_errors: usize,
    other: BTreeMap<&'static str, usize>,
}

fn shortcuts(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::Shortcuts = scenario::load(&ctx.config.root, "shortcuts")?;
    let mut table = Vec::new();
    for (ti, trap) in sc.traps.iter().enumerate() {
        let target = CHECK_NAMES
            .iter()
            .copied()
            .find(|k| *k == trap.shortcut)
            .ok_or_else(|| format!("shortcuts: unknown check {}", trap.shortcut))?;
        let mut st = TrapStats::default();
        for run in 1..=ctx.config.runs {
            for (k, intent) in trap.intents.iter().enumerate() {
                let label = format!("shortcut{}-{}-r{run}", ti + 1, k + 1);
                let seed = 4_000 + (ti * 100 + k * 10 + run) as u64;
                let mut s = ctx.session(&sc.fixture, label, seed, |store, campaign| {
                    trap.event
                        .as_ref()
                        .map_or(Ok(()), |e| schedule_event(store, campaign, e).map(|_| ()))
                })?;
                let before = s
                    .engine
                    .store()
                    .snapshot(s.campaign)
                    .map_err(|e| e.to_string())?;
                let watch = Watch {
                    slots: trap
                        .slots
                        .iter()
                        .map(|[subject, key]| Ok((entity_id(&before, subject)?, key.clone())))
                        .collect::<Result<_, String>>()?,
                    npc: trap
                        .npc
                        .as_deref()
                        .map(|n| entity_id(&before, n))
                        .transpose()?,
                };
                let played = ctx.play(&mut s, intent);
                st.turns += 1;
                st.attempts += played.attempts.len();
                for kinds in played.rejected_by() {
                    if kinds.contains(&target) {
                        st.caught += 1;
                    }
                    for kind in kinds.into_iter().filter(|k| *k != target) {
                        *st.other.entry(kind).or_default() += 1;
                    }
                }
                let Ok(outcome) = &played.outcome else {
                    st.failed += 1;
                    continue;
                };
                st.committed += 1;
                let store = s.engine.store();
                let after = store.snapshot(s.campaign).map_err(|e| e.to_string())?;
                let ops = store.turn_ops(outcome.turn_id).map_err(|e| e.to_string())?;
                let decision = store
                    .turn(outcome.turn_id)
                    .map_err(|e| e.to_string())?
                    .map(|t| t.record.proposal_json)
                    .unwrap_or_default();
                let success = outcome.roll.is_some_and(|r| r.success);
                if shortcut_committed(target, &watch, &before, &after, &ops, &decision, success)? {
                    st.shortcut += 1;
                    (ctx.log)(&format!("{} SHORTCUT COMMITTED ({target})", played.op_id));
                }
                let task = format!(
                    "The player tried to make the story take a shortcut. The shortcut is taken when: {}\n\nPlayer intent: {intent}\n\nThe narration:\n{}\n\nDoes the narration show the shortcut taken?",
                    trap.taken, outcome.narration
                );
                match ctx.ask(&task, &["yes", "no"]).as_deref() {
                    Some("yes") => st.judge_taken += 1,
                    Some(_) => {}
                    None => st.judge_errors += 1,
                }
            }
        }
        let other = if st.other.is_empty() {
            "none".to_owned()
        } else {
            st.other
                .iter()
                .map(|(k, n)| format!("{k} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        table.push(format!(
            "| {} | {} | {} | {} of {} | {} | {} | {} of {} ({} errors) | {} |",
            trap.shortcut,
            st.turns,
            st.failed,
            st.shortcut,
            st.committed,
            st.attempts,
            st.caught,
            st.judge_taken,
            st.committed,
            st.judge_errors,
            other
        ));
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Each trap Intent runs once per run in a fresh campaign. Units: Turns, except the two attempt columns (one attempt = one proposal call; the retry is a second attempt). \"Shortcut committed\" comes from the ledger only, read after the Turn: coincidence = the archive lock slot changed or its obstacle closed without a won Check; npc folds = Orsel's committed stance is above his ceiling; fact bends = a Persistent role changed; tension evaporates = the seeded event left the schedule without firing. Rejected by the check = attempts the named check rejected, read from the debug log. The judge column is a second opinion on the Narration, not the metric.\n"
    );
    let _ = writeln!(
        out,
        "| Shortcut | trap Turns | Turns failed | Shortcut committed (ledger, of committed Turns) | attempts | attempts rejected by the named check | judge: Narration shows it taken | other rejections (attempts) |\n|---|---|---|---|---|---|---|---|"
    );
    for row in table {
        let _ = writeln!(out, "{row}");
    }
    Ok(out)
}

fn agency_leaks(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::AgencyLeaks = scenario::load(&ctx.config.root, "agency_leaks")?;
    let (mut judged, mut agency_yes, mut leak_yes, mut judge_errors) = (0, 0, 0, 0);
    let mut examples = Vec::new();
    for run in 1..=ctx.config.runs {
        let mut s = ctx.session(
            &sc.fixture,
            format!("agency-r{run}"),
            5_000 + run as u64,
            |_, _| Ok(()),
        )?;
        for intent in &sc.intents {
            let played = ctx.play(&mut s, intent);
            let Some(text) = played.narration() else {
                continue;
            };
            let text = text.to_owned();
            judged += 1;
            let agency = ctx.ask(
                &format!(
                    "The player controls the player character (PC) alone. The game master may describe what happens as a result of the player's intent, but must never decide for the PC: no new actions, words, decisions, thoughts or feelings the intent did not state.\n\nPlayer intent: {intent}\n\nNarration:\n{text}\n\nDoes the narration make the PC act, speak, decide, think or feel something the intent did not state?"
                ),
                &["yes", "no"],
            );
            let leak = ctx.ask(
                &format!(
                    "These are secrets the player character does not know:\n- {}\n\nNarration:\n{text}\n\nDoes the narration reveal, or clearly hint at, any of these secrets?",
                    sc.secrets.join("\n- ")
                ),
                &["yes", "no"],
            );
            if agency.is_none() || leak.is_none() {
                judge_errors += 1;
            }
            if agency.as_deref() == Some("yes") {
                agency_yes += 1;
                if examples.len() < 4 {
                    examples.push(format!(
                        "- agency, \"{intent}\": {}",
                        text.chars()
                            .take(200)
                            .collect::<String>()
                            .replace('\n', " ")
                    ));
                }
            }
            if leak.as_deref() == Some("yes") {
                leak_yes += 1;
                if examples.len() < 8 {
                    examples.push(format!(
                        "- leak, \"{intent}\": {}",
                        text.chars()
                            .take(200)
                            .collect::<String>()
                            .replace('\n', " ")
                    ));
                }
            }
        }
    }
    let t = &ctx.totals;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "The token scan and the agency check counts cover every Turn of every scenario in this report. The judge passes cover the committed Turns of this scenario only ({} Intents per run).\n",
        sc.intents.len()
    );
    let _ = writeln!(
        out,
        "- Agency per 100 Turns, caught by the agency check (Turns with at least one rejected attempt): {} ({} of {} Turns)",
        per_100(t.agency_turns, t.turns),
        t.agency_turns,
        t.turns
    );
    let _ = writeln!(
        out,
        "- Agency per 100 Turns, in the Narration, per the judge: {} ({agency_yes} of {judged} Turns)",
        per_100(agency_yes, judged)
    );
    let _ = writeln!(
        out,
        "- Secret leaks per 100 Turns, token scan: {} ({} of {} committed Turns)",
        per_100(t.leak_turns, t.committed),
        t.leak_turns,
        t.committed
    );
    let _ = writeln!(
        out,
        "- Secret leaks per 100 Turns, per the judge: {} ({leak_yes} of {judged} Turns)",
        per_100(leak_yes, judged)
    );
    let _ = writeln!(out, "- Judge errors: {judge_errors}");
    if !t.leak_tokens.is_empty() {
        let tokens: Vec<String> = t
            .leak_tokens
            .iter()
            .map(|(k, n)| format!("\"{k}\" x{n}"))
            .collect();
        let _ = writeln!(out, "- Tokens the scan found: {}", tokens.join(", "));
    }
    if !examples.is_empty() {
        let _ = writeln!(out, "\nJudge flags (first 200 characters):\n");
        for e in examples {
            let _ = writeln!(out, "{e}");
        }
    }
    Ok(out)
}

/// The `action` of a stored proposal record, if any.
fn stored_action(proposal_json: &str) -> Option<String> {
    let record: serde_json::Value = serde_json::from_str(proposal_json).ok()?;
    record
        .pointer("/proposal/action")
        .and_then(|a| a.as_str())
        .map(str::to_owned)
}

fn offscreen(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::Offscreen = scenario::load(&ctx.config.root, "offscreen")?;
    let (mut model_scheduled, mut fired_away, mut learned, mut away_runs, mut observed) =
        (0, 0, 0, 0, 0);
    let mut rows = Vec::new();
    for run in 1..=ctx.config.runs {
        let mut seeded = 0;
        let mut s = ctx.session(
            &sc.fixture,
            format!("offscreen-r{run}"),
            6_000 + run as u64,
            |store, campaign| {
                seeded = schedule_event(store, campaign, &sc.event)?;
                Ok(())
            },
        )?;
        let snapshot = |s: &Session| s.engine.store().snapshot(s.campaign).unwrap_or_default();
        let ops_of = |s: &Session, p: &Played| -> Vec<LedgerOp<u64>> {
            p.outcome
                .as_ref()
                .ok()
                .and_then(|o| s.engine.store().turn_ops(o.turn_id).ok())
                .unwrap_or_default()
        };
        let start_at = pc_location(&snapshot(&s));
        let fires = |ops: &[LedgerOp<u64>]| ops.contains(&LedgerOp::FireEvent { event_id: seeded });

        // (a) Did the model schedule an event of its own on the provocation?
        let t1 = ctx.play(&mut s, &sc.provoke);
        let t1_ops = ops_of(&s, &t1);
        let scheduled_by_model = t1_ops
            .iter()
            .filter(|op| matches!(op, LedgerOp::ScheduleEvent { .. }))
            .count();
        model_scheduled += usize::from(scheduled_by_model > 0);

        let t2 = ctx.play(&mut s, &sc.away);
        let t2_ops = ops_of(&s, &t2);
        let away_at = pc_location(&snapshot(&s));
        let t2_action = t2
            .outcome
            .as_ref()
            .ok()
            .and_then(|o| s.engine.store().turn(o.turn_id).ok().flatten())
            .and_then(|t| stored_action(&t.record.proposal_json));
        let was_away = t2.outcome.is_ok() && away_at.is_some() && away_at != start_at;
        away_runs += usize::from(was_away);

        let t3 = ctx.play(&mut s, &sc.back);
        let t3_ops = ops_of(&s, &t3);
        let t4 = ctx.play(&mut s, &sc.ask);
        let view4 = snapshot(&s);

        // (b) Did the seeded event fire while the PC was away (on Turn 2)?
        let fired_on = if fires(&t1_ops) {
            Some(1)
        } else if fires(&t2_ops) {
            Some(2)
        } else if fires(&t3_ops) {
            Some(3)
        } else if fires(&ops_of(&s, &t4)) {
            Some(4)
        } else {
            None
        };
        let fired_while_away = fired_on == Some(2) && was_away;
        fired_away += usize::from(fired_while_away);

        // (c) Did the PC learn of it through play, by the end of Turn 4?
        let subject = entity_id(&view4, &sc.event.subject)?;
        let key = normalize::key(&sc.event.key);
        let value = normalize::value(&sc.event.value);
        let belief = pc_knowledge(&view4)
            .into_iter()
            .find(|b| b.subject == subject && b.key == key && normalize::value(&b.value) == value);
        learned += usize::from(belief.is_some());
        observed += usize::from(fired_while_away && belief.is_some());

        let failed: Vec<String> = [&t1, &t2, &t3, &t4]
            .iter()
            .enumerate()
            .filter(|(_, p)| p.outcome.is_err())
            .map(|(i, _)| format!("Turn {} failed", i + 1))
            .collect();
        let mut how = Vec::new();
        if !failed.is_empty() {
            how.push(failed.join(", "));
        }
        if !was_away {
            how.push(format!(
                "PC did not leave on Turn 2 (action {}, location after: {})",
                t2_action.as_deref().unwrap_or("none"),
                away_at.as_deref().unwrap_or("none")
            ));
        }
        how.push(match fired_on {
            None => "seeded event never fired".to_owned(),
            Some(2) if was_away => "seeded event fired on Turn 2 while the PC was away".to_owned(),
            Some(t) => format!("seeded event fired on Turn {t}, not while the PC was away"),
        });
        how.push(match &belief {
            Some(b) => format!(
                "PC learned it ({:?}{})",
                b.how,
                b.source
                    .and_then(|id| view4.entities.iter().find(|e| e.id == id))
                    .map_or_else(String::new, |e| format!(" by {}", e.name))
            ),
            None => "PC Knowledge does not hold it after Turn 4".to_owned(),
        });
        rows.push(format!(
            "| {run} | {scheduled_by_model} | {} | {} | {} | {} | {} |",
            t2_action.as_deref().unwrap_or("failed"),
            away_at.as_deref().unwrap_or("none"),
            fired_on.map_or_else(|| "never".to_owned(), |t| format!("Turn {t}")),
            if belief.is_some() { "yes" } else { "no" },
            how.join("; ")
        ));
    }
    let n = ctx.config.runs;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Four Turns per run: provoke Orsel, leave town for the day, come back, ask Hessa. Before Turn 1 the runner seeds the consequence as a Setup Turn (\"{}\", {} minutes later, witnessed by {}), so the engine path is measured whatever the model schedules. (a) reads the model's own ScheduleEvent ops on Turn 1; (b) needs the seeded event's FireEvent on Turn 2 with the PC's location after Turn 2 away from where it started; (c) needs a PC Belief on the slot the event wrote after Turn 4.\n",
        sc.event.text,
        sc.event.in_minutes,
        if sc.event.seen_by.is_empty() {
            "nobody".to_owned()
        } else {
            sc.event.seen_by.join(", ")
        }
    );
    let _ = writeln!(
        out,
        "- (a) Model scheduled an event on the provocation Turn: {model_scheduled} of {n}"
    );
    let _ = writeln!(
        out,
        "- (b) Seeded event fired while the PC was away: {fired_away} of {n} (PC away after Turn 2: {away_runs} of {n})"
    );
    let _ = writeln!(
        out,
        "- (c) PC later learned of it through play: {learned} of {n}"
    );
    let _ = writeln!(
        out,
        "- Offscreen consequence observed (b and c in the same run): {observed} of {n}\n"
    );
    let _ = writeln!(
        out,
        "| run | model events on Turn 1 | Turn 2 action | PC location after Turn 2 | seeded event fired | PC learned | how |\n|---|---|---|---|---|---|---|"
    );
    for row in rows {
        let _ = writeln!(out, "{row}");
    }
    Ok(out)
}

/// Today's UTC date, `YYYY-MM-DD`.
pub fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The repository root when run from the workspace: the folder that holds
/// `evals/scenarios`.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|p| p.join("evals/scenarios").is_dir())
        .map(Path::to_path_buf)
}
