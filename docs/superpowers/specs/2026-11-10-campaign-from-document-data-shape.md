# Campaign from a document: data shape

Date: 2026-11-10. Status: draft for review. Parent spec: `2026-11-10-campaign-from-document-design.md`. This page extends `2026-11-08-threshold-data-shape.md` and does not restate it. Terms in capitals are defined in `GLOSSARY.md`. Letters A to J name the calls that the design left to the architect.

## 1. Context

A Source Text becomes a Seed Draft, then an approved Adventure Seed, then a hidden world, then a Campaign file. This page fixes the types, the files and the module borders for that path. It fixes no prompt text.

The central decision: **the World Build returns data and writes nothing.** Its result is a `WorldSetup`, the same structure that `world.toml` holds today. One function turns a seed and a `WorldSetup` into a Campaign file. So a failed or cancelled build has no file to remove.

```
Source Text ─► build::draft_from_text ─► SeedDraft ─► drafts/<id>.json   (Player edits, approves)
                                                          │ approved
                              build::build_world ◄────────┘     (model calls, checks, no disk write)
                                     │ WorldSetup
                                     ▼
        setup::create_campaign_file ─► campaign-<n>.sqlite.part ─► rename ─► Engine::begin ─► Opening Turn
        (Setup Turns `__seed__` and `__world__`)
```

## 2. Boundaries (J)

`threshold-core` gains two modules. `fixture` becomes a thin TOML reader.

- `build` (new) owns the Seed Draft, its rules, the draft store, the stage runner, the stage prompts and the checks of design section 6.5. It calls the model through `Gm::propose` only. `RecordedGm` replays it. It does not know SQL.
- `setup` (new, split from `fixture.rs`) owns the map from names to `LedgerOp`s and the creation of a Campaign file. `fixture::load_world` becomes: read TOML into `WorldSetup`, then call `setup`. The two-file path and the build use the same code.
- `domain` gains `WorldSetup` and its entry types, moved from the private `WorldFile` of `fixture.rs`.
- `turn` gains `Engine::begin`. `context` gains `opening_prompt`.
- `src-tauri` owns the commands, the events and the paths `drafts/` and `campaigns/`. It owns the cancel flag, the one-job guard, the debug gate and the error text for the Player. It holds no build logic. A build does not hold the `Session` lock: it locks only to save the draft and to open the finished Campaign.

```rust
// build: the public surface
pub fn draft_from_text(text: &str, gm: &dyn Gm, ctl: &mut Control) -> Result<SeedDraft, BuildFailure>;
pub fn gaps(draft: &SeedDraft) -> Vec<Gap>;                      // derived, never stored
pub fn approve(draft: &mut SeedDraft) -> Result<(), Vec<Gap>>;
pub fn build_world(draft: &SeedDraft, gm: &dyn Gm, ctl: &mut Control) -> Result<WorldSetup, BuildFailure>;
pub struct Control<'a> { pub progress: &'a mut dyn FnMut(Progress), pub cancelled: &'a dyn Fn() -> bool,
                         pub log: &'a mut dyn FnMut(DebugEntry) }
pub struct Progress     { pub stage: Stage, pub done: u32, pub total: u32 }
pub struct BuildFailure { pub stage: Stage, pub check: BuildCheck }   // names only, safe for the Player
pub enum Stage { DraftSeed, DraftRoster, DraftKnowledge, HiddenFacts, NpcState, WorldEvents, StartState, Commit }
pub enum BuildCheck { Model, Schema, References, OnePc, Kinds, Ages, Completeness, Agency, SettledFacts,
                      OpenItems, WorldAutonomy, Secrecy, Cancelled, NotApproved }

// setup
pub fn commit_world(store: &mut Store, campaign: CampaignId, world: &WorldSetup) -> Result<(), SetupError>;
pub fn create_campaign_file(path: &Path, seed: &AdventureSeed, world: &WorldSetup, rng_seed: u64) -> Result<(), SetupError>;
```

`build_world` returns `NotApproved` for a draft that is not approved. `Gm` gets no new method.

## 3. Contracts

### 3.1 Adventure Seed (D)

