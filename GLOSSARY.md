# Threshold

Threshold is a local, single-player tabletop roleplaying game. A language model plays the Game Master inside a persistent world. The world keeps its own state and rules, and the model does not own them.

## Campaign and setup

**Campaign**:
One persistent game: its Adventure Seed, its world, its PC and its full history.
_Avoid_: Game, save, story

**Adventure Seed**:
The editable agreement that frames a Campaign: premise, PC concept, rules, content boundaries and GM-style preferences. Each choice is marked decided, proposed or open.
_Avoid_: Charter, settings, config, session zero doc, "seed" alone (a Roll seed is something else)

**Discovery**:
The interview that produces a Adventure Seed, conducted one question at a time.
_Avoid_: Character creation, onboarding, setup wizard

**Source Text**:
A document the Player gives to start a Campaign from a defined idea, such as the chronicle of a game played elsewhere.
_Avoid_: Import, save file, transcript

**Seed Draft**:
A Adventure Seed before the Player approves it, with the list of characters and the PC Knowledge taken from a Source Text.
_Avoid_: Proposal (a Decision Proposal is something else), preview

**World Build**:
The step that creates the hidden part of the world from an approved Adventure Seed: NPC goals, secrets, Voice Cards and World Events. The Player does not read its result.
_Avoid_: Generation, worldgen, import

**Content Boundary**:
A Adventure Seed rule that limits or allows a kind of content, such as sexual or violent content, or names a specific exclusion.
_Avoid_: Filter, safety setting

**Detail Level**:
The Adventure Seed setting for intimate content. Excluded: no Intimate Scene. Implied: the Narration tells that it happens, with no explicit description. Explicit: full description, which needs a Content Pack.
_Avoid_: Adult mode, NSFW level, rating

**Age Confirmation**:
The Player's statement, recorded by the core, that a character's stored age is right. Only a character with one can be in an Intimate Scene.
_Avoid_: Age check, verification

## Participants

**Player**:
The human at the keyboard.
_Avoid_: User

**PC**:
The character the Player controls. Only the Player decides the PC's voluntary actions, words, motives and feelings.
_Avoid_: Hero, protagonist, main character

**GM**:
The role the model plays: adjudicates, portrays NPCs, narrates. The GM is a role, not a model; one model can fill it or several.
_Avoid_: AI, assistant, narrator (for the whole role)

**NPC**:
A person in the world who is not the PC, with goals, limits and loyalties of their own.
_Avoid_: Character (ambiguous with PC), actor, agent

**Voice Card**:
The saved description of how one NPC speaks: habits, vocabulary and a few sample lines. It is written once and reused whenever the NPC is in a Scene.
_Avoid_: Persona, personality prompt, character sheet

**Faction**:
An organization or group that pursues goals in the world as a unit.
_Avoid_: Organization, guild, team

## Play

**Scene**:
A continuous stretch of play at one place and time, with a known set of participants.
_Avoid_: Chapter, encounter

**Intimate Scene**:
A Scene of sexual or erotic content between adults, from the first Turn tagged intimate to its end. A Turn is tagged intimate when its Intent or its outcome involves sexual or erotic content between any characters, whether the PC takes part or only watches.
_Avoid_: Sex scene, NSFW scene, adult scene

**Intimacy Tag**:
The value each Decision Proposal gives its Turn, in the field `intimacy`: "intimate" when THIS Turn's intent or its outcome involves sexual or erotic content between any characters, whether the PC takes part or only watches; "advance" when it is a romantic or sexual advance or invitation between characters, such as a kiss or asking someone to spend the night, and no such content happens in this Turn, whatever the answer; else "none". The core can raise it from the Intent's words (a Romance Word), never lower it. The age rule runs for "advance" and "intimate"; only "intimate" opens an Intimate Scene.
_Avoid_: Flag, rating, NSFW tag

**Advance**:
A Turn whose Intimacy Tag is "advance": a romantic or sexual invitation, without the act. The age rule runs for it, with the same final Refusal. It is told as a plain Turn, opens no Intimate Scene, and an NPC may accept or decline it in the story.
_Avoid_: Flirt mode, pre-scene

**Romance Word**:
A neutral word or phrase of the core's short list (`mature::ROMANCE_WORDS`), such as "kiss" or "spend the night with". When the Player's Intent holds one, the Turn is at least an Advance, whatever the model says.
_Avoid_: Bad word, filter list, blacklist

**Fade to Black**:
The Player's choice to skip the description of a Turn or of an Intimate Scene. The world keeps what happened, and the Player reads one neutral line.
_Avoid_: Skip, censor, cut

**Neutral Line**:
The fixed sentence the core writes in place of a Narration that the Player skipped, or that a later prompt may not read. No model writes it.
_Avoid_: Summary, placeholder

**Turn**:
One cycle from a Player Intent to the Narration of its committed result. A Closing Turn has no Intent.
_Avoid_: Message, round, exchange

**Closing Turn**:
The Turn that ends an Intimate Scene when the Player presses "End scene" or "Fade to black". It has no Intent.
_Avoid_: Epilogue, cleanup

**Intent**:
What the Player says the PC tries to do, in the Player's own words.
_Avoid_: Command, prompt, move

**OOC**:
Communication between the Player and the GM outside the fiction: clarifications, rule questions, Corrections, and settings the Player changes during play.
_Avoid_: Meta, system message

