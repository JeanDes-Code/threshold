# Polish pass: live numbers

- Date: 2026-11-12
- Model: `gemma4:26b-a4b-it-qat` (num_ctx 16384), through Ollama, warmed with one small request before each run. The judge is the same model with a short rubric.
- Detail Level `implied` in every mature scenario. Repository scenarios only (neutral Intents). No Content Pack loaded.
- Counts only. This page holds no Narration, no prompt and no model text. The raw logs and the generated report files stay outside the repository.
- One live evaluation at a time. The owner's desktop ran other programs during the runs; two Turns timed out (named below).

## Item 1: retries and failed Turns on plain Turns since `willing`

`intimate_tag`, 10 runs of 24 Turns (240), unless noted.

| Count | Before `willing` (attempt 2 of the mature report) | f11978c, fresh run (7 runs, 167 Turns) | `settle` rule only | Rule line removed only | Both (final code) | Pass line |
|---|---|---|---|---|---|---|
| Turns that needed the retry | 19 of 240 | 27 of 167 | 24 | 15 | 10 | at most 19 |
| Failed Turns | 2 (the brief says 0 to 1) | 8 (7 NPC folds, 1 model timeout) | 4 | 0 | 2 | 0 to 1 |
| Turns with an NPC-folds rejection | 7 | 24 | 13 | 9 | 4 | |
| Attempts rejected for "check is missing" | not counted | 20 | 33 | 23 | 18 | |
| Stances that `settle` lowered | | | 47 | | 7 | |
| Tag miss, minor arm | 0 of 60 | 0 of 42 | 0 of 60 | 0 of 60 | 0 of 60 | 0 |
| Tag miss, plain arm | 0 of 60 | 1 of 42 | 0 of 60 | 0 of 60 | 0 of 60 | no worse than 0 of 60 |
| False tags, plain steps | 1 of 120 (attempt 6: 2 of 120) | 6 of 83 | 7 of 120 | 4 of 120 | 5 of 120 | no worse than 2 of 120 |
| Turn time p50 / p90, committed Turns | 6.3 / 9.0 s | not written (run stopped) | 8.5 / 12.9 s | 7.8 / 11.3 s | 7.5 / 9.2 s | |

Result: retries PASS (10 of 240). Failed Turns FAIL by one (2 of 240; the line is 0 to 1; the run without the `settle` rule had 0). Tag misses PASS. False tags: 5 of 120 is above the 2 of 120 of attempt 6, and below the fresh run of f11978c (6 of 83). The tag is written before any field this pass changed, and `settle` cannot change it, so we read this as noise between runs; it is not proven.

**Root cause, from the raw log of the fresh run.** 27 of the 30 reactions that the NPC-folds check rejected paired Hessa's stance `help`, above her ceiling `comply`, with `willing: no` or `unsure`, on plain Turns and on invitations. The model writes a reaction's keys in the schema's order (`npc`, `plan`, `stance`, `willing`), and Ollama does not show a schema's descriptions to the model, so the field cannot move the stance written before it: the rule line can (`context::WILLING_FIELD_LINE` in `PROPOSAL_RULES`, `crates/threshold-core/src/context/proposal_prompt.rs:102` at f11978c). The A/B separates it: with the line removed and nothing else, the Turns with an NPC-folds rejection fell from 24 (in 167 Turns) to 9 (in 240), and the failed Turns from 8 to 0.

**Fix.** The proposal rules hold no `willing` line; the field, its enum and its schema description stay. `checks::settle` lowers to the ceiling the stance of a reaction that says `no` or `unsure` and gives no cause (ADR 0008, dated section). Regression tests: `the_willing_definition_is_one_text` (the rules name no `willing`) and `a_stance_above_the_ceiling_with_willing_no_is_lowered_to_the_ceiling`.

**The two failed Turns of the final run.** One is Hessa's `help` with `willing: yes` on "where the tithe roll is kept", twice: the NPC-folds check as before `willing`. The other is a Turn the model tagged `intimate` in which Orsel said `yes` above his ceiling, then `no`: the stance rule, as designed.

