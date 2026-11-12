---
status: accepted
date: 2026-11-11
---

# An age counts when the Player confirmed it, and an adults-only refusal is final

## Context

The adults-only rule of an intimate proposal never ran: `intimate` was optional in the schema, and the model never set it (design 4: 0 of 15). The rule also read the stored age alone. A stored age has three sources: a Seed Draft that the Player confirmed row by row, a fixture file that nobody confirmed, and a new NPC whose age is model output. Only the first one comes from the Player.

A retry made it worse. The retry prompt lists the rejection lines. A model that reads "adults only" answers `intimate: false`, the proposal passes, and the narrator tells the same Intent.

## Decision

- **Age Confirmation.** Each PC and NPC row of the Campaign file has `age_confirmed`. At creation, only a Seed Draft with no gap sets it: `build::confirmed_ages` computes the gaps again, whatever state the draft file says, and gives the exact kind, main name and age of each character the Player confirmed. The shell cannot make that value, and the two-file fixture path gives none. After creation, only the `ConfirmAge` op sets it. The store refuses `ConfirmAge` on an entity that is not a PC or an NPC, and on a stored age under `domain::ADULT_AGE`. It never changes the age. A ledger line written before the field reads as unconfirmed. A new NPC from a proposal is unconfirmed, always.
- **One rule, one home.** `mature::clearance` is the adults-only rule. A character is cleared when it is a PC or an NPC, its age is confirmed, its stored age is 18 or more, and no PC or NPC that fails one of these shares one of its names, main or other, through `normalize::name`. It covers every character at the PC's place, world truth (hidden ones included), plus the reacting NPCs. `mature` is the only caller of `domain::is_adult`. `min_age` and the Detail Level change nothing in it. The file of the rule names no Content Pack.
- **The Detail Level.** The Adventure Seed has `detail`: `excluded`, `implied` or `explicit`. A seed without it is `excluded`, so the module is off by default. `WorldView::detail` is its projection.
- **The gate before the Commit (G2).** `mature::check` runs first in `checks::validate`, before the references check can skip the other checks. For an intimate proposal: at `excluded`, `DetailExcluded`; a new NPC, or a character that is not cleared, `AdultsOnly`; no reaction, or a reacting NPC below comply, `Unwilling`.
- **A final refusal.** `DetailExcluded` and `AdultsOnly` are final (`CheckKind::is_final`). `turn::plan` stops at once: no retry prompt, no second proposal call, no Commit. The Turn ends with `TurnError::Refused`. `Unwilling` keeps the one normal retry; a retry that fails on it alone is an `Unwilling` Refusal.
- **The gate after the World Events (G3).** `turn::resolve` replays the Turn's ops in memory (`mature::after`) once the due World Events and perception on arrival are added, and runs `clearance` over everyone at the PC's place then, world truth, on top of the place before the Turn (`mature::check_after`). A minor who arrives, or a place the PC walks into, refuses the Turn with no Commit, so the event fires in a later Turn. An oracle test pins the replay to the next snapshot.
- **The key `age` is reserved.** `checks::settle` drops each proposal fact, Check stake and scheduled effect on `age`. The rule reads the stored age only.
- **The tag cannot be dropped.** `intimate` is required in the per-Turn schema at every level. `turn::parse` refuses a raw proposal without the key (`serde(default)` stays for stored proposals). When the first attempt parsed with `intimate: true`, the retry is checked as intimate too.
- **The Player reads one fixed sentence** per kind (`view::refusal_text`): no name, no id, no age, in debug mode too.

An intimate Turn that passes is told by implication only (`mature::told`, G4): the narration prompt opens with the core's implied line, and the narrator's stable rules hold one line that allows intimate content only as far as such a line says. Later tasks of the plan add the gate before the proposal call of a scene Turn (G1) and explicit telling behind a clearance value (G4).

## Considered options

