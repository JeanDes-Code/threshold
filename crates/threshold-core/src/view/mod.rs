//! The screen types and the functions that make them.
//!
//! Every function here reads `PcSight` only, never the world, so a view
//! cannot show a hidden fact: its input does not hold one.

use serde::Serialize;

use crate::{
    domain::{
        ClockMin, CloseOutcome, EntityId, EntityKind, STATE_KEYS, is_open_key, is_state_key,
        outcome_key, slot_sentence, you_sentence,
    },
    normalize,
    player::{Marks, Note, Target},
    rules::RollResult,
    store::{TurnId, TurnKind},
    world::{Belief, Moment, OpenItem, PcSight, SightLine, SightTurn},
};

/// The budget of one `play_view` call at 300 Turns, in a release build,
/// the reading of the Campaign included (data shape 3.6).
pub const PLAY_VIEW_MAX_MS: u64 = 50;

/// The Player's sentence for a Refusal at Detail Level `excluded`.
pub const REFUSAL_EXCLUDED: &str = "This Campaign excludes Intimate Scenes.";

/// The Player's sentence for a Refusal by the age rule. It names nobody: a
/// hidden character or a minor may be the one who blocks, and the sentence
/// is the same in every case.
pub const REFUSAL_ADULTS_ONLY: &str = "An Intimate Scene cannot happen here.";

/// The Player's sentence for a Refusal by the stance rule.
pub const REFUSAL_UNWILLING: &str = "Nobody here goes along with this.";

/// The one fixed sentence of a Refusal (data shape 5). No name, no id and
/// no age: the shell shows it as it is, in debug mode too.
pub fn refusal_text(kind: crate::mature::RefusalKind) -> &'static str {
    match kind {
        crate::mature::RefusalKind::Excluded => REFUSAL_EXCLUDED,
        crate::mature::RefusalKind::AdultsOnly => REFUSAL_ADULTS_ONLY,
        crate::mature::RefusalKind::Unwilling => REFUSAL_UNWILLING,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CharacterView {
    pub name: String,
    pub concept: String,
    pub clock_min: u64,
    /// What the PC knows about itself: no open item and no outcome of one
    /// (bug pass 3).
    pub facts: Vec<CharacterFact>,
    /// The Open Items the PC knows that are not closed: the side panel's
    /// and the Journal's list (`open_lines`), about any subject.
    pub open: Vec<FactLine>,
    /// The state lines of the Play screen's "You" card (`YouCard::state`):
    /// the Character screen shows them too (changed after review).
    pub state: Vec<StateLine>,
}

/// One thing the PC knows about itself.
#[derive(Clone, Debug, Serialize)]
pub struct CharacterFact {
    pub text: String,
    /// Who told it, as the Journal's lines say (review of bug pass 4, m5);
    /// `None` for what the PC saw, and for the PC's own Claim.
    pub told_by: Option<NameRef>,
}

/// The PC as the PC knows itself: its own Beliefs about itself, without an
/// open item or the outcome of one, and the Open Items of the side panel.
pub fn character_view(sight: &PcSight) -> CharacterView {
    let facts = sight
        .beliefs
        .iter()
        .filter(|b| b.subject == sight.pc && !is_open_key(&b.key) && closed_by(sight, b).is_none())
        .map(|b| CharacterFact {
            // "You are at The Copper Kettle.", not "Teodor Brandt is at
            // The Copper Kettle." (bug pass 5).
            text: pc_state_text(sight, b).unwrap_or_else(|| fact_text(&b.text, &b.key)),
            told_by: teller(sight, b),
        })
        .collect();
    CharacterView {
        name: sight.pc_name.clone(),
        concept: sight.concept.clone(),
        clock_min: sight.clock_min,
        facts,
        open: open_lines(sight),
        state: state_lines(sight),
    }
}

/// The PC's Beliefs about itself on `STATE_KEYS`, `location` left out, in
/// that order. The "You" card and the Character screen share them.
fn state_lines(sight: &PcSight) -> Vec<StateLine> {
    STATE_KEYS
        .iter()
        .filter(|key| **key != "location")
        .filter_map(|key| {
            sight
                .beliefs
                .iter()
                .find(|b| b.subject == sight.pc && b.key == *key)
        })
        .map(|b| StateLine {
            key: b.key.clone(),
            value: b.value.clone(),
            // The PC's own state, in the second person (bug pass 4).
            text: you_sentence(&b.key, &b.value),
        })
        .collect()
}

/// The Play screen: the story, and the side panel as the PC knows it now.
/// It mirrors `PlayView` of `ui/src/api.ts` (data shape 3.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlayView {
    pub pc_name: String,
    pub place: Option<PlaceLine>,
    pub clock_min: ClockMin,
    pub needs_begin: bool,
    pub can_correct: bool,
    pub scene: SceneState,
    pub story: Vec<StoryTurn>,
    /// The characters present now, the PC left out.
    pub here: Vec<NameRef>,
    pub you: YouCard,
    /// The Open Items the PC knows that are not closed, those about an NPC
    /// present first, then the newest (`open_lines`): the UI shows the
    /// first ones as they come.
    pub open: Vec<FactLine>,
    pub pinned: Vec<PinLine>,
    /// Each named NPC, place and Faction the PC knows.
    pub known: Vec<KnownLine>,
}

/// An entity as the PC knows it. `name` is `None` when the PC cannot name
/// it: the UI shows "unknown".
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NameRef {
    pub entity_id: EntityId,
    pub kind: EntityKind,
    pub name: Option<String>,
}

