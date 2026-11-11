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

## Changed in bug pass 3 (2026-11-11): `settles`, one choice among the items the Intent names

QA played 17 Turns on a Campaign built from a Source Text. The PC held 12 Open Items, several of them near-duplicates. `open_items` was then an object with 12 required status keys. The debug log of that Campaign shows:

- On "I accept. I will carry the winter post.", the model gave all 12 items a closing status. `settle` kept the first two in key order (`E1 pending_event`, `E1 pending_mystery`) and turned the rest back to `open`: two unrelated items closed, and the asked one stayed open.
- After "Correct last Turn", the same happened with three closing statuses: two unrelated items closed.
- The model then closed the asked item on later Turns, once on a Turn that only asked a distance. A clear refusal of the Guild's offer closed nothing.

The new scenario `open_items` (10 Open Items, two pairs of near-duplicates, items on absent NPCs) reproduced the late close on the code of d899a42: in 2 of 4 runs the acceptance closed nothing on its own Turn and closed on the next, unrelated Turn (`evals/reports/2026-11-open-items.md`, attempt 1). The model reads a status per item as "the state now", not as "what this Intent does".

- **`open_items` becomes `settles`.** One object `{item, outcome}`. `item` is `none` or one offered Open Item, named as before (`world::open_item_ref`). `outcome` is `open`, `accepted`, `refused`, `done`, `failed` or `dropped` (`domain::ItemStatus`). The model picks one name instead of filling a status for every item, so at most one item closes per Turn. `domain::MAX_CLOSES` is gone.
- **Only the items the Intent names are offered** (`world::items_named_by`). A content word of the Intent must match a word of the item's text, of its key or of its subject's name, through a short stem. Function words, words under 4 letters and the PC's own names match nothing. The offered items are listed with their text in the prompt's tail (`OPEN ITEMS (for settles)`), before the Intent, so the prompt prefix stays stable within a Scene. With none offered, the schema has no `settles`. On the QA Campaign's 17 Intents, the rule offers 0 to 6 items instead of 12.
- **What stays.** Only items the PC holds; the core writes the outcome fact `<Word>: <item text>`, Established, only when the verdict lets the action happen; a fact on the slot of the closing item is dropped; the model's own outcome fact on the PC is an agency error. Dropping a `settles` removes model output and adds no PC act, so the leniency rule holds.
- **The World Build merges a restated Open Item.** In the QA Campaign the World Build wrote 7 Open Items of the draft again, with the same sentence on a new key, hidden from the PC. The prompt then showed the GM an item the PC had settled as still open. `FactBook` now treats an open fact whose sentence (`normalize::value`) is the sentence of an accepted open fact as that fact: it adds its holders and no fact.
- **A Correction re-opens what its Turn closed.** This already held: the Correction reverts the Turn's outcome fact. A test now covers it (`a_correction_reopens_the_item_its_turn_closed`). The Journal's Decisions follow the closes (`world::sight::decision`), so they follow `settles`.

Live numbers: `evals/reports/2026-11-open-items.md` and `evals/reports/2026-11-play-basics.md` (attempt 6).

What this does not cover:

