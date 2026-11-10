# Play Experience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Play screen reads like a game: a story with speaker bubbles and name links, a side panel built from PC Knowledge only, an action bar, a Journal the app writes, Player Notes and Pins, a home screen, and a review screen that speaks in plain sentences and survives a rename.

**Architecture:** Every Player screen reads one type, `PcSight`, made by `world::pc_sight` from `WorldView` and the Turn rows. A `view` module turns `PcSight` plus the Player's `Marks` into screen types. Notes and Pins live in two Player tables that only the shell's `PlayerStore` reads; the `Engine` never holds them. The narrator gets one new rule line (the speech mark); no model call and no proposal field are added.

**Tech Stack:** as the previous plans. Rust edition 2024, `rusqlite` bundled, `serde`, `serde_json`, `schemars`; Tauri 2; React 19 + TypeScript strict + Vite + Vitest (node environment), npm. Plain CSS. No new crate and no new npm package. The only new files from outside are two OFL font families.

**Spec:** `docs/superpowers/specs/2026-11-10-play-experience-design.md` (the design) and `docs/superpowers/specs/2026-11-10-play-experience-data-shape.md` (the shape). Read both. The shape wins on mechanics. ADRs 0001 to 0005 bind. Terms in capitals are in `GLOSSARY.md`. Deferred findings to close: `docs/superpowers/plans/2026-11-10-campaign-from-document-rulings.md`.

## Global Constraints

- Every Player screen reads `PcSight` (or a `view` type made from it), never `WorldView`. Only `get_debug` and `get_build_debug` show hidden content.
- The ledger gains no op. The Decision Proposal gains no field. No model call is added. The narrator gets exactly one new rule line, `speech::RULE`.
- A Note or a Pin never reaches a prompt and never changes a Commit. `PlayerStore` has its own connection, owned by the shell in `Open`. Nothing in `turn`, `context`, `checks`, `world` or `build` names it.
- No panel shows a number or a state the core does not hold. No Lead (the core holds none).
- A wrong speaker bubble is a failure. A line with no certain speaker stays prose.
- The 15 s Turn budget of ADR 0001 stands. View commands run after the Turn, never inside it.
- Schema version 3. A version 2 file is refused by `Store::open_existing`, with no migration. The home screen shows it as a card with a problem.
- The UI parses no Narration and matches no name against story text. `nameKey` gets no new use.
- One tokens file (`ui/src/tokens.css`) holds every colour, space, radius, type size and font family. No colour literal outside it.
- Fonts ship with the app under `ui/src/assets/fonts/`, each with its OFL licence file. No network fetch at runtime: no `http` URL in any CSS file.
- No new UI dependency. No component library, no CSS framework.
- The repository holds no adult text and no private text. Every fixture is invented.
- Everything committed is English. Short sentences. Never write the assistant's personal name; write "Claude Code".
- **The gate.** Each task ends green on all six, run from the repository root:
  `cargo fmt --check` · `cargo clippy --workspace -- -D warnings` · `cargo test --workspace` · `npm --prefix ui run typecheck` · `npm --prefix ui run lint` · `npm --prefix ui test`.
  The pre-commit hook runs the first five. Run `npm --prefix ui test` yourself. Do not skip the hook.

## Review Focus

1. **Accents and case in speech tags and name links.** `CHLOÉ: "…"` and "chloe" in prose must resolve to the entity "Chloé", as `normalize::name` does. Test owned by Task 5 (`speech_tags_and_links_match_through_normalize`).
2. **Quote styles and mark placement.** `"…"`, `“…”` and `«…»` are marks; a mark inside a paragraph, or with narration after the closing quote, is prose. Tests owned by Task 5 (`parse_accepts_three_quote_styles`, `parse_keeps_a_mark_inside_a_paragraph_as_prose`).
3. **Probing `get_entity_card` with ids.** An id of a hidden entity and an id that does not exist must get the same refusal text, so the Player cannot learn that a hidden entity exists. Test owned by Task 6 (`card_refusal_does_not_tell_hidden_from_missing`).
4. **Note bodies at the edges.** Empty, whitespace only, exactly `NOTE_MAX_CHARS` characters, one more, emoji counted as characters, a Note about a Note. Expect a clear error and no write. Test owned by Task 8 (`note_body_rules`).
5. **A Correction after the Player pins or notes the corrected Turn**, and a Pin on a Belief that a Correction removed. Expect the Pin and Note to follow the Correction Turn; the lost Belief's Pin hides and its Note shows as a free Note. Test owned by Task 8 (`pin_follows_the_correction_and_lost_target_hides`).

---

## File structure

```
ui/src/tokens.css, fonts.css                 NEW (T1): the one tokens file; @font-face
ui/src/assets/fonts/                         NEW (T1): source-serif-4-*.woff2, inter-*.woff2, OFL-*.txt
ui/src/play/                                 NEW (T1): PlayScreen, TopBar, Story, SidePanel, ActionBar, PlayPreview, logic.ts
ui/src/fixtures/playView.ts                  NEW (T1): one invented PlayView
ui/src/api.ts                                view types of shape 3.6 (T1), new commands (T5-T10)
ui/src/journal/, ui/src/home/                NEW (T7, T9)
src-tauri/src/drafts.rs                      NEW (T2): Drafts moves out of session.rs
src-tauri/tests/support/mod.rs               NEW (T2): shared test helpers
crates/threshold-core/src/view/mod.rs        NEW (T2): screen types and functions
crates/threshold-core/src/world/sight.rs     NEW (T4): PcSight, the only door from WorldView
crates/threshold-core/src/speech.rs          NEW (T5): RULE and parse
crates/threshold-core/src/player/mod.rs      NEW (T3 types and SCHEMA, T8 PlayerStore)
crates/threshold-core/src/domain/types.rs    outcome_key, is_alias_key, alias_key (T2)
crates/threshold-core/src/store/             schema 3, Knowledge.turn (T3), peek (T9)
crates/threshold-core/src/build/             AgeMark, rename, confirm_age, check_source_text, Cut (T10)
crates/threshold-core/tests/                 hidden.rs support, sight.rs, view.rs, speech.rs, player.rs (new)
crates/threshold-eval/src/speech.rs          NEW (T11)
```

Test style as in the previous plans: each test has a name and the behaviour it pins. Write it, run it and see it fail, implement, run it and see it pass, commit. The UI tests run in Vitest's node environment: they test pure functions and fixtures, not the DOM.

---

### Task 1: The Play screen on fixture data (L) — HUMAN GATE

No core change. The owner approves the look here. **Execution stops after this task until the owner approves or asks for changes.**

