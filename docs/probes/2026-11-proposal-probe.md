# Proposal probe

Date: 2026-11-08
Model: `gemma4:26b-a4b-it-qat`
Command: `cargo run -p threshold-ollama --example probe --release`

## Run 1

### Method

The probe calls `OllamaGm::warm_up` before measuring. Because the initial cold load exceeded 60 seconds under memory and swap pressure, an external load request warmed the model first; the probe still called `warm_up` before its measured calls. It sends the same town-hall system, stable-world, and scene blocks for 30 runs of each Intent: a public insult, picking the locked archive door, and asking Hessa to fetch Orsel in ten minutes. Requests use `proposal_schema()`, `think: false`, `num_ctx: 16384`, and `keep_alive: 30m`.

A response is valid when its message content deserializes as `DecisionProposal`. Token and duration percentiles use nearest rank. `total_ms` is Ollama's `total_duration` converted from nanoseconds to milliseconds. Field counts include successfully parsed proposals only; optional list and scalar fields count as present when nonempty, nonzero, or true.

### Results

| Intent | Valid | Eval tokens p50 / p90 / max | Total ms p50 / p90 |
| --- | ---: | ---: | ---: |
| Public insult | 15/30 (50.0%) | 283 / 373 / 413 | 21497 / 23530 |
| Lock picking | 21/30 (70.0%) | 268 / 372 / 404 | 16287 / 19182 |
| Scheduled event | 21/30 (70.0%) | 235 / 273 / 281 | 19498 / 22025 |

These are the completed Run 1 results in `probe-output.txt`. The raw output records invalid handles and multiline JSON. The first report was written before the run completed.

## Run 2 (2b)

The probe used the same model and 30 calls per Intent. The per-Turn schema allowed E1-E5, F1-F6 and N1. It allowed no event or goal handles. The prompt requested one-line JSON, short stakes and plans, and no empty optional fields. A valid result parsed as `DecisionProposal` and passed `validate_shape`.

| Intent | Valid | Eval tokens p50 / p90 / max | Eval tok/s p50 | Prompt tokens p50 / p90 | Total ms p50 / p90 | Outputs with newline | Check with non-check verdict |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Public insult | 30/30 (100.0%) | 118 / 160 / 182 | 37.23 | 322 / 322 | 3257 / 4558 | 29 | 0 |
| Lock picking | 30/30 (100.0%) | 118 / 196 / 213 | 42.93 | 318 / 318 | 2703 / 4583 | 30 | 0 |
| Scheduled event | 30/30 (100.0%) | 110 / 155 / 177 | 43.82 | 322 / 322 | 2692 / 3826 | 25 | 0 |

Percentiles use nearest rank. Eval tok/s uses Ollama's `eval_count / eval_duration`. Prompt and eval counts come from successful calls. The valid rate uses all 30 attempts as its denominator. The probe did not check proposal semantics or validate the response against the JSON Schema after generation.

Every proposal included a Check, including all 30 scheduled-event proposals. None of the scheduled-event proposals included a schedule. These counts do not establish whether the decisions make sense for the Intent. The prompt did not prevent newlines.

## Prompt reuse

`cache_probe` sent five calls for each `num_ctx`, with the same 125-line system message and a different user message. Ollama reported `prompt_eval_count = 3044` for every call. The durations below are `prompt_eval_duration`, in milliseconds.

| num_ctx | Call 1 | Call 2 | Call 3 | Call 4 | Call 5 |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 16384 | 3276.580 | 1599.898 | 327.021 | 343.204 | 315.068 |
| 8192 | 3961.013 | 382.383 | 375.765 | 1018.917 | 368.233 |

The reported token count did not fall after the first call at either context size. Later prompt-evaluation durations were much shorter. These metrics alone do not establish how many prefix tokens Ollama actually recomputed. Changing `num_ctx` did not change the reported counts; this run does not isolate its effect on cache reuse.
