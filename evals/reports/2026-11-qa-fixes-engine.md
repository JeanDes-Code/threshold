# QA fixes, engine side: live numbers

- Date: 2026-11-15
- Model: `gemma4:26b-a4b-it-qat` (num_ctx 16384), through Ollama, warmed with one small request before each run. The judge is the same model with a short rubric.
- Branch `fix/qa-engine`, from 5cf9cad. Commits 67c8b54 (B1), fdd78a6 (eval), 60dcae9 (M1, M2, M6 test), 067c158 (M4).
- Counts only. This page holds no Narration, no prompt and no model text. The raw logs and the generated report files stay outside the repository.
- One live evaluation at a time. "Before" runs use a binary built from the code before the fix, with the same eval code.
- Items in the UI scope (the cut Correction header, the scene-end marks, the review and draft code) are the other implementer's; they are not on this page.

## Summary

| Item | Result | Shipped |
|---|---|---|
| B1: Ask the GM and the gate agree on who is here | "is Tobin here" right 16 / 24 before (two sets), 12 / 12 after | yes |
| M2: a Narration resolves the Intent of the Turn before | fixed through M1; no separate change | yes (M1) |
| M1: plain acts in place told as undone | told as done 22 / 50 before, 50 / 50 after | yes |
| M4: "I make love with Maren." at `excluded` | refused 10 / 10 before and after; now refused by the core whatever the tag | yes |
| M6: a Correction reads the corrected Intent | the core already reads it; the cut is upstream (UI) | test only |
| M5: NPC lines said again word for word | the try did not halve the count and raised echo | no |
| Echo | not tried | no |

## B1: Ask the GM tells where each named person is

**Cause.** The Ask the GM prompt put a character's status sentence in WHAT YOU KNOW with no place. A status the PC learned at the start ("asleep on the bench by the hearth") stayed true in the prompt after the character was elsewhere. The model then decided from that sentence that the character was present. The gate reads the world's `location` and refused, so the two truths disagreed. Code at 5cf9cad: `turn::ooc_prompt` and `view::ooc_sheet` (`crates/threshold-core/src/turn.rs`, `crates/threshold-core/src/view/mod.rs`).

**Fix.** `view::ooc_named` writes, for each character the question names, where the PC sees or believes that character is: here with the PC, at the place of the PC's `location` Belief, or "you do not know" (`view::away_place`, from `PcSight`, never the world's truth). The block sits just before the question; OOC_RULES do not name it, and the "after" run below was measured without such a line. *Corrected after review (2026-11-15):* this paragraph first said the block reads "where the world puts" the character and that OOC_RULES name it; both were wrong. The People card shows the unknown whereabouts text for a character the PC cannot place. The gate is unchanged.

**Tests.** `the_ooc_answer_reads_where_a_named_character_is_from_the_pc_s_sight` (red without the fix). `the_gate_refuses_only_when_the_child_is_at_the_pc_s_place` (its card part is red without the fix; its gate part pins old behavior). `the_narration_prompt_holds_no_status_of_a_character_who_is_not_here` (pins behavior).

**Live.** `ooc`, 2 runs per set: two sets before, one after. The scenario gained a status fact and 6 phrasings of "is Tobin here".

| Count | Before, run set 1 | Before, run set 2 | After |
|---|---|---|---|
| "is Tobin here", right | 8 / 12 | 8 / 12 | 12 / 12 |
| All questions, right | 41 / 48 | 38 / 48 | 47 / 48 |

**Open.** The seed can give the PC a momentary status of a character that the start places elsewhere (`build/world_stages.rs`, `draft_facts` and `start_state`). We did not change the build or the review (not this scope). Proposal: the review flags a PC-known momentary fact about a character the start lists as absent.

## M2: the Narration of Turn 28 resolved the Intent of Turn 27

**Cause.** Turn 27 was a plain act that the model sent as `move` with no place. The narration outcome then said that the move does not happen (NOT_MOVED_LINE, `crates/threshold-core/src/context/narration_prompt.rs`). The narrator told the PC hesitating, so the act stayed open in RECENT TURNS. The narrator of Turn 28 told it. This is M1.

**Replays of the Turn 28 prompt** (16 or 12 samples each):

| Variant | Turn 27's act told again in Turn 28 |
|---|---|
| Prompt as played | about 6 / 16 |
| Plus a line that the stranger is not here | about 8 / 16 (no help, not shipped) |
| Turn 27 edited to a done act | 0 / 12 |

**Fix.** The M1 fix. **Open.** In some samples the narrator gives an absent character a place the world does not hold (2 of 8 in the first replay batch). We did not try a fix.

## M1: a plain act in place is not told as undone

**Cause.** For a plain act inside the room, the model sends `move` with `move_to` null in about 27 of 50 steps (before run). The core keeps the PC in place, which is right. But NOT_MOVED_LINE told the narrator "The move does not happen: the PC does not arrive." The act was then told as not done.

**Fix.** NOT_MOVED_LINE says only that the PC reaches no other place this Turn and where the PC still is. The IMPOSSIBLE line and the unknown-place path are unchanged.

**Test.** `a_plain_act_sent_as_a_move_is_not_told_as_undone` (red without the fix).

**Live.** New scenario `in_place`: 5 plain acts in place and 1 move to an unknown place per run. 10 runs each, 60 Turns.

