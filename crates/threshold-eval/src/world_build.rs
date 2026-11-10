//! The world_build scenario (design section 8): a Seed Draft and a World
//! Build from a Source Text, scored against a hand-made list.
//!
//! The owner never reads the hidden world, so the report must say in numbers
//! whether a build is good: roster recall, fact recall, false past, open
//! recall, PC-centred share, ages, World Events, voice distance, times and
//! failures. The metrics are plain functions of a draft and a World Setup,
//! so tests check them with hand-made data and no model.
//!
//! A Source Text outside the repository is private: its report section
//! prints counts only, never a name, a fact or a Voice Card line.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
    time::Instant,
};

use serde::{Deserialize, Serialize};
use threshold_core::{
    build::{
        BuildFailure, Control, DebugEntry, Gap, PC_CENTRED_MAX_SHARE, Progress, SeedDraft, approve,
        build_world, draft_from_text, names_pc,
    },
    domain::{AdventureSeed, EntityKind, FactEntry, GoalEntry, OtherName, WorldSetup, is_adult},
    normalize,
};

use super::{Ctx, pct, scan::percentile, scan::without_names, scenario, secs};

// ---- The expected list -------------------------------------------------------

/// What a correct draft and world of the Source Text hold, written by hand.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub pc_name: String,
    #[serde(default)]
    pub pc_age: Option<u16>,
    #[serde(default)]
    pub pc_other_names: Vec<String>,
    #[serde(default)]
    pub start_place: String,
    #[serde(default)]
    pub start_present: Vec<String>,
    /// The NPCs. `kind` defaults to npc.
    #[serde(default)]
    pub characters: Vec<ExpectedEntity>,
    /// Expected as factions.
    #[serde(default)]
    pub groups: Vec<ExpectedEntity>,
    /// Expected as places.
    #[serde(default)]
    pub places: Vec<ExpectedEntity>,
    #[serde(default)]
    pub settled: Vec<Item>,
    #[serde(default)]
    pub open: Vec<OpenItem>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedEntity {
    pub name: String,
    #[serde(default)]
    pub kind: Option<EntityKind>,
    /// Only when the Source Text states it.
    #[serde(default)]
    pub age: Option<u16>,
    #[serde(default)]
    pub other_names: Vec<String>,
}

/// A settled item of the Source Text: a subject, and a short phrase whose
/// every word a matching fact holds.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub subject: String,
    pub about: String,
}

/// An open item of the Source Text: planned, proposed, pending or not
/// established. `about` finds it kept open. `done` lists phrases that only
/// a completed event would hold ("married", "sold wheel"): a settled fact
/// that holds one of them states the item as done. When no phrase can tell
/// a completed event from a correct settled fact, the item sets
/// `not_scored_for_false_past` instead, and false past skips it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenItem {
    pub subject: String,
    pub about: String,
    #[serde(default)]
    pub done: Vec<String>,
    #[serde(default)]
    pub not_scored_for_false_past: bool,
}

impl Expected {
    pub fn parse(text: &str) -> Result<Self, String> {
        let expected: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        for item in &expected.open {
            let at = format!("the open item on {} ({})", item.subject, item.about);
            match (item.done.is_empty(), item.not_scored_for_false_past) {
                (true, false) => {
                    return Err(format!(
                        "{at} needs a done phrase, or not_scored_for_false_past = true"
                    ));
                }
                (false, true) => {
                    return Err(format!(
                        "{at} has done phrases and not_scored_for_false_past: keep one"
                    ));
                }
                _ => {}
            }
            if item.done.iter().any(|p| words(p).is_empty()) {
                return Err(format!("{at} has an empty done phrase"));
            }
        }
        Ok(expected)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Every expected roster entry, the PC first.
    fn entries(&self) -> Vec<Entry> {
        let pc = Entry::new(
            &self.pc_name,
            &self.pc_other_names,
            EntityKind::Pc,
            self.pc_age,
        );
        let of = |list: &[ExpectedEntity], kind: EntityKind| -> Vec<Entry> {
            list.iter()
                .map(|e| Entry::new(&e.name, &e.other_names, e.kind.unwrap_or(kind), e.age))
                .collect()
        };
        std::iter::once(pc)
            .chain(of(&self.characters, EntityKind::Npc))
            .chain(of(&self.groups, EntityKind::Faction))
            .chain(of(&self.places, EntityKind::Place))
            .collect()
    }
}

/// One roster entry, expected or drafted.
struct Entry {
    main: String,
    /// Main and other names, through `normalize::name`.
    names: Vec<String>,
    kind: EntityKind,
    age: Option<u16>,
}

impl Entry {
    fn new(main: &str, others: &[String], kind: EntityKind, age: Option<u16>) -> Self {
        Self {
            main: main.to_owned(),
            names: std::iter::once(main)
                .chain(others.iter().map(String::as_str))
                .map(normalize::name)
                .filter(|n| !n.is_empty())
                .collect(),
            kind,
            age,
        }
    }

