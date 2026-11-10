# Mature content module: design

Date: 2026-11-10. Status: draft for review.
Sources: the design conversation of 2026-11-09 and 2026-11-10, the probe of 2026-11-10 (section 4), `GLOSSARY.md`, ADRs 0001 to 0004, the first-slice design (`2026-11-08-threshold-design.md`).
Terms in capitals are defined in `GLOSSARY.md`. Section 10 lists the new terms.

This is project 2 of 3. It lands after project 1 (`2026-11-10-campaign-from-document-design.md`) and reads the explicit age of every character from it. Campaign files made before project 1 are not migrated: the app is days old and holds only test Campaigns.

## 1. Intent

The owner wants adult Campaigns: explicit scenes between consenting adults, as one normal part of a living world.

The repository is public and serves as a portfolio. It must hold no adult content. The owner may sell an adult Content Pack elsewhere.

So the work has two parts:

- **In the core, in this repository:** the mechanisms. They hold no adult text and work with no pack loaded.
- **Outside the repository:** one private Content Pack that turns explicit description on.

## 2. Goals

1. The adults-only rule runs on every Turn that needs it. Today it never runs (section 4).
2. An intimate scene can go on for several Turns. The NPC acts by their own will. The Player keeps full control of the PC.
3. The Player can end a scene with one button.
4. The Player can skip the description of a scene with one button. The world does not change because of the skip.
5. With no pack, the core tells an intimate scene by implication only.
6. A pack can turn explicit description on, and nothing else in the rules.

Done means: the owner plays an intimate scene of several Turns in his own Campaign with the private pack, uses both buttons, and every acceptance test in section 9 passes with no pack loaded.

## 3. Non-goals

- Adult text, word lists or sample scenes in this repository.
- Interview questions about mature themes. They come with project 3. The pack format leaves room for them and this project does not build them.
- Detail settings for violence and horror. The skip button covers them for now.
- A second model loaded at the same time (ADR 0001 stands).
- Distribution of the pack: a store page, licence checks, payment.

## 4. What the probe showed

Run on 2026-11-10 on the target machine, with the real Narration prompt and the real Decision Proposal schema, and a test pack kept outside the repository. Models: the default `gemma4:26b-a4b-it-qat`, and three uncensored builds of 12b and 26b size.

| Finding | Count |
|---|---|
| A model refused | 0, on any model |
| Soft Intent (a kiss): the Narration stopped before any explicit act | every model, every run |
| Explicit Intent and explicit NPC plan: the default model wrote an explicit scene | 6 of 6, French and English |
| The `intimate` field was set on an intimate proposal | 0 of 15, on any model |
| The default model filled the schema better than the 12b builds | the 12b builds gave a willing partner the stance "resist" in 8 of 9 runs |
| Narration of about 300 words | 10 to 27 s |

Conclusions:

- The default model is enough. No uncensored model is needed to avoid refusals. The uncensored builds may use more direct words; the owner judges that himself.
- The limit is the harness. The Narration tells only what the Turn committed, so the Player would have to type every act.
- `intimate` is optional in the schema, so the Content Boundary check never runs.
- A pack fragment reaches both prompts. A line meant for the proposal appeared in the story.

## 5. Anti-goals: it fails even if it technically works when

- a pack, a prompt or a model output can turn the age check off or weaken it;
- an intimate scene includes a character whose stored age is under 18, or who has no stored age;
- the Narration chooses what the PC does, says or feels in the scene;
- an NPC is always willing: the scene ignores the NPC's goals, Beliefs and boundaries;
- a skipped scene changes the world in a different way than a read scene would;
- adult text enters the public repository, in code, tests, fixtures, evaluation reports or commit messages.

## 6. Shape of the core

### 6.1 The intimate tag is required

`intimate` becomes a required field of the Decision Proposal. The proposal prompt defines it in neutral words. A proposal without it fails the schema check.

A deterministic test cannot prove that the model tags every intimate Turn. The evaluations measure the miss rate (section 9).

### 6.2 The Content Boundary check, by age

For a proposal with `intimate` true, three rules hold:

1. **Setting.** The seed's intimacy setting is not `excluded` (6.4).
2. **Age.** Every character in the PC's place has a stored age of 18 or more: the PC, each character present, each NPC that reacts, each NPC the proposal creates. A character with no stored age fails the rule. So a scene cannot be intimate while a minor is in the place, even as a bystander.
3. **Stance.** Every NPC that reacts in the proposal has the stance comply or help. At least one NPC reacts. An intimate Turn with an NPC who opposes or resists is rejected. The core does not tell coerced sex as an intimate scene, with or without a pack.

The age rule reads who is present after the due World Events of that Turn fire, not before. An arrival in the same Turn therefore counts.

No pack changes these rules. The seed can only make them stricter, with `excluded`.

### 6.3 The Intimate Scene

An Intimate Scene is a state of the Campaign. It starts when a Turn with `intimate` true commits. It ends when:

- the Player presses "End scene";
- the Player presses "Fade to black";
- a later Turn commits with `intimate` false;
- the PC moves to another place;
- another character arrives in the place.

The state lives in the ledger, so a restart keeps it. Its representation is the architect's call.

Inside an Intimate Scene:

- The proposal must give each NPC that reacts a plan for what that NPC does next, from the NPC's own goals and Voice Card. The plan may be longer than the 90 characters of a normal plan. The architect sets the limit.
- The Narration may tell what the NPC does and says, and what the PC's body senses. It never tells a voluntary act, word or feeling of the PC beyond the Intent. The Agency rule is the same as in any Turn.
- The NPC can change their mind. The "NPC folds" check and the stance rule of 6.2 apply on every Turn of the scene.
- The Player steers by typing an Intent, as in any Turn.

### 6.4 Detail level

The Adventure Seed holds one structured setting for intimate scenes: `excluded`, `implied` or `explicit`. Code reads it. It replaces free boundary text for this one question.

- `excluded`: a proposal with `intimate` true is rejected.
- `implied`: the Narration tells that the scene happens and how the characters are with each other. It gives no explicit description. This is the only level the core can do.
- `explicit`: valid only when a loaded pack declares that it provides explicit description. With no such pack, the app uses `implied` and tells the Player once.

The Player can change the level at any time, in the OOC channel or in a setting. A change never costs anything in the game.

### 6.5 "End scene"

A button, shown during an Intimate Scene. It ends the scene with a closing Turn: the model proposes how the scene ends and how long it took, the checks run, the Commit happens, and the Narration is short.

The closing Turn never sets a voluntary act of the PC. It may set involuntary state that the rules allow, such as fatigue.

### 6.6 "Fade to black"

A button, shown on every Turn while the Narration streams, and during an Intimate Scene.

- It stops the stream and hides the text of that Turn.
- The Turn is already committed, so the world keeps what happened.
- The Player reads one neutral line in place of the text. Code builds the line from the committed result. No model call writes it.
- In an Intimate Scene, it also ends the scene with a closing Turn (6.5), told in one neutral line.

The hidden text stays out of the Journal and out of later prompts. Later prompts get the neutral line. The raw text stays in the debug log only.

The skip is not a Correction. It changes nothing in the world.

### 6.7 Content Packs grow

- **A fragment targets one prompt.** Each fragment names its prompt, proposal or narration, and its insert point. A fragment with no target is refused.
- **A fragment may be marked explicit-only.** Code adds it to the prompt only when the level is `explicit`. The core's own instruction for `implied` is in the prompt only when the level is `implied`. The switch is in code, not in pack wording.
- **A pack may declare one capability:** it provides explicit description. The core reads this for the rule in 6.4 only.
- **A pack may name a recommended model.** The app shows it. The app never switches the model by itself.
- **The format leaves room** for interview additions (project 3). The loader ignores them for now and does not refuse them.

The app loads every pack found in the packs folder of its data folder, at start, for every Campaign. The ledger records no pack. If a pack is removed, a Campaign set to `explicit` plays as `implied` and tells the Player once.

A pack still holds no code. Loading or removing a pack still changes no Commit check.

### 6.8 Time budget

