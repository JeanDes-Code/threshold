//! The OOC scenario (bug pass 5, M2): "Ask the GM" on a Campaign whose PC
//! holds Open Items, two of them already answered. Each question is asked
//! in several phrasings; an answer is right when it holds one word of each
//! `all_of` list and none of `none_of`. No judge: the words decide.

use std::{collections::BTreeMap, fmt::Write as _};

use threshold_core::{
    domain::{Canon, CloseOutcome, EntityKind, EntityRef, FactSource, LedgerOp, Slot, outcome_key},
    normalize,
    store::{CampaignId, Store, TurnKind, TurnRecord},
};

use super::{Ctx, entity_id, scenario};

/// Commits the items as one Setup Turn, then their answers as a second one,
/// each witnessed by the PC.
fn seed(
    store: &mut Store,
    campaign: CampaignId,
    items: &[scenario::AnsweredPending],
) -> Result<(), String> {
    let view = store.snapshot(campaign).map_err(|e| e.to_string())?;
    let pc = view
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Pc)
        .ok_or("the fixture has no PC")?
        .id;
    let (mut open, mut answers) = (Vec::new(), Vec::new());
    for item in items {
        let subject = EntityRef::Id(entity_id(&view, &item.subject)?);
        let key = normalize::key(&item.key);
        open.push(LedgerOp::AddFact {
            slot: Slot {
                subject,
                key: key.clone(),
                value: normalize::value(&item.value),
            },
            text: item.text.clone(),
            canon: Canon::Established,
            source: FactSource::AdventureSeed,
            seen_by: vec![EntityRef::Id(pc)],
            causes: vec![],
        });
        if let Some(word) = &item.outcome {
            let outcome = CloseOutcome::from_word(word).ok_or("unknown outcome")?;
            answers.push(LedgerOp::AddFact {
                slot: Slot {
                    subject,
                    key: outcome_key(&key).ok_or("not an open key")?.to_owned(),
                    value: outcome.word().to_owned(),
                },
                text: outcome.text(&item.text),
                canon: Canon::Established,
                source: FactSource::Proposal,
                seen_by: vec![EntityRef::Id(pc)],
                causes: vec![],
            });
        }
    }
    for (n, ops) in [open, answers].into_iter().enumerate() {
        let now = store.snapshot(campaign).map_err(|e| e.to_string())?;
        let record = TurnRecord {
            campaign_id: campaign,
            op_id: format!("__ooc_items_{n}__"),
            kind: TurnKind::Setup,
            intent: String::new(),
            proposal_json: "{}".into(),
            clock_before: now.clock_min,
            clock_after: now.clock_min,
        };
        store.commit(record, ops).map_err(|e| format!("{e:?}"))?;
    }
    Ok(())
}

/// True when `answer` holds one word of each `all_of` list and none of
/// `none_of`, whatever the case.
pub fn right(answer: &str, all_of: &[Vec<String>], none_of: &[String]) -> bool {
    let answer = answer.to_lowercase();
    all_of
        .iter()
        .all(|any| any.iter().any(|w| answer.contains(&w.to_lowercase())))
        && !none_of.iter().any(|w| answer.contains(&w.to_lowercase()))
}

pub(crate) fn scenario(ctx: &mut Ctx) -> Result<String, String> {
    let sc: scenario::Ooc = scenario::load(&ctx.config.root, "ooc")?;
    let mut by_topic: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for run in 1..=ctx.config.runs {
        let mut s = ctx.session(
            &sc.fixture,
            format!("ooc-r{run}"),
            13_000 + run as u64,
            |store, campaign| seed(store, campaign, &sc.items),
        )?;
        for (q, question) in sc.questions.iter().enumerate() {
            for (n, phrasing) in question.phrasings.iter().enumerate() {
                let op_id = format!("{}-q{q}-{n}", s.label);
                let answer = s.engine.ooc(op_id.clone(), phrasing);
                let entry = by_topic.entry(question.topic.clone()).or_default();
                entry.0 += 1;
                match answer {
                    Ok(answer) => {
                        let ok = right(&answer, &question.all_of, &question.none_of);
                        entry.1 += usize::from(ok);
                        (ctx.log)(&format!(
                            "{op_id} {}: {}",
                            if ok { "right" } else { "WRONG" },
                            answer.replace('\n', " ")
                        ));
                    }
                    Err(e) => {
                        entry.2 += 1;
                        (ctx.log)(&format!("{op_id} FAILED {e}"));
                    }
                }
            }
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "One campaign per run on the {} fixture. The PC holds {} Open Items; {} of them are answered. Each question is asked in each of its phrasings. An answer is right when it holds one listed word of each required group and no forbidden phrase.\n",
        sc.fixture,
        sc.items.len(),
        sc.items.iter().filter(|i| i.outcome.is_some()).count()
    );
    let _ = writeln!(out, "| Question | Asked | Right | Failed calls |");
    let _ = writeln!(out, "|---|---|---|---|");
    let (mut asked, mut good) = (0, 0);
    for (topic, (n, ok, failed)) in &by_topic {
        asked += n;
        good += ok;
        let _ = writeln!(out, "| {topic} | {n} | {ok} | {failed} |");
    }
    let _ = writeln!(out, "\n- Right answers in total: {good} / {asked}");
    Ok(out)
}
