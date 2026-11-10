---
status: accepted
date: 2026-11-10
---

# Open items are facts on `pending_` keys

A Source Text holds open items: a plan, a proposal, a thing that is pending. We store each one as an Established fact about the thread, on a key with the prefix `pending_`. The slot of the outcome stays empty, so no check and no prompt can read the plan as done.

## Considered options

- **A Possible fact on the outcome slot.** Rejected. A Possible fact becomes Established when a proposal cites it or when the PC witnesses it. The plan would then be a past event.
- **An `open` column on `fact`.** Rejected. Every check, every prompt line and the live-slot index would need the new rule.

## Consequences

- One function in `domain` knows the prefix. One marker in `context` shows an open fact as not done.
- An open key is a state key. The fact is never Persistent, and a later Turn can replace it.
- A status lives in a string. The link between `pending_k` and `k` depends on both sides using the same key. The false-past evaluation measures the drift.
- This is hard to reverse once ledgers hold such keys.

## Changed after QA (2026-11-10): the PC closes its own items

QA found that an Open Item on the PC could never close. The PC accepted a proposal ("I will carry the winter post"), but the agency check rejected any PC write on a key outside the involuntary keys, and the prompt never said that an item on `pending_x` closes with a fact on `x`. The rule is now:

- The PC may take a fact on key `x` when the PC holds a live fact on `pending_x` about the PC (`world::pc_open_outcomes`). Only a proposal fact counts, never a stake or an event effect. The model judges whether the Player's Intent settles the item; the check enforces the key rule only.
- A write on `x` for another subject was already allowed.
- One proposal rule line explains the closing rule. The PC's entity line lists the keys it may settle after `settles:`.
- `world::pc_sight` then closes the item in that Turn, and the Turn counts as a decision.
