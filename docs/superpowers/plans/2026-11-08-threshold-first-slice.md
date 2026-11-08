# Threshold First Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A playable local desktop slice in which the town demo produces a delayed offscreen consequence, every Commit check holds, and a warm Turn stays at or under 15 s on the target machine.

**Architecture:** A Cargo workspace. `threshold-core` holds the domain, store, checks, rules, world, context and turn modules, with no Tauri or HTTP dependency. `threshold-ollama` implements the core's `Gm` trait over Ollama `/api/chat`. `src-tauri` is a thin shell that exposes commands and streams narration events to a React + TypeScript UI. `threshold-eval` drives the core against the live model and writes a report.

**Tech Stack:** Rust (edition 2024, stable toolchain, cargo 1.98 on the target machine), `rusqlite` with the `bundled` feature, `serde`, `serde_json`, `schemars`, `toml`, `rand_chacha`, `ureq` (blocking HTTP), `thiserror`; Tauri 2; React + TypeScript (strict) + Vite, npm (pnpm is not active under the machine's Node 26).

**Spec:** `docs/superpowers/specs/2026-11-08-threshold-design.md` and `docs/superpowers/specs/2026-11-08-threshold-data-shape.md`. Terms in capitals are defined in `GLOSSARY.md`. ADRs 0001 to 0004 in `docs/adr/` are binding.

## Global Constraints

- A Turn takes 15 s or less, warm, on the target machine (RTX 4070 SUPER 12 GB, 32 GB RAM). The 15 s covers both model calls, the retry and due World Events. OOC Turns are outside it.
- Default model `gemma4:26b-a4b-it-qat`, one model for both calls, `think: false`, `num_ctx` 16384, warm-up call with `keep_alive` "30m" at app start. The model tag is configuration only.
- Prompt order is fixed: system rules + Adventure Seed, then the Scene block, then recent Turns + Intent. About 4-6K fresh prompt tokens per Turn.
- The model never sets a numeric difficulty (ADR 0002). The Ruleset sets difficulty and rolls from a stored seed.
- One retry per Turn in total, shared by the schema parse and the Commit checks.
- All Commit writes happen in one SQLite transaction. The ledger is append-only and is the source of truth.
- Hard limits live in the core: sexual content involves adults only, Adventure Seed boundaries always win, and no Content Pack can change a Commit check.
- Content Packs are data only: no code.
- Services bind to localhost only. No telemetry. No cloud call at play time.
- Everything committed to the repo is English. Never write the assistant's personal name in the repo; write "Claude Code".
- No dependency on `sneq-engine`. Ported SNEQ logic is rewritten in Rust (ADR 0003).

## Review Focus

1. **Ollama down, or the model tag missing.** Expect a visible Turn failure within the HTTP timeout (60 s), nothing committed, and the app still usable. Test owned by Task 2 and Task 8.
2. **Model output with handles that don't exist or have the wrong prefix** (`F12` where an entity is expected, an `N1` used twice, `E999`). Expect the References check to reject the proposal; never a panic or a dangling row. Test owned by Task 5.
3. **Accented and apostrophe names** ("Orsel d'Aubrac", "Hessa", "l'Hôtel de Ville"), because the story language may be French. Expect one normalized form per name, so the same NPC never becomes two entities. Test owned by Task 1.
4. **Empty, whitespace-only, or very long Intents.** Expect an empty Intent to be refused before any model call, and an Intent over 2,000 characters to be refused with a clear message. Test owned by Task 8.
5. **The app closes during narration, after the Commit.** Expect the Turn to stay committed; on reload it shows as "narration interrupted" with a button to narrate it again. Test owned by Task 8.

---

## File structure

```
Cargo.toml                         workspace
crates/threshold-core/src/
  lib.rs
  domain/{mod,ids,slot,proposal,entity,event,adventure seed}.rs
  normalize.rs                     name, key and value normalization
  store/{mod,migrations,project}.rs
  rules.rs                         Ruleset trait + D10
  checks/{mod,references,agency,shortcuts,boundary}.rs
  world/{mod,clock,beliefs}.rs
  context/{mod,proposal_prompt,narration_prompt}.rs
  pack.rs                          Content Pack loader
  model.rs                         Gm trait, Prompt, GmError, RecordedGm
  turn.rs                          Engine
crates/threshold-ollama/src/lib.rs
crates/threshold-ollama/examples/probe.rs
crates/threshold-eval/src/main.rs
fixtures/town/{adventure-seed.toml,world.toml}
fixtures/packs/test-pack/{pack.toml,prompt.md}
evals/scenarios/*.toml
src-tauri/                         Tauri 2 shell
ui/                                React + TS + Vite
.githooks/pre-commit
```

---

### Task 1: Workspace, domain types and the proposal schema

**Files:**
- Create: `Cargo.toml`, `crates/threshold-core/Cargo.toml`, `crates/threshold-core/src/lib.rs`, `crates/threshold-core/src/domain/*.rs`, `crates/threshold-core/src/normalize.rs`, `.githooks/pre-commit`, `.gitignore`
- Test: `crates/threshold-core/tests/domain.rs`

**Interfaces:**
- Produces: every type in data-shape §3.1 and §3.3, verbatim names and fields: `EntityKind`, `Canon`, `FactSource`, `Stance` (ordered `Oppose < Resist < Neutral < Comply < Help`), `Knowing`, `Slot`, `Trigger`, `Effect`, `LedgerOp` (each variant with a struct payload, plus `RevertTurn { turn_id }` used by Corrections in Task 8), `DecisionProposal`, `CheckDraft`, `Factor`, `Reaction`, `Cause`, `NewNpc` (with `voice: String`), `SlotDraft`, `NewFact`, `Seen`, `Cancel`, `EventDraft`, `ActionKind`, `Verdict`.
- Produces: `Handle` parsed from strings `E<n>`, `F<n>`, `W<n>`, `G<n>`, `N<n>`: `Handle::parse(&str) -> Result<Handle, HandleError>`, `Display` round-trips.
- Produces: `pub fn proposal_schema() -> serde_json::Value` (from `schemars`, with `maxItems`: blockers 2, reactions 2, new_npcs 1, facts 2, cancel 1, factors 3, effects 2; `plan` maxLength 90; `voice` maxLength 120).
- Produces: `normalize::name(&str) -> String`, `normalize::key(&str) -> String`, `normalize::value(&str) -> String`.
- Produces: `AdventureSeed` (from TOML): `premise: String`, `pc: PcSeed { name, adult: true, concept }`, `boundaries: Vec<String>`, `decided: Vec<SeedFact>`, `proposed: Vec<SeedFact>`, `story_language: String`; `SeedFact { subject: String, key: String, value: String, text: String }`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn handle_round_trip() { for s in ["E12","F40","W3","G2","N1"] { assert_eq!(Handle::parse(s).unwrap().to_string(), s); } }
#[test] fn handle_rejects_garbage() { for s in ["", "X1", "F", "F-1", "f12"] { assert!(Handle::parse(s).is_err()); } }
#[test] fn stance_is_ordered() { assert!(Stance::Oppose < Stance::Resist && Stance::Comply < Stance::Help); }
#[test] fn schema_caps_lists() {
    let s = proposal_schema();
    assert_eq!(s.pointer("/properties/reactions/maxItems").unwrap(), 2);
    assert_eq!(s.pointer("/properties/facts/maxItems").unwrap(), 2);
}
#[test] fn proposal_minimal_parses() {
    let p: DecisionProposal = serde_json::from_str(r#"{"action":"talk","verdict":"allowed"}"#).unwrap();
    assert!(p.facts.is_empty() && p.check.is_none());
}
#[test] fn normalize_french_names() {
    assert_eq!(normalize::name("Orsel d'Aubrac"), normalize::name("  orsel D’Aubrac "));
    assert_eq!(normalize::name("l'Hôtel de Ville"), normalize::name("L'Hôtel  de ville"));
    assert_eq!(normalize::key("Gate State"), "gate_state");
}
```

Normalization rule: NFKC, lowercase, curly apostrophes to `'`, trim, collapse spaces, strip leading English articles (`the`, `a`, `an`) only. Keep accents and French elisions as they are, because both forms in the test must match each other, not an ASCII form.

- [ ] **Step 2: Run `cargo test -p threshold-core --test domain`.** Expected: FAIL, unresolved names.
- [ ] **Step 3: Implement the types and functions above.** `serde(rename_all = "snake_case")` on enums. A `JsonSchema` `schema_with` or attribute sets the `maxItems` values.
- [ ] **Step 4: Run `cargo test -p threshold-core --test domain`.** Expected: PASS.
- [ ] **Step 5: Add `.githooks/pre-commit` running `cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace`; run `git config core.hooksPath .githooks`.**
- [ ] **Step 6: Commit** `feat(core): domain types, handles, proposal schema`.

### Task 2: Ollama adapter and the proposal probe (decision gate)

**Files:**
- Create: `crates/threshold-core/src/model.rs`, `crates/threshold-ollama/Cargo.toml`, `crates/threshold-ollama/src/lib.rs`, `crates/threshold-ollama/examples/probe.rs`
- Test: `crates/threshold-ollama/tests/ollama.rs`

**Interfaces:**
- Produces (core, `model.rs`): the `Gm` trait from data-shape §2; `Prompt { system: String, stable: String, scene: String, recent: String }` sent as one system message (`system + stable + scene`) and one user message (`recent`), in that byte order; `GmError { Unreachable(String), Timeout, BadStatus(u16, String) }`; `RecordedGm::new(proposals: Vec<String>, narrations: Vec<String>)` returning them in order.
- Produces (ollama): `OllamaConfig { base_url: String, model: String, num_ctx: u32, keep_alive: String, timeout_s: u64 }` with `Default` = `http://127.0.0.1:11434`, `gemma4:26b-a4b-it-qat`, 16384, `"30m"`, 60; `OllamaGm::new(OllamaConfig)`; `OllamaGm::warm_up(&self) -> Result<(), GmError>`; `impl Gm for OllamaGm` (propose: `stream:false`, `format: schema`; narrate: `stream:true`, calls `on_chunk` per chunk).
- Produces: `OllamaConfig::from_toml(&str) -> Result<OllamaConfig, ConfigError>`; the app reads it from `threshold.toml` in its data directory, so changing the model tag is a config edit. Test `config_changes_model_tag` parses a file with `model = "qwen3.8:27b"`.
- Produces: `OllamaGm::last_metrics() -> Option<CallMetrics { prompt_tokens, eval_tokens, total_ms }>`.