- **An Intent that settles an item without naming it** ("Yes." after an NPC's question) offers nothing, so the item stays open. The Player names it to close it. We chose a missed close over a wrong one.
- **Near-duplicates with different sentences** (the draft's summary "Osric Fell's daughter's wedding in the spring, and the boundary wall" on the PC, and "Osric Fell's daughter's wedding is planned for the first day of spring at Harrowmere" on Osric) are not merged: no deterministic rule tells them apart from two real matters. Settling one leaves the other open.
- **Whether the outcome word is right.** The model still picks `done` or `accepted`; the schema only bounds the choice.

## Changed in bug pass 3 (2026-11-11): what the narrator gets after a Check; which Turn is a wait

In the QA Campaign the arm-wrestle showed "Roll 3 vs 7: failure", but the Narration said "fifteen minutes pass in a slow, heavy silence… you remain seated by the fire", and the PC's `injury bruised_wrist` was not in the story. The narrator's outcome block for that Turn was: "The Check result: lose. The attempt fails.", the raw line "Teodor Brandt: injury = bruised_wrist", "15 minutes pass.", and "The move does not happen: the PC does not arrive". The model had labelled the act `move`, with `move_to: null` and `against: null`.

- **A contested move with no destination is an act**, like a contested wait. The narrator is no longer told that a move did not happen.
- **A contested Check names its opponent.** With no `against`, it opposes the NPC whose reaction has the lowest stance (`checks::oppose`). The Roll op carries `against`, and the narrator's line reads `- Check against <name>: the PC loses. Tell the attempt and how it fails. …` (or `wins`), naming the opponent only when the PC sees them.
- **A stake reads as a sentence** (`domain::slot_sentence`): "Teodor Brandt is injured (bruised wrist).", never `key = value`.
- **The time is no outcome item.** It was the first outcome line of nearly every Turn, and most Narrations opened with "N minutes pass". It is now one line after the outcome, `Time this Turn takes: N minutes.`, and the narrator rule says not to open with it.
- **A wait needs the Intent to ask for one** (`world::asks_to_wait`). "I nod to Sefa and warm my hands at the fire" came back as `wait` with no minutes, and the engine-owned wait took 60 minutes. Any other `wait` is an act, with the act default when the model gives no minutes.

Each change relabels or removes model output, or changes only the narrator's input: no PC act is added. Live numbers: `evals/reports/2026-11-play-basics.md`, attempt 7.

## Changed after review (2026-11-11)

A review of bug pass 3 found three blockers, three majors and a list of minors. Each change below removes model output, relabels it, or changes what a screen or the narrator reads. None adds a PC act, word or feeling: the core still writes only `move_to` and a closing `settles` from the Player's Intent.

- **A name in prose keeps its capitals; a hidden seed entry needs a Belief** (B1). The Opening Narration "A circle of lamplight lies on the gate square" named the hidden Adventure Seed Faction "The Circle": it got a card and a story link. A Narration now names an entity only with the name's own capitals (`normalize::name_in`): "the Circle" names it, "a circle" does not, and a leading article may be dropped or lower case. A hidden Adventure Seed entry is never named through a Narration word; it needs a PC Belief. `world::pc_can_name`, `world::known_places` (the `move_to` enum), the story links and a card's "named in" all use this rule. Speech tags keep the match without case.
- **The World Build merges a restated Open Item on its own subject only** (B2). Two NPCs' debts with one sentence were merged: one NPC lost her fact and became a holder of the other's. The review asked for subject, key and sentence. On the QA Campaign, the World Build restated 7 draft items: 6 on the same subject with a new key, 1 on another subject. Counted on its saved facts, a merge on the sentence alone merges 7, on subject, key and sentence 0, and on subject and sentence 6. The key is the model's label, so the merge needs the subject and the sentence, and the key may differ.
- **The outcome fact reads the text the PC holds** (B3). `<Word>: <item text>` read the live fact on the open key. A GM-only write can replace that fact, and the PC then held a text it never knew. The outcome fact and the offered items now read the PC's Belief (`world::pc_item_text`). With no text the PC holds, nothing closes and the item is not offered.
- **`settles` closes only an item this Turn offered** (M1). It was checked against the items the PC holds. With nothing offered the schema has no `settles`, but an answer that still carried one closed the item. The core sets `DecisionProposal::offered` from the Turn's offered items (never parsed); `settle` drops another item with one debug line, `validate` rejects it, and `to_ops` closes no other item.
- **Everyday words name no item** (M2). "meet", "plans", "time" or "night" offered items that only used the same word. The stop list of `world::items_named_by` gains everyday verbs and nouns of play. An NPC name still counts: "I tell Orsel yes." offers Orsel's request. The live `open_items` scenario gained two steps that name an NPC with items and settle nothing; the model chose `none` in 8 of 8 (`evals/reports/2026-11-open-items.md`, attempt 3). So a name-only match still offers.
- **A contest opposes the NPC the Intent names** (M3). `checks::oppose` took the reaction with the lowest stance: an NPC the Intent did not address, or a hidden NPC. The opponent is now, in order: the present NPC the PC perceives that the Intent names, when exactly one; else the only perceived NPC whose reaction is below neutral; else none. With none, `against` stays empty, the Check still rolls, and the narrator's line names nobody. `checks::default_check` uses the same rule. A `cause: check` stance of that NPC still passes the NPC-folds check only through the Roll, and a lost Roll clamps it.
- **Minors.**
  - `settle` runs before `engine_time`, so a contested move whose `move_to` `settle` drops is an act, and the narrator no longer hears "the move does not happen".
  - A wait needs a verb of waiting, sleeping or resting ("rest" as the verb, never "the rest of"), a phrase such as "to bed" or "keep watch", or a verb of staying with a duration ("I stay here for two hours"). "while" and "until" alone ask for nothing.
  - A Turn that closes an Open Item is a Decision whatever its action: Decisions follow `settles`.
  - The `OPEN ITEMS (for settles)` lines go through `context::fact_text`, so they carry the `(open, not done)` marker.
  - A slot whose value is "none", "no", "nothing" or empty reads as a plain no ("Ada is not injured.").
  - The Open Items of the side panel, the Character screen and the Journal list the items about an NPC present first, then the newest.

What this does not cover:

- **A hidden seed entry's name in a Narration** still reaches the Player as text, if the narrator writes it. It is no longer a link, a card or a People and places line.
- **A restatement on another subject** (1 of the 7 QA items) stays a GM-only Open Item. The proposal prompt can show it as open after the PC settled the first copy.
- **A word with a capital at the start of a sentence** ("Gate" as the first word of a sentence) still names the place Gate.

## Changed after the review of bug pass 4 (2026-11-11)

- **`settles` on an item whose outcome the world already wrote.** An event could fill an item's outcome slot without the PC learning it. The item now stays open for the PC and is offered (ADR 0005, one rule for "open"). When the PC settles it, the core does not overwrite the world's fact for those who know it: the outcome `<Word>: <item text>` becomes a Claim of the PC, told to the PC alone. It closes the item for the PC. The leniency rule holds: the Claim comes from the Player's Intent, through `settles`.
- **Decisions follow `settles` alone, or a Check whose stake changed a slot**, won or lost alike (data shape, section 13).
- **What the narrator gets of a witnessed event** changed too (data shape, section 13: a departure to a place the PC cannot name, the effects of a public event, a neutral line when every part of an event is guarded away). The first version read an effect from the event's text; the next section replaced that. None of it was measured on the live model.

## Changed after the second review of bug pass 4 (2026-11-11): `public` on a World Event

**The text rule is gone.** After the first review, the PC learned an effect of a witnessed event when the event's text held every content word of the effect's value (`world::arrival::states`). That is a bag of words, and no word rule can judge meaning: "Orsel paid no attention to the crowd" taught the PC `Orsel.debt_status = paid`, "Orsel has not paid the reeve" too, "says yes to another ale" taught `guilty = yes`, and "stares with dead eyes" taught `Pip.status = dead`. We removed it and everything that read it.

**What replaces it is this ADR's own rule:** what the code needs is a required, typed field the model fills, not a meaning the code guesses from prose. A World Event gets a required boolean `public` in both schemas that write events, the World Build's world events stage and a proposal's `schedule`: "everyone at the place sees or hears what this event changes (an announcement, a public act). False when only the people in seen_by notice it." The PC who witnesses a public event learns each effect on an entity it can name that is not hidden, and the outcome of an Open Item it holds; never a feeling or a choice of its own. A private event teaches only the perceptible effects (a move here, what happens to the PC on an involuntary key). Events stored without the field read as not public; the store keeps schema 3 and reads the flag from the event's ledger op.

**An arrival is perceptible.** An event whose effect moves one of its actors, not hidden, to the PC's place is witnessed: the wait stops at it, the PC sees the NPC come back, and the store retires the departure record at that write.

**One source for "the PC can name".** The store reads every standing Narration once per snapshot (`WorldView::story_names`); every rule that feeds the narrator or a screen reads it, instead of the last six Turns.

Each change removes model output, changes what the narrator or a screen reads, or adds a field the model fills. None adds a PC act, word or feeling. Details and costs: data shape, sections 13 and 14.

What we do not know yet: whether the model sets `public` sensibly. It is a new required field, untested on the live model. The live checks to run are in the data shape, section 14, and in the run report of this pass.

## Changed in bug pass 5 (2026-11-11): the Intent's duration owns a wait; each action has a longest time

QA pass 4 played a Campaign of 30 Turns. Its ledger shows: "I wait here for 30 minutes" took 60, 30, 60 and 60 minutes, because the model wrote `minutes: 60` and the wait kept it; "I take a room upstairs and sleep for eight hours" came as a `move` to the inn where the PC stood, with `minutes: 10`; a one-line talk came as `act` with `minutes: 90`; "I wait here for an hour" took 25 minutes because the model wrote 25, the time of the next World Event, and the story did not say that the wait ended early. "I walk over to Sefa Lind's corner" came as a `move` to the inn itself, and the narrator read "The move does not happen".

- **A wait Intent that states a duration takes it** (`world::intent_minutes`, `turn::engine_time`). A number or "a", "an", "half an" with minutes, hours or days, "an hour and a half", or "until" a named time of day (`rules::NAMED_TIMES`: dawn 06:00, morning 08:00, noon 12:00, dusk 18:00, nightfall 19:00, midnight 00:00) counted from the clock. When `world::asks_to_wait` holds, the Turn is not contested and does not move the PC, the Turn becomes a wait of those minutes, whatever the model's label or `minutes`. The wait still stops at a World Event the PC witnesses (`world::wait_minutes`), and the longest wait is 720 minutes (was 480), so a night's sleep fits. A wait with no stated duration keeps the model's minutes, as before.
- **The narrator hears a wait cut short.** When the wait took less than the Intent asked, the outcome block gets `- The wait ends early: N of the M minutes asked pass.` (`turn::wait_cut_line`). The stored proposal keeps `wait_asked`, so `renarrate` gives the same line.
- **Each action has a longest time** (`rules::max_minutes`): talk 15, act and observe 30, move 60. The model's `minutes` above it is cut to it, unless the Intent states a longer duration ("I work at the mill for four days"). The place data has no travel time yet, so no place raises the move's limit.
- **A move to the PC's own place is an act** (`world::is_pc_place`): `move_to` goes and the action becomes `act`, so the narrator no longer reads that the move did not happen.

Each change relabels the action or bounds the model's `minutes`; none adds a PC act. The minutes of a stated wait come from the Player's own words. Deterministic tests: `tests/time.rs`, and `a_stated_wait_takes_the_minutes_of_the_intent`, `a_stated_sleep_is_a_wait_whatever_the_model_labelled_it`, `the_model_minutes_are_capped_by_action`, `a_wait_cut_by_a_witnessed_event_tells_the_narrator`, `a_move_inside_the_current_place_is_an_act` in `tests/turn.rs`.

What this does not cover: a duration in words the parser does not know ("a couple of hours", "till the bells ring") leaves the length to the model, as before; "for days" with no number states nothing.

## Changed in bug pass 5 (2026-11-11): exits have a structured door

QA pass 4: the narrator wrote that Sefa Lind was out on the towpath, that Ludo Brenn strode to the door, and later that Sefa was "nowhere to be seen", while Here, the place card and both NPC cards kept them in the room, and later Narrations had Sefa asleep on a bench. The stored data shows two causes:

- **A World Event implied a move and wrote none.** The QA world's event "Sefa Lind walks the towpath, searching the mud for her lost earring" had one effect, `fatigue`, and no `location`. Sefa stood in the PC's room, so the PC witnessed the event and the narrator got its text. Replaying the QA World Build's `world_events` call 4 times on the live model: 0 of the 4 events whose text sends the actor elsewhere carried a location effect.
- **The narrator, or a reaction plan, made an NPC leave with no trace.** The proposal of "I tell Ludo his barge is loose" had no fact on Ludo; the narrator sent him out. The proposal had no field for it.

The decision, by this ADR's rule (a required, enum-typed field the model fills; the core writes the facts):

- **`leaves`**: a required list in the per-Turn schema, at most 2, possibly empty: `{npc, to}`. `npc` is an enum of the present NPCs the PC perceives (`checks::leavers`, `HandleContext::leavers`); `to` is an enum of every place handle (`HandleContext::destinations`) plus `away`. With no NPC here, the field is absent. The core writes the NPC's `location`: the place's main name, or no value for `away`; the text is "<name> left." or "<name> went to <place>." (`domain::location_sentence`). Everyone present witnesses it. When the PC cannot name the destination (`world::pc_can_name`), the PC gets the departure record of data shape 13 instead, as for a World Event; the store keeps a Possible fact a Turn writes Possible, and retires any departure record at the next canon write on its slot, whatever its source.
- **NPC acts apply whatever the verdict.** A leave is the GM's, not the PC's.
- **`settle`** drops a leave of an NPC the PC does not perceive here, of the PC, a second leave of one NPC, a `to` that is no place or `away`, and a leave to the NPC's own place; it drops a proposal fact on the `location` of an NPC who leaves (the core writes that slot). `validate` rejects the same shapes.
- **A departure is no cancel.** The tension check counted any write on the `location` of a scheduled event's actor as a cancel. In QA every NPC had a scheduled event, and the departures scenario's first run lost a Turn to it. A location the core writes from `leaves` or `moves_actor_to` is not checked by it: a scheduled event fires wherever its actor is.
- **World Events carry the actor's place.** The World Build's world events stage (`EventOut`) and a proposal's `schedule` (`EventDraft`) get `moves_actor_to`, required and nullable: a ROSTER place (a place handle in a proposal), `away`, or null. The setup (`setup::commit_world`) and `to_ops` add the actor's `location` effect, last, so it wins over another write of the slot. When the event fires, the existing perception rule tells the PC who leaves and who arrives.
- **The narrator's block states who is here.** The outcome block of THIS TURN ends with `- Here after this Turn besides the PC: <names>. Nobody else comes or goes.` (or `- Nobody else is here after this Turn.`), from `seen_present` after the Commit; a departure reads `- <name> leaves.` or `- <name> leaves for <place>.`, built from the slot. One narrator rule line says that only the people the outcome lists are present. The Present line of the scene block already came from `seen_present`, so a departed NPC is not written as present in later Narrations.

The leniency rule holds: `leaves` and `moves_actor_to` move NPCs only; the core removes model output and adds no PC act.

Live numbers (`evals/reports/2026-11-bug-pass-5.md`): on the departures scenario, Turns whose Narration told a departure that the world did not record went from 6 of 19 to 1 of 20; recorded departures from 3 to 6; no Turn failed. On the QA world's `world_events` call, 5 of the 6 events whose text clearly sends the actor elsewhere now carry a destination (0 of 4 before).

What this does not cover: the model still decides who leaves. It may leave a departing NPC out of `leaves` (1 Turn in 20), and it may send an NPC away because the Player's question presumed it ("I ask Orsel where Hessa went" while Hessa sat at her desk, 1 run in 4). An event's `moves_actor_to` also places an actor who does not move in its text ("Odile checks the chest" → `away`); that moves the actor to the event's place, which the text does not contradict.
