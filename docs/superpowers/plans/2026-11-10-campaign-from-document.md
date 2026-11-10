# Campaign from a Document Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Player gives the app a Source Text, approves a Seed Draft, lets the World Build create the hidden world, and starts play with "Begin" at the point where the text stopped.

**Architecture:** Two new modules in `threshold-core`. `build` owns the Seed Draft, the draft store, the stage runner and the checks; it returns data and writes no Campaign file. `setup` turns a seed and a `WorldSetup` into a Campaign file. `src-tauri` owns commands, events, paths and the cancel flag. The UI gets one new screen.

**Tech Stack:** as the first slice. Rust edition 2024, `rusqlite` bundled, `serde`, `serde_json`, `schemars`, `toml`; Tauri 2; React + TypeScript strict + Vite, npm. No new dependency without a line in the task that needs it.

**Spec:** `docs/superpowers/specs/2026-11-10-campaign-from-document-design.md` (the design) and `docs/superpowers/specs/2026-11-10-campaign-from-document-data-shape.md` (the shape). Read both. Where they differ on a mechanism, the shape wins. ADR 0005 and ADRs 0001 to 0004 are binding. Terms in capitals are defined in `GLOSSARY.md`.

## Global Constraints

- `threshold-core` does not depend on Tauri or Ollama. `build` calls the model through `Gm::propose` only. `Gm` gets no new method. `RecordedGm` replays every build test.
- World Build calls run with thinking off, on the play model. Each call is schema-constrained.
- The World Build returns a `WorldSetup` and writes nothing. Only `setup::create_campaign_file` writes a Campaign file: to `<name>.sqlite.part`, then rename.
- A failed or cancelled build leaves no Campaign file and keeps the approved draft.
- The PC never holds a secret or a hidden fact. No UI command returns one outside the debug view. A build failure shows a stage name and a check name, never model text.
- Progress carries a stage name and two counts only.
- Every character has an age. Code derives adult status with `domain::is_adult`. No type stores adult status. Code never assumes that a character is an adult.
- An open item is an Established fact on a `pending_` key (ADR 0005). No prompt shows it without the marker.
- Schema version 2. A version 1 Campaign file is refused with a clear message. No migration.
- Existing invariants of `2026-11-08-threshold-data-shape.md` hold unless the shape names the change.
- The repository holds no private chronicle and no adult text. The fixture chronicle is invented.
- Everything committed is English. Never write the assistant's personal name; write "Claude Code".
- Each task ends with `cargo fmt`, and the pre-commit hook must pass: `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, `npm --prefix ui run typecheck`, `npm --prefix ui run lint`. Do not skip the hook.

## Review Focus

1. **Empty, whitespace-only or very long Source Text.** Expect a refusal before any model call, with a clear message. The limit is a named constant with a tested value. Test owned by Task 5.
2. **The Player edits the draft into an inconsistent state**: removes a character that a known fact names, gives two characters one name, deletes an age. Expect a `Gap` in the review, never a panic, and `approve` refuses. Test owned by Task 4.
3. **A Source Text with no NPC, or one where the model returns an empty roster.** Expect a valid Campaign with the PC alone; the per-NPC stage runs zero calls. Test owned by Task 6.
4. **Accented names and second names that differ only by case or accent** ("Chloé", "chloe", "Léa"). Expect one entity per person through `entity_by_name`, and a `DuplicateName` gap when two roster entries collide. Test owned by Task 1 and Task 4.
5. **Double actions**: a second `start_build` while one runs, a second "Begin", the app closed during a build. Expect one build at a time, the stored Opening Turn returned on the second "Begin", and an `Approved` draft with no Campaign file after a restart. Tests owned by Task 7 and Task 8.

---

## File structure

```
crates/threshold-core/src/
  domain/adventure_seed.rs     new seed fields, PcSeed.age, OtherName, StartSeed, OpenDetail
  domain/types.rs              AddEntity{age, other_names}, is_adult, ADULT_AGE, OPEN_KEY_PREFIX, is_open_key
  domain/proposal.rs           NewNpc.age
  domain/world_setup.rs        NEW: WorldSetup and its entry types
  setup.rs                     NEW: names to LedgerOps, create_campaign_file, commit_world
  fixture.rs                   becomes a thin TOML reader over setup
  build/mod.rs                 NEW: public surface, Stage, BuildCheck, Control, Progress, BuildFailure
  build/draft.rs               NEW: SeedDraft, KnownFact, Gap, gaps, approve
  build/store.rs               NEW: DraftStore
  build/stages.rs              NEW: stage runner, prompts, schemas, repair and drop rule
  build/checks.rs              NEW: the checks of design 6.5
  store/{mod,project,migrations}.rs   schema 2, Entity.age, other_names, entity_by_name
  context/{mod,narration_prompt,proposal_prompt}.rs   stable block, open marker, opening_prompt
  turn.rs                      TurnKind::Opening, Engine::begin
