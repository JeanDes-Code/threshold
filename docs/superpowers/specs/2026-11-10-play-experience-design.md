# Play experience: design

Date: 2026-11-10. Status: draft for review.
Sources: the design conversation of 2026-11-10, the QA report on campaigns from a document (`docs/superpowers/plans/2026-11-10-campaign-from-document-rulings.md`), `GLOSSARY.md`, ADRs 0001 to 0005.
Terms in capitals are defined in `GLOSSARY.md`. Section 10 lists the new terms.

This project lands before the mature content module (`2026-11-10-mature-content-module-design.md`). That module puts its two buttons into the action bar defined here.

## 1. Intent

The owner played the first Turns of an imported Campaign. The flow worked. The screen did not feel like a game: "it is just so simple that it does not feel like I am playing some adventure".

The app today is a set of forms. The owner wants a first version with a real design: a game screen that shows where the PC is, who is there and what is open, and that remembers things for the Player.

On ChatGPT the owner scrolls back most often for three things: plans and open matters, decisions the PC made, and names of people and places. The app must hold these so that the Player does not scroll.

## 2. Goals

1. The Play screen looks and reads like a game: the story, a side panel, an action bar.
2. The Player sees at a glance the place, the time, who is present, and what is open.
3. The Player finds any person or place the PC knows in one click, with what the PC knows about it.
4. The app writes the Journal. The Player can add notes and pin entries.
5. Every screen uses one visual language: home, new Campaign, review, Play, Journal, Character.
6. The review screen speaks in plain sentences and survives a rename.

Done means: the owner plays ten Turns of his own Campaign without scrolling back for a name, a plan or a decision, and the QA role scores the flow 8 of 10 or more with no major finding.

## 3. Non-goals

- Images, portraits or sound made by a model.
- A combat grid, tokens, an inventory screen.
- Skins for other genres.
- A component library or a CSS framework. The UI stays React with plain CSS.
- The two buttons of the mature content module. This project leaves their place in the action bar.
- The Discovery interview and "surprise me" (project 3).

## 4. Anti-goals: it fails even if it technically works when

- a panel shows something the PC does not know: an NPC goal, a secret, a hidden fact, a character the PC cannot perceive;
- a Player note reaches a model prompt or changes the world;
- a spoken line is shown under the name of the wrong character;
- the Turn gets slower because of the new screens: the 15 s budget of ADR 0001 stands;
- the panel shows a number or a state that the core does not hold (an invented fatigue bar, for example);
- the app needs the network: fonts and assets ship with the app.

## 5. The Play screen

The layout is direction B of the mockups the owner chose on 2026-11-10 (kept outside the repository). In words:

- A top bar across the full width: place name on the left, clock on the right, one thin rule under it.
- Under it, two columns. The story column takes the free width. The side panel is a fixed column on the right, about 200 to 280 px, made of stacked cards with small upper-case titles.
- An action bar across the full width at the bottom, above a thin rule: the Intent field takes the free width, then one filled "Act" button, then quiet outline buttons.
- Colours: a cool, very dark blue ground, slightly lighter cards with a thin border, one blue accent for the main button, a warm colour per NPC for the name and the round initial next to a bubble.

### 5.1 Story column

- The Narration of each Turn, in order, with the Player's Intent shown as a short quiet line before it.
- A line spoken by an NPC is shown in a bubble with the NPC's name and a colour that stays the same for that NPC in the whole Campaign.
- When the app cannot tell with certainty who speaks, the line stays in the paragraph. A wrong bubble is worse than no bubble.
- A name of a person or place that the PC knows is a link. A click opens its card (5.3).
- A Roll is shown as a small line under the Turn, as today.
- While the Narration streams, the column follows the text unless the Player scrolled up.

### 5.2 Top bar

The place name and the World Clock in words the Player reads ("Day 3, 17:40"). The Campaign menu holds: home, Correct last Turn, debug mode, model status.

### 5.3 Side panel

