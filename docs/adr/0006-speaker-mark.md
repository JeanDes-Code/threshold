---
status: accepted
date: 2026-11-10
---

# The speaker of a line is a mark that the narrator writes

The Play screen shows an NPC's spoken line in a bubble with the NPC's name. The narrator writes the speaker before the line, in a fixed form: a line of its own with the NPC's name, a colon, then the spoken words in quotes. One rule line in the narrator prompt (`speech::RULE`) asks for it, and `speech::parse` reads it. The Narration is stored with its marks. The core shows a bubble only when the name resolves to exactly one entity, of kind NPC, present in that Turn, whom the PC can name, by a name the PC knows. Any other line stays prose, with its text unchanged.

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