/// The PC's place: the place entity the PC can name, if one has the
/// location's name, and that entity's name; else the location text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlaceLine {
    pub entity_id: Option<EntityId>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StoryTurn {
    pub turn_id: TurnId,
    pub kind: TurnKind,
    pub intent: String,
    pub roll: Option<RollResult>,
    /// A later Correction reverted this Turn. Its body holds prose only.
    pub reverted: bool,
    pub body: TurnBody,
}

/// The mature content module adds a `Faded` variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnBody {
    Narration {
        blocks: Vec<Block>,
    },
    /// The Turn is committed and its Narration failed.
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Block {
    Prose {
        parts: Vec<Part>,
    },
    /// A line spoken by a present NPC whom the PC can name; the words
    /// without the quote marks, and the stage note before them as a quiet
    /// line, without its parentheses (review minor).
    Speech {
        speaker: NameRef,
        parts: Vec<Part>,
        stage: Option<Vec<Part>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Part {
    Text {
        text: String,
    },
    /// A name the PC knows, as the text spells it.
    Name {
        entity_id: EntityId,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct YouCard {
    pub name: String,
    pub concept: String,
    pub state: Vec<StateLine>,
}

/// A PC Belief about the PC on a key of `STATE_KEYS`, never `location`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StateLine {
    pub key: String,
    pub value: String,
    /// The line as the Player reads it (`domain::slot_sentence`), never
    /// `key value` as data (bug pass 3).
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FactLine {
    pub belief: BeliefRef,
    pub subject: NameRef,
    pub text: String,
    /// An Open Item: planned or pending, not done.
    pub open: bool,
    pub told_by: Option<NameRef>,
}

/// A token for one Belief. The UI does not show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BeliefRef {
    pub subject_id: EntityId,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct KnownLine {
    pub who: NameRef,
    pub other_names: Vec<String>,
    pub here: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PinLine {
    pub target: Target,
    pub title: String,
    pub text: String,
}

/// The scene the Play screen is in (data shape 3.11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneState {
    Plain,
    /// An Intimate Scene is open, since the Turn that opened it.
    Intimate {
        since_turn: TurnId,
    },
}

/// The Play screen of `sight`. `marks` holds the Player's Notes and Pins.
pub fn play_view(sight: &PcSight, marks: &Marks) -> PlayView {
    let links = Links::new(sight);
    PlayView {
        pc_name: sight.pc_name.clone(),
        place: sight.place.as_ref().map(|(entity_id, text)| PlaceLine {
            entity_id: *entity_id,
            name: entity_id
                .and_then(|id| name_ref(sight, id)?.name)
                .unwrap_or_else(|| text.clone()),
        }),
        clock_min: sight.clock_min,
        needs_begin: sight.needs_begin,
        can_correct: sight.can_correct,
        scene: sight
            .scene
            .map_or(SceneState::Plain, |since_turn| SceneState::Intimate {
                since_turn,
            }),
        story: sight
            .story
            .iter()
            .map(|turn| story_turn(sight, &links, turn))
            .collect(),
        here: sight
            .present
            .iter()
            .filter_map(|id| name_ref(sight, *id))
            .filter(|who| who.kind == EntityKind::Npc)
            .collect(),
        you: YouCard {
            name: sight.pc_name.clone(),
            concept: sight.concept.clone(),
            state: state_lines(sight),
        },
        open: open_lines(sight),
        pinned: pin_lines(sight, marks),
        known: sight
            .known
            .iter()
            .filter(|k| {
                k.name.is_some()
                    && matches!(
                        k.kind,
                        EntityKind::Npc | EntityKind::Place | EntityKind::Faction
                    )
            })
            .filter_map(|k| {
                Some(KnownLine {
                    who: name_ref(sight, k.id)?,
                    other_names: k.other_names.clone(),
                    here: sight.present.contains(&k.id),
                })
            })
            .collect(),
    }
}

/// The Journal: the app's record of the Campaign, from PC Knowledge only
/// (data shape 3.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JournalView {
    /// One entry per Scene, oldest first.
    pub scenes: Vec<SceneEntry>,
    /// The Open Items the PC knows that are not closed.
    pub open: Vec<FactLine>,
    pub closed: Vec<ClosedItem>,
    /// "Known before the Campaign": PC Knowledge from the Setup Turns.
    pub known_before: Vec<FactLine>,
    pub notes: Vec<Note>,
    pub pins: Vec<Target>,
}

/// One Scene: one place and time, who was there, what the PC did and what
/// the PC learned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SceneEntry {
    /// The first Turn of the Scene.
    pub turn_id: TurnId,
    /// The name of the place, or the PC's location text; empty when the PC
    /// has no location.
    pub place: String,
    /// The World Clock after the first Turn of the Scene.
    pub clock_from: ClockMin,
    /// The World Clock after its last Turn.
    pub clock_to: ClockMin,
    /// Each entity present in one of its Turns, the PC left out.
    pub present: Vec<NameRef>,
    /// One per Play and Correction Turn. The Opening has no Intent.
    pub acts: Vec<Act>,
    pub learned: Vec<FactLine>,
}

/// What the PC did in one Turn: its Intent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Act {
    pub turn_id: TurnId,
    pub intent: String,
    /// A decision of the PC (data shape 3.2).
    pub decision: bool,
    /// The Open Items this Turn closed, each with how it ended (review
    /// M3): the Decisions read "Refused · <item text>".
    pub closed: Vec<ClosedLine>,
}

/// How one Open Item ended, as the Journal shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClosedLine {
    /// The outcome word: "Accepted", "Refused", "Done", "Failed",
    /// "Dropped", or the value a public event wrote ("Raised").
    pub outcome: String,
    /// The item's text.
    pub text: String,
}

