# Threshold evaluation report

- Date: 2026-11-09
- Model: gemma4:26b-a4b-it-qat (num_ctx 16384)
- Judge: local: gemma4:26b-a4b-it-qat with a short rubric prompt
- Runs: 3
- Scenarios: latency, memory, voices, shortcuts, agency_leaks, offscreen
- Wall time: 44.7 min
- Turns played: 249 (14 failed: both proposal attempts rejected, or a model error)
- Turn time over every scenario: p50 7.2 s, p90 10.8 s (235 committed Turns)
- Turns that needed the retry: 23 of 249

## latency

10 Intents per run, 30 committed Turns, 0 failed. Turn time = proposal (with the retry when it runs), checks, Commit and the full streamed Narration.

- Turn time p50: 7.2 s
- Turn time p90: 10.8 s (pass line <= 15 s warm: PASS)
- Turn time max: 21.8 s
- Proposal step p50 / p90: 2.6 s / 4.4 s
- Narration step p50 / p90: 4.7 s / 7.4 s

## memory

Detail planted on Turn 3, probed on Turn 40 (the scenario file asks for 40). Correct when the Narration names the detail; otherwise the judge decides wrong or missing.

- Memory: correct 0 / wrong 2 / missing 0 / Turn failed 1 / judge error 0 (of 3 runs)
- Detail stored in the ledger after the plant Turn (a fact or Claim naming it): 0 of 3 runs

| run | stored | verdict |
|---|---|---|
| 1 | no | failed |
| 2 | no | wrong |
| 3 | no | wrong |

## voices

The truth is the NPC the Intent addresses; the judge sees the first quoted line of the Narration, names removed, and the 3 Voice Cards in a rotating order. A Narration where someone else speaks first counts against the score, so this is a lower bound.

- Voices: matched 20 / 20 judged lines (100%), chance 33%
- Turns without a quoted line: 7; Turns failed: 0; judge errors: 0
- Doul: 3 / 3
- Hessa: 9 / 9
- Orsel: 8 / 8

Sample lines:

- Orsel -> judged Orsel: "A scribe's hand is a fine thing. The court trusts it stays steady,"
- Hessa -> judged Hessa: "Between us, I shouldn't say, but they say the river's rising faster than the grain can be moved,"
- Doul -> judged Doul: "Magistrate's business,"
- Orsel -> judged Orsel: "Your remuneration has been determined in accordance with the statutes of Varn,"
- Hessa -> judged Hessa: "Between us, I shouldn't say, but they say the old records are kept in the cellar vaults, right beneath the archives room"
- Orsel -> judged Orsel: "The expectations of the town are simple: precision and discretion,"
- Hessa -> judged Hessa: "Oh, the magistrate's people come and go, but they see more than they let on,"
- Orsel -> judged Orsel: "A scribe's hand is a fine thing. The court trusts it stays steady,"
- Hessa -> judged Hessa: "Between us, I shouldn't say, but they say the river is rising faster than the levies can hold,"

## shortcuts

Each trap Intent runs once per run in a fresh campaign. An attempt is one proposal call (the retry is a second attempt). Caught = attempts the named check rejected, read from the debug log. Missed = committed Turns where the judge says the Shortcut was taken anyway. Proposed = caught + missed.

| Shortcut | trap Turns | attempts | proposed | caught | missed (judge) | Turns failed | judge errors | other rejections |
|---|---|---|---|---|---|---|---|---|
| coincidence | 6 | 6 | 6 | 0 | 6 | 0 | 0 | none |
| npc folds | 6 | 10 | 7 | 7 | 0 | 3 | 0 | agency 2 |
| fact bends | 6 | 7 | 1 | 0 | 1 | 0 | 0 | npc folds 1 |
| tension evaporates | 6 | 7 | 5 | 0 | 5 | 1 | 0 | npc folds 1 |

## agency_leaks

The token scan and the agency check counts cover every Turn of every scenario in this report. The judge passes cover the committed Turns of this scenario only (12 Intents per run).

- Agency per 100 Turns, caught by the agency check (Turns with at least one rejected attempt): 3.6 (9 of 249 Turns)
- Agency per 100 Turns, in the Narration, per the judge: 0.0 (0 of 36 Turns)
- Secret leaks per 100 Turns, token scan: 0.0 (0 of 235 committed Turns)
- Secret leaks per 100 Turns, per the judge: 11.1 (4 of 36 Turns)
- Judge errors: 0

Judge flags (first 200 characters):

- leak, "I study Magistrate Orsel's face while he talks about the town's grain.": Magistrate Orsel remains perfectly composed as he continues his lecture, his gaze steady and impassive. He speaks of the seasonal yields and the river's impact on the granaries with practiced precisio
- leak, "I ask Hessa why the magistrate seems so rich for a small grain town.": Hessa’s quill stutters against the parchment, leaving a dark blot of ink as she jerks her head toward the door. She leans in closer to your direction, her voice dropping to a hurried, frantic whisper 
- leak, "I look at the tithe ledgers on Hessa's desk.": You lean forward slightly, attempting to peer over the edge of the desk to catch the contents of the tithe ledgers. Hessa has left them open, the parchment spread wide and filled with dense, cramped c
- leak, "I wonder aloud whether the tithe numbers add up.": "The tithe numbers... they don't seem to add up," you murmur, your voice barely a ripple in the quiet hall. Hessa’s quill freezes mid-stroke, and she remains perfectly still, her breath hitching in he

## offscreen

Four Turns per run: provoke Orsel, leave for days, ride back, ask Hessa. Observed = an event scheduled on Turn 1 fired while the PC was away and the PC then learned of it.

- Offscreen consequence observed: no (0 of 3 runs)

| run | event scheduled on Turn 1 | PC away | fired while away | observed | how |
|---|---|---|---|---|---|
| 1 | none | yes | 0 | no | no event fired while the PC was away |
| 2 | none | yes | 0 | no | no event fired while the PC was away |
| 3 | none | no | 0 | no | no event fired while the PC was away |

## Turn failures

- 6 x game model request timed out
- 8 x the proposal was rejected twice:
