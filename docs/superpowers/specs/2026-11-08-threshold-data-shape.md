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
                    AdvanceClock, RecordRoll /* each with its payload */ }
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
    #[serde(default)] pub closes: Vec<Close>,          // max 2: Open Items the Intent settles; required in the schema (section 11)
    #[serde(default)] pub blockers: Vec<Handle>,       // F*, max 2: what stands in the PC's way
    pub check: Option<CheckDraft>,                      // present iff verdict == check
    #[serde(default)] pub reactions: Vec<Reaction>,    // max 2
    #[serde(default)] pub new_npcs: Vec<NewNpc>,       // max 1
    #[serde(default)] pub facts: Vec<NewFact>,         // max 2
    #[serde(default)] pub cancel: Vec<Cancel>,         // max 1
    pub schedule: Option<EventDraft>,
    #[serde(default)] pub minutes: u32,
    #[serde(default)] pub intimate: bool,
}
pub struct CheckDraft { against: Option<Handle>, win: Option<SlotDraft>, lose: Option<SlotDraft>, factors: Vec<Factor> } // factors max 3; both stakes required in the schema, optional for the core (section 11)
pub struct Close      { item: String /* "E3 pending_x", an Open Item the PC holds */, outcome: String /* <= 40 chars */ }
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
- **The PC may settle its own Open Item.** Superseded by section 11 (`closes`). A proposal fact on the PC on key `x` passes the agency check when the PC holds a live fact on `pending_x` about the PC (ADR 0005, same date). The PC's line in the proposal prompt lists those keys after `settles:`.
- **An opposed contest is a Check.** Superseded by section 11 (`contested`). One proposal rule line says that a physical or social contest against an NPC, whose result is uncertain, is a Check with `against` set to that NPC. No schema change.
- **The World Clock starts at the seed's hour.** `start.hour` (0 to 23, missing means 8) sets the clock of Day 1 through one `AdvanceClock` op in the seed Setup Turn, so a rebuild finds it again. A World Event `at` in a world file counts from that start. Old seeds and drafts without the field load.

## 11. Structured proposal fields (ADR 0007, 2026-11-10)

The live `play_basics` evaluation showed that the model does not follow the prompt rules of section 10 for a move, a contest or an Open Item. What the code needs is now a required, enum-typed field of the proposal, and the code writes the facts.

- **`move_to`.** Required and nullable in the per-Turn schema. Its enum is the handles of the places the PC knows (`world::known_places`: every place entity the Adventure Seed does not hide) and `null`. The scene block lists them on one `PLACES (for move_to)` line. When the verdict is allowed, or a Check succeeds, `to_ops` writes the PC's `location` with the place's main name, witnessed by the PC. An impossible verdict or a lost Check moves nobody. A move with `move_to: null` commits, and the PC stays. For the Shortcut checks this write counts like a proposal fact, or like the win stake on a Check. The `move` check and `checks::move_note` are gone.
- **`closes`.** A required list in the per-Turn schema, possibly empty, at most 2 items. Each item is `{item, outcome}`; the `item` enum is the Open Items the PC holds (`world::pc_open_items`), named `E<subject> pending_<x>` (`world::open_item_ref`); with none held, the list takes nothing. For each item, `to_ops` writes the fact on `x` with the `outcome` value, witnessed by everyone present, the PC included (ADR 0005). A proposal fact on the PC's outcome key is an agency error again; a fact on the slot of an item that `closes` names is dropped.
- **The narrator is told where the PC is.** The outcome block gets one line from the Commit: `context::PC_PLACE_LINE` and the place, or `context::NOT_MOVED_LINE` and the place when the action was a move and the Commit wrote no PC `location`. No narrator rule line is added.
- **Stakes are optional for the core.** `CheckDraft.win` and `lose` stay required in the schema. The core drops a stake on the PC on a key outside `PC_INVOLUNTARY_KEYS` (and a PC `location` stake), and a Check with no stake still rolls; the narrator gets the result line.
- **Blockers.** An `impossible` verdict opens no obstacle: the act does not happen, and an obstacle it left would make the coincidence check reject a later, legal act on the same slot. Within the Turn, the coincidence check still reads the declared blockers. A blocker that is a fact on the PC on a key outside `PC_INVOLUNTARY_KEYS` (the PC's `location`, `age` or another identity fact) is dropped; an injury or a condition may still block.
- **`contested`.** A required boolean: an NPC actively opposes this act and the result is uncertain. With `contested: true`, a verdict that is not `impossible` and no Check, the first attempt gets one repair note (`CheckKind::Contest`). If the retry still has none, `checks::default_check` builds one: `against` is the NPC whose reaction has the lowest stance, with no stakes and no factors, so the Ruleset gives its normal difficulty. The note goes to the debug log; the core never rejects for this. A retry whose verdict is `check` but has no Check gets the same default Check. The shape check (Check present iff verdict `check`) now runs with the Commit checks, after these repairs.

**The leniency rule** (one rule, applied by `checks::settle`):

- An element that only *removes* model output about the PC is dropped, and one line goes to the debug log (step `settle`). These are: a reaction of the PC, the PC in a scheduled event's `seen_by`, a scheduled event whose actor is the PC, a PC `location` in a fact or a stake, a `move_to` that is not a known place, a `closes` item that names no Open Item the PC holds (or names one twice), a fact on the slot that a `closes` item writes, a stake on the PC on a key the agency rule refuses, and a blocker that is a fact about the PC itself.
- The core never *creates* a PC act, word or feeling, except the fields that come from the Player's Intent: `move_to` and `closes`.
- Every other agency case still rejects. `validate` still rejects the dropped shapes when it gets them directly. The Player reads one plain sentence for a failed Turn (section 10).

