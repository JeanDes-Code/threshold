//! Who is in the room: the one home of presence (QA run 3, B1).
//!
//! Every reader of "who is at the PC's place" reads `room`, or one of the
//! three sets it is made of:
//! - `truth`: world truth, hidden characters included
//!   (`WorldView::present`, the Commit checks);
//! - `seen`: who the PC knows is here (`context::seen_present`). HERE, Ask
//!   the GM, a character card's "Where" and the narration context read
//!   this set through `PcSight::present` or `seen_present`;
//! - `unsure`: each character the world state does not place away from the
//!   PC's place. The PC never reads this set: it is world truth, and a
//!   Player screen made from it would show a hidden fact.
//!
//! The age gate reads `Room::checked`, the union of the three sets and of
//! everyone the PC believes may be here. So HERE is a subset of what the
//! gate checks, and any doubt about a place goes to the gate as "here":
//! the gate fails closed.
//!
//! Two place rules live here. `at_place` is the rule of play: the same
//! `normalize::value`. `may_be_at` is the rule of doubt, for `unsure`
//! only: near names (`near_place`), and a place that holds the other one
//! (`holds`).

use std::collections::{BTreeSet, HashSet};

use crate::{
    domain::{EntityId, EntityKind},
    normalize,
    store::{Entity, WorldView},
    world::pc_knowledge,
};

/// Who is at the PC's place now. See the module documentation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Room {
    pub pc: Option<EntityId>,
    /// The PC's live `location` value, as stored.
    pub place: Option<String>,
    pub truth: BTreeSet<EntityId>,
    pub seen: BTreeSet<EntityId>,
    pub unsure: BTreeSet<EntityId>,
}

impl Room {
    /// The set the age gate checks: `truth`, `seen` and `unsure`, and the
    /// PC. Things and places can be in it; the gate checks the characters.
    pub fn checked(&self) -> BTreeSet<EntityId> {
        let mut all = self.truth.clone();
        all.extend(&self.seen);
        all.extend(&self.unsure);
        all.extend(self.pc);
        all
    }
}

/// The room of the PC now.
pub fn room(view: &WorldView) -> Room {
    let pc = pc_of(view);
    let place = pc
        .and_then(|pc| view.live_fact(pc, "location"))
        .map(|f| f.slot.value.clone());
    Room {
        pc,
        truth: truth(view).into_iter().collect(),
        seen: seen(view).into_iter().collect(),
        unsure: unsure(view, place.as_deref()),
        place,
    }
}

/// World truth at the PC's place, hidden characters included: the PC, and
/// each entity whose live `location` is the PC's by `at_place`. In the
/// order of `entities`. Empty when the campaign has no PC.
pub fn truth(view: &WorldView) -> Vec<EntityId> {
    let Some(pc) = pc_of(view) else {
        return Vec::new();
    };
    let place = view.live_fact(pc, "location").map(|f| f.slot.value.clone());
    view.entities
        .iter()
        .filter(|e| {
            e.id == pc
                || place.as_deref().is_some_and(|place| {
                    view.live_fact(e.id, "location")
                        .is_some_and(|f| at_place(&f.slot.value, place))
                })
        })
        .map(|e| e.id)
        .collect()
}

/// Who the PC knows is here: the PC, plus each entity whose `location`, as
/// the PC believes it (`pc_knowledge`), is the PC's live location by
/// `at_place`. A location fact the PC never witnessed or heard of does not
/// count, so a hidden watcher stays hidden. Sorted. Empty when the
/// campaign has no PC.
pub fn seen(view: &WorldView) -> Vec<EntityId> {
    let Some(pc) = pc_of(view) else {
        return Vec::new();
    };
    let Some(place) = view.live_fact(pc, "location").map(|f| f.slot.value.clone()) else {
        return vec![pc];
    };
    let mut seen: BTreeSet<EntityId> = pc_knowledge(view)
        .into_iter()
        .filter(|b| b.key == "location" && at_place(&b.value, &place))
        .map(|b| b.subject)
        .filter(|id| view.entities.iter().any(|e| e.id == *id))
        .collect();
    seen.insert(pc);
    seen.into_iter().collect()
}

/// The rule of play: two location values name one place when their
/// `normalize::value` forms are equal.
pub fn at_place(a: &str, b: &str) -> bool {
    normalize::value(a) == normalize::value(b)
}