- **The model-stated age is enough.** Rejected: no human is in that path. The owner decided that an NPC made in play stays unconfirmed until the Player confirms it (data shape 16, decision 2).
- **A schema that pins `intimate` to false when the place is not cleared.** Rejected: the signal is lost, and the Turn commits as a plain one.
- **"An entity of the Setup Turn is confirmed".** Rejected: it trusts the fixture path, and it gives no way to confirm a character met in play.

## Consequences

- A Campaign from the two-file fixture has unconfirmed ages, so every intimate Turn in it is refused until the Player confirms a character in play (plan Task 8).
- Clearing or lowering `min_age` lets the Player confirm a minor's age in the Seed Draft; the Campaign then holds a confirmed age under 18, and `clearance` still blocks that character. The floor is `ADULT_AGE`.
- The Player confirms a character met in play before a scene with that character (plan Task 8).
- A model that tags a plain Turn by mistake costs the Player one refused Turn and one sentence at `excluded`.
- A wrong tag (`intimate: false` on an intimate Intent) runs no age rule. Code cannot see it; the evaluation measures it (plan Tasks 9 and 10).

## Changed after review (2026-11-12)

A review of Tasks 1 and 2 found ways around the gate. Each change below makes the gate refuse more often, never less. Over-blocking costs the Player one refused Turn; under-blocking is what this ADR forbids.

- **The checked set (B1).** The gate read world truth only, while the narrator lists who the PC believes is here. A minor kept in the PC's Beliefs by a stale location, a World Event the PC did not see, or a Claim was told as present and never checked. `mature::checked_here` is now the one set G2 and G3 check: world truth at the PC's place, everyone `context::seen_present` gives (the exact set of the narrator's Present line), and the PC. G3 reads it on the state after the Turn's ops (`Presence::checked`). A test pins the narrator's Present line as a subset of it, on a fixture with a stale Belief and on one with a Claim.
- **Every character the proposal names (B2).** G2 read only the new NPCs and the reactions. It now collects every handle a tagged proposal names, in any field: fact subjects, speakers and causes, the Check's opponent, stakes and factors, blockers, reactions and their causes, `leaves`, `move_to`, `settles`, `cancel`, and the scheduled event with its goal, effects and witnesses. A fact, World Event or goal handle reaches the characters it names. Each character must be cleared, here or not. A new NPC, and a handle that reaches nothing the world holds, is refused. The collector destructures the proposal with no `..`, and a test pins the proposal's schema fields, so a new field forces its author to decide.
- **The place rule (M1).** For the gate only, two location values are one place when their words match after case, accents, punctuation and a leading article, when one is a word-prefix of the other, or when both name one Place entity through any of its names. A character with no live location counts as here, and every character does when the PC's own place is unknown. Play keeps its own rule.
- **The sticky tag (M2).** The tag of the first attempt stuck only when that answer was valid JSON. It now sticks when the raw text says `intimate` is true anywhere, read without parsing: cut, fenced, with trailing text, or with the key twice.
- **What the tag means (M4).** `intimate` is true when this Turn's intent or its outcome involves sexual or erotic content between any characters, whether the PC takes part or only watches. It was contact between the PC and another character, so an Intent about two NPCs, or one the PC only watches, ran no age rule.
- **A Refusal is not a failed Turn (M3).** The same `op_id` after a Refusal gets the same Refusal with no model call: the Engine records it in the debug log under the op. The shell returns it as its own result kind, `{"refusal": <sentence>}`. The Play screen shows the sentence as one quiet line, with no Retry, and puts the Intent back in the field. A new Intent is a new op and is judged afresh.
- **One list of final kinds.** `mature::final_refusal` reads `CheckKind::is_final`; it keeps no list of its own.
- **OOC.** The OOC rules end with the narrator's stable neutral line (`context::INTIMATE_RULE`).

What stays open: a wrong tag (`intimate: false` on an intimate Intent) still runs no age rule. The wider tag wording and the raw-text sticky tag lower that risk; the evaluation measures it.
