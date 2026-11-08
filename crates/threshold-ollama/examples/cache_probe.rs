use std::error::Error;

use serde_json::json;
use threshold_core::{Gm, Prompt};
use threshold_ollama::{OllamaConfig, OllamaGm};

fn main() -> Result<(), Box<dyn Error>> {
    let system = (1..=125)
        .map(|i| format!("World record {i}: The magistrate hears petitions at the town hall while clerks copy records and travelers wait outside.\n"))
        .collect::<String>();
    let schema = json!({"type":"object", "properties":{"answer":{"type":"string"}}, "required":["answer"], "additionalProperties":false});
    for num_ctx in [16_384, 8_192] {
        let gm = OllamaGm::new(OllamaConfig {
            num_ctx,
            timeout_s: 300,
            ..OllamaConfig::default()
        });
        gm.warm_up()?;
        println!("num_ctx={num_ctx}");
        for call in 1..=5 {
            let prompt = Prompt {
                system: system.clone(),
                stable: String::new(),
                scene: String::new(),
                recent: format!("Answer briefly: what is record {call} about?"),
            };
            gm.propose(&prompt, &schema)?;
            let metrics = gm.last_metrics().ok_or("missing metrics")?;
            println!(
                "call={call} prompt_eval_count={} prompt_eval_duration_ns={}",
                metrics.prompt_tokens, metrics.prompt_eval_duration_ns
            );
        }
    }
    Ok(())
}
