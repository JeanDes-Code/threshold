# Mature content module: data shape

Date: 2026-11-11. Status: the owner accepted the defaults of section 16 on 2026-11-11. Parent spec: `2026-11-10-mature-content-module-design.md`. This page extends `2026-11-08-threshold-data-shape.md`, `2026-11-10-campaign-from-document-data-shape.md` and `2026-11-10-play-experience-data-shape.md` and does not restate them. Terms in capitals are defined in `GLOSSARY.md`. Section 12 lists the new terms.

This page wins over the design on mechanics. It does not change the intent, the goals or the anti-goals of the design. Where the design has a gap, section 16 asks the owner and gives a default.

This page holds no adult text. Every prompt line, template and example that the builder adds to the repository follows the same rule.

## 1. Context

The design asks for Intimate Scenes of several Turns, two buttons, and explicit description that only a private Content Pack turns on. Today the adults-only rule never runs: `intimate` is optional in the schema and the model never sets it (probe: 0 of 15). The app loads no Content Pack at all: `src-tauri/src/session.rs:353` gives the `Engine` an empty list.

The central decision: **explicit text reaches a prompt only through a `Cleared` value, and only `mature::clearance` makes one.** `clearance` reads the stored ages and nothing else. It takes no pack, no prompt and no model output. A pack fragment that is explicit-only, and the Narration of an earlier Turn of the scene, both need that value to enter a prompt. So the age rule is not a check that a caller can forget. A caller that skips it cannot build the prompt.

The second decision: **an age counts only when the Player confirmed it through the core.** The Campaign file records the confirmation for each character. A character the model creates in play has an age and no confirmation.

The third decision: **a Fade to Black is a mark on the Turn row, like the Narration itself. It is not a ledger op.** The hidden text is never written to the `turn` table.

```
                      mature::clearance(view) ──► Cleared ─────────────┐
                        (stored ages only)                             │ needed by
                                                                       ▼
Intent ─► [G1] ─► proposal call ─► [G2] ─► Roll, to_ops, events ─► [G3] ─► Commit ─► [G4] ─► narration call
           │      (scene text only            │                     │      (SetScene)   │   (explicit text only
           │       with Cleared)              │                     │                   │    with Cleared)
           └ scene open and not cleared:      └ age or level        └ a character       └ the Player fades:
             the core closes it first           refused: final,       arrived: checked     the stream stops,
                                                no retry              again, final         no text is stored
```

## 2. Boundaries

`threshold-core` gains one module, `mature`. No other module is new.

- `mature` (new) owns the adult rule, the state changes of the Intimate Scene, the effective Detail Level, the choice of what a Narration may tell, and the neutral line. It is the only caller of `domain::is_adult`. It imports `domain`, `store::WorldView` and `pack` (for one function). It makes no prompt and calls no model.
- `checks` keeps every Commit check. `validate` calls `mature::check`. `checks::boundary` keeps the `min_age` rule for a new NPC and loses the adults-only rule. No file of `checks` imports `pack`.
- `context` owns every prompt line, as today: the neutral definition of `intimate`, the core's scene line, the implied line, the closing line. It adds a scene text only when the caller gives it the value that `mature` made.
- `pack` owns the pack format, the loader and the choice of fragments for one prompt.
- `turn` sequences the steps. It gains `end_scene`, `fade`, `confirm_age` and `set_detail_level`.
- `store` gains schema 4, three ops, one Turn kind and `set_faded`.
- `build` gains `confirmed_ages`. `setup` takes its result.
- `model::Gm` changes once: the reader of a stream can stop it, and a narration call can have a token cap.
- `view` and `world::sight` gain the scene state, the faded body and the refusal view.
- `src-tauri` loads the packs, owns the fade flag and the five commands. It holds no rule.
- `ui` owns the two buttons, the Detail Level select and the pack list.

```rust
// mature: the public surface
pub struct Cleared(());                                   // private field: only `clearance` makes one
pub fn clearance(view: &WorldView, also: &[EntityId]) -> Result<Cleared, Blocked>;
pub struct Blocked { pub who: Vec<EntityId> }             // for the debug log and view::refusal, never shown raw
pub fn check(p: &DecisionProposal, view: &WorldView, errs: &mut Vec<Rejection>);        // G2, called by checks::validate
pub fn after(view: &WorldView, ops: &[LedgerOp]) -> Presence;                           // who is at the PC's place after the ops
pub fn check_after(p: &DecisionProposal, view: &WorldView, after: &Presence) -> Result<(), Rejection>;   // G3
pub fn scene_op(view: &WorldView, p: &DecisionProposal, after: &Presence) -> Option<LedgerOp>;
pub fn effective(stored: DetailLevel, packs: &[ContentPack]) -> DetailLevel;
pub enum Told { Plain, Implied, Explicit(Cleared) }
pub fn told(view_after: &WorldView, now: &WorldView, intimate: bool, packs: &[ContentPack]) -> Told;    // G4
pub fn neutral_line(turn: &StoredTurn) -> Option<String>; // Some for a faded Turn or a Turn committed with `intimate`
pub fn confirmable(view: &WorldView, entity: EntityId) -> bool;
```

## 3. Contracts

Each call gives the chosen option, the reason, the option I rejected, and the cost if it is wrong.

### 3.1 Detail Level

```rust
// domain
#[serde(rename_all = "snake_case")]
pub enum DetailLevel { #[default] Excluded, Implied, Explicit }
AdventureSeed { #[serde(default)] pub detail: DetailLevel }            // the level the Campaign starts with
LedgerOp::SetDetailLevel { level: DetailLevel }
WorldView { pub detail: DetailLevel }                                  // projection `campaign.detail_level`
```

- A seed with no `detail` has `excluded`. The module is off until the Player sets a level.
- The Player sets it in two places: the Seed review, through `save_draft`, and in play, through `set_detail_level`.
- The draft stages never set it. The model's seed output has its own struct (`build/draft_stages.rs:220`) and that struct gets no `detail` field. A Source Text cannot turn the module on.
- In play, only the command changes it. The OOC model call cannot. The design says "in the OOC channel": the control sits in the OOC panel of the action bar, and no model reads it.
- `mature::effective` gives `explicit` only when the stored level is `explicit` and a loaded pack declares the capability (3.8). In any other case it gives `implied` for a stored `explicit`. The stored level does not change.
- Only `store` reads `AdventureSeed.detail`, at creation and at a rebuild. Every other reader uses `WorldView.detail`.

**Why an op.** The level changes in play, and the ledger is the truth. **Rejected:** an `UPDATE` of `adventure_seed_json`. A rebuild would keep it, but `snapshot_before` would give a Correction the level of today, not the level of the Turn it plans again. **Cost if wrong:** one op and one column to remove.

### 3.2 Age confirmation in the Campaign file

Today `Entity` holds `age` and nothing says who gave it. Three sources exist: a Seed Draft that the Player confirmed row by row, a fixture file that nobody confirmed, and a `NewNpc` whose age is model output.

```rust
// domain
pub struct ConfirmedAges(BTreeSet<(EntityKind, String, u16)>);         // exact main name; `pub(crate)` constructor
LedgerOp::AddEntity { local, kind, name, age, other_names, #[serde(default)] age_confirmed: bool }
LedgerOp::ConfirmAge { entity: E }
pub struct Entity { /* as today */ pub age_confirmed: bool }           // store; projection `entity.age_confirmed`

// build
pub fn confirmed_ages(draft: &SeedDraft) -> Result<ConfirmedAges, Vec<Gap>>;   // only for a draft with no gap

// setup
pub fn create_campaign_file(path: &Path, seed: &AdventureSeed, world: &WorldSetup, rng_seed: u64,
                            ages: Option<&ConfirmedAges>) -> Result<(), SetupError>;
```

- `AddEntity` writes `age_confirmed: true` only for an entry that `ConfirmedAges` holds with the same kind, exact main name and age. The shell cannot make a `ConfirmedAges`: its constructor is private to the crate.
- The fixture path gives `None`. Every age of a two-file Campaign is unconfirmed.
- `to_ops` writes `age_confirmed: false` for a `NewNpc`, always.
- `ConfirmAge` is the only op that sets the flag after creation. `Engine::confirm_age` writes it, in an OOC Turn (3.9), when `mature::confirmable` holds: the entity is a PC or an NPC, its stored age is `ADULT_AGE` or more, and the PC perceives it now or can name it (`PcSight.known`). It never changes the age.
- An old ledger line has no `age_confirmed`. Serde reads it as `false`.
- No op lowers or raises an age. That YAGNI cut of the campaign-from-document shape stands.

**A character is cleared** when all of these hold: its kind is PC or NPC; `age_confirmed` is true; its age is `ADULT_AGE` (18) or more; and no character that is not cleared shares one of its names, main or other, through `normalize::name`. The last rule exists because nothing stops a `NewNpc` from taking the name of a roster minor.

**`min_age` does not touch this rule.** The floor is the constant `domain::ADULT_AGE`. A cleared `min_age` removes the `AgeBelowMin` gap of the draft, as before, and changes nothing here. This closes the review note of the campaign-from-document shape.

