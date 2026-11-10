# Campaign from a document: rulings, open items and QA findings

Decisions the controller (Claude Code) took on the owner's behalf while executing the plan, in order. Each line: what was decided, why, and what it costs if wrong. Review and undo any you disagree with. Two were overturned by the final review and are marked.

## Rulings

- Ruling: `normalize::name` drops combining accents (Chloé = chloe) — plan Review Focus 4 asks for one entity per person across accents — if wrong, two names that differ only by an accent collide and show as a DuplicateName gap.
- Ruling: three validation minors (one-way duplicate check, same-normal-form other names, "world" as other name) go into fix round 1 with the Important one — each can crash or corrupt a seed commit, and they sit in the lines the fix touches — cost: a slightly larger fix diff.
- Ruling: world::hidden keeps lowest-id matching — the old code also hid a later NPC that only shared a hidden entity's name, which was wrong — if wrong, a same-named play NPC of a hidden seed entity is visible on arrival.
- Ruling: a FactEntry commits as Persistent like a seed decided fact (state keys stay Established) — shape 3.4 says a settled known fact commits as a decided fact does — if wrong, build facts are too protected against later change.
- Ruling: Narration and OOC prompts share the proposal stable block (no handle, seed-only) — one block for the cap; reviewer checked no id, key, goal or hidden fact reaches the narrator — if wrong, narrator prompt grows by the agreement text.
- Ruling: the double-prefix minor (`pending_pending_x`) goes into fix round 1 — one line in the same area — cost none.
- Ruling: `apply_edit` on a Built draft returns it to Draft and clears campaign_file and last_failure; the first Campaign stays in the list — the shape has no edge for it and blocking edits would trap the Player — if wrong, a re-approved draft builds a second Campaign.
- Ruling: `DraftStore::clear_debug` accepted (shape 3.9: a new run replaces the file).
- Ruling: an empty or too long Source Text returns {DraftSeed, Completeness}; the app message names the limit (Task 8) — no BuildCheck for input problems exists and one more variant is not worth it — if wrong, add `BuildCheck::Input`.
- Ruling: a model timeout on a build call is retried up to 2 times by `run_stage` before it fails as Model (Task 6) — the baseline shows about 10% of calls time out and a build has about 20 calls, so without it most builds would fail — cost: a slow failure when the model is really down (Unreachable still fails at once).
- Ruling: the repaired answer wins when it has no essential problem, even if shorter — simplest rule; fact recall is measured in Task 10 — if recall is low, keep the answer with more items.
- Ruling: slot conflicts among known facts are resolved at draft time for model output (later section wins) and reported as `GapKind::SlotConflict` before approval; `build_world` fails defensively instead of dropping — an approved fact must never vanish without a word — cost: one more gap kind beyond shape 3.5.
- Ruling: delete `build_writes_no_file` (plan-mandated, asserts nothing) — `build_world` takes no path; Task 8 tests the property through the app — if wrong, no test names the "build writes nothing" rule in core.
- Ruling: a persistent timeout (3 attempts) on any build call fails the build as Model — the model is likely down — cost: an optional section can fail a whole build.
- Ruling: strict Agency key list for build facts and effects about the PC — reuses checks::agency as the plan says — cost: non-choice hidden facts about the PC (parentage) are dropped; measure in Task 10.
- Ruling: save, approve and delete are refused for the draft a build runs on — closes an overwrite race — no cost.
- Ruling: add Task 5b "names by schema" before Task 9 — the live run shows References and Kinds failures that the repair call does not fix (start place as a phrase with a period; `age: null` for characters) — fix structurally: the roster stage runs first; every later stage's schema constrains name fields to an enum of roster names; characters and other entities are separate lists so a character's age is a required integer — the Turn proposal already uses handle enums for the same reason — cost: one more task and a reorder of the draft stages.
- Ruling: a text names the PC when it holds the full main name, a full other name, or the first word of the main name — live run showed "Teodor" alone was not counted for "Teodor Brandt" — cost: a first name shared with an NPC inflates the share.
- Ruling: the first word of the PC's main name counts alone only when no other roster name holds that word — a first name shared with an NPC must not fail WorldAutonomy — cost: a title as first word ("Captain ...") still over-counts; the limit is 0.5 and the live share is 3%.
- Ruling: `attitudes[].to` enum = characters, the PC and factions — design 6.3 says attitudes toward named entities, and the first data shape has Faction holders — if wrong, attitudes toward groups are noise.
- Ruling: fix with them two cheap minors that change live quality: the seed prompt lists other names ("also called"); the StartState `place` enum leaves out the start place.
- Ruling: `PC_CENTRED_MAX_SHARE` stays 0.5 — measured 3% average, 5% max; the trap test's data is built around 0.5 — if wrong, a world at 30-50% passes.
- Ruling: `SOURCE_TEXT_MAX_CHARS` 60,000 -> 40,000 — about 9,500 prompt tokens leave about 7,000 for the answer at num_ctx 16,384; not measured at the limit — if wrong, a long chronicle is refused or its roster answer is cut.
- Ruling: minors 2 (form editable during approve), 3 (silent loss on leaving), 6 (gap wording) go into fix round 1 — same files, each one traps or misleads the Player — cost: a slightly larger fix.
- Ruling overturned (justice): "a FactEntry commits as Persistent" -> Persistent only when the PC holds the fact, else Established — hidden plans and feelings must be able to change in play.
- Ruling overturned (justice): timeout retry at 600 s -> BUILD_TIMEOUT_S 240, two retries kept — one stalled call waited 30 minutes.
- Ruling: every character's age needs the Player's confirmation, whatever `age_stated` says; no "confirm all" button — the adults-only limit reads the stored age; the design already says the Player confirms each age — cost: about ten clicks per draft.
- Ruling: the debug file stays written whatever the debug flag says — a failed build must be diagnosable and the Campaign file holds the same content — cost: the hidden world sits in plain text next to the draft.
- Ruling: Source Text reaching play prompts and `intimate` as a model flag are an accepted risk, written in the design (project 2 makes the tag required; a hostile document is the Player's own choice of input).
- Ruling: playbook review step 2 (two-axis code review) skipped — each of the ten tasks had a spec and quality review by a fresh Opus reviewer.
- Ruling: T1 includes `opening` in schema 2 — one schema bump — if wrong, T7 edits the migration.
- Ruling: T2 commits `open` entries as plain facts; T3 adds the pending_ key — if wrong, one test moves.
- Ruling: Task 10 runs before Task 9 — T9 needs the fixture — no cost.
- Ruling: playbook step 3 (cap) skipped — owner decided scope and order on 2026-11-10 — no cost.

## Left open on purpose

- Residual (parked, real, not load-bearing today): the core trusts the `ages_to_confirm` list that `save_draft` sends; a UI bug could confirm an age with no click (none found). A `confirm_age` command with `apply_edit` unable to remove names would make the core the authority. For the owner to decide.
- Residual (minor): lists are cut to their cap silently (30 characters, 30 other entries); `rowNames` de-duplicates with the UI's `nameKey`; the B1 test uses the PC's main name only; drafts saved before f0d13a7 keep their old confirmation lists.
- Follow-up found: the review has no way to add a character the roster stage missed (1 of 5 runs missed two).

## Minor findings deferred by the per-task reviews

The final review triaged these as "fix later" or "drop", except the late cancel, which is fixed.

- Task 1: minor (deferred): alias fact text is fixed English even when story_language is fr (store/mod.rs ~747).
- Task 1: minor (deferred): an alias fact the PC does not hold feeds `secret_terms`, so plans and events that say that other name are dropped (narration_prompt.rs ~130-148).
- Task 1: minor (deferred): RED step was a compile failure only.
- Task 1: minor (deferred): an other name equal to the entity's own main name is not rejected (unconfirmed).
- Task 2: minor (deferred): tests/town.rs temp folder not removed on failure; reuse the TempDir guard of tests/setup.rs.
- Task 2: minor (deferred): no test for a stale `.part` being replaced (setup.rs:172).
- Task 2: minor (deferred): `remove_part` ignores delete errors; a stale `.part` that cannot be deleted is reopened (setup.rs:188).
- Task 3: minor (deferred): scan.rs allowed text lacks themes, gm_agreement, PC other names (false leak counts).
- Task 3: minor (deferred): gm_agreement is trimmed but not passed through one_line (a header-like line could confuse the model).
- Task 3: minor (deferred): a PC other name with empty known_to reaches the narrator.
- Task 3: minor (deferred): clippy --all-targets fails at tests/world.rs:545 (pre-existing).
- Task 4: minor (deferred): `list` fails on the first unreadable draft file (store.rs:533); skip it instead.
- Task 4: minor (deferred): `list` takes the id from file content, not the file name (store.rs:542).
- Task 4: minor (deferred): two saves of one id share `<id>.json.tmp` (store.rs:560); src-tauri must hold a lock when it saves (carry to Task 8).
- Task 4: minor (deferred): PC age 0 passes when min_age is None (draft.rs:166).
- Task 4: minor (deferred): untested branches: known_to names, decided/proposed subjects, duplicate PC other name, empty entity name.
- Task 5: minor (deferred): no test for "first answer wins when the repair breaks an essential part" (stages.rs:889) → carried into Task 6.
- Task 5: minor (deferred): `known_to_pc` adds the PC to every roster other name; an alias only an NPC knows becomes PC knowledge (draft_stages.rs:676).
- Task 5: minor (deferred): `is_heading` ignores indented headings and reads `#` inside code fences as headings (stages.rs:1074).
- Task 5: minor (deferred): repair notes quote model-written names; only a doc comment keeps a World Build note free of hidden text (stages.rs:838) → carried into Task 6.
- Task 6: minor (deferred): `names_pc` with a generic other name inflates the WorldAutonomy share (checks.rs:111).
- Task 6: minor (deferred): event `text` is not checked for PC acts.
- Task 6: minor (deferred): Stage 1 cannot add hidden holders to alias or decided facts (shape 3.3 allows it).
- Task 6: minor (deferred): placement check compares against the raw `start.place`, so a start place named by an other name lets an absent NPC be placed there (checks.rs:215).
- Task 6: minor (deferred): `secrecy_trap` new cases do not prove Secrecy fired (References would drop them too).
- Task 6: minor (deferred): the start place replaces a known `location` of the PC or a present NPC without an error; an open known `location` can sit next to the settled one code writes.
- Task 6: minor (deferred): checks.rs is 807 lines; a split is proposed in the report.
- Task 7: minor (deferred): `correct_last` right after Begin says "reaches back one play Turn only"; "no play Turn to correct" is clearer (turn.rs:257).
- Task 7: minor (deferred): no doc comment on `WorldView::recent_turns` saying it holds Opening Turns (store/mod.rs:181).
- Task 7: minor (deferred): the opening leak test has no positive check that scene() produced output (tests/begin.rs:753).
- Task 8: minor (deferred): a Campaign file write failure reads "a completeness check failed"; a `BuildCheck::Storage` variant would be honest (session.rs:911). Shape change, for the final review to triage.
- Task 8: minor (deferred): no test covers a failure at the Commit stage (commit_failure, Stage::Commit text).
- Task 8: minor (deferred): a cancel during the last model call is lost; the Player gets a Campaign after Cancel (shape §4 allows it).
- Task 8: minor (deferred): session.rs is 968 lines; move `Drafts` to drafts.rs.
- Task 10: minor (deferred): `subject_names` merges names in one pass (world_build.rs:675).
- Task 10: minor (deferred): eval world_build.rs is 1,026 lines; `section` is about 270 lines.
- Task 10: minor (deferred): no metric checks who knows the PC's second name.
- Task 5b: minor (deferred): a main name with edge punctuation in a Campaign file written on this branch before 55c7946 no longer resolves (no such real file exists).
- Task 5b: minor (deferred): the eval crate keeps its own `words` that splits on hyphens.
- Task 5b: minor (deferred): test helpers `node`, `resolve`, `names_at` are duplicated in tests/build.rs and tests/world_build.rs.
- Task 10: minor (deferred): `done` phrase "owns ferry" can match a merged Guild fact (expected.toml:884).
- Task 10: minor (deferred): settled item Odile/"missing" can be matched by an open inquiry fact, so fact recall is not a strict lower bound (expected.toml:858).
- Task 10: minor (deferred): `done = [""]` passes parse and never matches (world_build.rs:72).
- Task 5b: minor (deferred): `names_pc(roster: &[&str])` accepts an empty roster and does not drop the PC's own names from it (checks.rs:109).
- Task 5b: minor (deferred): the NpcState check accepts a place or an item as an attitude target; only the schema enum blocks it (checks.rs:582).
- Task 9: minor (deferred): `failureText` copies the backend's check phrases; a stored failure and a live one read differently. Have the backend send the sentence in DraftView.
- Task 9: minor (deferred): `nameKey` copies `normalize::name`; the "proposed" mark could come from the backend gaps.

## QA pass on the running app (2026-11-10)

QA pass (mat ae1df60e1384b1c8f): ship-readiness 6/10. All four states work on the live model; no hidden content, model text or raw error shown; cancel and retry, restart, rename and Journal probes pass. Majors (UX): an edited age keeps "from the text" and "confirmed"; three unlabelled boxes with internal keys under each known fact; renaming a character does not follow through its references and raises many gaps. Minors: "hello" is accepted as a Source Text; other-names boxes unlabelled and cut; Journal lists imported knowledge as "what the PC saw"; the opening narration put an NPC in the room who was not listed as present; the "Open campaign" block shows under every screen. Not fixed: there is no second fix wave after the final review; the owner decides.
