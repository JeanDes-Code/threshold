---
status: accepted
date: 2026-11-10
---

# What the code needs from a Decision Proposal is a required, enum-typed field

## Context

A bug pass (B1 to B5, B12) asked the model, through prompt lines and free `facts`, to write a move, a contest and the closing of an Open Item. The live evaluation `play_basics` on `gemma4:26b-a4b-it-qat` (`evals/reports/2026-11-play-basics.md`, attempts 1 and 2) showed that it does not work:

- Moves that changed the scene place: 1 of 8. On a move, the model sent no `facts`.
- Contests with a Roll: 0 of 4. The stakes wrote PC keys that the agency check refuses (`E1_strength`).
- Pending items closed: 0 of 4. An acceptance came with no outcome fact.
- Turns failed: 10 of 20.
- On the impossible act, the model named the PC's own `location` as a blocker. That opened an obstacle, and the coincidence check then rejected a correct later move.

The World Build had shown the opposite pattern on the same model: when every name field became a schema enum, a failing stage went from 1 of 7 to 3 of 3. This model follows a JSON schema much better than a prompt rule.

## Decision

What the code needs becomes a small field of the Decision Proposal that the per-Turn schema makes required and limits to an enum. The code then writes the facts itself, deterministically.

- **`move_to`**: required, nullable. Its enum is the handles of the places the PC knows (`world::known_places`: every place entity that the Adventure Seed does not hide), plus `null`. When the verdict lets the action happen (allowed, or a Check that succeeds) and `move_to` is a place, the core writes the PC's `location` with the place's main name, witnessed by the PC. A failed Check or an impossible verdict moves nobody. A move with `move_to: null` is not rejected: the PC stays. The model no longer writes the PC's `location`: one in `facts` or in a stake is dropped and logged. The `move` check and its repair note are gone.
- **The narrator is told where the PC is.** The outcome block ends with "After this Turn the PC is at <place>", read from the Commit. A move that the Commit did not make reads "The move does not happen: the PC does not arrive. After this Turn the PC is still at <place>".

## Consequences

- The schema changes every Turn with the known places, as the handle enums already do. The prompt shows them on one `PLACES` line in the scene block, so the prompt prefix stays stable within a Scene.
- A place that is not an entity cannot be a destination. The town fixture's mill, until now only a `location` value, is now a place entity.
- A place stays one spelling: the core writes the entity's main name, and `WorldView::present` reads it.
- The leniency rule of data shape section 11 holds: the core drops model output about the PC and logs it; it never creates a PC act beyond `move_to`, which comes from the Player's Intent.

## What this does not cover

- **A destination the PC does not know.** The proposal has no way to create a place: `new_npcs` creates NPCs only. Such a move commits with `move_to: null`, and the narrator says that the PC does not arrive. A place created in play needs its own decision.
- **Which places the PC knows** is a simple rule: every place the Adventure Seed does not hide. It does not read the PC's Beliefs.
- **A wrong destination.** The schema keeps the value inside the enum; it cannot check that the place is the one the Player named.