    fn same(&self, other: &Entry) -> bool {
        self.names.iter().any(|n| other.names.contains(n))
    }
}

/// The drafted roster, the PC first.
fn drafted(seed: &AdventureSeed) -> Vec<Entry> {
    let others = |names: &[OtherName]| names.iter().map(|o| o.name.clone()).collect::<Vec<_>>();
    std::iter::once(Entry::new(
        &seed.pc.name,
        &others(&seed.pc.other_names),
        EntityKind::Pc,
        Some(seed.pc.age),
    ))
    .chain(
        seed.entities
            .iter()
            .map(|e| Entry::new(&e.name, &others(&e.other_names), e.kind, e.age)),
    )
    .collect()
}

// ---- One run -------------------------------------------------------------------

/// Model calls of a draft or a build, read from its `Control.log`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Calls {
    pub calls: usize,
    /// Calls sent again after a timeout.
    pub retries: usize,
    /// Repair calls, not counting their timeout retries.
    pub repairs: usize,
}

impl Calls {
    pub fn of(log: &[DebugEntry]) -> Self {
        Self {
            calls: log.len(),
            retries: log.iter().filter(|e| e.step.contains(" retry ")).count(),
            repairs: log.iter().filter(|e| e.step.ends_with(" repair")).count(),
        }
    }
}

/// One judged Voice Card line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceLine {
    pub npc: String,
    /// The first sample line, names removed.
    pub line: String,
    /// How many Voice Cards the judge chose from.
    pub choices: usize,
    /// `None`: the judge failed.
    pub hit: Option<bool>,
}

/// What one run produced. A run stops at the first step that does not
/// succeed: a failed draft, gaps left before approval, or a failed build.
#[derive(Clone, Debug, Default)]
pub struct BuildRun {
    /// The draft, when `draft_from_text` returned one.
    pub draft: Option<SeedDraft>,
    /// Set for a draft that succeeded.
    pub draft_ms: Option<u64>,
    /// The gaps left once every proposed age is confirmed; empty when the
    /// draft was approved.
    pub gaps: Vec<Gap>,
    pub world: Option<WorldSetup>,
    /// Set for a build that succeeded.
    pub build_ms: Option<u64>,
    /// The facts of `world` that an NpcState call wrote as a secret.
    pub secrets: Vec<FactEntry>,
    /// The draft or build failure.
    pub failure: Option<BuildFailure>,
    pub draft_calls: Calls,
    pub build_calls: Calls,
    pub voices: Vec<VoiceLine>,
}

/// The secrets of `world`: the World Setup does not mark them, so they are
/// the facts whose text an `npc_state` call of `log` wrote as its secret.
pub fn secrets_of(world: &WorldSetup, log: &[DebugEntry]) -> Vec<FactEntry> {
    let texts: BTreeSet<String> = log
        .iter()
        .filter(|e| e.step.starts_with("npc_state"))
        .filter_map(|e| serde_json::from_str::<serde_json::Value>(&e.raw).ok())
        .filter_map(|v| v.pointer("/secret/text")?.as_str().map(str::to_owned))
        .collect();
    world
        .facts
        .iter()
        .filter(|f| texts.contains(&f.text))
        .cloned()
        .collect()
}

// ---- Metrics -------------------------------------------------------------------

/// Expected roster entries found in a draft roster.
#[derive(Debug, Default)]
pub struct RosterRecall {
    pub found: usize,
    pub expected: usize,
    /// Expected main names that no drafted entry carries.
    pub missing: Vec<String>,
    /// `<expected main name>: expected <kind>, drafted <kind>`.
    pub wrong_kinds: Vec<String>,
    /// Drafted main names that no expected entry carries.
    pub extra: Vec<String>,
}

/// An expected entry is found when one of its names, main or other, is a
/// name of a drafted entry after `normalize::name`.
pub fn roster_recall(expected: &Expected, seed: &AdventureSeed) -> RosterRecall {
    let wanted = expected.entries();
    let got = drafted(seed);
    let mut recall = RosterRecall {
        expected: wanted.len(),
        ..RosterRecall::default()
    };
    for entry in &wanted {
        match got.iter().find(|d| d.same(entry)) {
            None => recall.missing.push(entry.main.clone()),
            Some(d) => {
                recall.found += 1;
                if d.kind != entry.kind {
                    recall.wrong_kinds.push(format!(
                        "{}: expected {}, drafted {}",
                        entry.main,
                        label(entry.kind),
                        label(d.kind)
                    ));
                }
            }
        }
    }
    recall.extra = got
        .iter()
        .filter(|d| !wanted.iter().any(|e| e.same(d)))
        .map(|d| d.main.clone())
        .collect();
    recall
}

/// The known facts of a draft, as facts the PC holds.
pub fn known_facts(draft: &SeedDraft) -> Vec<FactEntry> {
    draft
        .knowledge
        .iter()
        .map(|k| FactEntry {
            subject: k.subject.clone(),
            key: k.key.clone(),
            value: k.value.clone(),
            text: k.text.clone(),
            known_by: vec![draft.seed.pc.name.clone()],
            open: k.open,
        })
        .collect()
}

