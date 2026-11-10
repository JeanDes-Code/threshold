# Play basics: live measurement

- Date: 2026-11-10
- Model: `gemma4:26b-a4b-it-qat` (num_ctx 16384), Ollama 0.32.15, warm
- Judge: the same local model, with the runner's short rubric prompt
- Scenario: `evals/scenarios/play_basics.toml`, town fixture, 4 runs of 5 Turns
- Code: commit d3292d5 (B1 to B11) plus the commit that adds this report (the scenario, then the changes of attempt 2 below)
- Command: `threshold-eval --scenario play_basics --runs 4`

Each run is one fresh campaign. It plays, in order: an arm-wrestle with Doul (contest), flying through the roof (impossible), "yes" to Orsel's pending request (acceptance), a walk to the Varn mill (move), and a walk back to the Varn town hall (move). A failed Turn counts in its step's total and not in its success count.

These are measured values. Attempts 1 and 2 were bad, and they are reported as they are. Attempt 3 measures the structured fields of ADR 0007 (section "Attempt 3" below).

## Results (attempts 1 and 2, before ADR 0007)

| Metric | Attempt 1 | Attempt 2 |
|---|---|---|
| Moves that changed the scene place | 0 / 8 | 1 / 8 |
| Impossible acts narrated as failure (judge) | 4 / 4 | 4 / 4 |
| Contests with a Roll | 0 / 4 | 0 / 4 |
| Pending items closed / acceptances | 0 / 4 | 0 / 4 |
| Turns failed | 12 / 20 | 10 / 20 |
| Turns that needed the retry | 14 / 20 | 12 / 20 |
| Turn time p50 (committed Turns) | 5.3 s | 4.8 s |
| Turn time p90 (committed Turns) | 9.2 s | 6.1 s |
| Turn time max (committed Turns) | 9.2 s | 7.1 s |

Failed Turns by the checks that rejected the last attempt:

- Attempt 1: move 9, agency and move 2, agency 1.
- Attempt 2: agency 4, move 4, agency and move 1, coincidence 1.

No Turn timed out. Wall time: 1.7 min of model time per attempt.

## What changed between the attempts

Attempt 1 ran the prompt and checks of B1 to B11. The raw proposals showed that the model leaves out `facts` on a move, and puts the outcome of the accepted request into a scheduled event's effects.

Attempt 2 changed three texts, nothing else:

- The `move` repair note (`checks::move_note`) now gives the exact fact to add, with the PC's handle.
- The proposal rule for a move now shows the fact as JSON.
- The proposal rule for an open item now says "in facts, never in a schedule" and shows the fact as JSON.

## What the raw proposals show (attempt 2)

- **Moves.** The model still often answers a move with no `facts`, on both attempts. Once it wrote the location as the effect of a scheduled event with the PC as actor; the core removed that event, and the retry wrote no location again.
- **A blocker on the PC's own location.** On the impossible act, the model cited the PC's `location` fact as the blocker. That opened an obstacle on the PC's location, and the coincidence check then rejected a correct move in a later Turn ("this write removes the obstacle location"). This is a new bug, not fixed here.
- **Contests.** The model now proposes a Check for the arm-wrestle. Its stakes write PC keys that the agency check refuses (`E1_strength`, `strength`, a handle as the key), so the Turn fails.
- **Acceptances.** The model answers with a talk and no fact on the outcome key. Once it put the fact in a schedule effect again.
- **Truncated output.** Twice, a first proposal ended in the middle of the JSON (after `"factors":[` and after one fact). The cause was not looked at. Attempt 3 found it: see "The cut JSON" below.

## Not measured

- A move or a contest that commits counts as a success in the table even when its Narration disagrees; only the impossible act has a judge.
- One fixture, one phrasing per step, and 4 runs: the counts are small.

## Attempt 3: the structured fields of ADR 0007

- Date: 2026-11-10, same model, Ollama and judge as above, warm
- Code: the core of commit d20ffcd (`move_to`, `closes`, `contested`, the dropped PC stakes and blockers); the scenario extension and the raw-log fix are in the commit that adds this section
- Command: `threshold-eval --scenario play_basics --runs 4 --raw-log <file outside the repository>`
- Scenario: the five steps above, then a sixth: "I cross the river and walk to the old watchtower on the far ridge.", a place that is not an entity. The Varn mill is now a place entity, so steps 4 and 5 are a move to a second known place and back. 4 runs of 6 Turns.
- Wall time: 3.2 min, judge included

| Metric | Target | Attempt 1 | Attempt 2 | Attempt 3 |
|---|---|---|---|---|
| Moves that changed the scene place | >= 7 / 8 | 0 / 8 | 1 / 8 | 8 / 8 |
| Impossible acts narrated as failure (judge) | 4 / 4 | 4 / 4 | 4 / 4 | 4 / 4 |
| Contests with a Roll | >= 3 / 4 | 0 / 4 | 0 / 4 | 4 / 4 |
| Pending items closed / acceptances | >= 3 / 4 | 0 / 4 | 0 / 4 | 4 / 4 |
| Unknown-place moves: PC stayed, Turn committed | (none) | not run | not run | 4 / 4 |
| Unknown-place moves told as not arriving (judge) | (none) | not run | not run | 4 / 4 |
| Turns failed | <= 2 / 20 | 12 / 20 | 10 / 20 | 0 / 24 |
| Turns that needed the retry | (none) | 14 / 20 | 12 / 20 | 3 / 24 |
| Turn time p50 (committed Turns) | (none) | 5.3 s | 4.8 s | 7.0 s |
| Turn time p90 (committed Turns) | <= 15 s | 9.2 s | 6.1 s | 10.0 s |
| Turn time max (committed Turns) | (none) | 9.2 s | 7.1 s | 12.0 s |