/// An Open Item that closed, how it ended, and the Turn and the Scene
/// that closed it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClosedItem {
    pub item: FactLine,
    pub closed_in: TurnId,
    /// The outcome word (`ClosedLine::outcome`, review M3).
    pub outcome: String,
    /// The first Turn of the Scene that holds `closed_in`, when that Turn
    /// stands.
    pub scene: Option<TurnId>,
}

/// The Journal of `sight`. No model writes it: each entry comes from
/// committed data and from Narration the Player already read.
pub fn journal_view(sight: &PcSight, marks: &Marks) -> JournalView {
    JournalView {
        scenes: sight
            .scenes()
            .into_iter()
            .filter_map(|scene| scene_entry(sight, scene))
            .collect(),
        open: open_lines(sight),
        closed: sight
            .open_items
            .iter()
            .filter_map(|item| {
                let closed_in = item.closed_in?;
                let mut line = item_line(sight, item)?;
                line.open = false;
                Some(ClosedItem {
                    item: line,
                    closed_in,
                    outcome: outcome_word(item)?,
                    scene: sight
                        .scenes()
                        .into_iter()
                        .find(|scene| scene.iter().any(|m| m.turn == closed_in))
                        .and_then(|scene| scene.first())
                        .map(|m| m.turn),
                })
            })
            .collect(),
        known_before: sight
            .before
            .iter()
            .filter_map(|b| fact_line(sight, b))
            .collect(),
        notes: notes_seen(sight, marks),
        pins: pins_seen(sight, marks),
    }
}

fn scene_entry(sight: &PcSight, scene: &[Moment]) -> Option<SceneEntry> {
    let (first, last) = (scene.first()?, scene.last()?);
    let mut present: Vec<NameRef> = Vec::new();
    for id in scene.iter().flat_map(|m| &m.present) {
        if *id != sight.pc && present.iter().all(|who| who.entity_id != *id) {
            present.extend(name_ref(sight, *id));
        }
    }
    Some(SceneEntry {
        turn_id: first.turn,
        place: first
            .place
            .as_deref()
            .map(|text| place_name(sight, text))
            .unwrap_or_default(),
        clock_from: first.clock_after,
        clock_to: last.clock_after,
        present,
        acts: scene
            .iter()
            .filter(|m| m.kind.has_intent())
            .filter_map(|m| {
                let turn = sight.story.iter().find(|t| t.turn == m.turn)?;
                Some(Act {
                    turn_id: m.turn,
                    intent: turn.intent.clone(),
                    decision: m.decision,
                    closed: sight
                        .open_items
                        .iter()
                        .filter(|item| item.closed_in == Some(m.turn))
                        .filter_map(|item| {
                            Some(ClosedLine {
                                outcome: outcome_word(item)?,
                                text: item_line(sight, item)?.text,
                            })
                        })
                        .collect(),
                })
            })
            .collect(),
        learned: scene
            .iter()
            .flat_map(|m| &m.learned)
            .filter(|b| learned_line(sight, b))
            .filter_map(|b| fact_line(sight, b))
            .collect(),
    })
}

/// True when `belief` belongs under "What you learned" (bug pass 4): not
/// the PC's own place, which the Scene's title already gives, and not the
/// outcome line the core built for a `settles` ("Refused: …"), which the
/// Decisions and the closed list show as "Refused · …". Anything else on
/// an item's outcome key is learned (review minor): news the PC is told,
/// or an outcome a public event wrote in front of the PC.
fn learned_line(sight: &PcSight, belief: &Belief) -> bool {
    let own_place = belief.subject == sight.pc && belief.key == "location";
    !own_place && !settle_line(sight, belief)
}

/// The Open Item that `belief` closed for the PC, if it is the Belief that
/// closed one (`OpenItem::outcome`).
fn closed_by<'a>(sight: &'a PcSight, belief: &Belief) -> Option<&'a OpenItem> {
    sight
        .open_items
        .iter()
        .find(|item| item.outcome.as_ref() == Some(belief))
}

/// True when `belief` is the outcome line the core built for a `settles`:
/// it closed an item, and its value and text are the core's template
/// (`CloseOutcome::text`).
fn settle_line(sight: &PcSight, belief: &Belief) -> bool {
    closed_by(sight, belief).is_some_and(|item| {
        CloseOutcome::from_word(&belief.value).is_some_and(|o| o.text(&item.text) == belief.text)
    })
}

