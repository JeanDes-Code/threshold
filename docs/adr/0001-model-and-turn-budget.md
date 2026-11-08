---
status: accepted
date: 2026-11-08
---

# One MoE model by default, 15 s per Turn

A Turn must take 15 s or less on the target machine (RTX 4070 SUPER, 12 GB VRAM, 32 GB RAM). The default GM model is `gemma4:26b-a4b-it-qat`, a mixture-of-experts model. It handles both the Decision Proposal and the Narration. The Player owns the 15 s budget; it is a product constraint, not a measurement.

## Considered options

Probes on 2026-11-08, Ollama 0.32.15, `num_ctx` 8192 to 16384:

| Model | Generation | Placement | Cold load |
|---|---|---|---|
| `qwen3.8:27b` (dense, Q4_K_M) | 6-7 tok/s | 49% CPU / 51% GPU | 48 s |
| `gemma4:26b-a4b-it-qat` (MoE) | 62-67 tok/s | 37% CPU / 63% GPU | 43 s |
| `huihui_ai/qwen3-abliterated:14b` | 48 tok/s | not recorded | 20 s, reloaded on every call (cause unknown) |

- **27B for everything.** Its Narration alone (about 300 tokens) takes about 45 s. Rejected for the default path. It may come back later as an opt-in narration mode, only if a blind comparison shows better prose.
- **Different models per role** (small model for decisions, 27B for Narration). Both models do not fit in 12 GB VRAM together, so each switch costs a reload of about 40 s. Rejected on this hardware.

With the MoE at 16K context and a 10K-token prompt, we measured a warm Turn (decision plus about 280 tokens of Narration) at 8.9 s. 15 of 15 decisions matched the schema.

## Consequences

- **Keep the model loaded.** The app sends a warm-up call with a long `keep_alive` at start. Without it, the first Turn costs about 45 s more.
- **Stable context first.** Ollama reuses the processed prompt when its start is unchanged. A fresh 10K-token prompt costs about 11 s to process (about 900 tok/s). A reused one costs almost nothing. The Context Builder therefore puts the Charter, the Ruleset and the world summary first, in a fixed order. It puts recent Turns and the Intent last.
- **Fresh context budget of about 4-6K tokens per Turn.** Above that, a Turn with a cold prompt exceeds 15 s.
- **One model behind an adapter.** Domain and world logic never name a model. Changing the Ollama tag is a configuration change.
- The `-64k` tag's `num_ctx 65536` is a ceiling, not a per-Turn setting. A larger context pushes more of the model onto the CPU.