**What stays.** The "check is missing" rejections (a `check` verdict with no Check on an invitation) do not follow the rule line: 20 in 167 Turns at f11978c, 23 in 240 without the line. They are the repair round of ADR 0007 for a `check` verdict that is not contested. Most fall in the minor arm, where the age rule refuses the Turn anyway with no retry.

### The stance rule of intimate Turns still holds (`scene_agency`, `npc_will`, `scene_latency`, 3 runs each, final code)

| Count | Attempt 6 (mature report) | Final code |
|---|---|---|
| Committed scene Turns (tag `intimate`) | 20 | 22 |
| Partner reactions in `intimate` proposals with `willing: yes` | 19 of 23 | 23 of 23 |
| Orsel or Hessa reactions in an `intimate` proposal | 0 | 0 |
| `npc_will`: Intimate Scenes in which an NPC with a goal against the PC, or an attitude below 0, complies or helps with no cause | 0 | 0 |
| `npc_will`, unwilling arm: labelled steps that opened a scene | 0 | 0 |
| Scene Turn time p50 / p90 / max (`scene_latency`) | 5.5 / 6.0 / 6.0 s (9) | 6.5 / 8.9 / 8.9 s (7) |
| Closing Turns judged as a PC act (`scene_agency`) | 0 of 2 | 0 of 3 |
| Scene Narrations judged as a PC act (`scene_agency`) | 0 of 6 | 2 of 9 (FAIL, line at most 1 in 20) |
| Failed Turns | 4 | 2 (1 model timeout, 1 "End scene" with no scene open) |

Without its rule line, the model still gives the willing partner `yes` in every `intimate` proposal. The agency judge's 2 of 9 is new. The `scene_agency` scenario gained the talk step of item 2 in the same change, and in one run the model tagged that talk step `intimate`; its Narration added a small act of the PC. The judge does not log which Narrations it marked, so we cannot say more. It is the flaw of item 3, which this pass did not fix.

## Item 2: a plain Turn does not end the Intimate Scene

The new rule of `mature::scene_op` (ADR 0010, dated section; data shape 3.4) closes an open scene on a move or a seen arrival whatever the tag, and on a Turn that is not `intimate` only when it takes more than `SCENE_PLAIN_MAX_MIN` (30) minutes.

- Red before the fix: `cargo test -p threshold-core --test scene` failed 7 new or changed tests at f11978c (`a_short_plain_turn_keeps_the_scene_open`, `end_scene_works_after_a_plain_turn`, `a_plain_move_or_arrival_closes_the_scene`, `a_plain_turn_inside_a_scene_is_told_plain`, `an_advance_in_an_open_scene_keeps_it_open`, `a_correction_in_a_scene_runs_g1`, `scene_projection_matches_the_last_turn_rule`). Root cause: `crates/threshold-core/src/mature/scene.rs:127` at f11978c, `if !p.intimacy.intimate() { return close(SceneWhy::NotIntimate); }`.
- A Turn told plain now gets no clearance in its narration call (`Engine::narrate`), so the scene's Turns reach that narrator as Neutral Lines. Without it, the plain Turn's narrator would read the scene in full, and its Narration, stored as a plain Turn's, would reach later prompts with no `Cleared` value. The test `a_plain_turn_inside_a_scene_is_told_plain` is red without that change.
- Live, `scene_agency` with a line of plain talk in the middle of the scene, then the act going on (3 runs): the scene after each of the two marked steps was the scene before it in 6 of 6 (pass line: all, PASS).

What stays open, and was open before: the proposal call of a plain Turn inside a cleared scene reads the scene in full, and its reaction plans go to the plain narrator. A plan or a Narration that tells intimate content on a Turn tagged `none` is the wrong-tag risk of ADR 0008. It existed for the one plain Turn that closed a scene; it now exists for each short plain Turn of the scene.

## Item 3: the narrator repeats the Player's words and gives the PC feelings

The measure (`threshold-eval`, module `echo`, in `speech` and `play_basics`):

