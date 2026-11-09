//! The evaluation runner (design §10, plan Task 11).
//!
//! Each scenario plays scripted Player Intents through the real `Engine`
//! against a `Gm` (the live model, or recorded answers in tests) and turns
//! what it observes into numbers: Turn times, the Commit-check rejections in
//! the `debug_log`, the ledger state, the Narration text, and a `Judge` for
//! what code cannot read. The model is not deterministic, so the report
//! gives counts, never a pass/fail, except the latency pass line.

mod judge;
pub mod scan;
mod scenario;

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub use judge::{GmJudge, JUDGE_RULES, Judge, JudgeError};
use threshold_core::{
    Gm,
    domain::{Effect, EntityKind, EntityRef, LedgerOp, Slot, Trigger},
    fixture::{load_adventure_seed, load_world},
    normalize,
    rules::D10,
    store::{CampaignId, Store, TurnKind, TurnRecord, WorldView},
    turn::{Engine, TurnOutcome, create_campaign},
    world::pc_knowledge,
};

use scan::{first_quote, forbidden_tokens, leaks, mentions_any, percentile, without_names};

/// Every scenario, in report order.
pub const SCENARIOS: [&str; 6] = [
    "latency",
    "memory",
    "voices",
    "shortcuts",
    "agency_leaks",
    "offscreen",
];

/// The latency pass line (design §10): p90 at or under 15 s, warm.
pub const P90_PASS_MS: u64 = 15_000;

/// The Commit checks, as `CheckKind` prints them at the start of a rejection
/// line in the debug log.
const CHECK_NAMES: [&str; 8] = [
    "schema",
    "references",
    "agency",
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
}

pub type GmFactory<'a> = &'a dyn Fn() -> Box<dyn Gm + Send>;

/// Runs the scenarios and returns the Markdown report.
pub fn run(
    config: &Config,
    gm: GmFactory<'_>,
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
        "- Turn time over every scenario: p50 {}, p90 {} ({} committed Turns)",
        secs(percentile(&t.total_ms, 50)),
        secs(percentile(&t.total_ms, 90)),
        t.total_ms.len()
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
        let _ = writeln!(out, "\n## Turn failures\n");
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
            .map(|lines| {
                let mut kinds: Vec<&'static str> = lines
                    .iter()
                    .filter_map(|l| CHECK_NAMES.iter().copied().find(|k| l.starts_with(k)))
                    .collect();
                kinds.dedup();
                kinds
            })
            .collect()
    }
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
                let first: String = e.lines().next().unwrap_or("").chars().take(160).collect();
                *t.errors.entry(first.clone()).or_default() += 1;
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
    let name = normalize::name(name);
    view.entities
        .iter()
        .find(|e| e.norm_name == name)
        .map(|e| e.id)
        .ok_or_else(|| format!("no entity {name}"))
}

fn pc_location(view: &WorldView) -> Option<String> {
    let pc = view.entities.iter().find(|e| e.kind == EntityKind::Pc)?;
    view.live_fact(pc.id, "location")
        .map(|f| f.slot.value.clone())
}

