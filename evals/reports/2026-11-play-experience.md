# Play experience: Turn time and speaker marks

The live check of Task 11 of `docs/superpowers/plans/2026-11-10-play-experience.md`. Every number below was measured in this run. The fixture is the invented town demo (`fixtures/town`).

- Run date: 2026-11-10
- Model: `gemma4:26b-a4b-it-qat`, `num_ctx` 16384, local Ollama. The model ran 65% on CPU and 35% on GPU (15 GB model, 12 GB GPU).
- Judge: the same local model, with a short rubric prompt.
- Before: commit `c16840d` (the plan's base: the last commit before Task 1), scenario `latency`, 3 runs.
- After: commit `ec4fcbb` (this branch, Tasks 1 to 11 code and the review fixes), scenarios `latency` and `speech`, 3 runs.
- Turns: 30 before (28 committed), 60 after (58 committed: 28 latency, 30 speech).
- Model time: 3.7 min before, 6.6 min after.

## Turn time

The budget of ADR 0001 is 15 s per Turn. The latency scenario plays the same ten Intents in both runs. The percentiles cover committed Turns only.

| Scenario | Commit | Committed Turns | p50 | p90 | max |
|---|---|---|---|---|---|
| latency | before `c16840d` | 28 of 30 | 7.3 s | 9.2 s | 10.1 s |
| latency | after `ec4fcbb` | 28 of 30 | 5.8 s | 8.2 s | 11.3 s |
| speech | after `ec4fcbb` | 30 of 30 | 5.8 s | 8.4 s | 9.5 s |

Proposal step p50 / p90: 3.0 s / 5.0 s before, 2.4 s / 4.6 s after. Narration step p50 / p90: 4.2 s / 4.9 s before, 3.5 s / 4.8 s after.

## Speaker marks

Each speech Turn's Narration is read the way the Play view shows it. A line of speech is each quoted span in the stored Narration.

- Lines of speech (quoted spans): 33 in 30 Turns
- Mark rate: 33 / 33 (100%); 11 / 11 in each of the three runs
- Marks that do not resolve: 0 (tag removed: 0; tag of the PC: 0; tag of no entity: 0; absent NPC or shared tag: 0)
- Marks written without quotes: 0
- Wrong marks (judge): 0 / 24 judged bubbles; the judge could not tell the speaker for 9; judge errors: 0

## Findings

- Turn time stays inside the budget. The p90 is 8.2 s after against 9.2 s before, and 8.4 s on the speech Intents. The after run is faster, but both runs are small (28 Turns each) and the model runs mostly on CPU, so the difference is noise, not a gain. The p90 did not grow by more than 1 s, so the plan's stop rule does not apply.
- The narrator followed the speech rule on every spoken line: each quoted span was a mark, and each mark became a bubble.
- No visible harm shows in the counters: no mark without its quotes, no mark tagged with the PC, no tag removed for a name the PC cannot use (so the narrator printed no unknown name in a tag), no tag naming no entity.
- The judge found no wrong speaker. For 9 of 33 bubbles it could not tell the speaker once the tag was removed: the narrator writes the mark instead of "he says", so those lines carry no other sign of their speaker. This is a limit of the judge test, not a wrong bubble.
- Each speech Turn holds about one line of speech (33 in 30 Turns), although the Intents invite dialogue. This run has no "before" count of quoted spans, so it cannot tell whether the rule makes the narrator write less dialogue.
- Limits: 3 runs of 10 Intents per scenario, one fixture, a local judge. The run read no Narration text by eye; the findings above come from the counters only. The 2 failed Turns in each latency run are proposals rejected twice: one by agency and one by NPC folds before, two by agency after. No Turn timed out.
