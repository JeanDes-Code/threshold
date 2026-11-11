//! The play-basics scenario (QA, 2026-11-10): a move, an impossible act,
//! a contest with an NPC, the acceptance of a pending item and a move to a
//! place that is not an entity, on the live model. Two negative steps
//! (review, 2026-11-10) talk about the open item and about another place
//! without settling or going. It counts what the ledger and the PC's sight
//! show after each Turn, and the Turns that move the World Clock; the judge
//! reads only the Narrations of the impossible act and of the unknown-place
//! move.

use std::{collections::HashSet, fmt::Write as _};

use threshold_core::{
    domain::{Canon, EntityKind, EntityRef, FactSource, LedgerOp, Slot},
    normalize,
    store::{CampaignId, Store, TurnId, TurnKind, TurnRecord},
    world::pc_sight,
};

use super::{
    Ctx, Session, entity_id, pc_location, pct, scan::percentile, scenario, scenario::BasicKind,
    secs,
};

#[derive(Default)]
struct Stats {
    turns: usize,
    failed: usize,
    total_ms: Vec<u64>,
    moves: usize,
    moved: usize,
    impossible: usize,
    told_failed: usize,
    judge_errors: usize,
    contests: usize,
    rolled: usize,
    /// Contests whose Narration tells the contest and its result (judge).
    told_contest: usize,
    /// Committed Narrations whose first sentence speaks of the time.
    time_first: usize,
    accepts: usize,
    closed: usize,
    unknown: usize,
    stayed: usize,
    told_stayed: usize,
    no_close: usize,
    false_closes: usize,
    no_move: usize,
    false_moves: usize,
    committed: usize,
    advanced: usize,
}

/// Commits the pending item as a Setup Turn, witnessed by the PC.
fn seed_pending(
    store: &mut Store,
    campaign: CampaignId,
    pending: &scenario::Pending,
) -> Result<(), String> {
    let view = store.snapshot(campaign).map_err(|e| e.to_string())?;
    let subject = entity_id(&view, &pending.subject)?;
    let pc = view
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Pc)
        .ok_or("the fixture has no PC")?
        .id;
    let record = TurnRecord {
        campaign_id: campaign,
        op_id: "__pending_item__".into(),
        kind: TurnKind::Setup,
        intent: String::new(),
        proposal_json: "{}".into(),
        clock_before: view.clock_min,
        clock_after: view.clock_min,
    };
    let ops = vec![LedgerOp::AddFact {
        slot: Slot {
            subject: EntityRef::Id(subject),
            key: normalize::key(&pending.key),
            value: normalize::value(&pending.value),
        },
        text: pending.text.clone(),
        canon: Canon::Established,
        source: FactSource::AdventureSeed,
        seen_by: vec![EntityRef::Id(pc)],
        causes: vec![],
    }];
    store.commit(record, ops).map_err(|e| format!("{e:?}"))?;
    Ok(())
}

/// The PC's location now, normalized: the scene place.
fn place(s: &Session) -> Result<Option<String>, String> {
    let view = s
        .engine
        .store()
        .snapshot(s.campaign)
        .map_err(|e| e.to_string())?;
    Ok(pc_location(&view).map(|p| normalize::name(&p)))
}

/// The World Clock now.
fn clock(s: &Session) -> Result<u64, String> {
    Ok(s.engine
        .store()
        .snapshot(s.campaign)
        .map_err(|e| e.to_string())?
        .clock_min)
}

/// True when the PC's sight shows the pending item closed in `turn`.
fn closed_in(s: &Session, pending: &scenario::Pending, turn: TurnId) -> Result<bool, String> {
    let store = s.engine.store();
    let view = store.snapshot(s.campaign).map_err(|e| e.to_string())?;
    let turns = store.turns(s.campaign).map_err(|e| e.to_string())?;
    let reverted: HashSet<TurnId> = store
        .reverted_turns(s.campaign)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let subject = entity_id(&view, &pending.subject)?;
    let key = normalize::key(&pending.key);
    let sight = pc_sight(&view, &turns, &reverted).ok_or("the campaign has no PC")?;
    Ok(sight
        .open_items
        .iter()
        .any(|i| i.subject == subject && i.key == key && i.closed_in == Some(turn)))
}

/// True when the first sentence of `narration` speaks of the time passing:
/// it names minutes or hours.
pub(crate) fn opens_with_time(narration: &str) -> bool {
    let first = narration
        .split(['.', '!', '?', '\n'])
        .find(|s| !s.trim().is_empty())
        .unwrap_or("")
        .to_lowercase();
    ["minute", "hour"].iter().any(|w| first.contains(w))
}

