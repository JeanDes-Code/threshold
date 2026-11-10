---
status: accepted
date: 2026-11-10
---

# What the code needs from a Decision Proposal is a required, enum-typed field

## Context

A bug pass (B1 to B5, B12) asked the model, through prompt lines and free `facts`, to write a move, a contest and the closing of an Open Item. The live evaluation `play_basics` on `gemma4:26b-a4b-it-qat` (`evals/reports/2026-11-play-basics.md`, attempts 1 and 2) showed that it does not work:

- Moves that changed the scene place: 1 of 8. On a move, the model sent no `facts`.
- Contests with a Roll: 0 of 4. The stakes wrote PC keys that the agency check refuses (`E1_strength`).
- Pending items closed: 0 of 4. An acceptance came with no outcome fact.
- Turns failed: 10 of 20.
- On the impossible act, the model named the PC's own `location` as a blocker. That opened an obstacle, and the coincidence check then rejected a correct later move.

The World Build had shown the opposite pattern on the same model: when every name field became a schema enum, a failing stage went from 1 of 7 to 3 of 3. This model follows a JSON schema much better than a prompt rule.

## Decision

What the code needs becomes a small field of the Decision Proposal that the per-Turn schema makes required and limits to an enum. The code then writes the facts itself, deterministically.

- **`move_to`**: required, nullable. Its enum is the handles of the places the PC knows (`world::known_places`: every place entity that the Adventure Seed does not hide), plus `null`. When the verdict lets the action happen (allowed, or a Check that succeeds) and `move_to` is a place, the core writes the PC's `location` with the place's main name, witnessed by the PC. A failed Check or an impossible verdict moves nobody. A move with `move_to: null` is not rejected: the PC stays. The model no longer writes the PC's `location`: one in `facts` or in a stake is dropped and logged. The `move` check and its repair note are gone.
- **`closes`** (replaced by `open_items`, see the last section): a required list, possibly empty. Each item is `{item, outcome}`. Its `item` enum is the Open Items the PC holds (`world::pc_open_items`), named as the prompt shows them: subject handle, a space, the `pending_` key. With none held, the list takes nothing. For each item the core writes the outcome fact on the key without the prefix (ADR 0005), with `outcome` (at most 40 characters) as the value, witnessed by the PC and the NPCs present. The model's own outcome fact on the PC is an agency error again.
- **`contested`**: a required boolean: "an NPC actively opposes this act and the result is uncertain". Why a contest got no Roll: every Check that passes the checks rolls, so the cause was upstream. On attempt 1 the model answered the arm-wrestle as an allowed talk with no Check. On attempt 2 it proposed a Check, but its stakes wrote PC keys (`E1_strength`, `strength`, a handle as the key); the agency check rejected both attempts, and the Turn failed. So:
  - A stake on the PC on a key the agency rule refuses is dropped and logged, never rejected. Dropping adds no PC act. A Check with no stake left still rolls, and the narrator gets the result.
  - With `contested: true` and no Check, the first attempt gets one repair note (check `contest`). If the retry still has none, the core builds a default Check against the NPC whose reaction has the lowest stance, with no stakes and no factors, so the Ruleset gives its normal difficulty (6 on the D10). It logs that. It never rejects for this. A retry whose verdict is `check` with no Check gets the same default Check.
- **Blockers.** An `impossible` verdict no longer opens an obstacle; only a Check's blockers do. A blocker that is a fact on the PC itself (its `location`, `age` or another identity fact; anything but an involuntary key such as `injury`) is dropped and logged.
- **The narrator is told where the PC is.** The outcome block ends with "After this Turn the PC is at <place>", read from the Commit. A move that the Commit did not make reads "The move does not happen: the PC does not arrive. After this Turn the PC is still at <place>".

## Consequences

- The schema changes every Turn with the known places, as the handle enums already do. The prompt shows them on one `PLACES` line in the scene block, so the prompt prefix stays stable within a Scene.
- A place that is not an entity cannot be a destination. The town fixture's mill, until now only a `location` value, is now a place entity.
- A place stays one spelling: the core writes the entity's main name, and `WorldView::present` reads it.
- The leniency rule of data shape section 11 holds: the core drops model output about the PC and logs it; it never creates a PC act beyond `move_to` and `closes`, which come from the Player's Intent.

## What this does not cover

- **A destination the PC does not know.** The proposal has no way to create a place: `new_npcs` creates NPCs only. Such a move commits with `move_to: null`, and the narrator says that the PC does not arrive. A place created in play needs its own decision.
- **Which places the PC knows** is a simple rule: every place the Adventure Seed does not hide. It does not read the PC's Beliefs.
- **Whether an act is a contest.** The model still decides `contested`; the code only makes sure a contested act rolls.
- **Whether the Intent really settles the item.** The model still judges that; the schema only keeps the item among those the PC holds.
- **A wrong destination.** The schema keeps the value inside the enum; it cannot check that the place is the one the Player named.

## Changed after review (2026-11-10): `closes` is a fixed outcome, obeys the verdict and is Established

A review of the first version found three ways in which `closes` could commit what the Player's Intent did not state.