**Files:**
- Create: `ui/src/tokens.css`, `ui/src/fonts.css`, `ui/src/assets/fonts/{source-serif-4-roman.woff2, source-serif-4-italic.woff2, inter-roman.woff2, OFL-source-serif-4.txt, OFL-inter.txt}`, `ui/src/play/{PlayScreen,TopBar,Story,SidePanel,ActionBar,PlayPreview}.tsx`, `ui/src/play/logic.ts`, `ui/src/fixtures/playView.ts`.
- Modify: `ui/src/api.ts` (types only), `ui/src/style.css` (reads tokens; its `:root` block and the light-theme media query move out), `ui/src/main.tsx` (dev-only preview route).
- Test: `ui/src/play/logic.test.ts`, `ui/src/fixtures/playView.test.ts`, `ui/src/theme.test.ts` (all new).

**Interfaces:**
- Produces (TypeScript mirror of shape 3.6, snake_case fields, every tagged enum tagged by `kind` with snake_case values): `PlayView`, `NameRef`, `PlaceLine`, `StoryTurn`, `TurnBody = {kind:"narration", blocks: Block[]} | {kind:"missing"}`, `Block = {kind:"prose", parts: Part[]} | {kind:"speech", speaker: NameRef, parts: Part[]}`, `Part = {kind:"text", text} | {kind:"name", entity_id, text}`, `YouCard`, `StateLine`, `FactLine`, `BeliefRef`, `KnownLine`, `PinLine`, `SceneState = {kind:"plain"}`, `Target` (shape 3.7), `TurnKind = "setup"|"play"|"correction"|"ooc"|"opening"`. Task 5 must serialize the Rust types to exactly this.
- Produces in `play/logic.ts`: `npcColor(entityId: number): string` (returns `var(--npc-N)`, N = `entityId % 8 + 1`); `nameText(who: NameRef): string` ("unknown" when `name` is null); `turnPhase(busy: boolean, stream: string): "idle" | "deciding" | "narrating"`; `shouldFollow(scrollTop: number, clientHeight: number, scrollHeight: number): boolean` (true within 24 px of the bottom); `isSendKey(e: {key: string; shiftKey: boolean}): boolean`; `storyItems(view: PlayView, stream: string | null): StoryItem[]` with `StoryItem = {kind: "turn"; turn: StoryTurn} | {kind: "stream"; text: string}`; the last item is the stream item while `stream` is not null.