/// How a closed item ended, as one word the Player reads (review M3): the
/// `settles` outcome ("Refused"), or the value a public event wrote, in words
/// ("Raised"). `None` while the item is open.
fn outcome_word(item: &OpenItem) -> Option<String> {
    let value = &item.outcome.as_ref()?.value;
    if let Some(outcome) = CloseOutcome::from_word(value) {
        return Some(outcome.head().to_owned());
    }
    let words = value.trim().replace('_', " ");
    let mut chars = words.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// A location as the PC names it: the name of a place the PC can name, when
/// one has that name, else the location text.
fn place_name(sight: &PcSight, text: &str) -> String {
    let wanted = normalize::name(text);
    sight
        .known
        .iter()
        .filter(|k| k.kind == EntityKind::Place)
        .filter_map(|k| k.name.as_ref())
        .find(|name| normalize::name(name) == wanted)
        .cloned()
        .unwrap_or_else(|| text.to_owned())
}

/// The Open Items the PC knows that are not closed: those about an NPC
/// present now first, then the newest (`OpenItem::learned`).
fn open_lines(sight: &PcSight) -> Vec<FactLine> {
    let mut items: Vec<&OpenItem> = sight
        .open_items
        .iter()
        .filter(|item| item.closed_in.is_none())
        .collect();
    // The items about someone here first, then the newest (review minor:
    // the side panel shows the first 5, and they came by subject id).
    let here = |item: &OpenItem| item.subject != sight.pc && sight.present.contains(&item.subject);
    items.sort_by_key(|item| (!here(item), std::cmp::Reverse(item.learned)));
    items
        .into_iter()
        .filter_map(|item| item_line(sight, item))
        .collect()
}

/// True when `belief` is the open line of an Open Item that closed: the
/// item's outcome line stands for it (bug pass 4, MC). The side panel, the
/// Journal and the cards read open and closed from `PcSight::open_items`.
fn closed_item(sight: &PcSight, belief: &Belief) -> bool {
    is_open_key(&belief.key)
        && sight.open_items.iter().any(|item| {
            item.subject == belief.subject && item.key == belief.key && item.closed_in.is_some()
        })
}

/// The line of an Open Item: the PC Belief on its open key.
fn item_line(sight: &PcSight, item: &OpenItem) -> Option<FactLine> {
    sight
        .beliefs
        .iter()
        .find(|b| b.subject == item.subject && b.key == item.key)
        .and_then(|b| fact_line(sight, b))
}

/// One person, place or Faction as the PC knows it (data shape 3.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntityCard {
    pub who: NameRef,
    /// The other names the PC holds an `alias_` Belief for.
    pub other_names: Vec<String>,
    /// Present now, as the PC perceives it.
    pub here: bool,
    /// The PC Beliefs about it, Open Items marked.
    pub facts: Vec<FactLine>,
    /// A place or a Faction only: the PC Beliefs about other subjects whose
    /// text names it, by a name the PC knows for it (bug pass 3). Empty for
    /// any other entity.
    pub named_in: Vec<FactLine>,
    /// A place only: the characters the PC believes are located there, from
    /// the PC's `location` Beliefs (QA, 2026-11-10). One the PC cannot
    /// name has no name. Empty for any other entity.
    pub people_here: Vec<NameRef>,
    /// A character the PC saw leave for a place the PC cannot name: "Left;
    /// you do not know where." (`DEPARTED_TEXT`). The departure is not
    /// among `facts` (review of bug pass 4, m4). `None` otherwise.
    pub whereabouts: Option<String>,
    /// The Player's Notes about it.
    pub notes: Vec<Note>,
    pub pinned: bool,
}

/// The card of `entity`, or `None` when the PC does not know it: it is not
/// in `PcSight.known`. A hidden entity and an id that does not exist look
/// the same here. A character the PC cannot name gets a card with no text:
/// its only Belief is a witnessed `location`, and that text holds its name.
pub fn entity_card(sight: &PcSight, marks: &Marks, entity: EntityId) -> Option<EntityCard> {
    let known = sight.known.iter().find(|k| k.id == entity)?;
    let who = name_ref(sight, entity)?;
    let named = known.name.is_some();
    let target = Target::Entity { entity_id: entity };
    let character = matches!(known.kind, EntityKind::Npc | EntityKind::Pc);
    Some(EntityCard {
        whereabouts: if named && character && entity != sight.pc {
            whereabouts(sight, entity)
        } else {
            None
        },
        other_names: if named {
            known.other_names.clone()
        } else {
            Vec::new()
        },
        here: sight.present.contains(&entity),
        facts: sight
            .beliefs
            .iter()
            .filter(|b| named && b.subject == entity && !closed_item(sight, b))
            // A character's place is its "Where" line, not a fact.
            .filter(|b| !(character && entity != sight.pc && b.key == "location"))
            .filter_map(|b| fact_line(sight, b))
            .collect(),
        people_here: if named && known.kind == EntityKind::Place {
            people_at(sight, known)
        } else {
            Vec::new()
        },
        named_in: if named && matches!(known.kind, EntityKind::Place | EntityKind::Faction) {
            named_in(sight, known)
        } else {
            Vec::new()
        },
        notes: notes_seen(sight, marks)
            .into_iter()
            .filter(|n| n.about.as_ref() == Some(&target))
            .collect(),
        pinned: pins_seen(sight, marks).contains(&target),
        who,
    })
}

/// The PC Beliefs about another subject whose text names `entity` by its
/// main name or an other name the PC knows, as whole words with the name's
/// own capitals (`normalize::name_in`, review B1). In Belief order.
fn named_in(sight: &PcSight, entity: &crate::world::Known) -> Vec<FactLine> {
    let names: Vec<Vec<String>> = entity
        .name
        .iter()
        .chain(&entity.other_names)
        .map(|n| normalize::name_pattern(n))
        .filter(|words| !words.is_empty())
        .collect();
    sight
        .beliefs
        .iter()
        .filter(|b| b.subject != entity.id && !closed_item(sight, b))
        .filter(|b| {
            let text = normalize::cased_words(&b.text);
            names.iter().any(|name| normalize::name_in(&text, name))
        })
        .filter_map(|b| fact_line(sight, b))
        .collect()
}

/// The characters the PC believes are at `place`: a location Belief whose
/// text is a name the PC knows for the place, through `normalize::name`,
/// the rule the PC's own place follows. In id order.
fn people_at(sight: &PcSight, place: &crate::world::Known) -> Vec<NameRef> {
    let names: Vec<String> = place
        .name
        .iter()
        .chain(&place.other_names)
        .map(|n| normalize::name(n))
        .collect();
    let mut ids: Vec<EntityId> = sight
        .whereabouts
        .iter()
        .filter(|(_, at)| names.contains(&normalize::name(at)))
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| name_ref(sight, id))
        .collect()
}

