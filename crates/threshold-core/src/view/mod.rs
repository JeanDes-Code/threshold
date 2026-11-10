//! The screen types and the functions that make them.
//!
//! Every function here reads `PcSight` only, never the world, so a view
//! cannot show a hidden fact: its input does not hold one.

use serde::Serialize;

use crate::{
    domain::{ClockMin, EntityId, EntityKind, Knowing, STATE_KEYS, is_open_key},
    normalize,
    player::{Marks, Target},
    rules::RollResult,
    store::{TurnId, TurnKind},
    world::{Belief, PcSight, SightLine, SightTurn},
};

/// The budget of one `play_view` call at 300 Turns, in a release build,
/// the reading of the Campaign included (data shape 3.6).
pub const PLAY_VIEW_MAX_MS: u64 = 50;

#[derive(Clone, Debug, Serialize)]
pub struct CharacterView {
    pub name: String,
    pub concept: String,
    pub clock_min: u64,
    pub facts: Vec<CharacterFact>,
}

/// One thing the PC knows about itself.
#[derive(Clone, Debug, Serialize)]
pub struct CharacterFact {
    pub text: String,
    /// An open item (ADR 0005): planned or pending, not done.
    pub open: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct JournalEntry {
    pub subject: String,
    pub text: String,
    pub source: Option<String>,
    /// An open item (ADR 0005): planned or pending, not done.
    pub open: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct JournalView {
    pub observations: Vec<JournalEntry>,
    pub claims: Vec<JournalEntry>,
}

/// The PC as the PC knows itself: only its own Beliefs about itself.
pub fn character_view(sight: &PcSight) -> CharacterView {
    let facts = sight
        .beliefs
        .iter()
        .filter(|b| b.subject == sight.pc)
        .map(|b| CharacterFact {
            open: is_open_key(&b.key),
            text: b.text.clone(),
        })
        .collect();
    CharacterView {
        name: sight.pc_name.clone(),
        concept: sight.concept.clone(),
        clock_min: sight.clock_min,
        facts,
    }
}

/// PC Knowledge, split: what the PC witnessed, and what it was told. A
/// subject or a speaker the PC cannot name reads "someone".
pub fn legacy_journal(sight: &PcSight) -> JournalView {
    let name = |id: EntityId| {
        sight
            .known
            .iter()
            .find(|k| k.id == id)
            .and_then(|k| k.name.clone())
            .unwrap_or_else(|| "someone".to_owned())
    };
    let mut journal = JournalView {
        observations: Vec::new(),
        claims: Vec::new(),
    };
    for belief in &sight.beliefs {
        let entry = JournalEntry {
            subject: name(belief.subject),
            text: belief.text.clone(),
            source: belief.source.map(name),
            open: is_open_key(&belief.key),
        };
        match belief.how {
            Knowing::Witnessed => journal.observations.push(entry),
            Knowing::Told => journal.claims.push(entry),
        }
    }
    journal
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
    /// The Open Items the PC knows that are not closed.
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
            state: STATE_KEYS
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
                })
                .collect(),
        },
        open: sight
            .open_items
            .iter()
            .filter(|item| item.closed_in.is_none())
            .filter_map(|item| {
                sight
                    .beliefs
                    .iter()
                    .find(|b| b.subject == item.subject && b.key == item.key)
            })
            .filter_map(|b| fact_line(sight, b))
            .collect(),
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

/// The Pins whose target the PC can still see. No Pin resolves yet: the
/// rules that follow a Pin through a Correction come with the Notes and
/// Pins commands, and until then the shell passes no Pin.
fn pin_lines(_sight: &PcSight, _marks: &Marks) -> Vec<PinLine> {
    Vec::new()
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
    Some(FactLine {
        belief: BeliefRef {
            subject_id: belief.subject,
            key: belief.key.clone(),
        },
        subject: name_ref(sight, belief.subject)?,
        text: fact_text(&belief.text, &belief.key),
        open: is_open_key(&belief.key),
        told_by: belief.source.and_then(|id| name_ref(sight, id)),
    })
}

/// The line of a fact: a play fact `X: key = value` (the text a Turn
/// writes) reads as `X key words: value`. Any other text stays.
fn fact_text(text: &str, key: &str) -> String {
    let parts = text
        .split_once(": ")
        .and_then(|(who, rest)| Some((who, rest.split_once(" = ")?)));
    match parts {
        Some((who, (k, value))) if k == key => {
            format!("{who} {}: {value}", key.replace('_', " "))
        }
        _ => text.to_owned(),
    }
}

/// The names the story links: the main name of each named NPC, place and
/// Faction the PC knows, as whole words after `normalize::name`. A name two
/// of them share links to neither.
struct Links {
    /// The words of each name, longest first, and its entity.
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
                let words = normalize::name_words(&normalize::name(k.name.as_deref()?));
                (!words.is_empty()).then_some((words, k.id))
            })
            .collect();
        let shared: Vec<Vec<String>> = names
            .iter()
            .filter(|(words, id)| names.iter().any(|(w, other)| w == words && other != id))
            .map(|(words, _)| words.clone())
            .collect();
        names.retain(|(words, _)| !shared.contains(words));
        names.sort_by_key(|(words, _)| std::cmp::Reverse(words.len()));
        Self { names }
    }

    /// `text` cut into plain text and name links. The parts give back
    /// `text` unchanged.
    fn parts(&self, text: &str) -> Vec<Part> {
        let words = word_spans(text);
        let folded: Vec<String> = words
            .iter()
            .map(|&(start, end)| normalize::name_words(&text[start..end]).concat())
            .collect();
        let mut parts = Vec::new();
        let mut written = 0;
        let mut i = 0;
        while i < words.len() {
            let found = self.names.iter().find(|(name, _)| {
                folded
                    .get(i..i + name.len())
                    .is_some_and(|run| run == name.as_slice())
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
/// combining accents, as `normalize::name_words` splits them.
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
