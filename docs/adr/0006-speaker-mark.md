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

What this does not cover:

- The narrator can still write an unknown name inside prose ("Kestrel smiles."). Rule 4 of data shape 3.5 then makes that name known, because the Narration printed it. Only a prompt change can stop that (data shape 3.5, "Cost if wrong").
- While a Narration streams, the Player reads the raw text, marks included. A tag with an unknown name shows until the Turn commits and the blocks replace the stream.

## Considered options

- **A guess from the prose.** Rejected. One name near a quote can be the listener, not the speaker: "…, the guard tells Mara" gives a Mara bubble. A wrong bubble is worse than no bubble.
- **A spoken line in the Decision Proposal, committed before the Narration.** Rejected. It adds about 1 s to each Turn, and the proposal model writes worse dialogue.
- **A structured Narration (JSON parts).** Rejected. It breaks the stream, and the model drifts more on it.
- **A model call after the Narration.** Rejected. It is a guess too, and it costs time.

## Consequences

- No model call, no proposal field and no ledger op is added. The cost is a few output tokens for each spoken line.
- Stored Narrations hold the mark. While a Narration streams, the Player reads the raw text, marks included. The bubbles come with the Play view after the Commit.
- A model that drops the mark gives prose and no error. A model that tags the wrong present NPC gives a wrong bubble, and code cannot see it. The evaluation measures both.
- The name in a mark does not count as a Narration naming the speaker: a line whose mark cannot resolve must not make its own speaker resolve. A hidden other name never resolves.
- It is easy to reverse: the parser stops, and the mark reads as plain text.