/// Where `target` points now, or `None` when it no longer resolves (data
/// shape 3.7). A Scene or an Act of a reverted Turn follows the Correction
/// that replaced it; a Scene resolves to its first Turn. A Belief resolves
/// while the PC still holds it, an entity while the PC knows it, and a Note
/// while it exists. The result is the target in its stored form.
pub fn resolve_target(sight: &PcSight, marks: &Marks, target: &Target) -> Option<Target> {
    match target.normalized() {
        Target::Scene { turn_id } => {
            let turn = follow(sight, turn_id);
            let scene = sight
                .scenes()
                .into_iter()
                .find(|scene| scene.iter().any(|m| m.turn == turn))?;
            Some(Target::Scene {
                turn_id: scene.first()?.turn,
            })
        }
        Target::Act { turn_id } => {
            let turn = follow(sight, turn_id);
            sight
                .moments
                .iter()
                .any(|m| m.turn == turn && m.kind.has_intent())
                .then_some(Target::Act { turn_id: turn })
        }
        Target::Belief { subject_id, key } => sight
            .beliefs
            .iter()
            .any(|b| b.subject == subject_id && normalize::key(&b.key) == key)
            .then_some(Target::Belief { subject_id, key }),
        Target::Entity { entity_id } => sight
            .known
            .iter()
            .any(|k| k.id == entity_id)
            .then_some(Target::Entity { entity_id }),
        Target::Note { note_id } => marks
            .notes
            .iter()
            .any(|n| n.id == note_id)
            .then_some(Target::Note { note_id }),
    }
}

/// The stored Pins that show as `target`: the target itself, and each Pin
/// that resolves to it. Removing the Pin of `target` removes all of them.
pub fn pins_resolving_to(sight: &PcSight, marks: &Marks, target: &Target) -> Vec<Target> {
    let target = target.normalized();
    let resolved = resolve_target(sight, marks, &target);
    marks
        .pins
        .iter()
        .filter(|pin| {
            pin.normalized() == target
                || resolved.is_some() && resolve_target(sight, marks, pin) == resolved
        })
        .cloned()
        .collect()
}

/// The Turn that stands for `turn`: the Correction Turn that replaced it,
/// through each later Correction.
fn follow(sight: &PcSight, mut turn: TurnId) -> TurnId {
    for _ in 0..=sight.replaced_by.len() {
        match sight.replaced_by.get(&turn) {
            Some(next) => turn = *next,
            None => break,
        }
    }
    turn
}

/// The Pins whose target still resolves, resolved, each once, in the order
/// the Player set them.
fn pins_seen(sight: &PcSight, marks: &Marks) -> Vec<Target> {
    let mut seen: Vec<Target> = Vec::new();
    for target in marks
        .pins
        .iter()
        .filter_map(|pin| resolve_target(sight, marks, pin))
    {
        if !seen.contains(&target) {
            seen.push(target);
        }
    }
    seen
}

/// Every Note, `about` resolved. A Note whose target no longer resolves
/// shows as a free Note; its stored target is kept.
fn notes_seen(sight: &PcSight, marks: &Marks) -> Vec<Note> {
    marks
        .notes
        .iter()
        .map(|note| Note {
            about: note
                .about
                .as_ref()
                .and_then(|about| resolve_target(sight, marks, about)),
            ..note.clone()
        })
        .collect()
}

/// The Pinned card: one line per Pin that still resolves.
fn pin_lines(sight: &PcSight, marks: &Marks) -> Vec<PinLine> {
    pins_seen(sight, marks)
        .into_iter()
        .filter_map(|target| {
            let (title, text) = pin_words(sight, marks, &target)?;
            Some(PinLine {
                target,
                title,
                text,
            })
        })
        .collect()
}

/// The title and the text of a resolved Pin.
fn pin_words(sight: &PcSight, marks: &Marks, target: &Target) -> Option<(String, String)> {
    let intent = |turn: TurnId| {
        sight
            .story
            .iter()
            .find(|t| t.turn == turn)
            .map(|t| t.intent.clone())
    };
    let scene_place = |turn: TurnId| {
        let scene = sight
            .scenes()
            .into_iter()
            .find(|scene| scene.iter().any(|m| m.turn == turn))?;
        Some(place_title(sight, scene.first()?.place.as_deref()))
    };
    match target {
        Target::Scene { turn_id } => {
            let scene = sight
                .scenes()
                .into_iter()
                .find(|scene| scene.first().is_some_and(|m| m.turn == *turn_id))?;
            let first_act = scene
                .iter()
                .filter(|m| m.kind.has_intent())
                .find_map(|m| intent(m.turn))
                .unwrap_or_default();
            Some((scene_place(*turn_id)?, first_act))
        }
        Target::Act { turn_id } => Some((intent(*turn_id)?, scene_place(*turn_id)?)),
        Target::Belief { subject_id, key } => {
            let belief = sight
                .beliefs
                .iter()
                .find(|b| b.subject == *subject_id && normalize::key(&b.key) == *key)?;
            let line = fact_line(sight, belief)?;
            Some((
                line.subject.name.unwrap_or_else(|| "unknown".into()),
                line.text,
            ))
        }
        Target::Entity { entity_id } => {
            let who = name_ref(sight, *entity_id)?;
            let kind = match who.kind {
                EntityKind::Pc => "You",
                EntityKind::Npc => "A person",
                EntityKind::Place => "A place",
                EntityKind::Faction => "A faction",
                EntityKind::Item => "A thing",
            };
            Some((
                who.name.unwrap_or_else(|| "unknown".into()),
                kind.to_owned(),
            ))
        }
        Target::Note { note_id } => {
            let note = marks.notes.iter().find(|n| n.id == *note_id)?;
            Some(("Your note".to_owned(), note.body.clone()))
        }
    }
}