Every part reads PC Knowledge and what the PC can perceive now. Nothing else.

- **Here**: each character present whom the PC can perceive, with the name the PC knows them by. A character the PC perceives and cannot name is shown as unknown.
- **You**: the PC's name, concept and the state that the core holds for the PC. The panel shows no gauge that the core does not hold.
- **Open**: the open items the PC knows (ADR 0005): plans, pending matters, Leads. An item leaves the list when the world closes it.
- **Pinned**: Journal entries the Player pinned (section 6).
- **People and places**: a search field and a list of every person, place and Faction the PC knows. A card shows the kind, the other names the PC knows, and each thing the PC knows about it, with open items marked.

On a narrow window the panel folds under a button.

### 5.4 Action bar

- One text field for the Intent, with "Act". Enter sends, Shift+Enter makes a new line.
- "Ask the GM" opens the OOC channel next to the field. An OOC answer is shown apart from the story and is not part of it.
- The bar keeps free space on its right for buttons that later projects add.
- While a Turn runs, the bar shows that the GM decides, then that the GM narrates. The retry of a failed Turn stays, as today.

## 6. The Journal

The app writes it. The Player reads it and adds to it.

- **Timeline**: one entry per Scene. An entry holds the place, the time, who was there, what the PC did (the Intents), and what the PC learned in that Scene.
- **Decisions**: the entries of the timeline that the app marks as a decision of the PC. Section 9 says what the builder must settle here.
- **Open and closed**: the open items the PC knows, and those that closed, with the Scene that closed them.
- **Known before the Campaign**: the PC Knowledge that came from the Seed Draft. It is no longer listed as "what the PC saw".
- **Notes**: text the Player writes, on its own or attached to an entry, a person or a place.
- **Pins**: the Player can pin any entry or note. Pinned items show in the side panel.

Rules:

- A note or a pin is the Player's. It is stored with the Campaign. It never reaches a prompt and never changes a Commit.
- Journal text made by the app comes from committed data and from Narration the Player already read.
- If the builder uses a model call to write a Scene entry, the call runs after the Narration ends, outside the Turn budget. It never blocks the next Intent. A failed call leaves the plain entry.
- A Turn that the Player faded to black (mature content module, 6.6) appears in the Journal by its neutral line only.

## 7. Home, new Campaign and review

### 7.1 Home

The app opens on a home screen: the Player's Campaigns as cards (PC name, premise, last played, Turn count) and the unfinished Seed Drafts. One button starts a new Campaign. The "Open campaign" block no longer sits under every screen.

### 7.2 New Campaign

Same steps as today: paste or load a Source Text, wait for the Seed Draft. A Source Text too short to hold a premise and a PC is refused with a plain sentence. The builder sets the limit.

### 7.3 Review

- A fact is one plain sentence with an "edit" action. The internal fields (subject, key, value) are not shown. Debug mode may show them.
- A rename of a character or place follows in every reference of the draft, in one action.
- The Player can add a character or a place that the draft missed.
- An age that the Player edits is labelled "edited by you". The label "from the text" stays only on an untouched value.
- Each age still needs the Player's confirmation, one by one. The core owns the confirmation: the UI asks the core to confirm one named age, and no longer sends its own list. This closes a residual of the first review.
- Each gap is a sentence that says what is wrong and what to do, next to the field it concerns.
- Lists that the app cut at a cap say so.

## 8. Visual language

- Dark theme by default, as in the chosen mockup: a cool dark ground, one accent colour, a warm colour per NPC.
- Two type families that ship with the app: one for the story, one for the interface.
- One set of tokens (colours, spacing, radius, type sizes) in one file. Every view reads it.
- Keyboard: every action has a focus state and works without a mouse.
- The first slice of the build is the Play screen on fixture data. The owner approves its look before the other screens follow.

## 9. Left to the architect and the builder