Every target is met on the first measured attempt, so no second attempt ran. Attempts 1 and 2 had 20 Turns; attempt 3 has 24, because of the sixth step. Attempts 1 and 2 timed only the 8 and 10 Turns that committed, so their times do not compare directly with attempt 3.

What the raw proposals show (attempt 3):

- **Moves.** Every known-place move named the right place in `move_to` (the mill, then the town hall), on the first attempt. The unknown-place move came back as `move_to: null`, with verdict `allowed` once and `impossible` three times; the PC stayed every time.
- **Contests.** The model set `contested: true` on all 4 arm-wrestles. Three first attempts had verdict `check` but no Check; the repair note made the retry add one. One first attempt had a full Check. The core's default Check was never needed. Two Checks had a stake on a PC key (`E1_reputation`, `stamina`); the core dropped it and the Check rolled.
- **Acceptances.** All 4 named `E1 pending_copy_tithe_roll` in `closes`, outcome `accepted`, on the first attempt.
- **Blockers.** On the impossible act, the model cited a fact about the PC itself as the blocker (`F1`, the PC's starting location) in 3 of 4 runs, and a fact about the PC again on the unknown-place move in 2 runs. The core dropped it each time; no later move was refused.

### The cut JSON (R6)

The two "truncated" proposals of attempt 2 were not cut. The model writes newlines inside its JSON: the grammar that Ollama builds from the schema allows whitespace between tokens, and this model puts a newline after an opening `[` and before its closing `]` (`"factors":[` newline `{"fact":"F3","helps":true}` newline `],...`). In attempt 3, 5 of 27 proposals did this, and all of them parsed. The old `THRESHOLD_EVAL_RAW` switch printed each raw proposal to the log line by line, so such a proposal read as one ending after `"factors":[` or after one fact. No limit in our code cut anything: we set no `num_predict`, `num_ctx` is 16384 for prompts of about 1,650 tokens and answers under 150 tokens, and a timeout gives an error, not a short answer. Twenty probe calls of the first proposals all stopped with `done_reason` `stop`. The raw log now writes one row per line, and the engine logs any `done_reason` other than `stop` with the raw proposal.

### Still wrong (attempt 3)

- The model labels the arm-wrestle as action `wait` in all 4 runs. The engine owns the length of a wait, so such a Turn can run the clock to the next World Event. The Roll happens; the action label is wrong.
- The model leaves out `minutes` on most Turns, so most Turns take 0 minutes, a move included.
- The unknown-place move: the model chose `impossible` three times out of four. The PC does not arrive, as wanted, but the narrator gets "the world prevents it" instead of "you do not find the way".
- One fixture, one phrasing per step and 4 runs: the counts are small. A move or a contest that commits counts as a success even when its Narration disagrees; only the impossible act and the unknown-place move have a judge.


## Attempt 4: the review fixes (2026-11-11)

- Code: commit cdefde8. The scenario has two negative steps per run: an Intent that asks for more time on the open item, and one that asks about the Varn mill without going there. A move counts only at the asked place.
- Command: `threshold-eval --scenario play_basics --runs 4 --raw-log <file outside the repository>`, 4 runs of 8 Turns, on a free machine.

| Measure | Result |
|---|---|
| Moves that reached the asked place | 7 / 8 |
| False closes on the negative step | 4 / 4 |
| False moves on the negative step | 0 / 4 |
| Turns with a clock advance | 32 / 32 |
| Impossible acts told as a failure (judge) | 4 / 4 |
| Contests with a Roll | 4 / 4 |
| Pending items closed | 0 / 4 |
| Unknown-place moves that left the PC in place | 4 / 4 |
| Turns failed | 0 / 32 |
| Turns that needed the retry | 7 / 32 |
| Turn time | p50 8.4 s, p90 11.9 s, max 13.7 s |

Finding: on "I ask Orsel for more time", the model closed the item in every run (`refused` 3 times, `accepted` once). The real acceptance one step later then had nothing to close. The optional `closes` list had no value for "the item stays open". The contest spent its retry in every run on "verdict is check but check is missing".

## Attempt 5: `open_items` and the default Check on the first attempt (2026-11-11)

- Code: commits 1b61692 and 7adfadf (ADR 0007, last section). Each Open Item the PC holds gets a required status, with `open` among the values. A contested act with verdict `check` and no Check gets the default Check at once.
- Same command, same machine, 4 runs of 8 Turns.

| Measure | Attempt 4 | Attempt 5 |
|---|---|---|
| Moves that reached the asked place | 7 / 8 | 8 / 8 |
| False closes on the negative step | 4 / 4 | 0 / 4 |
| False moves on the negative step | 0 / 4 | 0 / 4 |
| Turns with a clock advance | 32 / 32 | 32 / 32 |
| Impossible acts told as a failure (judge) | 4 / 4 | 4 / 4 |
| Contests with a Roll | 4 / 4 | 4 / 4 |
| Pending items closed | 0 / 4 | 4 / 4 |
| Unknown-place moves that left the PC in place | 4 / 4 | 4 / 4 |
| Turns failed | 0 / 32 | 0 / 32 |
| Turns that needed the retry | 7 / 32 | 1 / 32 |
| Turn time | p50 8.4 s, p90 11.9 s, max 13.7 s | p50 7.9 s, p90 9.9 s, max 10.2 s |

Limits:

- The raw log shows the status `done` more often than `accepted` on "I tell Orsel yes: I will copy the tithe roll". The item closes, but the outcome word says the copy is done when it is only promised. The report does not count the outcome word.
- A default Check has no stakes, so a won or lost arm-wrestle changes no slot unless the proposal's facts do.
- One fixture, one phrasing per step and 4 runs: the counts are small.
