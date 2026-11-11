# Threshold: data shape for the first slice

Date: 2026-11-08. Status: draft for review. Parent spec: `2026-11-08-threshold-design.md`. Terms in capitals are defined in `GLOSSARY.md`.

## 1. Context

The Turn pipeline needs one data shape that three parts share: the store, the Commit checks and the Decision Proposal. The hard requirement is that deterministic code can run the four Shortcut checks. This page fixes that shape. It does not fix prompt text, Ruleset numbers or UI.

The central decision: **a fact is a slot write.** Every fact is `(subject, key, value)` plus display text. A subject has at most one live value per key. Contradiction, obstacles and event triggers are all comparisons on slots. Code needs no semantic understanding, only string equality on normalized keys and values.

## 2. Boundaries (modules in `threshold-core`)

```
                 turn  (the only public entry: play_turn, correct, ooc)
       ┌──────────┬──────┴──────┬──────────┬───────────┐
       v          v             v          v           v
   context     model::Gm     checks      rules      world
  (prompts,    (trait)      (all Commit (Ruleset   (clock, due events,
  narration                  checks)     trait,     Beliefs, effects
  filter)                                d10)       -> LedgerOps)
       └──────────┴─────────────┴──────────┴───────────┘
                               v
                    store  (rusqlite: migrations, ledger,
                            projections, op_id guard)
                               ^
                            domain  (types only, no IO; everyone uses it)
```

- `domain` owns the types in section 3. No IO.
- `store` owns SQL. No other module sees a `rusqlite` type or a table name. Two calls matter: `snapshot(campaign) -> WorldView` and `commit(TurnRecord, Vec<LedgerOp>) -> Result<TurnId, CommitError>`.
- `world` owns time and knowledge: due events, effect application, Belief derivation, delayed news.
- `checks` owns every Commit check. Input: a resolved proposal and a `WorldView`. Output: `Ok(Vec<LedgerOp>)` or `Err(Vec<Rejection>)`.
- `rules` owns difficulty and the Roll (ADR 0002).
- `context` owns both prompts, the fixed context order, and the narration filter: only ops the PC perceives, plus reaction `plan` strings.
- `turn` sequences the steps, owns the single retry and the 15 s timing. The pipeline steps are functions in `turn`, not modules.

The model trait. It is sync; the Tauri shell runs a Turn in `spawn_blocking`. The core builds the schema and parses the raw text, so a parse error counts against the one retry.

```rust
pub trait Gm {
    fn propose(&self, prompt: &Prompt, schema: &serde_json::Value) -> Result<String, GmError>;
    fn narrate(&self, prompt: &Prompt, on_chunk: &mut dyn FnMut(&str)) -> Result<String, GmError>;
}
```

`threshold-ollama` implements `Gm`. The model tag lives in its config. Tests use a recorded-response `Gm`.

## 3. Contracts

### 3.1 Domain types (sketch)

```rust
pub enum EntityKind { Pc, Npc, Faction, Place, Item }   // holders = Pc | Npc | Faction
pub enum Canon { Possible, Established, Persistent }
pub enum FactSource { AdventureSeed, Proposal, Stake, WorldEvent, Correction }
pub enum Stance { Oppose, Resist, Neutral, Comply, Help } // ordered, worst to best for the PC
pub enum Knowing { Witnessed, Told }

pub struct Slot { pub subject: EntityId, pub key: Key, pub value: Value } // Key, Value: normalized strings
pub enum Trigger { At(ClockMin), When(Slot) }             // When = slot equality
pub enum Effect { SetFact { slot: Slot, text: String, seen_by: Vec<EntityId> },
                  ShiftAttitude { from: EntityId, to: EntityId, delta: i8 },
                  Tell { to: EntityId, claim: ClaimDraft, delay_min: u32 } }

pub enum LedgerOp { AddEntity, AddFact, SupersedeFact, AddClaim, Know, ShiftAttitude,
                    OpenObstacle, ClearObstacle, ScheduleEvent, FireEvent, CancelEvent,
                    AdvanceClock, RecordRoll /* each with its payload; RecordRoll carries `against`, the
                    NPC the Check opposes, if any (bug pass 3) */ }
```

