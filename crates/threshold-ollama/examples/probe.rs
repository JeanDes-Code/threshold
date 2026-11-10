use std::error::Error;

use threshold_core::{
    Gm, HandleContext, Prompt,
    domain::{DecisionProposal, Handle},
    proposal_schema_for, validate_shape,
};
use threshold_ollama::{OllamaConfig, OllamaGm};

const RUNS: usize = 30;

const SYSTEM: &str = "You are the Game Master for a single-player tabletop role-playing game. Never decide the player character's actions, words, or feelings. Return only one-line minified JSON with no spaces or newlines, matching the supplied schema. Stake values and reaction plans must be short snake_case or at most 6 words. Omit optional fields that are empty. Include check only when verdict is check; when verdict is check, include check. Never set numeric difficulty: express a Check through its win and lose stakes and factors. Reference entities and facts only by their handles.\n";

const STABLE: &str = "WORLD\nE1 Orsel (npc, age 54), a human magistrate\nE2 Hessa (npc, age 36), a human clerk\nE3 the player character\nE4 Rusk (npc, age 29), a human clerk\nE5 the archive door\nFACTS\nF1 E1 role = magistrate\nF2 E2 role = clerk\nF3 E4 role = clerk\nF4 E3 location = town_hall\nF5 E5 locked = yes (archive_door.locked = yes; OPEN OBSTACLE)\nF6 E5 location = archive\n";

const SCENE: &str = "SCENE\nThe town hall is open to the public. Magistrate Orsel is hearing petitions at the front of the room. Hessa and Rusk are copying records nearby and can hear a raised voice. The locked archive door stands at the back.\n";

#[derive(Default)]
struct IntentStats {
    valid: usize,
    eval_tokens: Vec<u64>,
    total_ms: Vec<u64>,
    prompt_tokens: Vec<u64>,
    eval_millitokens_per_second: Vec<u64>,
    newline_outputs: usize,
    unexpected_checks: usize,
    fields: FieldPresence,
}

#[derive(Default)]
struct FieldPresence {
    action: usize,
    verdict: usize,
    blockers: usize,
    check: usize,
    reactions: usize,
    new_npcs: usize,
    facts: usize,
    cancel: usize,
    schedule: usize,
    minutes: usize,
    intimate: usize,
}

impl FieldPresence {
    fn observe(&mut self, proposal: &DecisionProposal) {
        self.action += 1;
        self.verdict += 1;
        self.blockers += usize::from(!proposal.blockers.is_empty());
        self.check += usize::from(proposal.check.is_some());
        self.reactions += usize::from(!proposal.reactions.is_empty());
        self.new_npcs += usize::from(!proposal.new_npcs.is_empty());
        self.facts += usize::from(!proposal.facts.is_empty());
        self.cancel += usize::from(!proposal.cancel.is_empty());
        self.schedule += usize::from(proposal.schedule.is_some());
        self.minutes += usize::from(proposal.minutes > 0);
        self.intimate += usize::from(proposal.intimate);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let gm = OllamaGm::new(OllamaConfig::default());
    gm.warm_up()?;
    println!("model=gemma4:26b-a4b-it-qat runs_per_intent={RUNS} warm_up=ok");

    let intents = [
        (
            "public_insult",
            "Intent: I publicly insult Magistrate Orsel, calling him a pompous bribe-taking peacock so both clerks can hear.",
        ),
        (
            "lock_picking",
            "Intent: While Orsel and the clerks are distracted, I pick the lock on the archive door.",
        ),
        (
            "scheduled_event",
            "Intent: I ask Hessa to fetch Magistrate Orsel in ten minutes, then wait for her to return with him.",
        ),
    ];

    let schema = proposal_schema_for(&HandleContext {
        entities: (1..=5).map(Handle::Entity).collect(),
        facts: (1..=6).map(Handle::Fact).collect(),
        events: vec![],
        goals: vec![],
        new_slots: 1,
        ..Default::default()
    });
    for (name, intent) in intents {
        let prompt = Prompt {
            system: SYSTEM.into(),
            stable: STABLE.into(),
            scene: SCENE.into(),
            recent: intent.into(),
        };
        let mut stats = IntentStats::default();
        for run in 1..=RUNS {
            match gm.propose(&prompt, &schema) {
                Ok(raw) => {
                    stats.newline_outputs += usize::from(raw.contains('\n'));
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                        stats.unexpected_checks += usize::from(
                            value.get("check").is_some_and(|v| !v.is_null())
                                && value.get("verdict").and_then(|v| v.as_str()) != Some("check"),
                        );
                    }
                    match serde_json::from_str::<DecisionProposal>(&raw) {
                        Ok(proposal) => match validate_shape(&proposal) {
                            Ok(()) => {
                                stats.valid += 1;
                                stats.fields.observe(&proposal);
                            }
                            Err(error) => {
                                eprintln!("{name} run {run}: invalid shape: {error}; raw={raw}")
                            }
                        },
                        Err(error) => {
                            eprintln!("{name} run {run}: invalid proposal: {error}; raw={raw}")
                        }
                    }
                    if let Some(metrics) = gm.last_metrics() {
                        stats.eval_tokens.push(metrics.eval_tokens);
                        stats.prompt_tokens.push(metrics.prompt_tokens);
                        stats.total_ms.push(metrics.total_ms);
                        if metrics.eval_duration_ns > 0 {
                            stats.eval_millitokens_per_second.push(
                                (metrics.eval_tokens as f64 * 1e12
                                    / metrics.eval_duration_ns as f64)
                                    as u64,
                            );
                        }
                    }
                }
                Err(error) => eprintln!("{name} run {run}: call failed: {error}"),
            }
        }
        print_stats(name, &mut stats);
    }
    Ok(())
}