```rust
pub struct AdventureSeed {
    // unchanged: premise, pc, entities, boundaries, decided, proposed, story_language
    #[serde(default)] pub themes: Vec<String>,            // max 8 items, 40 chars each
    #[serde(default)] pub gm_agreement: String,           // max 1,200 chars
    #[serde(default)] pub resolution: String,             // the resolution agreement, text
    #[serde(default)] pub min_age: Option<u16>,           // the Player's age constraint, checked by code
    #[serde(default)] pub start: StartSeed,
    #[serde(default)] pub open_details: Vec<OpenDetail>,
}
pub struct PcSeed     { name: String, age: u16, other_names: Vec<OtherName>, concept: String }
pub struct SeedEntity { name: String, kind: EntityKind, age: Option<u16>, other_names: Vec<OtherName>, hidden: bool }
pub struct OtherName  { name: String, known_to: Vec<String> }      // 3.3
pub struct StartSeed  { place: String, present: Vec<String>, situation: String, when: String, hour: Option<u8> /* 0..=23, added after QA */ }
pub struct OpenDetail { text: String, mode: OpenMode }             // Delegated | Discover | ToAgree
```

States: every field of an approved seed is decided. `proposed` keeps its meaning. `open_details` is the open state.

The stable block holds: premise, the PC line with age and other names, story language, `boundaries`, one `min_age` line that code writes, `themes`, `gm_agreement`. `build::approve` refuses a block above `ADVENTURE_BLOCK_MAX_CHARS` (first value 2,400, about 600 tokens). The builder measures the token count and corrects the value.

No play prompt reads `resolution`, `start` or `open_details`. `resolution` would give the model dice rules, and ADR 0002 gives those to the Ruleset. `start` is read once, by the build and by the opening prompt. Only the World Build reads `open_details`.

### 3.2 Age replaces `adult` (B)

```rust
pub const ADULT_AGE: u16 = 18;                                     // domain
pub fn is_adult(age: Option<u16>) -> bool;                         // domain; None is false
LedgerOp::AddEntity { local, kind, name, age: Option<u16>, #[serde(default)] other_names: Vec<String> }
pub struct NewNpc   { id: Handle, name: String, age: u16 /* schema 0..=999 */, voice: String }
pub struct Entity   { id, kind, name, norm_name, age: Option<u16>, other_names: Vec<String> }   // store
```

No type stores adult status. `domain::is_adult` is the only derivation, and `checks::boundary` is its only caller. The proposal prompt shows `age 34` on an entity line. A `NewNpc` below `min_age` is a `ContentBoundary` rejection.

Schema version 2. `entity` loses `adult` and gains `age` and `other_names_json`. `CHECK ((kind IN ('pc','npc')) = (age IS NOT NULL))`. `turn.kind` gains `opening`. `Store::open_existing` refuses a version 1 file with a clear message and runs no migration. The town fixture, the tests and the two probe examples move to `age`.

### 3.3 Names (C)

`normalize::name` stays the only normal form. One new function resolves a name, and every name match in the core goes through it:

```rust
impl WorldView { pub fn entity_by_name(&self, name: &str) -> Option<&Entity>; }   // main name or other name; lowest id on a tie
```

Today five places compare `norm_name` by hand: `fixture.rs:250`, `context/mod.rs:81`, `narration_prompt.rs:67`, `world/arrival.rs:54`, `store/mod.rs:343`. `build::gaps` reports a name that resolves to two roster entries.

"Who knows that two names are one person" is one fact per other name. Slot: `(entity, alias_<key of the other name>, <other name>)`. It is Persistent. Its holders are the bearer and each character in `known_to`. `setup` writes it in the `__seed__` Setup Turn. A draft lists only the other names that the PC knows, so the PC is always in `known_to` there. Stage 1 can add hidden holders (3.5). No check and no prompt enforces this fact in play.

### 3.4 Open items (A)

An open item is an Established fact about the thread, not a fact about its outcome. Its key has a reserved prefix. It is never a Canon Level and never a Possible fact.

```rust
pub const OPEN_KEY_PREFIX: &str = "pending_";                      // domain
pub fn is_open_key(key: &str) -> bool;                             // is_state_key is true for an open key
```

