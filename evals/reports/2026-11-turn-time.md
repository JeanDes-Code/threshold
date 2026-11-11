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