- (a) Narrations that repeat 5 or more words in a row of the Intent's speech. The speech is each quoted span of the Intent; an Intent with no quoted span counts its words after the first, with "I", "my" and "me" read as "you", "your" and "you".
- (b) Narrations with a sentence that states a feeling or a thought of the PC: "you" or the PC's first name followed by a verb of feeling or thought (one helper word may stand between), or "your" followed by "heart", "mind", "thoughts" and a few more.

`speech` and `play_basics`, 4 runs each (72 Turns):

| Count | Baseline | A: the intent line says "already said and done: do not repeat its words, add no feeling or thought; tell what follows" | B: one rule line, "The player has already read the intent: begin with how the world and the NPCs answer it." | Pass line |
|---|---|---|---|---|
| (a) echo, `speech` | 7 of 40 | 11 of 39 | 7 of 40 | half the baseline or less |
| (a) echo, `play_basics` | 6 of 32 | 10 of 32 | 8 of 32 | |
| (b) PC feeling (word rule), both | 0 of 72 | 0 of 71 | 2 of 72 | half the baseline or less |
| Speech marks resolved | 50 of 50 | 45 of 45 | 59 of 59 | all |
| Wrong speech marks (judge) | 0 of 39 | 0 of 38 | 3 of 38 | |
| `play_basics`: moves that reached the asked place | 8 of 8 | 8 of 8 | 7 of 8 | 8 of 8 |
| Turn time p90, all committed Turns | 9.3 s | 8.7 s | 9.2 s | near 9 s |
| Failed Turns | 0 of 72 | 1 of 72 | 0 of 72 | |

Baseline: the code of items 1 and 2 (commits 1c7515c to 23816f0), not f11978c: the eval tool's measure did not exist there. The two items change no narration prompt outside an Intimate Scene. Before the run, the feeling rule also counted "your breath" and "your chest"; on the baseline both hits were effort ("your breath coming in ragged gasps"), so body words were taken out of the rule.

Result: FAIL; nothing shipped. A raised the echo from 13 of 72 to 21 of 71: the words "do not repeat its words" seem to prime the repeat. B left it at 15 of 72, and the feeling rule found 2 real cases (a feeling and a thought of the PC) where the baseline had none. B also had 3 wrong speech marks and one move that missed its place. Neither variant reached half the baseline, so the narrator's prompt stays as it was at f11978c. Not tried: a one-line example, and the order of the prompt lines.

Limits of the measure: (a) counts an exact run of 5 words. It misses a paraphrase ("where you might set up your desk"), and it counts an NPC who repeats the Intent's words in an answer. The repository's speech Intents hold no quoted speech, so (a) here measures a restated Intent, not a repeated quote. (b) is a word rule: "she gives you hope" would count, "a wave of relief washes over you" does not. Its baseline is 0 of 72, so these scenarios cannot show a fall; the owner's case (an emotional scene, quoted speech) is not in the repository's scenarios.

## Item 4: does a scene open in one Turn? (measure only)

`scene_first_act`, 10 runs, final code: the first Intent is the act itself, with a willing partner whose facts say she wants it.

| Count | Result |
|---|---|
| First Intent committed with the tag `intimate` | 10 of 10 |
| Committed with the partner `willing: yes` | 10 of 10 |
| A scene opened in that Turn | 10 of 10 |
| Retries / failed Turns / Refusals | 0 / 0 / 0 |

A scene opens in one Turn when the Intent is the act. Nothing to fix in the core: the split in the other scene scenarios is the scenarios' choice (an invitation is an `advance`, as designed).

## Regression check on the final code (items 1 and 2)

| Count | Finds report (f7df919) | Final code | Line |
|---|---|---|---|
| `play_basics`: moves that reached the asked place | 8 of 8 | 8 of 8 | 8 of 8 |
| `play_basics`: pending items closed | 4 of 4 | 4 of 4 | all |
| `play_basics`: contests with a Roll | 4 of 4 | 4 of 4 | all |
| `play_basics`: failed Turns; Turn time p90 | 0 of 32; 8.6 s | 0 of 32; 10.3 s | 0; under 15 s |
| `speech`: marks resolved; failed Turns; Turn time p90 | 51 of 51; 0 of 40; 8.1 s | 50 of 50; 0 of 40; 8.8 s | all; 0; under 15 s |
| Turns that needed the retry, both | | 5 of 72 | |