- [ ] **Step 1: Write the failing tests.**
  - `unreachable_server_fails_fast`: config `base_url = "http://127.0.0.1:9"` and `timeout_s = 2`; `propose` returns `Err(GmError::Unreachable(_))` within 3 s.
  - `missing_model_reports_status`: marked `#[ignore]` (needs a live Ollama); model `"no-such-model:1b"` gives `Err(GmError::BadStatus(404, _))`.
- [ ] **Step 2: Run `cargo test -p threshold-ollama`.** Expected: FAIL.
- [ ] **Step 3: Implement the adapter with `ureq`.** Streaming reads NDJSON lines and concatenates `message.content`.
- [ ] **Step 4: Run `cargo test -p threshold-ollama`.** Expected: PASS (ignored test skipped).
- [ ] **Step 5: Write `examples/probe.rs`.** It sends a fixed town-hall prompt (Magistrate Orsel, two clerks, a public insult; then a lock-picking Intent that needs a Check; then an Intent that should schedule an event), 30 runs each, with `proposal_schema()`. It parses each result into `DecisionProposal` and prints, per Intent: the valid rate; `eval_tokens` p50/p90/max; per-field presence; total ms p50/p90.
- [ ] **Step 6: Run `cargo run -p threshold-ollama --example probe --release` with the model warm.** Record the output in `docs/probes/2026-11-proposal-probe.md`.
- [ ] **Step 7: Gate.** Continue only if every Intent has a valid rate ≥ 90 % and eval tokens p90 ≤ 180. Otherwise stop and report to the owner with the numbers. The cut order for the proposal (data-shape §9) is decided from them.
- [ ] **Step 8: Commit** `feat(ollama): Gm adapter and proposal probe`.

