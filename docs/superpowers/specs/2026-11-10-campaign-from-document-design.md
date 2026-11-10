# Campaign from a document: design

Date: 2026-11-10. Status: draft for review.
Sources: the design conversation of 2026-11-09 and 2026-11-10, two spikes run in that conversation (section 4), `GLOSSARY.md`, ADRs 0001 to 0004, the first-slice design (`2026-11-08-threshold-design.md`).
Terms in capitals are defined in `GLOSSARY.md`. Section 9 lists the new terms.

The data shape is in `2026-11-10-campaign-from-document-data-shape.md`. Where the two differ on a mechanism, the data shape wins: it was written against the code.

This is project 1 of 3. Project 2 is the mature content module (`2026-11-10-mature-content-module-design.md`). Project 3 is the Discovery interview and the "surprise me" entry. Project 3 has no design yet.

## 1. Intent

Today a Campaign starts from two hand-written TOML files. The owner cannot test Threshold on a game he cares about.

The owner plays a long campaign in a chat product. He has a written chronicle of it: characters, settled events, open threads, and the point where play stopped. He wants to give that text to Threshold and resume from that point.

A Campaign must start in one of three ways. The Player brings a defined idea. The Player takes the Discovery interview. The Player asks for a surprise. The app never forces the interview. This project builds the first way and the step that all three share.

All three ways end with an approved Adventure Seed. After approval, the app must build the hidden world: NPC goals, secrets, Voice Cards, World Events. This step is the World Build. It is the same for the three ways, so this project builds it once.

## 2. Goals

1. The Player gives the app a text. The app drafts a player-facing Adventure Seed from it.
2. The Player reads the draft, edits it, and approves it. Approval does not start play.
3. The app builds the hidden world from the approved seed and the source text. The Player does not read the hidden world.
4. Code checks the hidden world. Nobody else does.
5. Play starts on an explicit "Begin", at the point where the source text stopped.

Done means: the owner resumes his private chronicle and plays 30 Turns, and every acceptance test in section 8 passes.

## 3. Non-goals

- The Discovery interview and the "surprise me" entry (project 3). The New Campaign screen shows one entry now.
- Explicit content, intimate scenes and their controls (project 2).
- A second Ruleset. The seed records the resolution agreement as text. The d10 prototype stays the only Ruleset.
- Import of a raw chat export. The input is a summary document, written by a person or a model.
- Transfer of another GM's secret state. The World Build creates new secrets.
- Tactical combat, maps, other genres' special UI.

## 4. What the spikes showed

Both spikes ran on 2026-11-10 with `gemma4:26b-a4b-it-qat`, thinking off, on the target machine. The input was the owner's private chronicle of about 15,000 characters (3,400 prompt tokens). The scripts were throwaway and are not in the repository.

Three World Build runs, three stages each (extract, hidden state per NPC, events), took 2.5 to 4 minutes per run.

| Result | Count |
|---|---|
| All seven named NPCs found | 3 of 3 |
| Premise and stopping point correct; a planned event stayed planned | 3 of 3 |
| Distinct Voice Cards in the story language | 3 of 3 |
| The PC's second name used as a separate subject | 3 of 3 |
| Facts extracted in one call: 11 or 12, from a text that holds several dozen | 3 of 3 |
| Groups declared as NPCs | 3 of 3 |
| Events with no effect on the world, written as long prose | 3 of 3 |
| A link between two entities that the text does not state | 1 of 3 |
| Most NPC goals and secrets are about the PC | 3 of 3 |

One run with thinking on returned cut-off JSON after several minutes. The cause is not confirmed. Thinking for the build is untested, not rejected.

Conclusion: the default model can do the World Build as small staged calls with a code check after each stage. One large call is too thin.

## 5. Anti-goals: it fails even if it technically works when

- the build invents a past event or a past Player decision;
- a planned or uncertain item in the source becomes a fact that it happened;
- the Player sees a secret during the build, in an error message, or in a progress label;
- the world turns around the PC: every NPC's goal and secret is about the PC;
- a failed build leaves a half-made Campaign on disk.

## 6. Shape

```
Player text ──► Seed Draft (seed, roster with ages, what the PC knows)
                     │
                     ▼
        Player edits, answers gaps ──► approves
                                          │
                   (hidden from the Player from here)
                                          ▼
   Hidden facts ─► Hidden state per NPC ─► World Events ─► Start state
        │                  │                    │              │
        └─ after each stage: code checks, one repair call, then drop or fail ─┘
                                                                │
                                                                ▼
                              Setup Turns committed ──► "Begin" ──► opening Narration
```

