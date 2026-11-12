---
status: accepted
date: 2026-11-12
---

# The Intimate Scene is one op and one column

## Context

An Intimate Scene lasts several Turns (design 6.4). Its state decides what enters a prompt and what a Commit check accepts, and both read `WorldView`, which comes from projections. The state must survive a restart, a rebuild and a Correction, and the age rule must hold on every Turn of the scene, not only on the Turn that opened it.

## Decision

- **One op, one column.** `LedgerOp::SetScene { open, why }` opens or closes the scene. Its projection is `campaign.scene_open_turn`: the Turn that committed the opening op, `WorldView::scene_open`. `rebuild` sets it to NULL and replays the ops, so a Correction that reverts a scene Turn takes its `SetScene` op with it.
- **The Turn's own Commit.** `mature::scene_op` runs in `turn::resolve`, after G3, on the state before the Turn and the presence after its ops (`mature::after`). It returns at most one op, added to the Turn's single Commit. The rows of data shape 3.4 that this task owns:

  | Scene before | The Turn | Op |
  |---|---|---|
  | closed | `intimate`, G2 and G3 pass | open, `Opened` |
  | closed | not `intimate` | none |
  | open | not `intimate` | close, `NotIntimate` |
  | open | `intimate`, the PC's place changes | close, `Moved` |
  | open | `intimate`, a character the PC did not see here now sees here | close, `Arrival` |
  | open | `intimate`, none of the above | none |
  | open | G1 failed when the Turn started | close, `Gate` |

  A Turn that closes a scene never opens one. An arrival the PC does not see does not end the scene; the age rule still reads world truth and refuses the Turn when that character is not cleared (G3).
- **G1 before the proposal call.** `turn::plan` calls `mature::scene_gate` first. With an open scene whose place is cleared, the proposal prompt gets the `Cleared` value: the core's `context::SCENE_LINE` opens the `recent` block, the Turns of the scene are shown in full, and the schema takes a reaction plan of 1 to `SCENE_PLAN_MAX_CHARS` (200) characters. When the place is not cleared, the Turn is planned as a plain one and its own Commit closes the scene with `why: Gate`. The value lives in that call only; no struct field holds it and no Commit outlives it.
- **One function says what a reader gets of a Turn.** `mature::turn_text` gives a Turn committed with `intimate` in full only to a prompt that holds a `Cleared` value and whose open scene started at or before that Turn. Every other reader (a prompt after the scene, a prompt whose place is no longer cleared, the OOC sheet) gets its Neutral Line, `mature::INTIMATE_TEMPLATE` with the Turn's minutes: no name, no fact value, no model text. The Play story still shows a Turn the Player read in full. The facts the scene committed stay in the world, as any fact does.
- **The narration call of a scene.** `Engine::narrate` gives the narration prompt the clearance of the state after the Turn only while that scene is still the open scene (`mature::narration_clearance`), so a `renarrate` after the scene ended reads neutral lines. A Narration told by implication inside a scene carries the token cap `mature::SCENE_NARRATION_MAX_TOKENS` (280). A Narration cut by the cap is stored up to its last full sentence (plan Task 3).
- **The prefix stays.** The scene line and the scene Turns sit in the `recent` block. The system, stable and scene blocks do not change when a scene opens or ends (ADR 0001).

## The token cap and the 15 s Turn budget