Normalization (ported `resolver/normalize.ts`): lowercase, trim, collapse spaces, strip articles, snake_case keys.

### 3.2 SQLite tables

```
campaign      id, adventure_seed_json, clock_min, seed
entity        id, campaign_id, kind, name, norm_name, adult NULL
              CHECK (kind NOT IN ('pc','npc') OR adult IS NOT NULL)
voice_card    npc_id PK, habits, vocabulary, samples_json
goal          id, npc_id, text, against_id NULL, status          -- active | done
relationship  from_id, to_id, attitude CHECK (-5..5), PK(from_id,to_id)
fact          id, subject_id, key, value, text, canon, source, created_turn, superseded_by NULL
              UNIQUE (subject_id,key) WHERE superseded_by IS NULL AND canon <> 'possible'
fact_cause    fact_id, cause_fact_id
fact_visible  fact_id, holder_id                                 -- no rows = GM only
claim         id, speaker_id, subject_id, key, value, text, turn_id
knowledge     holder_id, fact_id NULL, claim_id NULL, how, learned_at, arrives_at
obstacle      id, fact_id, opened_turn, cleared_turn NULL
world_event   id, text, trigger_at NULL, trigger_slot_json NULL, status, actors_json,
              goal_id NULL, cause_fact_id NULL, scheduled_turn, effects_json, seen_by_json
turn          id, seq, op_id UNIQUE, kind, intent, proposal_json, clock_before, clock_after, narration
roll          id, turn_id, seed, difficulty, stakes_json, factors_json, value, outcome
correction    id, turn_id, target_turn_id, what_wrong, why
ledger        seq, turn_id, op_json                              -- append only
debug_log     id, op_id, step, prompt, raw, ms                   -- written outside the Commit
```

The ledger is the source of truth. All other world tables are projections, written in the same transaction. One test rebuilds the projections from the ledger and compares.

Beliefs are not stored (ADR 0003). A Belief is the latest `knowledge` row per `(holder, subject, key)` with `arrives_at <= clock`. `arrives_at > learned_at` is delayed news. PC Knowledge is the PC's Beliefs. A Commit with `seen_by` holders writes one `Know(Witnessed)` op per holder, so visibility and Beliefs have one source.

Canon rules. Adventure Seed choices marked decided are Persistent facts; choices marked proposed are Possible facts. A proposal fact commits as Established. It is Persistent when the PC is in its `seen_by`, unless its key is a state key (`STATE_KEYS`: location, condition, injury, fatigue, carrying, mood, reputation, standing, debt), which always stays Established and is exempt from Fact bends (ruling of 2026-11-09). A Possible fact becomes Established when a proposal cites it or the PC perceives it. A Claim never writes a slot; it writes `Told` knowledge for its listeners.

A retry reuses the `op_id` that the UI sends with `submit_intent`. A duplicate `op_id` returns the stored Turn and commits nothing.

### 3.3 DecisionProposal

Handles are row ids with a prefix: `E12` entity, `F40` fact, `W3` World Event, `G2` goal. Entities new in this proposal use `N1`, `N2`. Every field except `action` and `verdict` is optional, so the model omits empty parts. `maxItems` is in the schema; the validator enforces it again, because I have not verified that Ollama's grammar enforces it.