- `FactEntry.open = true` makes `setup` write the key `pending_<key>`. A settled entry with that prefix is refused.
- An open key is a state key. So the fact is never Persistent, and a later Turn can replace it.
- The outcome slot stays empty. So no check reads the plan as done.
- `context` puts the marker `(open, not done)` before the text of an open fact, in one function. The proposal prompt also shows the key. `JournalEntry` gains `open: bool`, and so does each fact of the `CharacterView` (`CharacterFact { text, open }`).
- Possible does not fit. A cited Possible fact becomes Established (`checks/ops.rs:218`). A Possible fact that the PC witnesses becomes Established (`store/project.rs:497`). Each is the false past that the design forbids.

A settled PC Knowledge fact commits as a `decided` fact does, with the PC as holder. The drafted flow leaves `decided` and `proposed` empty.

`commit_world` commits a `FactEntry` as Persistent only when the PC is among its holders. A hidden fact or a secret (a private feeling, a plan) commits as Established, so a play Turn can change it when the PC acts on it. State keys and open keys stay Established as before.

### 3.5 Seed Draft and draft store (E)

```rust
pub struct SeedDraft {
    pub id: String,                        // "draft-<nanos>", also the file stem
    pub state: DraftState,                 // Draft | Approved | Built
    pub source_text: String,
    pub seed: AdventureSeed,               // the roster is seed.pc and seed.entities
    pub knowledge: Vec<KnownFact>,         // PC Knowledge; the PC is the implied holder
    pub ages_to_confirm: Vec<String>,      // names whose age the Player has not confirmed; a new draft lists every character, the PC too
    pub ages_stated: Vec<String>,          // names whose age the model says the text states; information only, never skips a confirmation
    pub last_failure: Option<BuildFailure>,
    pub campaign_file: Option<String>,     // set when the state is Built
}
pub struct KnownFact { subject: String, key: String, value: String, text: String, open: bool }
pub struct Gap { kind: GapKind, at: String }   // Missing, AgeMissing, AgeUnconfirmed, AgeBelowMin, UnknownName,
                                               // DuplicateName, PcInRoster, WrongKind, SeedBlockTooLong

pub struct DraftStore { dir: PathBuf }     // open, list, load, save, delete, append_debug, read_debug
```

Files: `<data_dir>/drafts/<id>.json` and `<id>.debug.jsonl`. `save` writes a temporary file, then renames it. JSON, because a draft is one document and the seed is already JSON in the Campaign.

```
draft_from_text ─► Draft ── approve, no Gap ─► Approved ── build and file ok ─► Built
                     ▲                            │  ▲
                     └──── any save_draft ────────┘  └── build failed or cancelled: stays Approved, last_failure set
```

Only three states are stored. A stored "building" state would be false after a crash. "Failed" is `Approved` with `last_failure`. "Begun" is a fact of the Campaign: its Opening Turn exists.

*Changed after QA (2026-11-10):*

- **One truth for an age.** QA found Pim with age 12 on the entry, "11" on the known `age` fact, and "twelve" in its text. Now `confirm_age` and `rename` rewrite each known fact with key `age` about that entry (main or other name, through `normalize::name`): the value becomes the age, the text the plain sentence "<name> is <age> years old." `apply_edit` (save_draft) does the same for each fact whose value is not the entry's age. The entry's age is the truth.
- **The review edits a fact's sentence only.** Outside debug mode, the edit panel of a fact shows its sentence and the open box. The internal fields About, Detail and Value show in debug mode only.

### 3.6 World Build stages (F)

```rust
pub struct WorldSetup { facts: Vec<FactEntry>, attitudes: Vec<AttitudeEntry>, goals: Vec<GoalEntry>,
                        voice_cards: Vec<VoiceCardEntry>, obstacles: Vec<ObstacleEntry>, events: Vec<EventEntry> }
pub struct FactEntry { subject: String, key: String, value: String, text: String,
                       #[serde(default)] known_by: Vec<String>, #[serde(default)] open: bool }      // new section
pub struct NpcState  { goals: Vec<GoalEntry> /* 1..=3 */, secret: Option<FactEntry>,
                       attitudes: Vec<AttitudeEntry>, voice_card: VoiceCardEntry /* samples 2..=4 */ }
pub struct Placement { npc: String, place: String }
```