### 6.1 The Adventure Seed grows

The seed gains the parts of the owner's Threshold template that the type lacks:

- the PC's age and other names (section 6.4);
- the resolution agreement, as text;
- themes and Content Boundaries;
- the starting situation: place, who is present, what is going on;
- the GM operating agreement, as text;
- open details: delegated to the GM, to discover in play, still to agree.

Each choice keeps its state: decided, proposed or open, as the glossary says.

The seed goes at the start of every prompt (ADR 0001). The builder must keep the seed block small enough for the Turn budget and must measure it.

### 6.2 Seed Draft and approval

Model calls draft three things from the Source Text:

1. **The seed** (6.1).
2. **The roster.** Every named character, place, group and significant object, with its kind. Each character has an age (6.4).
3. **PC Knowledge.** The facts the PC knows, extracted section by section, not in one call. Each has a status, settled or open. These facts appear in the Journal after "Begin", so the Player must be able to check them first.

The draft holds only player-facing content. It never holds an NPC's private feeling or a hidden cause, even when the Source Text states one. The World Build extracts that content later, as facts with limited Visibility.

The Player edits every text field, removes or fixes a fact, and confirms each age. The app lists what is missing and essential, one item at a time.

The draft and the approved seed live in a draft store in the app's data folder, apart from any Campaign file. A restart keeps them. A failed or cancelled World Build keeps them, so the Player can retry without a new review.

Approval does not start the World Build without the Player's action, and it does not start play.

**Mature content in the Source Text.** The draft neither removes it nor refuses the text. It records what the text states as facts, in neutral words, and records the themes in the seed. Until project 2 lands, the Narration has no instruction for explicit detail, and the Content Boundary check that exists stays in force.

### 6.3 The World Build

Four stages, in order. Each stage is one or more schema-constrained calls. Thinking is off. The build adds no character from the Source Text: the roster is fixed at approval.

1. **Hidden facts.** What the Source Text states and the PC does not know, extracted section by section, with the list of who knows each fact.
2. **Hidden state per NPC.** One call per NPC: goals, one secret, attitudes toward named entities, a Voice Card. The call sees the Voice Cards written before it and must differ from them.
3. **World Events.** Three to six, each tied to an open thread, a goal or a secret. Same structured form as an event that a Decision Proposal schedules: actor, short text, effects as data, witnesses. No event sets a voluntary act of the PC.
4. **Start state.** The PC's place, who is present, the World Clock.

In the draft and in the build, a settled fact follows the Canon Level rules that exist. An open item is stored so that no prompt can read it as done. The exact representation is the architect's call.

After each stage, code checks the output (6.5). A failed check sends the stage back once with the errors. If the repair fails, an optional item is dropped: one event, one secret, one attitude. An essential item fails the build: the start place, an NPC's Voice Card.

The build commits nothing until every stage passes. A failed or cancelled build removes the Campaign file, as `new_campaign` does today. The draft store keeps the approved seed.

The Player sees progress by stage name and count only: "Preparing characters, 4 of 9". Never names of hidden items, never model text.

The build has no 15 s budget. It may take several minutes. The Player can cancel it.

A failed build shows the Player the stage and the check that failed, never model text. Prompts and raw outputs of the build go to the debug log. The debug view shows them only behind its toggle, with a warning that they reveal secrets.

### 6.4 Ages and names

Every character has an explicit age in years: the PC, each NPC from the source, each NPC the model creates later in play. This replaces the `adult` flag. Code derives adult status as age 18 or more. Code never assumes that a character is an adult.

Where the Source Text gives no age, the model proposes one that fits the text. The seed review shows every age, the PC's included, with the model's claim ("from the text" or "proposed by the model"). The Player confirms or changes each one: the claim never skips a confirmation, because the hard limit on intimate scenes reads the stored age. So every age of a source character passes the Player's review. A Player statement in the seed, such as "every character is an adult", is a constraint on the model. Code still checks each stored age.

A character may have several names. Every name resolves to one entity. Whether a given NPC knows that two names are one person is a fact with holders, like any other knowledge. The spike showed that the model flattens this. This slice stores the fact and its holders. It does not yet enforce that an NPC without the fact acts as if they were two people. The evaluations measure it.

### 6.5 Checks on the World Build