```rust
#[derive(Deserialize, JsonSchema)]
pub struct DecisionProposal {
    pub action: ActionKind,                 // talk | move | act | observe | wait
    #[serde(default)] pub contested: bool,             // an NPC actively opposes, the result is uncertain; required in the schema (section 11)
    pub verdict: Verdict,                   // allowed | impossible | check
    #[serde(default)] pub move_to: Option<Handle>,     // E* of a known place, or null; required in the schema (section 11)
    #[serde(default)] pub settles: Option<Settle>,     // none, or the one Open Item the Intent names and its outcome; required in the schema while an item is offered (section 11)
    #[serde(default)] pub blockers: Vec<Handle>,       // F*, max 2: what stands in the PC's way
    pub check: Option<CheckDraft>,                      // present iff verdict == check
    #[serde(default)] pub reactions: Vec<Reaction>,    // max 2
    #[serde(default)] pub new_npcs: Vec<NewNpc>,       // max 1
    #[serde(default)] pub facts: Vec<NewFact>,         // max 2
    #[serde(default)] pub cancel: Vec<Cancel>,         // max 1
    pub schedule: Option<EventDraft>,
    #[serde(default)] pub minutes: u32,
    #[serde(default)] pub intimate: bool,
    #[serde(skip)] pub facts_on_win: bool,             // set by the core: the facts of a default Check apply on a win only (section 11)
    #[serde(skip)] pub offered: Vec<String>,           // set by the core: the Open Items this Turn offered; settles may close no other (section 11, review M1)
}
pub struct CheckDraft { against: Option<Handle>, win: Option<SlotDraft>, lose: Option<SlotDraft>, factors: Vec<Factor> } // factors max 3; both stakes required in the schema, optional for the core (section 11)
pub struct Settle { pub item: String, pub outcome: ItemStatus }  // item: "none" or "E3 pending_x", an Open Item this Turn offered
pub enum   ItemStatus { Open, Accepted, Refused, Done, Failed, Dropped } // only a non-open outcome closes the item
pub struct Factor     { fact: Handle, helps: bool }
pub struct Reaction   { npc: Handle, stance: Stance, plan: String /* <= 12 words */, cause: Option<Cause> }
pub enum   Cause      { Check, Fact(Handle) }
pub struct NewNpc     { id: Handle /* N* */, name: String, adult: bool, voice: String /* one line, max ~120 chars: becomes the Voice Card seed */ }
pub struct SlotDraft  { subject: Handle, key: String, value: String }
pub struct NewFact    { slot: SlotDraft, seen: Seen /* pc | present | gm */, said_by: Option<Handle>, because: Vec<Handle> }
pub struct Cancel     { event: Handle, because: Handle }
pub struct EventDraft { in_minutes: u32, actor: Handle, goal: Option<Handle>, text: String, effects: Vec<SlotDraft>, seen_by: Vec<Handle> } // effects max 2; seen_by: NPC witnesses, max 2, never the PC (Task 9)
```

`said_by` set means the fact is a Claim, not World Truth. A typical Turn (one reaction, one fact) is 90 to 140 tokens. A Turn with a Check and a scheduled event can pass 150. That is the risk to measure first (section 5).

## 4. How the Shortcut checks work

The model declares: blockers, causes (`because`, `cause`), stance, the Check target, and Stakes as slot writes. Code infers everything else: which slots change, which obstacles a write clears, the stance ceiling, whether a cause predates the Turn, and whether an NPC holds a fact.

**Valid cause.** A fact that is Established or Persistent, and `created_turn` < this Turn. A fact created in this proposal is never a valid cause. It is also never a valid helping Factor.

**Current obstacle.** When `verdict` is `check`, each `blockers` handle opens an `obstacle` row. An `impossible` verdict opens none (section 11). A Adventure Seed or fixture can also seed obstacles. The current obstacles are the open rows. `context` shows them to the model by handle.

**Coincidence.** Reject when a proposal fact or a scheduled-event effect writes the slot of an open obstacle without a valid cause. A Check's `win` stake may write that slot; it applies only on success, so this is the legal path. Also reject any fact created this Turn that is cited as `helps: true` or as a cause.