**Why a flag on the entity.** The gate needs one fact for each character, and it must survive a restart and a rebuild. **Rejected:** "an entity of the Setup Turn is confirmed" as an inference. It trusts the fixture path, and it gives no way to confirm a character met in play. **Rejected:** the model-stated age of a `NewNpc` counts. The design reads that way (6.2, "each NPC the proposal creates"); the brief for this page asks for an age confirmed through the core. Question 2 of section 16. **Cost if wrong:** the Player confirms a character met in play with one click before a scene with that character.

The confirmation in play shows no number. The Player reads "Confirm that <name> is an adult". The PC does not know an exact age, and no Player screen shows what the PC cannot know. The core refuses the confirmation of a character whose stored age is under 18.

### 3.3 Decision Proposal

```rust
pub struct DecisionProposal { /* as today */ pub intimate: bool }     // required in the per-Turn schema
pub struct Reaction { npc, stance, plan /* 90 chars; SCENE_PLAN_MAX_CHARS inside a cleared scene */, cause }
pub const SCENE_PLAN_MAX_CHARS: usize = 200;                           // first value
```

- `proposal_schema_for` puts `intimate` in `required`, on every Turn and at every level. The proposal rules define it in one neutral line: true when this Turn's intent or its outcome involves sexual or erotic content between any characters, whether the PC takes part or only watches (changed after review, 2026-11-12: it was contact between the PC and another character). `#[serde(default)]` stays on the struct, so stored proposals still parse.
- The evidence for "required" is ADR 0007: `contested`, a required boolean, came back true in every run of the arm-wrestle. No run has measured `intimate` as a required field yet.
- At `excluded` the field stays in the schema. The code needs the signal to refuse.
- Inside a cleared scene the per-Turn schema raises `maxLength` of `plan` to `SCENE_PLAN_MAX_CHARS` and sets `minLength` 1. `validate` enforces both limits again. Outside a scene the limit stays 90.
- A proposal fact, stake or event effect on the key `age` is dropped by `checks::settle`, with a debug line. The gate reads `entity.age` only, and a fact that says another age would only mislead the narrator.

`mature::check` (G2) runs inside `validate`, for a proposal with `intimate` true:

| Rule (design 6.2) | Test | Rejection kind | Retry |
|---|---|---|---|
| Setting | `view.detail` is not `excluded` | `DetailExcluded` | never |
| Age | every character the proposal names, in any field, is cleared, and `clearance` passes for everyone in `mature::checked_here` (see below); a new NPC, or a handle that reaches nothing the world holds, refuses | `AdultsOnly` | never |
| Stance | at least one NPC reacts, and each reacting NPC has stance comply or help | `Unwilling` (plan deviation 2) | the one normal retry |

**Changed after review (2026-11-12).** The age rule reads more than the place and the reactions:

- **The checked set.** `mature::checked_here(view)` is the union of world truth at the PC's place (`WorldView::present`, hidden characters included), everyone the PC believes is at the PC's place through `context::seen_present` (the exact set of the narrator's Present line, so a stale Belief or a Claim counts), and the PC. G2 and G3 both check it; G3 on the state after the Turn's ops.
- **The place rule, for the gate only.** A character is at the PC's place when the two location values have the same words after case, accents, punctuation and a leading article, when one is a word-prefix of the other, or when both name one Place entity through any of its names. A character with no live location counts as here, and every character does when the PC's own place is unknown. Play keeps `normalize::value` equality.
- **Every character the proposal names.** G2 collects every handle of a tagged proposal: fact subjects, `said_by`, `because`; Check `against`, stake subjects and factors' facts; blockers; reactions and their `cause`; `leaves.npc`; `move_to`; the `settles` item's subject; `cancel` event and cause; the scheduled event's actor, goal, effect subjects and `seen_by`; new NPCs. A fact handle reaches its subject, a World Event handle every entity it names, a goal handle its NPC and target. Each character it reaches must be cleared, here or not. A new NPC, and a handle that reaches nothing the world holds, refuses. Places named only as a destination (`leaves.to`, `moves_actor_to`) are left to G3.
- **The sticky tag.** When the raw text of the first attempt says `intimate` is true anywhere, read without parsing (cut, fenced, with trailing text, the key twice), the retry is checked as intimate.

**Changed after the live evaluation (2026-11-12): the tag has three values.** `intimate: bool` is now `intimacy: Intimacy`, a required enum `none` < `advance` < `intimate` (ADR 0008, dated section; one definition, `domain::INTIMACY_DEFINITION`). The age rule above (every character here and every character the proposal names; G3 too) runs for `advance` and `intimate`, with the same final Refusal. The setting rule (`excluded`), the stance rule, the scene (3.4), the telling (3.5) and the caps are for `intimate` only; an `advance` is a plain Turn that an NPC may accept or decline. The sticky tag keeps the strongest value of the raw answers. A stored proposal with the old boolean reads `true` as `intimate`, `false` as `none`.

`CheckKind` gains `DetailExcluded` and `AdultsOnly`, and `CheckKind::is_final()`. `turn::plan` stops at a final rejection: no retry prompt, no second proposal call, `TurnError::Refused`. `is_final` is the one list of final kinds (`mature::final_refusal` reads it).

**A Refusal is not a failed Turn** (changed after review, 2026-11-12). The Engine records it under its `op_id`, and the same `op_id` gets the same Refusal with no model call. The shell returns it as its own result kind, `{"refusal": <sentence>}`, and the Play screen shows the sentence as one quiet line, with no Retry; the Intent goes back to the field. A new Intent is a new op and is judged afresh.

**Why no retry.** The retry prompt lists the rejection lines. A model that reads "adults only" answers `intimate: false`, the proposal passes, and the narrator tells the same Intent. That is the model talking its way around the gate. **Rejected:** a schema that pins `intimate` to `false` when the place is not cleared. It loses the signal, and the Turn then commits as a plain one. **Cost if wrong:** a false tag at `excluded` costs the Player one refused Turn and one sentence. The evaluation counts those.

The stance rule keeps the retry. A missing reaction is a thing the model can repair, and the "NPC folds" check still caps the stance.

### 3.4 Intimate Scene state

```rust
LedgerOp::SetScene { open: bool, why: SceneWhy }
pub enum SceneWhy { Opened, EndScene, FadeToBlack, NotIntimate, Moved, Arrival, Gate, Excluded }
WorldView { pub scene_open: Option<TurnId> }                           // projection `campaign.scene_open_turn`
pub struct Presence { pub truth: BTreeSet<EntityId>, pub seen: BTreeSet<EntityId>, pub checked: BTreeSet<EntityId>, pub pc_place: Option<String> }
```

`mature::scene_op` runs in `turn::resolve`, after `world::with_witnessed_events`, on the snapshot before the Turn and the Turn's ops. It returns at most one op, and that op is part of the Turn's single Commit.

| Scene before | The Turn | Op | Scene after |
|---|---|---|---|
| closed | `intimate`, G2 and G3 pass | `SetScene { open: true, why: Opened }` | open |
| closed | not `intimate` | none | closed |
| open | not `intimate` | `why: NotIntimate` | closed |
| open | `intimate`, the PC's `location` changes | `why: Moved` | closed |
| open | `intimate`, `after.seen` holds a character that `seen_present(view)` does not | `why: Arrival` | closed |
| open | `intimate`, none of the above | none | open |
| open | a Closing Turn (3.7) | `why: EndScene` or `FadeToBlack` | closed |
| open | `set_detail_level(excluded)` | `why: Excluded`, in the same OOC Turn | closed |
| open | G1 fails at the start of any Turn | `why: Gate`, in its own Commit, before the proposal call | closed |

- A Turn that closes a scene never opens one. The next `intimate` Turn opens a new scene.
- "Another character arrives" reads what the PC perceives. A hidden arrival does not end the scene on the screen. The age rule reads world truth: a hidden minor refuses the Turn (G3).
- A scene Turn that a Correction reverts takes its `SetScene` op with it: a rebuild skips the ops of a reverted Turn (`store/project.rs:568`).

**Why an op and not a view rule.** A Scene of the Journal is a view. This state is different: it decides what enters a prompt and what a Commit check accepts, and both read `WorldView`, which comes from projections. **Rejected:** the state as a rule over the last standing Turn's `proposal_json` and ops. It needs no schema, but the gate would then parse stored JSON on each Turn, and "End scene" would have to be an `intimate: false` Play Turn with a reserved Intent text that the Player can also type. That rule stays useful as a test: the projection must equal it (`scene_projection_matches_the_last_turn_rule`). **Cost if wrong:** one op and one column.

`mature::after` applies the `location` writes of the ops to the view's locations. The oracle test `presence_after_matches_the_next_snapshot` compares it with `WorldView::present` and `context::seen_present` of the snapshot after the Commit.

### 3.5 The gate at four points

| Point | When | Reads | On failure |
|---|---|---|---|
| G1 | `turn::plan`, before the proposal call, when `view.scene_open` is set | `clearance(view, &[])` | the core commits `SetScene { open: false, why: Gate }` in an OOC Turn, then plans the Turn as a plain one |
| G2 | `checks::validate`, after the proposal call | the three rules of 3.3 | final refusal, or the retry for the stance rule |
| G3 | `turn::resolve`, after the due World Events fire, before the Commit | `clearance` over `after.checked` (`checked_here` after the ops) | final refusal; nothing commits, so the event fires in a later Turn |
| G4 | `Engine::narrate`, before every narration call, `renarrate` and a retry under the same `op_id` included | `mature::told` and `mature::narration_clearance`: G1 on the state after the Turn AND on the state now, and the same open scene now (`mature::open_scene`) | the Narration is told at `Implied`, with no pack fragment and the scene's other Turns as Neutral Lines |

G1 gives `Option<Cleared>` to `context::proposal_context`. With `None`, the proposal prompt holds no scene line, no pack scene fragment and no Narration of the scene.

`mature::told`:

```
not intimate                                               -> Plain
intimate, and the scene is open after this Commit,
  and it is still the open scene now (open_scene(now)),
  and effective(now.detail, packs) is explicit,
  and clearance(view_after) passes,
  and clearance(now) passes                                -> Explicit(Cleared)
intimate, any other case                                   -> Implied
```

A Turn that ends a scene is never told at `Explicit`. A `renarrate` of a Turn of a scene that has ended is told at `Implied`.

**Changed after review (2026-11-12, second review, B1).** The clearance of a narration call (`mature::narration_clearance`) needs G1 to pass on BOTH the state right after the Turn and the state now, and the scene of the state after the Turn to be the open scene now as every reader sees it (`mature::open_scene`). Before, it read the state after the Turn only: a `renarrate` of a scene Turn whose Narration had failed, or the same op sent again, after another program brought an unconfirmed minor to the place, was told at `Explicit` with the pack fragments and the scene's earlier Narrations. Now such a Turn is told at `Implied`, with no pack fragment, and the scene's other Turns are their Neutral Lines. Test: `a_scene_narration_told_again_reads_the_gate_now`.

G1 cannot fail on a file that this code wrote: an open scene means that G2 and G3 passed on its last Turn, and no op removes a confirmation. It exists for a file that another program changed. The core does not defend against a Player who edits the SQLite file by hand. `rebuild_projections` puts the projections back to what the ledger says.

### 3.6 Fade to Black

**What is stored.**

```
turn  + faded INTEGER NOT NULL DEFAULT 0      -- written by Store::set_faded, like `narration` by set_narration
```

```rust
pub struct StoredTurn { /* as today */ pub faded: bool }
impl Store { pub fn set_faded(&mut self, t: TurnId) -> Result<(), StoreError>; }   // narration = NULL, faded = 1
```

- The hidden text is never in `turn.narration`. A faded Turn has `narration` NULL and `faded` 1.
- The neutral line is not stored. `mature::neutral_line` builds it from the Turn row: its kind and the minutes between `clock_before` and `clock_after`. It holds no name, no fact value and no model text. Three fixed templates: a faded Turn, a Turn committed with `intimate`, a Closing Turn. The builder writes them in neutral words.
- The ledger of a faded Turn is the ledger of the same Turn read in full. The record of the skip is the `faded` column.

**One function says what a reader gets for a Turn.** `context` (the `recent` block of both prompts) and `world::sight` (the OOC sheet) both call it:

```rust
// mature
pub enum TurnText<'a> { Full { intent: &'a str, narration: Option<&'a str> }, Line(String) }
pub fn turn_text<'a>(turn: &'a StoredTurn, scene: Option<TurnId>, cleared: Option<&Cleared>) -> TurnText<'a>;
```

- A faded Turn gives `Line`, always.
- A Turn committed with `intimate` gives `Full` only to a prompt that holds a `Cleared` value and whose open scene started at or before that Turn. Every other prompt gets `Line`: a plain Turn after the scene, the OOC sheet, a Turn where the place is no longer cleared.
- `Line` replaces the Intent too. The Player's own words for that Turn stay on the Play screen and in the Journal.
- The Play screen is not a prompt: it shows a Turn that the Player read in full with its Narration, as today.

So the Narration of a scene Turn never reaches a prompt that the gate did not clear. Facts that the scene committed stay in the world and reach every later prompt, as any fact does.

**Changed after review (2026-11-12, second review, M3).** Every reader that scans past Turn text for a prompt goes through `turn_text`:

| Reader | What it reads | Through |
|---|---|---|
| `context::turns_window` (the RECENT TURNS of the proposal, closing and narration prompts) | Intent and Narration | `turn_text` with the prompt's clearance (as before) |
| `context::Retrieval::new` (MENTIONED ENTITIES, MENTIONED FACTS and the schema handles they add) | the names in the Intent and the Narration of the last 3 Turns | `turn_text` with the prompt's clearance (changed: it read every Narration and Intent raw) |
| `view::ooc_sheet` (the OOC prompt) | Intent and Narration | `SightTurn.line`, which is `turn_text(turn, None, None)` (as before) |

A faded Turn is its Neutral Line in each, its Intent included. The Journal and a pinned act are Player screens, not prompts: they read `view::act_words`, the one rule for both, so a faded Turn's act is its Neutral Line on both (a pinned act showed the Intent before). `WorldView::story_names` (what the PC can name, read by `world::pc_can_name`) keeps reading every standing Narration the Player read on screen, a scene Turn's included: it gives a prompt no text, only which names the PC may use, and the Player did read those names. A faded Turn has no stored Narration, so it names nobody there. A `RecordCorrection` op keeps the corrected Turn's Intent in its `what_wrong` text; no prompt and no screen reads it.

**The stream.** `Gm::narrate` cannot be stopped today, and a running Turn holds the `Session` lock.

```rust
// model
pub enum Stream { More, Stop }
pub trait Gm {
    fn propose(&self, prompt: &Prompt, schema: &serde_json::Value) -> Result<String, GmError>;
    fn narrate(&self, prompt: &Prompt, max_tokens: Option<u32>,
               on_chunk: &mut dyn FnMut(&str) -> Stream) -> Result<String, GmError>;   // Stop: returns the text so far
}
// turn
pub struct TurnControl<'a> { pub on_chunk: &'a mut dyn FnMut(&str), pub faded: &'a dyn Fn() -> bool }
```

- The shell owns one `AtomicBool`, the fade flag. `fade_to_black` sets it and takes no lock, like `cancel_build`.
- `turn::narrate_through` reads `faded()` before the call and after each chunk. When it is true it sends no more text to `on_chunk` and returns `Stream::Stop`. `OllamaGm` then drops the HTTP response, which ends the generation on the server.
- A flag that is already up when the narration would start means no narration call at all.
- The Engine then calls `set_faded`. When the Commit of that Turn left a scene open, it also runs the Closing Turn (3.7) with no narration call, in the same locked command.

**The three cases of the button.**

| When the Player presses | What the core does |
|---|---|
| A Narration streams (a Play, Correction or Closing Turn, or a `renarrate`) | stops the stream, `set_faded` on that Turn; if a scene is open, a Closing Turn that is born faded |
| No Turn runs, a scene is open | a Closing Turn that is born faded: one proposal call, no narration call. The Turns already read keep their text |
| No Turn runs, no scene | `TurnError::NothingToFade`. The UI does not show the button then |
| The Narration streamed, and its Turn ended before it read the flag (a late fade) | the UI sends the fade again with that Turn's op_id (a renarration's op_id names the Turn it told). `Engine::fade` fades that Turn when it is the latest Turn and a Play, Correction or Closing Turn: `set_faded`, no text stored, and a scene it left open closes with a Closing Turn born faded. Any other Turn stays as stored, with no model call |

**Changed after review (2026-11-12, second review).** The late fade (ADR 0011) fades a Turn whose Narration the Player already saw stream in full. It follows the owner's decision that a faded Turn stores no text: the Player asked to skip it, so the row loses its text and every screen and prompt reads its Neutral Line from then on. The Opening is never faded, by the button or by a late fade (section 10), and the UI disables the button while Begin or a renarration of the Opening streams. `fade_to_black` tells a running Turn from any other holder of the session lock (`session::FadeGate`): with no Turn running it takes the lock and runs the idle path, and it never leaves the flag up.

**Correct last Turn.** A faded Play Turn that is the last Turn can be corrected. The Correction reverts it and commits a new Turn with a new Narration, read in full unless the Player fades again. The reverted Turn stays in the story as its neutral line. A faded Turn of a scene is followed by its Closing Turn, so it is no longer the last Turn and cannot be corrected. A Closing Turn has no Intent and cannot be corrected.

**Rebuild.** `rebuild` replays ledger ops into projections. The `turn` table is not a projection, so `narration` and `faded` are untouched. No hidden text exists to rebuild from. `project::historical` copies Turn rows by column list (`store/project.rs:983`): the builder adds `faded` there. A missed copy shows a faded Turn of the past as `Missing`, never as text.

**`renarrate`.** It refuses a faded Turn. `stored_outcome` (a retry under the same `op_id`) returns the neutral line for a faded Turn and makes no model call.

**Debug log.** The `narrate` row of a faded Turn keeps the prompt and the text received before the stop, then the line `[faded]`. The design says so (6.6). `debug_log` rows are written in every Campaign, debug mode or not; only `get_debug` reads them, and only in debug mode. Question 5 of section 16 asks if the owner wants the partial text kept.

**Rejected:** a `FadeTurn` ledger op. It needs a carrier Turn row after the faded Turn, and "Correct last Turn" then finds a carrier where it expects a Play Turn. The Narration was never in the ledger; its absence does not belong there either. **Rejected:** the neutral line written into `turn.narration`. Readers would need no change, but a Turn committed with `intimate` and read in full needs the same line in later prompts, so the readers branch anyway. One function for both cases is smaller. **Cost if wrong:** the Player cannot un-fade. That is the intent of the button.