/// Per settled item: found in a fact, open or not, through `about`.
pub fn fact_recall(expected: &Expected, seed: &AdventureSeed, facts: &[FactEntry]) -> Vec<bool> {
    let items = expected
        .settled
        .iter()
        .map(|i| (&i.subject, std::slice::from_ref(&i.about)));
    found(expected, seed, items, facts, None)
}

/// Per open item: a settled fact (`open = false`) holds one of its `done`
/// phrases, so it states the item as done. `None` for an item that is not
/// scored for false past.
pub fn false_past(
    expected: &Expected,
    seed: &AdventureSeed,
    facts: &[FactEntry],
) -> Vec<Option<bool>> {
    let items = expected
        .open
        .iter()
        .map(|i| (&i.subject, i.done.as_slice()));
    found(expected, seed, items, facts, Some(false))
        .into_iter()
        .zip(&expected.open)
        .map(|(hit, item)| (!item.not_scored_for_false_past).then_some(hit))
        .collect()
}

/// Per open item: found in an open fact, through `about`.
pub fn open_recall(expected: &Expected, seed: &AdventureSeed, facts: &[FactEntry]) -> Vec<bool> {
    let items = expected
        .open
        .iter()
        .map(|i| (&i.subject, std::slice::from_ref(&i.about)));
    found(expected, seed, items, facts, Some(true))
}

/// Per item, given as a subject and its phrases: some fact of `facts`, with
/// the given openness, has the subject (through the names of the expected
/// list and of the draft roster) and holds every word of one of the
/// phrases in its key, value and text.
fn found<'a>(
    expected: &Expected,
    seed: &AdventureSeed,
    items: impl Iterator<Item = (&'a String, &'a [String])>,
    facts: &[FactEntry],
    open: Option<bool>,
) -> Vec<bool> {
    let wanted = expected.entries();
    let got = drafted(seed);
    items
        .map(|(subject, phrases)| {
            let names = subject_names(&wanted, &got, subject);
            let phrases: Vec<Vec<String>> = phrases.iter().map(|p| words(p)).collect();
            facts.iter().any(|f| {
                open.is_none_or(|o| f.open == o)
                    && names.contains(&normalize::name(&f.subject))
                    && {
                        let said = words(&format!("{} {} {}", f.key, f.value, f.text));
                        phrases
                            .iter()
                            .any(|p| !p.is_empty() && p.iter().all(|w| said.contains(w)))
                    }
            })
        })
        .collect()
}

/// Every normalized name of the entity that `subject` names: its own, those
/// of the expected entry that carries it, and those of each drafted entry
/// that shares one of them.
fn subject_names(wanted: &[Entry], got: &[Entry], subject: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::from([normalize::name(subject)]);
    for entry in wanted.iter().chain(got) {
        if entry.names.iter().any(|n| names.contains(n)) {
            names.extend(entry.names.iter().cloned());
        }
    }
    names
}