/// The characters the world state does not place away from the PC's place
/// (`pc_place`). A character is unsure when:
/// - it has no live `location`: unknown is not away;
/// - the PC's place is unknown: then every character is;
/// - its live `location`, or its `location` as the PC believes it, may be
///   the PC's place by `may_be_at`.
///
/// It reads the world state only. A Narration or an NPC line that puts a
/// character here is not needed, and it is not read.
fn unsure(view: &WorldView, pc_place: Option<&str>) -> BTreeSet<EntityId> {
    let characters = view.entities.iter().filter(|e| is_character(e));
    let Some(pc_place) = pc_place else {
        return characters.map(|e| e.id).collect();
    };
    let mut out: BTreeSet<EntityId> = characters
        .filter(|e| {
            view.live_fact(e.id, "location")
                .is_none_or(|f| may_be_at(view, &f.slot.value, pc_place))
        })
        .map(|e| e.id)
        .collect();
    out.extend(
        pc_knowledge(view)
            .into_iter()
            .filter(|b| b.key == "location" && may_be_at(view, &b.value, pc_place))
            .map(|b| b.subject)
            .filter(|id| view.entities.iter().any(|e| e.id == *id && is_character(e))),
    );
    out
}

/// The rule of doubt: a character at `location` may be at `pc_place` when
/// the two values name one place (`near_place`), or when one place holds
/// the other (`holds`, both ways). The village that holds the PC's house
/// does not say the child is not in the house (QA run 3, B1).
fn may_be_at(view: &WorldView, location: &str, pc_place: &str) -> bool {
    near_place(view, location, pc_place)
        || holds(view, location, pc_place)
        || holds(view, pc_place, location)
}

/// Two location values name one place when, after the strongest normal
/// form the core has (`place_words`: case, accents, punctuation, a leading
/// article), their words are equal, or one is a word-prefix of the other
/// ("gate" and "gate square, by the well"), or both name one Place entity
/// through any of its names (review M1). A value with no words is unknown
/// and counts as the same place.
fn near_place(view: &WorldView, a: &str, b: &str) -> bool {
    let (wa, wb) = (place_words(a), place_words(b));
    if wa.is_empty() || wb.is_empty() || wa.starts_with(&wb) || wb.starts_with(&wa) {
        return true;
    }
    let places = |value: &str| -> HashSet<EntityId> {
        view.entities_by_name(value)
            .filter(|e| e.kind == EntityKind::Place)
            .map(|e| e.id)
            .collect()
    };
    !places(a).is_disjoint(&places(b))
}

/// True when the world state puts the place `inner` inside the place
/// `outer`: a live `location` fact whose value is `outer` (`near_place`)
/// and whose subject is a place named `inner`, or whose text names `inner`
/// as a run of whole words. "The mill house is in Millbrook." on any
/// subject, with the value "millbrook", puts the mill house in Millbrook:
/// the QA file holds that sentence on an item (the ferry).
///
/// The rule reads free text, so it can find a place inside another where
/// none is. That only makes the gate check one more character: it never
/// clears one.
fn holds(view: &WorldView, outer: &str, inner: &str) -> bool {
    let inner_words = place_words(inner);
    if inner_words.is_empty() || near_place(view, outer, inner) {
        return false;
    }
    view.live_facts
        .iter()
        .filter(|f| f.slot.key == "location" && near_place(view, &f.slot.value, outer))
        .any(|f| {
            let place_subject = view
                .entities
                .iter()
                .find(|e| e.id == f.slot.subject)
                .is_some_and(|e| e.kind == EntityKind::Place && near_place(view, &e.name, inner));
            place_subject || has_run(&normalize::name_words(&f.text), &inner_words)
        })
}

/// True when `words` holds `run` as consecutive items.
fn has_run(words: &[String], run: &[String]) -> bool {
    !run.is_empty() && words.windows(run.len()).any(|w| w == run)
}

/// The words of a location value for `near_place`: `normalize::name_words`
/// (lower case, no accents, split on every character that is not a letter
/// or a digit), with a leading article left out.
fn place_words(value: &str) -> Vec<String> {
    let mut words = normalize::name_words(value);
    if words.len() > 1 && ["the", "a", "an"].contains(&words[0].as_str()) {
        words.remove(0);
    }
    words
}

fn pc_of(view: &WorldView) -> Option<EntityId> {
    view.entities
        .iter()
        .find(|e| e.kind == EntityKind::Pc)
        .map(|e| e.id)
}

fn is_character(e: &Entity) -> bool {
    matches!(e.kind, EntityKind::Pc | EntityKind::Npc)
}