### 3.7 The Closing Turn

```rust
pub enum TurnKind { Setup, Play, Correction, Ooc, Opening, Closing }
impl TurnKind { pub fn tells(self) -> bool;        // Opening, Play, Correction, Closing: has a Narration
                pub fn has_intent(self) -> bool; } // Play, Correction
impl Engine { pub fn end_scene(&mut self, op_id: OpId, ctl: &mut TurnControl) -> Result<TurnOutcome, TurnError>;
              pub fn fade(&mut self, op_id: OpId) -> Result<TurnOutcome, TurnError>; }
pub const SCENE_CLOSE_DEFAULT_MIN: u32 = 15;       // first values
pub const SCENE_CLOSE_MAX_MIN: u32 = 240;
pub const CLOSING_NARRATION_MAX_TOKENS: u32 = 90;
```

A Closing Turn has an empty Intent and the kind `closing`. `end_scene` fails with `TurnError::NoScene` when no scene is open.

1. `context::closing_prompt(view)`: the proposal rules, the stable and scene blocks, and one core line in place of the Intent: the Player ends the scene; propose how each NPC present takes it and how many minutes it took. No pack fragment.
2. `closing_schema_for(handles)` pins what the code does not want: `action` is `wait`, `verdict` is `allowed`, `contested` and `intimate` are `false`, `move_to` and `check` are `null`, and `settles`, `new_npcs`, `schedule` and `cancel` are absent. Free fields: `minutes`, `reactions` (max 2), `facts` (max 1).
3. `checks::settle` forces the same pins on the parsed proposal and logs each change. It removes model output and adds no PC act. `minutes` is clamped to 1 to `SCENE_CLOSE_MAX_MIN`. A Closing Turn moves nobody (changed after review, 2026-11-12): `checks::settle_closing` also drops every fact on the key `location`, on any subject, and the closing schema says so on `facts`.
4. `validate` runs as on any Turn. The Agency check keeps a fact on the PC to `PC_INVOLUNTARY_KEYS`, so fatigue is allowed and a PC choice is not.
5. `to_ops`, the World Clock and the due World Events run as on any Turn. `SetScene { open: false }` is added. One Commit.
6. The narration is told at `Plain`, with `context::CLOSING_LINE` and `CLOSING_NARRATION_MAX_TOKENS`. The narrator gets no Intent line. A faded Closing Turn gets no narration call.

**The button always works.** When the model fails or both attempts are rejected, the core commits `mature::default_close`: no reaction, no fact, `SCENE_CLOSE_DEFAULT_MIN` minutes, the `SetScene` op. The Player's choice to stop never depends on a model answer.

**Why a kind.** A Play Turn with an empty Intent would be offered to "Correct last Turn", listed as an act of the PC in the Journal, and counted in `turn_count`. Six places match on `TurnKind` today (`world/sight.rs:169`, `:425`, `:699`, `view/mod.rs:418`, `context/mod.rs:386`, `store/project.rs:901`). `tells` and `has_intent` replace those matches before the new kind lands.

**Rejected:** a closing that code writes with no model call. The design asks the model for the minutes and the NPC reactions (6.5). **Cost if wrong:** the narrator has no Intent to tell, so it may fill the gap with a PC act. Code cannot see that. The agency metric of the evaluation counts Closing Turns apart.

### 3.8 Content Pack format 2

A pack is a folder in `<data_dir>/packs/`. `pack.toml`:

```toml
format = 2                                  # required; any other value is refused
name = "sample"                             # required, 1 to 60 chars
provides = ["explicit_description"]         # optional; the one capability that exists
recommended_model = "some-tag"              # optional, one line, 80 chars; shown, never applied
seed_options = ["..."]                      # as today
model_hints = ["..."]                       # as today

[[fragments]]
prompt = "narration"                        # required: "proposal" or "narration"
at = "intimate_scene"                       # required: "tone", "scenes" or "intimate_scene"
when = "explicit"                           # optional: "always" (default) or "explicit"
file = "scene.md"                           # a file name in the pack folder, UTF-8 text

[interview]                                 # reserved for project 3; read and ignored
```

```rust
pub enum Target { Proposal, Narration }
pub enum InsertPoint { Tone, Scenes, IntimateScene }
pub enum When { Always, Explicit }
pub enum Capability { ExplicitDescription }
pub struct Fragment { pub prompt: Target, pub at: InsertPoint, pub when: When, pub text: String }
pub struct ContentPack { pub name: String, pub fragments: Vec<Fragment>, pub provides: Vec<Capability>,
                         pub recommended_model: Option<String>, pub seed_options: Vec<String>, pub model_hints: Vec<String> }
pub struct PackProblem { pub folder: String, pub rule: PackRule, pub at: Option<String> }   // a rule name and a file name; never pack text
pub fn load_all(dir: &Path) -> (Vec<ContentPack>, Vec<PackProblem>);
pub fn scene_fragments<'a>(packs: &'a [ContentPack], prompt: Target, cleared: &Cleared, explicit: bool) -> Vec<&'a str>;
pub const FRAGMENT_MAX_CHARS: usize = 1_200;             // first values
pub const SCENE_FRAGMENTS_MAX_CHARS: usize = 1_600;      // per prompt, over all loaded packs
```

**What code does with a fragment.**

| `at` | `when` | Enters the named prompt |
|---|---|---|
| `tone`, `scenes` | `always` | every Turn, in the system block, as today, but only in the prompt it names |
| `intimate_scene` | `always` | only with a `Cleared` value: an open scene, at `implied` or `explicit` |
| `intimate_scene` | `explicit` | only with a `Cleared` value and `effective` equal to `explicit` |

The scene fragments and the core's own scene lines go at the head of the `recent` block, not in the system block. The prompt prefix then stays the same when a scene opens or ends (ADR 0001). The core's implied line is in the narration prompt only when `Told` is `Implied`. The switch is the `Told` value, never pack wording.

**Validation.** The loader refuses the whole pack on the first failed rule:

1. The folder holds a symbolic link or a file with a code extension (as today).
2. `pack.toml` is missing, is not TOML, or has a key that this page does not name. `interview` is the one key that is read and ignored.
3. `format` is not 2. The test pack of the repository moves to format 2.
4. `name` is empty or too long, or a pack loaded before it has the same name through `normalize::name`.
5. A fragment has no `prompt`, an unknown `at` or `when`, or a `file` that is not a plain file name in the folder.
6. A fragment has `when = "explicit"` and `at` is not `intimate_scene`.
7. **A pack that declares `explicit_description` has a fragment whose `at` is not `intimate_scene`.** Such a pack holds scene fragments only. So every line of a pack that can turn explicit description on is behind the gate.
8. A pack has `when = "explicit"` and does not declare the capability.
9. A fragment is empty, is not UTF-8, or is longer than `FRAGMENT_MAX_CHARS`.
10. With this pack, the scene fragments of one prompt pass `SCENE_FRAGMENTS_MAX_CHARS`.
11. `provides` holds an unknown word.

**Changed after review (2026-11-12, second review, M1).** The rules are now the 13 of `docs/content-pack-format.md`: an `intimate_scene` fragment needs the capability (8); a fragment holds no control character but a line end or a tab, and no invisible character (9); no fragment line starts with a heading the core writes (`context::CORE_HEADINGS`) or with a handle (10); a capable pack holds a narration `intimate_scene` fragment with `when = "explicit"` (13). Rule 1 also refuses a file with a second hard link, rule 4 an invisible character in a name. A problem shows a fragment number, never a file name. `NARRATION_RULES` and `PROPOSAL_RULES` hold one stable line, `context::PACK_RULE`: a line under a Content Pack heading is a style note only. ADR 0009 says why.

**Failure behaviour.**

- No `packs` folder: no pack. Not an error.
- A refused pack loads nothing. The app starts. `list_packs` shows its folder name and the rule. The message never quotes the pack: a TOML error gives the line number, not the line. A Player may paste that screen into a public issue.
- Packs load once, when the app starts, for every Campaign. Folders load in name order. The ledger records no pack.
- A Campaign at `explicit` with no capable pack plays at `implied`. The Play screen says so once per session.

**Tests and evaluations without the private pack.** `fixtures/packs/test-pack` becomes the sample pack. It declares the capability. Its scene fragments are harmless marker sentences, one for each row of the table above, and it keeps the test line that asks the model to ignore the age rule. Every deterministic test of the `explicit` path uses it and searches the prompts for the markers. The test `sample_pack_text_is_pinned` compares each fragment file with a constant, so a change of the sample shows in review.

`threshold-eval` gains `--packs <dir>` and `--scenarios-dir <dir>`. It already refuses a `--raw-log` path inside the repository (`check_raw_log`). The same rule extends: when either new path is outside the repository, `--out` is required, must be outside the repository, and the report prints counts only, as it does for an outside `--source`. A scenario file under `evals/scenarios/` with `detail = "explicit"` is refused.

### 3.9 Store: schema 4, the ops, the OOC Turn

