# Open items at scale: live measurement

- Date: 2026-11-11
- Model: `gemma4:26b-a4b-it-qat` (num_ctx 16384), Ollama 0.32.15. The machine was under load: about 10 to 14 GB of RAM free, the model 63% on CPU and 37% on GPU.
- Scenario: `evals/scenarios/open_items.toml`, town fixture, 4 runs of 7 Turns
- Command: `threshold-eval --scenario open_items --runs 4 --raw-log <file outside the repository>`

The PC holds 10 Open Items before play, as in a Campaign built from a Source Text: two pairs of near-duplicates (the tithe roll, the flour debt), items on NPCs who are absent (Tobin, at the mill), and one item on a Faction. Each run plays, in order: an unrelated look around, "I ask Orsel for more time" (keep), "I tell Orsel yes: I will copy the tithe roll before dusk" (accept), a question about the walk to the mill (unrelated), "I will not come to supper" to Hessa (refuse), a question about Tobin and the flour (keep), and sharpening a quill (unrelated). A step with items is right when one of them closes in its Turn. For the acceptance, either tithe-roll item counts. Any other item that closes in a Turn is a wrong close.

## Results

| Measure | Target | Attempt 1 (d899a42, `open_items`) | Attempt 2 (`settles`) |
|---|---|---|---|
| Acceptances that closed the asked item | >= 3 / 4 | 2 / 4 | 4 / 4 |
| Refusals that closed the asked item | >= 3 / 4 | 4 / 4 | 4 / 4 |
| Wrong closes on keep steps | 0 | 0 / 8 steps | 0 / 8 steps |
| Wrong closes on unrelated steps | 0 | 4 / 12 steps | 0 / 12 steps |
| Wrong closes in total | <= 1 | 4 | 0 |
| Turns failed | (none) | 0 / 28 | 1 / 28 (timeout) |
| Turn time | (none) | p50 10.0 s, p90 13.8 s, max 57.2 s | p50 8.3 s, p90 11.6 s, max 30.0 s |
| Proposal prompt tokens | (none) | p50 2441, max 3157 | p50 2470, max 3078 |

Attempt 1 is the code before this change: one required status for each of the 10 items. Attempt 2 is ADR 0007, section "Changed in bug pass 3": one `settles` choice among the items the Intent names.

What the raw proposals show:

- **Attempt 1.** In 2 of 4 runs, the acceptance came back with all 10 statuses `open`. On the next Turn, a question about the walk to the mill, the model set both tithe-roll items to `accepted` and `done`: 2 wrong closes per run. This is the late close QA saw.
- **Attempt 2.** Every acceptance named `E1 pending_tithe_roll_answer` with outcome `done`, and every refusal `E7 pending_supper_invitation` with `refused`, on the first attempt. On "I ask Orsel for more time", the model named a tithe-roll item with outcome `open` 4 times. On the question about Tobin and the flour, it chose `none` 4 times. The unrelated steps that name no item had no `settles` field at all.
- **The failed Turn** of attempt 2 is the first Turn of run 1. Its proposal call timed out at 60 s. The first Turns of attempt 1 took 50 and 57 s. Both came right after a `cargo` build, on a machine short of memory: see `evals/reports/2026-11-turn-time.md`.

## The QA Campaign, offline

On the 17 Intents of the QA Campaign (`world::items_named_by` run on its saved states, no model call), the rule offers 0 to 6 items instead of 12. The acceptance offers the winter post request with 2 items about Harrowmere. The refusal offers the Guild's offer with 4 other items about the ferry or a sale. The distance question offers 2 items about the ferry house, and not the winter post.

On that Campaign's acceptance Turn, 3 calls of each shape on the live model: the old answer has about 190 tokens (4.5 s to generate), the new one about 73 tokens (1.6 s). The old shape closed 3 items in its first call; the new one closed the winter post request.

## Limits

- One fixture, one phrasing per step and 4 runs: the counts are small.
- Only acceptances and refusals were asked. `done`, `failed` and `dropped` were not measured as steps.
- The acceptance closed the `Wren` copy of the tithe roll with `done`. Its near-duplicate on Orsel stays open: the build merges only items with the same sentence.
- An Intent that settles an item without naming it ("Yes.") offers no item, so it closes nothing. The scenario does not test that case.
- The judge is not used: closes are read from the ledger through `world::pc_sight`.