**Choices fixed here (the owner can change them at the gate):**
- Fonts: **Source Serif 4** for the story, **Inter** for the interface. Both are SIL OFL 1.1. Take the variable WOFF2 files and the licence text from the official release archives (`github.com/adobe-fonts/source-serif/releases`, `github.com/rsms/inter/releases`), once, at implementation time. Check that each archive's licence is OFL 1.1 before committing. Commit the files. Nothing is fetched at runtime.
- Palette tokens (dark by default): `--ground #0e1320`, `--card #151c2b`, `--card-border #253049`, `--text #e4e8f0`, `--muted #95a0b4`, `--accent #4f8cff`, `--accent-text #ffffff`, `--rule #222b3d`, `--danger #e5675c`, `--ok #5cc28a`, and eight warm NPC colours `--npc-1` to `--npc-8`: `#e8a15c #e07a5f #d9a46b #f0c35a #e5989b #c98a6b #f4a261 #d6b07a`. Spacing `--space-1..6` (4, 8, 12, 16, 24, 32 px), radius `--radius-1..3` (4, 8, 999 px), type sizes `--text-xs..xl`, `--font-story`, `--font-ui`, `--focus-ring`.
- Layout: direction B of design 5. Side panel 248 px. Narrow-window breakpoint 760 px: below it the panel folds under a "Panel" button. Media queries cannot read a custom property, so `760px` appears once in `style.css` with a comment that names the token.
- The preview: in a dev build only, `main.tsx` renders `PlayPreview` when the URL has `?preview=play`. The owner runs `npm --prefix ui run dev` and opens `http://127.0.0.1:5173/?preview=play` in any browser. `PlayPreview` has two buttons: "Replay the last Turn's stream" (feeds the fixture's raw stream text in chunks, with marks visible, then swaps to the blocks) and "Busy" (shows the deciding and narrating states).

**Must NOT:** change any Rust file; call Tauri from the preview; add an npm package; fetch anything over the network; show a gauge or a state the fixture's `you.state` does not hold; parse Narration text or match names in the UI (the fixture already holds blocks and name parts); put real or adult content in the fixture.

- [ ] **Step 1: Write the failing tests.**
  - `logic.test.ts › npcColor gives each NPC one stable colour`: `npcColor(7) === npcColor(7)`; ids 1 to 8 give 8 different values; `npcColor(9) === npcColor(1)`.
  - `logic.test.ts › an unnamed character reads as unknown`: `nameText({entity_id: 9, kind: "npc", name: null}) === "unknown"`.
  - `logic.test.ts › the stream shows raw text until the Turn commits`: with `stream = 'Orsel: "Late."'` the last item is `{kind: "stream", text: 'Orsel: "Late."'}`; with `null` no stream item exists and each `StoryTurn` gives one item.
  - `logic.test.ts › the bar says deciding, then narrating`: `turnPhase(true, "") === "deciding"`, `turnPhase(true, "x") === "narrating"`, `turnPhase(false, "") === "idle"`.
  - `logic.test.ts › the story follows the stream unless the Player scrolled up`: `shouldFollow(976, 24, 1000) === true`; `shouldFollow(500, 24, 1000) === false`.
  - `logic.test.ts › Enter sends and Shift+Enter does not`.
  - `playView.test.ts › the fixture holds every case the owner must see`: at least one `speech` block; one `prose` block whose text holds a quote (the unsure line); one `here` entry with `name: null`; two or more `open` lines; one `pinned` line; one `reverted` StoryTurn; one StoryTurn with a `roll`; one `missing` body; a `streamText` export that holds a speech mark.
  - `playView.test.ts › every name part points at a known entity`: each `Part` of kind `name` has an `entity_id` found in `known`, `here` or `you`.
  - `theme.test.ts › no colour outside the tokens file`: load every `src/**/*.css` with `import.meta.glob(..., {query: "?raw", import: "default", eager: true})`; no file except `tokens.css` holds `#[0-9a-f]{3,8}\b` or `rgb(`.
  - `theme.test.ts › fonts ship with the app`: no CSS file holds `http`; each `url(...)` in `fonts.css` names a file found by `import.meta.glob("./assets/fonts/*.woff2")`; each font family has an `OFL-*.txt` next to it.
- [ ] **Step 2: Run, expect FAIL.** `npm --prefix ui test`
- [ ] **Step 3: Implement.** Components read the fixture through props, so Task 5 swaps the source only. The action bar is one component, `ActionBar`, with props `scene: SceneState` and `phase`, and an empty named slot `<div className="action-bar__later">` on its right for the mature content module. "Ask the GM" opens an OOC field next to the Intent field; its answer shows in a box apart from the story. Every button and link has a `:focus-visible` ring from `--focus-ring`. The story column uses `--font-story`; everything else `--font-ui`.
- [ ] **Step 4: Run the gate, expect PASS.** Also `npm --prefix ui run build`: the built `dist/` holds the three `.woff2` files.
- [ ] **Step 5: Look at it.** Run the preview. Check by keyboard only: Tab reaches every control, Enter sends, the panel button works below 760 px. Take three screenshots (wide, narrow, streaming) for the owner.
- [ ] **Step 6: Commit** `feat(ui): Play screen on fixture data, tokens and bundled fonts`.
- [ ] **Step 7: HUMAN GATE.** The controller sends the owner the screenshots and the preview command. Stop. Resume Task 2 only after the owner approves the look. Record any change he asks for as a fix commit in this task.

**Done when:** the gate is green, the preview shows every fixture case, and the owner approved the look.

---

### Task 2: Structure only (M)

No new behaviour. Every existing test keeps its expectations.

**Files:**
- Create: `src-tauri/src/drafts.rs`, `src-tauri/tests/support/mod.rs`, `crates/threshold-core/src/view/mod.rs`.
- Modify: `src-tauri/src/{lib,session}.rs`, `src-tauri/tests/commands.rs`, `crates/threshold-core/src/{lib.rs, domain/types.rs, domain/mod.rs, store/mod.rs, build/draft.rs, build/checks.rs, context/narration_prompt.rs}`, `crates/threshold-core/tests/world.rs:545`.
- Test: `crates/threshold-core/tests/domain.rs`.

**Interfaces:**
- Produces in `domain`: `pub fn outcome_key(key: &str) -> Option<&str>` (strips `OPEN_KEY_PREFIX`; `None` when the key has no prefix or nothing after it); `pub const ALIAS_KEY_PREFIX: &str = "alias_"`; `pub fn is_alias_key(key: &str) -> bool`; `pub fn alias_key(other_name: &str) -> String` (`"alias_" + normalize::name(other_name)` with spaces as `_`). The shape names `is_alias_key` only; `alias_key` is the writer side, so no site writes the prefix by hand.
- Produces in `view` (temporary signatures, Task 4 and Task 7 replace them): `CharacterView`, `CharacterFact`, `JournalEntry`, `JournalView` move from `session.rs` with the same fields; `pub fn character_view(view: &WorldView) -> Option<CharacterView>`; `pub fn legacy_journal(view: &WorldView) -> JournalView`.
- Produces in `src-tauri`: `drafts.rs` holds `Drafts`, `DraftView`, `DraftLine`, `Job`, `JobSlot`, `job_error`, `draft_error`, `commit_failure`. `session.rs` keeps `Session` and the Turn types.

**Must NOT:** change any serialized output, any command name, any prompt text, or any test expectation.

- [ ] **Step 1: Write the failing tests.**
  - `domain.rs::outcome_key_strips_the_open_prefix_only`: `outcome_key("pending_trip") == Some("trip")`; `outcome_key("trip") == None`; `outcome_key("pending_") == None`.
  - `domain.rs::alias_key_is_the_one_writer`: `alias_key("Jade Moss") == "alias_jade_moss"`; `alias_key("Chloé") == "alias_chloe"`; `is_alias_key(&alias_key("Jade"))`; `!is_alias_key("location")`.
- [ ] **Step 2: Run, expect FAIL.** `cargo test -p threshold-core --test domain`
- [ ] **Step 3: Implement.** Replace the three hand-written `alias_` keys (`store/mod.rs:435`, `build/draft.rs:404`, `build/checks.rs:273`) with `alias_key`, and `key_term` in `narration_prompt.rs:175` with `outcome_key(key).unwrap_or(key)`. Move the Drafts code and the shared shell test helpers (`session`, `town`, `base`, `allowed`, `with_fact`, `town_dir`, `entity`) to their new files. In `tests/world.rs:545` write `const { assert!(WAIT_DEFAULT_MIN >= 1) };`.
- [ ] **Step 4: Run the gate, expect PASS.** Then run `cargo clippy --workspace --all-targets -- -D warnings`. Fix each further finding that is a lint fix of a few lines. Record any larger one in the commit body and leave it.
- [ ] **Step 5: Commit** `refactor: Drafts in its own file, one owner for open and alias keys, view module`.

**Done when:** the gate is green, `grep -rn '"alias_' crates src-tauri` finds only `ALIAS_KEY_PREFIX`, and `session.rs` holds no draft code.

---

### Task 3: Schema 3 (S)

No new behaviour.

**Files:**
- Create: `crates/threshold-core/src/player/mod.rs` (types and `SCHEMA` only).
- Modify: `crates/threshold-core/src/{lib.rs, store/mod.rs, store/migrations.rs, store/project.rs}`, tests and fixtures that build `Knowledge` or check the version.
- Test: `crates/threshold-core/tests/store.rs`.

**Interfaces:**
- Produces: `Knowledge { holder, fact_id, claim_id, how, learned_at, arrives_at, pub turn: TurnId }`, from the new column `knowledge.turn_id INTEGER NOT NULL REFERENCES turn(id)`, written by `project::apply` for each `Know` op and kept by `rebuild`. `pub(crate) const SCHEMA_VERSION: u32 = 3` in `store`.
- Produces in `player` (shape 3.7): `pub const SCHEMA: &str` (tables `player_note` and `player_pin`; one Pin per target; the builder writes the SQL); `pub enum Target { Scene { turn_id }, Act { turn_id }, Belief { subject_id, key }, Entity { entity_id }, Note { note_id: u64 } }` with `#[serde(tag = "kind", rename_all = "snake_case")]`; `pub struct Note { pub id: u64, pub body: String, pub about: Option<Target>, pub created_ms: u64, pub updated_ms: u64 }`; `#[derive(Default)] pub struct Marks { pub notes: Vec<Note>, pub pins: Vec<Target> }`. `store::migrations` runs `player::SCHEMA` in the version-0 batch.

**Must NOT:** read or write the Player tables from any store function; add a migration from version 2.

- [ ] **Step 1: Write the failing tests.**
  - `store.rs::knowledge_rows_carry_their_turn`: commit a Turn with a fact the PC witnesses; the PC's knowledge row for it has `turn` equal to that Turn's id; after `rebuild_projections`, the same.
  - `store.rs::version_2_file_is_refused`: a database at `user_version` 2 fails `Store::open_existing` with a message that names version 2; the file bytes are unchanged.
  - `store.rs::new_file_has_the_player_tables`: a new file has `user_version` 3 and the tables `player_note` and `player_pin`.
- [ ] **Step 2: Run, expect FAIL.** `cargo test -p threshold-core --test store`
- [ ] **Step 3: Implement.** Update `version_1_file_is_refused` only where it names the current version.
- [ ] **Step 4: Run the gate, expect PASS.**
- [ ] **Step 5: Commit** `feat(store): schema 3 with knowledge.turn_id and the Player tables`.

**Done when:** the gate is green and no store function names `player_`.

---

### Task 4: `PcSight` (L)

**Files:**
- Create: `crates/threshold-core/src/world/sight.rs`, `crates/threshold-core/tests/support/hidden.rs`, `crates/threshold-core/tests/sight.rs`, `crates/threshold-core/tests/view.rs`.
- Modify: `world/{mod,beliefs}.rs`, `context/mod.rs` (`seen_present` becomes `pub`), `turn.rs` (`stored_action`), `normalize.rs` (`name_words`), `view/mod.rs`, `src-tauri/src/session.rs` (`get_character`).
- Test: `tests/sight.rs`, `tests/view.rs`, `tests/turn.rs`.

**Interfaces:**
- Consumes: Task 3 (`Knowledge.turn`), Task 2 (`outcome_key`, `is_alias_key`).
- Produces (shape 2, 3.2 to 3.5): `pub fn pc_sight(view: &WorldView, turns: &[StoredTurn], reverted: &HashSet<TurnId>) -> Option<PcSight>` (callers convert `Store::reverted_turns`'s `BTreeSet`); `Moment`, `Known`, `OpenItem` exactly as the shape writes them; `pub const SCENE_GAP_MIN: ClockMin = 240`; `pub fn stored_action(proposal_json: &str) -> Option<(ActionKind, Verdict)>` in `turn` (reads `record.proposal.action` and `.verdict`); `pub fn name_words(text: &str) -> Vec<String>` in `normalize` (lowercase, accents dropped as `name` does, split on every non-alphanumeric char).
- Produces, filling what the shape leaves open:
  ```rust
  pub struct PcSight {
      pub pc: EntityId, pub pc_name: String, pub concept: String, pub clock_min: ClockMin,
      pub place: Option<(Option<EntityId>, String)>,  // place entity the PC can name, and the location text
      pub present: Vec<EntityId>,                     // seen_present now, the PC included
      pub known: Vec<Known>,                          // the PC, each present entity, each subject or speaker of a kept Belief
      pub beliefs: Vec<Belief>,                       // PC Beliefs now, Setup rows included, none about an unnamed entity
      pub before: Vec<Belief>,                        // "Known before the Campaign": rows of Setup Turns
      pub moments: Vec<Moment>,                       // one per standing Opening, Play and Correction Turn
      pub open_items: Vec<OpenItem>,
      pub story: Vec<SightTurn>,                      // Opening, Play, Correction Turns, reverted ones included
      pub replaced_by: BTreeMap<TurnId, TurnId>,      // reverted Turn -> the Correction Turn that replaced it
      pub needs_begin: bool, pub can_correct: bool,
  }
  pub struct SightTurn { pub turn: TurnId, pub kind: TurnKind, pub intent: String, pub roll: Option<RollResult>,
                         pub reverted: bool, pub narration: Option<String> }
  impl PcSight { pub fn scenes(&self) -> Vec<&[Moment]>; }
  ```
  `replaced_by`: a reverted Turn maps to the first standing Correction Turn after it in `seq` order. `can_correct`: the latest Turn of the campaign is a standing Play Turn (the rule of `Engine::correct_last`). `pc_sight` keeps no Belief whose subject is a `Known` with `name: None`: its only Belief is a witnessed `location`, already in `present`, and its text holds the name.
- Produces in `world::beliefs`: one private function that holds the Belief rule (latest `learned_at` wins, only arrived rows count) and returns each Belief with its knowledge row. `beliefs` and `pc_sight` both call it.
- Produces in `view`: `pub fn character_view(sight: &PcSight) -> CharacterView` (same JSON as before). `Session::get_character` builds the `PcSight`, then calls it.

**Must NOT:** read `Entity.other_names` in `sight.rs` or `view` (other names come from `alias_` Beliefs only); add committed state; let `view` name `WorldView` or `Store`.

- [ ] **Step 1: Write the hidden-world fixture.** `tests/support/hidden.rs`, `HiddenWorld::new()`, built on the town gate of `tests/support/mod.rs`, invented content only. It adds: an other name of Orsel, "the Toll-Wolf", known to Brann only; a seed entry "Vey", `hidden: true`, at the gate square; a stranger NPC "Kestrel" added by a play Turn at the gate square with the PC as witness, never named in any Narration; three Narrations stored with `set_narration` that name Orsel and Brann only; one open item the PC knows (`ada.pending_meet = Brann at the mill`). It exports `HIDDEN: &[&str] = &["gate key", "Keep Ada out of town", "Orsel rings the bell", "Toll-Wolf", "Vey", "Kestrel", "owes", "Pip"]`.
- [ ] **Step 2: Write the failing tests.**
  - `sight.rs::pc_sight_holds_no_hidden_world` (shape): `format!("{sight:?}")` holds no string of `HIDDEN`.
  - `sight.rs::moment_matches_seen_present_of_snapshot_before` (shape): on a campaign of six Turns with one move and one arrival, for each Moment `m`, `m.present == seen_present(&store.snapshot_before(c, seq(m.turn) + 1))`.
  - `sight.rs::pc_names_by_seed_belief_or_narration`: the PC, a non-hidden seed entry and a Belief subject have a `name`; the stranger has `name: None` until a standing Narration holds "Kestrel"; "Rose" does not match "Roseanne"; "Chloé" matches "chloe"; a reverted Turn's Narration does not count.
  - `sight.rs::other_names_come_from_alias_beliefs_only`: Orsel's `Known.other_names` is empty for the PC; after a Turn where the PC witnesses the alias fact, it holds "the Toll-Wolf".
  - `sight.rs::learned_rows_land_in_their_moment`: a Claim with `delay_min: 90` lands in the first Moment whose `clock_after` reaches its `arrives_at`; a Setup row is in `before` and in no Moment; a witnessed `location` of another entity is in no `learned`.
  - `sight.rs::scenes_break_on_place_change_and_long_gaps`: a move starts a Scene; a Turn of 241 minutes starts one; a Turn of 240 does not.
  - `sight.rs::open_item_closes_on_a_fact_not_a_claim`: a Claim on `(ada, meet)` leaves the item open; a new value on `pending_meet` changes its `text` and keeps it open; a fact on `(ada, meet)` the PC witnesses sets `closed_in` to that Turn.
  - `sight.rs::offscreen_outcome_keeps_the_item_open` (shape): a fact on `(ada, meet)` with no PC witness leaves `closed_in: None`.
  - `sight.rs::decision_rule_marks_lasting_witnessed_acts`: one case per shape 3.2 line. `talk` with a Roll: true. `act` that writes a PC-witnessed fact from `proposal`: true. `act` that writes a pending fact the PC knows: true. `move` with only a `location` fact: false. `act` with verdict `impossible`: false. `observe`: false. A reverted `act`: false. The Opening Turn: false.
  - `sight.rs::correction_replaces_the_reverted_turn`: after `correct_last`, `replaced_by[old] == new` and `can_correct` is false.
  - `turn.rs::stored_action_reads_the_proposal_record`: a stored Play Turn gives `Some((Talk, Allowed))`; `"{}"` gives `None`.
  - `view.rs::character_view_shows_pc_knowledge_only` (shape): on the hidden world, the JSON holds no string of `HIDDEN`, and it equals the JSON `Session::get_character` gave before this task for the town fixture.
  - `view.rs::view_reads_no_world_view`: the files under `src/view/` hold neither `WorldView` nor `Store`.
- [ ] **Step 3: Run, expect FAIL.** `cargo test -p threshold-core --test sight --test view --test turn`
- [ ] **Step 4: Implement.** Moment `place` comes from the PC's `location` facts and their `created_turn`. Moment `present` applies the `seen_present` rule to the Beliefs the PC held after that Turn: rows with a Turn of `seq` up to it and `arrives_at <= clock_after`.
- [ ] **Step 5: Run the gate, expect PASS.**
- [ ] **Step 6: Commit** `feat(world): PcSight, the one door from WorldView to the screens`.

**Done when:** the gate is green and `get_character` reads `PcSight`.

---

### Task 5: `speech`, `play_view`, `get_play_view` (L)

**Files:**
- Create: `crates/threshold-core/src/speech.rs`, `crates/threshold-core/tests/speech.rs`, `src-tauri/tests/play.rs`, `ui/src/play/session.ts`, `docs/adr/0006-speaker-mark.md` (the ADR of shape section 6, in the form of ADR 0005).
- Modify: `lib.rs`, `context/narration_prompt.rs` (`NARRATION_RULES`), `world/sight.rs` (resolved lines), `view/mod.rs`, `src-tauri/src/{lib,session,drafts}.rs`, `ui/src/{api.ts, App.tsx, views/Debug.tsx}`, `ui/src/play/*`, `ui/src/views/Play.tsx` (deleted).
- Test: `tests/speech.rs`, `tests/view.rs`, `tests/context.rs`, `tests/turn.rs`, `src-tauri/tests/play.rs`, `ui/src/api.test.ts`, `ui/src/play/session.test.ts`.

**Interfaces:**
- Consumes: Task 4, Task 3 (`Marks`), Task 1 (UI types and components).
- Produces in `speech` (shape 2, 3.1): `pub const RULE: &str` (one line); `pub fn parse(narration: &str) -> Vec<Line<'_>>` with `pub enum Line<'a> { Prose(&'a str), Tagged { name: &'a str, words: &'a str } }`. A mark is a whole line: a name of one to four words, a colon, then the spoken words in `"…"`, `“…”` or `«…»`, and nothing after the closing quote. First wording of `RULE`: `When an NPC speaks, put each spoken line on a line of its own: the NPC's name, a colon, then the words in quotes.` Task 11 measures it.
- Produces in `sight.rs`: `SightTurn.lines: Vec<SightLine>` with `pub enum SightLine { Prose(String), Speech { speaker: EntityId, words: String } }`. `pc_sight` resolves each tag by the three rules of shape 3.1, because rule 1 needs `WorldView::entities_by_name` and `view` has no `WorldView`. A tag that fails a rule gives a `Prose` line with the text unchanged.
- Produces in `view` (shape 3.6, 3.9): `pub fn play_view(sight: &PcSight, marks: &Marks) -> PlayView` and every type of shape 3.6 that `PlayView` holds, `Serialize`, with `TurnBody`, `Block`, `Part` and `SceneState` tagged by `kind`, snake_case; `pub const PLAY_VIEW_MAX_MS: u64 = 50`. Name parts: whole-word matches (`normalize::name_words`) of the main name of each `Known` with a `name` and a kind of NPC, place or Faction, in Prose and Speech text. A play fact text of the form `X: key = value` reads as `X key words: value` (`Orsel mood: annoyed`); any other text stays.
- Produces in the shell: command `get_play_view() -> PlayView` (with `Marks::default()` until Task 8); `campaign_summary` removed; `CampaignSummary` and the `start_build` result lose `turns`.
- Produces in the UI: `api.getPlayView()`; `createPlaySession(api)` in `play/session.ts` with `opened()`, `turnEnded()`, `noteChanged()`, each calling `get_play_view` once. The Debug view takes Turn ids from `PlayView.story`.

**Must NOT:** add a model call or a proposal field; change any prompt except the one `RULE` line in `NARRATION_RULES`; show a bubble or a link during the stream (raw text, marks included); parse Narration in the UI; show a Lead; call a view inside the Turn.

- [ ] **Step 1: Write the failing tests.**
  - `speech.rs::parse_reads_a_full_speech_line`: `Orsel: "You are late."` gives `Tagged { name: "Orsel", words: "You are late." }`.
  - `speech.rs::parse_accepts_three_quote_styles` (Review Focus 2).
  - `speech.rs::parse_keeps_a_mark_inside_a_paragraph_as_prose` (Review Focus 2): a quote mid-paragraph, a mark followed by `he says.`, `Orsel: no quotes`, a name of five words; each is `Prose` with its text unchanged.
  - `speech.rs::rule_is_one_line`: `RULE` holds no `\n`.
  - `context.rs::narration_rules_add_one_line_only`: `NARRATION_RULES` has 8 lines and its last line is `RULE`; the opening prompt holds `RULE`; the OOC and proposal prompts do not.
  - `turn.rs::turn_makes_the_same_model_calls` (shape; design 11 "Turn time does not grow"): one `play_turn` on `RecordedGm` records exactly `[Proposal, Narration]`; `begin` records `[Narration]`.
  - `view.rs::play_view_shows_pc_knowledge_only` (shape; design 11 test 1): the serialized `PlayView` of the hidden world holds no string of `HIDDEN`.
  - `view.rs::unnamed_character_is_shown_unknown` (shape; design 11 test 2): the stranger is in `here` with `name: null`, is in no `known` line, and has no fact line.
  - `view.rs::speech_line_without_a_certain_speaker_stays_prose` (shape; design 11 test 5): a tag shared by two entities (one of them hidden), a tag of an absent NPC, a tag of the stranger, and a tag of the PC each give one `Prose` block with the line's text; a tag of present, named Orsel gives a `Speech` block.
  - `view.rs::speech_tags_and_links_match_through_normalize` (Review Focus 1).
  - `view.rs::open_item_leaves_open_in_the_turn_that_closes_it` (shape; design 11 test 3): the item is in `open` after the Turn that opens it and after one unrelated Turn; it is absent after the Turn whose PC-witnessed fact closes it.
  - `view.rs::play_view_json_matches_the_ui_types`: `story[i].body.kind == "narration"`, `blocks[j].kind == "speech"`, `parts[k].kind == "name"`, `scene.kind == "plain"`; field names as `ui/src/api.ts`.
  - `view.rs::reverted_turn_keeps_prose_only` and `view.rs::you_card_holds_state_keys_only` (no `location`, no gauge from a non-state key).
  - `play.rs::play_view_builds_within_budget_at_300_turns`: 300 committed Turns with Narrations; one `get_play_view` call, timed, is at most `PLAY_VIEW_MAX_MS` in a release build and 10 times that in a debug build (`cfg!(debug_assertions)`). Run it once with `--release` and write the number in the commit body.
  - `play.rs::get_play_view_after_open_and_after_a_turn`: the story grows by one Turn; `needs_begin` turns false after `begin`.
  - `api.test.ts › getPlayView calls get_play_view`; `session.test.ts › one view call after open, after each Turn result and after each note command`.
- [ ] **Step 2: Run, expect FAIL.** `cargo test --workspace` and `npm --prefix ui test`
- [ ] **Step 3: Implement.** Wire `PlayScreen` to live data in `App.tsx`; it replaces `views/Play.tsx`. Keep Begin, Retry, renarrate, Correct last Turn (a quiet outline button until Task 9) and OOC. Port every shell test that reads `CampaignSummary.turns` (among them `turn_streams_and_reopens`, `summary_marks_reverted_turns`, `opening_survives_a_restart`) to `get_play_view.story`, with the same assertions. Write ADR 0006.
- [ ] **Step 4: Run the gate, expect PASS.**
- [ ] **Step 5: Run the app** (`npm --prefix ui run tauri dev`, town fixture through the `new_campaign` command or a built Campaign). Play three Turns. Report what you saw, including anything wrong.
- [ ] **Step 6: Commit** `feat: speech marks, play_view and the live Play screen`.

**Done when:** the gate is green and the live Play screen shows bubbles only for certain speakers.

---

### Task 6: `get_entity_card` and "People and places" (S)

**Files:**
- Modify: `view/mod.rs`, `src-tauri/src/{lib,session}.rs`, `ui/src/{api.ts}`, `ui/src/play/{SidePanel,Story,logic}.tsx/ts`.
- Test: `tests/view.rs`, `src-tauri/tests/play.rs`, `ui/src/play/logic.test.ts`.

**Interfaces:**
- Consumes: Task 5.
- Produces: `pub fn entity_card(sight: &PcSight, marks: &Marks, entity: EntityId) -> Option<EntityCard>` (shape 3.6); command `get_entity_card(entity_id) -> EntityCard`, refused with one fixed text, `"the PC does not know this one"`, for any id not in `PcSight.known`; `searchFold(text: string): string` (lowercase, NFD, marks removed) and `filterKnown(lines: KnownLine[], query: string): KnownLine[]` (main name or other name holds the folded query) in `play/logic.ts`.

**Must NOT:** let the refusal text differ between a hidden id and a missing id; give a stranger card any text; use `nameKey`.

- [ ] **Step 1: Write the failing tests.**
  - `view.rs::entity_card_shows_pc_knowledge_only` (shape): Orsel's card JSON holds no string of `HIDDEN`; its `facts` are the PC Beliefs about Orsel, open items marked.
  - `view.rs::entity_card_refuses_an_entity_the_pc_does_not_know` (shape): `None` for Vey, Pip and an id above every entity id.
  - `view.rs::stranger_card_holds_no_text`: the stranger's card has `who.name == null`, no facts and no other names.
  - `play.rs::card_refusal_does_not_tell_hidden_from_missing` (Review Focus 3): the error text for Vey's id equals the one for id 99,999.
  - `logic.test.ts › filterKnown matches main and other names, case and accents aside`.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement.** A click on a name part, a `here` line or a `known` line opens the card in the panel. **Step 4: Run the gate, expect PASS.**
- [ ] **Step 5: Commit** `feat: entity cards and People and places`.

**Done when:** the gate is green and every name link opens a card.

---

### Task 7: `journal_view` and the Journal screen (M)

**Files:**
- Create: `ui/src/journal/{Journal.tsx, logic.ts, logic.test.ts}`.
- Modify: `view/mod.rs` (`legacy_journal` deleted), `src-tauri/src/session.rs`, `src-tauri/tests/commands.rs` (four `journal_*` tests move to the new form), `ui/src/{api.ts, App.tsx}`, `ui/src/views/Journal.tsx` (deleted).
- Test: `tests/view.rs`, `src-tauri/tests/play.rs`, `ui/src/journal/logic.test.ts`.

**Interfaces:**
- Consumes: Task 4 (`scenes`, `before`, `open_items`), Task 5.
- Produces: `pub fn journal_view(sight: &PcSight, marks: &Marks) -> JournalView` with `JournalView`, `SceneEntry`, `Act`, `ClosedItem` of shape 3.6; `get_journal` returns it; `decisions(view: JournalView): {act: Act; scene: SceneEntry}[]` in `journal/logic.ts`.

**Must NOT:** call a model to write an entry; show a reverted Turn in a Scene entry; list Setup knowledge under a Scene.

- [ ] **Step 1: Write the failing tests.**
  - `view.rs::journal_view_shows_pc_knowledge_only` (shape).
  - `view.rs::seed_knowledge_is_known_before` (shape): a known fact from the seed is in `known_before` and in no `learned` (closes the QA minor "imported knowledge listed as what the PC saw").
  - `view.rs::scene_entry_holds_place_time_people_acts_and_learned`: two Scenes from a move; `clock_from` and `clock_to` match the Moments; `present` excludes the PC.
  - `view.rs::closed_item_names_the_turn_that_closed_it`.
  - `view.rs::reverted_turn_is_in_no_scene_entry`.
  - `logic.test.ts › the Decisions tab lists decision acts with their scene's place and time`.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement** the screen with five parts: Timeline, Decisions, Open and closed, Known before the Campaign, Notes (empty until Task 8). **Step 4: Run the gate, expect PASS.**
- [ ] **Step 5: Commit** `feat: the Journal from PcSight`.

**Done when:** the gate is green and the Journal shows no "what the PC saw" list of seed facts.

---

### Task 8: Notes and Pins (M)

**Files:**
- Modify: `crates/threshold-core/src/player/mod.rs`, `view/mod.rs` (Marks join), `src-tauri/src/{lib,session}.rs` (`Open` gains `player: PlayerStore`), `ui/src/{api.ts}`, `ui/src/journal/*`, `ui/src/play/SidePanel.tsx`.
- Create: `crates/threshold-core/tests/player.rs`, `src-tauri/tests/notes.rs`.

**Interfaces:**
- Consumes: Task 3 (types, tables), Tasks 5 to 7 (views take `&Marks`).
- Produces (shape 2, 3.7; the shape elides the arguments, these fix them):
  ```rust
  pub const NOTE_MAX_CHARS: usize = 2_000;
  pub enum PlayerError { Empty, TooLong { max: usize }, AboutANote, NoSuchNote(u64), Store(String) }
  impl PlayerStore {
      pub fn open(path: &Path) -> Result<Self, PlayerError>;
      pub fn marks(&self) -> Result<Marks, PlayerError>;
      pub fn add_note(&self, body: &str, about: Option<Target>, now_ms: u64) -> Result<Note, PlayerError>;
      pub fn edit_note(&self, id: u64, body: &str, now_ms: u64) -> Result<Note, PlayerError>;
      pub fn delete_note(&self, id: u64) -> Result<(), PlayerError>;   // also removes the Note's Pin
      pub fn set_pin(&self, target: &Target, on: bool) -> Result<(), PlayerError>;
  }
  ```
  Commands `add_note(body, about) -> Note`, `edit_note(note_id, body) -> Note`, `delete_note(note_id)`, `set_pin(target, on)`. The shell stamps `now_ms` from the system clock. `view` resolves targets: a Target on a reverted Turn follows `replaced_by`; a Target that no longer resolves hides its Pin and shows its Note as a free Note.

**Must NOT:** give the `Engine` a `PlayerStore` or `Marks`; send any Note text to the `Engine`; let `rebuild_projections` touch the Player tables; add a lock for `PlayerStore` (the `Session` lock orders writes).

- [ ] **Step 1: Write the failing tests.**
  - `notes.rs::note_and_pin_survive_a_restart` (shape; design 11 test 4a): add a Note about Orsel and pin it; drop the `Session`; a new one opens the file; `get_play_view.pinned` and `get_entity_card(orsel).notes` hold them.
  - `notes.rs::note_text_is_in_no_prompt_of_the_next_turn` (shape; design 11 test 4b): a Note with body `MARKER-7f3a`, then one Turn and one OOC call; no field of any `RecordedGm::calls()` prompt and no `debug_log` row of either op id holds the marker.
  - `player.rs::no_turn_module_names_the_player_store` (shape): the sources under `src/turn.rs`, `src/context/`, `src/checks/`, `src/world/` and `src/build/` hold none of `PlayerStore`, `Marks`, `player::`, `player_note`, `player_pin`.
  - `player.rs::rebuild_keeps_notes_and_pins` (shape).
  - `player.rs::note_body_rules` (Review Focus 4): `""` and `"   "` give `Empty`; 2,000 characters pass; 2,001 give `TooLong { max: 2000 }`; 2,000 emoji pass; `about: Some(Target::Note{..})` gives `AboutANote`; each failure writes nothing.
  - `player.rs::delete_note_removes_its_pin`.
  - `view.rs::pin_follows_the_correction_and_lost_target_hides` (Review Focus 5): a Pin and a Note on an Act of a Turn that `correct_last` reverts show on the Correction Turn; a Pin on a Belief the Correction removed is not in `pinned`, and its Note is in `notes` with `about` unresolved.
  - `api.test.ts › note commands send their arguments`, and `session.test.ts` already covers the reload after a note command.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement** the UI: a Note field in the Journal's Notes part and on each entity card; a "Note" button in the Play screen's action bar that opens a quick Note field (a free Note, or about the current Scene), asked by the owner on 2026-11-10; a pin toggle on each Scene, Act, fact line, card and Note; the panel's Pinned card. **Step 4: Run the gate, expect PASS.**
- [ ] **Step 5: Commit** `feat: Player Notes and Pins, outside the world`.

**Done when:** the gate is green and the marker test passes.

---

### Task 9: Home and the Campaign menu (M)

**Files:**
- Create: `ui/src/home/{Home.tsx, logic.ts, logic.test.ts}`, `src-tauri/tests/home.rs`.
- Modify: `store/mod.rs` (`peek`), `src-tauri/src/{lib,session}.rs`, `ui/src/{api.ts, App.tsx, style.css}`, `ui/src/play/TopBar.tsx`, `ui/src/views/{Character,NewCampaign}.tsx` (tokens only).

**Interfaces:**
- Consumes: Task 3 (`SCHEMA_VERSION`), Task 5.
- Produces: `pub struct CampaignPeek { pub pc_name: String, pub premise: String, pub turn_count: u32, pub needs_begin: bool }`; `pub fn peek(path: &Path) -> Result<CampaignPeek, StoreError>` in `store` (opens read-only, runs no migration, refuses any version but 3 with a message that names it; `turn_count` counts standing Play and Correction Turns); `CampaignCard` of shape 3.6 in `session.rs`; `list_campaigns() -> Vec<CampaignCard>`, newest `last_played_ms` first, a schema 2 file as a card with `problem: Some("made by an older version of Threshold: import its Source Text again")`. UI: `lastPlayedText(ms: number, now: number): string` and `cardLine(card: CampaignCard): string` in `home/logic.ts`.

**Must NOT:** write to a file in `peek`; open a card that has a `problem`; keep the "Open campaign" block or the path field.

- [ ] **Step 1: Write the failing tests.**
  - `home.rs::campaign_card_reads_the_seed_and_counts_turns_only` (shape): after Begin, three Play Turns and one Correction, the card has the seed's PC name and premise, `turn_count == 3` (the reverted Turn does not count, the Opening does not count), `needs_begin == false`.
  - `home.rs::schema_2_file_shows_a_problem_card_and_does_not_open`.
  - `home.rs::peek_writes_nothing`: the file bytes are the same before and after.
  - `logic.test.ts › last played reads in words`, `logic.test.ts › a card line names the Turn count`.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement.** The app opens on Home: Campaign cards and unfinished Seed Drafts (`list_drafts`), one "New Campaign" button. The top bar's Campaign menu holds Home, Correct last Turn (moved from the action bar), Debug mode and the model status. The Character and New Campaign screens use the tokens. **Step 4: Run the gate, expect PASS.**
- [ ] **Step 5: Commit** `feat(ui): home screen and Campaign menu`.

**Done when:** the gate is green and no screen shows the old "Open campaign" block (closes that QA minor).

---

### Task 10: The review screen (L)

**Files:**
- Modify: `build/{mod,draft,draft_stages,stages,store}.rs`, `src-tauri/src/{lib,drafts}.rs`, `crates/threshold-eval/src/world_build.rs:634,1155`, `crates/threshold-eval/tests/report.rs:899`, `ui/src/{api.ts, newCampaign.ts}`, `ui/src/views/{SeedReview,fields,NewCampaign}.tsx`.
- Test: `crates/threshold-core/tests/{draft,build}.rs`, `src-tauri/tests/commands.rs`, `ui/src/newCampaign.test.ts`, `ui/src/api.test.ts`.

**Interfaces:**
- Consumes: Task 2 (`drafts.rs`).
- Produces (shape 3.8, exact): `AgeMark`, `AgeSource`, `AgeLine`, `SeedDraft.ages: BTreeMap<String, AgeMark>` (`#[serde(default)]`), `SeedDraft.cut: Vec<Cut>` (`#[serde(default)]`), `Cut { at, cap }`, `confirm_age`, `rename`, `apply_edit(draft, seed, knowledge)`, `check_source_text`, `SourceTextProblem::{Empty, TooShort { min }, TooLong { max }}`, `SOURCE_TEXT_MIN_CHARS: usize = 400`, `pub enum ReviewError { UnknownName, NameInUse, Empty }`. `ages_to_confirm` and `ages_stated` are removed everywhere. `DraftView` gains `ages: Vec<AgeLine>` and `cut: Vec<Cut>`. Commands `save_draft(draft_id, seed, knowledge)`, `confirm_age(draft_id, name)`, `rename_draft_entity(draft_id, from, to)`. The stage runner returns each list it cut at a `maxItems` cap; `draft_from_text` keeps them in `cut`.

**Must NOT:** let `apply_edit` or `save_draft` write `confirmed`; add a command to add an entry (the UI appends to `seed.entities` and saves); send a name change through `save_draft` (the UI sends it through `rename_draft_entity`); show `subject`, `key` or `value` fields outside edit mode or debug mode; add a "confirm all" button.

- [ ] **Step 1: Write the failing tests.**
  - `draft.rs::rename_updates_every_reference_and_adds_no_gap` (design 11 test 6): rename the character "Mira Holt" to "Mira Vance" in a draft that names her in a `known_to`, in `start.present`, as the subject of a known, a decided and a proposed fact, in the text of a known fact, in `start.situation` and as the `ages` key; every field changes, `gaps` is the same list as before, and her age stays confirmed. Rename the place "Old Mill" to "Grey Mill": `start.place` follows.
  - `draft.rs::rename_refuses_a_name_in_use_and_changes_nothing`: to another main name, to another entry's other name; the draft is equal to before.
  - `draft.rs::rename_changes_whole_words_only`: "Rose" to "Iris" leaves "Roseanne" and "rosewood" as they were.
  - `draft.rs::rename_to_the_same_name_in_other_case_is_allowed`: "chloé" to "Chloé".
  - `draft.rs::edited_age_is_labelled_edited` (design 11 test 8a): `FromText` when stated and untouched, `Proposed` when not stated, `Edited` after `apply_edit` changes it and for an added entry.
  - `draft.rs::approval_needs_every_age_confirmed_through_the_core` (design 11 test 8b): `apply_edit` with any input leaves `confirmed` as it was; `approve` fails with `AgeUnconfirmed` until `confirm_age` names each character; an edit of a confirmed age makes it unconfirmed again.
  - `draft.rs::confirm_age_refuses_an_unknown_name`.
  - `draft.rs::old_draft_file_without_ages_loads_unconfirmed`: a draft JSON with `ages_to_confirm` and no `ages` loads; each character has an `AgeUnconfirmed` gap.
  - `build.rs::added_character_reaches_the_world_build` (shape; design 11 test 7): append an NPC with an age, `save_draft`, `confirm_age`, `approve`, `build_world` on `RecordedGm`; one `NpcState` call names it and the `WorldSetup` holds its Voice Card.
  - `build.rs::short_source_text_is_refused_before_any_call` (design 11 test 9): 399 characters give `TooShort { min: 400 }` and zero recorded calls; 400 pass the check; `""` gives `Empty`.
  - `build.rs::cut_lists_are_reported`: a roster stage output of 31 characters gives `cut == [Cut { at: "entities", cap: 30 }]` (path form of `Gap.at`).
  - `commands.rs::save_draft_cannot_confirm_an_age` and `commands.rs::confirm_age_confirms_one_name`.
  - `newCampaign.test.ts › an age label says edited by you only for an edited age`, `› a typed name stays out of the saved seed until the rename returns`, `› each gap sentence says what is wrong and what to do`, `› a cut list says it was cut`.
- [ ] **Step 2: Run, expect FAIL.** `cargo test --workspace` and `npm --prefix ui test`
- [ ] **Step 3: Implement.** `check_source_text` is the one length check: `draft_from_text` and the shell's `Drafts::draft_from_text` both call it; the shell maps each problem to a sentence that names the limit. The review shows each known fact as its `text` with an "edit" action; edit mode or debug mode shows the three fields, each labelled. A name field commits through `rename_draft_entity` on Enter or blur. "Add a character" and "Add a place" append an entry and save. Each gap sentence sits next to its field. Other-name boxes get labels. The eval crate reads `ages[..].stated` instead of `ages_stated`. The UI no longer calls `nameKey` for ages.
- [ ] **Step 4: Run the gate, expect PASS.**
- [ ] **Step 5: Run the app** with `fixtures/chronicle/source.md`: rename a character, add one, edit and confirm an age, approve, build. Report what you saw.
- [ ] **Step 6: Commit** `feat(build): core-owned ages, rename, added entries and plain review`.

**Done when:** the gate is green; this closes the QA majors (age label, internal keys, rename), the minors "hello accepted" and "other names unlabelled and cut", the `ages_to_confirm` residual and "add a missed character".

---

### Task 11: The live check: Turn time and speaker marks (M)

**Files:**
- Create: `evals/scenarios/speech.toml`, `crates/threshold-eval/src/speech.rs`.
- Modify: `crates/threshold-eval/src/{lib,main}.rs`.
- Test: `crates/threshold-eval/tests/report.rs`.

**Interfaces:**
- Consumes: Task 5 (`speech::parse`, `pc_sight`, `play_view`).
- Produces: scenario `speech` (town fixture, ten Intents that invite dialogue with Orsel, Hessa and Doul; three runs). Per run and in total: Turn time p50, p90 and max (as `latency`); **mark rate** = speech lines that resolve to a `Speech` block / lines of speech, where a line of speech is each quoted span (`"…"`, `“…”`, `«…»`) in the stored Narration; marks that do not resolve, counted apart; **wrong marks** = share of `Speech` blocks whose speaker the local judge rejects (the judge sees the Narration with the mark removed and the present NPCs, and names the speaker).

**Must NOT:** change the engine or any prompt for the evaluation; put private text in a scenario.

- [ ] **Step 1: Write the failing tests** (no live model): `report_has_every_speech_metric`; `mark_rate_counts_resolved_marks_over_spoken_lines` (a Narration with three quoted spans, two marks, one resolving: rate 1/3, unresolved 1); `quoted_spans_are_counted_once_per_span`; `a_turn_with_no_speech_adds_nothing_to_the_rate`.
- [ ] **Step 2: Run, expect FAIL. Step 3: Implement. Step 4: Run the gate, expect PASS.**
- [ ] **Step 5: Commit** `feat(eval): speech mark scenario`.
- [ ] **Step 6 (controller, live model).** Warm the model. "Before": in a clean worktree of the base commit (`git worktree add /mnt/data/tmp/threshold-before master`), run `cargo run -p threshold-eval --release -- --scenario latency --runs 3 --out /mnt/data/tmp/before.md`. "After": on this branch, `--scenario latency,speech --runs 3`. Write `evals/reports/2026-11-play-experience.md`: model, date, Turn time p50/p90/max before and after in one table, mark rate, unresolved marks, wrong marks, and one line per finding. If the p90 grows by more than 1 s, or the mark rate is under 50%, say so in the report and stop for the owner before tuning `RULE`. Remove the worktree. Commit the report.

**Done when:** the report is committed with real numbers from both runs.

---

## After the last task

The controller runs the whole-branch review. Then the QA role plays the full flow on the live model, from Home to ten Turns of the fixture chronicle, with screenshots, scored against design section 2. The owner then plays his own Campaign by hand; it never enters the repository.

## Not closed by this plan

- QA minor "the opening Narration put an NPC in the room who was not listed as present": the Here panel now shows the core's list, but the narrator can still describe someone else. The QA pass checks it again.
- Deferred minors of the previous plan not named above stay deferred.