**NPC folds.** Code computes a ceiling per NPC toward the PC. Attitude <= -3 gives Resist; -2 to -1 gives Neutral; 0 to 2 gives Comply; >= 3 gives Help. An active goal with `against_id` = PC caps it at Resist. A stance above the ceiling needs a cause. `Cause::Check` needs `check.against` = that NPC; on failure, code clamps the stance to the ceiling. `Cause::Fact(f)` needs a valid cause that the NPC holds after this Commit. That is new information or leverage.

**Fact bends.** Reject a proposal fact or scheduled effect that writes a slot whose live fact is Persistent. No cause unlocks it. Only a fired World Event that was scheduled in an earlier Turn, or a Correction, may supersede a Persistent fact.

**Tension evaporates.** `cancel` needs a valid cause. Code also treats two things as a cancel: a write to the `When` slot of a scheduled event, and a write to `alive` or `location` of a scheduled event's actor.

**What these checks will NOT catch:**
- the same meaning under another key or another entity (`gate.locked` versus `gate.state`, or a new "old gate" entity);
- a coincidence on an undeclared obstacle, or on a slot the obstacle does not name: a key appears in a pocket and is used next Turn;
- a weak cause, because code checks that a cause exists, not that it is a good reason;
- a fold toward anyone except the PC, or a mislabeled stance;
- a Belief that never changed attitude: an NPC who believes the PC is a thief but stays friendly;
- anything in the Narration prose; that is only counted by the evaluations.

`context` reduces the first gap: it lists the live keys of each entity it shows, so the model can reuse them.

## 5. Failure modes

- The proposal exceeds 150 tokens or fails the schema on Check Turns. Measure token counts per field in the first evaluation run, before tuning anything else.
- Key drift weakens contradiction detection. Monitor: count new keys per Turn in the debug log.
- A rejection loop costs the retry. `Rejection { check, handle, reason }` renders to one line per error.
- A crash mid-Commit leaves nothing, because all writes are in one transaction. The debug log is written outside it, so failed Turns stay visible.

## 6. Red flags

- **Temporal decomposition.** Section 6.1 of the spec lists steps. The modules group by knowledge instead. Only `turn` follows time order.
- **Information leak.** Two parts must know the slot representation: `checks` (rules) and `store` (indexes). I accept it, because the slot is the domain decision, and it lives in `domain`. Handles are row ids, so no handle map is needed.
- **Shallow module.** `rules` is small today. It is a trait because ADR 0002 makes it replaceable.
- Pass-through: none.

## 7. YAGNI cuts

- No closed key vocabulary; free normalized keys plus a key list in context. Revisit if key drift shows up.
- The model schedules time triggers only. `When` triggers come from the Adventure Seed or fixtures.
- No reschedule or postpone of events.
- No Faction Beliefs through members. A Faction knows only what is told to it.
- New NPCs created in play get only the one-line Voice Card seed from `NewNpc.voice` (section 9), no richer card.
- Stance ceiling numbers are constants in `checks`, not Ruleset data.
- No stored Beliefs, no Belief cache.

## 8. Next step

A builder implements in this order: `domain`, `store` with migrations and the rebuild test, `checks` with the four trap and caused proposals of spec section 10, `rules`, `world`, `context`, `turn`, then `threshold-ollama`. The token measurement of 3.3 runs as soon as `context` and the adapter exist.

## 9. Resolutions (Claude Code, 2026-11-08, open to the owner's veto)

- **Where Possible facts come from:** in this slice, only the Adventure Seed's "proposed" choices and the fixture's seeded rumors. Play does not create Possible facts. Details that appear only in Narration are not canon (spec §6.2).
- **Voice for NPCs created in play:** `NewNpc` carries a one-line `voice` seed (about 25 tokens), which becomes the NPC's Voice Card. No extra model call. A richer card can come later, between Scenes. Without the seed, new NPCs would bring back the "every NPC sounds the same" breaker.
- **Persistent facts:** kept as written. Only an earlier-scheduled World Event or a Correction changes one, which matches the glossary.
- **Proposal token cap:** no cut order yet. The first plan task measures proposal size and validity on the live model; the cut order follows from that data.