- **Who speaks.** How the app learns the speaker of a line: light marks in the Narration, a structured field, or another way. The answer must not raise Turn time and must keep section 4.
- **Decisions.** What marks a Turn as a decision of the PC: a field in the Decision Proposal, a rule on committed data, or the Scene summary. A cheap rule that is right most of the time beats a model call.
- **Scene boundaries** for Journal entries, from the existing Scene definition.
- **The view commands** that give the Play screen its data in one call per Turn.
- **Where notes and pins live** in the Campaign file, and the schema version.
- **When an open item is closed**, for the Open list and the Journal. ADR 0005 names the weak point: the link between a `pending_` key and its outcome key.
- **The name the PC knows a character by**, and how the core tells that the PC perceives a character it cannot name.
- The fonts, the exact palette, the narrow-window breakpoint, the short-text limit.

The core may gain new committed state for any of these, with a new schema version, when a view on the existing ledger is not enough. The architect chooses, and prefers the view.

## 10. New terms for the glossary

- **Note**: text the Player writes in the Journal. Never part of the world.
- **Pin**: the Player's mark that keeps a Journal entry in the side panel.
- **Journal**: the Player's record of the Campaign, written by the app from PC Knowledge. The word is used in the UI today and has no entry.

The domain-modeling step decides whether "open item" gets its own entry or stays under ADR 0005, and how it relates to Lead.

## 11. Acceptance tests

Deterministic, with recorded model responses:

- the Play view data holds no hidden fact, no NPC goal and no character the PC cannot perceive, on the hidden-world fixture;
- a character present whom the PC cannot name is shown as unknown;
- an open item appears under Open, and leaves it in the Turn that closes it;
- a note and a pin survive a restart, and appear in no prompt of the next Turn (checked on the debug log);
- a line with no certain speaker stays in the paragraph;
- a rename in the review updates every reference and raises no new gap;
- a character added in the review passes approval and reaches the World Build;
- an edited age is labelled as edited, and the core refuses approval while one age is not confirmed through the core;
- a Source Text under the limit is refused;
- the Turn time on recorded responses does not grow.

By the QA role, on the live model: the full flow from home to ten Turns, with screenshots, scored against section 2.

## 12. Calls made by Claude Code, open to the owner's veto

- "Correct last Turn" and debug mode move into the Campaign menu.
- Home screen with Campaigns as cards.
- Notes and pins never reach a prompt.
- No invented gauges: the "You" part shows only the state the core holds. The fatigue bar of the mockup was an example.
- Plain CSS with tokens, no component library.
- A wrong speaker bubble is treated as a failure; an unsure line stays plain text.
- This project runs before the mature content module.
- The core-owned age confirmation moves into this project.
- Campaign files made before this project are not migrated when the schema changes. The owner imports his Campaign again. The Journal of a new Campaign is built from its first Turn.
- An OOC answer lives for the session only. It is not saved and is not in the Journal.
- A character or place that the Player adds in the review is the Player's word, like any edit of the Seed Draft. Its age follows the same rule as the others: the Player confirms it through the core.
- *Bug pass 3 (2026-11-11).* The eight NPC colours spread over the hue circle, at least 30 degrees apart and away from the blue accent: two warm tones that QA saw as one (#d9a46b, #d6b07a) gave way. This drops "warm" from 5 and 8.
- *Bug pass 3.* Only the last Turn's name links are in the tab order; older ones stay links for the mouse. Before, the keyboard walked every name of the whole story before it reached the side panel.
- *Bug pass 3.* The Open card shows 5 items and a "Show all" button, so Pinned stays in view. The Intent field clears when the Intent is sent, and keeps what the Player types while the Turn runs. "Ask the GM" puts the focus in its field. `*word*` in a Narration reads as emphasis; any other asterisk is dropped.

## 13. Decisions from the owner

- Direction B for the Play screen (2026-11-10).
- The panel must hold plans and quests, the PC's decisions, and names of people and places (2026-11-10).
- A Journal, with the app doing most of the work for the Player (2026-11-10).
- The revamp and the mature content module both land before the owner's real test (2026-11-10).
