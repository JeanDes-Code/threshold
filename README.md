<img src="assets/icon.svg" width="64" height="64" alt="">

# Threshold

A single-player tabletop RPG that runs on your own machine. A local language model plays the Game Master. The world keeps its own state and rules, and the model does not own them.

Status: first playable slice. It works end to end, and it still has known gaps (see below).

## How a Turn works

1. You type what your character tries to do.
2. The model returns a structured Decision Proposal: the action, whether it needs a Check, the Stakes, how NPCs react, how long it takes.
3. Rust code validates the proposal before anything changes. It rejects references to things that do not exist, decisions taken for your character, and four kinds of Shortcut (a convenient coincidence, an NPC who folds without cause, a fact that bends, a tension that evaporates).
4. The Ruleset rolls a seeded die. The world clock advances and scheduled World Events fire.
5. One SQLite transaction commits the result to the ledger.
6. The model narrates only what was committed and what your character can perceive. The text streams as it is written.

Secrets stay out of the narration: the narrator sees only what your character believes. Each NPC keeps a Voice Card so they do not all sound alike.

## Stack

- Rust core (`crates/threshold-core`): domain types, checks, world clock, ledger store (`rusqlite`).
- `crates/threshold-ollama`: the Ollama adapter. The model gets a JSON Schema built for each Turn.
- `crates/threshold-eval`: an eval harness with scenarios in `evals/scenarios/`.
- Tauri 2 desktop shell (`src-tauri`) and a React + TypeScript UI (`ui`).

Default model: `gemma4:26b-a4b-it-qat` through Ollama. On an RTX 4070 SUPER (12 GB VRAM) a Turn takes 7.7 s at p50 and 12.6 s at p90.

## Run it

You need Rust, Node and a running Ollama with the model pulled.

```sh
ollama pull gemma4:26b-a4b-it-qat
npm --prefix ui install
npm --prefix ui run tauri dev
```

Tests:

```sh
cargo test --workspace
npm --prefix ui test
```

## Known gaps

Measured in `evals/reports/2026-11-baseline.md`:

- Memory: a detail you mention in passing is not stored, so the GM forgets it after six Turns.
- Offscreen consequences: the model rarely schedules a later World Event.
- The narrator sometimes repeats a claim of yours that the ledger rejected.
- About 10% of Turns time out at 60 s. The cause is not known yet.

## Content Packs

Content Packs are removable data folders (prompt fragments, extra Adventure Seed options). They hold no code, and the loader refuses code files and symlinks. No pack can override a Content Boundary. Sexual content always involves adults only.

## Docs

- `GLOSSARY.md`: the domain terms.
- `docs/adr/`: architecture decisions.
- `docs/superpowers/specs/`: the design and the data shape.