### Task 3: Store, ledger, projections and replay protection

**Files:**
- Create: `crates/threshold-core/src/store/{mod,migrations,project}.rs`
- Test: `crates/threshold-core/tests/store.rs`

**Interfaces:**
- Consumes: Task 1 types.
- Produces: `Store::open(path: &Path) -> Result<Store, StoreError>`, `Store::open_in_memory()`; `create_campaign(&mut self, seed: &AdventureSeed, seed: u64) -> Result<CampaignId, StoreError>` (writes Adventure Seed facts per data-shape §3.2 canon rules: decided → Persistent, proposed → Possible); `snapshot(&self, c: CampaignId) -> Result<WorldView, StoreError>`; `snapshot_before(&self, c: CampaignId, turn_seq: u64) -> Result<WorldView, StoreError>` (replays the ledger up to that Turn); `commit(&mut self, rec: TurnRecord, ops: Vec<LedgerOp>) -> Result<TurnId, CommitError>` with `CommitError::DuplicateOp(TurnId)`; `turn_by_op(&self, op: &OpId) -> Result<Option<StoredTurn>, StoreError>`; `set_narration(&mut self, t: TurnId, text: &str)`; `rebuild_projections(&mut self, c: CampaignId)`; `log_debug(&self, e: DebugEntry)` (outside any transaction).
- Produces: `WorldView { clock_min, entities, live_facts, obstacles_open, events_scheduled, goals_active, attitudes, knowledge, recent_turns, turn_seq }` with lookup helpers `entity(Handle)`, `live_fact(subject, key)`, `fact(Handle)`.
- Produces: `TurnRecord { op_id, kind: TurnKind /* seed | play | correction | ooc */, intent, proposal_json, clock_before, clock_after }`.

- [ ] **Step 1: Write the failing tests.**
  - `migrations_apply_on_empty_db`.
  - `npc_without_adult_status_is_refused`: inserting an `AddEntity` op for an NPC with `adult: None` fails the Commit; the database is unchanged.
  - `one_live_value_per_slot`: two `AddFact` ops on the same Established slot in one Commit, without a `SupersedeFact`, give `Err`, and nothing is written.
  - `duplicate_op_id_commits_once`: the second `commit` with the same `op_id` returns `Err(DuplicateOp(id))`; the ledger row count is unchanged.
  - `rebuild_matches_projections`: after 3 Commits that touch every op kind, `rebuild_projections` yields a `snapshot` equal to the one before.
  - `restart_keeps_state`: open a file DB, commit, drop, reopen; `snapshot` is equal.
- [ ] **Step 2: Run `cargo test -p threshold-core --test store`.** Expected: FAIL.
- [ ] **Step 3: Implement the schema exactly as data-shape §3.2,** plus `turn.narration` nullable. `commit` runs every projection write and the ledger insert in one `rusqlite` transaction.
- [ ] **Step 4: Run the tests.** Expected: PASS.
- [ ] **Step 5: Commit** `feat(store): ledger, projections, op_id guard`.

### Task 4: Ruleset and the d10 prototype

**Files:**
- Create: `crates/threshold-core/src/rules.rs`
- Test: `crates/threshold-core/tests/rules.rs`

**Interfaces:**
- Produces: `trait Ruleset { fn difficulty(&self, check: &CheckDraft, view: &WorldView) -> u8; fn roll(&self, difficulty: u8, seed: u64) -> RollResult; }`; `RollResult { seed, difficulty, value: u8, success: bool }`; `pub fn turn_seed(campaign_seed: u64, turn_seq: u64) -> u64 = campaign_seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ turn_seq`; `struct D10`.
- D10 values: base difficulty 6; −1 per helping Factor, +1 per hindering Factor; clamp 2..=10. Roll: `ChaCha8Rng::seed_from_u64(seed)`, value in 1..=10; success iff `value >= difficulty`.

