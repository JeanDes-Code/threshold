# Turn time on a large world: live measurement

- Date: 2026-11-11
- Model: `gemma4:26b-a4b-it-qat` (num_ctx 16384), Ollama 0.32.15
- Machine: 30 GB of RAM, 10 to 14 GB available during the tests, a 12 GB GPU. `ollama ps` shows the model 63% on CPU and 37% on GPU.
- Prompts: the proposal and Narration prompts of the QA Campaign (17 Turns, 12 Open Items), read from its debug log, sent to Ollama's `/api/chat` with the app's options. A copy of the Campaign file, outside the repository.

QA saw 13 of 17 Turns over 15 s (median 17 s), and two first Turns after about 3.5 minutes idle at 45 s. The eval gives a p90 under 10 s on the small fixture. These are the numbers behind the difference.

## Prompt size

| Prompt | Tokens |
|---|---|
| Proposal, QA Campaign (Turn 15) | 4,606 |
| Proposal, QA Campaign (Turn 6) | 3,735 |
| Narration, QA Campaign (Turn 15) | 2,543 |
| Proposal, town fixture with 10 Open Items (eval `open_items`) | p50 2,441, max 3,157 |

A real world doubles the proposal prompt. Read cold, the 4,606 tokens take 4.8 s on this machine; read from Ollama's prompt cache, 0.05 to 0.07 s.

## The prompt cache works

Five calls in a row: proposal, Narration, the next proposal, the Narration again, the next proposal again. The second call of each prompt read its prefix from the cache: 134 ms for the Narration and 128 ms for the proposal. Ollama keeps both prompts' prefixes, so the two calls of a Turn do not evict each other. The prompts already put the stable blocks first (rules, Adventure, scene) and the Turn's history and Intent last. No change is needed here.

## keep_alive

The app sends `keep_alive: "30m"` (`threshold.toml`, the default of `OllamaConfig`). After 4 minutes idle the model was still loaded: `load_duration` 2 ms. The slow Turns after an idle gap are not a model reload. A break longer than 30 minutes unloads the model, and the next Turn pays the load. We kept 30 minutes: a longer value holds 15 GB after the Player quits.

## After an idle gap: the runner is swapped out

| Call | Prompt read | Answer |
|---|---|---|
| Turn 15 proposal, not cached | 4.8 s | 0.8 s |
| Same prompt again at once | 0.05 s | 0.6 s |
| Same prompt after 4 minutes idle | 87.8 s | 1.5 s |
| Same prompt again at once | 0.07 s | 1.0 s |

After 4 minutes idle the same prompt took 87.8 s to read, 18 times the cold time. At that moment the `llama-server` runner had 9.2 GB of model file in RAM and 7.6 GB of its own memory in swap (`VmSwap`). The kernel's `vm.swappiness` is 150, so it swaps the runner's memory out while it is idle. The next call pages it back in. The same thing happened at the start of eval runs: the first Turn after a `cargo` build took 35 to 60 s, and one Turn per run timed out at 60 s in 3 of the 5 eval runs of this pass.

So the 45 s Turns after an idle gap are the machine's memory, not the prompt. No change to the app removes them.

## What the code changed: a shorter answer

`settles` (ADR 0007, bug pass 3) replaced one status per Open Item with one choice. On the QA Campaign's acceptance Turn, 3 calls of each shape with the prompt cached:

| Shape | Answer | Generation | Wall time |
|---|---|---|---|
| `open_items`, 12 statuses | about 190 tokens | 4.5 s | 5.0 s |
| `settles`, one choice | about 73 tokens | 1.6 s | 2.2 s |

The proposal of a large world gets about 2.8 s shorter. The prompt grows by 92 tokens, the list of the offered items.

## Options, not done

These change the machine or carry a risk, so they are the owner's call:

- Lower `vm.swappiness` (for example to 10), so the kernel keeps the runner's memory while it is idle.
- Free GPU memory: `voxtype` holds 1.75 GB, the browser and the compositor about 1.2 GB. More layers on the GPU means less on the CPU, and less to swap.
- Lock the model in RAM (Ollama's `use_mlock`). It would pin about 17 GB on a 30 GB machine.
- A smaller `num_ctx` (8192) halves the KV cache. The QA prompts fit, but the app has no guard against a prompt that Ollama cuts silently, so we did not do it.
- Warm the prompt prefix when the Player starts typing. It overlaps the swap-in with typing; it does not remove it.

## Limits

- One machine, one session, a few calls per measure. The idle test ran once.
- The swapped size is a snapshot of `/proc/<pid>/status`, not a trace over time.
- The Narration's time (6 to 9 s in QA) was not changed: about 200 tokens of prose at 40 to 50 tokens per second.

## Bug pass 4 (2026-11-11): what the prompt prefix shares, Turn after Turn

Not measured on the live model yet. These numbers come from prompt text only: the QA Campaign's stored prompts (QA pass 3, 20 consecutive proposal pairs) and the same Turns' proposal prompts rebuilt by the code before and after the reorder of ADR 0001 (bug pass 4). Tokens are estimated at 4 characters each. Script: `scripts/qa/prompt_prefix.py` on a copy of the Campaign file.

| Proposal prompts | Mean shared prefix | Share of the prompt | Pairs that part before 6,000 chars |
|---|---|---|---|
| Stored by QA pass 3 | 9,862 chars (about 2,470 tokens) | 59.1% | 6 of 20: 4,598 to 5,651 chars |
| Rebuilt, before the reorder | 10,398 chars (about 2,600 tokens) | 59.4% | 6 of 20: 4,746 to 5,955 chars |
| Rebuilt, after the reorder | 11,369 chars (about 2,840 tokens) | 64.8% | 2 of 20: 4,746 chars (the two place changes) |

The rebuilt prompts are longer than the stored ones by about 700 characters: they carry the other-name notes of bug pass 4 (MA). Of the 6 early parts, 4 were an ENTITIES line whose key list had changed and 2 a place change. After the reorder, those 4 Turns share 8,917 to 11,531 characters instead of 4,977 to 5,955: about 1,000 to 1,500 tokens fewer to read on such a Turn, about 1 to 1.5 s at the cold rate above, if the cache holds. The 13 other Turns already part at `Minute N`, the first line of the `recent` block, and do not change. The `recent` block (about 5,500 characters, 1,400 tokens: the recent Turns and the Intent) is read on every Turn, because the window of recent Turns moves by one Turn each time.

The stored timings show a second cause that the prompt does not own:

| Call | Turns 2 to 4 | Turn 6 on |
|---|---|---|
| Proposal after the previous Turn's narration | 3.3 to 3.8 s | 5.4 to 11 s |
| Retry right after a proposal (same prefix) | (none) | 2.0 to 2.8 s |

From Turn 6 on, a proposal that shared about 3,000 tokens with the previous proposal read its whole prompt, while a retry right after a proposal read from the cache. So the narration call between two proposals took the proposal's cache. A proposal prompt and a narration prompt share about 12 characters at the start, so with one cache slot no call reuses anything. Turn 6 is also the Turn whose first proposal call timed out after 60 s. We cannot tell from the Campaign file whether Ollama then ran with fewer slots; the server log of that night would. The app cannot choose a slot: Ollama's `/api/chat` has no such field, and the number of slots is `OLLAMA_NUM_PARALLEL`, a server setting we did not change.

Options, not done:

- Start the proposal and the narration prompt with the same block (the Adventure Seed block, about 800 characters in this Campaign, about 200 tokens), so that one slot still serves both calls a little. It moves the rules after that block; the live model must be measured first.
- Cut the window of recent Turns in steps (keep 3 to 6 Turns) instead of one Turn at a time, so its start stays the same for several Turns. It changes what the model reads.

Live checks still to run: the per-call `prompt_eval_count` of Ollama on a replay of these Turns, with the server's slot count noted.

## After the review of bug pass 4 (2026-11-11)

Nothing here was measured on the live model. The review removed the inline other-name notes that the rebuilt prompts of the table above carried (about 700 characters), and put one NAMES block at the end of the stable block instead. So the "Rebuilt" rows above no longer describe the code: their prefix lengths include notes that the prompts no longer hold. A narration prompt also gained the departure line of review B1 and, on a guarded event, the line `Nothing the PC notices happens.`; both are in the `recent` block. A replay of the QA Campaign's Turns with Ollama's per-call `prompt_eval_count` is still the live check to run.

## Bug pass 5 (2026-11-11): a replay of the QA Campaign, per call

- Command: `threshold-eval --scenario replay --runs 1 --campaign <copy of the QA Campaign after Begin> --intents <its 28 Intents>`; both files stay outside the repository. Each model call writes a `metrics` debug row with Ollama's numbers; the eval reads them.
- Machine: 10 to 11 GB of RAM available before each run, the model loaded, 63% CPU and 37% GPU.
- "Prompt read" is `prompt_eval_duration`. Ollama 0.32.15 reports `prompt_eval_count` as the whole prompt, cache included, so it does not show the cache; a call counts as reused when its prompt is read in under 400 µs per token (a cold read takes about 1,000).

| Measure | Before (ca1fc37) | One shared block (tried, removed) | After (window first, clock after) |
|---|---|---|---|
| Turn time p50 / p90 / max | 13.0 / 15.2 / 17.1 s | 12.6 / 15.2 / 41.8 s | 9.0 / 13.1 / 13.6 s |
| Turns over 15 s | 4 / 28 | 4 / 27 | 0 / 28 |
| First word of the Narration, p50 / p90 | 9.9 / 11.0 s | 9.4 / 11.0 s | 5.3 / 9.7 s |
| Proposal prompt, tokens p50 | 4,558 | 5,210 | 4,437 |
| Proposal prompt read, p50 / p90 | 4,732 / 5,146 ms | 3,856 / 5,484 ms | 1,191 / 4,645 ms |
| Proposals reused from the cache | 0 / 28 | not counted | 15 / 28 |
| Narration prompt, tokens p50 | 2,602 | 2,540 | 2,425 |
| Narration prompt read, p50 / p90 | 2,968 / 3,045 ms | 2,688 / 3,073 ms | 1,142 / 2,585 ms |
| Proposal answer, tokens p50 / generation p50 | 60 / 1,473 ms | 75 / 1,778 ms | 75 / 1,725 ms |
| Narration answer, tokens p50 / generation p50 | 171 / 3,367 ms | 167 / 3,205 ms | 162 / 3,077 ms |
| Turns failed | 0 | 1 (timeout, first Turn) | 0 |

- A first run of the "after" layout right after a `cargo` build had one Turn time out (the first, 17 s of prompt read, as in the section on the swapped runner above), one narration read in 6.5 s, and p50 10.3 s; the run in the table followed it with no build in between.
- In the "after" run, the proposal was read in full on 13 of 28 Turns: every third Turn, when the window of past Turns steps, and on Turns after one that changed the scene. A narration read in about 1.1 s reused its cache in part (the newest Turn and THIS TURN, about 600 tokens, were read); the count of reused calls uses the stricter 400 µs per token.
- Probes behind the layout (scripts outside the repository): a 4,608-token prompt changed 500 tokens before its end was read in about 700 ms, changed 550 or more tokens before its end in about 4,700 ms; three different prompts sent in turn all stayed in the cache.

Limits:

- One run of 28 Turns per layout, one world, one machine. The "before" run started after a model load, the "after" run in the table on a warm model.
- The scene still comes before the Turns, so a Turn that changes it costs the next proposal a full read.
- Owner's options, not measured: none of the server settings named in bug pass 4 is the limit. A model with no sliding window, or a larger one, would reuse more; that is a model choice.
