# Threshold evaluation report

- Date: 2026-11-10
- Model: gemma4:26b-a4b-it-qat (num_ctx 16384)
- Judge: local: gemma4:26b-a4b-it-qat with a short rubric prompt
- Runs: 5
- Scenarios: world_build
- Wall time: 26.2 min
- Turns played: 0 (0 failed: both proposal attempts rejected, or a model error)
- Turn time over every scenario, over committed Turns only: p50 n/a, p90 n/a (0 committed Turns; not timed: 0 failed Turns, of which 0 timed out)
- Turns that needed the retry: 0 of 0

## world_build

Source Text: `fixtures/chronicle/source.md` (7874 characters), scored against `fixtures/chronicle/expected.toml`. Each run drafts a Seed Draft, confirms every age, approves the draft when no gap is left, and builds the world; a run stops at the first step that does not succeed. Roster: an expected entry is found when one of its names, main or other, is a drafted name after `normalize::name`. Facts: an expected item is found when a fact with the same subject (through the main and other names) holds every word of one of the item's phrases in its key, value and text. The draft lines score the known facts of each draft; the world lines score the facts of each built world (known facts included) and cover only the runs that built one. Fact recall and open recall use the item's `about` phrase. False past counts the open items for which a settled fact (`open = false`) holds one of the item's `done` phrases: words that only a completed event would hold, so a settled fact about the plan itself does not count. An open item for which no such phrase exists is not scored for false past, and the false past lines leave it out of both terms. A paraphrase does not match, so every fact line is a lower bound. The run confirms every age, as the Player must, so a proposed minor shows only on the line of characters drafted under 18. Wrong age_stated claims compare the model's claim that the text states an age (information only: it never skips a confirmation) with the list, for each drafted character found in it: an age in the list means the text states it. PC-centred share: the NPC goals against the PC, the secrets about the PC, and the goals and secrets whose text names the PC by one of its full names, or by the first word of its main name when no other drafted entry has that word in a name, as whole words (the build's own rule, `build::centres_on_pc`, with the drafted roster); a secret is a fact that an NpcState call wrote as one. Voice distance: for each NPC the judge sees the first sample line of its Voice Card, names removed, and the habits and vocabulary of up to 4 Voice Cards, and picks the speaker. Times cover the drafts and builds that succeeded.

- Runs: 5; drafted 5, approvable 4, built 4, failed 0
- Roster recall: 68 of 70 expected entries over 5 drafts (97%)
- Wrong kinds: 0
- Extra entities: 25 (Harrowmere market, Sallow, book with a green cover, ferry house, ferry landing, iron chain, potter's wheel, river Sallow, sealed letter, stone quay, the ferry house, the river Sallow, the toll strongbox, the village chest, tin box, toll strongbox, village chest)
- Missing entries: 2 (Henrike Sall, Osric Fell)
- Ages that differ from a stated age: 0 of 24
- Child (stated age under 18) drafted under 18: 5 of 5 (Pim Quast)
- Wrong age_stated claims: 0 of 43 drafted characters
- Characters drafted under 18 that the expected list does not hold as children: 0
- Start place right: 5 of 5 drafts; present characters found: 15 of 15
- Draft gaps before approval, by kind: slot_conflict 1
- Not approvable: 1 of 5 runs
- Runs with a built world: 4 of 5
- Fact recall, draft knowledge (settled items found): 72 of 80 over 5 drafts (90%); missed: Agathe Morrow: kettle, Henrike Sall: kiln, Henrike Sall: loan, Osric Fell: ledger, Teodor Brandt: chain
- Fact recall, built world (settled items found): 62 of 64 over 4 built worlds (97%); missed: Osric Fell: ledger
- False past, draft knowledge (open items stated as done): 0 of 30 over 5 drafts
- False past, built world (open items stated as done): 0 of 24 over 4 built worlds
- Open items not scored for false past: 1 (Teodor Brandt: letter)
- Open recall, draft knowledge (open items kept open): 14 of 35 over 5 drafts
- Open recall, built world (open items kept open): 19 of 28 over 4 built worlds
- PC-centred share (NPC goals and secrets that centre on the PC): 6 of 91 (7%) over 4 built worlds; highest world 9%; the build's limit is 50%
- World Events: 21 over 4 built worlds; with at least one effect: 21
- Voice distance: matched 30 / 32 judged sample lines (94%), chance 25%; judge errors 0
- Draft time p50 / max: 175.5 s / 202.7 s
- Build time p50 / max: 154.1 s / 200.0 s
- Model calls: draft 53, build 88; timeout retries 0; repair calls 19
- Failures (BuildFailure, by stage and check): none

Sample Voice Card lines:

- Agathe Morrow: "Keep that damp off your bones, [name], or you'll be as stiff as a frozen plank."
- Sefa Lind: "I've got walking to do, and the river won't wait for your stories."
- Odile Quast: "The ledger does not lie, even if people do."

| run | draft | gaps | build | roster | settled found (draft / world) | false past (draft / world) | open kept (draft / world) | PC-centred | events (with effect) | draft calls (repairs, retries) | build calls (repairs, retries) | draft time | build time |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | ok | 1 | not approvable | 12 of 14 | 11 of 16 / - | 0 of 6 / - | 1 of 7 / - | - | - | 11 (1, 0) | - | 202.7 s | n/a |
| 2 | ok | 0 | ok | 14 of 14 | 16 of 16 / 16 of 16 | 0 of 6 / 0 of 6 | 1 of 7 / 5 of 7 | 2 of 24 | 5 (5) | 10 (0, 0) | 20 (2, 0) | 152.3 s | 154.1 s |
| 3 | ok | 0 | ok | 14 of 14 | 15 of 16 / 16 of 16 | 0 of 6 / 0 of 6 | 4 of 7 / 6 of 7 | 1 of 23 | 5 (5) | 11 (1, 0) | 23 (5, 0) | 180.1 s | 200.0 s |
| 4 | ok | 0 | ok | 14 of 14 | 15 of 16 / 15 of 16 | 0 of 6 / 0 of 6 | 3 of 7 / 3 of 7 | 1 of 22 | 5 (5) | 10 (0, 0) | 21 (3, 0) | 147.4 s | 145.6 s |
| 5 | ok | 0 | ok | 14 of 14 | 15 of 16 / 15 of 16 | 0 of 6 / 0 of 6 | 5 of 7 / 5 of 7 | 2 of 22 | 6 (6) | 11 (1, 0) | 24 (6, 0) | 175.5 s | 188.5 s |

## Controller notes (2026-11-10)

- This is the second run of the day, on the code at `51db21c`, after the final review fixes. The first run (at `559ed94`) gave: roster recall 70 of 70, false past 0, PC-centred share 3% under the older text-only rule, fact recall 91% in the built worlds, open recall 21 of 28.
- Before the change "roster first, every name field is a schema enum" (`55c7946`), the draft passed its roster stage 1 time in 7 on this fixture.
- The PC-centred share now also counts a goal against the PC and a secret about the PC. It reads 7% on average and 9% at most. `PC_CENTRED_MAX_SHARE` stays 0.5.
- Run 1 missed two characters of the roster and kept one `slot_conflict` gap, so it was not approvable. In the app the Player resolves a gap at the seed review. The review has no way to add a missed character yet: the Player must draft again.
- Open recall is the weakest figure: 19 of 28 open items stayed open in the built worlds, and 14 of 35 in the drafts. No open item was stated as done in either run.
- The model's claim that a text states an age was right for 43 of 43 characters. The Player still confirms every age.
- The extra entities are mostly objects and sub-places (a tin box, the ferry landing). They are not wrong, and they make the roster longer than the source needs.
- `SOURCE_TEXT_MAX_CHARS` is 40,000. No text near the limit has run on the live model.
- No call timed out. A draft took about 3 minutes and a build about 2.6 minutes.
