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

## Attempt 3: review fixes (2026-11-11)

- Code: the review fixes of ADR 0007, section "Changed after review (2026-11-11)". Three of them touch this scenario: `settles` closes only an item this Turn offered (M1); everyday verbs and nouns ("meet", "plans", "time", "night", "tell", "offer") name no item (M2); the `OPEN ITEMS (for settles)` lines carry the `(open, not done)` marker.
- Scenario: two negative steps were added, so a run has 9 Turns. "I tell Orsel a joke." comes before the acceptance; "I ask Doul about his plans for the winter." comes after the refusal. Each names an NPC who has an Open Item, and settles nothing. Through the NPC's name, each offers that NPC's items.
- Command: `threshold-eval --scenario open_items --runs 4 --raw-log <file outside the repository>`, after a release build. 21 GB of RAM were free before the run, and no model was loaded.

| Measure | Target | Attempt 2 (`settles`) | Attempt 3 (review fixes) |
|---|---|---|---|
| Acceptances that closed the asked item | >= 3 / 4 | 4 / 4 | 4 / 4 |
| Refusals that closed the asked item | >= 3 / 4 | 4 / 4 | 4 / 4 |
| Wrong closes on keep steps | 0 | 0 / 8 steps | 0 / 8 steps |
| Wrong closes on unrelated steps | 0 | 0 / 12 steps | 0 / 20 steps |
| Wrong closes on the two new name-only steps | 0 | (not played) | 0 / 8 steps |
| Wrong closes in total | <= 1 | 0 | 0 |
| Turns failed | (none) | 1 / 28 (timeout) | 0 / 36 |
| Turns that needed the retry | (none) | (not counted) | 4 / 36 |
| Turn time | (none) | p50 8.3 s, p90 11.6 s, max 30.0 s | p50 7.7 s, p90 10.6 s, max 11.1 s |
| Proposal prompt tokens | (none) | p50 2470, max 3078 | p50 2697, max 3284 |

What the raw proposals show:

- On both new steps, the schema offered the NPC's items, and the model answered `settles: {item: none, outcome: open}` in 8 of 8 Turns. A name-only offer led to no wrong close, so we did not tighten the rule (the review's fallback: a name-only match offers an item only with an answer word such as "yes" or "refuse"). This was the only live run for M2.
- Every acceptance closed `copy_answer` (Wren's item) on the first attempt, and every refusal closed `supper`.
- The 4 retries were all the same rejection: `npc folds` on Hessa, a stance `help` above her ceiling `comply`. The retry gave `comply` or `neutral`.
- The first Turn of run 1 took 10.7 s. No Turn was slow.
- The prompt is about 230 tokens longer at p50 than in attempt 2. Two causes are likely, and we did not split them: the scenario has two more steps, so later Turns carry more recent Turns; and each offered item now carries the open marker.

Limits:

- One fixture, one phrasing per step and 4 runs: the counts are small.
- The two new steps name an NPC with a joke and with a question. An Intent that names the NPC with an answer word but means another matter ("I tell Orsel no, I am not hungry") was not measured.
- The review's fix for a `settles` on an item that was not offered (M1) is not visible here: the model sent no such close. A deterministic test covers it (`a_settles_the_turn_did_not_offer_closes_nothing`).
- The judge is not used: closes are read from the ledger through `world::pc_sight`.