fn latency(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::Latency = scenario::load(&ctx.config.root, "latency")?;
    let (mut total, mut propose, mut narrate) = (Vec::new(), Vec::new(), Vec::new());
    let mut failed = 0;
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
                Err(_) => failed += 1,
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
        "{} Intents per run, {} committed Turns, {failed} failed. Turn time = proposal (with the retry when it runs), checks, Commit and the full streamed Narration.\n",
        sc.intents.len(),
        total.len()
    );
    let _ = writeln!(out, "- Turn time p50: {}", secs(percentile(&total, 50)));
    let _ = writeln!(
        out,
        "- Turn time p90: {} (pass line <= 15 s warm: {verdict})",
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
        let mut was_stored = false;
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
                let view = s.engine.store().snapshot(s.campaign).unwrap_or_default();
                was_stored = view
                    .facts
                    .iter()
                    .any(|f| mentions_any(&format!("{} {}", f.slot.value, f.text), &sc.expect))
                    || view.claims.iter().any(|c| {
                        mentions_any(
                            &format!("{} {}", c.claim.slot.value, c.claim.text),
                            &sc.expect,
                        )
                    });
            }
            if t == total {
                probe = Some(played);
            }
        }
        stored += usize::from(was_stored);
        let verdict = match probe.as_ref().and_then(Played::narration) {
            None => "failed",
            Some(text) if mentions_any(text, &sc.expect) => "correct",
            Some(text) => {
                let task = format!(
                    "Earlier in the game this detail was established: {}\n\nMuch later, the player asked: {}\n\nThe game master narrated:\n{text}\n\nDoes the narration use the detail correctly (correct), state a different detail that contradicts it (wrong), or not use it at all (missing)?",
                    sc.detail, sc.probe
                );
                match ctx.ask(&task, &["correct", "wrong", "missing"]).as_deref() {
                    Some("correct") => "correct",
                    Some("wrong") => "wrong",
                    Some(_) => "missing",
                    None => "judge error",
                }
            }
        };
        *counts.entry(verdict).or_default() += 1;
        rows.push(format!(
            "| {run} | {} | {verdict} |",
            if was_stored { "yes" } else { "no" }
        ));
    }
    let n = |k: &str| counts.get(k).copied().unwrap_or(0);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Detail planted on Turn {}, probed on Turn {total} (the scenario file asks for {}). Correct when the Narration names the detail; otherwise the judge decides wrong or missing.\n",
        sc.plant_turn, sc.total_turns
    );
    let _ = writeln!(
        out,
        "- Memory: correct {} / wrong {} / missing {} / Turn failed {} / judge error {} (of {} runs)",
        n("correct"),
        n("wrong"),
        n("missing"),
        n("failed"),
        n("judge error"),
        ctx.config.runs
    );
    let _ = writeln!(
        out,
        "- Detail stored in the ledger after the plant Turn (a fact or Claim naming it): {stored} of {} runs\n",
        ctx.config.runs
    );
    let _ = writeln!(out, "| run | stored | verdict |\n|---|---|---|");
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

/// The Setup Turn that schedules a trap's World Event.
fn schedule_trap_event(
    store: &mut Store,
    campaign: CampaignId,
    event: &scenario::TrapEvent,
) -> Result<(), String> {
    let view = store.snapshot(campaign).map_err(|e| e.to_string())?;
    let actor = entity_id(&view, &event.actor)?;
    let subject = entity_id(&view, &event.subject)?;
    let record = TurnRecord {
        campaign_id: campaign,
        op_id: "__trap_event__".into(),
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
            seen_by: vec![],
        }],
        seen_by: vec![],
    }];
    store.commit(record, ops).map_err(|e| format!("{e:?}"))?;
    let after = store.snapshot(campaign).map_err(|e| e.to_string())?;
    if after.events_scheduled.is_empty() {
        return Err("the trap event was not scheduled".into());
    }
    Ok(())
}

