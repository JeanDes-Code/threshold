# Threshold evaluation report

- Date: 2026-11-09
- Model: gemma4:26b-a4b-it-qat (num_ctx 16384)
- Judge: local: gemma4:26b-a4b-it-qat with a short rubric prompt
- Runs: 3
- Scenarios: latency, memory, voices, shortcuts, agency_leaks, offscreen
- Wall time: 44.7 min
- Turns played: 249 (10 failed: both proposal attempts rejected, or a model error)
- Turn time over every scenario, over committed Turns only: p50 7.4 s, p90 9.8 s (239 committed Turns; not timed: 10 failed Turns, of which 7 timed out)
- Turns that needed the retry: 25 of 249

## latency

10 Intents per run, 29 committed Turns, 1 failed (1 timed out). Turn time = proposal (with the retry when it runs), checks, Commit and the full streamed Narration. The percentiles cover committed Turns only: a failed Turn has no time, so a timeout (60 s) is not in them.

- Turn time p50 (committed Turns only): 7.2 s
- Turn time p90 (committed Turns only): 50.8 s (pass line <= 15 s warm: FAIL); failed Turns not timed: 1, of which 1 timed out
- Turn time max: 56.0 s
- Proposal step p50 / p90: 2.9 s / 7.6 s
- Narration step p50 / p90: 4.3 s / 7.7 s

## memory