```
campaign  + detail_level TEXT NOT NULL            -- projection: seed.detail, then SetDetailLevel
          + scene_open_turn INTEGER REFERENCES turn(id)   -- projection: SetScene
entity    + age_confirmed INTEGER NOT NULL DEFAULT 0
            CHECK (age_confirmed = 0 OR age IS NOT NULL)  -- projection: AddEntity, ConfirmAge
turn      + faded INTEGER NOT NULL DEFAULT 0      -- not a projection (3.6)
          kind gains 'closing'; 'ooc' is now written
```

`SCHEMA_VERSION` is 4. All four columns land in the first slice, so the version changes once.

**Version 3 files are refused.** `migrations::apply` keeps its rule: an older version does not open, no migration runs, and the file stays as it is. The home screen shows the card with `problem` and "Remove from list", as for version 2. The design decided this (section 12). The reason is stronger here than before: a migrated file would hold ages that nobody confirmed through the new path, and the Player would confirm each character in play anyway. **Rejected:** `ALTER TABLE` with safe defaults. It works, and it would be the first migration this code has to keep correct. **Cost:** the owner builds his test Campaigns again from their Source Texts.

`rebuild` sets `detail_level` back to the seed's `detail` and `scene_open_turn` to NULL, then replays. Entities are deleted and replayed as today, so `age_confirmed` follows the ops.

**The OOC Turn.** `TurnKind::Ooc` exists and nothing writes it. `confirm_age` and `set_detail_level` each commit one Turn of that kind: empty Intent, no model call, no clock change, one or two ops. The glossary already calls such an act OOC. An OOC Turn is in no story, no Moment and no count. It is the latest Turn after it commits, so "Correct last Turn" is closed until the next Play Turn. I accept that limit: the other choice is a Correction that plans against a state without the confirmation the Player just gave.

### 3.10 What each model call receives

| Call | Scene text | Earlier Turns of the scene | Cap |
|---|---|---|---|
| Proposal, no open scene | none | `Line` for each Turn committed with `intimate` | as today |
| Proposal, open scene, G1 passed | the core's scene line; pack `proposal` scene fragments; the explicit-only ones only at `explicit` | `Full` | `plan` up to 200 chars |
| Proposal of a Closing Turn | the closing line, no pack fragment | `Full` when G1 passed | pinned schema |
| Narration, `Plain` | none | `Line` | as today |
| Narration, `Implied` | the core's implied line | `Full` only with a `Cleared` value | `SCENE_NARRATION_MAX_TOKENS` (changed after review, 2026-11-12: every Turn told as intimate, the Turn that ends its scene and a `renarrate` included; a Closing Turn gets `CLOSING_NARRATION_MAX_TOKENS`) |
| Narration, `Explicit(Cleared)` | pack `narration` scene fragments, explicit-only ones included; no implied line | `Full` | `SCENE_NARRATION_MAX_TOKENS` |
| Opening, OOC, every draft and World Build call | none, at any level | `Line` | as today |

`NARRATION_RULES` gains one stable line, in neutral words: a Narration describes sexual content only when THIS TURN says so, and only as it says. It costs about 25 tokens and never changes, so the prefix stays stable.

A cut at the token cap can end in the middle of a sentence. When `last_done_reason` is `length`, the core stores the Narration up to its last full sentence.

**Changed after review (2026-11-12, second review, B2).** A cut can also end inside a speech mark: `Dusk falls.` then `Vale: "Go.` on the next line. The reader of a stored Narration shows a line that is not a complete mark as prose, its tag included, so the stored text printed the name of a hidden NPC that the stream filter had held back. Now, after the cut to the last full sentence, a last line that opens a quote it does not close, or that has a speech tag and no complete quoted span after it, is dropped whole (`speech::without_cut_mark`): the stored text ends at the end of the line before it. It then holds only lines the model ended, and the reader cleans it as it cleans a full Narration. The stream agrees: the start of the last line that the stream filter still holds at a cut (a tag whose colon never came, or a short line) streams only as far as the stored text keeps it. When the Turn ends, the Play screen reloads the view and shows the stored text in place of the stream, so the Player sees the cut line disappear, never a new name. Test: `a_cut_narration_never_shows_a_name_the_full_text_hides` (every cut point over a text with the marks of a hidden NPC, an NPC the PC cannot name and a present NPC).

### 3.11 Player screens

```rust
// view
pub enum SceneState { Plain, Intimate { since_turn: TurnId } }
pub enum TurnBody   { Narration { blocks: Vec<Block> }, Missing, Faded { line: String } }
pub struct PlayView { /* as today */ pub detail: DetailView }
pub struct DetailView { pub stored: DetailLevel, pub effective: DetailLevel }
pub struct Act { /* as today */ pub faded: Option<String> }           // Some: `intent` is empty, the Journal shows the line
pub struct RefusalView { pub kind: RefusalKind /* excluded | adults_only | unwilling */, pub text: String,
                         pub to_confirm: Vec<NameRef> }
pub fn refusal(sight: &PcSight, kind: RefusalKind, blocked: &[EntityId]) -> RefusalView;
```

`SceneState` and `TurnBody` are the seam that the play-experience shape left (3.9). They exist in `view/mod.rs:142` and `:230` with one and two variants. They are the right seam: this page adds one variant to each and changes no field.

`PcSight` gains `scene: Option<TurnId>` and `detail`, a Moment for each Closing Turn, and `faded` and `line` on each `SightTurn`. It gains no age and no hidden character.

| Screen | Shows | Never shows |
|---|---|---|
| Play, story | a Turn read in full, as today; a faded Turn as its Intent line and `Faded { line }`; a Closing Turn with no Intent line | the hidden text of a faded Turn |
| Play, action bar | the two buttons (3.12); a refusal with its sentence and, for `adults_only`, a "Confirm" control for each name in `to_confirm` | the age of anyone; the name of a character the PC cannot name; why a hidden character blocks |
| Play, notice | once per session, when `stored` is `explicit` and `effective` is `implied`: the level in use and the reason | pack text |
| Journal | a faded Turn by its neutral line only (`Act.faded`); the facts the PC learned, as today | the Intent and the Narration of a faded Turn |
| Entity card | as today | age, confirmation |
| OOC answer | an answer made from the OOC sheet, where each Turn committed with `intimate` and each faded Turn is its neutral line | scene Narration; pack text |
| Debug view | everything, pack fragments and faded text included, in debug mode only | nothing is hidden there, as today |
| Home, menu | the pack list: name, "explicit description: yes or no", recommended model, or the problem of a refused pack | pack text |
| Seed review | the Detail Level select | no age rule changes with it |

`to_confirm` lists a blocked character only when the PC perceives it here and `mature::confirmable` holds. A blocked character that is hidden or under 18 is in no list. The Player then reads the one fixed sentence of `adults_only` and no name.

### 3.12 Commands and the two buttons

| Command | Arguments | Returns | Lock |
|---|---|---|---|
| `end_scene` | `op_id` | `TurnSummary`; emits `narration_chunk`, then `turn_committed` | session |
| `fade_to_black` | `op_id` | `FadeResult { pending: bool }` | none to set the flag; `try_lock` after |
| `confirm_age` (play) | `entity_id`, `op_id` | `PlayView` | session |
| `set_detail_level` | `level`, `op_id` | `PlayView` | session |
| `list_packs` | none | `Vec<PackLine>` | none |

Changed: `submit_intent`, `correct_last` and `renarrate` read the fade flag through `TurnControl` and clear it when they start. `TurnSummary` gains `faded: bool`. `turn_failed` gains `refusal: Option<RefusalView>`. `save_draft` carries `seed.detail`. `start_build` calls `build::confirmed_ages` and gives the result to `create_campaign_file`. The draft command `confirm_age` keeps its name; the play command is `confirm_age_in_play` in the shell if the two collide.

`fade_to_black` sets the flag, then tries the session lock. When a Turn holds it, the command returns `pending: true` and the running command finishes the fade. When the lock is free, it clears the flag and calls `Engine::fade`.

**The UI contract for `action-bar__later`.** The slot gets what `ActionBar` already has: `scene: SceneState` and `phase: TurnPhase`. It needs no list of actions from the core.

| Button | Shown when | Enabled when | On click |
|---|---|---|---|
| End scene | `scene.kind` is `intimate` | `phase` is `idle` | `end_scene`; the story streams as for an Intent |
| Fade to black | `phase` is `narrating`, or `scene.kind` is `intimate` | `phase` is `narrating`, or `phase` is `idle` with an intimate scene | the UI hides the stream text at once and drops later chunks of that `op_id`; `fade_to_black`; then `get_play_view` |

One click each, no confirm step. Both are real buttons with a visible label. The slot is empty in the Begin row.

## 4. The Turn flow of a mature Turn

A Turn inside an open scene, at `explicit`, with the sample pack or the private one.