crates/threshold-core/tests/   names.rs, setup.rs, seed.rs, draft.rs, build.rs, begin.rs (new); others move to `age`
src-tauri/src/{lib,session}.rs commands, events, .part cleanup, one-job guard
ui/src/views/NewCampaign.tsx   NEW: source, review, progress, begin
ui/src/{api,App}.tsx
fixtures/chronicle/            NEW: source.md, expected.toml (invented, no private content)
evals/scenarios/world_build.toml, crates/threshold-eval/src/world_build.rs   NEW
```

Test style, as in the first plan: each test has a name and the behaviour it pins. Write the test first, run it and see it fail, implement, run it and see it pass, commit.

---

### Task 1: Age, other names, one name resolver, schema 2

No new behaviour. This task isolates the widest mechanical change.

**Files:**
- Modify: `domain/adventure_seed.rs`, `domain/types.rs`, `domain/proposal.rs`, `store/{mod,project,migrations}.rs`, `checks/boundary.rs`, `context/{mod,proposal_prompt,narration_prompt}.rs`, `world/arrival.rs`, `fixture.rs`, `fixtures/town/adventure-seed.toml`, `crates/threshold-ollama/examples/{probe,behaviour_probe}.rs`, `crates/threshold-eval/src/*`, every test and `tests/support/mod.rs` that uses `adult`.
- Test: `crates/threshold-core/tests/names.rs` (new), `tests/store.rs`, `tests/checks.rs`, `tests/domain.rs`.

**Interfaces:**
- Produces (shape 3.2, 3.3): `pub const ADULT_AGE: u16 = 18; pub fn is_adult(age: Option<u16>) -> bool` in `domain`; `PcSeed { name, age: u16, other_names: Vec<OtherName>, concept }`; `SeedEntity { name, kind, age: Option<u16>, other_names: Vec<OtherName>, hidden }`; `OtherName { name: String, known_to: Vec<String> }`; `LedgerOp::AddEntity { local, kind, name, age: Option<u16>, other_names: Vec<String> }` with `#[serde(default)]` on `other_names`; `NewNpc { id, name, age: u16, voice }` with schema range 0..=999; `Entity { .., age: Option<u16>, other_names: Vec<String> }`; `impl WorldView { pub fn entity_by_name(&self, name: &str) -> Option<&Entity> }` (main name or other name, through `normalize::name`, lowest id on a tie).

- [ ] **Step 1: Write the failing tests.**
  - `names.rs::resolves_main_and_other_names`: an entity "Jean" with other name "Jade"; `entity_by_name("jade")`, `("JADE")` and `("Jean")` return the same id; an unknown name returns `None`.
  - `names.rs::accents_and_case_share_one_entity`: "Chloé", "chloe" and "CHLOÉ" resolve to one entity (Review Focus 4).
  - `names.rs::tie_takes_the_lowest_id`.
  - `domain.rs::is_adult_needs_a_stored_age`: `is_adult(None) == false`, `is_adult(Some(17)) == false`, `is_adult(Some(18)) == true`.
  - `store.rs::character_without_age_is_refused` (replaces `npc_without_adult_status_is_refused`): an `AddEntity` for a PC or an NPC with `age: None` fails the Commit; a place with an age fails too.
  - `store.rs::version_1_file_is_refused`: a database at `user_version` 1 fails `Store::open_existing` with a message that names the version; the file is unchanged.
  - `checks.rs::intimate_with_minor_rejected` and `intimate_with_adults_ok` pass on ages 17 and 18.
  - `domain.rs::new_npc_schema_requires_age`.
- [ ] **Step 2: Run, expect FAIL.** `cargo test -p threshold-core`
- [ ] **Step 3: Implement.** Schema version 2 in `migrations.rs`: `entity` has `age` and `other_names_json`, no `adult`, and `CHECK ((kind IN ('pc','npc')) = (age IS NOT NULL))`. Replace the five hand comparisons of `norm_name` that shape 3.3 lists with `entity_by_name`. `checks::boundary` is the only caller of `is_adult`. The proposal prompt shows `age 34` on an entity line where it showed `adult`. The seed writes one Persistent fact per other name on the slot `(entity, alias_<normalized other name>, <other name>)`, held by the bearer and each `known_to` name.
- [ ] **Step 4: Run, expect PASS.** `cargo test --workspace`; no test refers to `adult`.
- [ ] **Step 5: Commit** `refactor(core): ages and other names replace the adult flag; schema 2`.

### Task 2: `WorldSetup`, the `setup` module, one way to write a Campaign file

**Files:**
- Create: `domain/world_setup.rs`, `setup.rs`. Modify: `fixture.rs`, `lib.rs`, `src-tauri/src/session.rs`.
- Test: `crates/threshold-core/tests/setup.rs` (new), `tests/town.rs`.

**Interfaces:**
- Consumes: Task 1.
- Produces (shape 3.6): `WorldSetup { facts, attitudes, goals, voice_cards, obstacles, events }`; `FactEntry { subject, key, value, text, known_by: Vec<String>, open: bool }` (both defaulted); the other entry types moved unchanged from the private `WorldFile` of `fixture.rs`; `pub fn commit_world(store: &mut Store, campaign: CampaignId, world: &WorldSetup) -> Result<(), SetupError>`; `pub fn create_campaign_file(path: &Path, seed: &AdventureSeed, world: &WorldSetup, rng_seed: u64) -> Result<(), SetupError>`. `fixture::load_world` reads TOML into `WorldSetup` and calls `commit_world`.

- [ ] **Step 1: Write the failing tests.**
  - `setup.rs::world_toml_facts_section_commits`: a `[[facts]]` entry with `known_by = ["Hessa"]` becomes a fact held by Hessa and not by the PC.
  - `setup.rs::unknown_name_fails_and_writes_nothing`: an attitude toward an undeclared name returns `SetupError` and the store has no `__world__` Turn.
  - `setup.rs::campaign_file_appears_only_when_complete`: after `create_campaign_file`, `path` exists and `path + ".part"` does not; when `commit_world` fails, neither exists.
  - `setup.rs::names_resolve_through_other_names`: an entry that names "Jade" lands on the entity "Jean".
  - `town.rs`: every existing test still passes through the new path.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement. Step 4: Run, expect PASS.** `Session::new_campaign` calls `create_campaign_file`, then opens the file.
- [ ] **Step 5: Commit** `refactor(core): setup module writes Campaign files from a WorldSetup`.

### Task 3: Seed fields, the stable block and its cap, open items

**Files:**
- Modify: `domain/adventure_seed.rs`, `domain/types.rs`, `setup.rs`, `context/{mod,proposal_prompt,narration_prompt}.rs`, `store/project.rs`, `src-tauri/src/session.rs` (Journal entry).
- Test: `crates/threshold-core/tests/seed.rs` (new), `tests/context.rs`, `tests/checks.rs`.

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces (shape 3.1, 3.4): on `AdventureSeed`, all `#[serde(default)]`: `themes: Vec<String>` (max 8, 40 chars each), `gm_agreement: String` (max 1,200 chars), `resolution: String`, `min_age: Option<u16>`, `start: StartSeed { place, present: Vec<String>, situation, when }`, `open_details: Vec<OpenDetail { text, mode: OpenMode }>` with `OpenMode::{Delegated, Discover, ToAgree}`. `pub const OPEN_KEY_PREFIX: &str = "pending_"; pub fn is_open_key(key: &str) -> bool`; `is_state_key` is true for an open key. `pub const ADVENTURE_BLOCK_MAX_CHARS: usize = 2_400`. `JournalEntry.open: bool`.

- [ ] **Step 1: Write the failing tests.**
  - `seed.rs::old_seed_toml_still_parses`: the town seed parses with every new field at its default.
  - `seed.rs::stable_block_holds_the_agreed_fields`: the block has the premise, the PC line with age and other names, the story language, the boundaries, one `min_age` line, the themes and `gm_agreement`; it has no text of `resolution`, `start` or `open_details`.
  - `seed.rs::open_entry_commits_on_a_pending_key`: a `FactEntry { key: "shopping_trip", open: true }` becomes a fact on `pending_shopping_trip`, Established, never Persistent, and the slot `shopping_trip` is empty.
  - `seed.rs::settled_entry_with_the_prefix_is_refused`.
  - `context.rs::open_fact_carries_the_marker_in_both_prompts`: the text of an open fact is preceded by `(open, not done)` in the proposal prompt and in the Narration prompt.
  - `checks.rs::open_fact_can_be_replaced_by_a_turn`: a proposal may write the same `pending_` slot; "Fact bends" does not reject it.
  - `seed.rs::new_npc_below_min_age_is_a_content_boundary_rejection`.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement.** One function in `context` writes the marker. A settled known fact commits with the PC as holder.
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(core): seed agreement fields and open items on pending keys`.

### Task 4: Seed Draft, gaps, approval, draft store

**Files:**
- Create: `build/mod.rs`, `build/draft.rs`, `build/store.rs`. Modify: `lib.rs`.
- Test: `crates/threshold-core/tests/draft.rs` (new).

**Interfaces:**
- Consumes: Tasks 1 to 3.
- Produces (shape 3.5): `SeedDraft { id, state: DraftState, source_text, seed, knowledge: Vec<KnownFact>, ages_to_confirm: Vec<String>, last_failure: Option<BuildFailure>, campaign_file: Option<String> }`; `DraftState::{Draft, Approved, Built}`; `KnownFact { subject, key, value, text, open: bool }`; `Gap { kind: GapKind, at: String }` with `GapKind::{Missing, AgeMissing, AgeUnconfirmed, AgeBelowMin, UnknownName, DuplicateName, PcInRoster, WrongKind, SeedBlockTooLong}`; `pub fn gaps(draft: &SeedDraft) -> Vec<Gap>`; `pub fn approve(draft: &mut SeedDraft) -> Result<(), Vec<Gap>>`; `DraftStore { dir }` with `open, list, load, save, delete, append_debug, read_debug`. Also the shared types `Stage`, `BuildCheck`, `BuildFailure { stage, check }`, `Progress { stage, done, total }`, `Control`, `DebugEntry`, exactly as shape section 2 writes them.

- [ ] **Step 1: Write the failing tests.**
  - `gaps_name_each_problem`, one case per `GapKind`: a missing premise or PC name or start place (`Missing`); a character with no age; a name in `ages_to_confirm`; an age below `min_age`; a known fact whose subject is not in the roster; two roster entries with one normalized name, counting other names; the PC or one of its names in the roster; a Voice Card owner or an age on a place; a stable block above `ADVENTURE_BLOCK_MAX_CHARS`.
  - `edited_draft_never_panics` (Review Focus 2): remove an entity that a known fact names, then call `gaps` and `approve`; the result is an `UnknownName` gap and `Err`.
  - `approve_needs_zero_gaps_and_sets_the_state`.
  - `any_save_returns_the_draft_to_draft_state`: an `Approved` draft that is edited and saved is `Draft` again.
  - `store_round_trip_and_atomic_save`: `save` then `load` gives an equal draft; no `*.tmp` file stays in the folder; `list` returns id, title and state.
  - `debug_file_is_replaced_per_run_and_deleted_with_the_draft`.
  - `failed_build_keeps_the_approved_draft`: setting `last_failure` leaves `state == Approved`.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement. Step 4: Run, expect PASS.** Files: `<dir>/<id>.json` and `<dir>/<id>.debug.jsonl`. `save` writes a temporary file, then renames it.
- [ ] **Step 5: Commit** `feat(build): Seed Draft, gaps, approval and the draft store`.

### Task 5: `draft_from_text` and the stage runner

**Files:**
- Create: `build/stages.rs`. Modify: `build/mod.rs`.
- Test: `crates/threshold-core/tests/build.rs` (new).

**Interfaces:**
- Consumes: Task 4, `Gm`, `RecordedGm`.
- Produces: `pub fn draft_from_text(text: &str, gm: &dyn Gm, ctl: &mut Control) -> Result<SeedDraft, BuildFailure>`; `pub const SOURCE_TEXT_MAX_CHARS: usize` (first value 60,000; the builder tests the largest text that still fits one section call at `num_ctx` 16,384 and records the value). Internal: one stage runner, `run_stage`, that sends a schema-constrained call, runs the stage's checks, sends one repair call with the errors on failure, and returns the items that passed plus the items to drop. Stages here: `DraftSeed`, `DraftRoster`, `DraftKnowledge`. `DraftKnowledge` runs one call per section of the text; a section is a Markdown heading block, or a fixed character window when the text has no heading.

- [ ] **Step 1: Write the failing tests** (all with `RecordedGm`).
  - `empty_or_long_source_is_refused_before_any_call` (Review Focus 1): `""`, `"   "` and a text above the limit return an error; `RecordedGm` records zero calls.
  - `draft_holds_seed_roster_and_known_facts`: three recorded stages give a draft with the premise, the PC with its age, the roster with kinds and ages, and the known facts with `open` set where the stage says so.
  - `knowledge_runs_one_call_per_section`: a text with three headings sends three `DraftKnowledge` calls.
  - `proposed_ages_are_listed_for_confirmation`: an age that the stage marks as proposed lands in `ages_to_confirm`.
  - `draft_keeps_hidden_causes_out`: a stage output that marks a fact as not known to the PC is absent from `knowledge`.
  - `one_repair_then_failure_names_stage_and_check`: two bad outputs for `DraftRoster` return `BuildFailure { stage: DraftRoster, check: Schema }`; the error holds no model text.
  - `progress_reports_stage_and_counts_only`: the recorded `Progress` values have a stage and two numbers; no string field exists on the type.
  - `cancel_stops_between_two_calls`: `cancelled` turns true after the first call; the result is `BuildFailure { check: Cancelled, .. }` and the second call is never sent.
  - `every_call_is_logged`: `Control.log` receives one `DebugEntry` per call, with prompt and raw output.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement.** The prompts tell the model: use only what the text states; never invent a past event or a Player decision; the PC is one entity and its other names go in `other_names`; a planned, proposed or pending item is `open`; mature content in the text becomes neutral facts and themes, and nothing is removed or refused (design 6.2). Prompt wording is the builder's.
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(build): Seed Draft from a Source Text`.

### Task 6: `build_world` and its checks

**Files:**
- Create: `build/checks.rs`. Modify: `build/stages.rs`, `build/mod.rs`.
- Test: `crates/threshold-core/tests/build.rs`.

**Interfaces:**
- Consumes: Tasks 4 and 5.
- Produces: `pub fn build_world(draft: &SeedDraft, gm: &dyn Gm, ctl: &mut Control) -> Result<WorldSetup, BuildFailure>`; `NpcState { goals: Vec<GoalEntry> /* 1..=3 */, secret: Option<FactEntry>, attitudes: Vec<AttitudeEntry>, voice_card: VoiceCardEntry /* samples 2..=4 */ }`; `Placement { npc, place }`; `pub const PC_CENTRED_MAX_SHARE: f32` (first value 0.5; see the last test). Stages, in order (shape 3.6): `HiddenFacts` (one call per section, `known_by` never the PC), `NpcState` (one call per NPC; each call sees the Voice Cards written before it), `WorldEvents` (one call, 3 to 6 `EventEntry`), `StartState` (one call, `Placement` for absent NPCs). Code adds the known facts with the PC as holder, and the `location` of the PC and of each name in `start.present`.

- [ ] **Step 1: Write the failing tests** (all with `RecordedGm`). One trap output and one correct output per check of design 6.5:
  - `references_trap`: an attitude, a fact subject, an event actor, a witness or a placement that names an undeclared entity.
  - `one_pc_trap` and `kinds_trap`: a Voice Card or a goal for a Faction or a place.
  - `completeness_trap`: an NPC with no goal, or a Voice Card with one sample.
  - `agency_trap`: an event effect or a fact that sets a voluntary act, word, feeling or motive of the PC. Reuse the key rules of `checks::agency`.
  - `settled_facts_trap`: a hidden fact on the slot of an accepted fact with another value fails; the same slot and value adds holders and adds no fact.
  - `open_items_trap`: a settled fact on `(s, k)` fails when `(s, pending_k)` exists.
  - `secrecy_trap`: a hidden fact or a secret with the PC in `known_by` fails; a secret is held by its NPC.
  - `world_autonomy_trap`: when more than `PC_CENTRED_MAX_SHARE` of NPC goals and secrets name the PC or one of its names, the `NpcState` stage fails `WorldAutonomy`.
  - `optional_item_is_dropped_and_the_build_goes_on`: a secret, an attitude or an event that fails its repair is absent from the `WorldSetup`; the build returns `Ok`.
  - `essential_item_fails_the_build`: a Voice Card or the start place that fails its repair returns `BuildFailure`.
  - `build_needs_an_approved_draft`: a `Draft` state returns `BuildFailure { check: NotApproved, .. }` with zero calls.
  - `build_adds_no_character`: the entity names of the result equal the approved roster.
  - `roster_without_npc_builds` (Review Focus 3): zero `NpcState` calls, a valid `WorldSetup`.
  - `build_writes_no_file`: the temporary folder is empty after a full build.
  - `full_build_commits_through_setup`: `create_campaign_file` accepts the result; a reopened store shows the PC knowing the known facts and not the hidden ones; an open known fact is on its `pending_` key.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement.** The `NpcState` prompt asks for a life that does not turn around the PC. After this task, the controller runs the live evaluation of Task 10 and sets `PC_CENTRED_MAX_SHARE` from the measured share; the builder leaves 0.5.
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(build): World Build stages and checks`.