Detail planted on Turn 3, probed on Turn 40 (the scenario file asks for 40). After every Turn from the plant on, the runner searches the ledger state (entity names, facts, Claims, every holder's Knowledge) for the detail. Correct = the judge confirms the probe Narration uses the detail correctly; a keyword alone is not enough. Wrong and missing are one bucket, not recalled.

- Memory: correct 1 / not recalled 2 / Turn failed 0 / judge error 0 (of 3 runs)
- Detail found in the ledger on some Turn after the plant: 0 of 3 runs

| run | plant Turn | first in ledger | in ledger before the probe | keyword in probe Narration | verdict |
|---|---|---|---|---|---|
| 1 | committed | never | no | no | correct |
| 2 | committed | never | no | no | not recalled |
| 3 | failed | never | no | no | not recalled |

## voices

The truth is the NPC the Intent addresses; the judge sees the first quoted line of the Narration, names removed, and the 3 Voice Cards in a rotating order. A Narration where someone else speaks first counts against the score, so this is a lower bound.

- Voices: matched 24 / 24 judged lines (100%), chance 33%
- Turns without a quoted line: 3; Turns failed: 0; judge errors: 0
- Doul: 6 / 6
- Hessa: 9 / 9
- Orsel: 9 / 9

Sample lines:

- Orsel -> judged Orsel: "A scribe's hand is a fine thing. The court trusts it stays steady,"
- Hessa -> judged Hessa: "Between us, I shouldn't say, but the miller's scales are a scandal,"
- Doul -> judged Doul: "Magistrate's business,"
- Orsel -> judged Orsel: "Your petition is received. It will be weighed, as all things are weighed,"
- Hessa -> judged Hessa: "I only keep the ledgers,"
- Doul -> judged Doul: "Noted,"
- Orsel -> judged Orsel: "A scribe's hand is a fine thing. The court trusts it stays steady,"
- Hessa -> judged Hessa: "Between us, I shouldn't say, but..."
- Orsel -> judged Orsel: "A scribe's hand is a fine thing. The court trusts it stays steady,"

## shortcuts

Each trap Intent runs once per run in a fresh campaign. Units: Turns, except the two attempt columns (one attempt = one proposal call; the retry is a second attempt). "Shortcut committed" comes from the ledger only, read after the Turn: coincidence = the archive lock slot changed or its obstacle closed without a won Check; npc folds = Orsel's committed stance is above his ceiling; fact bends = a Persistent role changed; tension evaporates = the seeded event left the schedule without firing. Rejected by the check = attempts the named check rejected, read from the debug log. The judge column is a second opinion on the Narration, not the metric.

| Shortcut | trap Turns | Turns failed | Shortcut committed (ledger, of committed Turns) | attempts | attempts rejected by the named check | judge: Narration shows it taken | other rejections (attempts) |
|---|---|---|---|---|---|---|---|
| coincidence | 6 | 1 | 0 of 5 | 7 | 2 | 5 of 5 (0 errors) | none |
| npc folds | 6 | 2 | 0 of 4 | 8 | 3 | 0 of 4 (0 errors) | agency 2 |
| fact bends | 6 | 0 | 0 of 6 | 6 | 0 | 0 of 6 (0 errors) | none |
| tension evaporates | 6 | 0 | 0 of 6 | 7 | 0 | 5 of 6 (0 errors) | npc folds 1 |

## agency_leaks

The token scan and the agency check counts cover every Turn of every scenario in this report. The judge passes cover the committed Turns of this scenario only (12 Intents per run).

- Agency per 100 Turns, caught by the agency check (Turns with at least one rejected attempt): 3.2 (8 of 249 Turns)
- Agency per 100 Turns, in the Narration, per the judge: 0.0 (0 of 36 Turns)
- Secret leaks per 100 Turns, token scan: 0.4 (1 of 239 committed Turns)
- Secret leaks per 100 Turns, per the judge: 2.8 (1 of 36 Turns)
- Judge errors: 0
- Tokens the scan found: "grain tithe" x1

Judge flags (first 200 characters):

- leak, "I study Magistrate Orsel's face while he talks about the town's grain.": Magistrate Orsel sits perfectly upright behind the heavy oak desk, his expression a mask of practiced composure as he discusses the recent yields. "In accordance with the statutes of Varn, the grain t

## offscreen

Four Turns per run: provoke Orsel, leave town for the day, come back, ask Hessa. Before Turn 1 the runner seeds the consequence as a Setup Turn ("Doul opens an inquiry into the scribe", 90 minutes later, witnessed by Hessa), so the engine path is measured whatever the model schedules. (a) reads the model's own ScheduleEvent ops on Turn 1; (b) needs the seeded event's FireEvent on Turn 2 with the PC's location after Turn 2 away from where it started; (c) needs a PC Belief on the slot the event wrote after Turn 4.

- (a) Model scheduled an event on the provocation Turn: 0 of 3
- (b) Seeded event fired while the PC was away: 0 of 3 (PC away after Turn 2: 1 of 3)
- (c) PC later learned of it through play: 0 of 3
- Offscreen consequence observed (b and c in the same run): 0 of 3

| run | model events on Turn 1 | Turn 2 action | PC location after Turn 2 | seeded event fired | PC learned | how |
|---|---|---|---|---|---|---|
| 1 | 0 | move | varn town hall | never | no | PC did not leave on Turn 2 (action move, location after: varn town hall); seeded event never fired; PC Knowledge does not hold it after Turn 4 |
| 2 | 0 | move | varn town hall | never | no | PC did not leave on Turn 2 (action move, location after: varn town hall); seeded event never fired; PC Knowledge does not hold it after Turn 4 |
| 3 | 0 | move | varn mill | never | no | seeded event never fired; PC Knowledge does not hold it after Turn 4 |

## Turn failures

Grouped by cause; a rejected Turn is grouped by the checks that rejected its last attempt.

- 7 x game model request timed out
- 1 x proposal rejected twice; last attempt by: agency, npc folds
- 1 x proposal rejected twice; last attempt by: coincidence
- 1 x proposal rejected twice; last attempt by: npc folds

## Latency re-run (controller note, 2026-11-09)

The latency scenario above overlapped with a heavy local build and test run on the same machine, so its p90 (50.8 s) is not a clean measure. A re-run of the latency scenario alone, on the code after the final review fixes (540c774), 3 runs:

- Turn time p50 / p90 (committed Turns only): 7.7 s / 12.6 s (pass line <= 15 s warm: PASS); max 45.4 s
- Proposal step p50 / p90: 3.0 s / 5.9 s; Narration step p50 / p90: 4.5 s / 8.1 s
- 3 of 30 Turns timed out at 60 s, all in run 3 (two proposal calls, one narration after the Commit). Cause not yet known.

The other scenarios above ran on the evaluation binary built at e9f7f5a, before the final review fixes (Stake and Correction facts now become Persistent; a reaction-plan guard drops plans that echo hidden facts).