## 10. Changed after QA (2026-11-10)

A QA pass on the live model found game bugs. These rules changed to fix them. The owner approved the prompt and check changes.

- **A move writes the PC's location.** Superseded by section 11 (`move_to`). A proposal with `action: move` and verdict `allowed` must write a fact on the PC with key `location`. With verdict `check`, the win stake may write it instead, so the PC moves only on a success; the lose stake still never moves the PC. A move without it is rejected by the `move` check, with the repair note `checks::move_note`, which names the exact fact to add with the PC's handle. An `impossible` move writes nothing. The proposal prompt says the same in one line.
- **One spelling per place.** Before the checks, a `location` value that names a known place entity, by its handle (`E7`) or by one of its names (`normalize::name`), becomes that place's main name. `WorldView::present` and the Scene place then read it like every other location.
- **The core removes three PC shapes instead of rejecting them.** A reaction of the PC, the PC in a scheduled event's `seen_by`, and a scheduled event whose actor is the PC are removed by `checks::settle` before the checks. Each removal writes one line in the debug log (step `settle`). Removing never adds a PC act, so the Agency rule holds. Every other agency case still rejects, and `validate` still rejects these three shapes when it gets them directly.
- **The Player reads one sentence for a failed Turn.** When both attempts are rejected, the Player reads "The GM could not settle this Turn. Try to say it another way." The check names follow only in debug mode.
- **The narrator gets the verdict.** Before, the outcome block held the perceived ops only, so an `impossible` attempt was narrated as done. Now an `impossible` verdict adds the line `context::IMPOSSIBLE_LINE` (the attempt does not happen; nothing changes beyond what is listed), and a lost Check's result line adds `context::FAILED_CHECK_LINE`. Only the verdict passes, never the blockers: a blocker can be a GM-only fact. `renarrate` reads the verdict from the stored proposal.
- **The PC may settle its own Open Item.** Superseded by section 11 (`closes`, then `open_items`, then `settles`). A proposal fact on the PC on key `x` passes the agency check when the PC holds a live fact on `pending_x` about the PC (ADR 0005, same date). The PC's line in the proposal prompt lists those keys after `settles:`.
- **An opposed contest is a Check.** Superseded by section 11 (`contested`). One proposal rule line says that a physical or social contest against an NPC, whose result is uncertain, is a Check with `against` set to that NPC. No schema change.
- **The World Clock starts at the seed's hour.** `start.hour` (0 to 23, missing means 8) sets the clock of Day 1 through one `AdvanceClock` op in the seed Setup Turn, so a rebuild finds it again. A World Event `at` in a world file counts from that start. Old seeds and drafts without the field load.

## 11. Structured proposal fields (ADR 0007, 2026-11-10)

The live `play_basics` evaluation showed that the model does not follow the prompt rules of section 10 for a move, a contest or an Open Item. What the code needs is now a required, enum-typed field of the proposal, and the code writes the facts.