The other entry types keep the fields that `fixture.rs` has today. Entries name entities, never ids. `world.toml` gains `[[facts]]` at no cost.

| Stage | Calls | Output | Becomes, in the `__world__` Setup Turn |
|---|---|---|---|
| (code) | none | `knowledge` as `FactEntry`, `known_by` = PC | `AddFact`, `seen_by` = PC |
| Hidden facts | one per section | `Vec<FactEntry>`, `known_by` never the PC | `AddFact`, `seen_by` = holders |
| Hidden state per NPC | one per NPC | `NpcState` | `AddGoal`, `AddFact` (`seen_by` = that NPC), `ShiftAttitude`, `AddVoiceCard` |
| World Events | one | `Vec<EventEntry>`, 3 to 6 | `ScheduleEvent` with `Trigger::At` |
| Start state | one | `Vec<Placement>` for absent NPCs | `AddFact` on `location` |

Code writes the `location` of the PC and of each entity in `start.present`, with the PC as holder. An entity marked `hidden` gets no PC holder. A hidden fact with the slot and value of an accepted fact adds holders and adds no fact. The same slot with another value fails `SettledFacts`. A settled fact on `(s, k)` fails `OpenItems` when `(s, pending_k)` exists.

No new `LedgerOp`. Only `AddEntity` changes its payload. `FactSource::AdventureSeed` covers every build fact. The Campaign clock starts at 0, and `start.when` is display text. *Changed after QA (2026-11-10):* the clock starts at `start.hour` of Day 1 (missing means 8), through one `AdvanceClock` op in the seed Setup Turn. The draft seed stage reads the hour from the Source Text; the review shows it as a plain field. A World Event's `at` counts from that start.

### 3.7 Begin (G)

```rust
pub enum TurnKind { Setup, Play, Correction, Ooc, Opening }
pub const BEGIN_OP_ID: &str = "__begin__";
impl Engine { pub fn begin(&mut self, on_chunk: &mut dyn FnMut(&str)) -> Result<TurnOutcome, TurnError>; }
pub fn opening_prompt(view: &WorldView, packs: &[ContentPack]) -> Prompt;   // context
```

"Begin" is a Turn of kind `Opening` with zero ops and an empty Intent. Narration lives only on a `turn` row, so a step with no Turn could not survive a restart. `begin` fails when a Play Turn exists. A second call returns the stored Turn, or narrates it again when the Narration is missing. `opening_prompt` reuses the system, stable and scene blocks. Its last block holds `start.situation` and `start.when`. `Session::summary` must list `Opening` Turns.

### 3.8 Commands and events (H)

| Command | Arguments | Returns |
|---|---|---|
| `draft_from_text` | `text` | `DraftView` |
| `list_drafts` | none | `Vec<DraftLine { draft_id, title, state }>` |
| `get_draft` | `draft_id` | `DraftView` |
| `save_draft` | `draft_id`, `seed`, `knowledge`, `ages_to_confirm`, `ages_stated` | `DraftView`, state back to `draft` |
| `approve_draft` | `draft_id` | `DraftView`, or the gaps as an error |
| `start_build` | `draft_id` | `CampaignSummary` of the opened Campaign |
| `cancel_build` | none | nothing; sets a flag and takes no lock |
| `begin` | none | `TurnSummary` |
| `delete_draft` | `draft_id` | nothing |
| `get_build_debug` | `draft_id` | `Vec<DebugLine>`; `DebugOff` unless debug is on |

`DraftView { draft_id, state, seed, knowledge, ages_to_confirm, ages_stated, gaps, last_failure }`. `CampaignSummary` gains `needs_begin: bool`.

One new event: `build_progress { stage, done, total }`. `draft_from_text` and `start_build` emit it. `begin` reuses `narration_chunk` (with `op_id` `__begin__`), `turn_committed` and `turn_failed`. A failure is the command error: the stage name and the check name, nothing else. `save_draft` replaces the whole document. `cancel_build` also stops a running draft.

### 3.9 Debug records (I)

The `debug_log` table is inside the Campaign file. That file does not exist during a build or after a failed one. So `Control.log` appends each call to `drafts/<id>.debug.jsonl`: `DebugEntry { op_id: draft id, step: stage and item, prompt, raw, ms }`. A new run of the same stage group replaces the file. `get_build_debug` is the only reader. `delete_draft` removes the file.

