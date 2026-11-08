# Proposal probe

Date: 2026-11-08
Model: `gemma4:26b-a4b-it-qat`
Command: `cargo run -p threshold-ollama --example probe --release`

## Method

The probe calls `OllamaGm::warm_up` before measuring. Because the initial cold load exceeded 60 seconds under memory and swap pressure, an external load request warmed the model first; the probe still called `warm_up` before its measured calls. It sends the same town-hall system, stable-world, and scene blocks for 30 runs of each Intent: a public insult, picking the locked archive door, and asking Hessa to fetch Orsel in ten minutes. Requests use `proposal_schema()`, `think: false`, `num_ctx: 16384`, and `keep_alive: 30m`.

A response is valid when its message content deserializes as `DecisionProposal`. Token and duration percentiles use nearest rank. `total_ms` is Ollama's `total_duration` converted from nanoseconds to milliseconds. Field counts include successfully parsed proposals only; optional list and scalar fields count as present when nonempty, nonzero, or true.

## Results

Pending the completed warm-model run.