| Step | What runs | Model input | Time |
|---|---|---|---|
| 1 | duplicate `op_id`, Intent check, snapshot | | under 0.1 s |
| 2 | **G1**: `clearance(view)`. The result goes to `proposal_context` | | under 1 ms |
| 3 | proposal call | rules, Adventure, scene block (cached prefix); then the core's scene line, the pack's proposal fragments, the Turns of the scene in full, the Intent | read: about 1 s for a tail of 900 tokens; answer: about 150 tokens, 3.5 s |
| 4 | parse, `settle`, engine time | | |
| 5 | **G2** in `validate`, with every other Commit check | | under 0.1 s |
| 6 | Roll, `to_ops`, World Clock, due World Events, arrival | | under 0.1 s |
| 7 | **G3**: `check_after`; then `scene_op` | | |
| 8 | one Commit: the Turn's ops and the `SetScene` op, if any | | under 0.1 s |
| 9 | **G4**: `told` | | under 1 ms |
| 10 | narration call, through the fade flag | rules, Adventure, scene block (cached prefix); then the pack's narration fragments, the outcome, the NPC plans, the Intent | read: about 0.7 s; answer: 280 tokens at most, 6.5 s |
| 11 | `set_narration`, or `set_faded` and the Closing Turn | | |

About 12 s. The rates are the ones of `evals/reports/2026-11-turn-time.md`: 43 tokens per second on the QA machine, about 900 tokens per second to read a prompt, a cached prefix in about 0.1 s. Every number of steps 3 and 10 is an estimate from those rates. No mature Turn has been timed.

- A proposal retry adds about 3.5 s and passes 15 s. That is true of an ordinary Turn on a large world today (the same report: 13 of 17 Turns over 15 s before `settles`). This page does not fix it. By the same rates, a scene Turn adds about 1 s to the common path: the scene text to read and the longer plans.
- `SCENE_NARRATION_MAX_TOKENS` is 280 as a first value. It is the size of today's ordinary Narration (ADR 0001), now enforced by the model's token limit and not by a prompt rule. The probe's 300 words took 10 to 27 s; 280 tokens are about 200 words. The builder measures and corrects it.
- An "End scene" is one small proposal and a Narration of 90 tokens: about 6 s by the same rates. A Fade to Black inside a scene is the proposal only.

**No second call and no second model.**

- A call that only classifies the Intent would fix a wrong tag. It costs a prompt read and an answer, about 1.5 s, on every Turn. The required field costs about 4 output tokens.
- A call that checks the Narration after the stream costs 3 to 5 s, and the Player has read the text by then.
- A second model costs a reload of about 40 s on 12 GB (ADR 0001). The probe found no refusal on the default model.

## 5. The gates

| Gate | Where enforced | What the Player reads when it refuses | Test |
|---|---|---|---|
| Module off by default | `DetailLevel::default`; `mature::check` | "This Campaign excludes Intimate Scenes. You can change the Detail Level under Ask the GM." | `a_seed_without_detail_excludes`; `an_intimate_proposal_is_refused_at_excluded` |
| Explicit needs a capable pack | `mature::effective`; `mature::told` | once: "No loaded Content Pack gives explicit description. Scenes are told by implication." | `explicit_without_a_capable_pack_is_told_implied` |
| Every character the gate checks at the PC's place (`checked_here`: world truth, the PC's Beliefs, the gate's place rule, no known location), and every character the proposal names, is a confirmed adult, before the Turn | G2, `mature::check` | "An Intimate Scene cannot happen here." and, when a perceived adult is unconfirmed, "Not confirmed as an adult:" with the names | `a_minor_in_the_place_refuses`; `an_unconfirmed_adult_refuses`; `a_hidden_minor_refuses_and_names_nobody`; `a_new_npc_in_an_intimate_turn_refuses` |
| The same, after the World Events of the Turn | G3, `mature::check_after` | the same sentence | `an_event_that_brings_a_minor_refuses_the_turn` |
| A Refusal is not a failed Turn; the same op is not run again | `Engine::stored_refusal`; `AppError::Refused`; the Play screen | the same sentence, as a quiet line with no Retry; the Intent is back in the field | `the_same_op_after_a_refusal_makes_no_model_call`; `a_refusal_is_its_own_result_kind`; `a Refusal is a quiet line, not a failure, and offers no retry` |
| An age refusal is final | `turn::plan`, `CheckKind::is_final` | the same sentence, after one proposal call | `an_age_refusal_makes_no_second_proposal_call` |
| No scene text in a proposal without clearance | G1, `proposal_context` | nothing: the Turn is planned as a plain one | `a_scene_that_is_not_cleared_is_closed_before_the_proposal_call` |
| No explicit text in a narration without clearance | G4, `Told::Explicit(Cleared)` | nothing: the Narration is implied | `explicit_fragments_need_a_cleared_value` (compile-level, plus a prompt search) |
| An NPC who does not go along | G2, stance rule, with the one retry | "Nobody here goes along with this." | `an_intimate_turn_with_a_resisting_npc_is_rejected`; `an_intimate_turn_with_no_reaction_is_rejected` |
| The Player ends a scene | `Engine::end_scene`; `mature::default_close` | the short Narration of the Closing Turn | `end_scene_closes_when_the_model_fails` |
| The Player skips a description | `fade_to_black`; `narrate_through`; `set_faded` | the neutral line | `a_fade_stops_the_stream_and_stores_no_text`; `the_neutral_line_makes_no_model_call` |
| Confirmation in play | `Engine::confirm_age`; `mature::confirmable` | "This character cannot be confirmed as an adult." | `a_stored_minor_cannot_be_confirmed`; `a_hidden_character_cannot_be_confirmed` |

The builder writes the final sentences. Their meaning is fixed here.

## 6. The paths to the age rule

Each row is a way a character could be in an Intimate Scene, or a way the rule could be weakened.

| # | Path | What holds | Test |
|---|---|---|---|
| 1 | A roster character of a Seed Draft | `confirmed_ages` needs a draft with no gap; `AgeUnconfirmed` is a gap | `a_built_campaign_marks_only_confirmed_ages` |
| 2 | A Seed Draft edit of an age or a name after the confirmation | the mark follows the exact entry (play-experience shape 3.8) | existing draft tests; `an_edited_age_is_unconfirmed_in_the_campaign` |
| 3 | A Seed Draft edit that clears `min_age` | the floor is `ADULT_AGE`, a constant | `clearing_min_age_changes_no_clearance` |
| 4 | A Seed Draft edit of `detail` | a setting; no age rule reads it | `detail_explicit_clears_nobody` |
| 5 | A Source Text that asks for explicit content or states ages | the draft model has no `detail` field; `stated` never confirms | `the_draft_stage_cannot_set_detail` |
| 6 | An old draft file that says `approved` | `approve` and `build_world` compute the gaps again (existing) | existing |
| 7 | The two-file fixture path | `ages: None`: every age unconfirmed | `a_fixture_campaign_has_no_confirmed_age` |
| 8 | A `NewNpc` in the intimate Turn | refused: `new_npcs` must be empty | `a_new_npc_in_an_intimate_turn_refuses` |
| 9 | A `NewNpc` created in an earlier Turn | unconfirmed until `ConfirmAge` | `an_npc_made_in_play_blocks_until_confirmed` |
| 10 | A `NewNpc` with the name of a roster minor | the same-name rule of 3.2 | `a_namesake_of_a_minor_is_not_cleared` |
| 11 | A proposal fact on the key `age` | dropped; the gate reads `entity.age` | `a_fact_on_the_age_key_changes_no_clearance` |
| 12 | A bystander, a hidden character | `checked_here` holds world truth | `a_hidden_minor_refuses_and_names_nobody`; `g2_alone_refuses_a_hidden_minor` |
| 13 | An arrival in the same Turn | G3 | `an_event_that_brings_a_minor_refuses_the_turn` |
| 14 | A proposal that moves the minor away in the same Turn | G2 reads the place before the Turn, G3 after: both must pass | `moving_the_minor_out_in_the_same_turn_still_refuses` |
| 15 | A retry that answers `intimate: false` after an age refusal | no retry | `an_age_refusal_makes_no_second_proposal_call` |
| 16 | A Correction | it runs `plan` on `snapshot_before`: G1 to G4 as on any Turn | `a_correction_runs_the_same_gates` |
| 17 | `renarrate`, or a retry under the same `op_id` | every narration goes through `Engine::narrate` and G4 | `renarrate_of_an_ended_scene_is_implied` |
| 18 | A pack fragment that tells the model to ignore the rule | `clearance` and `checks` take no pack | `the_sample_pack_asks_to_ignore_the_age_rule_and_the_check_rejects`; `no_check_names_a_pack` (source search) |
| 19 | A pack that puts explicit rules in an always-on fragment | rule 7 of 3.8 | `a_capable_pack_with_a_tone_fragment_is_refused` |
| 20 | A pack removed, or a level changed, during a scene | `effective` and `told` read the state at each call | `a_removed_pack_gives_implied`; `excluded_closes_the_open_scene` |
| 21 | An old Campaign file | version 3 does not open; an old op has no `age_confirmed` | `a_version_3_file_is_refused`; `an_op_without_the_flag_is_unconfirmed` |
| 22 | The Opening Turn, the OOC call, the draft and World Build calls | `Plain`, no pack scene text, by construction | `no_scene_text_outside_a_play_narration` |
| 23 | An earlier scene Narration in a later prompt | `mature::turn_text` | `scene_narration_is_in_no_prompt_after_the_scene` |
| 24 | The tag is wrong: an intimate Intent with `intimate: false` | not held by code. See section 9. The tag covers any characters and a PC who only watches | the evaluation `intimate_tag`; `the_intimate_line_covers_any_characters` |
| 25 | A character the PC believes is here: a stale Belief, an unseen World Event, a Claim | `checked_here` holds `seen_present`; the narrator's Present line is a subset | `the_gate_checks_who_the_pc_believes_is_here`; `the_narrator_present_line_is_a_subset_of_the_checked_set` |
| 26 | A tagged proposal that names a minor who is elsewhere: a fact, a speaker, a Check, a stake, a factor, a schedule | G2 clears every character the proposal names; an unknown handle refuses | `a_tagged_proposal_checks_every_character_it_names`; `a_tagged_proposal_that_names_an_unknown_handle_is_refused`; `a_check_against_a_minor_in_a_tagged_turn_is_refused`; `every_proposal_field_is_sorted_for_the_gate` |
| 27 | A location written another way: case, accents, punctuation, a prefix, another name of the place; no location at all | the gate's place rule; unknown counts as here | `a_near_place_name_counts_as_here`; `a_character_with_no_place_counts_as_here`; `another_place_does_not_block` |
| 28 | A first answer that says the tag but does not parse | the sticky tag reads the raw text | `a_malformed_first_answer_keeps_the_tag` |
| 29 | The same op sent again after a Refusal | the Refusal is recorded under its op; no model call | `the_same_op_after_a_refusal_makes_no_model_call` |