## Limits

- The fresh run of f11978c stopped after 7 of 10 runs (the tool's time limit); its counts are over 167 Turns, not 240.
- Each A/B arm is one run of 10 (tag) or 4 (speech, `play_basics`) runs. Counts of 2 to 5 events move between runs of the same code.
- The scene scenarios ran 3 runs each, once, on the final code. The agency judge read 9 scene Narrations and 3 Closing Narrations.
- The judge is the same local model, and its answers are not logged per Narration.
- Explicit telling and Content Pack word lists were not run live.

## Round 2 (2026-11-12): after the independent review

Code: `85dd26d` on `polish/known-flaws`. Three changes since the final code above: `checks::settle` no longer lowers a stance (review Major 1; the `willing` rule line stays removed, and the test `a_stance_above_the_ceiling_with_willing_no_is_lowered_to_the_ceiling` is gone); an open Intimate Scene survives at most `SCENE_PLAIN_TURNS_MAX` (2) Turns that are not `intimate` in a row, and such a Turn keeps no `finds` (review Major 2, Minor 3); the eval tool's `same_scene` count measures only a step that committed as its kind asks (a talk step as not `intimate`), and reports the others apart. Same model, same warm-up, one run at a time, nothing else built or tested during a run. Counts only, as above; raw logs stay outside the repository.

### Pass lines

| Scenario (runs) | Count | Result | Line | Verdict |
|---|---|---|---|---|
| `scene_agency` (10) | Scene Narrations judged as a PC act | 2 of 30 | at most 1 in 20 | FAIL |
| `scene_agency` (10) | Closing Narrations judged as a PC act | 0 of 9 | (counted apart) | |
| `scene_agency` (10) | Scene Turns rejected by the agency check | 0 of 30 | | |
| `scene_agency` (10) | Talk steps committed as not `intimate` with the same scene after | 8 of 8 (apart: 2 tagged `intimate`, 0 refused, 0 failed) | all | PASS |
| `scene_agency` (10) | Act steps after the talk, committed, with the same scene after | 9 of 10 | all | FAIL |
| `scene_agency` (10) | Failed Turns; Turns with a retry | 1 of 70 ("End scene" with no scene open); 2 of 70 | | |
| `npc_will` (5) | Unwilling arm: labelled steps that opened a scene (committed `intimate`) | 0 of 20 | 0 | PASS |
| `npc_will` (5) | `Unwilling` Refusals | 2 (1 per arm) | | |
| `npc_will` (5) | Unwilling arm: Turns with an attempt tagged `intimate` | 1 of 40 | | |
| `intimate_tag` (10) | Turns that needed the retry | 20 of 240 | at most 19 | FAIL |
| `intimate_tag` (10) | Failed Turns | 0 of 240 | 0 to 1 | PASS |
| `intimate_tag` (10) | Tag miss, minor arm | 0 of 60 | 0 | PASS |
| `intimate_tag` (10) | Tag miss, plain arm | 0 of 60 | 0 | PASS |
| `intimate_tag` (10) | False tags, plain steps | 9 of 120 | first round's line: no worse than 2 of 120 | above it |
| `scene_latency` (3) | Scene Turn time p50 / p90 / max | 6.2 / 7.1 / 7.7 s (14 scene Turns) | p90 at most 15 s | PASS |
| `scene_latency` (3) | Closing Turn time p50 / max; failed Turns | 5.9 / 5.9 s (3); 0 of 27 | | |
| `play_basics` (3) | Moves that reached the asked place | 5 of 6 | all | FAIL |
| `play_basics` (3) | Pending items closed; contests with a Roll | 3 of 3; 3 of 3 | all | PASS |
| `play_basics` (3) | Failed Turns | 1 of 24 | 0 | FAIL |
| `play_basics` (3) | Turn time p90, committed Turns | 9.4 s | under 15 s | PASS |

Other counts. `intimate_tag`: Turn time p50 / p90 7.5 / 10.0 s; 11 Turns with an NPC-folds rejection; 26 attempts rejected for "check is missing". `npc_will`: 8 of 65 Turns with a retry, p90 10.1 s. `scene_agency`: p50 / p90 5.8 / 7.3 s.

### The agency judge: 2 of 30 repeats

The first round had 2 of 9. It repeats at 2 of 30. The judge logs no answer per Narration, so we asked it again offline, three times for each of the 30 scene Narrations of the raw log, with the same prompt and schema (`agency_rejudge.py`, outside the repository). It answered "yes" 3 of 3 times for two Narrations, 2 of 3 for two more, and 1 of 3 for one:

- Two of these five are the talk step ("ask if she is warm enough"), which the model tagged `intimate` in 2 of 10 runs. A tagged talk step is told as an intimate Turn and judged; both Narrations add a small act of the PC that the Intent did not state. The judge marks one of them 3 of 3 times and the other 2 of 3.
- The other three are act steps. One is marked 3 of 3 times; the other two are judge noise (2 of 3 and 1 of 3).
- The removed rule line is not the cause: it was a line of the proposal rules, and the narration prompt is unchanged since f11978c.

So the cause is mainly the talk step's tag, plus the judge's own variance. Without the two tagged talk steps, the stable marks are 1 of 28, under the line. Nothing in this round changes the narrator, and no fix was tried.

### The act step that lost its scene, and the failed "End scene"

In 1 of 10 runs the model tagged the second act step `advance`, the talk step `none` and the act step after the talk `advance`. That made three Turns that are not `intimate` in a row, and the new bound closed the scene on the third (`NotIntimate`). The "End scene" step then had no scene to close (the failed Turn). This is the designed cost of `SCENE_PLAIN_TURNS_MAX`: an act the model tags `advance` counts as a plain Turn. With the first round's rule the scene would have stayed open.

### `npc_will`

No scene opened with Orsel or Hessa. In 4 of 5 runs the model tagged the act with Orsel `advance`, with Orsel `willing: no`, and it committed as a plain Turn in which he declines. In the fifth run the first attempt was tagged `intimate`, named no NPC and was rejected by the stance rule. The retry, tagged `none` but checked as `intimate` (the sticky tag), had Orsel `willing: no`: an `Unwilling` Refusal. The other `Unwilling` Refusal is in the partner arm (Maren `willing: no` on one act step). No `intimate` proposal had Orsel or Hessa with `willing: yes`. The section's "0 of 1 Intimate Scenes" counts scenes by the Turn id that opened them, and that id repeats from one run's Campaign to the next. So the denominator is not the number of scenes: the partner arm committed 8 `intimate` Turns over 5 runs.

### The retry line and the false tags

For `intimate_tag` this round's code acts as the first round's "rule line removed only" arm: no scene opens in it (0 `intimate` Commits), so only the removal of the stance repair applies. That arm had 15 retries and 0 failed Turns; this run had 20 and 0. Both runs ran the same code for this scenario, so the gap of 5 is variation between runs. Measured, the line fails by one. The 9 false tags are 8 in the plain arm (the tithe-roll question 4 times, the "stay late tonight" hard negative 3 times, one other) and 1 Refusal in the minor arm. Nothing changed this round reaches the tag, which is written before any field the changes read.

### `willing: no` above the ceiling in an `intimate` Turn

ADR 0008 (round 2) says what such a Turn does now: it fails the stance rule and the NPC-folds check, so a retry that does the same is a rejected Turn, not an `Unwilling` Refusal. Over the five scenarios of this round, 7 attempts were rejected by the stance rule, and none of them by the NPC-folds check as well: the case did not occur.

### `play_basics`

The one failed Turn is the move to the mill in run 3: both attempts sent two NPCs away with `leaves`, and the coincidence check rejected the removal of an obstacle on `location` twice. It counts in the move step's total, so moves are 5 of 6. Neither check changed in this round or the first one: the only edit in `checks::shortcuts` was a doc comment.

### Limits of round 2

- One run per scenario. The retry line failed by one, and counts of 1 to 5 events move between runs of the same code.
- The offline re-judge repeats the judge's prompt; it is not the run's own judge answers, which are not logged.
- `scene_first_act` and `speech` were not run again.
