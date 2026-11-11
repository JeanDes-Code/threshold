//! The departures scenario (bug pass 5, M1): the Player sends an NPC away,
//! a World Event makes another NPC leave, then the PC looks around and
//! talks. QA saw the narrator make NPCs leave while Here, the place card and
//! the NPC cards kept them in the room.
//!
//! For each committed Turn the judge reads the Narration with the list of
//! people the PC knows are here after the Turn (`seen_present`) and counts:
//! - a departure told with no recorded departure: the Narration says that
//!   someone on the list leaves or is gone;
//! - a departed NPC told as present: the Narration shows someone who left
//!   in an earlier Turn here.
//!
//! Code counts the recorded departures: NPCs on the list before the Turn
//! and not after it.

use std::{collections::BTreeSet, fmt::Write as _};

use threshold_core::{
    context::seen_present,
    domain::{Effect, EntityKind, EntityRef, LedgerOp, Slot, Trigger},
    normalize,
    store::{CampaignId, Store, TurnKind, TurnRecord, WorldView},
};

use super::{Ctx, Session, entity_id, scenario};

/// The Setup Turn that schedules the event. With `moves_actor_to`, the
/// event carries the actor's `location` effect: the place's name, or no
/// value for `away`.
fn seed_event(
    store: &mut Store,
    campaign: CampaignId,
    event: &scenario::DepartureEvent,
) -> Result<(), String> {
    let view = store.snapshot(campaign).map_err(|e| e.to_string())?;
    let actor = entity_id(&view, &event.actor)?;
    let seen_by = event
        .seen_by
        .iter()
        .map(|n| entity_id(&view, n).map(EntityRef::Id))
        .collect::<Result<Vec<_>, _>>()?;
    let effects = event
        .moves_actor_to
        .iter()
        .map(|to| {
            let away = normalize::value(to) == "away";
            let value = if away { String::new() } else { to.clone() };
            let text = if away {
                format!("{} left.", event.actor)
            } else {
                format!("{} went to {to}.", event.actor)
            };
            Effect::SetFact {
                slot: Slot {
                    subject: EntityRef::Id(actor),
                    key: "location".into(),
                    value,
                },
                text,
                seen_by: Vec::new(),
            }
        })
        .collect();
    let record = TurnRecord {
        campaign_id: campaign,
        op_id: "__departure_event__".into(),
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
        effects,
        seen_by,
        public: false,
    }];
    store.commit(record, ops).map_err(|e| format!("{e:?}"))?;
    Ok(())
}

/// The NPCs the PC knows are here, by name.
fn here(view: &WorldView) -> BTreeSet<String> {
    let present = seen_present(view);
    view.entities
        .iter()
        .filter(|e| e.kind == EntityKind::Npc && present.contains(&e.id))
        .map(|e| e.name.clone())
        .collect()
}

fn snapshot(s: &Session) -> Result<WorldView, String> {
    s.engine
        .store()
        .snapshot(s.campaign)
        .map_err(|e| e.to_string())
}

#[derive(Default)]
struct Counts {
    turns: usize,
    failed: usize,
    judged: usize,
    told_unrecorded: usize,
    recorded: usize,
    gone_judged: usize,
    gone_shown: usize,
    judge_errors: usize,
}

pub(crate) fn scenario(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::Departures = scenario::load(&ctx.config.root, "departures")?;
    let mut c = Counts::default();
    for run in 1..=ctx.config.runs {
        let mut s = ctx.session(
            &sc.fixture,
            format!("departures-r{run}"),
            15_000 + run as u64,
            |store, campaign| seed_event(store, campaign, &sc.event),
        )?;
        // Who left in an earlier Turn of this run.
        let mut gone: BTreeSet<String> = BTreeSet::new();
        for intent in &sc.steps {
            let before = here(&snapshot(&s)?);
            let played = ctx.play(&mut s, intent);
            c.turns += 1;
            let Ok(outcome) = &played.outcome else {
                c.failed += 1;
                continue;
            };
            let after = here(&snapshot(&s)?);
            let left: Vec<&String> = before.difference(&after).collect();
            c.recorded += left.len();
            let list =
                |names: &BTreeSet<String>| names.iter().cloned().collect::<Vec<_>>().join(", ");
            (ctx.log)(&format!(
                "{} departures: here after [{}], left this Turn [{}]",
                played.op_id,
                list(&after),
                left.iter()
                    .map(|n| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            if !after.is_empty() {
                c.judged += 1;
                let task = format!(
                    "The people here with the player character after this turn are: {}.\n\nThe narration:\n{}\n\nDoes the narration say that one of these people leaves, has left, or is no longer here (yes), or not (no)?",
                    list(&after),
                    outcome.narration
                );
                match ctx.ask(&task, &["yes", "no"]).as_deref() {
                    Some("yes") => {
                        c.told_unrecorded += 1;
                        (ctx.log)(&format!(
                            "{} judge: a departure told, none recorded",
                            played.op_id
                        ));
                    }
                    Some(_) => {}
                    None => c.judge_errors += 1,
                }
            }
            let earlier: BTreeSet<String> = gone.difference(&after).cloned().collect();
            if !earlier.is_empty() {
                c.gone_judged += 1;
                let task = format!(
                    "These people left earlier and are not here: {}.\n\nThe narration:\n{}\n\nDoes the narration show one of them here, in this place: seen, heard, speaking, sleeping or acting here (yes), or not (no)? Being mentioned as absent or elsewhere is no.",
                    list(&earlier),
                    outcome.narration
                );
                match ctx.ask(&task, &["yes", "no"]).as_deref() {
                    Some("yes") => {
                        c.gone_shown += 1;
                        (ctx.log)(&format!(
                            "{} judge: a departed NPC shown here",
                            played.op_id
                        ));
                    }
                    Some(_) => {}
                    None => c.judge_errors += 1,
                }
            }
            gone.extend(left.into_iter().cloned());
            gone.retain(|n| !after.contains(n));
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "One campaign per run on the {} fixture, {} steps. A seeded World Event ({}) makes {} leave. The judge reads each Narration with the people the PC knows are here after its Turn.\n",
        sc.fixture,
        sc.steps.len(),
        if sc.event.moves_actor_to.is_some() {
            "with the actor's location effect"
        } else {
            "with no location effect"
        },
        sc.event.actor
    );
    let _ = writeln!(out, "| Measure | Count |");
    let _ = writeln!(out, "|---|---|");
    let _ = writeln!(
        out,
        "| Departures told with no recorded departure (judge) | {} of {} Turns |",
        c.told_unrecorded, c.judged
    );
    let _ = writeln!(out, "| Recorded departures | {} |", c.recorded);
    let _ = writeln!(
        out,
        "| Departed NPCs shown as present later (judge) | {} of {} Turns |",
        c.gone_shown, c.gone_judged
    );
    let _ = writeln!(out, "| Turns failed | {} of {} |", c.failed, c.turns);
    let _ = writeln!(out, "| Judge errors | {} |", c.judge_errors);
    Ok(out)
}
