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

## Changed after QA run 3 (2026-11-15): one room, and doubt is not cleared

QA run 3 (B1) played a Campaign where an 11-year-old's live `location` was "ostley crossing", the village, while the PC stood in "the ferry house". One live fact says "The ferry house is in Ostley Crossing.". The place rule saw two places, so the gate checked nobody but the adult NPC, and an Intimate Scene opened and went on. The story kept the child asleep by the hearth of that house.

- **One home of presence.** `world::presence` holds who is at the PC's place. `room(view)` gives three sets: `truth` (world truth, hidden characters included; `WorldView::present`), `seen` (what the PC knows is here; `context::seen_present`, which HERE, Ask the GM, the character card and the narration context read) and `unsure`. `mature::checked_here` is `Room::checked`: the union of the three, and the PC.
- **Doubt is not cleared.** `unsure` holds each character the world state does not place away from the PC's place: no live location; the PC's place unknown; a location that names the PC's place another way (the M1 rule); or a location that names a place which holds the PC's place, or which the PC's place holds. A place holds another when a live `location` fact has the outer place as its value, and its subject is the inner place or its text names the inner place as whole words. The rule reads the world state only: a Narration or an NPC line is not needed and is not read.
- **The Player never reads `unsure`.** It is world truth. HERE and the screens keep the PC's sight, so a Refusal can come from a child the PC does not know is near. The Refusal names nobody, as before.
- **The rule reads free text** to find a place inside another, so it can see one where there is none. That only makes the gate check one more character. (Superseded the same day: stored Inside Links replace this reading, see the next section.)

## Changed after the review of QA run 3 (2026-11-15): stored Inside Links replace free text

The rule above found "a place that holds the PC's place" in free text: a live `location` fact whose value is the outer place, and whose subject or text names the inner place. That failed open. A minor whose location was the village blocked only when some fact happened to say the PC's house is in the village; with no such sentence the two were "two places", and the gate cleared the minor. The world also had no way to say that two houses of one village are apart.