/// A Scene's place as a title: the place name, or "Somewhere".
fn place_title(sight: &PcSight, place: Option<&str>) -> String {
    match place.map(|text| place_name(sight, text)) {
        Some(name) if !name.trim().is_empty() => name,
        _ => "Somewhere".to_owned(),
    }
}

fn story_turn(sight: &PcSight, links: &Links, turn: &SightTurn) -> StoryTurn {
    let body = match turn.narration {
        None => TurnBody::Missing,
        Some(_) => TurnBody::Narration {
            blocks: turn
                .lines
                .iter()
                .map(|line| match line {
                    SightLine::Speech {
                        speaker,
                        note,
                        words,
                    } => match name_ref(sight, *speaker) {
                        Some(speaker) => Block::Speech {
                            speaker,
                            parts: links.parts(words),
                            stage: note.as_deref().map(|note| links.parts(note)),
                        },
                        None => Block::Prose {
                            parts: links.parts(words),
                        },
                    },
                    SightLine::Prose(text) => Block::Prose {
                        parts: links.parts(text),
                    },
                })
                .collect(),
        },
    };
    StoryTurn {
        turn_id: turn.turn,
        kind: turn.kind,
        intent: turn.intent.clone(),
        roll: turn.roll,
        reverted: turn.reverted,
        body,
    }
}

/// `id` as the PC knows it; `None` when it is not in `PcSight.known`.
fn name_ref(sight: &PcSight, id: EntityId) -> Option<NameRef> {
    sight.known.iter().find(|k| k.id == id).map(|k| NameRef {
        entity_id: k.id,
        kind: k.kind,
        name: k.name.clone(),
    })
}

/// The card's `whereabouts` of a character the PC saw leave for a place it
/// cannot name (review of bug pass 4, m4).
pub const DEPARTED_TEXT: &str = "Left; you do not know where.";

/// True when `belief` is a departure record (`world::with_witnessed_events`):
/// the `location` of another character, with no place.
fn departure(sight: &PcSight, belief: &Belief) -> bool {
    belief.subject != sight.pc && belief.key == "location" && belief.value.trim().is_empty()
}

fn fact_line(sight: &PcSight, belief: &Belief) -> Option<FactLine> {
    let mut text = fact_text(&belief.text, &belief.key);
    // A departure reads in the past, whatever its stored text: a record
    // written before review m4 says "Orsel leaves.".
    if departure(sight, belief) {
        let name = name_ref(sight, belief.subject)
            .and_then(|who| who.name)
            .unwrap_or_else(|| "Someone".into());
        text = format!("{name} left.");
    }
    // A state of the PC that a Turn wrote reads in the second person; a
    // sentence the Adventure Seed wrote stays as it is (bug pass 4).
    if let Some(you) = pc_state_text(sight, belief) {
        text = you;
    }
    Some(FactLine {
        belief: BeliefRef {
            subject_id: belief.subject,
            key: belief.key.clone(),
        },
        subject: name_ref(sight, belief.subject)?,
        text,
        open: is_open_key(&belief.key),
        told_by: teller(sight, belief),
    })
}

/// A state of the PC that a Turn wrote, in the second person: "You are at
/// The Copper Kettle." (`domain::you_sentence`). `None` for any other
/// Belief, and for a sentence the Adventure Seed wrote. The text matches
/// the core's sentence whatever its case: the slot value is stored in lower
/// case, the text keeps the place's capitals (bug pass 5: the Character tab
/// read "Teodor Brandt is at The Copper Kettle.").
fn pc_state_text(sight: &PcSight, belief: &Belief) -> Option<String> {
    let core = slot_sentence(&sight.pc_name, &belief.key, &belief.value);
    let ours = belief.subject == sight.pc
        && is_state_key(&belief.key)
        && !is_open_key(&belief.key)
        && fact_text(&belief.text, &belief.key).to_lowercase() == core.to_lowercase();
    ours.then(|| {
        if belief.key == "location" && !belief.value.trim().is_empty() {
            format!("You are at {}.", place_name(sight, &belief.value))
        } else {
            you_sentence(&belief.key, &belief.value)
        }
    })
}

/// Where a character is, as a card's "Where" line: "At The Copper Kettle."
/// from the PC's `location` Belief, or the departure text. QA (bug pass 5)
/// read "Sefa Lind is at The Copper Kettle." among the card's facts.
fn whereabouts(sight: &PcSight, entity: EntityId) -> Option<String> {
    let belief = sight
        .beliefs
        .iter()
        .find(|b| b.subject == entity && b.key == "location")?;
    if departure(sight, belief) {
        return Some(DEPARTED_TEXT.to_owned());
    }
    Some(format!("At {}.", place_name(sight, &belief.value)))
}

/// Who told `belief`: the speaker of a Claim, as the PC knows them. The
/// PC's own Claim (a `settles` outcome, review M2) has no teller.
fn teller(sight: &PcSight, belief: &Belief) -> Option<NameRef> {
    belief
        .source
        .filter(|id| *id != sight.pc)
        .and_then(|id| name_ref(sight, id))
}

/// The line of a fact. A play fact `X: key = value` (the text a Turn wrote
/// before bug pass 3) reads as a plain sentence (`domain::slot_sentence`),
/// the `pending_` prefix left out. Any other text stays.
fn fact_text(text: &str, key: &str) -> String {
    let parts = text
        .split_once(": ")
        .and_then(|(who, rest)| Some((who, rest.split_once(" = ")?)));
    match parts {
        Some((who, (k, value))) if k == key => {
            let key = outcome_key(key).unwrap_or(key);
            slot_sentence(who, key, value)
        }
        _ => text.to_owned(),
    }
}