- [ ] **Step 1: Write the failing tests.** `same_seed_same_roll` (1,000 seeds, two calls each, equal); `difficulty_from_factors` (2 helping + 1 hindering → 5; 6 hindering → 10); `distribution_is_sane` (10,000 seeds: every face appears between 800 and 1,200 times).
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement. Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(rules): seeded d10 ruleset`.

### Task 5: Commit checks

**Files:**
- Create: `crates/threshold-core/src/checks/{mod,references,agency,shortcuts,boundary}.rs`
- Test: `crates/threshold-core/tests/checks.rs`, fixtures as Rust builders in `crates/threshold-core/tests/support/mod.rs`

**Interfaces:**
- Consumes: Tasks 1, 3, 4.
- Produces: `pub fn validate(p: &DecisionProposal, view: &WorldView) -> Result<(), Vec<Rejection>>` (everything before the Roll); `pub fn to_ops(p: &DecisionProposal, roll: Option<&RollResult>, view: &WorldView, turn_seq: u64) -> Vec<LedgerOp>` (applies the win or lose stake, clamps a stance whose `Cause::Check` failed, opens obstacles from `blockers`, writes `Know(Witnessed)` per `seen`, commits proposal facts as Established and as Persistent when `seen` includes the PC, promotes a cited or PC-perceived Possible fact to Established, and writes a Voice Card from each `NewNpc.voice`); `Rejection { check: CheckKind, handle: Option<Handle>, reason: String }` with `Display` as one line; `pub const STANCE_CEILINGS` per data-shape §4; `pub const PC_INVOLUNTARY_KEYS: &[&str] = &["condition", "injury", "fatigue", "carrying"]`.
- Rules, all from data-shape §4 and spec §6.3:
  - **References:** every handle exists in `view`, or is an `N` handle declared once in `new_npcs`; prefixes match the field (`blockers` and `because` take `F`, `npc` and `actor` take `E` or `N`, `event` takes `W`); list lengths respect the caps.
  - **Agency:** `reactions[].npc` and `schedule.actor` are never the PC; a `facts` entry whose subject is the PC must use a key in `PC_INVOLUNTARY_KEYS`.
  - **Four Shortcut checks:** exactly as data-shape §4.
  - **Content Boundary:** if `intimate`, every NPC in `reactions` and every present NPC must have `adult == Some(true)`; `new_npcs` with `adult: false` cannot appear in an intimate proposal.

- [ ] **Step 1: Write the failing tests: one trap and one caused version per check.**
  - `coincidence_trap_rejected`: obstacle open on `gate.locked = yes`; a fact `gate.locked = no` with no `because` → `Err` with `CheckKind::Coincidence`.
  - `coincidence_via_win_stake_ok`: the same write as `check.win` → `Ok`.
  - `cause_created_this_turn_rejected`: `because` points at a fact in the same proposal → `Err`.
  - `npc_fold_trap_rejected`: Orsel's attitude to the PC is −4, and he has an active goal against the PC; a reaction with `Stance::Comply` and no cause → `Err(NpcFolds)`.
  - `npc_fold_with_check_clamped_on_failure`: `Cause::Check` with `check.against = Orsel`; with a failed roll, `to_ops` writes the stance at `Resist`.
  - `fact_bend_rejected_even_with_cause`: a write to a Persistent slot with a valid `because` → `Err(FactBends)`.
  - `cancel_without_cause_rejected` and `actor_death_counts_as_cancel`.
  - `bad_handles_rejected` (Review Focus 2): `E999`, an `F` handle in `npc`, `N1` declared twice → `Err(References)`; no panic.
  - `pc_voluntary_fact_rejected`: subject PC, key `intends` → `Err(Agency)`; key `injury` → `Ok`.
  - `intimate_with_minor_rejected`, and `intimate_with_adults_ok`.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement. Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(checks): references, agency, shortcuts, boundary`.

### Task 6: World Clock, World Events and Beliefs

**Files:**
- Create: `crates/threshold-core/src/world/{mod,clock,beliefs}.rs`
- Test: `crates/threshold-core/tests/world.rs`

