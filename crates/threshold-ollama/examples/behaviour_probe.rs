//! Measures two behaviours of the proposal prompt on the live model:
//! whether it adds a Check when none is warranted, and whether it schedules
//! an event when an NPC will act later. Prompts come from
//! `context::proposal_context` on a small fixture built through the store.
//!
//! Run: `cargo run -p threshold-ollama --example behaviour_probe`

use std::error::Error;

use threshold_core::{
    Gm,
    checks::validate,
    context::proposal_context,
    domain::{
        AdventureSeed, DecisionProposal, EntityKind, EntityRef, LedgerOp, PcSeed, SeedEntity,
        SeedFact, StartSeed, Verdict,
    },
    proposal_schema_for,
    store::{Store, TurnKind, TurnRecord, WorldView},
    validate_shape,
};
use threshold_ollama::{OllamaConfig, OllamaGm};

const RUNS: usize = 10;

fn fact(subject: &str, key: &str, value: &str, text: &str) -> SeedFact {
    SeedFact {
        subject: subject.into(),
        key: key.into(),
        value: value.into(),
        text: text.into(),
    }
}

fn npc(name: &str) -> SeedEntity {
    SeedEntity {
        name: name.into(),
        kind: EntityKind::Npc,
        age: Some(40),
        other_names: vec![],
        hidden: false,
    }
}

/// The town hall: Ada (PC), Magistrate Orsel (proud, retaliates later through
/// others) and Hessa (a friendly clerk), all present.
fn fixture() -> Result<WorldView, Box<dyn Error>> {
    let mut store = Store::open_in_memory()?;
    let seed = AdventureSeed {
        premise: "A courier arrives in a river town run by a proud magistrate.".into(),
        pc: PcSeed {
            name: "Ada".into(),
            age: 30,
            other_names: vec![],
            concept: "a blunt courier".into(),
        },
        entities: vec![npc("Orsel"), npc("Hessa")],
        boundaries: vec!["No graphic torture.".into()],
        decided: vec![
            fact("Ada", "location", "town hall", "Ada is in the town hall."),
            fact(
                "Orsel",
                "location",
                "town hall",
                "Orsel is in the town hall.",
            ),
            fact(
                "Hessa",
                "location",
                "town hall",
                "Hessa is in the town hall.",
            ),
            fact(
                "Orsel",
                "role",
                "magistrate",
                "Orsel is the town magistrate.",
            ),
            fact("Hessa", "role", "clerk", "Hessa is a clerk."),
            fact(
                "Orsel",
                "employs",
                "investigator",
                "Orsel keeps an investigator for quiet work.",
            ),
        ],
        proposed: vec![],
        story_language: "en".into(),
        themes: vec![],
        gm_agreement: String::new(),
        resolution: String::new(),
        min_age: None,
        start: StartSeed::default(),
        open_details: vec![],
    };
    let campaign = store.create_campaign(&seed, 11)?;
    let view = store.snapshot(campaign)?;
    let id = |name: &str| {
        view.entities
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.id)
            .expect("fixture entity")
    };
    let (orsel, hessa) = (id("Orsel"), id("Hessa"));
    store.commit(
        TurnRecord {
            campaign_id: campaign,
            op_id: "fixture".into(),
            kind: TurnKind::Setup,
            intent: String::new(),
            proposal_json: "{}".into(),
            clock_before: 0,
            clock_after: 0,
        },
        vec![
            LedgerOp::AddVoiceCard {
                npc: EntityRef::Id(orsel),
                habits: "Formal and polite to a fault in public; never forgets a slight and repays it later, through others.".into(),
                vocabulary: "measured, legal words".into(),
                samples: vec!["How very kind of you to say so.".into()],
            },
            LedgerOp::AddVoiceCard {
                npc: EntityRef::Id(hessa),
                habits: "Warm, chatty, a little nosy.".into(),
                vocabulary: "plain town speech".into(),
                samples: vec!["Morning, love. Need a stamp?".into()],
            },
            LedgerOp::AddGoal {
                npc: EntityRef::Id(orsel),
                text: "Protect his standing in town".into(),
                against: None,
            },
        ],
    )?;
    Ok(store.snapshot(campaign)?)
}

