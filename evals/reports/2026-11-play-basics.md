# Play basics: live measurement

- Date: 2026-11-10
- Model: `gemma4:26b-a4b-it-qat` (num_ctx 16384), Ollama 0.32.15, warm
- Judge: the same local model, with the runner's short rubric prompt
- Scenario: `evals/scenarios/play_basics.toml`, town fixture, 4 runs of 5 Turns
- Code: commit d3292d5 (B1 to B11) plus the commit that adds this report (the scenario, then the changes of attempt 2 below)
- Command: `threshold-eval --scenario play_basics --runs 4`

Each run is one fresh campaign. It plays, in order: an arm-wrestle with Doul (contest), flying through the roof (impossible), "yes" to Orsel's pending request (acceptance), a walk to the Varn mill (move), and a walk back to the Varn town hall (move). A failed Turn counts in its step's total and not in its success count.

These are measured values. They are bad, and they are reported as they are.

## Results

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
- **Truncated output.** Twice, a first proposal ended in the middle of the JSON (after `"factors":[` and after one fact). The cause was not looked at.

## Not measured

- A move or a contest that commits counts as a success in the table even when its Narration disagrees; only the impossible act has a judge.
- One fixture, one phrasing per step, and 4 runs: the counts are small.