ADR 0001 gives a Turn 15 s. The replay of bug pass 5 (`evals/reports/2026-11-turn-time.md`, warm model) measured a Turn at p50 9.0 s and p90 13.1 s, a Narration of 162 tokens at p50, generated in about 3.1 s (about 52 tokens per second; the data shape's planning rate is 43).

- **The cap is a ceiling.** 280 tokens take 5.3 s at the measured rate and 6.5 s at 43 tokens per second. A scene Narration that reaches the cap costs about 118 tokens more than today's p50 Narration: 2.3 to 2.7 s. The narrator's rules still ask for two short paragraphs, so most scene Narrations should stay near today's length.
- **The prompt reads no more than today.** A scene Turn shows the Turns of the scene in full, as every Turn showed every past Turn before this task, inside the same window of `TURNS_WINDOW_MAX_CHARS`. The scene line adds about 60 tokens to read. Outside a scene, a Neutral Line replaces a Narration and the prompt gets shorter.
- **The proposal can write more.** Two reaction plans of up to 200 characters add at most about 60 output tokens, about 1.2 s.
- **What it means for the budget.** A scene Turn whose Narration reaches the cap and whose plans are full adds about 3 to 4 s to a Turn at the p50, so about 12 to 13 s; at today's p90 it can pass 15 s. No scene Turn has been timed. The `scene_latency` evaluation of plan Task 10 measures it on the live model, and the plan's one allowed correction lowers `SCENE_NARRATION_MAX_TOKENS` by the measured gap. A cap that changes with the time left in the Turn is out of scope (data shape 10).
- **Without the cap.** The probe of the design (300 words) took 10 to 27 s for the Narration alone. The cap is what holds a scene Narration near the budget; a prompt rule alone did not.

## Considered options

- **A view rule over the last standing Turn's `proposal_json` and ops.** It needs no schema change, but the gate would parse stored JSON on every Turn, and "End scene" would have to be a Play Turn with a reserved Intent text that the Player can also type. Rejected. The rule stays as a test: `scene_projection_matches_the_last_turn_rule` checks that the projection equals it over a scripted run.
- **G1 closes the scene in an OOC Turn of its own, before the proposal call** (data shape 3.5). Rejected for this plan (deviation 1): inside `correct_last`, a separate Commit would make the corrected Turn no longer the last Turn. Closing in the Turn's own Commit gives the prompt the same thing: no scene text.

## Consequences

- A file that another program changed can hold an open scene whose place is not cleared. The next Turn is planned as a plain one, and its Commit closes the scene.
- `checks::validate` reads `WorldView::scene_open`, not the result of G1, for the plan limit (plan deviation 16). In such a file the schema of that one Turn says 90 characters while the validator accepts 1 to 200, so an empty plan there costs a retry.
- After a scene ends, the GM model and the narrator know its committed facts, not its wording.
- The model can still end a scene by mistake with `intimate: false`. The next intimate Turn opens a new one; no state is lost.

## Changed in Task 5 (2026-11-12)

- **"End scene" closes the scene in a Closing Turn** (`TurnKind::Closing`, data shape 3.7). `Engine::end_scene` runs `plan` with the closing prompt (`context::closing_context`: the scene in full while G1 passes, and `context::CLOSING_ASK` in place of an Intent), the pinned schema (`closing_schema_for`) and the same pins in code (`checks::settle_closing`). Its Commit holds `SetScene { open: false, why: EndScene }`. When the model fails or both attempts are rejected, the core commits `mature::default_close`: 15 minutes and the `SetScene` op, nothing else. The Narration is told plain under `context::CLOSING_LINE`, with no Player intent line and a cap of `mature::CLOSING_NARRATION_MAX_TOKENS` (90). Later prompts and the OOC sheet read a Closing Turn as its Neutral Line (`mature::CLOSING_TEMPLATE`).
- **A scene that G1 no longer clears is open on no screen.** A review found that a Turn whose G1 fails and that G2 or G3 then refuses commits nothing, so the ledger kept the scene open and the Play screen showed it until a later Turn committed. Every reader outside the ledger now reads `mature::open_scene`: the scene of `WorldView::scene_open` while G1 passes on it. `PcSight::scene` (the Play screen, the two buttons) and the plan limit of `checks::validate` read it, so the schema and the validator agree on 90 characters in such a file (this replaces the "Consequences" line above on the plan limit, and plan deviation 16). No prompt could hold the scene's text before either: G1 and `narration_clearance` give no `Cleared` value. The ledger closes the scene at the next Turn that commits (`why: Gate`). We rejected a separate core-only Turn that commits the close at once: inside `correct_last` it would stop the corrected Turn from being the last Turn, and "Correct last Turn" would need a rule to skip it.