## 7. Other acceptance tests

Each test of design section 9 has a home: `a_proposal_without_intimate_fails_the_schema`; `a_fragment_reaches_only_the_prompt_it_names`; `an_explicit_only_fragment_is_absent_at_implied`; `the_implied_line_is_absent_at_explicit`; `the_scene_state_survives_a_restart`; one test for each row of the table in 3.4; `a_fade_leaves_the_ledger_as_a_read_turn_does`; `fade_and_end_scene_commit_the_same_closing_ops`; `a_faded_turn_is_in_no_later_prompt_and_not_in_the_journal`; `rebuild_keeps_the_fade_and_finds_no_text`; `a_correction_reopens_the_scene_its_turn_closed`; `turn_makes_the_same_model_calls` (still one proposal call and one narration call).

The view tests of the play-experience shape run again on the hidden-world fixture with a scene: `play_view_shows_pc_knowledge_only`, `journal_view_shows_pc_knowledge_only`, and `a_refusal_names_only_perceived_characters`.

## 8. Failure modes

- **The model tags a plain Turn as intimate.** At `excluded`: one refused Turn. At another level: the Turn opens a scene if the place is cleared, and the Player ends it with the next plain Intent.
- **The model ends a scene by mistake** with `intimate: false`. The scene closes. The next intimate Turn opens a new one. No state is lost.
- **The app stops between `set_faded` and the Closing Turn.** After a restart the Turn is faded and the scene is open. Either button closes it.
- **The app stops during a stream.** `narration` is NULL and `faded` is 0: the Turn shows as `Missing` and can be narrated again, as today.
- **A pack is malformed.** It is skipped and listed. Play goes on at `implied`.
- **The Closing Turn's model call fails.** The default closing commits.
- **The token cap cuts a sentence.** The stored Narration ends at the last full sentence, without a last line that is a speech mark cut short (3.10). The stream showed a few more words of a long last line; it never showed a tag that the stored text leaves out.
- **A World Event keeps a scene Turn refused.** The Player reads the fixed sentence each time. A plain Intent lets the event fire.
- **`fade_to_black` arrives just after the Turn ended.** The UI sends it with the op_id of the Turn that was streaming. When that Turn is the latest Turn and a Play, Correction or Closing Turn, the core fades it (no text stored; changed after review, 2026-11-12, to match ADR 0011), and closes a scene it left open, faded. A fade with no op_id of a Turn: with a scene open it closes the scene, faded; with none it returns `NothingToFade` and the UI drops it.

## 9. Red-flag pass on this design

**Where it could leak to the Player.**

- A refusal for `adults_only` with no name tells the Player that someone in the place blocks the scene: a hidden character, or one under 18. Safety wins over secrecy here. The sentence is the same in both cases and names nobody.
- `to_confirm` tells that a perceived character has a stored age of 18 or more. That is the one age fact a Player screen shows.
- "Nobody here goes along with this" tells an NPC's will with no Narration. The design chose a rejection (6.2). Question 3.
- The neutral line holds no name and no value, so it cannot leak.

**Where the model could talk its way around a gate.**

- **A wrong tag (path 24).** This is the weakest point, and code does not close it. A Turn with `intimate: false` runs no age rule. What still holds: no explicit-only fragment and no scene Narration can be in its prompts, so the narrator has the core's rules only, as in the app today. What does not hold: the probe shows that the default model can write explicit prose from an explicit Intent alone. The evaluation measures the miss rate, with a subset where a minor is in the place. If it fails the pass line of question 6, the next step costs no repository text: the private pack ships a word list, and an Intent that matches it is treated as tagged. A pack can then make the gate run more often and never less.
- **A stranger who complies.** An NPC with no attitude row has attitude 0 and the ceiling Comply (`checks/mod.rs:86`). So an NPC met a minute ago may go along with no cause. The "NPC will" evaluation counts it. Question 4.
- **Fact values.** A fact of a scene Turn has a value of 40 characters of model text. It stays in the world and reaches later prompts and screens. Code cannot judge it. The cap is the only limit.
- **Shorter `minutes` to avoid a due arrival.** The arrival then fires in a later Turn, and G3 runs on that Turn.
- **The Closing Narration.** With no Intent to tell, the narrator may give the PC an act. Prompt only; the evaluation counts it.
- **A Narration at `Implied` that is explicit anyway.** Prompt only. The token cap and the evaluation are the limits. Later prompts get its neutral line once the scene ends.

**Where the repository could come to hold adult text.**

| Carrier | Where the text is | Mitigation |
|---|---|---|
| A Campaign file | `turn.narration`, `debug_log.prompt` and `raw`, fact values | the data folder is outside the repository; `.gitignore` gains `*.sqlite*` and `*.debug.jsonl`; test `the_repo_tracks_no_campaign_file` |
| An evaluation report or raw log | Intents, Narrations, prompts | the outside-path rule of 3.8: counts only, and `--out` outside the repository |
| A recorded model response in a test | hand-written text | tests use the sample pack only; `sample_pack_text_is_pinned`; `the_repo_tracks_no_other_pack` |
| A UI fixture or a view snapshot | story text | made from the neutral fixtures only |
| A debug dump pasted into a QA report, an ADR, a commit message | prompt text with pack fragments | `scripts/qa/private_text_scan.sh`: it reads the private pack folder from an environment variable and searches the work tree and the staged diff for any fragment line of 20 characters or more. It runs as a local pre-commit hook and does nothing when the variable is not set. It needs no word list in the repository |
| A pack error on screen, then in an issue | pack text in a message | `PackProblem` holds a rule name and a file name only |
| The core's own lines | the `intimate` definition, the implied line, the neutral templates | they are in the repository by design; the owner reads each one before the merge |
| This page, the ADRs, the glossary | wording | neutral words only |

The scan does not catch model prose pasted without its prompt. Nothing in the repository can, without a word list that the design forbids.

## 10. YAGNI cuts

- No table of scenes and no scene history. One nullable column.
- No stored neutral line. A function of the Turn row.
- No ledger op for a fade.
- No un-fade.
- No migration of version 3 files.
- No change of an age in play. A wrong model-stated age under 18 keeps that character out of every scene.
- No pack choice per Campaign, no reload without a restart, no pack hash in the ledger.
- No settings screen. One select in the Seed review, one in the OOC panel, one list in the home menu.
- No list of actions from the core for the action bar.
- No fade of the Opening Narration.
- No dynamic token cap from the time left in the Turn. If the p90 of scene Turns fails, that is the next step.
- No word list, in the repository or in the pack, until the evaluation asks for it.
- No interview additions. The loader reads and ignores the reserved table.
- No "Correct last Turn" across an OOC Turn or a Closing Turn.

## 11. ADRs to write

**0008. An age counts when the Player confirmed it, and an adults-only refusal is final.** Each PC and NPC in the Campaign file has `age_confirmed`. Only a confirmed Seed Draft and the `ConfirmAge` op set it. `mature::clearance` is the one adult rule: confirmed, 18 or more, no namesake that is not cleared. It runs before the proposal call of a scene Turn, in the Commit checks, after the World Events and before each narration. A refusal by this rule or by the `excluded` level makes no retry, because a retry prompt teaches the model to drop the tag. We rejected the model-stated age as enough, and a schema that pins the tag to false. Cost: the Player confirms a character met in play before a scene.

**0009. Explicit text enters a prompt only with a clearance value.** An explicit-only pack fragment, a pack scene fragment and the Narration of an earlier Turn of the scene need a `Cleared` value to enter a prompt. A pack that declares the explicit capability may hold scene fragments only. Fragments name one prompt. We rejected always-on fragments for such a pack: they would carry explicit rules into every prompt, with no gate. Cost: after a scene ends, the narrator and the GM know its committed facts and not its wording.

**0010. The Intimate Scene is one op and one column.** `SetScene` is written by `resolve` in the Turn's Commit, from the snapshot before the Turn and the Turn's ops. We rejected a view rule over stored proposals: the state gates prompts and checks, which read `WorldView`. The view rule stays as a test of the projection.