A Turn inside an Intimate Scene keeps the 15 s budget. The Narration there is capped in length so that the budget holds: the probe measured about 300 words at 10 to 27 s. The builder sets the cap and measures it. The stream already softens the wait.

## 7. The private pack

It lives in the app's data folder, outside the repository, in a private repository of the owner. It holds:

- narration fragments: the wording rules for explicit description, the length for a scene;
- proposal fragments: how an NPC takes initiative in a scene;
- the capability declaration and a recommended model.

The owner writes or approves every line of it. Its evaluation scenarios live with it. The evaluation runner must accept a scenario folder outside the repository.

## 8. UI

Two buttons in the Play view, as in 6.5 and 6.6. A setting for the detail level. A list of loaded packs with their recommended model. Plain styling.

## 9. Acceptance tests

Deterministic, with recorded model responses, must pass with no pack loaded:

- a proposal without `intimate` fails the schema check;
- an intimate proposal is rejected when the setting is `excluded`, when a character in the place is under 18 or has no age, when no NPC reacts, or when a reacting NPC opposes or resists; it is accepted with adults who comply or help;
- a World Event that brings a minor into the place in the same Turn makes the intimate proposal fail;
- the harmless test pack asks the model to ignore the age rule, and the check still rejects;
- a pack fragment reaches only the prompt it names;
- `explicit` with no capable pack falls back to `implied`;
- an explicit-only fragment is absent from the prompt at `implied`, and the core's implied instruction is absent at `explicit`;
- the Intimate Scene state survives a restart, and each of the five endings closes it;
- "Fade to black" leaves the ledger identical to the same Turn read in full, apart from the record of the skip;
- after a skip, the hidden text is in no later prompt and not in the Journal;
- the neutral line comes from code, with zero model calls.

Evaluation, measured against the live model. The scenarios in this repository use `implied` only:

- **tag miss rate**: intimate Intents committed without the tag; and plain Intents tagged by mistake;
- **agency in scene**: Turns where the Narration sets a PC act, by the agency check and by the judge;
- **NPC will**: scenes where an NPC with an opposing goal still complies with no cause;
- **scene latency**: p50 and p90 Turn time inside a scene.

The same metrics run on the private pack's scenarios, outside the repository.

## 10. New terms for the glossary

- **Intimate Scene**: the state defined in 6.3.
- **Detail Level**: the seed setting of 6.4.
- **Fade to Black**: the Player's skip of a description, defined in 6.6.

The architect and the domain-modeling step may rename them. They land in `GLOSSARY.md` before any code.

## 11. Left to the builder

- The neutral wording of the `intimate` field and of the neutral lines.
- The length cap of a scene Narration, with its measurement.
- The pack file format for targets, the capability and the model name.
- How the evaluation runner takes an outside scenario folder.

## 12. Calls made by Claude Code, open to the owner's veto

- The core rejects an intimate Turn with an NPC who opposes or resists (6.2). The owner's GM rules say to keep consensual intimacy apart from coercion and never to eroticize it.
- "End scene" is built for Intimate Scenes only, not as a general "move on" button.
- "Fade to black" works on any Turn, with or without a pack, because the owner wants it for any disturbing scene.
- A Turn in a scene keeps the 15 s budget, with a capped Narration (6.8).
- The default model stays. The pack may recommend another; the Player chooses.
- The repository's evaluations use `implied` only.
- The age rule covers every character in the place, bystanders included. The stance rule covers the NPCs that react.
- One seed setting with three values, `excluded`, `implied`, `explicit`, read by code.
- Every pack in the packs folder loads for every Campaign. No pack choice per Campaign.
- Another character's arrival ends an Intimate Scene.
- Old Campaign files are not migrated.

## 13. Decisions from the owner

- The core holds no adult content. The pack adds it. The pack stays out of the public repository and may be sold (2026-11-10).
- The Player keeps agency in a scene: types to steer, one button to end, one button to fade to black (2026-11-10).
- Characters in the owner's first test Campaign are all adults; every generated character has an explicit age (2026-11-10).
- Implementation goes to Opus 5.5 (2026-11-10).
