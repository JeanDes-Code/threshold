# Threshold evaluation report

- Date: 2026-11-10
- Model: gemma4:26b-a4b-it-qat (num_ctx 16384)
- Judge: local: gemma4:26b-a4b-it-qat with a short rubric prompt
- Runs: 5
- Scenarios: world_build
- Wall time: 25.4 min
- Turns played: 0 (0 failed: both proposal attempts rejected, or a model error)
- Turn time over every scenario, over committed Turns only: p50 n/a, p90 n/a (0 committed Turns; not timed: 0 failed Turns, of which 0 timed out)
- Turns that needed the retry: 0 of 0

## world_build

Source Text: `fixtures/chronicle/source.md` (7874 characters), scored against `fixtures/chronicle/expected.toml`. Each run drafts a Seed Draft, confirms every proposed age, approves the draft when no gap is left, and builds the world; a run stops at the first step that does not succeed. Roster: an expected entry is found when one of its names, main or other, is a drafted name after `normalize::name`. Facts: an expected item is found when a fact with the same subject (through the main and other names) holds every word of one of the item's phrases in its key, value and text. The draft lines score the known facts of each draft; the world lines score the facts of each built world (known facts included) and cover only the runs that built one. Fact recall and open recall use the item's `about` phrase. False past counts the open items for which a settled fact (`open = false`) holds one of the item's `done` phrases: words that only a completed event would hold, so a settled fact about the plan itself does not count. An open item for which no such phrase exists is not scored for false past, and the false past lines leave it out of both terms. A paraphrase does not match, so every fact line is a lower bound. The run confirms every proposed age, so a proposed minor shows only on the line of characters drafted under 18. PC-centred share: the NPC goals and secrets whose text names the PC by one of its full names, or by the first word of its main name when no other drafted entry has that word in a name, as whole words (the build's own rule, `build::names_pc`, with the drafted roster); a secret is a fact that an NpcState call wrote as one. Voice distance: for each NPC the judge sees the first sample line of its Voice Card, names removed, and the habits and vocabulary of up to 4 Voice Cards, and picks the speaker. Times cover the drafts and builds that succeeded.