/// The names the story links: the main name of each named NPC, place and
/// Faction the PC knows, as whole words with the name's own capitals
/// (`normalize::name_in`, review B1: "gate square" is no link to the place
/// Gate). A name two of them share links to neither.
struct Links {
    /// The words of each name (`normalize::name_pattern`), longest first,
    /// and its entity.
    names: Vec<(Vec<String>, EntityId)>,
}

impl Links {
    fn new(sight: &PcSight) -> Self {
        let mut names: Vec<(Vec<String>, EntityId)> = sight
            .known
            .iter()
            .filter(|k| {
                matches!(
                    k.kind,
                    EntityKind::Npc | EntityKind::Place | EntityKind::Faction
                )
            })
            .filter_map(|k| {
                let words = normalize::name_pattern(k.name.as_deref()?);
                (!words.is_empty()).then_some((words, k.id))
            })
            .collect();
        let folded =
            |words: &[String]| -> Vec<String> { words.iter().map(|w| w.to_lowercase()).collect() };
        let shared: Vec<Vec<String>> = names
            .iter()
            .filter(|(words, id)| {
                names
                    .iter()
                    .any(|(w, other)| folded(w) == folded(words) && other != id)
            })
            .map(|(words, _)| folded(words))
            .collect();
        names.retain(|(words, _)| !shared.contains(&folded(words)));
        names.sort_by_key(|(words, _)| std::cmp::Reverse(words.len()));
        Self { names }
    }

    /// `text` cut into plain text and name links. The parts give back
    /// `text` unchanged.
    fn parts(&self, text: &str) -> Vec<Part> {
        let words = word_spans(text);
        let folded: Vec<String> = words
            .iter()
            .map(|&(start, end)| normalize::cased_words(&text[start..end]).concat())
            .collect();
        let mut parts = Vec::new();
        let mut written = 0;
        let mut i = 0;
        while i < words.len() {
            let found = self.names.iter().find(|(name, _)| {
                folded
                    .get(i..i + name.len())
                    .is_some_and(|run| normalize::name_in(run, name))
            });
            let Some((name, entity_id)) = found else {
                i += 1;
                continue;
            };
            let (start, end) = (words[i].0, words[i + name.len() - 1].1);
            if start > written {
                parts.push(Part::Text {
                    text: text[written..start].to_owned(),
                });
            }
            parts.push(Part::Name {
                entity_id: *entity_id,
                text: text[start..end].to_owned(),
            });
            written = end;
            i += name.len();
        }
        if written < text.len() || parts.is_empty() {
            parts.push(Part::Text {
                text: text[written..].to_owned(),
            });
        }
        parts
    }
}