### Task 7: Begin

**Files:**
- Modify: `turn.rs`, `store/{mod,migrations}.rs`, `context/{mod,narration_prompt}.rs`, `src-tauri/src/session.rs`.
- Test: `crates/threshold-core/tests/begin.rs` (new), `src-tauri/tests/commands.rs`.

**Interfaces:**
- Consumes: Tasks 2 and 3.
- Produces (shape 3.7): `TurnKind::Opening`; `pub const BEGIN_OP_ID: &str = "__begin__"`; `impl Engine { pub fn begin(&mut self, on_chunk: &mut dyn FnMut(&str)) -> Result<TurnOutcome, TurnError> }`; `pub fn opening_prompt(view: &WorldView, packs: &[ContentPack]) -> Prompt` in `context`. The last block of the prompt holds `start.situation` and `start.when`.

- [ ] **Step 1: Write the failing tests.**
  - `begin_commits_an_opening_turn_with_no_op`: one Turn of kind `Opening`, op id `__begin__`, empty Intent, zero ops, the Narration stored.
  - `second_begin_returns_the_stored_turn` (Review Focus 5): `RecordedGm` records one narration call for two `begin` calls.
  - `begin_narrates_again_when_the_narration_is_missing`.
  - `begin_fails_after_a_play_turn`.
  - `opening_prompt_has_no_hidden_fact`: no GM-only fact, no goal, no scheduled event, no secret in the prompt; reuse the leak assertions of `narration_input_has_no_gm_only_fact`.
  - `opening_survives_a_restart`: `Session::summary` lists the Opening Turn after the file is reopened.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement. Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(turn): Begin as an Opening Turn`.

### Task 8: Commands and events

**Files:**
- Modify: `src-tauri/src/{lib,session}.rs`, `src-tauri/capabilities/default.json`.
- Test: `src-tauri/tests/commands.rs`.

**Interfaces:**
- Consumes: Tasks 4 to 7.
- Produces (shape 3.8): commands `draft_from_text(text) -> DraftView`, `list_drafts() -> Vec<DraftLine { draft_id, title, state }>`, `get_draft(draft_id) -> DraftView`, `save_draft(draft_id, seed, knowledge, ages_to_confirm) -> DraftView`, `approve_draft(draft_id) -> DraftView` or the gaps as an error, `start_build(draft_id) -> CampaignSummary`, `cancel_build()`, `begin() -> TurnSummary`, `delete_draft(draft_id)`, `get_build_debug(draft_id) -> Vec<DebugLine>`. `DraftView { draft_id, state, seed, knowledge, ages_to_confirm, gaps, last_failure }`. `CampaignSummary` gains `needs_begin: bool`. One new event: `build_progress { stage, done, total }`. `begin` reuses `narration_chunk` with op id `__begin__`, `turn_committed` and `turn_failed`.

- [ ] **Step 1: Write the failing tests.**
  - `draft_flow_end_to_end`: text, draft, save, approve, build, begin, with `RecordedGm`; the Campaign opens with `needs_begin` true, then false.
  - `failed_build_leaves_no_campaign_and_keeps_the_draft`: no `.sqlite` and no `.part` file in `campaigns/`; the draft is `Approved` with `last_failure`; a second `start_build` with good outputs succeeds with no new review.
  - `build_error_names_stage_and_check_only`: the error text holds the stage name and the check name and no recorded model text.
  - `one_build_at_a_time` (Review Focus 5): a second `start_build` during a build returns an error; the first one finishes.
  - `cancel_build_stops_draft_and_build`.
  - `part_files_are_deleted_at_start`.
  - `get_build_debug_refused_when_debug_off`.
  - `draft_view_never_holds_hidden_content`: after a build, `get_draft` returns no goal, secret, hidden fact or event.
  - `journal_marks_open_facts`.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement.** A build does not hold the `Session` lock. It locks to save the draft and to open the finished Campaign. `cancel_build` sets a flag and takes no lock. Paths: `<data_dir>/drafts/`, `<data_dir>/campaigns/`.
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(app): draft, build and begin commands`.