- Runs: 5; drafted 5, approvable 4, built 4, failed 0
- Roster recall: 70 of 70 expected entries over 5 drafts (100%)
- Wrong kinds: 0
- Extra entities: 33 (Harrowmere magistrate, Harrowmere market, Harrowmere quay, Sallow, book with a green cover, ferry house, ferry house floor, ferry landing, green cover book, iron chain, potter's wheel, river toll, sealed letter, stone quay, tin box, toll strongbox, towpath, village chest, wax seal)
- Missing entries: 0
- Ages that differ from a stated age: 0 of 25
- Child (stated age under 18) drafted under 18: 5 of 5 (Pim Quast)
- Characters drafted under 18 that the expected list does not hold as children: 3 (Sefa Lind)
- Start place right: 5 of 5 drafts; present characters found: 15 of 15
- Draft gaps before approval, by kind: slot_conflict 1
- Not approvable: 1 of 5 runs
- Runs with a built world: 4 of 5
- Fact recall, draft knowledge (settled items found): 65 of 80 over 5 drafts (81%); missed: Agathe Morrow: kettle, Gerrit Vane: ferry, Henrike Sall: kiln, Henrike Sall: loan, Odile Quast: missing, Osric Fell: ledger, Teodor Brandt: chain, Teodor Brandt: letter, Teodor Brandt: uncle
- Fact recall, built world (settled items found): 58 of 64 over 4 built worlds (91%); missed: Henrike Sall: loan, Osric Fell: ledger, Teodor Brandt: chain, Teodor Brandt: letter, Teodor Brandt: uncle
- False past, draft knowledge (open items stated as done): 0 of 30 over 5 drafts
- False past, built world (open items stated as done): 0 of 24 over 4 built worlds
- Open items not scored for false past: 1 (Teodor Brandt: letter)
- Open recall, draft knowledge (open items kept open): 21 of 35 over 5 drafts
- Open recall, built world (open items kept open): 21 of 28 over 4 built worlds
- PC-centred share (NPC goals and secrets that name the PC): 3 of 90 (3%) over 4 built worlds; highest world 5%; the build's limit is 50%
- World Events: 22 over 4 built worlds; with at least one effect: 22
- Voice distance: matched 25 / 32 judged sample lines (78%), chance 25%; judge errors 0
- Draft time p50 / max: 153.7 s / 186.4 s
- Build time p50 / max: 162.3 s / 192.4 s
- Model calls: draft 51, build 91; timeout retries 0; repair calls 20
- Failures (BuildFailure, by stage and check): none

Sample Voice Card lines:

- Agathe Morrow: "Sit yourself down, [name]. The hearth's got more life in it than you do after that [name]."
- Sefa Lind: "Got it. I've got it."
- Odile Quast: "The law does not care for excuses, only for the balance of the ledger."

| run | draft | gaps | build | roster | settled found (draft / world) | false past (draft / world) | open kept (draft / world) | PC-centred | events (with effect) | draft calls (repairs, retries) | build calls (repairs, retries) | draft time | build time |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | ok | 0 | ok | 14 of 14 | 14 of 16 / 15 of 16 | 0 of 6 / 0 of 6 | 6 of 7 / 6 of 7 | 1 of 24 | 6 (6) | 10 (0, 0) | 21 (3, 0) | 186.4 s | 162.3 s |
| 2 | ok | 0 | ok | 14 of 14 | 15 of 16 / 16 of 16 | 0 of 6 / 0 of 6 | 5 of 7 / 5 of 7 | 0 of 21 | 5 (5) | 10 (0, 0) | 23 (5, 0) | 153.7 s | 152.6 s |
| 3 | ok | 1 | not approvable | 14 of 14 | 12 of 16 / - | 0 of 6 / - | 1 of 7 / - | - | - | 11 (1, 0) | - | 173.7 s | n/a |
| 4 | ok | 0 | ok | 14 of 14 | 11 of 16 / 12 of 16 | 0 of 6 / 0 of 6 | 4 of 7 / 5 of 7 | 1 of 22 | 5 (5) | 10 (0, 0) | 24 (6, 0) | 145.6 s | 192.4 s |
| 5 | ok | 0 | ok | 14 of 14 | 13 of 16 / 15 of 16 | 0 of 6 / 0 of 6 | 5 of 7 / 5 of 7 | 1 of 23 | 6 (6) | 10 (0, 0) | 23 (5, 0) | 148.5 s | 184.6 s |

## Controller notes (2026-11-10)

- The run used the code at `559ed94`: the roster stage runs first, and every name field of a later stage is a schema enum of roster names. Before that change, the draft passed its roster stage 1 time in 7 on this fixture.
- Run 3 was not approvable: one `slot_conflict` gap was left. In the app the Player resolves that gap at the seed review; the evaluation does not edit a draft.
- Sefa Lind has no stated age in the source. The model proposed an age under 18 for her in 3 of 5 drafts. The app lists every proposed age for the Player to confirm; the evaluation confirms them all.
- The 33 extra entities are mostly objects and sub-places (a tin box, a wax seal, the ferry landing). They are not wrong, and they make the roster longer than the source needs.
- Open recall is the weakest figure: 21 of 28 open items stayed open in the built worlds. No open item was stated as done.
- `PC_CENTRED_MAX_SHARE` stays 0.5. The measured share is 3% on average and 5% at most.
- `SOURCE_TEXT_MAX_CHARS` goes from 60,000 to 40,000. One probe measured about 3,550 prompt tokens for 15,000 characters. No text near the limit has run on the live model.
- No call timed out with the 600 s limit for draft and build calls. A draft took about 2.5 minutes and a build about 2.7 minutes.
