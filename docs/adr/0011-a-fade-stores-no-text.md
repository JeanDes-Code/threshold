---
status: accepted
date: 2026-11-12
---

# A Fade to Black stores no text and is not in the ledger

## Context

"Fade to black" (design 6.6) lets the Player stop and hide a Narration with one button, during any Turn or inside an Intimate Scene. The Turn is already committed when its Narration streams, so the world keeps what happened. The hidden text must reach no Player screen and no later prompt, and must stay hidden after a rebuild from the ledger and after a restart. The stream must stop at once, although a running Turn holds the session lock.

## Decision

- **No text is stored.** `Store::set_faded` sets `turn.narration` to NULL and `turn.faded` to 1. `Store::set_narration` refuses a faded Turn, so nothing un-fades it. The column is not a projection: `rebuild` leaves the `turn` table as it is, and `project::historical` copies `faded` with the row.
- **Not in the ledger.** No op records a fade. The ledger of a faded Turn is the ledger of the same Turn read in full; `faded` is the record of the skip.
- **One function says what a reader gets.** `mature::turn_text` gives a faded Turn's Neutral Line to every prompt, and `mature::neutral_line` makes it from the Turn row by kind: `FADED_TEMPLATE` for a faded Play or Correction Turn, `CLOSING_TEMPLATE` for a Closing Turn, `INTIMATE_TEMPLATE` for a Turn committed with `intimate`. No model writes it; it holds the minutes of the Turn and no name, fact value or model text.
- **Screens.** `SightTurn.faded` carries the mark. The Play story shows the Player's own Intent and `TurnBody::Faded { line }`; the Journal shows `Act.faded` in place of the Intent; the OOC sheet and a pinned act show the line.
- **The stream stops.** `Gm::narrate` takes a reader that can answer `Stream::Stop` (plan Task 3). The shell owns one fade flag; `fade_to_black` raises it with no lock, like `cancel_build`. `turn::narrate_through` reads the flag before the call and after each chunk: a flag already up makes no narration call; once it rises, no more text reaches the screen and the model gets `Stop`. The Engine reads it once more when the call returns, so a fade after the last chunk still fades. Each Turn command clears the flag when it starts.
- **Inside a scene.** When the Commit of the faded Turn left an Intimate Scene open, the Engine runs the Closing Turn in the same call, born faded: one proposal call, no narration call, `why: FadeToBlack`, under the op `<op_id>__fade_close`. With no Turn running, `Engine::fade` does the same; with no scene either, it returns `NothingToFade`. The Turns already read keep their text (owner's decision, data shape 16).
- **A fade the running Turn missed.** The Player can press the button in the last moment of a stream, after the Turn read the flag for the last time. The UI sends the fade with the op_id of the Turn that was streaming, and sends it once more when that Turn comes back read in full. `Engine::fade` then fades the Turn that op_id names when it is the latest Turn of the Campaign, not faded and not the Opening (and closes a scene it left open, faded). An older Turn is never faded this way.
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
