//! The open-items scenario (bug pass 3, 2026-11-11): the PC holds 10 Open
//! Items before play, as in a Campaign built from a Source Text, and plays
//! an acceptance, a refusal, two negative steps and unrelated Turns. It
//! counts, from the PC's sight after each Turn, the right closes and the
//! wrong ones: an item that closes in a Turn whose step does not name it.

use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as _,
};

use threshold_core::{
    domain::{Canon, EntityKind, EntityRef, FactSource, LedgerOp, Slot},
    normalize,
    store::{CampaignId, Store, TurnId, TurnKind, TurnRecord},
    world::pc_sight,
};

use super::{Ctx, Session, entity_id, scan::percentile, scenario, scenario::ItemKind, secs};

/// Commits every item as one Setup Turn, each witnessed by the PC.
fn seed_items(
    store: &mut Store,
    campaign: CampaignId,
    items: &[scenario::NamedPending],
) -> Result<(), String> {
    let view = store.snapshot(campaign).map_err(|e| e.to_string())?;
    let pc = view
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Pc)
        .ok_or("the fixture has no PC")?
        .id;
    let mut ops = Vec::new();
    for item in items {
        ops.push(LedgerOp::AddFact {
            slot: Slot {
                subject: EntityRef::Id(entity_id(&view, &item.subject)?),
                key: normalize::key(&item.key),
                value: normalize::value(&item.value),
            },
            text: item.text.clone(),
            canon: Canon::Established,
            source: FactSource::AdventureSeed,
            seen_by: vec![EntityRef::Id(pc)],
            causes: vec![],
        });
    }
    let record = TurnRecord {
        campaign_id: campaign,
        op_id: "__open_items__".into(),
        kind: TurnKind::Setup,
        intent: String::new(),
        proposal_json: "{}".into(),
        clock_before: view.clock_min,
        clock_after: view.clock_min,
    };
    store.commit(record, ops).map_err(|e| format!("{e:?}"))?;
    Ok(())
}

/// The ids of the items that the PC's sight shows closed in `turn`.
fn closed_in(
    s: &Session,
    items: &[(String, u64, String)],
    turn: TurnId,
) -> Result<Vec<String>, String> {
    let store = s.engine.store();
    let view = store.snapshot(s.campaign).map_err(|e| e.to_string())?;
    let turns = store.turns(s.campaign).map_err(|e| e.to_string())?;
    let reverted: HashSet<TurnId> = store
        .reverted_turns(s.campaign)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let sight = pc_sight(&view, &turns, &reverted).ok_or("the campaign has no PC")?;
    Ok(items
        .iter()
        .filter(|(_, subject, key)| {
            sight
                .open_items
                .iter()
                .any(|i| i.subject == *subject && i.key == *key && i.closed_in == Some(turn))
        })
        .map(|(id, _, _)| id.clone())
        .collect())
}

#[derive(Default)]
struct KindStats {
    steps: usize,
    failed: usize,
    right: usize,
    wrong: usize,
}

pub(crate) fn scenario(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::OpenItems = scenario::load(&ctx.config.root, "open_items")?;
    let mut kinds: BTreeMap<ItemKind, KindStats> = BTreeMap::new();
    let (mut total_ms, mut tokens): (Vec<u64>, Vec<u64>) = (Vec::new(), Vec::new());
    let mut turns = 0;
    let mut wrong_items: BTreeMap<String, usize> = BTreeMap::new();
    for run in 1..=ctx.config.runs {
        let mut s = ctx.session(
            &sc.fixture,
            format!("items-r{run}"),
            11_000 + run as u64,
            |store, campaign| seed_items(store, campaign, &sc.items),
        )?;
        let view = s
            .engine
            .store()
            .snapshot(s.campaign)
            .map_err(|e| e.to_string())?;
        let items: Vec<(String, u64, String)> = sc
            .items
            .iter()
            .map(|i| {
                Ok((
                    i.id.clone(),
                    entity_id(&view, &i.subject)?,
                    normalize::key(&i.key),
                ))
            })
            .collect::<Result<_, String>>()?;
        for step in &sc.steps {
            let played = ctx.play(&mut s, &step.intent);
            turns += 1;
            let stats = kinds.entry(step.kind).or_default();
            stats.steps += 1;
            let Ok(outcome) = &played.outcome else {
                stats.failed += 1;
                continue;
            };
            total_ms.push(outcome.timings.total_ms);
            tokens.extend(outcome.timings.proposal_prompt_tokens);
            let closed = closed_in(&s, &items, outcome.turn_id)?;
            if closed.iter().any(|id| step.closes.contains(id)) {
                stats.right += 1;
            }
            for id in closed.iter().filter(|id| !step.closes.contains(id)) {
                stats.wrong += 1;
                *wrong_items.entry(id.clone()).or_default() += 1;
            }
            (ctx.log)(&format!(
                "{} {:?}: closed [{}]",
                played.op_id,
                step.kind,
                closed.join(", ")
            ));
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "One campaign per run on the {} fixture. The PC holds {} Open Items before play. A step with items is right when one of them closes in its Turn; any other item that closes is a wrong close. A failed Turn counts in its step's total only.\n",
        sc.fixture,
        sc.items.len()
    );
    let _ = writeln!(out, "| Step kind | Steps | Right | Wrong closes | Failed |");
    let _ = writeln!(out, "|---|---|---|---|---|");
    let mut wrong = 0;
    for (kind, st) in &kinds {
        wrong += st.wrong;
        let right = match kind {
            ItemKind::Accept | ItemKind::Refuse => format!("{} / {}", st.right, st.steps),
            ItemKind::Keep | ItemKind::Unrelated => "(none asked)".to_owned(),
        };
        let _ = writeln!(
            out,
            "| {kind:?} | {} | {right} | {} | {} |",
            st.steps, st.wrong, st.failed
        );
    }
    let _ = writeln!(out, "\n- Wrong closes in total: {wrong}");
    if !wrong_items.is_empty() {
        let list: Vec<String> = wrong_items
            .iter()
            .map(|(id, n)| format!("{id} {n}"))
            .collect();
        let _ = writeln!(out, "- Wrong closes by item: {}", list.join(", "));
    }
    let _ = writeln!(
        out,
        "- Turn time (committed Turns, {} of {turns}): p50 {}, p90 {}, max {}",
        total_ms.len(),
        secs(percentile(&total_ms, 50)),
        secs(percentile(&total_ms, 90)),
        secs(total_ms.iter().max().copied())
    );
    let _ = writeln!(
        out,
        "- Proposal prompt tokens: p50 {}, max {}",
        percentile(&tokens, 50).map_or_else(|| "n/a".into(), |n| n.to_string()),
        tokens
            .iter()
            .max()
            .map_or_else(|| "n/a".into(), ToString::to_string)
    );
    Ok(out)
}