**Correction**:
An OOC request to fix a continuity error or a misread Intent. It changes the record openly and does not pretend the original outcome never happened.
_Avoid_: Undo, retcon, edit

**Refusal**:
A Turn that the core stops for a Content Boundary, with no retry and no Commit. The Player reads one fixed sentence. A Refusal is not a failed Turn: the Play screen shows the sentence as a quiet line with no Retry, and the Intent goes back to the field. The same operation sent again gets the same Refusal with no model call; a new Intent is judged afresh.
_Avoid_: Block, censorship, rejection (a rejection gets a retry)

**Decision Proposal**:
The GM's structured interpretation of an Intent: what kind of action it is, whether it needs a Check, the Stakes and Factors, NPC reactions and the time it takes. It is not canon until committed.
_Avoid_: Plan, tool call, response

**Commit**:
The act that makes a validated change part of the world's state and history.
_Avoid_: Save, apply, write

**Narration**:
The prose the Player reads. It describes only committed outcomes that the PC can perceive.
_Avoid_: Response, output, story text

**Shortcut**:
A GM move that makes the next line easy to write at the cost of the world's consistency. Four kinds: a convenient coincidence, an NPC who folds without cause, an established fact that bends, a tension that evaporates.
_Avoid_: Plot hole, cheat, railroading

**Content Pack**:
A removable set of data that extends what the GM may portray: prompt fragments, extra Adventure Seed options, model hints. It holds no code and cannot override a Content Boundary. A pack may declare that it gives explicit description; such a pack holds scene fragments only.
_Avoid_: Mod, module, plugin, DLC

**Journal**:
The Player's record of the Campaign. The app writes it from PC Knowledge and from Narration the Player has read.
_Avoid_: Log, history, quest log

**Note**:
Text the Player writes in the Journal. It is never part of the world and never reaches a prompt.
_Avoid_: Memo, comment

**Pin**:
The Player's mark that keeps a Journal entry or a Note in view during play.
_Avoid_: Favourite, bookmark

**Open Item**:
A plan or pending matter that is not done yet, stored as an Established fact on a pending key (ADR 0005). A Lead is something the PC could pursue; an Open Item is something expected to happen or to be done.
_Avoid_: Quest, task, todo

## Resolution

**Ruleset**:
The replaceable set of game rules that decides Checks, outcomes and the PC's sheet.
_Avoid_: System, engine, mechanics

**Check**:
A rules resolution for an uncertain action with real Stakes.
_Avoid_: Test, skill check, saving throw

**Stakes**:
What the PC can gain or lose on a Check.
_Avoid_: Risk, danger level

**Factor**:
An established fact of the fiction that makes a Check easier or harder.
_Avoid_: Modifier, bonus

**Roll**:
The seeded random result a Check consumes. It can be reproduced from its seed.
_Avoid_: Dice result, RNG

**Consequence**:
A committed change that follows from a Check or an action: injury, loss, reputation, relationship.
_Avoid_: Effect, outcome (when meaning the change itself)

## World

**World Clock**:
The fictional time of the Campaign, which moves in discrete steps.
_Avoid_: Timer, game time, real time

**World Event**:
Something that happens in the world at a fictional time, whether the PC is present or not. It can be scheduled, triggered, resolved or canceled.
_Avoid_: Plot point, trigger, quest event

**Public Event**:
A World Event whose changes everyone at its place sees or hears, such as an announcement or a public act. A PC who witnesses it learns what it changes. The World Build and the GM declare it; the event's text never decides it.
_Avoid_: Announcement (for the flag), visible event

**Offscreen**:
Happening where the PC cannot perceive it.
_Avoid_: Background, hidden (for events)

**Lead**:
Something the PC knows about that they could pursue. Not every story is a Lead.
_Avoid_: Quest, mission, hook

## Knowledge and canon

**Canon Level**:
How firmly a detail belongs to the world: Possible, Established or Persistent.
_Avoid_: Truth level, certainty

**Possible**:
A plausible detail that nobody has committed. It is not canon and may never be.
_Avoid_: Draft, idea, rumor (a rumor can be Established)

**Established**:
A detail that became part of the world through a witnessed event, a Check, or a committed GM decision.
_Avoid_: Confirmed, real

**Persistent**:
An Established detail that is protected. Only a causal World Event or a Correction can change it. Changing state, such as where someone is or how hurt they are, is never Persistent.
_Avoid_: Locked, permanent, frozen

**World Truth**:
What is actually so in the world, including secrets and Offscreen facts.
_Avoid_: Ground truth, reality, GM state

**Claim**:
A statement someone makes, with its source and certainty. A Claim can be Established as said while its content is false.
_Avoid_: Fact (when the content may be false), rumor (as the general term)

**Belief**:
What one NPC or Faction holds to be true. It may be wrong.
_Avoid_: Memory, knowledge (for NPCs)

**PC Knowledge**:
What the PC has observed, been told or inferred. The Player's view of the world shows this and nothing more.
_Avoid_: Player knowledge, known facts, discovered facts

**Visibility**:
Who may perceive a detail: the GM only, specific NPCs, or the PC.
_Avoid_: Permission, access, privacy

**Provenance**:
The source and time a detail entered the world: which Commit, which event, which Claim.
_Avoid_: Origin, history