**0011. A Fade to Black stores no text and is not in the ledger.** The hidden Narration is never written to the `turn` table. `turn.faded` is the record of the skip. The neutral line is a function of the Turn row. `Gm::narrate` can be stopped by its reader. We rejected a ledger op with a carrier Turn, and a line written into `narration`. The partial text stays in the debug log (design 6.6), subject to question 5.

**0012. Old Campaign files are refused, not migrated.** Schema 4 refuses version 3, as 3 refused 2. The first migration will be written when a file exists that the owner cannot build again.

## 12. Glossary

Existing terms that this page uses as they are: Detail Level, Intimate Scene, Fade to Black, Content Pack, Content Boundary.

New terms, to land in `GLOSSARY.md` before any code:

- **Age Confirmation**: The Player's statement, recorded by the core, that a character's stored age is right. Only a character with one can be in an Intimate Scene. _Avoid_: age check, verification.
- **Closing Turn**: The Turn that ends an Intimate Scene when the Player presses "End scene" or "Fade to black". It has no Intent. _Avoid_: epilogue, cleanup.
- **Neutral Line**: The fixed sentence the core writes in place of a Narration that the Player skipped, or that a later prompt may not read. No model writes it. _Avoid_: summary, placeholder.
- **Refusal**: A Turn that the core stops for a Content Boundary, with no retry and no Commit. The Player reads one fixed sentence. _Avoid_: block, censorship, rejection (a rejection gets a retry).

Changed entries: **Turn** gains "A Closing Turn has no Intent." **OOC** gains "and settings the Player changes during play". **Content Pack** gains "A pack may declare that it gives explicit description; such a pack holds scene fragments only."

## 13. Build order

Structure first inside each slice, behaviour second, one commit each. Every slice ships and is tested with no private pack.

1. **The gates.** Schema 4 with its four columns. `age_confirmed`, `ConfirmedAges`, the `setup` argument. `DetailLevel` on the seed and the select in the Seed review. `mature::clearance`, `check`, `after`, `check_after`. `intimate` required, with its neutral line. `DetailExcluded`, `AdultsOnly`, the final refusal and its three sentences. The reserved `age` key. An intimate Turn is told with the core's implied line. Tests: paths 1 to 16 and 21, and the first six rows of section 5. No scene, no button, no pack.
2. **The scene.** `SetScene`, the projection, `scene_op` with the automatic endings, `SceneState::Intimate`, the scene line and the longer `plan`, `Told` with `Plain` and `Implied`, `turn_text` and the neutral line in later prompts. The `Gm::narrate` change lands first as a structure commit, with `None` and `More` everywhere; then the token cap.
3. **End scene.** `TurnKind::tells` and `has_intent`, then `Closing`, `end_scene`, `default_close`, the command and the button.
4. **Fade to Black.** `turn.faded` in use, `TurnControl`, the fade flag, `Engine::fade`, `TurnBody::Faded`, the Journal line, the `renarrate` refusal, the debug row, the button.
5. **Packs.** Format 2, `load_all`, the sample pack, the shell loads `<data_dir>/packs/`, `list_packs`, fragments by prompt, `effective`, `Told::Explicit(Cleared)`, the fallback notice.
6. **Settings in play.** The OOC Turn, `ConfirmAge`, `SetDetailLevel`, their commands, `RefusalView` with "Confirm", the select in the OOC panel.
7. **Evaluations and guards.** The scenarios `intimate_tag`, `scene_agency`, `npc_will` and `scene_latency`, at `implied`. `--packs`, `--scenarios-dir` and the outside-path rule. The scan script, the `.gitignore` lines, the repository tests of section 9.
8. **The owner's pass.** The private pack, his Campaign, both buttons, and the same four metrics outside the repository.

Slice 1 alone makes the adults-only rule run on every Turn that needs it, which is goal 1 of the design.

## 14. Design red flags checked

- **Shallow module:** `mature` has eleven functions over the adult rule, the same-name rule, the nine scene transitions, the effective level, the three telling modes and the neutral line. A caller does not learn those to use it. `pack::scene_fragments` hides the table of 3.8.
- **Information leak:** the adult rule had two homes (`checks::boundary` and `domain::is_adult` callers); it now has one. "What a reader gets for a Turn" is one function for `context` and `sight`. Two leaks are accepted. The pins of the closing schema are written twice, in `closing_schema_for` and in `settle`, because the validator must not trust the grammar (data shape 3.3 says the same of `maxItems`). `project::historical` must know the `faded` column.
- **Temporal decomposition:** G1 to G4 are named by time, and they are four calls into one module that holds one rule. Only `turn` follows the order.
- **Pass-through:** `checks::boundary` would only forward the adults-only rule, so `validate` calls `mature::check` directly and `boundary` keeps `min_age`. The five commands add the lock, the fade flag and the error text.
- **Complexity alarm:** it rings. One new module, three ops, one Turn kind, one trait change, about 30 files. Two variants were drawn (section 15). Slice 1 holds the schema change apart, and each widening change (`Gm::narrate`, `TurnKind` helpers) lands as a structure commit before its behaviour.

No innovation token is spent: flags, an enum, a column, a private constructor.

## 15. Variants compared

Two variants with opposite limits were drawn by separate agents, for the scene state, the fade, "End scene" and the age gate.

- **"No new committed state."** No op, no kind, no schema change: 19 files. The scene is a rule over the last standing Turn; a fade overwrites `narration` with the neutral line and is found again by string equality; "End scene" is a Play Turn with a reserved Intent; "confirmed" means "created by the Setup Turn". It showed that the scene state can be computed from data the file already has, and this shape keeps that rule as the oracle test of the projection. It broke where a gate needs memory: a Player can type the reserved Intent, a change of the neutral wording un-fades old Turns, a character met in play is shut out for the whole Campaign, and the fixture path counts as confirmed.
- **"Capture existing flows."** No new `Engine` method: about 29 files. "End scene" rides `play_turn` through an ask enum; a fade rides `renarrate` and a flag read after the stream; confirmation rides the Seed Draft only. It showed that the Closing Turn needs no pipeline of its own, only a different prompt and schema in `plan`, and that one `SetScene` op with a reason is enough. This shape takes both. It broke on the stream: the model writes the hidden text to the end while the lock is held. It also has no way to confirm a character met in play, and a Closing Turn of kind Play is offered to "Correct last Turn".

Both variants named the same weakest point without being asked to agree: the tag is model output, and a false tag skips every age rule. Neither found a fix in code. Section 9 says the same of this shape.

The shape takes the computed scene rule as a test from the first, and the single op and the shared `plan` from the second. It adds what both lacked: a stored confirmation, a stream that stops, and a value that a prompt needs before it can hold explicit text.

## 16. Decisions from the owner (2026-11-11)

The owner accepted every default of this section on 2026-11-11 ("ok for your defaults"). Each item below is now his decision. The numbers stay, so the references to "question N" in this page still point here.

1. **The Detail Level of a new Adventure Seed is `excluded`.** The Player picks `implied` or `explicit` in the Seed review. At `excluded`, a Turn that the model tags by mistake is refused. Rejected: `implied` as the default, which would turn the module on for every Campaign.
2. **An NPC that the model creates in play has an unconfirmed age** until the Player confirms it in play, with one click. The screen says "adult" and shows no number. Rejected: counting the model's age, with no human in that path.
3. **An intimate Turn with an NPC who does not go along is rejected**, as the design says (6.2), with the one retry. The Player reads "Nobody here goes along with this." The age rule runs first. Rejected: removing the tag and committing a plain Turn told in the NPC's own voice.
4. **A stranger may comply.** The existing stance ceiling stays: an NPC with attitude 0 passes the stance rule with no cause, and the "NPC will" evaluation counts it. Rejected: a higher bar for an intimate Turn (attitude 3 or more, or a cause).
5. **The debug log of a faded Turn keeps the text received before the stop**, as the design says (6.6). The log is written in every Campaign, debug mode or not. Rejected: logging `[faded]` and the number of characters only.
6. **The pass line for the tag:** on the `intimate_tag` scenario, no missed tag in the 10 runs where a minor is in the place, and at most 1 in 20 elsewhere. If the live model fails it, the next step is the pack word list of section 9, before the owner's pass. Rejected: none was offered.

The four smaller calls stand, also by the owner's decision of 2026-11-11: the Narration of an ended scene is replaced by its neutral line in later prompts; a Fade to Black between two Turns of a scene hides the Closing Turn and not the Turns already read; a settings change closes "Correct last Turn" until the next Play Turn; the confirmation in play shows no age.

## Left to the builder

- The neutral wording of the `intimate` field, the scene line, the implied line, the closing line, the stable narrator line and the three neutral templates. The owner reads each one.
- The measured values of `SCENE_NARRATION_MAX_TOKENS`, `CLOSING_NARRATION_MAX_TOKENS`, `SCENE_PLAN_MAX_CHARS` and the two fragment limits.
- The SQL of schema 4 and the message for a version 3 file.
- The marker sentences of the sample pack.
- The Intents of the four evaluation scenarios, in neutral words.
- The place of `intimate` in the schema order, with the measured miss rate.
- The sentence for each `PackRule`.

A review of this shape before the plan is useful. No reviewer has read it yet.