#[derive(Default)]
struct Stats {
    calls: usize,
    valid: usize,
    passes_checks: usize,
    checks: usize,
    schedules: usize,
    eval_tokens: Vec<u64>,
    total_ms: Vec<u64>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let view = fixture()?;
    let gm = OllamaGm::new(OllamaConfig::default());
    gm.warm_up()?;
    println!("model=gemma4:26b-a4b-it-qat runs_per_scenario={RUNS} warm_up=ok");

    let scenarios = [
        (
            "public_insult (expect: schedule, no check)",
            "I publicly insult Magistrate Orsel, calling him a pompous bribe-taking peacock so the whole hall can hear.",
        ),
        (
            "plain_greeting (expect: allowed, no check, no schedule)",
            "I greet Hessa the clerk with a friendly good morning.",
        ),
    ];
    for (name, intent) in scenarios {
        let ctx = proposal_context(&view, &[], intent);
        let schema = proposal_schema_for(&ctx.handles);
        let mut s = Stats::default();
        for run in 1..=RUNS {
            s.calls += 1;
            let raw = match gm.propose(&ctx.prompt, &schema) {
                Ok(raw) => raw,
                Err(error) => {
                    eprintln!("{name} run {run}: call failed: {error}");
                    continue;
                }
            };
            if let Some(m) = gm.last_metrics() {
                s.eval_tokens.push(m.eval_tokens);
                s.total_ms.push(m.total_ms);
            }
            let proposal: DecisionProposal = match serde_json::from_str(&raw) {
                Ok(p) => p,
                Err(error) => {
                    eprintln!("{name} run {run}: invalid proposal: {error}; raw={raw}");
                    continue;
                }
            };
            if let Err(error) = validate_shape(&proposal) {
                eprintln!("{name} run {run}: invalid shape: {error}; raw={raw}");
                continue;
            }
            s.valid += 1;
            s.checks += usize::from(proposal.verdict == Verdict::Check);
            s.schedules += usize::from(proposal.schedule.is_some());
            match validate(&proposal, &view) {
                Ok(_) => s.passes_checks += 1,
                Err(errs) => {
                    let errs: Vec<String> = errs.iter().map(ToString::to_string).collect();
                    eprintln!(
                        "{name} run {run}: rejected: {}; raw={raw}",
                        errs.join(" | ")
                    );
                }
            }
            println!("{name} run {run}: {raw}");
        }
        report(name, &mut s);
    }
    Ok(())
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        "n/a".into()
    } else {
        format!("{:.0}%", 100.0 * n as f64 / d as f64)
    }
}

fn report(name: &str, s: &mut Stats) {
    s.eval_tokens.sort_unstable();
    s.total_ms.sort_unstable();
    let p50 = |v: &[u64]| v.get(v.len().saturating_sub(1) / 2).copied().unwrap_or(0);
    println!("\nSCENARIO {name}");
    println!(
        "valid={}/{} ({}) passes_commit_checks={}/{}",
        s.valid,
        s.calls,
        pct(s.valid, s.calls),
        s.passes_checks,
        s.valid
    );
    println!(
        "check_rate={}/{} ({}) schedule_rate={}/{} ({})",
        s.checks,
        s.valid,
        pct(s.checks, s.valid),
        s.schedules,
        s.valid,
        pct(s.schedules, s.valid)
    );
    println!(
        "eval_tokens p50={} max={} total_ms p50={}",
        p50(&s.eval_tokens),
        s.eval_tokens.last().copied().unwrap_or(0),
        p50(&s.total_ms)
    );
}