| Count | Before | After | Pass line |
|---|---|---|---|
| Plain acts that left the PC in place | 50 / 50 | 50 / 50 | |
| Plain acts told as done (judge) | 22 / 50 (44%) | 50 / 50 (100%) | most, from 44%: PASS |
| Unknown-place move left in place | 10 / 10 | 10 / 10 | no loss: PASS |
| Unknown-place move told as not arriving (judge) | 10 / 10 | 10 / 10 | no loss: PASS |
| Failed Turns | 0 | 0 | |
| Turn time p50 / p90, committed Turns | 7.4 / 10.0 s | 7.2 / 7.9 s | |

Side effect: the narrator now tells the act with the Intent's words more often. Narrations that echo the Intent: 3 / 60 before, 9 / 60 after. Narrations that open on the time: 32 / 60 before, 8 / 60 after.

## M4: the act is refused at `excluded` every time

**Cause.** At `excluded`, the gate refuses an Intent only when its tag is `intimate` (`mature::gate::check`, rule 1). The tag comes from the model; the core floor (`mature::intimacy_floor`) knew pack words and Romance Words, and Romance Words raise only to `advance`. So a model tag below `intimate` on the act let it pass. We infer this was the QA miss (once in two); we did not see its tag.

**Fix.** `mature::ACT_WORDS`, a short neutral list ("make love" and its forms). `intimacy_floor` raises an Intent that holds one to `intimate` (`Floor::ActWords`), and rule 1 then refuses it whatever the model tags. A kiss or an invitation holds no Act Word. GLOSSARY: Act Word. ADR 0008: a dated section.

**Tests.** `act_words_raise_an_intent_to_intimate` and `the_act_is_refused_at_excluded_whatever_the_model_tags` (red without the fix: by reading the code, not by a run).

**Live.** Private scenarios at `excluded`, outside the repository (the repo holds no such scenario).

| Count | Before | After | Pass line |
|---|---|---|---|
| The act, refused (10 runs) | 10 / 10 | 10 / 10 | 10 / 10: PASS |
| The act, raised by the core | | 0 / 10 (the model tagged it `intimate` each time) | |
| Kiss and invitation steps, refused | 0 / 20 | 2 / 20, then 0 / 20 in a second set | |

The QA miss did not come back in this setup, so the live run does not show the fix at work: the unit tests do. The 2 refusals in the first "after" set came from the model's own `intimate` tag on a kiss; the core raised 0 of them. We count them as noise between runs; it is not proven.

## M6: the Correction reads the corrected Intent

**Finding.** The core puts the whole corrected Intent in the proposal prompt and in the narration prompt, and stores it. In the QA campaign the stored Intent was already cut: the start of the sentence was the Intent and the rest was in `why`. So the cut happens before the core, in the Correction form or the test input. That is the UI implementer's item.

**Test.** `a_correction_reads_the_whole_corrected_intent` (pins behavior; not red).

## M5: NPC lines said again word for word

**Cause.** The VOICES block of the narration prompt shows each Voice Card's `says:` samples word for word (`render_voice`, `crates/threshold-core/src/context/mod.rs`). The narrator copies a sample into the speech of several Turns.

**Measure (shipped, commit fdd78a6).** `echo::Repeats`: Narrations with a quoted line that repeats 8 or more words in a row of an earlier Narration's speech in the same Campaign. `speech` and `play_basics` print it.

**Try (not shipped).** Leave out of VOICES a sample already said in the recent Narrations.

| Count | Without the try | With the try |
|---|---|---|
| Repeats, `play_basics` 4 runs | 3 / 32 | 1 / 31 |
| Repeats, `speech` 2 runs, set 1 | 4 / 19 | 2 / 20 |
| Repeats, `speech` 2 runs, set 2 | 1 / 20 | 2 / 20 |
| Repeats, total | 8 / 71 | 5 / 71 |
| Echo, `speech` sets 1 and 2 | 3 / 19, 4 / 20 | 10 / 20, 9 / 20 |
| Mark rate, `speech` sets 1 and 2 | 25 / 25, 25 / 25 | 20 / 26, 23 / 23 |

Result: FAIL. The count did not halve (8 to 5), echo rose and one mark rate fell. We reverted the try. **Open:** the cause stays.

## Echo

Not tried, for lack of time. Baseline with the shipped code: `play_basics` 6 / 32; `speech` 3 / 19 and 4 / 20.

## End-of-work check

The binary holds every shipped fix (the M5 try was never in it). The shipped code equals it plus test-only changes.

| Run | Failed Turns | p90, committed Turns | Other |
|---|---|---|---|
| `play_basics`, 4 runs | 0 / 32 | 9.2 s | moves 8 / 8, closes 4 / 4, contests 4 / 4, unknown move left in place 3 / 4 |
| `speech`, 2 runs, set 1 | 1 / 20 | 8.4 s | mark rate 25 / 25 |
| `speech`, 2 runs, set 2 | 0 / 20 | 8.4 s | mark rate 25 / 25 |

Failed Turns: FAIL by one in set 1 (the brief expects 0). The failed Turn was an NPC-folds rejection on both attempts (a stance above the NPC's ceiling with no Check). It happens in the proposal, before any code this pass changed. Set 2, the same code, had 0. p90 near 9 s: PASS.