### Task 9: The New Campaign screen

**Files:**
- Create: `ui/src/views/NewCampaign.tsx`. Modify: `ui/src/{api,App}.tsx`, `ui/src/style.css`, `ui/src/views/{Journal,Debug}.tsx`.
- Test: `ui/src/api.test.ts`.

**Interfaces:**
- Consumes: Task 8.
- Produces: typed wrappers in `api.ts` for each command and for `build_progress`.

Four states of one screen, plain styling:
1. **Source.** A text area and a file button for `.md` and `.txt`. A "Draft" button. A list of saved drafts.
2. **Review.** Every text field of the seed is editable. A table of characters: name, other names, kind, age, with a "confirm" mark on each proposed age. A list of known facts: text, an "open" mark, a delete button. The gaps appear one at a time above the form. "Approve" is disabled while a gap exists.
3. **Build.** One line: the stage label and "4 of 9". A "Cancel" button. On failure: the stage, the check, a "Retry" button.
4. **Ready.** A "Begin" button. The Play view opens on it and streams the opening Narration.

The Journal shows an open fact with an "open" mark. The Debug view gets a "World Build" tab that calls `get_build_debug`, behind a second click that says the content reveals secrets.

The old entry with two TOML paths leaves the UI. `new_campaign` stays as a command for tests and evaluations.