Deterministic code:

| Check | Rule |
|---|---|
| References | Every name used in a fact, attitude, event or start state resolves to a declared entity. |
| One PC | No roster entry is the PC or one of the PC's names. |
| Kinds | Only a character has a Voice Card, a goal or an age. A group is a Faction. |
| Ages | Every character has an age. A Player constraint on ages holds. |
| Completeness | Every NPC has at least one goal and a Voice Card with at least two sample lines. |
| Agency | No event effect and no fact sets a voluntary act, word, feeling or motive of the PC. |
| Settled facts | No generated fact contradicts a settled fact on the same subject and key. |
| World autonomy | At most a set share of NPC goals and secrets centre on the PC: the text names the PC, the goal is against the PC, or the secret is about the PC. The builder proposes the number and tests it on the fixture. |
| Secrecy | The PC never holds a secret or a hidden fact. No command that the UI can call returns one outside the debug view. |

### 6.6 Begin

"Begin" produces the opening Narration from the committed start state. It uses the Narration path that exists. It commits no decision for the PC. Whether it is a Turn of a new kind or a narration-only step is the architect's call.

## 7. UI

A New Campaign screen with one entry, "From a document": paste text or choose a `.md` or `.txt` file. Then the seed review (edit, answer gaps, approve), the build progress, and a "Begin" button. Plain styling.

The old path, two TOML files, stays for tests and evaluations. It leaves the normal UI.

## 8. Acceptance tests

Deterministic, with recorded model responses, must pass:

- each check in 6.5 rejects its trap output and accepts a correct one;
- a failed repair of an optional item drops that item and the build continues;
- a failed repair of an essential item fails the build and leaves no Campaign file;
- a cancelled build leaves no Campaign file;
- after a failed or cancelled build, the approved seed is still in the draft store and a retry needs no new review;
- the World Build adds no character that the approved roster lacks;
- the build cannot start before approval; the opening Narration cannot start before "Begin";
- an open item never appears in a prompt as a thing that happened;
- no UI command returns a GM-only fact, a goal or a scheduled event outside the debug view;
- restart after "Begin": no loss of state.

Evaluation, measured against the live model, on a fixture chronicle that is invented for the repository and holds no private content:

- **roster recall**: named characters found, against a hand-made list;
- **fact recall**: facts found, against a hand-made list;
- **false past**: generated facts that state an open item as done;
- **PC-centred share**: NPC goals and secrets that centre on the PC (named in the text, a goal against the PC, a secret about the PC);
- **voice distance**: the blind voice judge that exists, on the generated Voice Cards;
- **build time**: p50 and max.

The owner tests his private chronicle by hand. It never enters the repository.

## 9. New terms for the glossary

- **Seed Draft**: the Adventure Seed before approval.
- **World Build**: the step that creates the hidden world from an approved Adventure Seed.
- **Source Text**: the document the Player gives to start a Campaign.

The architect and the domain-modeling step may rename them. They land in `GLOSSARY.md` before any code.

## 10. Left to the builder

- Prompt wording and the number of calls per stage.
- The size limit on the Source Text, with a tested value.
- Table layout and migrations for ages, names and the new seed fields.
- The threshold for the World autonomy check.
- The fixture chronicle and its hand-made lists.

## 11. Calls made by Claude Code, open to the owner's veto

- The World Build runs with thinking off, on the same model as play.
- A failed optional item is dropped without telling the Player what it was.
- The roster and PC Knowledge are part of the seed review. The owner stays blind to the hidden world only.
- The draft store is separate from the Campaign file, so a failed build costs no review work.
- Build prompts and outputs go to the debug view behind its toggle, with a spoiler warning.
- Mature content in the Source Text becomes neutral facts and seed themes; nothing is removed or refused.
- The model proposes missing ages, and the Player confirms them at the seed review. The owner said: every generated character needs an explicit age.
- This slice stores who knows that two names are one person, and does not enforce it in play.
- The old two-file path leaves the normal UI.

## 12. Decisions from the owner

- Three ways to start a Campaign; the interview is never forced (2026-11-10).
- The owner's first real test resumes his private chronicle from its stopping point (2026-11-10).
- The owner stays blind to the whole hidden world (2026-11-10).
- Every generated character has an explicit age (2026-11-10).
- Order: this project, then the mature content module, then the interview and "surprise me" (2026-11-10).
- Implementation goes to Opus 5.5 (2026-11-10).
