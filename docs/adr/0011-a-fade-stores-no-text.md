---
status: accepted
date: 2026-11-12
---

# A Fade to Black stores no text and is not in the ledger

## Context

"Fade to black" (design 6.6) lets the Player stop and hide a Narration with one button, during any Turn or inside an Intimate Scene. The Turn is already committed when its Narration streams, so the world keeps what happened. The hidden text must reach no Player screen and no later prompt, and must stay hidden after a rebuild from the ledger and after a restart. The stream must stop at once, although a running Turn holds the session lock.

## Decision

- **No text is stored.** `Store::set_faded` sets `turn.narration` to NULL and `turn.faded` to 1, for a Play, Correction or Closing Turn only; it refuses any other kind. `Store::set_narration` refuses a faded Turn, so nothing un-fades it. The column is not a projection: `rebuild` leaves the `turn` table as it is, and `project::historical` copies `faded` with the row.
- **Not in the ledger.** No op records a fade. The ledger of a faded Turn is the ledger of the same Turn read in full; `faded` is the record of the skip.
- **One function says what a reader gets.** `mature::turn_text` gives a faded Turn's Neutral Line to every prompt, and `mature::neutral_line` makes it from the Turn row by kind: `FADED_TEMPLATE` for a faded Play or Correction Turn, `CLOSING_TEMPLATE` for a Closing Turn, `INTIMATE_TEMPLATE` for a Turn committed with `intimate`. No model writes it; it holds the minutes of the Turn and no name, fact value or model text.
- **Screens.** `SightTurn.faded` carries the mark. The Play story shows the Player's own Intent and `TurnBody::Faded { line }`; the Journal shows `Act.faded` in place of the Intent; the OOC sheet and a pinned act show the line.
- **The stream stops.** `Gm::narrate` takes a reader that can answer `Stream::Stop` (plan Task 3). The shell owns one fade flag; `fade_to_black` raises it with no lock, like `cancel_build`. `turn::narrate_through` reads the flag before the call and after each chunk: a flag already up makes no narration call; once it rises, no more text reaches the screen and the model gets `Stop`. The Engine reads it once more when the call returns, so a fade after the last chunk still fades. Each Turn command clears the flag when it starts.
- **Inside a scene.** When the Commit of the faded Turn left an Intimate Scene open, the Engine runs the Closing Turn in the same call, born faded: one proposal call, no narration call, `why: FadeToBlack`, under the op `<op_id>__fade_close`. With no Turn running, `Engine::fade` does the same; with no scene either, it returns `NothingToFade`. The Turns already read keep their text (owner's decision, data shape 16).
- **A fade the running Turn missed.** The Player can press the button in the last moment of a stream, after the Turn read the flag for the last time. The UI sends the fade with the op_id of the Turn that was streaming, and sends it once more when that Turn comes back read in full. `Engine::fade` then fades the Turn that op_id names only when it is the latest Turn of the Campaign, is not faded, and is a Play, Correction or Closing Turn (`TurnKind::fades`), and closes a scene it left open, faded. Any other Turn (an older one, the Opening, a Setup or OOC Turn) is returned as stored, with no model call, even when its Narration is missing. `fade` makes no model call but the proposal of a faded Closing Turn.
- **Retry and renarrate.** A retry under the same `op_id` returns the Neutral Line with no model call. `renarrate` of a faded Turn is `TurnError::Faded`.
- **The debug log keeps the text received before the stop** (owner's decision 5): the `narrate` row holds the prompt, that text, then the line `[faded]`. The log is written in every Campaign; only `get_debug` reads it, in debug mode.
- **Correct last Turn.** A faded Play Turn that is the last Turn can be corrected; the Correction is read in full unless the Player fades again. A faded Turn of a scene is followed by its Closing Turn, so it is no longer the last Turn.

## Considered options

- **A `FadeTurn` ledger op.** It needs a carrier Turn row after the faded Turn, and "Correct last Turn" then finds a carrier where it expects a Play Turn. The Narration was never in the ledger; its absence does not belong there either. Rejected.
- **The Neutral Line written into `turn.narration`.** Readers would need no change, but a Turn committed with `intimate` and read in full needs the same line in later prompts, so the readers branch anyway, and a change of the wording would un-fade old Turns. Rejected.
- **Logging `[faded]` and a character count only.** Rejected by the owner (decision 5).

## Consequences

- The Player cannot un-fade. That is the intent of the button.
- A fade with no Turn running and no op_id of a Turn fades nothing that was read: with a scene open it closes the scene, faded; with none it is `NothingToFade` and the UI drops it.
- After a crash between `set_faded` and the Closing Turn, the Turn shows faded and the scene is still open; either button closes it.

## Changed after review (2026-11-12, second review)

- **A late fade acts on the latest fadeable Turn only, and calls no model.** `fade(op)` faded an older Turn whose Narration was missing (through `stored_outcome` and a hidden `renarrate`), and `fade("__begin__")` made a hidden narration call for an Opening whose Narration was missing. Now `Engine::fade` fades only the latest Turn, and only a Play, Correction or Closing Turn (`TurnKind::fades`); any other Turn is returned as stored, with no model call. `Store::set_faded` refuses every other kind. The review asked for Play and Closing only; Correction stays fadeable because the button can fade a Correction while it streams (data shape 3.6), and refusing `set_faded` there would fail that fade after its Commit.
- **The Opening never reads the flag**, by the button, a `renarrate` or a late fade; the UI disables the button while Begin or a renarration of the Opening streams.
- **The shell knows which Turn runs.** `fade_to_black` used `try_lock` and took any holder of the session lock for a running Turn, then left the flag up. `session::FadeGate` now holds the flag and the op_id of the running Turn command; each command marks itself as running, which clears the flag, and the mark and the flag go down together when it ends. With no Turn running, `fade_to_black` takes the lock as any command does and runs the idle path.
- **A renarration gets the late fade too.** The UI sends the fade again with the renarration's op_id when it ends; the shell maps that op_id to the Turn it told.
- **Every reader of a faded Turn reads its line.** A pinned act of a faded Turn showed its Intent. The Journal and a pin now share `view::act_words`. The Play story keeps showing the Player's own Intent above the Neutral Line (data shape 3.11).
- **Data shape 3.6 and 8 now describe the late fade**: a Turn the Player saw stream in full is faded and stores no text, as the owner decided for every faded Turn.

## Changed after the final review (2026-11-12)

- **A late fade skips OOC Turns.** "The latest Turn" meant the latest Turn row, so an OOC Turn the Player committed after the stream (an Age Confirmation, a level) left the streamed Turn unfaded. `Engine::fade` now fades the Turn its op_id names when that Turn is the latest Turn that tells (`TurnKind::tells`: Opening, Play, Correction, Closing), is not faded, and is a Play, Correction or Closing Turn. Test: `a_late_fade_skips_an_ooc_turn_after_the_streamed_turn`.
- **A Turn of another kind under the same op_id is never narrated.** `play_turn`, `correct_last`, `end_scene` and Begin find a stored Turn under their op_id only when it is of their own kind; another kind is `TurnError::OpIdTaken`, with no model call. `confirm_age` and `set_detail_level` do the same for an OOC Turn. `renarrate` and `Store::set_narration` refuse a Turn that tells nothing (Setup, OOC).
- **At `excluded`, no narration call for an earlier intimate Turn.** `renarrate` of a Turn committed with `intimate`, and a retry of its op when its Narration is missing, give its Neutral Line with no model call and leave the row as it is.