- [ ] **Step 1: Write the failing tests** (Vitest): `api.ts` maps a gap error of `approve_draft` to a typed list; `build_progress` payloads parse; a stage name maps to a label and an unknown stage falls back to a neutral label.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement. Step 4: Run, expect PASS.** `npm --prefix ui test`, `typecheck`, `lint`.
- [ ] **Step 5: Run the app** with `npm --prefix ui run tauri dev` and the fixture chronicle of Task 10. Go through the four states once. Report what was seen, including anything that looks wrong.
- [ ] **Step 6: Commit** `feat(ui): New Campaign from a document`.

### Task 10: Fixture chronicle and the World Build evaluation

**Files:**
- Create: `fixtures/chronicle/source.md`, `fixtures/chronicle/expected.toml`, `evals/scenarios/world_build.toml`, `crates/threshold-eval/src/world_build.rs`. Modify: `crates/threshold-eval/src/{lib,main}.rs`.
- Test: `crates/threshold-eval/tests/report.rs`.

**Interfaces:**
- Consumes: Tasks 5 and 6.
- Produces: the scenario `world_build` in the evaluation runner and its report section.

The fixture is an invented chronicle of about 8,000 characters, in the form of the owner's private ones: a provenance note, a PC with a second name that some NPCs do not know, eight named NPCs with stated or missing ages, one child, two groups, three places, settled events, at least five open items marked "planned" or "not established", a stopping point, open threads. Fantasy or modern, no adult content, no real person. `expected.toml` lists by hand: the characters with their kinds, the settled facts, the open items.

- [ ] **Step 1: Write the failing tests** (no live model): `report_has_every_world_build_metric`; `false_past_counts_a_settled_fact_on_an_open_item`; `pc_centred_share_counts_goals_and_secrets_that_name_the_pc`; `roster_recall_matches_through_other_names`.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement** the metrics of design section 8: roster recall, fact recall, false past, PC-centred share, voice distance (the blind judge that exists), build time p50 and max. Three runs.
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(eval): World Build scenario on an invented chronicle`.
- [ ] **Step 6 (controller, live model):** run the scenario, write the numbers to `evals/reports/2026-11-world-build.md`, set `PC_CENTRED_MAX_SHARE` and `SOURCE_TEXT_MAX_CHARS` from them, commit.

---

## After the last task

The controller runs the whole-branch review, then the QA pass on the running app. The owner then tests his private chronicle by hand. It never enters the repository.