- **One stored link per Place.** Each Place holds one Inside Link (glossary): `Inside::Place(parent)`, `Inside::Top` or `Inside::Unknown`. The ledger op is `SetInside { place, inside }`; the projection is `entity.inside_known` and `entity.inside_id` (schema 5). The store refuses a link from or to a non-Place and a link that closes a cycle.
- **Who sets it.** The World Build has a last stage, `place_tree`: one essential call that answers, for each Place of the roster, the Place that holds it or "none". The schema lists every Place as `place` and every Place plus "none" as `inside`. The checks refuse a name that is no Place, a Place listed twice, a Place inside itself and a loop; a link that stays bad after the repair is dropped, and that Place keeps an unknown link. The links go to `WorldSetup.places` and then to `SetInside` ops in the Setup Turn. A Place created in play gets no link: it is unknown until something sets it. A version 4 Campaign gets links once, when it opens (ADR 0012, changed): `world::presence::inferred_links` reads the old rule's facts and the store writes one `SetInside` per link it finds. It never writes `Top`. (Superseded 2026-11-16: the stage moved to the draft, and only the links the Player confirms reach the file; see the last section.)
- **The gate rule (`PlaceTree::away`).** A character is away from the PC only when both location values name one known Place each, both chains of links are known up to a top-level Place, and neither Place is on the other's chain. So a character counts as maybe here when it is at the PC's place, at a Place around it (a minor "in the village" blocks every Place inside the village), at a Place inside it, at a value that names no Place or several, with no location, or when either chain has an unknown link. Two Places that share only an ancestor are apart: two houses of one village, or two villages under one top Place. `holds()` and its free-text reading are gone from the gate; `room()` stays the one home of presence and builds the tree once.
- **What the Player reads does not change.** HERE is still the PC's perception. A Refusal names nobody; when it offers nobody to confirm, it adds one fixed sentence, "Someone may not be a confirmed adult." (`view::REFUSAL_NOBODY_TO_CONFIRM`; it said "Someone nearby" until 2026-11-16, see the last section). The list is empty for a hidden cause, a known minor and a character the PC cannot name alike, so the sentence tells nothing the list does not. The trade-off of the section above stands: a Refusal can come from a character the PC does not know is near, so a Player who tries an Intimate Scene in each place can still learn that someone may be there.
- **Limits.** An old Campaign keeps unknown links except the ones its facts give, so a minor with any location blocks almost everywhere in it: the gate fails closed. The World Build prompt offers "none" as the answer for a Place inside no other; a model that answers "none" for a Place that is in fact inside another makes the two apart, and the gate fails open for that pair. No eval has measured the `place_tree` stage on the live model yet. (The fail-open case is closed 2026-11-16 by the fact check and the Player's confirmation; see the last section.)

## Changed in Task 8 (2026-11-12): settings in play

- **Age Confirmation in play.** `Engine::confirm_age` writes `ConfirmAge` in an OOC Turn when `mature::confirmable` holds: a PC or an NPC, a stored age of 18 or more, and the PC perceives it here now or can name it. Any other entity, a hidden one or a minor included, gets `TurnError::NotConfirmable` and one fixed sentence (`view::CANNOT_CONFIRM`) that says neither which nor why. No Turn row, no model call. The screen says "adult" and never shows a number.
- **The Refusal offers names.** An `adults_only` Refusal reaches the Play screen as `view::RefusalView`: its sentence, and one "Confirm" button for each character in `mature::to_confirm`: those the PC perceives here and can name, with an adult stored age and no confirmation yet. The list reads what the PC perceives now and never the ids that blocked the Turn. So it is the same whether or not a hidden character, a minor, a namesake or a character named elsewhere is the real cause (test `the_confirm_list_is_the_same_whatever_hides_behind_a_refusal`). Each character it lists blocks the Turn anyway: `clearance` covers everyone the PC believes is here.
- **The Detail Level in play.** `Engine::set_detail_level` writes `SetDetailLevel` in an OOC Turn, and at `excluded` closes an open scene in the same Turn (`SceneWhy::Excluded`). A level change clears nobody. The `excluded` sentence now says where to change the level: under "Ask the GM". Changed after QA (2026-11-15): the select moved to the Campaign menu, and the sentence says so.
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

## Changed after the live evaluation (2026-11-12): the stance rule reads `willing`

The scene scenarios of attempt 2 committed 3 scene Turns. The raw answers showed why. On 33 scene steps that the scenarios label intimate, the model tagged 11 `intimate`, 12 `advance` and 10 `none` at the first attempt. In the proposals tagged `intimate`, the willing partner (attitude 5) had 16 reactions: 12 `resist`, 2 `neutral` and 2 `comply`, and each plan went along. The stance rule rejected 19 attempts and refused 8 Turns. The prompt and the schema did not define a stance.

- **A defined stance was tried and removed.** A rule line and a schema description defined each stance value. The partner's stance then matched her plan in 14 of 14 reactions, and 14 scene Turns committed. But the model also gave the magistrate `comply` or `help` when the PC accepted his own request ("I tell Orsel yes"), above his ceiling `neutral`: in `play_basics` the NPC-folds check rejected that Turn twice in 4 of 8 runs. A second wording ("what the PC asks or wants of it") lost it in 5 of 8 runs. A named stance value pushes plain cooperation above the ceiling. The stance stays undefined in the prompt, as before.
- **`willing`.** Each reaction has a required field `willing`, an enum of `yes`, `unsure` and `no`. Its one definition is `domain::WILLING_DEFINITION`; the proposal rule line (`context::WILLING_FIELD_LINE`), the schema's description and the glossary hold the same text, and a test pins them together:

  whether this NPC freely agrees, in THIS Turn, to what the PC does with it or asks of it, as its plan says: "yes" agrees and takes part; "unsure" hesitates or has not agreed yet; "no" declines or objects.

- **The stance rule reads `willing`, never the stance.** For an `intimate` proposal: no reaction, or a reacting NPC whose `willing` is not `yes`, is `Unwilling`. `unsure`, `no` and a missing value refuse: the rule fails closed. It keeps the one retry and the fixed sentence (owner's decision 3).
- **The ceiling holds (owner's decision 4).** In an intimate Turn, `willing: yes` counts as at least `comply` for the NPC-folds check. An NPC whose attitude or goal caps it below `comply` needs a cause: a Check against it, or a fact it holds. A stranger (attitude 0) may say `yes`. On any other Turn nothing reads `willing`, so plain play keeps its ceiling on the stance alone.
- **Every reaction carries it.** The schema is built before the call and cannot depend on the tag the model is about to write. The cost is a few tokens per reaction.
- **Stored proposals still read.** A reaction stored before the field reads no value. The gate never runs on a stored proposal; if it did, it would refuse.

Considered: a rule that reads both the stance and `willing` (rejected: the stance went against the plan in 12 of 16 reactions, so the rule would still refuse a willing partner); a defined stance (measured above, rejected for its effect on the ceiling).

The scene scenarios changed too. An invitation ("I ask Maren to come to bed with me.") is an `advance` and opens no scene, so the act now comes as its own Intent, then one or two Turns of the act going on, then "End scene". The unwilling arm of `npc_will` gained the act with an NPC who does not go along. The live numbers are in `evals/reports/2026-11-mature-content.md` (attempts 4 to 6).

Measured cost (attempt 6): on plain Turns of the tag scenario, Hessa's stance rose to `help` above her ceiling far more often (55 Turns with an NPC-folds rejection, 7 before), often with `willing: no`. Retries rose from 19 to 47 of 240 and failed Turns from 2 to 9. `play_basics` and `speech` did not show it. What stays open: whether the field or its rule line moves the stance; and an act with an NPC who does not go along is tagged `advance` by the model (3 of 3), so it commits as a plain Turn in which the NPC declines, and the stance rule does not run.

## Changed in the polish pass (2026-11-12): no rule line for `willing`; a "no" never stands above the ceiling

The cost of `willing` on plain Turns (the section above) had no separated cause. A fresh run of `intimate_tag` on f11978c (7 of 10 runs, 167 Turns, before the tool's time limit stopped it) gave 27 Turns with a retry and 8 failed Turns (7 by the NPC-folds check, 1 by a model timeout). Its raw log shows two kinds of rejection:

- The NPC-folds check: 27 of the 30 rejected reactions paired Hessa's stance `help`, above her ceiling `comply`, with `willing: no` or `unsure`, on plain Turns and on invitations alike.
- "verdict is check but check is missing": 20 rejected attempts, nearly all on invitations tagged `advance`. Most of them were in the minor arm, where the age rule refuses the Turn anyway, with no retry.

The model writes a reaction's keys in the schema's order (`npc`, `plan`, `stance`, `willing`), and Ollama does not put a schema's descriptions in the prompt, so the field itself cannot move the stance written before it. What the model reads is the rule line. Two A/B runs, 10 runs of 240 Turns each, separate it:

| Code | Turns with a retry | Failed Turns | Turns with an NPC-folds rejection | Attempts rejected for "check is missing" |
|---|---|---|---|---|
| f11978c (7 runs, 167 Turns) | 27 | 8 | 24 | 20 |
| `settle` lowers a contradicting stance (below), rule line kept | 24 | 4 | 13 | 33 |
| rule line removed, nothing else | 15 | 0 | 9 | 23 |
| both (the code of this section) | 10 | 2 | 4 | 18 |

The rule line ("whether this NPC freely agrees ... to what the PC does with it or asks of it") is the cause of the NPC-folds cost: with it, an ordinary question to Hessa read as a request she agrees to. The "check is missing" rejections do not follow the line; they are the repair round of ADR 0007 for a `check` verdict that is not contested, and they stay.

- **No rule line.** `PROPOSAL_RULES` no longer holds `WILLING_FIELD_LINE`; the constant is gone. The field stays required on every reaction with the same enum, and `WILLING_DEFINITION` stays the schema's description, the glossary's and this ADR's text (test `the_willing_definition_is_one_text`, which also checks that the rules name no `willing`). The model reads the field's name and its three values.
- **A "no" never stands above the ceiling (removed in round 2, below).** `checks::settle` lowered the stance of a reaction whose `willing` is `no` or `unsure` and that gives no cause to the NPC's ceiling, on any Turn (`settle_stances`). With the rule line removed, 6 of the 9 rejected reactions were still `help` with `no`; with both changes, `settle` lowered 7 stances in 240 Turns.

Final numbers of this section: `evals/reports/2026-11-polish-pass.md`.

## Changed in the polish pass, round 2 (2026-11-12): no stance repair

An independent review (Major 1) found that `settle_stances` committed a declined reaction as `comply`, and the narrator reads the stance only: an NPC whose plan says no was told as an NPC who goes along. The table above shows that the repair was not needed: with the rule line removed and nothing else, 15 Turns of 240 needed the retry and none failed. The lead removed it.

- `checks::settle` changes no stance. `settle_stances`, its call and its test are gone. The rule line stays removed (`the_willing_definition_is_one_text`).
- The NPC-folds check rejects a stance above the ceiling with no cause again, whatever `willing` says, as at f11978c. On a Turn that is not `intimate` nothing reads `willing`.
- **An `intimate` Turn with `willing: no` and a stance above the ceiling.** It fails two checks: the stance rule (`Unwilling`) and the NPC-folds check. The retry prompt lists both. When the retry does the same, `mature::only_unwilling` is false, so the Turn is not an `Unwilling` Refusal: it fails as a rejected Turn (`TurnError::Rejected`), with no Commit and the Player's Retry. With `settle_stances`, the stance was lowered first and the same Turn ended as an `Unwilling` Refusal. At or below the ceiling, `no` is still an `Unwilling` Refusal after the retry. Test: `an_unwilling_npc_above_its_ceiling_fails_two_checks`. The final run of the first round had one such Turn (Orsel's `yes` above his ceiling, then `no`); its count in round 2 is in the report.


## Changed in the QA fixes (2026-11-15): Act Words make the Intent `intimate`

QA played "I make love with Maren." at `excluded`. The model tagged it `advance` in one run of two, so the Romance Words floor (`advance`) let it pass, and the level refuses only `intimate`. `mature::ACT_WORDS` is a shorter list of neutral phrases that name the act itself ("make love" and its usual forms). When the Intent holds one, `mature::intimacy_floor` raises the tag to `intimate`, as a pack's word list does: the level refuses it, and at `implied` the age rule and the stance rule run. The debug row says the core raised the tag from its Act Words, never which word. A kiss and an invitation ("share my bed") stay Romance Words. Cost: "I ask Maren to make love" is `intimate` too. The live numbers are in `evals/reports/2026-11-qa-fixes-engine.md`.

## Changed in QA run 3 (2026-11-15): taking someone to bed is an Act Word phrase; an advance is told as one

At `implied`, "I take Maren to bed." came back tagged `advance`, against the tag's own definition ("going to bed together"). No Intimate Scene opened, so no End scene and no Fade to black, but the Narration led Maren to the alcove and had her undress the PC. Two changes:

- **The phrase.** `mature::intimacy_floor` also reads "take or lead someone to bed" as an Act Word: a verb of `mature::TO_BED_VERBS` (take, lead and their forms), then her, him, them, me, or one or two words with a capital (a name), then "to bed" or "into bed". The capital is read from the Player's own words. "I go to bed early", "I take my book to bed" and "I take the lamp to bed" stay `none`. "Put", "carry" and "tuck" are not on the list: they are how a Player puts a child to bed. Cost: "I take Pim to bed" is `intimate` too, so the age rule refuses it when Pim is a child, and "I take my wife to bed" is not caught (no capital): the model's tag must carry it. This is still a word list; we found no better signal in the design, since the model's tag is the signal it missed.
- **The narrator of an advance.** A Turn tagged `advance` opens no scene, and the stable `INTIMATE_RULE` alone did not stop the Narration. Its outcome now holds `context::ADVANCE_LINE`: no intimate scene begins; tell the advance and the answer, then stop before anyone undresses or anything intimate happens. A Narrate again of such a Turn gets it too. This holds for any advance the net misses, with no word list. Neither change was measured on the live model.
- **Widened after review (QA B1, 2026-11-15).** A name word may be lower case when it is a word of the name of a character the PC can name (`world::pc_can_name`): "i take maren to bed" holds the phrase. One possessive may stand before "bed": "I lead her to my bed", "Maren takes me to her bed". The names are never world truth: a lower-case name of a hidden character would be refused at `excluded` where "i take tea to bed" is not, and the Player would learn that the character exists. "I go to bed early", "I take my book to bed" and "i take tea to bed" stay `none`; "I take Pim to bed" stays `intimate`; "I take my wife to bed" is still not caught.

## Changed after the review of the place tree (2026-11-16): the Player confirms the Inside Links

The review found that the `place_tree` stage was a trust boundary with no check. The model's links decide which Places are apart, and two Places that are apart let `mature::clearance` treat a character as away. A model that answered "none" for every Place, or put a room inside a river, let an Intimate Scene through with a child in the next room. An Inside Link is now trusted only after a fact check and the Player's choice.

- **The stage moved to the draft.** `place_tree` is the last stage of `draft_from_text`, not of the World Build. Its prompt also holds the PC's known facts and the Source Text. It is optional: a model error leaves no proposal, and every Place waits for the Player. A cancel still stops the draft.
- **The fact check.** `world::presence::doubted_places` uses the rules of `inferred_links`. Rule 1: the Places that a Place's location facts name must sit on the proposed chain above it. Rule 2: two Places that name each other in a location fact must be on one chain. A Place that breaks a rule gets the link "unknown" and the mark "doubted". The check only moves a link toward "unknown", never away from it.
- **The Seed review.** Each Place row has an "Inside" select: "Choose…", "Top level: inside no other place", the other Places (not the Place itself, nor one inside it) and "Not sure". A doubted link shows "needs a choice". "Confirm" calls `confirm_inside`. The core refuses a parent that is no Place of the draft, the Place itself and a loop. Until each Place is confirmed it has a `place_unconfirmed` gap, so Approve waits, as for an age. A rename of a Place asks for a new choice and follows the name in the other links.
- **Only a backed link starts filled (added the same day).** The fact check cannot catch a wrong link that no fact contradicts. In the QA campaign `campaign-1794515377502292640`, no fact says where the Copper Kettle is. A model that answers "none" for it, or puts it in the river Sallow, passes the check. With Pim (11) in Ostley Crossing, the gate then cleared the PC in the Kettle, if the Player pressed "Confirm" on the model's choice. So the review starts on the model's link only when a fact backs it: `world::presence::backed_places` finds that same parent through the rules of `inferred_links` (subject rule, else text rule). An ancestor does not back a link. `Top` is never backed, since no fact can say that a Place is inside nothing. Every other link starts on "Choose…", and the model's guess shows as a quiet hint ("Suggested: inside Ostley Crossing"). The Player must pick and confirm on purpose. The core holds the rule: `PlaceLink.backed` is stored on the draft, and `place_lines` gives the starting choice in `inside` and the guess in `suggested`. A draft saved before the field existed starts every unconfirmed link blank. The gaps and Approve do not change.
- **The World Build writes the confirmed links only.** "Not sure" and a link still unconfirmed give no `SetInside`: the Place stays unknown. The fact check runs again on the facts that the World Build adds. A new fact that contradicts a confirmed link drops that link, so it fails closed. A fact the Player saw in the draft does not count twice.
- **Limits.** Rule 2 doubts too much: two Places that name each other in a fact both lose their link. The Player can confirm them again, but the World Build check can drop a Player's choice for a fact it adds. That is the safe side. A Player who picks a wrong link makes the gate fail open for that pair: the check reads facts, not the world. A fact can back a wrong link too, when the fact itself is wrong. A rename does not change a Place name inside a fact's value.
- **The Refusal text.** With nobody to confirm, it now says "An Intimate Scene cannot happen here. Someone may not be a confirmed adult." It no longer says "nearby": with unknown links, the person may be anywhere.
