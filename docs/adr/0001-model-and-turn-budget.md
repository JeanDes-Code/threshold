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
- **Stable context first.** Ollama reuses the processed prompt when its start is unchanged. A fresh 10K-token prompt costs about 11 s to process (about 900 tok/s). A reused one costs almost nothing. The Context Builder therefore puts the Adventure Seed, the Ruleset and the world summary first, in a fixed order. It puts recent Turns and the Intent last.
- **Fresh context budget of about 4-6K tokens per Turn.** Above that, a Turn with a cold prompt exceeds 15 s.
- **One model behind an adapter.** Domain and world logic never name a model. Changing the Ollama tag is a configuration change.
- The `-64k` tag's `num_ctx 65536` is a ceiling, not a per-Turn setting. A larger context pushes more of the model onto the CPU.

## Changed in bug pass 4 (2026-11-11): the scene block puts what a Turn changes last

QA saw the proposal call of most Turns from Turn 6 on read about 4,500 tokens again, against 700 to 1,100 on Turns 2 to 5. `scripts/qa/prompt_prefix.py` compares the proposal prompts that the QA Campaign's debug log stored, Turn after Turn. On 13 of 20 pairs the prompts share about 11,900 characters (about 3,000 tokens) and part at `Minute N`, the first line of the `recent` block, as designed. On 6 pairs they part early in the scene block, at 4,600 to 5,700 characters: four times at an ENTITIES line whose key list had changed (a new key on Agathe, Ludo, Sefa or the PC), twice at a place change. One pair parts at the SCHEDULED EVENTS block.

The proposal prompt's scene block now has this order: place, PLACES, ENTITIES (handle, name, kind and age only), VOICES, NPC GOALS, FACTS (Persistent facts first, then the others, each in id order), OPEN OBSTACLES, SCHEDULED EVENTS, then ENTITY STATE: one line per entity with its attitude to the PC and its key list. The model gets the same facts, but not the same lines: each entity line is split in two (its identity under ENTITIES, its attitude and keys under the new `ENTITY STATE` header), and the facts are in a new order (corrected after review, see below). `MENTIONED ENTITIES` in the `recent` block keeps the one-line form. The narrator prompt is unchanged.

The prompt cache does not explain the rest of the QA numbers. From Turn 6 on, a proposal that shared about 3,000 tokens with the previous proposal still took 6 to 11 s, while a retry right after a proposal took 2 to 3 s. So the previous proposal's cache was gone when the next proposal came: the narration call between them took its place. A proposal and a narration prompt share about 12 characters ("You are the "), so with one cache slot each call reads its whole prompt. Ollama's `/api/chat` has no field to choose a slot, so the adapter cannot keep one slot per call; the number of slots is a server setting (`OLLAMA_NUM_PARALLEL`). The numbers are in `evals/reports/2026-11-turn-time.md`.

## Changed after the review of bug pass 4 (2026-11-11)

- **The reorder changed lines, not only their order.** The section above said "the same lines; only their order changed". That was wrong: the entity lines were split, and `ENTITY STATE` is a new header. The sentence is corrected above.
- **`MENTIONED ENTITIES` keeps the one-line form** (`context/proposal_prompt.rs`, `recent`). It sits in the `recent` block, after `Minute N`, the first line that changes on every Turn, so a split there would save no prefix and would add a header to the part the model reads fresh each Turn. The model reads two forms of an entity line; the evaluation would show if that confuses it.
- **The NAMES block** (data shape, section 13, review M5) sits at the end of the stable block of the narrator, Begin and proposal prompts. It follows the data, so it changes only when a knower of an other name changes. It replaces the inline notes of bug pass 4, which marked lines in every block, the `recent` block included.
- None of the review's changes was measured on the live model.
