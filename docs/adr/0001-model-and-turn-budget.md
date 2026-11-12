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

## Changed in bug pass 5 (2026-11-11): the sliding window decides what the cache can reuse

QA pass 4: median Turn 13.8 s, 6 of 30 Turns over 15 s, and the first word of a Narration came later after Turn 10. A replay of the QA Campaign's 28 Intents on a copy of its file (`threshold-eval --scenario replay`, numbers in `evals/reports/2026-11-turn-time.md`) gave the same picture before this change: every proposal read its prompt in about 4.7 s and every narration in about 3.0 s, so nothing came from the cache.

Probes on the live server (scripts outside the repository) showed that the reading of bug pass 4 was wrong on two points:

- **`prompt_eval_count` counts the whole prompt** on Ollama 0.32.15, cached part included. The same 4,608-token prompt sent twice reports 4,608 both times, read in 4,700 ms and then in 260 ms. Only `prompt_eval_duration` shows the cache. The "4,500 tokens read fresh" of bug pass 4 was the prompt's size.
- **The server keeps several cache slots.** Three different prompts A, B, C, then A and B again: A and B were read from the cache. One slot was not the limit; `OLLAMA_NUM_PARALLEL` is not the owner's lever here.
- **The limit is the model's sliding window.** `gemma4:26b-a4b-it-qat` has attention layers with a sliding window. A prompt that parts from a cached one 500 tokens or less before its end was read in about 700 ms; one that parts 550 tokens or more before the end was read in full, about 4,700 ms, however long the shared start. So a prompt is reused only when it differs from the cached sequence (the previous prompt and its answer) in its last ~500 tokens.

So the old layout could never be reused: the first line of the `recent` block was `Minute N`, which changes every Turn, and the window of recent Turns dropped its oldest Turn every Turn, both about 1,500 tokens before the end.

We first tried what bug pass 4 proposed: both prompts start with one shared block (the Adventure Seed, the PC's scene, the past Turns). Measured on the replay, it was worse: the narration then shared a long start with the proposal, Ollama gave it the proposal's slot, could not reuse it past the window, and read it in full; proposals read 3.9 s and narrations 2.7 s, with 0 of 27 narrations reused. We removed it.

The layout now:

- **The past Turns come first in `recent`, the clock and the Intent after them** (`context::turns_window`). Proposal: RECENT TURNS, then NOW (`Minute N`), MENTIONED, OPEN ITEMS (for settles), INTENT. Narration: RECENT TURNS (the Turns before the one it narrates; THIS TURN tells that one), then THIS TURN. The two prompts still start differently ("You are the Game Master" / "You are the narrator"), so each finds its own slot.
- **The window of past Turns moves in steps.** Its first Turn is the latest step boundary (`seq` 1, 4, 7, …) that leaves at least 3 Turns (`TURNS_WINDOW_MIN`, `TURNS_WINDOW_STEP`), so it holds 3 to 5 Turns and keeps its start for three Turns in a row. Over 5,000 chars (`TURNS_WINDOW_MAX_CHARS`) it starts one step later. Between two steps, the next proposal differs from the cached one only after the newest Turn.
- **What grows is bounded.** The GM's FACTS block is at most 4,500 chars (`GM_FACTS_MAX_CHARS`): over it, the oldest facts that are not a `location` or an open item go, Established ones first. The narrator's WHAT THE PC KNOWS HERE is at most 3,000 chars (`PC_KNOWS_MAX_CHARS`), its oldest lines going 5 at a time. The QA world stays under both.

A proposal is still read in full when the window steps (one Turn in three) or when the Turn before changed the scene (a fact, a key, an attitude, who is here), because the scene comes before the Turns. Putting the scene after the Turns would put about 900 tokens after the point where two prompts part, past the window.

Replay of the 28 QA Intents, warm model (`evals/reports/2026-11-turn-time.md`): Turn time p50 13.0 s → 9.0 s, p90 15.2 s → 13.1 s, max 17.1 s → 13.6 s, Turns over 15 s 4 → 0; first word of the Narration p50 9.9 s → 5.3 s; 15 of 28 proposals and most narrations read from the cache.

## Changed on 2026-11-12: the HTTP timeout of a model call is 180 s

The default `timeout_s` of `OllamaConfig` goes from 60 to 180 s. The owner's machine swaps the model out of memory between sessions; a call that waits for it to come back took 63 s and failed at 60, so the Turn failed while the model was only slow. The 15 s budget is a product target for a warm Turn, not a timeout: a call that runs past 15 s still finishes. The World Build keeps its own timeout (`build::BUILD_TIMEOUT_S`, 240 s).

The shell writes `threshold.toml` with the defaults only when the file is absent (`session::load_config`). A file written before this change keeps `timeout_s = 60` until the Player edits it.