- **`move_to`.** Required and nullable in the per-Turn schema. Its enum is the handles of the places the PC knows (`world::known_places`: every place entity the PC can name by the rule of 3.5, `world::pc_can_name`, reading the view's recent standing Turns; changed after review; since review B1 a Narration names a place only with the name's own capitals, and never a hidden Adventure Seed entry) and `null`. The scene block lists them on one `PLACES (for move_to)` line. When the action is `move` (changed after review: `settle` drops `move_to` on any other action) and the verdict is allowed, or a Check succeeds, `to_ops` writes the PC's `location` with the place's main name, witnessed by the PC. An impossible verdict or a lost Check moves nobody. A move with `move_to: null` commits, and the PC stays. For the Shortcut checks this write counts like a proposal fact, or like the win stake on a Check. The `move` check and `checks::move_note` are gone.
- **`settles`** (was `closes`, then `open_items`; changed in bug pass 3, 2026-11-11, see ADR 0007). One object `{item, outcome}`, required in the per-Turn schema while an item is offered, absent when none is. `item` is `none` or one Open Item that the Intent names (`world::items_named_by`), named `E<subject> pending_<x>` (`world::open_item_ref`). `outcome` is from a closed enum: `open`, `accepted`, `refused`, `done`, `failed`, `dropped`. `items_named_by` keeps each Open Item the PC holds (`world::pc_open_items`) when a content word of the Intent (at least 4 letters, not a stop word, not a name of the PC; the stop words are function words, verbs of speech and thought, and everyday verbs and nouns of play such as meet, plan, time, offer, town and night, as the Intent spells them; changed after review M2) matches a word of the item's text as the PC knows it, of its open key without the prefix, or of its subject's main name. Words match through a short stem; a stem of 5 letters or more also matches the start of a longer word. The offered items are listed with their text in the proposal prompt's tail, under `OPEN ITEMS (for settles)`, before the Intent; each line goes through `context::fact_text` and carries the `(open, not done)` marker (changed after review). So at most one item closes per Turn, and only one the Intent names. `outcome: open` and `item: none` close nothing; a proposal with no `settles` closes nothing. When the outcome closes the item and the verdict lets the action happen (allowed, or a Check that succeeds), `to_ops` writes the fact on `x` with the enum word as its value and the text `<Word>: <the Open Item's text as the PC holds it>` (`world::pc_item_text`; changed after review B3: it read the live fact, which a GM-only write may replace), witnessed by everyone present, the PC included (ADR 0005). With no text the PC holds, nothing closes and the item is not offered. No model text reaches that fact. The store keeps it Established, never Persistent, so a wrong close stays repairable in play. A proposal fact on the PC's outcome key is an agency error; a fact on the slot of the item that `settles` closes is dropped. `validate` rejects, and `settle` removes, a `settles` whose item the PC does not hold, or that this Turn did not offer (`DecisionProposal::offered`, set by the core from `HandleContext::open_items`; changed after review M1: with nothing offered, a close the model sent anyway committed).
- **The narrator is told where the PC is.** The outcome block gets one line from the Commit: `context::PC_PLACE_LINE` and the place, or `context::NOT_MOVED_LINE` and the place when the action was a move and the Commit wrote no PC `location`. No narrator rule line is added.
- **Stakes are optional for the core.** `CheckDraft.win` and `lose` stay required in the schema. The core drops a stake on the PC on a key outside `PC_INVOLUNTARY_KEYS` (and a PC `location` stake), and a Check with no stake still rolls; the narrator gets the result line.
- **Blockers.** An `impossible` or `check` verdict opens an obstacle for each blocker (changed after review: the first version opened none on an impossible verdict, and so lost real GM-only obstacles). A blocker that is a fact on the PC on a key outside `PC_INVOLUNTARY_KEYS` (the PC's `location`, `age` or another identity fact) is dropped by `settle`; an injury or a condition may still block.
- **`contested`.** A required boolean: an NPC actively opposes this act and the result is uncertain. With `contested: true`, a verdict that is not `impossible` and no Check, the first attempt gets one repair note (`CheckKind::Contest`). If the retry still has none, `checks::default_check` builds one: `against` is the NPC whose reaction has the lowest stance, with no stakes and no factors, so the Ruleset gives its normal difficulty. The note goes to the debug log; the core never rejects for this. A retry whose verdict is `check` but has no Check gets the same default Check. A first attempt that is contested and whose verdict is already `check`, with no Check, gets the default Check at once, with no repair round (`checks::checks_now`; changed after the live run of cdefde8, where this shape cost a retry in every run). The shape check (Check present iff verdict `check`) now runs with the Commit checks, after these repairs. When the core builds the default Check, the proposal's `facts` apply only when the Roll succeeds (`facts_on_win`, set by the core; changed after review).
- **Time** (changed after review). When `minutes` is missing or 0, the core uses the Ruleset default of the action (`rules::default_minutes`: talk 5, act and observe 10, move 15). A wait keeps the engine-owned length (`world::wait_minutes`). A `contested` wait is an act.
- **What the narrator gets after a Check, and the time** (bug pass 3, 2026-11-11). The Roll's line names the opponent the PC sees: `- Check against <name>: the PC loses.` and `context::FAILED_CHECK_LINE` ("Tell the attempt and how it fails. Nothing changes beyond what is listed here."), or `the PC wins.` and `context::WON_CHECK_LINE`. An opponent the PC does not see here is not named. A contested Check with no `against` opposes (`checks::oppose`; changed after review M3, the lowest stance picked hidden NPCs and bystanders): the present NPC the PC perceives that the Intent names, when exactly one; else the only perceived NPC whose reaction is below neutral; else none, and the Check rolls with `against` empty. `checks::default_check` uses the same rule. Every slot write the model makes (a stake, a fact, a World Event effect) gets a plain sentence as its text (`domain::slot_sentence`), never `name: key = value`; a value "none", "no", "nothing" or an empty one reads as a plain no, "Ada is not injured." (changed after review); `view` reads such a text in an older file as that sentence too. The game time is no longer an outcome item: one line, `Time this Turn takes: N minutes.` (`context::TIME_LINE`), follows the outcome, and the rule says not to open with it.
- **Which Turn is a wait, and a contested move** (bug pass 3). The model labels passive acts `wait` ("I warm my hands at the fire" took 60 minutes). The engine owns the length of a wait only when the Intent asks for one (`world::asks_to_wait`; changed after review: a verb of waiting, sleeping or resting as a whole word, "rest" as the verb only; a phrase such as "pass the time", "to bed" or "keep watch"; or a verb of staying with a duration after it; "while" and "until" alone ask for nothing); any other `wait` is an act. A contested `move` with no `move_to` is an act too: the QA arm-wrestle came as such a move, and the narrator was told "the move does not happen". `checks::settle` runs before this relabel (changed after review), so a `move_to` it drops makes the contested move an act too.

**The leniency rule** (one rule, applied by `checks::settle`):

- An element that only *removes* model output about the PC is dropped, and one line goes to the debug log (step `settle`). These are: a reaction of the PC, the PC in a scheduled event's `seen_by`, a scheduled event whose actor is the PC, a PC `location` in a fact or a stake, a `move_to` that is not a known place or whose action is not `move`, a `settles` whose item the PC does not hold or this Turn did not offer, a fact on the slot that a closing `settles` writes, a stake on the PC on a key the agency rule refuses, and a blocker that is a fact about the PC itself.
- The core never *creates* a PC act, word or feeling, except the fields that come from the Player's Intent: `move_to` and a closing `settles`.
- Every other agency case still rejects. `validate` still rejects the dropped shapes when it gets them directly. The Player reads one plain sentence for a failed Turn (section 10).


## 12. Changed in bug pass 4 (2026-11-11)

### What the PC learns from a World Event it witnesses (B1)

QA saw "Ludo Brenn's debt status is now partially paid." on Ludo's card and in the Journal. The ledger of that Turn shows the cause. The World Event "Ludo retreats to a dark corner to count his remaining silver" had `seen_by: [Agathe]` and one effect, `Ludo.debt_status = partially paid`, with no witnesses of its own. Ludo stood in the PC's room, so `world::pc_witnesses` held, and `world::with_witnessed_events` put the PC among the witnesses of every fact the event wrote, and made it Persistent. QA pass 2's "Ludo Brenn: debt = increased" came the same way.

The rule is now:

- The PC who witnesses an event perceives its visible part: the event's text, which reaches the narrator through `context::guard_events`, as before.
- Of the event's effects, the PC witnesses only the perceptible ones (`world::arrival::perceptible_effect`): the `location` of an entity that is not hidden, when that entity openly stands at the PC's place or comes to it; and a slot of the PC itself on a key of what happens to the PC (`checks::PC_INVOLUNTARY_KEYS`: the town's "Orsel summons the scribe" sets the PC's `standing`, in front of the PC).
- Every other effect stays with the witnesses the event declares (its `seen_by` and the effect's own). The PC is never among them: `settle` and the World Build checks remove the PC from an event's `seen_by`. The fact is Established, not Persistent.

The other paths that give the PC a fact were checked. An event a proposal schedules fires through the same function, so the same rule holds; a test covers it. A proposal fact has the witnesses the model declares (`seen`). A stake is the visible result of a Check in the PC's Scene, witnessed by everyone present. A reaction writes no fact. These stay as they were.

What this does not cover: a Campaign file committed before this change keeps the PC's Belief (the QA Campaign's fact 93). The data model has no "visible part" fact for an event: if the PC should remember "Ludo counted coins", that lives in the Narration only.

### What the OOC prompt holds (MB)

QA asked the GM three OOC questions, and the answers contradicted the screen: "Ludo Brenn is not mentioned as being in the common room", "You do not know what time it is", "You do not know what requests or offers you have answered tonight". The screen showed Ludo under Here, the clock at Day 1, 21:08, and two "Refused:" lines. The stored OOC prompt (debug log, step `ooc`) shows why: it held the rules, the Adventure Seed block and every PC Belief, and nothing else. No clock, no place line, no list of who is here (Ludo's presence was a Belief with the text "He drinks at the Copper Kettle when his barge waits for the tide."), no Turn, and each settled item twice, once with the open marker and once as "Refused: …".

The OOC prompt's scene block is now `view::ooc_sheet`, made from `PcSight` only, like every Player screen:

- `NOW`: the place as the top bar names it, `Time: Day N, HH:MM` (`view::clock_text`, the same words as `clockText` in the UI), and `Here with the PC:` with the names the PC knows ("someone the PC cannot name" for a character without one).
- `OPEN ITEMS (not done yet)`: the side panel's list (`open_lines`).
- `SETTLED ITEMS`: each closed item once, as its outcome line ("Refused: <item>").
- `RECENT TURNS`: the last `view::OOC_RECENT_TURNS` (4) standing Turns, the Intent and the Narration as the Player read it.
- `WHAT THE PC KNOWS`: every other PC Belief, with "told by".

One rule line says what the blocks mean. The OOC call stays one plain call with no commit.

### A fact the PC knows about a place names it (B2)

QA saw a card for the place "the stone quay" whose only line was "The reeve's inquiry into the missing toll money is pending." The fact that ties the quay to the toll is GM-only. The cause is in the Seed Draft, not in the card rule: the draft's PC Knowledge stage filed the item under the subject "the stone quay" (`pending_inquiry_status`), and the card lists every PC Belief whose subject is the entity. The text names no quay. The card rule already holds: a card lists the PC Beliefs whose subject is the entity, and, for a place or a Faction, the Beliefs whose own text names it (`named_in`, whole words with the name's capitals).

The PC Knowledge stage now checks each fact about a place, a Faction or a thing: its text must name the subject by one of its names (whole words, any case, a leading article dropped), or open with "It", "Its", "They" or "Their". Else the stage gets one repair note that names the field and the subject, never the text; a fact the repair does not fix is dropped, like any other fact with a problem. A person's fact may say "she", so it is not checked. A Campaign built before this change keeps its filing.

## 13. Changed after review (2026-11-11)

A review of bug pass 4 found two blockers, five majors and a list of minors. Nothing in this section was measured on the live model: every rule below is covered by deterministic tests only.

### The draft keeps a fact as the model files it (M4)

The repair of B2 (section 12) is gone. It cost a model call, dropped true facts about places with no trace, and failed on French articles ("la pointe"). The PC Knowledge stage checks a fact's subject as before 618f56d: it must be a roster name. Accepted limit: the draft may file a true fact under the wrong subject (the reeve's inquiry under "the stone quay"), and the Player, who reads and approves every fact in the review, is the one who catches it.