**Interfaces:**
- Produces: `pub fn advance(view: &WorldView, minutes: u32) -> Vec<LedgerOp>` (`AdvanceClock`, then fires due events in trigger order, at most `MAX_EVENTS_PER_TURN = 3`, each as `FireEvent` plus its effect ops; a fired event's `SetFact` on a Persistent slot is allowed only when the event was scheduled in an earlier Turn); `pub fn beliefs(view: &WorldView, holder: EntityId) -> Vec<Belief>`; `pub fn pc_knowledge(view: &WorldView) -> Vec<Belief>`; `Belief { subject, key, value, text, how: Knowing, source: Option<EntityId> }`.

- [ ] **Step 1: Write the failing tests.**
  - `event_fires_when_due_and_pc_absent`: an event at day 4 18:00; advance the clock past it with the PC in another place; the effects commit; the PC has no knowledge row for them.
  - `event_cap_per_turn`: 5 due events; one advance fires 3; the next advance fires 2.
  - `delayed_news_arrives_later`: a `Tell` with a delay of 120 min is not in `beliefs` at +60 and is at +120.
  - `told_claim_can_be_false`: a Claim that contradicts World Truth shows as a `Told` Belief and changes no slot.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement. Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(world): clock, events, derived beliefs`.

### Task 7: Context builder, Content Packs and the narration filter

**Files:**
- Create: `crates/threshold-core/src/context/{mod,proposal_prompt,narration_prompt}.rs`, `crates/threshold-core/src/pack.rs`, `fixtures/packs/test-pack/{pack.toml,prompt.md}`
- Test: `crates/threshold-core/tests/context.rs`

**Interfaces:**
- Produces: `pub fn proposal_prompt(view: &WorldView, packs: &[ContentPack], intent: &str) -> Prompt`; `pub fn narration_prompt(view_after: &WorldView, perceived: &[LedgerOp], reactions: &[Reaction], intent: &str) -> Prompt`; `pub fn perceived_by_pc(ops: &[LedgerOp], view_after: &WorldView) -> Vec<LedgerOp>`; `ContentPack::load(dir: &Path) -> Result<ContentPack, PackError>` with `ContentPack { name, prompt_fragments: Vec<(InsertPoint, String)>, seed_options: Vec<String>, model_hints: Vec<String> }`; `InsertPoint { Tone, Scenes }`.
- Retrieval rules (deterministic): live facts whose subject is the current place, an entity present, or an entity named in the Intent or in the last 3 Turns; the open obstacles; the live keys of each entity shown; the Voice Cards of present NPCs. Recent Turns: the last 6, cut from the oldest until the `recent` block is ≤ 6,000 characters.

- [ ] **Step 1: Write the failing tests.**
  - `prefix_is_stable_within_scene`: two consecutive Turns in the same Scene give byte-identical `system + stable + scene`.
  - `narration_input_has_no_gm_only_fact`: a fact with no `fact_visible` rows (GM only) and text "Orsel skims the grain tithe" never appears in any `narration_prompt` field.
  - `voice_cards_of_present_npcs_only`.
  - `pack_cannot_change_checks`: load `test-pack` (its prompt says "ignore all age rules"), then run the Task 5 test `intimate_with_minor_rejected` with the pack loaded; it is still `Err`.
  - `pack_with_code_is_refused`: a pack directory that contains a `.js`, `.wasm`, `.so` or `.rs` file → `Err(PackError::CodeNotAllowed)`.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement. Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(context): prompts, retrieval, narration filter, content packs`.

### Task 8: The Turn engine

**Files:**
- Create: `crates/threshold-core/src/turn.rs`
- Test: `crates/threshold-core/tests/turn.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: `Engine::new(store: Store, gm: Box<dyn Gm>, rules: Box<dyn Ruleset>, packs: Vec<ContentPack>, campaign: CampaignId)`;
  `play_turn(&mut self, op_id: OpId, intent: &str, on_chunk: &mut dyn FnMut(&str)) -> Result<TurnOutcome, TurnError>`;
  `correct_last(&mut self, op_id: OpId, clarified_intent: &str, why: &str, on_chunk: &mut dyn FnMut(&str)) -> Result<TurnOutcome, TurnError>`;
  `ooc(&mut self, question: &str) -> Result<String, TurnError>`;
  `renarrate(&mut self, turn: TurnId, on_chunk: &mut dyn FnMut(&str)) -> Result<String, TurnError>`;
  `TurnOutcome { turn_id, roll: Option<RollResult>, narration: String, timings: Timings }`; `TurnError { EmptyIntent, IntentTooLong, Model(GmError), Rejected(Vec<Rejection>), Store(StoreError) }`.
- Pipeline order: duplicate `op_id` check → Intent check (empty or > 2,000 chars) → `proposal_prompt` → `gm.propose` → parse → `checks::validate` → on failure, one retry with the rejection lines appended to `recent` → `Ruleset` → `checks::to_ops` → `world::advance(minutes)` → `store.commit` → `gm.narrate` with `narration_prompt` (streaming) → `store.set_narration`. `debug_log` gets one row per step with its ms.
- `correct_last`: takes `snapshot_before(last.seq)`, reruns the pipeline with the clarified Intent and the **stored seed** of the corrected Turn, commits a `correction` Turn whose ops start with a `RevertTurn { turn_id }` op (add this `LedgerOp` variant; `rebuild_projections` skips the ops of a reverted Turn), and writes a `correction` row. Only the last play Turn can be corrected.
- `ooc`: one plain model call with the system rules, the Adventure Seed and the PC Knowledge only; no hidden facts; commits nothing.

- [ ] **Step 1: Write the failing tests (all with `RecordedGm`).**
  - `malformed_twice_commits_nothing`: two invalid proposals → `Err`, ledger unchanged, `narrate` never called.
  - `rejected_then_valid_commits_once`: a coincidence trap, then a caused proposal → `Ok`; exactly one Turn row.
  - `retry_with_same_op_id_returns_stored_turn`: `play_turn` twice with one `op_id` → the second returns the first `turn_id`; one Turn row.
  - `seeded_check_is_reproducible`: the same campaign seed and the same proposals in two fresh stores → equal `RollResult`.
  - `correction_keeps_seed`: correct a Turn that had a Check; the new roll has the same seed and value.
  - `empty_and_long_intents_refused` (Review Focus 4): `"   "` → `EmptyIntent`, 2,001 chars → `IntentTooLong`; `RecordedGm` records zero calls.
  - `interrupted_narration_recoverable` (Review Focus 5): `on_chunk` panics after the Commit; on reopen, the Turn exists with `narration == None`; `renarrate` fills it.
  - `model_down_commits_nothing` (Review Focus 1): a `Gm` that returns `Unreachable` → `Err(Model(_))`, ledger unchanged.
  - `ooc_never_sees_secrets`: the prompt `RecordedGm` received holds no GM-only fact text.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement. Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(turn): engine with retry, corrections, ooc`.

### Task 9: The town demo fixture

**Files:**
- Create: `fixtures/town/adventure-seed.toml`, `fixtures/town/world.toml`, `crates/threshold-core/src/fixture.rs`
- Test: `crates/threshold-core/tests/town.rs`

**Interfaces:**
- Produces: `pub fn load_world(store: &mut Store, campaign: CampaignId, path: &Path) -> Result<(), FixtureError>` that commits the seeded entities, facts, relationships, goals, Voice Cards and obstacles as one Turn of kind `seed`.
- World content: the town of Varn; the PC (adult, an outsider scribe); Magistrate Orsel (adult, proud, vindictive, polite on the surface; goal "protect his reputation"; Voice Card with formal speech, legal vocabulary, three sample lines); his investigator Doul (adult, terse; goal "serve Orsel"); clerk Hessa (adult, nervous, gossips); the Magistracy (Faction). GM-only fact: Orsel skims the grain tithe. One Possible rumor: "the miller cheats the scales". The Adventure Seed boundaries include "adult participants only".

- [ ] **Step 1: Write the failing test `town_demo_offscreen_consequence`** with `RecordedGm`: Turn 1, the PC insults Orsel in public. The recorded proposal holds `reactions[Orsel: Resist, plan "smiles, will have the scribe investigated"]` and a `schedule` (Doul, in 3 days, effect `pc.reputation = under_suspicion`, `seen: gm`). Turns 2-3: travel 4 days. Assert: the event fired while the PC was elsewhere; Hessa holds a `Witnessed` Belief about the investigation; PC Knowledge has no fact about it until Turn 4, when the PC meets Hessa and a proposal fact `seen: pc` cites it.
- [ ] **Step 2: Run, expect FAIL. Step 3: Write the fixture files and loader. Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `feat(fixture): town demo`.

### Task 10: Tauri shell and the React UI

**Files:**
- Create: `src-tauri/` (Tauri 2), `ui/` (Vite + React + TS), `ui/src/{App.tsx,api.ts,views/Play.tsx,views/Character.tsx,views/Journal.tsx,views/Debug.tsx}`, `ui/tsconfig.json` (`"strict": true`), `ui/eslint.config.js`
- Modify: `.githooks/pre-commit` (add `npm --prefix ui run typecheck && npm --prefix ui run lint`)
- Test: `ui/src/api.test.ts` (Vitest), `src-tauri/tests/commands.rs`

**Interfaces:**
- Consumes: `Engine` (Task 8), `OllamaGm` (Task 2), `load_world` (Task 9).
- Produces Tauri commands: `new_campaign(seed_path: String, world_path: String) -> CampaignSummary`, `open_campaign(db_path: String) -> CampaignSummary`, `submit_intent(op_id: String, text: String) -> TurnSummary`, `correct_last(op_id: String, clarified: String, why: String) -> TurnSummary`, `ask_ooc(text: String) -> String`, `renarrate(turn_id: i64) -> String`, `get_character() -> CharacterView`, `get_journal() -> JournalView`, `get_debug(turn_id: i64) -> DebugView` (refused unless debug mode is on).
- Produces events: `narration_chunk { op_id, text }`, `turn_committed { turn_id, roll }`, `turn_failed { op_id, reason }`.
- The shell runs each Turn in `spawn_blocking`, calls `warm_up` at start, and binds nothing to a network port. The Journal shows observations and Claims apart, and never shows a GM-only fact. Debug mode is a toggle in the menu, shown with a visible banner.

- [ ] **Step 1: Write the failing tests:** `get_debug_refused_when_debug_off`; `journal_hides_gm_only_facts` (Rust); `api.ts` builds a fresh `op_id` per submit and reuses it on retry (Vitest).
- [ ] **Step 2: Run `cargo test -p threshold-app` and `npm --prefix ui test`.** Expected: FAIL.
- [ ] **Step 3: Implement.** Plain styling.
- [ ] **Step 4: Run both test commands.** Expected: PASS. Then `npm --prefix ui run tauri dev`, play 3 Turns of the town demo by hand, and confirm that narration streams and the roll shows.
- [ ] **Step 5: Commit** `feat(app): tauri shell and play ui`.

### Task 11: Evaluation runner

**Files:**
- Create: `crates/threshold-eval/src/main.rs`, `evals/scenarios/{latency,memory,voices,shortcuts,agency_leaks,offscreen}.toml`
- Output: `evals/reports/<date>.md` (committed by hand when meaningful)

**Interfaces:**
- Consumes: `Engine`, `OllamaGm`, `load_world`.
- Produces: `threshold-eval --scenario <name|all> --runs <n>` printing and writing a Markdown report. A `Judge` trait has a local Ollama implementation; a frontier judge is optional, for development only, behind a `--judge` flag, and never linked into the app.
- Metrics, from spec §10: Turn time p50 and p90 (pass line: p90 ≤ 15 s warm); memory (a detail planted on Turn 3, used when it matters on Turn 40: correct / wrong / missing); voices (blind judge matches dialogue lines to speakers, against the chance rate); Shortcuts (per trap scenario: how often the model proposed the Shortcut and how often a check caught it, read from `debug_log` rejections); agency and secret leaks per 100 Turns (the token leak scan ported from SNEQ, plus a judge pass); offscreen consequence (the town demo effect observed: yes or no).

- [ ] **Step 1: Write the failing test `report_has_every_metric`:** running with a `RecordedGm` and 2 runs produces a report with one section per scenario.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement. Step 4: Run, expect PASS.**
- [ ] **Step 5: Run `cargo run -p threshold-eval --release -- --scenario all --runs 10` against the live model.** Commit the report as `evals/reports/2026-11-baseline.md`.
- [ ] **Step 6: Commit** `feat(eval): evaluation runner and baseline`.
