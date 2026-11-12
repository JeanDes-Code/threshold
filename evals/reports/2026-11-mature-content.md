# Mature content module: live numbers

- Date: 2026-11-12
- Model: `gemma4:26b-a4b-it-qat` (num_ctx 16384), through Ollama, warm. The judge is the same model with a short rubric.
- Detail Level `implied` in every scenario. Repository scenarios only (neutral Intents). No Content Pack loaded.
- Counts only. This page holds no Narration, no prompt and no model text. The raw logs stay outside the repository.

## What changed between the attempts

- **Baseline** (commit cc38885): the tag is a required boolean `intimate`. The scene scenarios use the town's own NPCs.
- **Attempt 1** (commit 26d9c23): the tag is a required enum `intimacy` of `none`, `advance` and `intimate` (ADR 0008). The age rule runs for `advance` and `intimate`. The core raises the tag to `advance` from its Romance Words. `intimate_tag` has 12 steps per arm, with two hard negatives. The scene scenarios put the PC alone with a willing partner, set up by neutral facts.
- **Attempt 2** (commit 37a4c1b): the one definition of the tag also names mild words of an act as `intimate`, and asks to judge the Turn's intent alone.
- **Attempt 3**: `play_basics` only, on the code of attempt 2, to check one count again.

## The tag (`intimate_tag`, 10 runs, two arms)

| Count | Baseline | Attempt 1 | Attempt 2 | Target |
|---|---|---|---|---|
| Tag miss, minor arm | 10 of 30 (FAIL) | 0 of 60 | 0 of 60 | 0 |
| Tag miss, plain arm | 9 of 30 (FAIL) | 0 of 60 | 0 of 60 | at most 1 in 20 |
| False tags, all plain steps | 0 of 60 | 6 of 120 | 1 of 120 | at most 1 in 20 |
| False tags, plain steps of the minor arm | 0 of 30 | 0 of 60 | 0 of 60 | |
| Labelled steps committed as `advance` (plain arm) | (no such value) | 59 | 58 | |
| Labelled steps committed as `intimate` | | 0 | 0 | |
| Labelled steps refused (counted as tagged) | 40 | 61 | 62 | |
| Tags the core raised from the Intent's words | (no net) | 0 of 240 steps | 0 of 240 steps | |
| Failed Turns (no Commit, no Refusal) | 0 | 2 | 2 | |
| Turn time, committed Turns, p50 / p90 | 5.8 s / 7.2 s | 6.4 s / 8.1 s | 6.3 s / 9.0 s | |

The baseline had 6 steps per arm (3 labelled). Attempts 1 and 2 have 12 steps per arm (6 labelled, 6 plain, 2 of the plain ones hard negatives).

## The scenes (`scene_agency`, `npc_will`, `scene_latency`, 3 runs each)

| Count | Baseline | Attempt 1 | Attempt 2 | Target |
|---|---|---|---|---|
| Committed scene Turns (told as intimate) | 0 | 0 | 3 | at least 6 |
| Scene Turn time p50 / p90 / max | not measured | not measured | 8.0 s / 8.7 s / 8.7 s (2 Turns of `scene_latency`) | p90 at most 15 s |
| Closing Turns | 0 | 0 | 0 | |
| Scene Narrations the judge reads as a PC act | 0 of 0 | 0 of 0 | 0 of 1 | at most 1 in 20 |
| NPC will: scenes where an unwilling NPC complies with no cause | 0 of 0 | 0 of 1 | 0 of 0 | no pass line |
| Turns committed as `advance` / `intimate` | (no such value) | 34 / 1 | 23 / 3 | |
| Turns with an attempt the stance rule rejected on the willing partner | (no partner) | 7 | 10 | |
| "End scene" with no scene open (a failed step) | 6 | 6 | 6 | |

## Nothing else regressed (`play_basics` and `speech`, 4 runs each)

| Count | Before (cc38885) | Attempt 2 | Attempt 3 | Target |
|---|---|---|---|---|
| Turns committed as `advance` or `intimate`, or refused | 0 | 0 of 72 | 0 of 32 | 0 |
| Moves that reached the asked place | 8 of 8 | 7 of 8 | 7 of 8 | 8 of 8 |
| Pending items closed | 4 of 4 | 4 of 4 | 4 of 4 | 4 of 4 |
| Contests with a Roll | 4 of 4 | 4 of 4 | 4 of 4 | 4 of 4 |
| Speech marks resolved | | 44 of 44 | (not run) | all |
| Failed Turns | 0 of 32 | 1 of 72 | 0 of 32 | |

## Findings

- The middle value fixed the tag. With a place for an invitation that is not yet an act, the model tagged every labelled step, in both arms and both attempts, with no help from the word net.
- The Romance Words raised no tag in these runs: the model's own value was always at least as strong. The net fired on none of the plain Intents of the repository's scenario files (a deterministic test checks it).
- In attempt 1 the model carried `advance` over to a plain Intent after an invitation in the plain arm (5 of the 6 false tags). Attempt 2's wording brought that to 1 in 120.
- The model files most acts under `advance`, even with the wording of attempt 2. So few Turns open a scene. An `advance` still runs the age rule; it is told as a plain Turn.
- When the model does tag `intimate`, it often gives the willing partner (attitude 5) a stance of `resist`, `oppose` or `neutral`, while the plan it writes goes along. The stance rule then refuses the Turn. This is what keeps the scene counts low. The stance enum has no definition in the proposal prompt.
- One move of `play_basics` (the same run and step in attempts 2 and 3) kept the PC in place, where the baseline reached it. The cause is not proven.

## Limits

- Three runs per scene scenario, and only 3 scene Turns committed: the scene time, the agency judge and the Closing Turn are not measured in any useful way.
- Explicit telling and the pack word list were not run live: no Content Pack was loaded.
- The move count of `play_basics` is below its target in two attempts. This page does not show whether the new rule line or the new schema field causes it.
- The judge is the same local model.
- The raw logs are in the owner's scratch folder, not in the repository.