/// The words of `text` after `normalize::name`.
fn words(text: &str) -> Vec<String> {
    normalize::name(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// How many of `goals` and `secrets` name the PC, and how many there are.
/// The build's own rule, `build::names_pc`: a text names the PC when it
/// holds, as whole words, a full name of the PC, or the first word of its
/// main name when no name of `roster` holds that word. `pcs` gives the PC's
/// names as (main name, other names); a text that names one of them names
/// the PC. `roster` holds the main and other names of every other drafted
/// entry, as the WorldAutonomy check passes them (`roster_names`).
pub fn pc_centred(
    pcs: &[(String, Vec<String>)],
    roster: &[&str],
    goals: &[GoalEntry],
    secrets: &[FactEntry],
) -> (usize, usize) {
    let named = |text: &str| {
        pcs.iter()
            .any(|(main, others)| names_pc(main, others, roster, text))
    };
    goals
        .iter()
        .map(|g| g.text.as_str())
        .chain(secrets.iter().map(|s| s.text.as_str()))
        .fold((0, 0), |(naming, total), text| {
            (naming + usize::from(named(text)), total + 1)
        })
}

/// The main and other names of every drafted entry but the PC.
pub fn roster_names(seed: &AdventureSeed) -> Vec<&str> {
    seed.entities
        .iter()
        .flat_map(|e| std::iter::once(&e.name).chain(e.other_names.iter().map(|o| &o.name)))
        .map(String::as_str)
        .collect()
}

/// The PC's names, as (main name, other names): the expected ones and the
/// drafted ones.
fn pc_names(expected: &Expected, seed: &AdventureSeed) -> Vec<(String, Vec<String>)> {
    vec![
        (expected.pc_name.clone(), expected.pc_other_names.clone()),
        (
            seed.pc.name.clone(),
            seed.pc.other_names.iter().map(|o| o.name.clone()).collect(),
        ),
    ]
}

/// The snake_case name serde gives a value (`hidden_facts`, `faction`).
fn label<T: Serialize + std::fmt::Debug>(value: T) -> String {
    serde_json::to_value(&value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{value:?}"))
}

fn failure_label(failure: &BuildFailure) -> String {
    format!("{} / {}", label(failure.stage), label(failure.check))
}

// ---- The report section --------------------------------------------------------

/// What the section may show of its source.
pub enum Shown {
    /// A file of the repository: names, missed items and three Voice Card
    /// lines may be printed.
    Fixture { source: String, expected: String },
    /// A file outside the repository: counts only.
    Private,
}

/// Sums over the drafts of a scenario.
#[derive(Default)]
struct Tally {
    drafts: usize,
    roster: (usize, usize),
    wrong_kinds: (usize, BTreeSet<String>),
    extra: (usize, BTreeSet<String>),
    missing: (usize, BTreeSet<String>),
    ages: (usize, usize, BTreeSet<String>),
    child: (usize, usize, BTreeSet<String>),
    /// Drafted characters under 18 that the expected list does not hold
    /// as children, by drafted main name.
    minors: (usize, BTreeSet<String>),
    start_place: usize,
    present: (usize, usize),
    /// Facts scored on the known facts of each draft.
    draft: Recall,
    /// Facts scored on the facts of each built world.
    world: Recall,
}

/// Fact scores over several fact lists: one per draft, or one per built
/// world.
#[derive(Default)]
struct Recall {
    lists: usize,
    /// Found, of, the expected items missed.
    settled: (usize, usize, BTreeSet<String>),
    /// Stated as done, of, the expected items stated as done.
    false_past: (usize, usize, BTreeSet<String>),
    open: (usize, usize),
}

impl Recall {
    /// Scores one fact list. Returns its table cells: settled found, false
    /// past, open kept.
    fn add(
        &mut self,
        expected: &Expected,
        seed: &AdventureSeed,
        facts: &[FactEntry],
    ) -> [String; 3] {
        self.lists += 1;
        let settled = fact_recall(expected, seed, facts);
        let past = false_past(expected, seed, facts);
        let open = open_recall(expected, seed, facts);
        let hits = |v: &[bool]| v.iter().filter(|b| **b).count();
        self.settled.0 += hits(&settled);
        self.settled.1 += settled.len();
        for (item, _) in expected.settled.iter().zip(&settled).filter(|(_, f)| !**f) {
            self.settled
                .2
                .insert(format!("{}: {}", item.subject, item.about));
        }
        // Only the scored open items count, in both terms.
        let scored: Vec<bool> = past.iter().flatten().copied().collect();
        self.false_past.0 += hits(&scored);
        self.false_past.1 += scored.len();
        for (item, _) in expected
            .open
            .iter()
            .zip(&past)
            .filter(|(_, f)| **f == Some(true))
        {
            self.false_past
                .2
                .insert(format!("{}: {}", item.subject, item.about));
        }
        self.open.0 += hits(&open);
        self.open.1 += open.len();
        let cell = |v: &[bool]| format!("{} of {}", hits(v), v.len());
        [cell(&settled), cell(&scored), cell(&open)]
    }
}

/// One draft's row of the per-run table, and its sums.
fn tally_draft(expected: &Expected, run: &BuildRun, draft: &SeedDraft, t: &mut Tally) -> String {
    let seed = &draft.seed;
    t.drafts += 1;
    let recall = roster_recall(expected, seed);
    t.roster.0 += recall.found;
    t.roster.1 += recall.expected;
    t.wrong_kinds.0 += recall.wrong_kinds.len();
    t.wrong_kinds.1.extend(recall.wrong_kinds);
    t.extra.0 += recall.extra.len();
    t.extra.1.extend(recall.extra);
    t.missing.0 += recall.missing.len();
    t.missing.1.extend(recall.missing);

    let wanted = expected.entries();
    let got = drafted(seed);
    for entry in wanted.iter().filter(|e| e.age.is_some()) {
        let stated = entry.age.unwrap_or_default();
        let Some(d) = got.iter().find(|d| d.same(entry)) else {
            continue;
        };
        t.ages.1 += 1;
        if d.age != Some(stated) {
            t.ages.0 += 1;
            t.ages.2.insert(format!(
                "{}: stated {stated}, drafted {}",
                entry.main,
                d.age.map_or_else(|| "none".to_owned(), |a| a.to_string())
            ));
        }
    }
    for child in wanted
        .iter()
        .filter(|e| e.age.is_some() && !is_adult(e.age))
    {
        t.child.1 += 1;
        t.child.2.insert(child.main.clone());
        let drafted_age = got.iter().find(|d| d.same(child)).and_then(|d| d.age);
        t.child.0 += usize::from(drafted_age.is_some() && !is_adult(drafted_age));
    }
    for d in got.iter().filter(|d| d.age.is_some() && !is_adult(d.age)) {
        let listed = wanted
            .iter()
            .any(|e| e.age.is_some() && !is_adult(e.age) && e.same(d));
        if !listed {
            t.minors.0 += 1;
            t.minors.1.insert(d.main.clone());
        }
    }
    let place = subject_names(&wanted, &got, &expected.start_place);
    t.start_place += usize::from(place.contains(&normalize::name(&seed.start.place)));
    for name in &expected.start_present {
        let names = subject_names(&wanted, &got, name);
        t.present.1 += 1;
        t.present.0 += usize::from(
            seed.start
                .present
                .iter()
                .any(|p| names.contains(&normalize::name(p))),
        );
    }

    let draft_cells = t.draft.add(expected, seed, &known_facts(draft));
    let world_cells = match &run.world {
        Some(world) => t.world.add(expected, seed, &world.facts),
        None => ["-".to_owned(), "-".to_owned(), "-".to_owned()],
    };
    let cells: Vec<String> = draft_cells
        .iter()
        .zip(&world_cells)
        .map(|(d, w)| format!("{d} / {w}"))
        .collect();
    format!(
        "{} of {} | {}",
        recall.found,
        recall.expected,
        cells.join(" | ")
    )
}

/// The world_build report section.
pub fn section(
    expected: &Expected,
    runs: &[BuildRun],
    shown: &Shown,
    source_chars: usize,
) -> String {
    let private = matches!(shown, Shown::Private);
    // A count, with the names when the source is not private.
    let named = |n: usize, names: &BTreeSet<String>| {
        if private || names.is_empty() {
            n.to_string()
        } else {
            format!(
                "{n} ({})",
                names.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        }
    };

    let mut t = Tally::default();
    let mut rows = Vec::new();
    let (mut built, mut failed, mut not_approvable) = (0, 0, 0);
    let mut gaps: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures: BTreeMap<String, usize> = BTreeMap::new();
    let (mut naming, mut items, mut highest): (usize, usize, Option<f64>) = (0, 0, None);
    let (mut events, mut with_effect) = (0, 0);
    let (mut draft_ms, mut build_ms) = (Vec::new(), Vec::new());
    let (mut draft_calls, mut build_calls) = (Calls::default(), Calls::default());
    let mut voices: Vec<&VoiceLine> = Vec::new();

    for (i, run) in runs.iter().enumerate() {
        draft_ms.extend(run.draft_ms);
        build_ms.extend(run.build_ms);
        for (sum, calls) in [
            (&mut draft_calls, run.draft_calls),
            (&mut build_calls, run.build_calls),
        ] {
            sum.calls += calls.calls;
            sum.retries += calls.retries;
            sum.repairs += calls.repairs;
        }
        if let Some(f) = &run.failure {
            failed += 1;
            *failures.entry(failure_label(f)).or_default() += 1;
        }
        if !run.gaps.is_empty() {
            not_approvable += 1;
        }
        for gap in &run.gaps {
            *gaps.entry(label(gap.kind)).or_default() += 1;
        }
        voices.extend(&run.voices);

        let Some(draft) = &run.draft else {
            rows.push(format!(
                "| {} | failed: {} | - | - | - | - | - | - | - | - | {} | - | - | - |",
                i + 1,
                run.failure.as_ref().map_or("?".into(), failure_label),
                calls_cell(run.draft_calls)
            ));
            continue;
        };
        let scores = tally_draft(expected, run, draft, &mut t);
        let mut centred = "-".to_owned();
        let mut world_events = "-".to_owned();
        if let Some(world) = &run.world {
            built += 1;
            let (n, of) = pc_centred(
                &pc_names(expected, &draft.seed),
                &roster_names(&draft.seed),
                &world.goals,
                &run.secrets,
            );
            naming += n;
            items += of;
            if of > 0 {
                let share = n as f64 / of as f64;
                highest = Some(highest.map_or(share, |h: f64| h.max(share)));
            }
            centred = format!("{n} of {of}");
            let effect = world
                .events
                .iter()
                .filter(|e| !e.effects.is_empty())
                .count();
            events += world.events.len();
            with_effect += effect;
            world_events = format!("{} ({effect})", world.events.len());
        }
        let build = match (&run.world, &run.failure, run.gaps.is_empty()) {
            (Some(_), _, _) => "ok".to_owned(),
            (None, _, false) => "not approvable".to_owned(),
            (None, Some(f), true) => format!("failed: {}", failure_label(f)),
            (None, None, true) => "-".to_owned(),
        };
        rows.push(format!(
            "| {} | ok | {} | {build} | {scores} | {centred} | {world_events} | {} | {} | {} | {} |",
            i + 1,
            run.gaps.len(),
            calls_cell(run.draft_calls),
            calls_cell(run.build_calls),
            secs(run.draft_ms),
            secs(run.build_ms)
        ));
    }

    let mut out = String::new();
    match shown {
        Shown::Fixture { source, expected } => {
            let _ = write!(
                out,
                "Source Text: `{source}` ({source_chars} characters), scored against `{expected}`."
            );
        }
        Shown::Private => {
            let _ = write!(
                out,
                "Source Text: a private file outside the repository ({source_chars} characters). Counts only: this section prints no name, no fact and no Voice Card line."
            );
        }
    }
    let _ = writeln!(
        out,
        " Each run drafts a Seed Draft, confirms every proposed age, approves the draft when no gap is left, and builds the world; a run stops at the first step that does not succeed. Roster: an expected entry is found when one of its names, main or other, is a drafted name after `normalize::name`. Facts: an expected item is found when a fact with the same subject (through the main and other names) holds every word of one of the item's phrases in its key, value and text. The draft lines score the known facts of each draft; the world lines score the facts of each built world (known facts included) and cover only the runs that built one. Fact recall and open recall use the item's `about` phrase. False past counts the open items for which a settled fact (`open = false`) holds one of the item's `done` phrases: words that only a completed event would hold, so a settled fact about the plan itself does not count. An open item for which no such phrase exists is not scored for false past, and the false past lines leave it out of both terms. A paraphrase does not match, so every fact line is a lower bound. The run confirms every proposed age, so a proposed minor shows only on the line of characters drafted under 18. PC-centred share: the NPC goals and secrets whose text names the PC by one of its full names, or by the first word of its main name when no other drafted entry has that word in a name, as whole words (the build's own rule, `build::names_pc`, with the drafted roster); a secret is a fact that an NpcState call wrote as one. Voice distance: for each NPC the judge sees the first sample line of its Voice Card, names removed, and the habits and vocabulary of up to {VOICE_CHOICES} Voice Cards, and picks the speaker. Times cover the drafts and builds that succeeded.\n"
    );

    let n = runs.len();
    let _ = writeln!(
        out,
        "- Runs: {n}; drafted {}, approvable {}, built {built}, failed {failed}",
        t.drafts,
        t.drafts - not_approvable
    );
    let _ = writeln!(
        out,
        "- Roster recall: {} of {} expected entries over {} drafts ({})",
        t.roster.0,
        t.roster.1,
        t.drafts,
        pct(t.roster.0, t.roster.1)
    );
    let _ = writeln!(
        out,
        "- Wrong kinds: {}",
        named(t.wrong_kinds.0, &t.wrong_kinds.1)
    );
    let _ = writeln!(out, "- Extra entities: {}", named(t.extra.0, &t.extra.1));
    let _ = writeln!(
        out,
        "- Missing entries: {}",
        named(t.missing.0, &t.missing.1)
    );
    let _ = writeln!(
        out,
        "- Ages that differ from a stated age: {} of {}",
        t.ages.0,
        named(t.ages.1, &t.ages.2)
    );
    if t.child.1 == 0 && t.drafts > 0 {
        let _ = writeln!(
            out,
            "- Child (stated age under 18) drafted under 18: no child in the expected list"
        );
    } else {
        let _ = writeln!(
            out,
            "- Child (stated age under 18) drafted under 18: {} of {}",
            t.child.0,
            named(t.child.1, &t.child.2)
        );
    }
    let _ = writeln!(
        out,
        "- Characters drafted under 18 that the expected list does not hold as children: {}",
        named(t.minors.0, &t.minors.1)
    );
    let _ = writeln!(
        out,
        "- Start place right: {} of {} drafts; present characters found: {} of {}",
        t.start_place, t.drafts, t.present.0, t.present.1
    );
    let _ = writeln!(
        out,
        "- Draft gaps before approval, by kind: {}",
        counts(&gaps)
    );
    let _ = writeln!(out, "- Not approvable: {not_approvable} of {n} runs");
    let _ = writeln!(out, "- Runs with a built world: {} of {n}", t.world.lists);
    for (r, over, lists) in fact_lines(&t) {
        let _ = writeln!(
            out,
            "- Fact recall, {over} (settled items found): {} of {} over {lists} ({}){}",
            r.settled.0,
            r.settled.1,
            pct(r.settled.0, r.settled.1),
            missed("missed", private, &r.settled.2)
        );
    }
    for (r, over, lists) in fact_lines(&t) {
        let _ = writeln!(
            out,
            "- False past, {over} (open items stated as done): {} of {} over {lists}{}",
            r.false_past.0,
            r.false_past.1,
            missed("stated as done", private, &r.false_past.2)
        );
    }
    let unscored: BTreeSet<String> = expected
        .open
        .iter()
        .filter(|i| i.not_scored_for_false_past)
        .map(|i| format!("{}: {}", i.subject, i.about))
        .collect();
    let _ = writeln!(
        out,
        "- Open items not scored for false past: {}",
        named(unscored.len(), &unscored)
    );
    for (r, over, lists) in fact_lines(&t) {
        let _ = writeln!(
            out,
            "- Open recall, {over} (open items kept open): {} of {} over {lists}",
            r.open.0, r.open.1
        );
    }
    let _ = writeln!(
        out,
        "- PC-centred share (NPC goals and secrets that name the PC): {naming} of {items} ({}) over {built} built worlds; highest world {}; the build's limit is {:.0}%",
        pct(naming, items),
        highest.map_or_else(|| "n/a".to_owned(), |h| format!("{:.0}%", h * 100.0)),
        PC_CENTRED_MAX_SHARE * 100.0
    );
    let _ = writeln!(
        out,
        "- World Events: {events} over {built} built worlds; with at least one effect: {with_effect}"
    );
    let judged: Vec<&&VoiceLine> = voices.iter().filter(|v| v.hit.is_some()).collect();
    let matched = judged.iter().filter(|v| v.hit == Some(true)).count();
    let chance = if judged.is_empty() {
        "n/a".to_owned()
    } else {
        let mean = judged
            .iter()
            .map(|v| 1.0 / v.choices.max(1) as f64)
            .sum::<f64>()
            / judged.len() as f64;
        format!("{:.0}%", mean * 100.0)
    };
    let _ = writeln!(
        out,
        "- Voice distance: matched {matched} / {} judged sample lines ({}), chance {chance}; judge errors {}",
        judged.len(),
        pct(matched, judged.len()),
        voices.len() - judged.len()
    );
    let max = |v: &[u64]| v.iter().max().copied();
    let _ = writeln!(
        out,
        "- Draft time p50 / max: {} / {}",
        secs(percentile(&draft_ms, 50)),
        secs(max(&draft_ms))
    );
    let _ = writeln!(
        out,
        "- Build time p50 / max: {} / {}",
        secs(percentile(&build_ms, 50)),
        secs(max(&build_ms))
    );
    let _ = writeln!(
        out,
        "- Model calls: draft {}, build {}; timeout retries {}; repair calls {}",
        draft_calls.calls,
        build_calls.calls,
        draft_calls.retries + build_calls.retries,
        draft_calls.repairs + build_calls.repairs
    );
    let _ = writeln!(
        out,
        "- Failures (BuildFailure, by stage and check): {}",
        if failures.is_empty() {
            "none".to_owned()
        } else {
            failures
                .iter()
                .map(|(k, n)| format!("{n} x {k}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    if !private && !voices.is_empty() {
        let _ = writeln!(out, "\nSample Voice Card lines:\n");
        for v in voices.iter().take(3) {
            let _ = writeln!(out, "- {}: \"{}\"", v.npc, v.line);
        }
    }
    let _ = writeln!(
        out,
        "\n| run | draft | gaps | build | roster | settled found (draft / world) | false past (draft / world) | open kept (draft / world) | PC-centred | events (with effect) | draft calls (repairs, retries) | build calls (repairs, retries) | draft time | build time |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"
    );
    for row in rows {
        let _ = writeln!(out, "{row}");
    }
    out
}

/// The draft and world fact scores, with their names and denominators.
fn fact_lines(t: &Tally) -> [(&Recall, &'static str, String); 2] {
    [
        (
            &t.draft,
            "draft knowledge",
            format!("{} drafts", t.draft.lists),
        ),
        (
            &t.world,
            "built world",
            format!("{} built worlds", t.world.lists),
        ),
    ]
}

/// `calls (repairs, retries)`, or `-` when no call ran.
fn calls_cell(c: Calls) -> String {
    if c.calls == 0 {
        "-".to_owned()
    } else {
        format!("{} ({}, {})", c.calls, c.repairs, c.retries)
    }
}

fn counts(map: &BTreeMap<String, usize>) -> String {
    if map.is_empty() {
        "none".to_owned()
    } else {
        map.iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// `; <what>: a, b` with the expected items, unless the source is private.
fn missed(what: &str, private: bool, items: &BTreeSet<String>) -> String {
    if private || items.is_empty() {
        String::new()
    } else {
        format!(
            "; {what}: {}",
            items.iter().cloned().collect::<Vec<_>>().join(", ")
        )
    }
}

// ---- The runner ----------------------------------------------------------------

/// The most Voice Cards the judge chooses from.
const VOICE_CHOICES: usize = 4;
const LABELS: [&str; VOICE_CHOICES] = ["A", "B", "C", "D"];

pub(crate) fn scenario(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::WorldBuild = scenario::load(&ctx.config.root, "world_build")?;
    let root = &ctx.config.root;
    let source = ctx
        .config
        .source
        .clone()
        .unwrap_or_else(|| root.join(&sc.source));
    let list = ctx
        .config
        .expected
        .clone()
        .unwrap_or_else(|| root.join(&sc.expected));
    let private = outside(root, &source) || outside(root, &list);
    let text = fs::read_to_string(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    let expected = Expected::load(&list)?;

    let mut runs = Vec::new();
    for run in 1..=ctx.config.runs {
        runs.push(one_run(ctx, &text, run));
    }
    let shown = if private {
        Shown::Private
    } else {
        let rel = |p: &Path| {
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        };
        Shown::Fixture {
            source: rel(&source),
            expected: rel(&list),
        }
    };
    Ok(section(&expected, &runs, &shown, text.chars().count()))
}

/// True when `path` is not inside the repository `root`.
fn outside(root: &Path, path: &Path) -> bool {
    match (fs::canonicalize(root), fs::canonicalize(path)) {
        (Ok(root), Ok(path)) => !path.starts_with(root),
        _ => !path.starts_with(root),
    }
}

fn ms_since(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// One draft, approval and build. The log lines carry stage names, counts,
/// times and check names only, never text of the Source Text or the world.
fn one_run(ctx: &mut Ctx, text: &str, run: usize) -> BuildRun {
    let tag = format!("world_build-r{run}");
    let gm = (ctx.build)();
    let mut out = BuildRun::default();

    let mut entries = Vec::new();
    let started = Instant::now();
    let drafted = {
        let log = &mut *ctx.log;
        let mut progress =
            |p: Progress| log(&format!("{tag} {} {}/{}", label(p.stage), p.done, p.total));
        let mut record = |e: DebugEntry| entries.push(e);
        let mut ctl = Control {
            progress: &mut progress,
            cancelled: &|| false,
            log: &mut record,
        };
        draft_from_text(text, gm.as_ref(), &mut ctl)
    };
    out.draft_calls = Calls::of(&entries);
    let mut draft = match drafted {
        Ok(draft) => draft,
        Err(failure) => {
            (ctx.log)(&format!("{tag} draft FAILED {}", failure_label(&failure)));
            out.failure = Some(failure);
            return out;
        }
    };
    out.draft_ms = Some(ms_since(started));
    (ctx.log)(&format!("{tag} draft ok {}", secs(out.draft_ms)));

    draft.ages_to_confirm.clear();
    if let Err(gaps) = approve(&mut draft) {
        let kinds: Vec<String> = gaps.iter().map(|g| label(g.kind)).collect();
        (ctx.log)(&format!("{tag} not approvable: {}", kinds.join(", ")));
        out.gaps = gaps;
        out.draft = Some(draft);
        return out;
    }

    let mut entries = Vec::new();
    let started = Instant::now();
    let built = {
        let log = &mut *ctx.log;
        let mut progress =
            |p: Progress| log(&format!("{tag} {} {}/{}", label(p.stage), p.done, p.total));
        let mut record = |e: DebugEntry| entries.push(e);
        let mut ctl = Control {
            progress: &mut progress,
            cancelled: &|| false,
            log: &mut record,
        };
        build_world(&draft, gm.as_ref(), &mut ctl)
    };
    out.build_calls = Calls::of(&entries);
    match built {
        Ok(world) => {
            out.build_ms = Some(ms_since(started));
            (ctx.log)(&format!("{tag} build ok {}", secs(out.build_ms)));
            out.secrets = secrets_of(&world, &entries);
            out.voices = judge_voices(ctx, &world, &draft.seed, run);
            out.world = Some(world);
        }
        Err(failure) => {
            (ctx.log)(&format!("{tag} build FAILED {}", failure_label(&failure)));
            out.failure = Some(failure);
        }
    }
    out.draft = Some(draft);
    out
}

/// The blind voice judge on the built Voice Cards: for each card with a
/// sample line, the judge sees that line, names removed, and the habits and
/// vocabulary of up to `VOICE_CHOICES` cards in a rotating order.
fn judge_voices(
    ctx: &mut Ctx,
    world: &WorldSetup,
    seed: &AdventureSeed,
    run: usize,
) -> Vec<VoiceLine> {
    let cards: Vec<_> = world
        .voice_cards
        .iter()
        .filter(|c| c.samples.iter().any(|s| !s.trim().is_empty()))
        .collect();
    if cards.len() < 2 {
        return vec![];
    }
    let names: Vec<String> =
        std::iter::once(&seed.pc.name)
            .chain(seed.pc.other_names.iter().map(|o| &o.name))
            .chain(seed.entities.iter().flat_map(|e| {
                std::iter::once(&e.name).chain(e.other_names.iter().map(|o| &o.name))
            }))
            .cloned()
            .collect();
    let k = cards.len().min(VOICE_CHOICES);
    let mut out = Vec::new();
    for (i, card) in cards.iter().enumerate() {
        let sample = card
            .samples
            .iter()
            .find(|s| !s.trim().is_empty())
            .map_or("", String::as_str);
        let line = without_names(sample, &names);
        // The speaker and the next k - 1 cards, in a rotating order.
        let pool: Vec<usize> = (0..k).map(|j| (i + j) % cards.len()).collect();
        let order: Vec<usize> = (0..k).map(|j| pool[(j + i + run) % k]).collect();
        let mut task = String::from(
            "Below are the voice descriptions of several characters, then one line of dialogue with names removed. Which character most likely spoke the line?\n\n",
        );
        for (label, c) in LABELS.iter().zip(&order) {
            let _ = writeln!(
                task,
                "{label}: habits: {}; vocabulary: {}",
                cards[*c].habits, cards[*c].vocabulary
            );
        }
        let _ = write!(task, "\nLine: \"{line}\"");
        let hit = ctx.ask(&task, &LABELS[..k]).map(|answer| {
            LABELS[..k]
                .iter()
                .position(|l| *l == answer)
                .is_some_and(|p| order[p] == i)
        });
        out.push(VoiceLine {
            npc: card.npc.clone(),
            line,
            choices: k,
            hit,
        });
    }
    out
}
