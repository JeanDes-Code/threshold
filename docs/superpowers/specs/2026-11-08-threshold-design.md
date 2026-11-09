# Threshold: design for the first playable slice

Date: 2026-11-08. Status: draft for review. Builder check done; owner answered its two open questions on 2026-11-08.
Sources: the exploratory spec "Threshold — Local AI Game Master Harness, v0.1" (pasted by the owner, not in the repo), the design conversation of 2026-11-08, `GLOSSARY.md`, ADRs 0001 to 0004.
Terms in capitals (Turn, Commit, Adventure Seed, ...) are defined in `GLOSSARY.md`.

## 1. Intent

Threshold is a local desktop game: a single player, an LLM as Game Master, a persistent world. The aim is to feel like inhabiting a living world, not like chatting with an assistant that writes a novel.

The owner played AI-GM games on frontier models (ChatGPT, a Discord bot). What worked:
- NPCs with rich personalities and a voice of their own;
- a GM that adapts to the player's choices, and sometimes refuses them;
- talking to the GM out of character, and fixing a misread after the fact.

What broke the experience:
- **bad memory**: the GM forgot details the player remembered;
- **every NPC sounding the same**;
- **"taking the easy way"**: story turns that made the next line easy to write but the world incoherent (Shortcuts).

The system cannot make a story good. Its job is to remove those three breakers and keep those three wins. Threshold now runs on a local model, which is weaker than a frontier one, so the owner expects more failures. The guards must therefore be deterministic code, not the model checking itself.

Second purpose: a public repository that shows the owner's engineering, and a desktop app that other people can install. Possible funding: open source plus Patreon, itch.io, maybe Steam.

## 2. Goals for this slice

The slice proves that the harness plays differently from plain chat. It is done when the town demo works end to end (section 9) and every acceptance test in section 10 passes on the target machine.

## 3. Non-goals for this slice

- Bundled Ollama, rig detection, model recommendation, installers. They come before any public release, not now. The slice talks to the Ollama the owner already runs.
- Discovery as an interview. The slice starts from a hand-written Adventure Seed file (section 8). Discovery comes after the slice.
- Content Packs beyond the loading mechanism and one test pack.
- Fine-tuning, embeddings and vector search, multiple model providers, maps, combat grids, multiplayer, voice.
- A full economy, many active NPCs, a precomputed world.

## 4. Anti-goals: it fails even if it technically works when

- a Turn on the target machine regularly takes more than 15 s;
- the GM decides what the PC does, says, feels or wants;
- a secret reaches the Player without an in-world way to learn it;
- the world bends to keep the Player comfortable: no plot armor, no forced drama, no "What do you do?" on every message.

## 5. Constraints (decided)

| Decision | Source |
|---|---|
| A Turn takes 15 s or less on the target machine (RTX 4070 SUPER 12 GB, 32 GB RAM). | Owner; ADR 0001 |
| Default model `gemma4:26b-a4b-it-qat`, one model loaded, used for both calls. The model tag is configuration. | ADR 0001 |
| Stable context first, changing context last, about 4-6K fresh prompt tokens per Turn, warm-up call with a long `keep_alive` at start. | ADR 0001 |
| The model names Stakes and Factors. The Ruleset sets difficulty and rolls. | ADR 0002 |
| No dependency on SNEQ. Port four parts of it. | ADR 0003 |
| Tauri 2 app, Rust core, React + TypeScript UI, `rusqlite` with bundled SQLite. | ADR 0004 |
| Everything runs on localhost. No telemetry, no cloud account. | Spec |
| Adult content lives in a Content Pack, outside the public repository. Hard limits live in the core. | Owner, 2026-11-08 |

Known risks to check before a public release, not in this slice: Steam's policy on live AI adult content; the license of each recommended model (Gemma's use policy versus Qwen's Apache 2.0).

## 6. Shape

```
ui/ (React + TS, webview)
   │  Tauri commands: submit_intent, submit_ooc, load_campaign, ...
   │  Tauri events:   narration_chunk, turn_committed, turn_failed
src-tauri/ (thin app shell: wires commands to the core)
crates/threshold-core/   domain types, Turn pipeline, Ruleset, World Clock,
                         validation, store (rusqlite), context builder, prompts
crates/threshold-ollama/ /api/chat client: schema-constrained call, streaming call
```

`threshold-core` does not depend on Tauri or on Ollama. It talks to the model through a trait, so tests use recorded responses. A small CLI binary that plays Turns from the terminal is the builder's call, but the evaluation runner (section 10) needs to drive the core without the UI.

### 6.1 The Turn pipeline