pub(crate) fn scenario(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::PlayBasics = scenario::load(&ctx.config.root, "play_basics")?;
    let mut st = Stats::default();
    for run in 1..=ctx.config.runs {
        let mut s = ctx.session(
            &sc.fixture,
            format!("basics-r{run}"),
            9_000 + run as u64,
            |store, campaign| seed_pending(store, campaign, &sc.pending),
        )?;
        for step in &sc.steps {
            let before = place(&s)?;
            let clock_before = clock(&s)?;
            let played = ctx.play(&mut s, &step.intent);
            st.turns += 1;
            let outcome = match &played.outcome {
                Ok(o) => {
                    st.total_ms.push(o.timings.total_ms);
                    st.committed += 1;
                    if opens_with_time(&o.narration) {
                        st.time_first += 1;
                    }
                    if clock(&s)? > clock_before {
                        st.advanced += 1;
                    }
                    Some(o)
                }
                Err(_) => {
                    st.failed += 1;
                    None
                }
            };
            match step.kind {
                BasicKind::Move => {
                    st.moves += 1;
                    let after = place(&s)?;
                    // Right only at the asked place, when the step names it.
                    let right = match &step.to {
                        Some(to) => after.as_deref() == Some(normalize::name(to).as_str()),
                        None => after != before,
                    };
                    if outcome.is_some() && right {
                        st.moved += 1;
                    }
                }
                BasicKind::NoClose => {
                    st.no_close += 1;
                    if let Some(o) = outcome
                        && closed_in(&s, &sc.pending, o.turn_id)?
                    {
                        st.false_closes += 1;
                    }
                }
                BasicKind::NoMove => {
                    st.no_move += 1;
                    if outcome.is_some() && place(&s)? != before {
                        st.false_moves += 1;
                    }
                }
                BasicKind::Impossible => {
                    st.impossible += 1;
                    if let Some(o) = outcome {
                        let task = format!(
                            "The player tried something the world does not allow.\n\nPlayer intent: {}\n\nThe narration:\n{}\n\nDoes the narration tell that the attempt fails or does not happen (yes), or does it tell the attempt as done (no)?",
                            step.intent, o.narration
                        );
                        match ctx.ask(&task, &["yes", "no"]).as_deref() {
                            Some("yes") => st.told_failed += 1,
                            Some(_) => {}
                            None => st.judge_errors += 1,
                        }
                    }
                }
                BasicKind::Contest => {
                    st.contests += 1;
                    if let Some(o) = outcome
                        && let Some(roll) = o.roll
                    {
                        st.rolled += 1;
                        let result = if roll.success { "won" } else { "lost" };
                        let task = format!(
                            "The player character had a contest with another character, and the player character {result} it.\n\nPlayer intent: {}\n\nThe narration:\n{}\n\nDoes the narration tell the contest and that the player character {result} it (yes), or does it leave the contest or its result out (no)?",
                            step.intent, o.narration
                        );
                        match ctx.ask(&task, &["yes", "no"]).as_deref() {
                            Some("yes") => st.told_contest += 1,
                            Some(_) => {}
                            None => st.judge_errors += 1,
                        }
                    }
                }
                BasicKind::UnknownMove => {
                    st.unknown += 1;
                    if let Some(o) = outcome
                        && place(&s)? == before
                    {
                        st.stayed += 1;
                        let task = format!(
                            "The player tried to go to a place that does not exist in this world, so the player character does not get there.\n\nPlayer intent: {}\n\nThe narration:\n{}\n\nDoes the narration tell that the player character does not arrive there (yes), or does it tell the arrival as done (no)?",
                            step.intent, o.narration
                        );
                        match ctx.ask(&task, &["yes", "no"]).as_deref() {
                            Some("yes") => st.told_stayed += 1,
                            Some(_) => {}
                            None => st.judge_errors += 1,
                        }
                    }
                }
                BasicKind::Accept => {
                    st.accepts += 1;
                    if let Some(o) = outcome
                        && closed_in(&s, &sc.pending, o.turn_id)?
                    {
                        st.closed += 1;
                    }
                }
            }
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "One campaign per run on the {} fixture; each run plays the steps in order. A failed Turn counts in its step's total and not in its success count. The scene place is the PC's `location` after the Turn; a move is right only when it is the place the step names. A negative step talks about the open item or another place without settling it or going there: a close or a move there is false. The judge reads only the Narrations of the impossible act and of the unknown-place move.\n",
        sc.fixture
    );
    let ratio = |n: usize, d: usize| format!("{n} / {d} ({})", pct(n, d));
    let _ = writeln!(
        out,
        "- Moves that reached the asked place: {}",
        ratio(st.moved, st.moves)
    );
    let _ = writeln!(
        out,
        "- False closes / negative steps: {}",
        ratio(st.false_closes, st.no_close)
    );
    let _ = writeln!(
        out,
        "- False moves / negative steps: {}",
        ratio(st.false_moves, st.no_move)
    );
    let _ = writeln!(
        out,
        "- Turns with a clock advance > 0 (committed Turns): {}",
        ratio(st.advanced, st.committed)
    );
    let _ = writeln!(
        out,
        "- Impossible acts narrated as failure (judge): {} ({} judge errors)",
        ratio(st.told_failed, st.impossible),
        st.judge_errors
    );
    let _ = writeln!(
        out,
        "- Contests with a Roll: {}",
        ratio(st.rolled, st.contests)
    );
    let _ = writeln!(
        out,
        "- Contests told with their result (judge): {}",
        ratio(st.told_contest, st.rolled)
    );
    let _ = writeln!(
        out,
        "- Narrations that open with the time (committed Turns): {}",
        ratio(st.time_first, st.committed)
    );
    let _ = writeln!(
        out,
        "- Pending items closed: {}",
        ratio(st.closed, st.accepts)
    );
    let _ = writeln!(
        out,
        "- Moves to an unknown place that left the PC in place: {}; told as not arriving (judge): {}",
        ratio(st.stayed, st.unknown),
        ratio(st.told_stayed, st.unknown)
    );
    let _ = writeln!(out, "- Turns failed: {}", ratio(st.failed, st.turns));
    let _ = writeln!(
        out,
        "- Turn time (committed Turns): p50 {}, p90 {}, max {}",
        secs(percentile(&st.total_ms, 50)),
        secs(percentile(&st.total_ms, 90)),
        secs(st.total_ms.iter().max().copied())
    );
    Ok(out)
}
