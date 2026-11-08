---
status: accepted
date: 2026-11-08
---

# Start clean and copy parts of SNEQ, not depend on it

Threshold does not depend on `sneq-engine`, even though Arcanum Solo does. SNEQ keeps a private SQLite connection, so it cannot join Threshold's single Commit, which writes rules state, world state and the event log in one transaction. It has no World Clock and no scheduled World Events. And it can add 0 to 2 model calls per Turn, which the 15 s budget (ADR 0001) cannot pay for. We copy four parts instead: the holder and derived-Belief model with delayed news (`core/derive-beliefs.ts`, `domain/carriage.ts`, `domain/holder.ts`), the promotion rules for invented details (`core/promotion.ts`), the append-only ledger with projected state and `operationId` replay protection (`core/projection.ts`), and name normalisation with the word-level leak check (`resolver/normalize.ts`, `core/containment.ts`). Source: `sneq-engine` 0.6.2, MIT, owned by the same author.
