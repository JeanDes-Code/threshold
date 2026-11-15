---
status: accepted
date: 2026-11-11
---

# Old Campaign files are refused, not migrated

## Context

The mature content module adds four columns to the Campaign file: `campaign.detail_level`, `campaign.scene_open_turn`, `entity.age_confirmed` and `turn.faded`. The schema version moves from 3 to 4, once, for all four.

## Decision

Schema 4 refuses a version 3 file, as version 3 refused version 2. `migrations::apply` keeps its rule: an older version does not open, no migration runs, and the file stays as it is. `Store::open_existing` and `store::peek` both refuse it with a message that names its version. The home screen shows the card with its problem and "Remove from list".

## Considered options

- **`ALTER TABLE` with safe defaults.** It works. It would be the first migration this code has to keep correct. A migrated file would also hold ages that nobody confirmed through the new path, so the Player would confirm each character in play anyway.

## Consequences

- The owner builds his test Campaigns again from their Source Texts.
- The first migration will be written when a file exists that the owner cannot build again.

## Changed after the review of QA run 3 (2026-11-15): the first migration, version 4 to 5

Schema 5 adds the Inside Link of a Place (`entity.inside_known`, `entity.inside_id`; ADR 0008). The owner's QA run 3 Campaigns are version 4 files, and B1 must be checked on them, so this is the first migration the code keeps. `migrations::MIGRATES_FROM` is 4: a version 4 file gets the two columns and, once, the links its facts give (`world::presence::inferred_links`), written to the ledger as `SetInside` ops in the Turn of the later of the two Places, so a rebuild and a historical view find them. All of it runs in one transaction. A version 3 file, or any other version, is still refused and stays as it is.
