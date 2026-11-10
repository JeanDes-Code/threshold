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