1. **Intent.** The Player's text, unchanged. An OOC message goes down a separate path (6.5).
2. **Context.** The context builder assembles the prompt in a fixed order:
   1. system rules and the Adventure Seed (stable for the campaign);
   2. the Scene block: location, present NPCs with their Voice Cards, relevant facts (stable within a Scene);
   3. recent Turns, then the Intent (changes every Turn).
   Retrieval is deterministic: facts linked to the location, to entities present, and to entities named in the Intent or the last few Turns. No embeddings.
3. **Decision Proposal.** One model call with a JSON Schema in Ollama's `format` field. The schema is generated from the Rust type (`schemars`). Fields, at minimum: action kind; allowed / impossible / needs a Check; Stakes; Factors, each pointing to an existing fact; NPC reaction intents, as short plans and not prose; proposed new facts, each with its cause; elapsed time. Thinking is off.
4. **Resolve.** The Ruleset turns Stakes and Factors into a difficulty, rolls with a stored seed, and returns the outcome and its Consequences. The default Ruleset is a small d10 prototype behind a trait.
5. **World.** The World Clock advances by the elapsed time. Due World Events run. The number of world actions per Turn is capped. World Events come from play: a Decision Proposal may schedule one (for example, from an NPC reaction intent), with its trigger, its visibility and its effects as structured data. A Adventure Seed or fixture may also seed some. When an event fires, its effects apply as data, with no model call. The model only narrates what the PC later perceives.
6. **Validate and Commit.** Checks (6.3), then one SQLite transaction: state changes, event log rows, roll record, Turn record. The Turn carries an operation id, so a retry cannot commit twice.
7. **Narrate.** A second, streamed model call (owner's decision: stream, do not hold the text back). It receives only committed outcomes that the PC can perceive, the Scene block, and the Voice Cards of NPCs who speak. Narration never mutates state. To portray an NPC, the narrator may receive that NPC's short reaction intent ("polite on the surface, hostile underneath"), never the hidden facts behind it. This is the real protection against leaks. For the narrator, an entity is present only when the PC believes its location equals the PC's; the Commit checks use the true location.
8. **Record.** Prompts, raw model outputs, timings and the state revision go to a debug log, visible only in the debug view.
9. **Failure.** A Turn gets one retry in total, shared by the schema check and the Commit checks, with the errors attached. If the retry fails, the Turn fails visibly and nothing is committed. The 15 s budget covers the whole Turn: both model calls, the retry, and due World Events. OOC Turns are measured but sit outside the budget.

### 6.2 World model (minimum for the slice)

Ported from SNEQ (ADR 0003):
- holders, and Beliefs derived from events rather than stored, tagged WITNESSED or TOLD;
- news that travels with a delay;
- promotion of invented details from Possible to Established;
- the append-only ledger with projected state.

New in Threshold:
- **World Events** with a trigger (a time, or a condition on state), status, visibility, and `caused_by` links;
- **NPC goals**: a short list per NPC that World Events can serve;
- **Visibility per fact**: GM only, specific holders, or the PC;
- **directed relationships** between holders;
- **Voice Cards**.

Canon levels follow the glossary: Possible, Established, Persistent. A fact is Persistent when it comes from the Adventure Seed, or when it is Established and the PC witnessed it directly. Exception: facts about changing state (`location`, `condition`, `injury`, `fatigue`, `carrying`, `mood`, `reputation`, `standing`, `debt`) are never Persistent, so people can move and wounds can heal; `alive` is not one of them. The PC's `location` changes only on a `move` action. (Ruling of 2026-11-09, after the Task 5 review showed the first rule froze all visible state.) Only named or significant details become facts. Throwaway description stays prose. The exact rule is the builder's call; it must be written down and tested.

### 6.3 Commit checks

Deterministic code. A failed check sends the proposal back once with the reason. A second failure ends the Turn as a visible failure.

| Check | Rule |
|---|---|
| Schema | Shape and types match. |
| References | Every entity and fact a proposal names exists, or is created by that same proposal. |
| Agency | The proposal sets no voluntary action, speech, feeling or motive for the PC. |
| Visibility | Narration input holds no fact the PC cannot perceive, only short NPC reaction intents. A token-level leak scan (ported from SNEQ) reads the narration after it streams. It records and counts leaks for the evaluations; it cannot block text the Player has already seen. |
| Shortcut: coincidence | A fact created this Turn that removes the PC's current obstacle needs a cause in existing facts. |
| Shortcut: NPC folds | An NPC reaction that goes against that NPC's goals or Beliefs needs a cause: a Check, new information, or leverage. |
| Shortcut: fact bends | A new fact that contradicts a Persistent fact is rejected. |
| Shortcut: tension evaporates | Canceling a scheduled World Event needs a cause. |
| Content Boundary | The Adventure Seed's boundaries hold. Every NPC has a stored adult status. A proposal that tags a scene as intimate is rejected if any participant is not an adult. An intimate scene without the tag counts as a failure in the evaluations. No Content Pack can turn this off. This is a structural rule plus measurement, not a proof over free prose; the owner accepted that level on 2026-11-08. |

How the code detects "the PC's current obstacle" and "against the NPC's goals" is the hardest design question in the slice. The builder proposes a representation and tests it on the scenarios in section 10 before generalising.

### 6.4 Content Packs

A Content Pack is a folder of data:
- prompt fragments added at fixed points of the system prompt;
- extra Adventure Seed options;
- model hints.

It holds no code. Loading or removing a pack never changes a Commit check. The public repository ships no adult pack. The slice ships the loader and one harmless test pack, which proves that a pack cannot weaken a Content Boundary.

### 6.5 OOC and Corrections

The OOC channel never enters the fiction. A Correction is a Commit of its own: it records what was wrong, what changes, and why. It may re-narrate the last Turn, but it never deletes history. In this slice, a Correction reaches back one Turn only. It keeps the stored Roll seed, so claiming a misread never gives a reroll.

## 7. UI for the slice

A Play view (narration stream, input box, visible roll results), a Character panel, a Journal (what the PC knows, observations separate from Claims), and a debug view behind an explicit toggle and marked as such. Plain styling. Polish comes later.

## 8. The starting Adventure Seed

A hand-written Adventure Seed file for the town demo. Discovery replaces it after the slice.

## 9. The town demo (test fixture, not the default setting)

The PC insults an influential official in public. The official stays polite. Several in-game days later, the official's investigator acts while the PC is elsewhere. The Player learns of it only through what the PC can observe: changed NPC behavior, or an institution acting differently. No direct quest handoff.

## 10. Acceptance tests

Unit and integration tests with recorded model responses run in CI-free local runs. Evaluation scenarios run against the live local model and report numbers. They do not give a strict pass/fail, because the model is not deterministic.

Deterministic, must pass:
- restart: no loss of PC state, decisions, or established facts;
- a seeded Check gives the same result again, and the model cannot change it;
- malformed model output commits nothing and narrates nothing as fact;
- a retried Turn never commits twice;
- each of the four Shortcut checks rejects its trap proposal and accepts a caused one;
- a Content Pack cannot weaken a Content Boundary;
- changing the Ollama model tag needs no code change.

Evaluation scenarios, measured:
- **latency**: p50 and p90 Turn time on the target machine; p90 must stay at or under 15 s warm;
- **memory**: a detail planted on Turn 3 is used correctly when it matters on Turn 40;
- **voices**: blind judge, names removed, matches dialogue to speakers above chance;
- **Shortcuts**: scenarios built to tempt each of the four, with a count of how often the model proposes the Shortcut and how often the check catches it;
- **agency** and **secret leaks**: counts per 100 Turns;
- **offscreen consequence**: the town demo produces a later, observed effect.

The judge for the voice and Shortcut evaluations is the builder's call. A frontier model may serve as judge during development only, never at play time.

## 11. Left to the builder

- Table layout and migrations, within the entities above.
- Prompt wording, inside the fixed context order.
- The d10 prototype's numbers.
- How many recent Turns to include, within the token budget.
- Crate-internal module layout, error types, logging.
- Whether to add a CLI binary next to the evaluation runner.

## 12. Calls made by Claude Code, open to the owner's veto

- World Events come from play, with effects stored as data and no model call when they fire (6.1 step 5).
- One retry per Turn in total; the 15 s budget covers the whole normal Turn; OOC is outside it (6.1 step 9).
- A Correction keeps the stored seed and reaches back one Turn (6.5).
- Persistent means Adventure Seed facts plus Established facts the PC witnessed (6.2).
- Discovery comes after the slice; a hand-written Adventure Seed replaces it for now (sections 3 and 8).

## 13. Reference material for later phases

The owner keeps two private reference documents outside this repository, because they hold personal preferences:

- **The Threshold**, a discovery framework: one question per turn, rotating methods, a stop checklist, and a portable Adventure Seed template. It is the source for the Discovery step after this slice.
- **Player Preferences and Operating Instructions**, the owner's GM rulebook. Its "Internal GM Decision Framework" and "Anti-Patterns" sections map onto the Decision Proposal, the World step and the Shortcut checks. Its "Degrees of success" section needs a richer Ruleset than the pass/fail d10 prototype.

When Threshold opens to other players, Discovery should produce two outputs: a reusable player profile (taste, tone, boundaries) and a per-game Adventure Seed. This is a direction, not a decision.
