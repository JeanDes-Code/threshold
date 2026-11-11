---
status: accepted
date: 2026-11-10
---

# The speaker of a line is a mark that the narrator writes

The Play screen shows an NPC's spoken line in a bubble with the NPC's name. The narrator writes the speaker before the line, in a fixed form: a line of its own with the NPC's name, a colon, then the spoken words in quotes. One rule line in the narrator prompt (`speech::RULE`) asks for it, and `speech::parse` reads it. The Narration is stored with its marks. The core shows a bubble only when the name resolves to exactly one entity, of kind NPC, present in that Turn, whom the PC can name, by a name the PC knows. Any other line stays prose. When its tag is a name the PC cannot use for some entity it names, the tag goes and the quoted words stay. Otherwise the text stays unchanged.

## Changed after review (2026-11-10)

The first version kept every line that did not resolve with its text unchanged. A review found that this prints a name the PC does not know: `Kestrel: "Nobody knows my name."` showed the stranger's name while the side panel said "unknown", and `The Toll-Wolf: "…"` showed a hidden other name. The rule is now:

- The tag is a name of an entity (main name or other name, through `normalize::name`), and the PC cannot call that entity by that name. The line shows the quoted words only, with their quote marks. The tag and the colon are removed.
- The tag names several entities and at least one of them is not nameable by the PC by that name. The tag is removed too.
- The tag names no entity ("Thunder"). The text stays unchanged: it tells nothing about the world.

## Changed after a second review (2026-11-10): the stream

While a Narration streamed, the Player read the raw text, so `Kestrel: "…"` showed until the Turn committed. The core now closes that case. `speech::TagFilter` sits between the model's chunks and the `narration_chunk` events. At each line start it holds the text back until a colon, a newline or 48 characters (`TAG_HOLD_CHARS`). When the text before the colon is, through `normalize::name`, a name in `world::unnameable_names`, it drops that text, the colon and the blanks after it. Any other text passes unchanged. The set is read once before the narrator call, from the same rule as the commit-time one, so a tag is in it exactly when the screens remove it after the Commit. The filter covers the play Turn, the Correction, the opening Narration and `renarrate`. It adds no model call and no change to the `Gm` trait. The stored Narration does not change.

What this does not cover:

- The narrator can still write an unknown name inside prose ("Kestrel smiles."), in the stream and after the Commit. Rule 4 of data shape 3.5 then makes that name known, because the Narration printed it. Only a prompt change can stop that (data shape 3.5, "Cost if wrong").
- The stream filter drops a hidden name before a colon even when the line is not a full mark (`Kestrel: he waits.`). The screens keep such a line after the Commit, because it is not a mark. That is the safe side: the name shows later, from the stored text, by the commit-time rule.
- A tag of 48 characters or more is not held, so it is not dropped while it streams. No entity name in the fixtures comes close to that length.

## Considered options

- **A guess from the prose.** Rejected. One name near a quote can be the listener, not the speaker: "…, the guard tells Mara" gives a Mara bubble. A wrong bubble is worse than no bubble.
- **A spoken line in the Decision Proposal, committed before the Narration.** Rejected. It adds about 1 s to each Turn, and the proposal model writes worse dialogue.
- **A structured Narration (JSON parts).** Rejected. It breaks the stream, and the model drifts more on it.
- **A model call after the Narration.** Rejected. It is a guess too, and it costs time.

## Consequences

- No model call, no proposal field and no ledger op is added. The cost is a few output tokens for each spoken line.
- Stored Narrations hold the mark. While a Narration streams, the Player reads the text with the tags of unknown names removed, other marks included. The bubbles come with the Play view after the Commit.
- A model that drops the mark gives prose and no error. A model that tags the wrong present NPC gives a wrong bubble, and code cannot see it. The evaluation measures both.
- The name in a mark does not count as a Narration naming the speaker: a line whose mark cannot resolve must not make its own speaker resolve. A hidden other name never resolves.
- It is easy to reverse: the parser stops, and the mark reads as plain text.

## Changed in bug pass 4 (2026-11-11): a short stage note before the quote

QA saw `Agathe Morrow: (Wiping the bar) "…"` shown as a script line: the parser wanted the quote right after the colon. A mark may now hold one stage note in parentheses between the colon and the quote, of at most 60 characters with the parentheses (`speech::MAX_STAGE_NOTE_CHARS`) and with no quote mark inside. `speech::parse` drops the note and keeps the words. The bubble rule is unchanged: exactly one present NPC whom the PC can name. A longer note, a note with a quote mark, or one that never closes leaves the line as prose. The stream filter is unchanged: it reads only the text before the colon.