fn print_stats(name: &str, stats: &mut IntentStats) {
    let valid_percent = 100.0 * stats.valid as f64 / RUNS as f64;
    println!("\nINTENT {name}");
    println!("valid={}/{} ({valid_percent:.1}%)", stats.valid, RUNS);
    stats.eval_tokens.sort_unstable();
    stats.total_ms.sort_unstable();
    stats.prompt_tokens.sort_unstable();
    stats.eval_millitokens_per_second.sort_unstable();
    println!(
        "eval_tokens p50={} p90={} max={}",
        display_stat(percentile(&stats.eval_tokens, 0.50)),
        display_stat(percentile(&stats.eval_tokens, 0.90)),
        display_stat(stats.eval_tokens.last().copied()),
    );
    println!(
        "total_ms p50={} p90={}",
        display_stat(percentile(&stats.total_ms, 0.50)),
        display_stat(percentile(&stats.total_ms, 0.90)),
    );
    let speed = percentile(&stats.eval_millitokens_per_second, 0.50)
        .map_or_else(|| "n/a".to_owned(), |v| format!("{:.2}", v as f64 / 1000.0));
    println!("eval_tok_per_s p50={speed}");
    println!(
        "prompt_tokens p50={} p90={}",
        display_stat(percentile(&stats.prompt_tokens, 0.50)),
        display_stat(percentile(&stats.prompt_tokens, 0.90))
    );
    println!(
        "outputs_with_newline={} check_with_non_check_verdict={}",
        stats.newline_outputs, stats.unexpected_checks
    );
    let fields = &stats.fields;
    println!(
        "fields action={} verdict={} blockers={} check={} reactions={} new_npcs={} facts={} cancel={} schedule={} minutes={} intimate={}",
        fields.action,
        fields.verdict,
        fields.blockers,
        fields.check,
        fields.reactions,
        fields.new_npcs,
        fields.facts,
        fields.cancel,
        fields.schedule,
        fields.minutes,
        fields.intimate,
    );
}

fn percentile(sorted_values: &[u64], percentile: f64) -> Option<u64> {
    if sorted_values.is_empty() {
        return None;
    }
    let rank = (sorted_values.len() as f64 * percentile).ceil() as usize;
    sorted_values.get(rank.saturating_sub(1)).copied()
}

fn display_stat(value: Option<u64>) -> String {
    value.map_or_else(|| "n/a".into(), |value| value.to_string())
}