## 4. Failure modes

- **Crash during a build.** No Campaign file exists. The draft on disk is still `Approved`.
- **Crash between the two Setup Turns.** The file has the suffix `.part`. `list_campaigns` never shows it. The app deletes `.part` files at start.
- **Cancel is slow.** A `Gm` call cannot stop. Cancel acts between two calls, so the wait can be one full call.
- **Key drift.** `OpenItems` and `SettledFacts` compare normalized keys. The same meaning under another key passes. The false-past metric measures it.
- **The draft save fails after the Campaign file exists.** A retry makes a second Campaign. Accepted: the Player sees both in the list.
- **Secrets on disk.** The debug file holds hidden content in plain text, as `debug_log` does. Only the debug view reads it.
- **A model names the PC as holder of a hidden fact.** `Secrecy` fails, one repair runs, then the fact is dropped.

## 5. YAGNI cuts

- No `open` column and no fourth Canon Level. The key prefix carries the status.
- No `FactSource::WorldBuild`. `created_turn` tells a build fact from a seed fact.
- No stored link from a World Event to its thread, goal or secret. The prompt asks for it and the evaluation measures it.
- No obstacle from the build. No `When` trigger from the build.
- No change of an age after creation. No ageing with the World Clock.
- No model for two roster entries that are one person. They stay two entities.
- No code that closes an open item in play. A Turn replaces the fact as it replaces any state.
- No "building" or "begun" draft state. No terminal build event: the command result is the end.
- No Content Pack fragment in a build prompt. No new `Gm` method. No embeddings.
- No unique-name rule for a `NewNpc` in play. Today nothing checks it, and this slice does not change that.

## 6. ADR to write: open items are facts on `pending_` keys

We store an open item of a Source Text (planned, proposed, pending) as an Established fact about the thread. Its key has the prefix `pending_`. Its outcome slot stays empty. We rejected a Possible fact on the outcome slot: a cited or witnessed Possible fact becomes Established, which makes the plan a past event. We rejected an `open` column on `fact`: every check, every prompt line and the live-slot index would need the new rule. The prefix needs one function in `domain` and one marker in `context`. The cost: a status lives in a string, and the link between `pending_k` and `k` depends on key agreement. This is hard to reverse once ledgers hold such keys.

## 7. Next step

Structure first, behaviour second. One commit per step.

1. Age, other names, `entity_by_name`, schema 2, fixtures and tests. No new behaviour.
2. `WorldSetup` in `domain`, the `setup` module, `create_campaign_file`. The two-file path uses it.
3. New seed fields, the stable block and its cap, open keys and the marker.
4. `build`: Seed Draft, `gaps`, `approve`, `DraftStore`.
5. `build_world`: the stages and each check of design section 6.5, with `RecordedGm`.
6. `TurnKind::Opening` and `Engine::begin`.
7. `src-tauri` commands and events, then the UI, then the fixture chronicle and the evaluations.

A review of this shape before the plan is useful. No reviewer has read it yet.

## Design red flags checked

- **Shallow module:** none. `build` has four functions over the stages, the repair rule and the checks. `setup` has two over name resolution, key and holder rules, and the file swap.
- **Information leak:** two, accepted. `domain` owns the `pending_` prefix; `store` and `context` call its function. Name resolution was in five places and is now one function.
- **Temporal decomposition:** avoided. Draft and World Build run at different times and share one module, because they share the roster rules and the stage runner.
- **Pass-through:** `fixture::load_world` stays as a two-line reader. It adds the TOML format, so it stays.
- **Complexity alarm:** the work touches more than eight files, most of them for `age`. Step 1 isolates that change. New modules: two.

## Left to the builder

- Prompt text, schema limits, the number of calls and the section size for each stage.
- The measured value of `ADVENTURE_BLOCK_MAX_CHARS` and the size limit of the Source Text.
- The threshold of the World autonomy check, and how a text is found to name the PC.
- The exact `Gap` kinds and which gaps are essential.
- The marker wording, and the Journal display of an open fact.
- The SQL for schema 2 and the message for a version 1 file.
- The fixture chronicle and its hand-made lists.
