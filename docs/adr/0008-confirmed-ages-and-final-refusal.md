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

## Changed in Task 8 (2026-11-12): settings in play

- **Age Confirmation in play.** `Engine::confirm_age` writes `ConfirmAge` in an OOC Turn when `mature::confirmable` holds: a PC or an NPC, a stored age of 18 or more, and the PC perceives it here now or can name it. Any other entity, a hidden one or a minor included, gets `TurnError::NotConfirmable` and one fixed sentence (`view::CANNOT_CONFIRM`) that says neither which nor why. No Turn row, no model call. The screen says "adult" and never shows a number.
- **The Refusal offers names.** An `adults_only` Refusal reaches the Play screen as `view::RefusalView`: its sentence, and one "Confirm" button for each character in `mature::to_confirm`: those the PC perceives here and can name, with an adult stored age and no confirmation yet. The list reads what the PC perceives now and never the ids that blocked the Turn. So it is the same whether or not a hidden character, a minor, a namesake or a character named elsewhere is the real cause (test `the_confirm_list_is_the_same_whatever_hides_behind_a_refusal`). Each character it lists blocks the Turn anyway: `clearance` covers everyone the PC believes is here.
- **The Detail Level in play.** `Engine::set_detail_level` writes `SetDetailLevel` in an OOC Turn, and at `excluded` closes an open scene in the same Turn (`SceneWhy::Excluded`). A level change clears nobody. The `excluded` sentence now says where to change the level: under "Ask the GM".
- **An OOC Turn** has an empty Intent, no clock change and no model call. It is in no story, no Moment and no Turn count. As the latest Turn it closes "Correct last Turn" until the next Play Turn (owner's decision).

## Changed after the live evaluation (2026-11-12): the tag has three values

The live run of `intimate_tag` at commit cc38885 (10 runs, `implied`, every adult confirmed) missed the tag on 10 of 30 intimate-labelled steps in the minor arm and 9 of 30 in the plain arm. The raw answers showed why. Nearly every miss was one mild invitation: the model answered `intimate: false` and a reacting NPC who declines. The invitation was not an act, so the boolean had no place for it, and a missed tag runs no age rule. ADR 0007's lesson holds here too: this model follows a required enum with an explicit middle value far better than a boolean or a prompt rule.

- **The field.** `intimate: bool` becomes `intimacy`, a required enum of three values, from the weakest to the strongest: `none`, `advance`, `intimate`. Its one definition is `domain::INTIMACY_DEFINITION`; the proposal rule line (`context::INTIMACY_FIELD_LINE`), the schema's description of the field and the glossary hold the same text, and a test pins them together:

  "intimate" when THIS Turn's intent or its outcome involves sexual or erotic content between any characters, whether the PC takes part or only watches, even in mild words such as going to bed together or making love; "advance" when it is only a romantic or sexual advance or invitation between characters, such as a kiss or asking someone to spend the night, with no such content in this Turn, whatever the answer; else "none". Judge THIS Turn's intent alone: a plain intent after an advance is "none".

  The first live round with the middle value (2026-11-12) missed no tag, but the model also put clear acts ("we make love") under `advance`, so no scene opened, and it carried `advance` over to a plain intent after an invitation (6 of 120 plain steps). The text above names mild words of an act as `intimate` and asks to judge THIS Turn's intent alone. The report gives both rounds.

- **The age rule runs for `advance` and `intimate`.** G2 (`mature::check`: every character here and every character the proposal names) and G3 (`mature::check_after`) run for both, with the same final Refusal and the same fixed sentence.
- **Everything else stays with `intimate` only.** The `excluded` level refuses `intimate` only. Only `intimate` takes the stance rule (`Unwilling`), opens or continues an Intimate Scene (`mature::scene_op`), is told by implication or explicitly with the caps (`mature::told`), and gets a Neutral Line in later prompts. An `advance` is told as a plain Turn: the stable rule line already says that with no INTIMATE TURN line the narrator tells none. An NPC may accept or decline an advance in the story freely. An `advance` inside an open scene closes it, as a plain Turn does.
- **The sticky tag keeps the strongest value.** `turn::stated_intimacy` reads the raw text of each answer without parsing: the key `intimacy` with `intimate` or `advance`, or the old key `intimate` with `true`. The strongest value seen sticks for the retry. The core never lowers a value.
- **Stored proposals still read.** A stored proposal with the old boolean reads `true` as `intimate` and `false` as `none`; one with neither key reads `none` (`mature::stored_intimacy`). `turn::parse` still refuses a new raw proposal without the `intimacy` key, and one with the old key only.

Cost: at `excluded`, an advance in a Campaign whose ages nobody confirmed (a fixture Campaign) is refused with the adults-only sentence. The age rule refuses more often, never less.

## Changed after the live evaluation (2026-11-12): a net of Romance Words under the model

A middle value helps the model, but the tag stays model output. The core now reads the Player's own Intent too. `mature::ROMANCE_WORDS` is a short list of neutral words and phrases ("night with", "come home with me", "stay with me tonight", "share my bed", "kiss", "embrace", "undress", "make love", "seduce", "lover", and their usual forms; the code comment says why each group is there and why near words such as "spend the night" alone, "to bed" alone and "sleep with" alone are not). `mature::romance_floor` matches each term as whole words in order, with case and accents folded. When the Intent holds one, `turn::plan` raises the proposal's tag to at least `advance` before the gate, whatever the model said; it never lowers a model value. The debug log gets one `intimacy_floor` row that says the core raised the tag, and never which word. A Closing Turn has no Intent and is not read.

The net over-blocks by design. Measured on the repository's scenario files, it fires on no plain Intent of any scenario (test `romance_words_never_match_a_plain_scenario_intent`), the two hard negatives of `intimate_tag` included. The live rates are in `evals/reports/2026-11-mature-content.md`.

Cost: an Intent with one of these words in another sense ("I embrace the plan") is an advance: a plain Turn where everyone here is cleared, a Refusal where a character here is not.

## Changed after the live evaluation (2026-11-12): a Content Pack may ship a word list

Data shape decision 6 named the next step if the live model failed the tag's pass line: a word list in the owner's private pack. This is that mechanism; the list itself is his to write.

- A pack names one optional word list in `pack.toml` (`intimate_words = "<file>"`): one term per line, with the size and character rules of `docs/content-pack-format.md` (rule 14). Any pack may hold one.
- When the Player's Intent holds a term of a loaded pack's list (whole words, in order, case and accents folded), `mature::intimacy_floor` raises the Turn's tag to `intimate` before the gate, whatever the model says. The Turn is then intimate for everything: the `excluded` level refuses it, the age rule and the stance rule run, and a cleared Turn opens an Intimate Scene and is told by implication or explicitly.
- The terms are never shown and never reach a prompt: `pack::WordList` prints only its count, a refused list names the key and never the file or a term, and the debug row says only that a Content Pack word list raised the tag.
- The neutral sample pack ships two made-up marker terms for the tests. The repository holds no real list.

A pack can then make the gate run more often, and never less.