/// The byte spans of the words of `text`: runs of letters, digits and
/// combining accents, as `normalize::cased_words` splits them.
fn word_spans(text: &str) -> Vec<(usize, usize)> {
    let is_word = |c: char| c.is_alphanumeric() || ('\u{300}'..='\u{36f}').contains(&c);
    let mut spans = Vec::new();
    let mut start = None;
    for (at, c) in text.char_indices() {
        match (is_word(c), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                spans.push((from, at));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        spans.push((from, text.len()));
    }
    spans
}

/// How many of the latest standing Turns the OOC sheet shows.
pub const OOC_RECENT_TURNS: usize = 4;

/// The World Clock in the Player's words, as the top bar shows it
/// (`clockText` of `ui/src/api.ts`): "Day 1, 08:05".
pub fn clock_text(minutes: ClockMin) -> String {
    format!(
        "Day {}, {:02}:{:02}",
        minutes / 1440 + 1,
        (minutes % 1440) / 60,
        minutes % 60
    )
}

/// The heading of the OOC sheet's block of open items: the words a Player
/// uses for them (bug pass 5, M2).
pub const OOC_OPEN_HEADING: &str =
    "REQUESTS, OFFERS, PLANS AND PROMISES STILL OPEN (not answered or not done yet)";

/// The heading of the OOC sheet's block of answered items.
pub const OOC_ANSWERED_HEADING: &str = "REQUESTS, OFFERS, PLANS AND PROMISES ALREADY ANSWERED (what you accepted, refused, did, failed or gave up)";

/// What the GM reads to answer an OOC question (bug pass 4, MB): the Play
/// screen's facts, as text. An OOC answer is a Player screen, so it is made
/// from `PcSight` only, like every view. QA asked "Who is in the room, what
/// time is it?" and got "You do not know what time it is" and "Ludo Brenn is
/// not mentioned as being in the common room", while the screen showed the
/// clock and Ludo under Here: the old prompt held PC Knowledge only, with
/// no clock, no presence, no Turn, and each closed item twice, once open.
///
/// The blocks: NOW (place, clock, who is here by the names the PC knows),
/// the items still open (`open_lines`), the items already answered (each
/// with its outcome as a sentence about "you"), RECENT TURNS (the Intent and
/// the Narration as the Player read it), then WHAT YOU KNOW (the other
/// Beliefs).
///
/// Bug pass 5 (M2): after two refusals QA asked "Which requests have I
/// answered and how?" and got "You have not answered any requests.": the
/// block was "SETTLED ITEMS" with lines "Refused: <item>". The headings now
/// use the Player's words (requests, offers, plans, promises; open,
/// answered), a settled line reads "You refused: <item>", and an empty
/// block says so.
pub fn ooc_sheet(sight: &PcSight) -> String {
    let mut out = String::from("NOW\n");
    // "You" is the PC: the items and facts below name the PC by name.
    out.push_str(&format!("You are {}.\n", one_line(&sight.pc_name)));
    let place = sight.place.as_ref().map(|(entity_id, text)| {
        entity_id
            .and_then(|id| name_ref(sight, id)?.name)
            .unwrap_or_else(|| text.clone())
    });
    out.push_str(&format!(
        "Place: {}\n",
        place.as_deref().unwrap_or("unknown")
    ));
    out.push_str(&format!("Time: {}\n", clock_text(sight.clock_min)));
    let here: Vec<String> = sight
        .present
        .iter()
        .filter(|id| **id != sight.pc)
        .filter_map(|id| name_ref(sight, *id))
        .map(|who| who.name.unwrap_or_else(|| "someone you cannot name".into()))
        .collect();
    if here.is_empty() {
        out.push_str("Here with you: nobody\n");
    } else {
        out.push_str(&format!("Here with you: {}\n", here.join(", ")));
    }
    // Every person the PC can name, so a question about one who is away
    // finds them (bug pass 5, M2: "What do I know about Tobin?" got "You do
    // not know anything about Tobin" while an open request named him).
    let away: Vec<String> = sight
        .known
        .iter()
        .filter(|k| k.kind == EntityKind::Npc && !sight.present.contains(&k.id))
        .filter_map(|k| k.name.clone())
        .collect();
    if !away.is_empty() {
        out.push_str(&format!(
            "People you know of who are not here: {}\n",
            away.join(", ")
        ));
    }

    let open = open_lines(sight);
    out.push_str(OOC_OPEN_HEADING);
    out.push('\n');
    if open.is_empty() {
        out.push_str("- None.\n");
    }
    // The marker of every prompt line of an open item (ADR 0005).
    for line in &open {
        out.push_str(&format!("- (open, not done) {}\n", ooc_line(line)));
    }
    // Each closed item once, as a sentence: "You refused: <item>".
    let settled: Vec<String> = sight
        .open_items
        .iter()
        .filter(|item| item.closed_in.is_some())
        .filter_map(|item| {
            let key = outcome_key(&item.key)?;
            let outcome = sight
                .beliefs
                .iter()
                .find(|b| b.subject == item.subject && b.key == key)?;
            let text = match CloseOutcome::from_word(&outcome.value)
                .filter(|_| settle_line(sight, outcome))
            {
                Some(close) => format!("{}: {}", close.you(), one_line(&item.text)),
                None => ooc_line(&fact_line(sight, outcome)?),
            };
            Some(format!("- {text}\n"))
        })
        .collect();
    out.push_str(OOC_ANSWERED_HEADING);
    out.push('\n');
    if settled.is_empty() {
        out.push_str("- None yet: you have answered no request, offer, plan or promise.\n");
    }
    out.push_str(&settled.concat());

    let turns: Vec<&SightTurn> = sight
        .story
        .iter()
        .filter(|t| !t.reverted && t.narration.is_some())
        .collect();
    let recent = &turns[turns.len().saturating_sub(OOC_RECENT_TURNS)..];
    if !recent.is_empty() {
        out.push_str("RECENT TURNS (oldest first)\n");
        for turn in recent {
            // A Turn committed with `intimate` is its Neutral Line, in place
            // of its Intent and its Narration (data shape 3.6).
            if let Some(line) = &turn.line {
                out.push_str(&format!("{}\n", one_line(line)));
                continue;
            }
            if turn.kind.has_intent() {
                out.push_str(&format!("Player: {}\n", one_line(&turn.intent)));
            }
            let text: Vec<String> = turn
                .lines
                .iter()
                .map(|line| match line {
                    SightLine::Prose(text) => one_line(text),
                    SightLine::Speech {
                        speaker,
                        note,
                        words,
                    } => {
                        let name = name_ref(sight, *speaker)
                            .and_then(|who| who.name)
                            .unwrap_or_else(|| "Someone".into());
                        let note = note
                            .as_deref()
                            .map(|n| format!("({}) ", one_line(n)))
                            .unwrap_or_default();
                        format!("{name}: {note}\"{}\"", one_line(words))
                    }
                })
                .filter(|line| !line.is_empty())
                .collect();
            out.push_str(&format!("Narration: {}\n", text.join(" ")));
        }
    }

    // Everything else the PC holds: no open key and no outcome that closed
    // an item, which the item blocks above already show. News told on the
    // outcome key of an item that stays open is here (review minor).
    let item_keys: Vec<(EntityId, String)> = sight
        .open_items
        .iter()
        .map(|item| (item.subject, item.key.clone()))
        .collect();
    let known: Vec<String> = sight
        .beliefs
        .iter()
        .filter(|b| !item_keys.contains(&(b.subject, b.key.clone())))
        .filter(|b| closed_by(sight, b).is_none())
        .filter_map(|b| {
            let line = fact_line(sight, b)?;
            // A departure says that the PC does not know where (m4).
            Some(if departure(sight, b) {
                format!(
                    "- {}; you do not know where.\n",
                    line.text.trim_end_matches('.')
                )
            } else {
                format!("- {}\n", ooc_line(&line))
            })
        })
        .collect();
    if !known.is_empty() {
        out.push_str("WHAT YOU KNOW\n");
        out.push_str(&known.concat());
    }
    out
}

/// One fact line of the OOC sheet: its text, and who told it.
fn ooc_line(line: &FactLine) -> String {
    let text = one_line(&line.text);
    match line.told_by.as_ref().and_then(|who| who.name.as_deref()) {
        Some(name) => format!("{text} (told by {name})"),
        None => text,
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
