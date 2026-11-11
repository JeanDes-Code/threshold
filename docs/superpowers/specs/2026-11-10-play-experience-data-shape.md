# Play experience: data shape

Date: 2026-11-10. Status: draft for review. Parent spec: `2026-11-10-play-experience-design.md`. This page extends `2026-11-08-threshold-data-shape.md` and `2026-11-10-campaign-from-document-data-shape.md` and does not restate them. Terms in capitals are defined in `GLOSSARY.md`. Numbers 1 to 10 name the calls that the design left to the architect.

This page wins over the design on mechanics. It does not change the intent, the goals or the anti-goals of the design.

## 1. Context

The design asks for six screens that show the world as the PC knows it. Today the shell builds two small views in `src-tauri/src/session.rs`, straight from `WorldView`. `WorldView` holds the whole world, hidden facts included. Each new screen would be one more place where a hidden fact can leak.

The central decision: **every Player screen reads one type, `PcSight`, and never `WorldView`.** One function makes `PcSight` from the world. It keeps only what the PC knows or perceives. A view function cannot show a hidden fact, because its input does not hold one.

The second decision: **the screens are views.** The ledger gains no op. The Decision Proposal gains no field. No model call is added. The Campaign file gains one projection column and two Player tables, so the schema version becomes 3.

```
ledger ─► projections ─► WorldView ─► context ─► prompts            (unchanged: the only road to a model)
                             │
turn rows ───────────────────┤
(Intent, Narration)          └─► world::pc_sight ─► PcSight ─┐
                                                             ├─► view::* ─► Tauri commands ─► UI
player tables ─► player::PlayerStore ─► Marks ───────────────┘
                 (owned by the shell; the Engine never holds it)
```

## 2. Boundaries

`threshold-core` gains three modules. `world` gains one file.

- `world::sight` (new file) owns `PcSight`. It holds five rules: who the PC can name, who is here, the Scene of each Turn, the state of each Open Item, and which Turn is a decision. It is the only door from `WorldView` to a screen.
- `speech` (new) owns the speech mark: the rule line that the narrator reads, and the parser. The writer side and the reader side of the format are in one module.
- `view` (new) owns the screen types and three functions. It takes `&PcSight` and `&Marks`. It does not import `store`.
- `player` (new) owns Notes and Pins: the types, the SQL text and the table definitions. Nothing in `turn`, `context`, `checks`, `world`, `build` or `view` imports `PlayerStore`.
- `build` gains the rename, the age confirmation, the Source Text length check and the cut report (3.8).
- `store` gains `Knowledge.turn`, a read-only `peek` for the home screen, and schema 3.
- `domain` gains `outcome_key` and `is_alias_key`. Today `narration_prompt.rs:174` strips the `pending_` prefix by hand, and `store/mod.rs:435` and `build/draft.rs:404` write `alias_` by hand.
- `src-tauri` owns the commands and holds no view logic. `Session::get_character` and `Session::get_journal` move their logic to `view`. `Open` gains a `PlayerStore`.
- `ui` owns layout, tokens, fonts, focus, the colour of an NPC (from `entity_id`) and the clock text. It parses no Narration and matches no name.

```rust
// world::sight
pub fn pc_sight(view: &WorldView, turns: &[StoredTurn], reverted: &HashSet<TurnId>) -> Option<PcSight>;  // None: no PC

// speech
pub const RULE: &str;                                   // one line, added to NARRATION_RULES
pub fn parse(narration: &str) -> Vec<Line<'_>>;         // Line::Prose(&str) | Line::Tagged { name: &str, words: &str }

// view
pub fn play_view(sight: &PcSight, marks: &Marks) -> PlayView;
pub fn journal_view(sight: &PcSight, marks: &Marks) -> JournalView;
pub fn entity_card(sight: &PcSight, marks: &Marks, entity: EntityId) -> Option<EntityCard>;   // None: the PC does not know it

// player
pub const SCHEMA: &str;                                 // the two tables; store::migrations runs it
impl PlayerStore { pub fn open(path: &Path) -> Result<Self, PlayerError>; pub fn marks(&self) -> Result<Marks, PlayerError>;
                   pub fn add_note(..); pub fn edit_note(..); pub fn delete_note(..); pub fn set_pin(..); }
```

## 3. Contracts

Each call gives the chosen option, the reason, and the cost if it is wrong.

### 3.1 Who speaks (1)

**Chosen.** The narrator writes a speech mark. An NPC line is a full line of the Narration: the name of the NPC, a colon, then the spoken words in quotes. `speech::RULE` asks for it. `speech::parse` reads it. The Narration is stored with its marks.

`view` shows a tagged line as a `Speech` block only when all of these hold:

1. `WorldView::entities_by_name` gives exactly one entity for the tag, and its kind is NPC.
2. That NPC is in the `present` list of the Turn (3.3).
3. The PC can name that NPC (3.5).

Any other line is a `Prose` block. A line that the PC speaks is prose: the Intent line already shows it.

**Changed after review (2026-11-10).** The first rule kept every such line with its text unchanged. A review found that this prints a name the PC does not know: `Kestrel: "Nobody knows my name."` showed "Kestrel" in the story while the panel said "unknown", and `The Toll-Wolf: "…"` showed a hidden other name. The rule is now:

- The tag is a name of an entity (main name or any other name, through `normalize::name`) and the PC cannot call that entity by that name (3.5: the PC cannot name the entity, or the tag is an other name without the PC's `alias_` Belief). The block is prose made of the quoted words only, with their quote marks. The tag and the colon are removed.
- The tag names more than one entity, and at least one of them is not nameable by the PC by that name. The tag is removed, as above.
- The tag names no entity at all. The text stays unchanged.
- Any other line that is not a bubble (an absent NPC the PC can name, the PC, a tag shared by entities the PC can all name) stays unchanged.

This covers the tag only. A name the narrator writes inside prose is not covered (ADR 0006).

**Changed after QA (2026-11-10).** The narrator tags a speaker by the first name: `Agathe:` for Agathe Morrow. That tag named no entity, so the line stayed plain text with its tag, and a stranger's first name printed. A tag now also names each character (PC or NPC) whose main name starts with the tag's words, through `normalize::name`: "agathe" names Agathe Morrow. Rules 1 to 3 and the removal rules above apply to that match as to a main name, and `world::unnameable_names` holds those first names too.

**Why.** It adds no model call and no proposal field. The cost is about three output tokens for each spoken line, which is an estimate and not a measurement. The speaker is the narrator's own statement, so it is as certain as the prose. Recent Turns in the prompt show the mark, and that helps the model keep the format. When the Gemma model drifts, the parser finds no mark and the line stays plain text.

Rejected:

- A guess from the prose (one name near a quote). "…, the guard tells Mara" gives a Mara bubble. That is the failure of design section 4.
- A structured Narration (JSON parts). It breaks the stream and the model drifts more on it.
- A spoken line in the Decision Proposal, committed before the Narration. It adds about 1 s to each Turn, and the proposal model writes worse dialogue.
- A model call after the Narration. It is a guess too, and it costs time.

**Cost if wrong.** If the model ignores the rule, the story has no bubble and reads as it does today. If the model tags a line with the wrong present NPC, the bubble is wrong. Code cannot see that case. The evaluation measures both: the share of spoken lines with a mark, and the share of wrong marks by the judge.

**Stream.** `narration_chunk` does not change. While the Narration streams, the UI shows the streamed text, marks included. The blocks arrive with the `PlayView` after the Turn commits. A running Turn holds the `Session` lock, so no view command can run during the stream.

**Changed after review (2026-11-10): the stream drops unknown tags too.** The streamed text showed a tag such as `Kestrel: "…"` until the Commit. The core now filters the stream, not the UI, since the UI parses no Narration. `speech::TagFilter` holds each line start until a colon, a newline or 48 characters. When the text before the colon is, through `normalize::name`, in `world::unnameable_names` (each name, main or other, of an entity the PC cannot call by that name), it drops that text, the colon and the blanks after it. The Engine reads the set once before each narrator call, for a play Turn, a Correction, the opening Narration and `renarrate`. It adds no model call. One measure at 300 Turns, in a release build: about 10 ms of store reads and 2 ms to compute, before a narrator call of several seconds. The stored Narration is unchanged; the commit-time rule above still handles it. A name written inside prose is still not covered (ADR 0006).

### 3.2 Decisions (2)

**Chosen.** A rule on committed data. A Turn is a decision of the PC when all of these hold:

- its kind is Play or Correction, and no later Turn reverted it;
- its stored `action` is `talk`, `move` or `act`, and its `verdict` is not `impossible`;
- at least one of: it has a Roll; it wrote a Persistent fact with the source `proposal`, `stake` or `correction`; it wrote or closed an Open Item that the PC knows.

*Changed after review (2026-11-11):* a Turn that closed an Open Item the PC knows is a decision whatever its action. Decisions follow `settles` (ADR 0007), and a close on an `observe` or `wait` Turn was missed.

*Changed in bug pass 4 (2026-11-11):* a Roll alone no longer makes a decision. Every arm-wrestle of QA pass 3 carried the DECISION badge, though it closed nothing and wrote nothing lasting: a contest decides no matter the PC holds. A Turn with a Check is a decision when it closes an Open Item, writes a Persistent fact, or writes an Open Item the PC knows, like any other Turn. The other two rules stay: a lasting witnessed fact and a new Open Item are things the PC chose that last.

```rust
pub fn stored_action(proposal_json: &str) -> Option<(ActionKind, Verdict)>;   // turn, next to stored_roll
```

**Why.** The store already makes a Turn fact Persistent only when the PC witnesses it and its key is not a state key (`store/project.rs:167`). So "wrote a Persistent fact" means "the PC did something that lasts, and saw it". A move, a mood or a look around does not count.

**Cost if wrong.** The Decisions list is too long or too short. The rule is a view: a change needs no new schema. A Pin on an act (3.7) lets the Player keep a decision that the rule missed. The next step, if the rule fails, is a required field in the Decision Proposal.

### 3.3 Scene boundaries (3)

**Chosen.** `PcSight` holds one `Moment` for each Opening, Play and Correction Turn that stands. A Scene is a run of Moments. A new Scene starts when the place of the PC changes. It also starts when one Turn moves the World Clock by more than `SCENE_GAP_MIN` (first value 240).

```rust
pub struct Moment { pub turn: TurnId, pub kind: TurnKind, pub place: Option<String>, pub clock_after: ClockMin,
                    pub present: Vec<EntityId>, pub learned: Vec<Belief>, pub decision: bool }
pub struct Knowledge { /* as today */ pub turn: TurnId }      // store; new projection column knowledge.turn_id
```

- `place` is the `location` of the PC after the Turn, from the PC `location` facts and their `created_turn`.
- `present` is the rule of `context::seen_present`, applied at that Turn: each entity whose `location`, as the PC believes it then, equals `place`.
- `learned` holds the PC `knowledge` rows of that Turn. A row that arrives later goes to the first Moment whose `clock_after` reaches its `arrives_at`.
- A row from a Setup Turn is "Known before the Campaign". It is in no Moment.
- A witnessed `location` of another entity is presence. It is not in `learned`.

One private function holds the Belief rule (the latest `learned_at` wins; only rows that arrived count). `world::beliefs` and `pc_sight` both call it.

**Why.** The Scene of the glossary is "one place and time". The ledger already says where the PC is and what time it is. The `knowledge` table has no Turn today, and the clock cannot give it: a Turn of zero minutes shares its clock with the next one. The ledger does hold the Turn of each `Know` op, so the new column is a richer projection, not new committed state.

**Cost if wrong.** Two spellings of one place ("back room", "tavern back room") split a Scene. A Turn that moves the PC and writes no `location` gives one long Scene. The text of an entry is never wrong. A change of the rule cuts old Journals again; a Pin names a Turn, not a Scene, so it survives.

*Changed in bug pass 4 (2026-11-11): what a Scene lists as learned.* QA read "Teodor Brandt's fatigue is now heavy.", "Teodor Brandt is at The Copper Kettle." and "Accepted: …" lines under "What you learned". A Scene's `learned` now leaves out the PC's own `location` (the Scene's title names the place) and the outcome fact of an Open Item (the Decisions and the closed items show it). A state of the PC that a Turn wrote, whose text is the core's sentence (`domain::slot_sentence`), reads in the second person on every Player screen (`domain::you_sentence`: "You are tired (heavy).", "You are injured (bruised wrist).", "Your reputation is now respected."). The "You" card and the Character screen's state lines use it too. A sentence the Adventure Seed wrote stays as it is.

### 3.4 When an Open Item is closed (4)

**Chosen.** A view rule on PC Knowledge.

- An Open Item is a PC Belief on an open key `(s, pending_k)`.
- It is closed when the PC holds a Belief from a fact (not from a Claim) on `(s, k)`, learned after the open one. `domain::outcome_key` gives `k`.
- It is closed in the Turn of that knowledge row. The Journal shows the Scene of that Turn.
- A new value on `pending_k` changes the item. It does not close it.
- A Claim does not close it, because a Claim can be false.

```rust
pub fn outcome_key(key: &str) -> Option<&str>;                // domain; None when key is not an open key
pub struct OpenItem { pub subject: EntityId, pub key: Key, pub text: String, pub closed_in: Option<TurnId>,
                      pub learned: usize }   // changed after review: the place of the PC's first row on it, in write order
```

*Changed after review (2026-11-11):* the Open lists of the views (`open_lines`: the side panel, the Character screen, the Journal) put the items about an NPC present now first, then the newest by `learned`. The side panel shows the first 5, and they came sorted by subject id. Both keys come from PC Knowledge and from what the PC perceives.

**Why.** The Open list must follow what the PC knows. If the world closes an item where the PC cannot perceive it, the item stays open on the screen. To remove it would tell the Player a hidden fact. The rule needs no code at Commit and no model output.

**Cost if wrong.** ADR 0005 names it: the model writes the outcome on another key, and the item stays open for good. That is stale and never a leak. Two fixes exist, in this order. First, a Player mark that hides an item, stored like a Pin, with no schema change. Second, a required status for each shown Open Item in the Decision Proposal, at about 50 output tokens for each Turn.

A closed item still reaches the prompts with its `(open, not done)` marker, as today. This project does not change that (section 5).

*Changed in bug pass 4 (2026-11-11):* an entity card reads open and closed from `PcSight::open_items`, the source of `open_lines`. QA saw a refused item on the cards of the Bargemen's Guild, Agathe Morrow and the Lamplighters with the open marker, and the "Refused:" line under it: `entity_card` listed every Belief about the entity, and marked each one on an open key as open. A card's `facts` and `named_in` now leave out the open line of an item that closed (`view::closed_item`); the item shows once, as its outcome line, not open.

### 3.5 The name the PC knows (5)

What exists: `Entity.name`, `Entity.other_names`, and one `alias_` fact for each other name, held by the characters in `known_to`. Nothing records that the PC knows a main name. The narrator prompt gets the main name of each present character (`narration_prompt.rs:247`).

**Chosen.** A view rule. The PC can name an entity when one of these holds:

1. it is the PC;
2. it is an entry of the Adventure Seed that is not `hidden` (the Player read that name in the review);
3. the PC holds a Belief about it that is not a witnessed `location`;
4. its main name is in a Narration of a Turn that stands, as whole words after `normalize::name`.

*Changed after review (2026-11-11):* rule 4 keeps the name's own capitals (`normalize::name_in`: accents and a leading article are dropped, and every capital of the name must be a capital in the text), and it never applies to a `hidden` Adventure Seed entry, which needs a Belief (rule 3). The Opening Narration "A circle of lamplight lies on the gate square" named the hidden Faction "The Circle", and "gate square" linked to the place Gate. The story links and a card's "named in" list use the same match.

```rust
pub struct Known { pub id: EntityId, pub kind: EntityKind, pub name: Option<String>, pub other_names: Vec<String> }
```

- `name` is the main name, or `None`. `None` with presence means "perceived, not named". The UI shows "unknown".
- `other_names` holds only the other names for which the PC holds the `alias_` Belief. No view reads `Entity.other_names`: that list holds names the PC does not know.
- `PcSight.known` lists the PC, each entity in a `present` list, and each subject or speaker of a PC Belief. No other entity is in `PcSight`. *Changed in bug pass 3 (2026-11-11):* it also lists each place and Faction the PC can name by the rule above. QA searched People and places for "Harrowmere", an Adventure Seed place named in 7 facts the PC held and the target of the main request, and got "No match": no Belief had the place as its subject, though `world::known_places` offered it as a move target. An NPC elsewhere still needs presence or a Belief.
- An entity with `name: None` has no fact line and no card text. Its only Belief is a witnessed `location`, and the text of that fact holds its name (`checks/ops.rs:209`).
- A known name in the story is a link. `view` finds it with the same match as rule 4 and returns it as a `Part::Name`.

**Why.** The Narration tells only what the PC perceives. So a name that the Narration printed is a name the PC heard. The rule reads committed data and text the Player already read.

**Cost if wrong.** A main name that is a common word ("Rose") can match too early. With today's narrator prompt, "unknown" ends at the first Narration that names the character. A stranger that stays unnamed for many Turns needs a prompt change: the narrator gets a description, not the name, for a character the PC cannot name. That is a different feature, and this project does not build it.

*Changed in bug pass 4 (2026-11-11): who may say an other name.* QA: Ludo Brenn called the PC "Wick" in three Narrations. The Adventure Seed gives "Wick" to Agathe Morrow and Sefa Lind only (`known_to`), and the `alias_wick` fact has those two and the PC as holders. The narrator prompt had the name unmarked in five places: the premise, the PC line ("also called Wick"), two PC Beliefs, and Ludo's own earlier lines in the recent Turns. One prompt serves every NPC of the Scene, so it cannot give each NPC its own names. Each prompt now marks every line that uses an other name with a note after the line: `[Wick: a name only Agathe Morrow and Sefa Lind know; nobody else says it]` (`context::alias_notes`, `context::mark_aliases`). The knowers are the Adventure Seed's `known_to`, without the bearer. The proposal prompt (the GM) marks every other name. The narrator and Begin prompts mark only the other names the PC knows (the PC's own, or one whose `known_to` holds the PC), and never name a hidden Adventure Seed entry as a knower: the narrator must not learn a name, or who knows it, that the PC does not know. The notes follow the data, so they do not change between two Turns of one Scene and the prompt prefix stays stable (ADR 0001). A World Build that adds hidden holders to an `alias_` fact does not change the notes.

### 3.6 View commands (6)

Changed and new commands. Every other command stays as it is.

| Command | Arguments | Returns | Source |
|---|---|---|---|
| `list_campaigns` | none | `Vec<CampaignCard>` | view: `store::peek` and the file time |
| `remove_campaign` | `card_id` | nothing | moves the card's file to `<data_dir>/archive/` (added after QA, 2026-11-10) |
| `open_campaign` | `db_path` | `CampaignSummary` (no `turns`) | as today |
| `get_play_view` | none | `PlayView` | view |
| `get_journal` | none | `JournalView` (new form) | view |
| `get_entity_card` | `entity_id` | `EntityCard`, or an error for an entity the PC does not know | view |
| `get_character` | none | `CharacterView` | view; *changed after review (2026-11-10):* it gains `state`, the state lines of `YouCard`, and the screen uses the cards of the Play screen. *Changed in bug pass 3 (2026-11-11):* it gains `open`, the side panel's Open list (`open_lines`); `facts` holds no open key and no outcome of an Open Item, and `CharacterFact` loses `open` |
| `add_note` | `body`, `about: Option<Target>` | `Note` | new state (3.7) |
| `edit_note` | `note_id`, `body` | `Note` | new state |
| `delete_note` | `note_id` | nothing | new state |
| `set_pin` | `target`, `on` | nothing | new state |

`campaign_summary` is removed. `CampaignSummary` and the `start_build` result lose `turns`. The Debug view takes its Turn ids from `PlayView.story`. The UI calls `get_play_view` once after `open_campaign`, and once after each `turn_committed`, `turn_failed` or note command.

```rust
pub struct CampaignCard { id: String, db_path: String, pc_name: String, premise: String, last_played_ms: u64,
                          turn_count: u32, needs_begin: bool, problem: Option<String> }

pub struct PlayView { pc_name: String, place: Option<PlaceLine>, clock_min: ClockMin, needs_begin: bool,
                      can_correct: bool, scene: SceneState, story: Vec<StoryTurn>, here: Vec<NameRef>,
                      you: YouCard, open: Vec<FactLine>, pinned: Vec<PinLine>, known: Vec<KnownLine> }
pub struct NameRef   { entity_id: EntityId, kind: EntityKind, name: Option<String> }
pub struct PlaceLine { entity_id: Option<EntityId>, name: String }
pub struct StoryTurn { turn_id: TurnId, kind: TurnKind, intent: String, roll: Option<RollResult>,
                       reverted: bool, body: TurnBody }
pub enum TurnBody    { Narration { blocks: Vec<Block> }, Missing }                    // tagged by `kind`
pub enum Block       { Prose { parts: Vec<Part> }, Speech { speaker: NameRef, parts: Vec<Part> } }
pub enum Part        { Text { text: String }, Name { entity_id: EntityId, text: String } }
pub struct YouCard   { name: String, concept: String, state: Vec<StateLine> }
pub struct StateLine { key: String, value: String, text: String }                     // a key of STATE_KEYS, not `location`; text: domain::slot_sentence (bug pass 3)
pub struct FactLine  { belief: BeliefRef, subject: NameRef, text: String, open: bool, told_by: Option<NameRef> }
pub struct BeliefRef { subject_id: EntityId, key: String }                            // a token; the UI does not show it
pub struct KnownLine { who: NameRef, other_names: Vec<String>, here: bool }           // named NPC, place or Faction
pub struct PinLine   { target: Target, title: String, text: String }

pub struct JournalView { scenes: Vec<SceneEntry>, open: Vec<FactLine>, closed: Vec<ClosedItem>,
                         known_before: Vec<FactLine>, notes: Vec<Note>, pins: Vec<Target> }
pub struct SceneEntry  { turn_id: TurnId /* its first Turn */, place: String, clock_from: ClockMin, clock_to: ClockMin,
                         present: Vec<NameRef>, acts: Vec<Act>, learned: Vec<FactLine> }
pub struct Act         { turn_id: TurnId, intent: String, decision: bool }
pub struct ClosedItem  { item: FactLine, closed_in: TurnId }

pub struct EntityCard  { who: NameRef, other_names: Vec<String>, here: bool, facts: Vec<FactLine>,
                         named_in: Vec<FactLine>, people_here: Vec<NameRef>, notes: Vec<Note>, pinned: bool }
```

*Changed after QA (2026-11-10):* a place card was empty ("Nothing more yet"), because facts sit on people. `people_here` lists, for a place card only, the characters the PC believes are located there. It comes from the PC's `location` Beliefs only (`PcSight.whereabouts`), matched to the place's names through `normalize::name`. A character the PC cannot name has a `NameRef` with no name. The card shows them under "Who you know is here".

*Changed in bug pass 3 (2026-11-11):* `named_in` lists, for a place or a Faction card only, the PC Beliefs about other subjects whose text names it, by its main name or an other name the PC knows, as whole words after `normalize::name_words`. The card shows them under "Named in". On the QA Campaign, Harrowmere's card lists 7 such facts.

Rules:

- `CampaignCard.last_played_ms` is the change time of the file. `turn_count` counts the Play and Correction Turns that stand. `pc_name` and `premise` come from the Adventure Seed. A file of schema 2 gives a card with `problem` set and cannot open. *Changed after QA (2026-11-10):* such a card has a "Remove from list" action. After a confirm step, `remove_campaign` moves the file (and its SQLite side files) into `<data_dir>/archive/`; it never deletes. It takes the card's own `id` (the file name), never a path, so only a file the list shows can move, and it refuses the open Campaign. *Changed after review (2026-11-10):* the shell, not the UI, refuses a card with no `problem`: a Campaign file that opens never moves. A side file that cannot move is reported as an error, never ignored.
- `YouCard.state` holds the PC Beliefs about the PC on a state key. The panel shows no other gauge.
- `PlayView.open` holds the Open Items with `closed_in: None`. The core holds no Lead today (section 8).
- The Decisions tab is each `Act` with `decision: true`, with the place and time of its `SceneEntry`.
- A reverted Turn stays in `story` with `reverted: true` and prose blocks only. It is in no `SceneEntry`.
- The home screen lists unfinished Seed Drafts with the existing `list_drafts`.

**Why one `PlayView` for each Turn.** The side panel changes with each Commit, so one call replaces five. **Cost if wrong.** The call returns the full story and grows with the Campaign. A test holds it under `PLAY_VIEW_MAX_MS` (first value 50) at 300 Turns. If it fails, `get_play_view` takes `after_turn` and returns the new Turns only.

The proof for each view. Each `..._only` test and `pc_sight_holds_no_hidden_world` run on the hidden-world fixture. Each one searches the serialized result for the hidden texts, values, goals, event texts, names and other names.

| View | Built from | Test |
|---|---|---|
| `PcSight` | `WorldView`, Turn rows | `pc_sight_holds_no_hidden_world`; `moment_matches_seen_present_of_snapshot_before` |
| `PlayView` | `PcSight`, `Marks` | `play_view_shows_pc_knowledge_only`; `unnamed_character_is_shown_unknown` |
| `JournalView` | `PcSight`, `Marks` | `journal_view_shows_pc_knowledge_only`; `seed_knowledge_is_known_before` |
| `EntityCard` | `PcSight`, `Marks` | `entity_card_shows_pc_knowledge_only`; `entity_card_refuses_an_entity_the_pc_does_not_know` |
| `CharacterView` | `PcSight` | `character_view_shows_pc_knowledge_only` |
| `CampaignCard` | seed premise, PC name, Turn count | `campaign_card_reads_the_seed_and_counts_turns_only` |

Other tests that this page needs: `speech_line_without_a_certain_speaker_stays_prose`, `open_item_leaves_open_in_the_turn_that_closes_it`, `offscreen_outcome_keeps_the_item_open`, `turn_makes_the_same_model_calls` (one proposal call and one Narration call, as today).

### 3.7 Notes and Pins (7)

**Chosen.** Two tables in the Campaign file, outside the ledger. `player` owns them. Schema version 3. `Store::open_existing` refuses a version 2 file and runs no migration, as it does today for version 1.

```rust
pub enum Target { Scene { turn_id: TurnId }, Act { turn_id: TurnId }, Belief { subject_id: EntityId, key: String },
                  Entity { entity_id: EntityId }, Note { note_id: u64 } }                // tagged by `kind`
pub struct Note  { pub id: u64, pub body: String, pub about: Option<Target>, pub created_ms: u64, pub updated_ms: u64 }
pub struct Marks { pub notes: Vec<Note>, pub pins: Vec<Target> }
```

- Tables: `player_note` and `player_pin`. `player::SCHEMA` holds their definition. The builder writes the SQL.
- `Scene { turn_id }` means "the Scene that holds this Turn". A change of the Scene rule does not break it.
- A Note cannot be about a Note. `body` has `NOTE_MAX_CHARS` (first value 2,000). `delete_note` also removes the Pin of that Note.
- A target that no longer resolves (a Belief that a Correction removed) is not lost. Its Pin is not shown. Its Note is shown as a free Note.
- A Pin or a Note on a reverted Turn follows the `correction` row to the Turn that replaced it.
- The shell stamps `created_ms` and `updated_ms` from the system clock.
- `Store::rebuild_projections` does not touch the two tables.

**Proof by construction that a Note cannot reach a prompt.**

1. A prompt is made only by `context`, from `&WorldView`, `&[ContentPack]` and the Intent text.
2. `WorldView` has no Note field. `store::project::snapshot` reads no `player_` table.
3. `PlayerStore` has its own connection. The shell owns it, in `Open`, next to the `Engine`. The `Engine` holds no `PlayerStore` and no `Marks`, so no function of the Turn can read one.
4. `view` returns data to the UI only. No function takes a `view` type and returns a `Prompt`.
5. No command sends Player text to the `Engine`, apart from the Intent, the Correction and the OOC question.

Tests: `note_and_pin_survive_a_restart`; `note_text_is_in_no_prompt_of_the_next_turn` (a marker text in a Note, then one Turn and one OOC call, then a search of each `debug_log.prompt`); `no_turn_module_names_the_player_store` (a search of the sources of `turn`, `context`, `checks`, `world` and `build`); `rebuild_keeps_notes_and_pins`.

**Cost if wrong.** A second connection on one file is safe only while the `Session` lock orders the writes. A note command waits for a running Turn. If the Player must write a Note during the stream, `PlayerStore` gets its own lock outside the `Session`.

### 3.8 Review screen (8)

**The core owns the age confirmation.** The draft stores the age that the Player confirmed, not a flag.

```rust
pub struct AgeMark { pub drafted: Option<u16>,    // the age the draft stage gave; None for an entry the Player added
                     pub stated: bool,            // the model says the text states it; information only
                     pub confirmed: Option<u16> } // the age the Player confirmed through the core
pub enum AgeSource { FromText, Proposed, Edited }  // derived, never stored
pub struct AgeLine { name: String, source: AgeSource, confirmed: bool }

SeedDraft.ages: BTreeMap<String, AgeMark>          // key: normalize::name of the main name; replaces ages_to_confirm and ages_stated
SeedDraft.cut:  Vec<Cut>                           // lists that the draft run cut at their cap
pub struct Cut { pub at: String /* same path form as Gap.at */, pub cap: u32 }

// build
pub fn confirm_age(draft: &mut SeedDraft, name: &str) -> Result<(), ReviewError>;
pub fn rename(draft: &mut SeedDraft, from: &str, to: &str) -> Result<(), ReviewError>;
pub fn apply_edit(draft: &mut SeedDraft, seed: AdventureSeed, knowledge: Vec<KnownFact>);
pub fn check_source_text(text: &str) -> Result<(), SourceTextProblem>;   // Empty | TooShort { min } | TooLong { max }
pub const SOURCE_TEXT_MIN_CHARS: usize;            // first value 400
```

- An age is confirmed when `confirmed == Some(the current age)`. So an edit of an age removes the confirmation, on any path. `build::gaps` reports `AgeUnconfirmed` from this rule.
- Only `confirm_age` writes `confirmed`. `apply_edit` cannot: it does not take the map. A UI bug can no longer confirm an age.
- `AgeSource` is `Edited` when the current age differs from `drafted`, or when `drafted` is `None`. Else it is `FromText` when `stated`, and `Proposed` when not.
- An old draft file has no `ages` field. It loads, and each age needs a new confirmation.

**Changed after review (2026-11-10): the age mark follows the exact entry.** The first rule keyed a mark by `normalize::name` of the main name. A review found three holes. "jéan", added beside a confirmed "Jean" of the same age, took the confirmation once "Jean" was deleted. A rename named its entry by name only, so with a place and a character of one name it could move the character's mark to the place. And an old draft file that said `approved` built with no confirmation at all, because the build checked the state only. The rule is now:

```rust
pub struct AgeMark { pub name: String,             // the entry's main name, exactly as written
                     pub kind: EntityKind,         // Pc or Npc
                     pub drafted: Option<u16>, pub stated: bool, pub confirmed: Option<u16> }
pub struct AgeRows { pub pc: Option<AgeMark>, pub entities: Vec<Option<AgeMark>> }   // one slot per row
pub struct EntryRef { pub index: Option<usize> /* None: the PC */, pub name: String, pub kind: EntityKind }

SeedDraft.age_marks: AgeRows                       // replaces `ages`; an old `ages` map is ignored
pub fn confirm_age(draft: &mut SeedDraft, entry: &EntryRef) -> Result<(), ReviewError>;
pub fn rename(draft: &mut SeedDraft, entry: &EntryRef, to: &str) -> Result<(), ReviewError>;
```

- A mark counts for a row only while the row holds a character with the mark's exact main name and kind, and its age equals `confirmed`. An age changed and set back to the confirmed value on the same entry is confirmed again.
- `EntryRef` names one row, with its exact main name and kind as a check. `confirm_age` and `rename` refuse an entry that its row does not hold (`UnknownName`): the screen is out of date. So a rename moves only the mark of its own row.
- `apply_edit` keeps a mark only while its row still fits it. A renamed entry, an entry that became a place, and a name that comes back are new entries. Marks follow rows by position. A save that removes exactly one row and leaves the others as they were moves the later marks up by one. Any other change of the row count keeps the positions, so a later row whose entry changed loses its mark: a delete and an add in one save leave both unconfirmed. Limit: deleting the last row and adding the same name, kind and age in its place in one save gives the same data, and the core cannot see it.
- `approve` and `build_world` compute the gaps again. A draft with any gap is refused with its gaps, and an `Approved` draft goes back to `Draft`. `start_build` saves it back in `Draft` and returns the gaps; the UI opens the review, not the build.
- The UI shows "confirmed", and enables "Confirm age", only while the age field shows the very age that the store holds: no waiting or failed save, and no text in the field that differs from the stored age. The age field takes decimal digits only. `newCampaign.ts` holds this rule (`ageStatus`).

| Command | Arguments | Returns |
|---|---|---|
| `save_draft` | `draft_id`, `seed`, `knowledge` | `DraftView`, state back to `draft` |
| `confirm_age` | `draft_id`, `entry` (changed after review: was `name`) | `DraftView` |
| `rename_draft_entity` | `draft_id`, `entry`, `to` (changed after review: was `from`) | `DraftView`, state back to `draft` |

`DraftView` loses `ages_to_confirm` and `ages_stated`. It gains `ages: Vec<AgeLine>` and `cut: Vec<Cut>`. After review, `AgeLine` also carries the row (`index`) and `kind` of its entry.

**Rename.** `build::rename` changes each field that names the entry: the main name, each `known_to`, `start.place`, `start.present`, the `subject` of each known, decided and proposed fact, and the key of `ages`. It also changes the old name in the text of each known fact and in `start.situation`, as whole words with the same spelling. It refuses a name that another entry has, and then changes nothing. The UI sends each name change through this command. A name changed through `save_draft` is a new entry for the core, and its age is not confirmed. That direction is the safe one.

**Add a character or a place.** No new command. The UI adds one entry to `seed.entities` and calls `save_draft`. The entry has no `AgeMark`, so its age is `Edited` and not confirmed. The World Build already treats each roster NPC the same way. Test: `added_character_reaches_the_world_build`.

**Source Text length.** `check_source_text` is the one check. `build::draft_from_text` and the shell both call it. Today the shell and `draft_stages.rs:46` each compare the length. Changed after review: both limits count the text without the blanks at its ends. Before, the 40,000 cap counted them, so a full text with two trailing newlines was refused.

**Cut lists.** The stage runner already cuts a list at its cap (`build/stages.rs:181`) and tells no one. It now returns each cut, and the draft keeps them. `apply_edit` keeps `cut`.

**Cost if wrong.** A rename of a name that is a common word ("Rose") can change another word in a fact text. The Player reads the review before approval. If that happens in practice, the text change becomes a list that the Player accepts line by line.

### 3.9 The seam for the mature content module (9)

`PlayView` exposes three things. That module then adds variants and does not change a field.

```rust
pub enum SceneState { Plain }                       // tagged by `kind`; that module adds Intimate
pub enum TurnBody   { Narration { .. }, Missing }   // that module adds Faded { line }
```

1. `PlayView.scene`. The UI reads the tag. An "Intimate Scene" state is one more variant.
2. `StoryTurn.body`. A Fade to Black is one more variant with its neutral line. `SceneEntry.acts` holds the Intent and no Narration, so the Journal needs no change.
3. The action bar is one UI component with a named place on its right. It gets `scene` and the busy state of the Turn. The two buttons decide from these two values if they show.

This page defines no list of actions from the core. Two buttons do not need one.

**Cost if wrong.** If that module needs a button whose rule the UI cannot derive, `PlayView` gains an `actions` list. That adds a field and reshapes nothing.

## 4. Failure modes

- **The model drops the speech mark.** The line is prose. No error. The evaluation counts it.
- **The model tags the wrong present NPC.** A wrong bubble. Code cannot see it. The judge metric counts it.
- **Two entities share a name.** The tag resolves to two entities, so the line is prose.
- **Key drift on an Open Item.** The item stays open. See 3.4.
- **The Narration failed after the Commit.** `TurnBody::Missing`. `renarrate` works as today.
- **A view command during a Turn.** It waits for the `Session` lock. The UI calls views after the Turn ends.
- **A schema 2 file.** The home screen shows its card with `problem`. It does not open. The owner imports again.
- **The file time is wrong** after a copy or a backup. "Last played" is wrong. Nothing else reads it.
- **A write of a Note fails.** The command returns the error. The world and the Turn are not affected.
- **A hidden other name.** `Entity.other_names` holds it. `view` cannot read it, because `Known.other_names` comes from PC Beliefs only.
- **Debug mode is on.** The views do not change. Only `get_debug` and `get_build_debug` show hidden content, as today.
- **A rename to a name that exists.** Refused. The draft does not change.

## 5. YAGNI cuts

- No ledger op and no table for a Scene. The rule is a view.
- No model call for a Journal entry. The design allows one after the Narration; the plain entry is enough for the first version.
- No bubble and no link during the stream. Bubbles come when the Turn commits. Slice 1 shows this change on fixture data. If the owner refuses it, the `Engine` gives the `present` list before the Narration starts, and the shell parses each chunk with the same `speech::parse`.
- No stored "the PC knows this name" fact, and no description for a stranger.
- No code that closes an Open Item in the world. No change to the `(open, not done)` marker in prompts. Cost: the GM can still read a closed item as open. The fix is one filter in `context` that calls the Open Item rule.
- No Player mark to close an Open Item by hand. It is the first fix if key drift is high.
- No wall-clock time on a Turn. The file time gives "last played".
- No `after_turn` argument on `get_play_view` before the budget test fails.
- No list of actions from the core for the action bar.
- No Item in "People and places". The design lists persons, places and Factions.
- No own lock for `PlayerStore`.
- No saved OOC answer. The UI keeps it for the session.
- No new command to add a draft entry.

## 6. ADR to write: the speaker of a line is a mark that the narrator writes

The narrator writes the name of an NPC before a spoken line, in a fixed line form. The core shows a bubble only when that name resolves to one NPC that is present and that the PC can name. We rejected a guess from the prose: it can name the listener as the speaker. We rejected a spoken line in the Decision Proposal: it adds about 1 s to each Turn. The cost: stored Narrations hold the mark, and a model that tags the wrong NPC gives a wrong bubble. It is easy to reverse: the parser stops, and the mark reads as plain text.

## 7. Next step

Structure first, behaviour second. One commit per step.

1. **The Play screen on fixture data.** The TypeScript types of 3.6, one fixture `PlayView`, the tokens file, the two fonts, the layout of direction B. No core change. The fixture holds a bubble, an unsure line, an unknown character, Open Items, a Pin, and the change from stream text to blocks. The owner approves the look here.
2. **Structure only.** `Drafts` moves out of `session.rs`. `domain::outcome_key` and `is_alias_key` replace the three hand-written prefixes. The logic of `get_character` and `get_journal` moves to a `view` module with the same output.
3. **Schema 3.** `knowledge.turn_id`, `Knowledge.turn`, `player::SCHEMA`. Fixtures and tests move. No new behaviour.
4. **`PcSight`.** Names, Moments, Open Items, decisions, with the tests of 3.6.
5. **`speech`, `play_view`, `get_play_view`.** The narrator rule line. The Play screen reads live data. The budget test and the model-call test.
6. **`get_entity_card`** and "People and places".
7. **`journal_view`** and the Journal screen.
8. **`player`**: Notes, Pins, their commands and their tests, then the UI.
9. **Home.** `store::peek`, `CampaignCard`, the Campaign menu.
10. **Review.** `AgeMark`, `confirm_age`, `rename`, `check_source_text`, `Cut`, then the review screen.
11. **The evaluation** of the speech mark, then the QA pass on the live model.

A review of this shape before the plan is useful. No reviewer has read it yet.

## 8. Limits against the design, and open questions

- **Leads.** Design 5.3 lists Leads under Open. The core stores no Lead. The Open list shows Open Items only, by the anti-goal "no state that the core does not hold".
- **Bubbles after the stream.** See section 5.
- **Fact text from play.** A fact that a Turn writes had the text `name: key = value`. Changed in bug pass 3 (2026-11-11): the core builds a plain sentence from a fixed template per key (`domain::slot_sentence`, no model text), and `view` reads an old `name: key = value` text as that sentence. A key with no template reads "<name>'s <key words> is now <value words>.".

No question needs the owner before the build. The first two limits are calls that the owner can veto.

## Design red flags checked

- **Shallow module:** none found. `pc_sight` is one function over five rules (names, presence, Scenes, Open Items, decisions). `speech` has one rule line and one parser. `view` has three functions over the story blocks, the name links and the join with `Marks`. `player` has five functions over two tables.
- **Information leak:** four, and each has one owner now. The `pending_` and `alias_` prefixes go to `domain`. The Belief rule stays one private function for `beliefs` and `pc_sight`. The speech format lives in `speech` for the writer and the reader. Name matching stays in the core; the UI copy `nameKey` gets no new use. One leak is accepted: `store::migrations` runs `player::SCHEMA`, so the version number and the Player tables have two owners.
- **Temporal decomposition:** avoided. `PcSight` groups by what the PC knows, not by "load, filter, render". The age rules of the draft, the review and the approval stay in `build`.
- **Pass-through:** the Tauri commands stay thin. They add the lock, the error text and the `PlayerStore`. `Session::get_character` would only pass through after step 2, so it calls `view` directly.
- **Complexity alarm:** it rings. The work adds three core modules and touches more than eight files. Two variants were drawn before this shape (below). Steps 2 and 3 hold the structure changes apart from the behaviour.

## Variants compared

Two variants with opposite limits were drawn for calls 1, 3, 4 and 5.

- **"Views only, no new state, no prompt change."** It showed that calls 3, 4 and 5 need no committed state. It showed that call 1 has no certain answer without one line in the narrator rules. It gave the oracle test: a Moment must equal `seen_present` of `snapshot_before` for the same Turn.
- **"Commit all at Turn time, no rule at read time."** It cost about 115 output tokens for each Turn. That is about 1.9 s by estimate, against anti-goal 4. It relied on proposal fields, and the probe of the mature content design measured an optional field at 0 of 15. This shape keeps two of its findings. A Pin must name a Turn, so that a new Scene rule does not break it. A wrong bubble under a mark needs the narrator itself to name the wrong NPC.

The shape takes the views of the first variant and the one prompt line that it named as its smallest addition. It takes the Turn anchor from the second.

## Left to the builder

- The exact glyphs of the speech mark and the wording of `speech::RULE`, with the measured share of marked lines.
- The values of `SCENE_GAP_MIN`, `PLAY_VIEW_MAX_MS`, `NOTE_MAX_CHARS` and `SOURCE_TEXT_MIN_CHARS`.
- The SQL of schema 3 and the message for a version 2 file.
- The line that `view` makes from a play fact.
- The hidden-world fixture for the view tests, with a hidden other name and a character that no Narration names.
- The sentence for each `SourceTextProblem`, `ReviewError` and `Gap`.
- The fonts, the palette, the tokens file, the narrow-window breakpoint, and the colour of an NPC from its `entity_id`.

## 9. Changed after the review of bug pass 4 (2026-11-11)

Nothing in this section was measured on the live model; deterministic tests cover each rule.

- **How an item ended (M3).** After 924b8e2 the Journal no longer said how an item ended: "Refused: …" left "What you learned", and the Closed list and Decisions showed the item's open text with a check mark only. `OpenItem` now carries `outcome`, the PC's Belief that closed the item. `ClosedItem` carries `outcome`, the word the Player reads ("Accepted", "Refused", "Done", "Failed", "Dropped", or the value an event stated, "Raised"), and `scene`, the first Turn of the Scene that closed it. `Act` carries `closed`, one `ClosedLine { outcome, text }` per item its Turn closed. The Journal's Closed list and Decisions read "Refused · <item text>". The side panel shows no closed item. The core's outcome lines stay out of "What you learned".
- **News on an open item's outcome is learned (minor).** Brann telling `debt: doubled` while `pending_debt` stays open vanished from "What you learned" and from the OOC sheet, and showed on the card. A told Claim never closes an item (3.4), so it is something learned: both now show it. Only the outcome line the core built for a `settles` (value and text from `CloseOutcome`) is left out of "What you learned"; the OOC sheet and the Character screen leave out the Belief that closed an item, which the closed lists show.
- **Open for the PC (M2).** 3.4's rule reads PC Knowledge only, as before, but it is now the one rule for every path, the offer of `settles` included (data shape of 2026-11-08, section 13). A row that closes an item is a fact on the outcome slot, or the PC's own Claim on it.
- **Who may say an other name (M5, B2).** The bracket notes of 3.5's bug pass 4 paragraph are gone: they marked the Player's Intent and the narrator's own lines, and the model could copy one into the story. The narrator, Begin and proposal prompts carry one `NAMES` block in their stable part instead, with the knowers read from the store and resolved to entities. The rule is in the data shape of 2026-11-08, section 13.