#[derive(Default)]
struct TrapStats {
    turns: usize,
    attempts: usize,
    caught: usize,
    missed: usize,
    committed: usize,
    failed: usize,
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
                        .map_or(Ok(()), |e| schedule_trap_event(store, campaign, e))
                })?;
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
                let decision = s
                    .engine
                    .store()
                    .turn(outcome.turn_id)
                    .ok()
                    .flatten()
                    .map(|t| t.record.proposal_json)
                    .unwrap_or_default();
                let task = format!(
                    "The player tried to make the story take a shortcut. The shortcut is taken when: {}\n\nPlayer intent: {intent}\n\nThe committed decision (JSON; E, F, W and G handles are ids of entities, facts, events and goals):\n{decision}\n\nThe narration:\n{}\n\nWas the shortcut taken?",
                    trap.taken, outcome.narration
                );
                match ctx.ask(&task, &["yes", "no"]).as_deref() {
                    Some("yes") => st.missed += 1,
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
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            trap.shortcut,
            st.turns,
            st.attempts,
            st.caught + st.missed,
            st.caught,
            st.missed,
            st.failed,
            st.judge_errors,
            other
        ));
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Each trap Intent runs once per run in a fresh campaign. An attempt is one proposal call (the retry is a second attempt). Caught = attempts the named check rejected, read from the debug log. Missed = committed Turns where the judge says the Shortcut was taken anyway. Proposed = caught + missed.\n"
    );
    let _ = writeln!(
        out,
        "| Shortcut | trap Turns | attempts | proposed | caught | missed (judge) | Turns failed | judge errors | other rejections |\n|---|---|---|---|---|---|---|---|---|"
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

fn offscreen(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::Offscreen = scenario::load(&ctx.config.root, "offscreen")?;
    let mut observed_runs = 0;
    let mut rows = Vec::new();
    for run in 1..=ctx.config.runs {
        let mut s = ctx.session(
            &sc.fixture,
            format!("offscreen-r{run}"),
            6_000 + run as u64,
            |_, _| Ok(()),
        )?;
        let snapshot = |s: &Session| s.engine.store().snapshot(s.campaign).unwrap_or_default();
        let start_at = pc_location(&snapshot(&s));
        let t1 = ctx.play(&mut s, &sc.provoke);
        let view1 = snapshot(&s);
        let scheduled: Vec<_> = match &t1.outcome {
            Ok(o) => view1
                .events_scheduled
                .iter()
                .filter(|w| w.scheduled_turn == o.turn_id)
                .cloned()
                .collect(),
            Err(_) => Vec::new(),
        };
        let t2 = ctx.play(&mut s, &sc.away);
        let away_at = pc_location(&snapshot(&s));
        let t3 = ctx.play(&mut s, &sc.back);
        let fired_ops: Vec<LedgerOp<u64>> = [&t2, &t3]
            .iter()
            .filter_map(|p| p.outcome.as_ref().ok())
            .flat_map(|o| s.engine.store().turn_ops(o.turn_id).unwrap_or_default())
            .collect();
        let fired: Vec<_> = scheduled
            .iter()
            .filter(|w| fired_ops.contains(&LedgerOp::FireEvent { event_id: w.id }))
            .collect();
        let was_away = away_at.is_some() && away_at != start_at;
        let t4 = ctx.play(&mut s, &sc.ask);
        let view4 = snapshot(&s);
        let slots: Vec<(u64, String)> = fired
            .iter()
            .flat_map(|w| &w.effects)
            .filter_map(|e| match e {
                Effect::SetFact { slot, .. } => Some((slot.subject, slot.key.clone())),
                _ => None,
            })
            .collect();
        let knows = pc_knowledge(&view4)
            .iter()
            .any(|b| slots.contains(&(b.subject, b.key.clone())));
        let (observed, how) = if fired.is_empty() || !was_away {
            (false, "no event fired while the PC was away")
        } else if knows {
            (true, "PC Knowledge holds a slot the event wrote")
        } else if let Some(text) = t4.narration() {
            let events: Vec<&str> = fired.iter().map(|w| w.text.as_str()).collect();
            let task = format!(
                "While the player character was away, this happened offscreen: {}\n\nBack in town, the player asked: {}\n\nThe narration:\n{text}\n\nDoes the narration let the player character learn about this event or its result?",
                events.join("; "),
                sc.ask
            );
            match ctx.ask(&task, &["yes", "no"]).as_deref() {
                Some("yes") => (true, "the judge reads it in the Narration"),
                Some(_) => (false, "fired, but the PC did not learn of it"),
                None => (false, "fired; judge error"),
            }
        } else {
            (false, "fired; the last Turn failed")
        };
        observed_runs += usize::from(observed);
        let event_text: Vec<String> = scheduled
            .iter()
            .map(|w| w.text.chars().take(70).collect())
            .collect();
        rows.push(format!(
            "| {run} | {} | {} | {} | {} | {how} |",
            if event_text.is_empty() {
                "none".into()
            } else {
                event_text.join("; ")
            },
            if was_away { "yes" } else { "no" },
            fired.len(),
            if observed { "yes" } else { "no" }
        ));
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Four Turns per run: provoke Orsel, leave for days, ride back, ask Hessa. Observed = an event scheduled on Turn 1 fired while the PC was away and the PC then learned of it.\n"
    );
    let _ = writeln!(
        out,
        "- Offscreen consequence observed: {} ({observed_runs} of {} runs)\n",
        if observed_runs > 0 { "yes" } else { "no" },
        ctx.config.runs
    );
    let _ = writeln!(
        out,
        "| run | event scheduled on Turn 1 | PC away | fired while away | observed | how |\n|---|---|---|---|---|---|"
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
