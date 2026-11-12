//! The finds scenario (ADR 0007, 2026-11-12): Intents that read, open,
//! look through or ask for a list, and negative Intents (a plain talk, a
//! look around, an NPC's long answer, a move). It counts, from each Turn's
//! record (`turn::stored_found`, what the screens show), the lookups that
//! found something and the negatives that did; the judge reads each
//! lookup's Narration with its found items and says whether the Narration
//! tells at least one of them. A deterministic count stands next to the
//! judge: the item titles whose content words all appear in the Narration
//! (`names_title`). The pass lines are in the scenario file's header.

use std::fmt::Write as _;

use threshold_core::turn::stored_found;

use super::{Ctx, pct, scan::percentile, scenario, scenario::FindKind, secs};

#[derive(Default)]
struct Stats {
    turns: usize,
    failed: usize,
    total_ms: Vec<u64>,
    lookup_ms: Vec<u64>,
    lookups: usize,
    found: usize,
    /// Items recorded on the lookup Turns that found something.
    items: usize,
    told: usize,
    judge_errors: usize,
    /// Lookup Narrations that name at least one item title (`names_title`).
    named: usize,
    /// Item titles on the lookup Turns that found something, and those the
    /// Narration names.
    titles: usize,
    titles_named: usize,
    negatives: usize,
    false_found: usize,
}

/// Words that carry no content: a title's content words are its others.
const FUNCTION_WORDS: &[&str] = &[
    "a", "an", "and", "at", "by", "for", "from", "in", "into", "of", "on", "or", "the", "to",
    "with",
];

/// The words of `text`, lower case, split on what is not a letter or a
/// digit, with a plural `s` cut from words of more than three letters.
fn word_set(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| match w.strip_suffix('s') {
            Some(stem) if w.chars().count() > 3 && !w.ends_with("ss") => stem.to_owned(),
            _ => w.to_owned(),
        })
        .collect()
}

/// True when every content word of `title` appears in `narration`
/// (`word_set`): the Narration names the item, in any order. A title with
/// no content word is never named.
pub fn names_title(narration: &str, title: &str) -> bool {
    let told = word_set(narration);
    let content: Vec<String> = word_set(title)
        .into_iter()
        .filter(|w| !FUNCTION_WORDS.contains(&w.as_str()))
        .collect();
    !content.is_empty() && content.iter().all(|w| told.contains(w))
}

/// The judge's task: does the Narration tell at least one found item.
fn told_task(narration: &str, source: &str, items: &[(String, String)]) -> String {
    let list: Vec<String> = items
        .iter()
        .map(|(title, line)| format!("- {title}: {line}"))
        .collect();
    format!(
        "The player character looked at {source}. It shows these entries:\n{}\n\nNarration:\n{narration}\n\nDoes the Narration tell at least one of these entries: its title, or what it says? Answer yes or no.",
        list.join("\n")
    )
}

pub(crate) fn scenario(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::Finds = scenario::load(&ctx.config.root, "finds")?;
    let mut st = Stats::default();
    for run in 1..=ctx.config.runs {
        let mut s = ctx.session(
            &sc.fixture,
            format!("finds-r{run}"),
            13_000 + run as u64,
            |_, _| Ok(()),
        )?;
        for step in &sc.steps {
            let played = ctx.play(&mut s, &step.intent);
            st.turns += 1;
            match step.kind {
                FindKind::Lookup => st.lookups += 1,
                FindKind::Negative => st.negatives += 1,
            }
            let Ok(outcome) = &played.outcome else {
                st.failed += 1;
                continue;
            };
            st.total_ms.push(outcome.timings.total_ms);
            let stored = s
                .engine
                .store()
                .turn(outcome.turn_id)
                .map_err(|e| e.to_string())?
                .ok_or("the committed Turn is missing")?;
            let found = stored_found(&stored.record.proposal_json);
            (ctx.log)(&format!(
                "{} {:?}: {} items found{}",
                played.op_id,
                step.kind,
                found.as_ref().map_or(0, |f| f.items.len()),
                found.as_ref().map_or_else(String::new, |f| format!(
                    " (from {}, source {:?})",
                    f.from.word(),
                    f.source
                ))
            ));
            match (step.kind, found) {
                (FindKind::Negative, found) => {
                    if found.is_some() {
                        st.false_found += 1;
                    }
                }
                (FindKind::Lookup, None) => st.lookup_ms.push(outcome.timings.total_ms),
                (FindKind::Lookup, Some(found)) => {
                    st.lookup_ms.push(outcome.timings.total_ms);
                    st.found += 1;
                    st.items += found.items.len();
                    let items: Vec<(String, String)> = found
                        .items
                        .iter()
                        .map(|i| (i.title.clone(), i.line.clone()))
                        .collect();
                    let named = found
                        .items
                        .iter()
                        .filter(|i| names_title(&outcome.narration, &i.title))
                        .count();
                    st.titles += found.items.len();
                    st.titles_named += named;
                    if named > 0 {
                        st.named += 1;
                    }
                    let task = told_task(&outcome.narration, &found.source, &items);
                    match ctx.ask(&task, &["yes", "no"]).as_deref() {
                        Some("yes") => st.told += 1,
                        Some(_) => {}
                        None => st.judge_errors += 1,
                    }
                }
            }
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "One campaign per run on the {} fixture; each run plays the steps in order. A lookup finds something when its Turn records at least one item (`turn::stored_found`: the act happened, and the guard kept an item). A negative step that records an item is a false positive. The judge reads each lookup Narration with its items. A failed Turn counts in its step's total only.\n",
        sc.fixture
    );
    let _ = writeln!(
        out,
        "- Lookups that found something: {} / {} ({})",
        st.found,
        st.lookups,
        pct(st.found, st.lookups)
    );
    let _ = writeln!(
        out,
        "- Items per lookup that found something: {}",
        if st.found == 0 {
            "n/a".to_owned()
        } else {
            format!("{:.1}", st.items as f64 / st.found as f64)
        }
    );
    let _ = writeln!(
        out,
        "- Negatives with a found result (false positives): {} / {} ({})",
        st.false_found,
        st.negatives,
        pct(st.false_found, st.negatives)
    );
    let _ = writeln!(
        out,
        "- Lookup Narrations that tell a found item (judge): {} / {} ({}) ({} judge errors)",
        st.told,
        st.lookups,
        pct(st.told, st.lookups),
        st.judge_errors
    );
    let _ = writeln!(
        out,
        "- Lookup Narrations that name an item title (every content word; no pass line): {} / {} ({})",
        st.named,
        st.lookups,
        pct(st.named, st.lookups)
    );
    let _ = writeln!(
        out,
        "- Item titles the Narration names: {} / {} ({})",
        st.titles_named,
        st.titles,
        pct(st.titles_named, st.titles)
    );
    let _ = writeln!(
        out,
        "- Turns failed: {} / {} ({})",
        st.failed,
        st.turns,
        pct(st.failed, st.turns)
    );
    let _ = writeln!(
        out,
        "- Turn time (committed Turns, {} of {}): p50 {}, p90 {}, max {}",
        st.total_ms.len(),
        st.turns,
        secs(percentile(&st.total_ms, 50)),
        secs(percentile(&st.total_ms, 90)),
        secs(st.total_ms.iter().max().copied())
    );
    let _ = writeln!(
        out,
        "- Lookup Turn time: p50 {}, p90 {}, max {}",
        secs(percentile(&st.lookup_ms, 50)),
        secs(percentile(&st.lookup_ms, 90)),
        secs(st.lookup_ms.iter().max().copied())
    );
    Ok(out)
}
