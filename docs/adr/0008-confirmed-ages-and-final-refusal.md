---
status: accepted
date: 2026-11-11
---

# An age counts when the Player confirmed it, and an adults-only refusal is final

## Context

The adults-only rule of an intimate proposal never ran: `intimate` was optional in the schema, and the model never set it (design 4: 0 of 15). The rule also read the stored age alone. A stored age has three sources: a Seed Draft that the Player confirmed row by row, a fixture file that nobody confirmed, and a new NPC whose age is model output. Only the first one comes from the Player.

A retry made it worse. The retry prompt lists the rejection lines. A model that reads "adults only" answers `intimate: false`, the proposal passes, and the narrator tells the same Intent.

## Decision

- **Age Confirmation.** Each PC and NPC row of the Campaign file has `age_confirmed`. Only the `ConfirmAge` op sets it after creation (and, from Task 2 of the plan, a Seed Draft with no gap at creation). The store refuses `ConfirmAge` on an entity that is not a PC or an NPC, and on a stored age under `domain::ADULT_AGE`. It never changes the age. A ledger line written before the field reads as unconfirmed. A new NPC from a proposal is unconfirmed, always.
- **One rule, one home.** `mature::clearance` is the adults-only rule. A character is cleared when it is a PC or an NPC, its age is confirmed, its stored age is 18 or more, and no PC or NPC that fails one of these shares one of its names, main or other, through `normalize::name`. It covers every character at the PC's place, world truth (hidden ones included), plus the reacting NPCs. `mature` is the only caller of `domain::is_adult`. `min_age` and the Detail Level change nothing in it. The file of the rule names no Content Pack.
- **The Detail Level.** The Adventure Seed has `detail`: `excluded`, `implied` or `explicit`. A seed without it is `excluded`, so the module is off by default. `WorldView::detail` is its projection.
- **The gate before the Commit (G2).** `mature::check` runs first in `checks::validate`, before the references check can skip the other checks. For an intimate proposal: at `excluded`, `DetailExcluded`; a new NPC, or a character that is not cleared, `AdultsOnly`; no reaction, or a reacting NPC below comply, `Unwilling`.
- **A final refusal.** `DetailExcluded` and `AdultsOnly` are final (`CheckKind::is_final`). `turn::plan` stops at once: no retry prompt, no second proposal call, no Commit. The Turn ends with `TurnError::Refused`. `Unwilling` keeps the one normal retry; a retry that fails on it alone is an `Unwilling` Refusal.
- **The tag cannot be dropped.** `intimate` is required in the per-Turn schema at every level. `turn::parse` refuses a raw proposal without the key (`serde(default)` stays for stored proposals). When the first attempt parsed with `intimate: true`, the retry is checked as intimate too.
- **The Player reads one fixed sentence** per kind (`view::refusal_text`): no name, no id, no age, in debug mode too.

Later tasks of the plan add the same check after the World Events of the Turn (G3), before the proposal call of a scene Turn (G1) and before each narration (G4).

## Considered options

- **The model-stated age is enough.** Rejected: no human is in that path. The owner decided that an NPC made in play stays unconfirmed until the Player confirms it (data shape 16, decision 2).
- **A schema that pins `intimate` to false when the place is not cleared.** Rejected: the signal is lost, and the Turn commits as a plain one.
- **"An entity of the Setup Turn is confirmed".** Rejected: it trusts the fixture path, and it gives no way to confirm a character met in play.

## Consequences

- Every Campaign built before Task 2 of the plan has unconfirmed ages, so every intimate Turn is refused. The gate is closed until the Seed Draft path confirms ages.
- The Player confirms a character met in play before a scene with that character (plan Task 8).
- A model that tags a plain Turn by mistake costs the Player one refused Turn and one sentence at `excluded`.
- A wrong tag (`intimate: false` on an intimate Intent) runs no age rule. Code cannot see it; the evaluation measures it (plan Tasks 9 and 10).