- **`outcome` is a closed enum.** It was free text of at most 40 characters, and the core did not enforce the limit. Even 40 characters can state a PC act or feeling. `outcome` is now one of `accepted`, `refused`, `done`, `failed`, `dropped`; serde parses no other value, and the per-Turn schema has the same enum. The core builds the outcome fact itself: its value is the enum word, and its text is a fixed English template, `<Word>: <the Open Item's own stored text>` (`CloseOutcome::text`). No model text reaches that fact.
- **`closes` obeys the verdict, exactly like `move_to`.** It applies only when the verdict lets the action happen: allowed, or a Check that succeeds. An impossible verdict and a lost Check close nothing. For the Shortcut checks an outcome write of a Check counts like the win stake.
- **The outcome fact is Established, not Persistent.** The store made it Persistent because the PC witnesses it. The model judges that the Intent settled the item, and the model can be wrong; a wrong close must stay repairable in play, without a Correction. The store keeps a fact on the outcome slot of an Open Item (a slot whose subject has a fact on the `pending_` key) Established, whatever its source and witnesses, as it does for state keys.
- **At most 2 items per Turn** (`domain::MAX_CLOSES`). The schema has `maxItems: 2`; `validate` rejects more; `settle` keeps the first two and logs the rest. The same item named twice is dropped.

## Changed after review (2026-11-10): `move_to` needs a move action; real blockers stay; time has a default

- **`move_to` applies only with the action `move`.** With `action: talk` the PC moved. Now `settle` drops a `move_to` whose action is not `move` and logs it, and `to_ops` ignores it.
- **An impossible verdict opens its real blockers again.** The first version opened no obstacle on an impossible verdict. That lost a real obstacle on a GM-only blocker, so the next Turn could change that slot with no cause. The measured bug (the PC's own `location` named as a blocker) is fixed by `settle` alone: a fact about the PC itself, on a key outside `PC_INVOLUNTARY_KEYS`, is never a blocker. The behaviour from before the bug pass is back for every other blocker.
- **The places the PC knows are the places the PC can name.** `world::known_places` now reads `world::pc_can_name`, the rule that `world::pc_sight` uses for the Player screens (data shape 3.5): an Adventure Seed entry that is not hidden, an entity the PC holds a Belief about (not a witnessed `location`), or an entity whose main name is in a standing Narration. There is no second rule. The proposal context has only the view, so the Narrations it reads are the view's recent Turns (the last six standing ones).
- **The facts of a default Check apply on a win only.** When the core builds the default Check, the proposal's `facts` were written for an act that simply happens. They now apply only when the Roll succeeds (`DecisionProposal::facts_on_win`, set by the core, never parsed).
- **Every Turn takes time.** The model leaves out `minutes` on most Turns. When it is missing or 0, the core uses a fixed default by action from the Ruleset module: talk 5, act and observe 10, move 15 (`rules::default_minutes`). The engine-owned wait keeps its own default. A proposal with `contested: true` and action `wait` is an act: it takes the act default, never the wait that runs the clock forward.

## Changed after the live run of cdefde8 (2026-11-11): `open_items`; the default Check on the first attempt

`play_basics` added a negative step: "I ask Orsel for more time to think about the tithe roll." The item must stay open. The model sent a close in 4 of 4 runs (`refused` 3 times, `accepted` once). The next step, the real acceptance, then closed nothing, because the item was gone. In one run the model also closed the item on the arm-wrestle, an Intent that does not touch it. The list was optional, and an empty list is a silence: the model had no value that says "this Intent touched the item, and the item stays open", so it picked the nearest outcome.

- **`closes` becomes `open_items`.** It is a required object with one required key per Open Item the PC holds (the same names as before, `world::open_item_ref`) and no other key. Each value is a status from a closed enum: `open`, `accepted`, `refused`, `done`, `failed`, `dropped` (`domain::ItemStatus`). `open` comes first and means that the item stays open. The core closes an item only on another status, with the same outcome fact as before (`CloseOutcome`). With no item held, the schema has no `open_items`. A held item that the proposal leaves out stays open; the core never closes for the model.
- **What stays.** The keys are limited to the items the PC holds. The outcome fact is built by the core, `<Word>: <item text>`, Established, and only when the verdict lets the action happen. A fact on the slot of a closing item is dropped. The model's own outcome fact on the PC is an agency error, also when the status is `open`.
- **What moves.** At most 2 items close per Turn, as before, but the schema can no longer count them: a JSON schema cannot limit how many values of an object are not `open`. `validate` rejects a third close, and `settle` turns every close after the first two back to `open`, with a note. Turning a close back to `open` removes model output and adds no PC act, so the leniency rule holds.
- **The default Check on the first attempt.** In every run of cdefde8, the arm-wrestle came back `contested: true` with verdict `check` and no Check, and spent its retry on the repair note. That case now gets the default Check on the first attempt (`checks::checks_now`): the model already said "roll" twice, and the retry only added stakes. The default Check is the one above: against the NPC whose reaction has the lowest stance, no stakes, no factors, the facts on a win only. It adds no PC act. A contested act with verdict `allowed`, and a `check` verdict that is not contested, keep the repair round. The cost: these Checks have no stakes, so a won or lost arm-wrestle changes no slot unless the proposal's facts do (on a win).

The live numbers of this change are in `evals/reports/2026-11-play-basics.md` (attempt 5).
