//! The screen types and the functions that make them.
//!
//! Every function here reads `PcSight` only, never the world, so a view
//! cannot show a hidden fact: its input does not hold one.

use serde::Serialize;

use crate::{
    domain::{
        ClockMin, EntityId, EntityKind, STATE_KEYS, is_open_key, is_state_key, outcome_key,
        slot_sentence, you_sentence,
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
}

/// The PC as the PC knows itself: its own Beliefs about itself, without an
/// open item or the outcome of one, and the Open Items of the side panel.
pub fn character_view(sight: &PcSight) -> CharacterView {
    let outcome = |key: &str| {
        sight
            .open_items
            .iter()
            .any(|i| i.subject == sight.pc && outcome_key(&i.key).is_some_and(|k| k == key))
    };
    let facts = sight
        .beliefs
        .iter()
        .filter(|b| b.subject == sight.pc && !is_open_key(&b.key) && !outcome(&b.key))
        .map(|b| CharacterFact {
            text: fact_text(&b.text, &b.key),
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
    /// without the quote marks.
    Speech {
        speaker: NameRef,
        parts: Vec<Part>,
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

/// The mature content module adds an `Intimate` variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneState {
    Plain,
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
        scene: SceneState::Plain,
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
}

/// An Open Item that closed, and the Turn that closed it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClosedItem {
    pub item: FactLine,
    pub closed_in: TurnId,
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
            .filter(|m| m.kind != TurnKind::Opening)
            .filter_map(|m| {
                let turn = sight.story.iter().find(|t| t.turn == m.turn)?;
                Some(Act {
                    turn_id: m.turn,
                    intent: turn.intent.clone(),
                    decision: m.decision,
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
/// outcome of an Open Item, which the Decisions and the Open and closed
/// list show ("Accepted: …").
fn learned_line(sight: &PcSight, belief: &Belief) -> bool {
    let own_place = belief.subject == sight.pc && belief.key == "location";
    let outcome = sight.open_items.iter().any(|item| {
        item.subject == belief.subject && outcome_key(&item.key) == Some(belief.key.as_str())
    });
    !own_place && !outcome
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
    Some(EntityCard {
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
                .any(|m| m.turn == turn && m.kind != TurnKind::Opening)
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
                .filter(|m| m.kind != TurnKind::Opening)
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
                    SightLine::Speech { speaker, words } => match name_ref(sight, *speaker) {
                        Some(speaker) => Block::Speech {
                            speaker,
                            parts: links.parts(words),
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

fn fact_line(sight: &PcSight, belief: &Belief) -> Option<FactLine> {
    let mut text = fact_text(&belief.text, &belief.key);
    // A state of the PC that a Turn wrote reads in the second person; a
    // sentence the Adventure Seed wrote stays as it is (bug pass 4).
    if belief.subject == sight.pc
        && is_state_key(&belief.key)
        && !is_open_key(&belief.key)
        && text == slot_sentence(&sight.pc_name, &belief.key, &belief.value)
    {
        text = you_sentence(&belief.key, &belief.value);
    }
    Some(FactLine {
        belief: BeliefRef {
            subject_id: belief.subject,
            key: belief.key.clone(),
        },
        subject: name_ref(sight, belief.subject)?,
        text,
        open: is_open_key(&belief.key),
        // The PC's own Claim (a `settles` outcome, review M2) has no teller.
        told_by: belief
            .source
            .filter(|id| *id != sight.pc)
            .and_then(|id| name_ref(sight, id)),
    })
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

/// What the GM reads to answer an OOC question (bug pass 4, MB): the Play
/// screen's facts, as text. An OOC answer is a Player screen, so it is made
/// from `PcSight` only, like every view. QA asked "Who is in the room, what
/// time is it?" and got "You do not know what time it is" and "Ludo Brenn is
/// not mentioned as being in the common room", while the screen showed the
/// clock and Ludo under Here: the old prompt held PC Knowledge only, with
/// no clock, no presence, no Turn, and each closed item twice, once open.
///
/// The blocks: NOW (place, clock, who is here by the names the PC knows),
/// OPEN ITEMS (`open_lines`), SETTLED ITEMS (each closed item with its
/// outcome line), RECENT TURNS (the Intent and the Narration as the Player
/// read it), then WHAT THE PC KNOWS (the other Beliefs).
pub fn ooc_sheet(sight: &PcSight) -> String {
    let mut out = String::from("NOW\n");
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
        .map(|who| {
            who.name
                .unwrap_or_else(|| "someone the PC cannot name".into())
        })
        .collect();
    if here.is_empty() {
        out.push_str("Here with the PC: nobody\n");
    } else {
        out.push_str(&format!("Here with the PC: {}\n", here.join(", ")));
    }

    let open = open_lines(sight);
    if !open.is_empty() {
        out.push_str("OPEN ITEMS (not done yet)\n");
        // The marker of every prompt line of an open item (ADR 0005).
        for line in &open {
            out.push_str(&format!("- (open, not done) {}\n", ooc_line(line)));
        }
    }
    // Each closed item once, as its outcome line: "Refused: <item>".
    let settled: Vec<(&OpenItem, Belief)> = sight
        .open_items
        .iter()
        .filter(|item| item.closed_in.is_some())
        .filter_map(|item| {
            let key = outcome_key(&item.key)?;
            let outcome = sight
                .beliefs
                .iter()
                .find(|b| b.subject == item.subject && b.key == key)?;
            Some((item, outcome.clone()))
        })
        .collect();
    if !settled.is_empty() {
        out.push_str("SETTLED ITEMS (done, refused or given up)\n");
        for (_, outcome) in &settled {
            if let Some(line) = fact_line(sight, outcome) {
                out.push_str(&format!("- {}\n", ooc_line(&line)));
            }
        }
    }

    let turns: Vec<&SightTurn> = sight
        .story
        .iter()
        .filter(|t| !t.reverted && t.narration.is_some())
        .collect();
    let recent = &turns[turns.len().saturating_sub(OOC_RECENT_TURNS)..];
    if !recent.is_empty() {
        out.push_str("RECENT TURNS (oldest first)\n");
        for turn in recent {
            if turn.kind != TurnKind::Opening {
                out.push_str(&format!("Player: {}\n", one_line(&turn.intent)));
            }
            let text: Vec<String> = turn
                .lines
                .iter()
                .map(|line| match line {
                    SightLine::Prose(text) => one_line(text),
                    SightLine::Speech { speaker, words } => {
                        let name = name_ref(sight, *speaker)
                            .and_then(|who| who.name)
                            .unwrap_or_else(|| "Someone".into());
                        format!("{name}: \"{}\"", one_line(words))
                    }
                })
                .filter(|line| !line.is_empty())
                .collect();
            out.push_str(&format!("Narration: {}\n", text.join(" ")));
        }
    }

    // Everything else the PC holds: no open key and no outcome line, which
    // the item blocks above already show.
    let item_keys: Vec<(EntityId, String)> = sight
        .open_items
        .iter()
        .flat_map(|item| {
            let outcome = outcome_key(&item.key).map(str::to_owned);
            [Some(item.key.clone()), outcome]
                .into_iter()
                .flatten()
                .map(move |key| (item.subject, key))
        })
        .collect();
    let known: Vec<String> = sight
        .beliefs
        .iter()
        .filter(|b| !item_keys.contains(&(b.subject, b.key.clone())))
        .filter_map(|b| fact_line(sight, b))
        .map(|line| format!("- {}\n", ooc_line(&line)))
        .collect();
    if !known.is_empty() {
        out.push_str("WHAT THE PC KNOWS\n");
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
