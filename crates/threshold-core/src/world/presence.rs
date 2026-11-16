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
//! `normalize::value`. `PlaceTree::away` is the rule of doubt, for
//! `unsure` only: it reads the stored Inside Links of the Places (glossary)
//! after the fact check (`PlaceTree::checked`). The check reads the free
//! text of `location` facts, and it can only set a link to `Unknown`.
//! `inferred_links` reads free text, once, when an old Campaign file is
//! opened (`store::migrations`); the gate never calls it.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::{
    domain::{Canon, EntityId, EntityKind, Inside},
    normalize,
    store::{Entity, WorldView},
    world::{Belief, pc_knowledge},
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

/// The room of the PC now. It reads each live `location` once
/// (`Locations`), the PC's Beliefs once and the place tree once, so its
/// cost grows with the facts plus the characters, not with their product.
/// The fact check of the tree (`PlaceTree::checked`) is the exception: it
/// reads each `location` fact once per Place.
pub fn room(view: &WorldView) -> Room {
    let pc = pc_of(view);
    let locations = Locations::of(view);
    let place = pc.and_then(|pc| locations.get(pc)).map(str::to_owned);
    let knowledge = if pc.is_some() {
        pc_knowledge(view)
    } else {
        Vec::new()
    };
    let tree = PlaceTree::checked(view);
    Room {
        pc,
        truth: truth_in(view, pc, place.as_deref(), &locations)
            .into_iter()
            .collect(),
        seen: seen_in(view, pc, place.as_deref(), &knowledge)
            .into_iter()
            .collect(),
        unsure: unsure(view, &tree, &locations, &knowledge, place.as_deref()),
        place,
    }
}

/// World truth at the PC's place, hidden characters included: the PC, and
/// each entity whose live `location` is the PC's by `at_place`. In the
/// order of `entities`. Empty when the campaign has no PC.
pub fn truth(view: &WorldView) -> Vec<EntityId> {
    let pc = pc_of(view);
    let locations = Locations::of(view);
    let place = pc.and_then(|pc| locations.get(pc));
    truth_in(view, pc, place, &locations)
}

fn truth_in(
    view: &WorldView,
    pc: Option<EntityId>,
    place: Option<&str>,
    locations: &Locations,
) -> Vec<EntityId> {
    let Some(pc) = pc else {
        return Vec::new();
    };
    view.entities
        .iter()
        .filter(|e| {
            e.id == pc
                || place.is_some_and(|place| {
                    locations
                        .get(e.id)
                        .is_some_and(|value| at_place(value, place))
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
    let pc = pc_of(view);
    let place = pc
        .and_then(|pc| view.live_fact(pc, "location"))
        .map(|f| f.slot.value.clone());
    let knowledge = if pc.is_some() && place.is_some() {
        pc_knowledge(view)
    } else {
        Vec::new()
    };
    seen_in(view, pc, place.as_deref(), &knowledge)
}

fn seen_in(
    view: &WorldView,
    pc: Option<EntityId>,
    place: Option<&str>,
    knowledge: &[Belief],
) -> Vec<EntityId> {
    let Some(pc) = pc else {
        return Vec::new();
    };
    let Some(place) = place else {
        return vec![pc];
    };
    let ids: HashSet<EntityId> = view.entities.iter().map(|e| e.id).collect();
    let mut seen: BTreeSet<EntityId> = knowledge
        .iter()
        .filter(|b| b.key == "location" && at_place(&b.value, place))
        .map(|b| b.subject)
        .filter(|id| ids.contains(id))
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
/// (`pc_place`). A character is unsure unless `PlaceTree::away` says its
/// location is away from the PC's place. So it is unsure when:
/// - it has no live `location`: unknown is not away;
/// - the PC's place is unknown: then every character is;
/// - its live `location`, or its `location` as the PC believes it, is not
///   away by `PlaceTree::away`: the same place, a place above or below the
///   PC's place, a value that names no known Place, or a chain with a
///   missing Inside Link.
///
/// It reads the world state only. A Narration or an NPC line that puts a
/// character here is not needed, and it is not read.
fn unsure(
    view: &WorldView,
    tree: &PlaceTree,
    locations: &Locations,
    knowledge: &[Belief],
    pc_place: Option<&str>,
) -> BTreeSet<EntityId> {
    let characters = view.entities.iter().filter(|e| is_character(e));
    let Some(pc_place) = pc_place else {
        return characters.map(|e| e.id).collect();
    };
    let mut out: BTreeSet<EntityId> = characters
        .clone()
        .filter(|e| {
            locations
                .get(e.id)
                .is_none_or(|value| !tree.away(value, pc_place))
        })
        .map(|e| e.id)
        .collect();
    let character_ids: HashSet<EntityId> = characters.map(|e| e.id).collect();
    out.extend(
        knowledge
            .iter()
            .filter(|b| b.key == "location" && !tree.away(&b.value, pc_place))
            .map(|b| b.subject)
            .filter(|id| character_ids.contains(id)),
    );
    out
}

/// Each entity's live `location` value, read once: an Established or
/// Persistent fact first, as `WorldView::live_fact` reads it, else a
/// Possible one.
struct Locations<'a>(HashMap<EntityId, &'a str>);

impl<'a> Locations<'a> {
    fn of(view: &'a WorldView) -> Self {
        let mut firm: HashMap<EntityId, &'a str> = HashMap::new();
        let mut possible: HashMap<EntityId, &'a str> = HashMap::new();
        for f in view.live_facts.iter().filter(|f| f.slot.key == "location") {
            let map = if f.canon == Canon::Possible {
                &mut possible
            } else {
                &mut firm
            };
            map.entry(f.slot.subject).or_insert(f.slot.value.as_str());
        }
        for (id, value) in possible {
            firm.entry(id).or_insert(value);
        }
        Self(firm)
    }

    fn get(&self, id: EntityId) -> Option<&'a str> {
        self.0.get(&id).copied()
    }
}

/// The Places of a view and their stored Inside Links (glossary), indexed
/// by name words. Built once per `room`.
pub struct PlaceTree {
    links: HashMap<EntityId, Inside>,
    /// The `place_words` of each name, main or other, and the Places that
    /// answer to it.
    names: HashMap<Vec<String>, Vec<EntityId>>,
}

impl PlaceTree {
    pub fn of(view: &WorldView) -> Self {
        let mut links = HashMap::new();
        let mut names: HashMap<Vec<String>, Vec<EntityId>> = HashMap::new();
        for e in view.entities.iter().filter(|e| e.kind == EntityKind::Place) {
            links.insert(e.id, e.inside);
            for name in std::iter::once(&e.name).chain(&e.other_names) {
                let words = place_words(name);
                if !words.is_empty() {
                    let ids = names.entry(words).or_default();
                    if !ids.contains(&e.id) {
                        ids.push(e.id);
                    }
                }
            }
        }
        Self { links, names }
    }

    /// The tree the gate reads (ADR 0008, review of 2026-11-16): the
    /// stored links, with each Place that `doubted_in` names set to
    /// `Unknown`. A fact written in play after the Player confirmed a link
    /// can only add doubt to that link; it never fills a link.
    pub fn checked(view: &WorldView) -> Self {
        let mut tree = Self::of(view);
        for id in doubted_in(view) {
            tree.links.insert(id, Inside::Unknown);
        }
        tree
    }

    /// The Places a location value names through any of their names.
    fn named(&self, value: &str) -> &[EntityId] {
        self.names
            .get(&place_words(value))
            .map_or(&[], Vec::as_slice)
    }

    /// The one Place a location value names, or `None` when it names none
    /// or several.
    pub fn resolve(&self, value: &str) -> Option<EntityId> {
        match self.named(value) {
            [one] => Some(*one),
            _ => None,
        }
    }

    /// The chain of `place` up to its top-level Place: `place`, its parent,
    /// and so on. `None` when a link on the way is unknown, names no Place
    /// of the view, or loops.
    pub fn chain(&self, place: EntityId) -> Option<Vec<EntityId>> {
        let mut chain = vec![place];
        let mut at = place;
        loop {
            match self.links.get(&at)? {
                Inside::Unknown => return None,
                Inside::Top => return Some(chain),
                Inside::Place(parent) => {
                    if chain.contains(parent) || !self.links.contains_key(parent) {
                        return None;
                    }
                    chain.push(*parent);
                    at = *parent;
                }
            }
        }
    }

    /// The rule of doubt (ADR 0008, Inside Links): true only when a
    /// character at `location` is surely not at `pc_place`. Both values
    /// must name one known Place each (`resolve`); both chains must be
    /// known up to a top-level Place (`chain`); and neither Place may sit
    /// on the other's chain. Two Places that share only an ancestor (two
    /// houses of one village) are away from each other. Everything else is
    /// doubt: the same place by name (`near_place`), a village that holds
    /// the PC's house, the house that the PC's village holds, a value that
    /// names no Place or several, and any missing link.
    pub fn away(&self, location: &str, pc_place: &str) -> bool {
        if self.near_place(location, pc_place) {
            return false;
        }
        let (Some(at), Some(pc)) = (self.resolve(location), self.resolve(pc_place)) else {
            return false;
        };
        let (Some(up_at), Some(up_pc)) = (self.chain(at), self.chain(pc)) else {
            return false;
        };
        !up_at.contains(&pc) && !up_pc.contains(&at)
    }

    /// Two location values name one place when, after the strongest normal
    /// form the core has (`place_words`: case, accents, punctuation, a
    /// leading article), their words are equal, or one is a word-prefix of
    /// the other ("gate" and "gate square, by the well"), or both name one
    /// Place entity through any of its names (review M1). A value with no
    /// words is unknown and counts as the same place.
    fn near_place(&self, a: &str, b: &str) -> bool {
        let (wa, wb) = (place_words(a), place_words(b));
        if wa.is_empty() || wb.is_empty() || wa.starts_with(&wb) || wb.starts_with(&wa) {
            return true;
        }
        let pb = self.named(b);
        self.named(a).iter().any(|id| pb.contains(id))
    }
}

/// The Inside Links an old Campaign's facts give (the migration of
/// version 4 files, `store::migrations`): for each Place with no stored
/// link, the one other Place that holds it. A Place is inside a Place `Q`
/// when a live `location` fact names `Q` as its value (`PlaceTree::
/// resolve`), and either its subject is that Place, or its text names that
/// Place as a run of whole words. "The ferry house is in Ostley
/// Crossing." on the ferry (an item) puts the ferry house in Ostley
/// Crossing: QA run 3 holds that sentence.
///
/// A link from the subject wins over one from text. A Place with two
/// different candidates gets none, and a link that would close a cycle is
/// left out. It never writes `Top`: a Place that no fact holds keeps no
/// link, so the gate keeps doubting it. It reads free text and can find a
/// link where none is; in the gate, a wrong parent link only adds doubt,
/// since a chain is known only up to a top-level Place.
pub fn inferred_links(view: &WorldView) -> Vec<(EntityId, EntityId)> {
    let tree = PlaceTree::of(view);
    let places: Vec<&Entity> = view
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::Place && e.inside == Inside::Unknown)
        .collect();
    let locations: Vec<(EntityId, EntityId, Vec<String>)> = view
        .live_facts
        .iter()
        .filter(|f| f.slot.key == "location" && f.canon != Canon::Possible)
        .filter_map(|f| {
            tree.resolve(&f.slot.value)
                .map(|q| (f.slot.subject, q, normalize::name_words(&f.text)))
        })
        .collect();
    let mut links: HashMap<EntityId, EntityId> = HashMap::new();
    for place in places {
        let names: Vec<&str> = std::iter::once(&place.name)
            .chain(&place.other_names)
            .map(String::as_str)
            .collect();
        let candidates = fact_parents(place.id, &names, &locations);
        if let [parent] = candidates.into_iter().collect::<Vec<_>>()[..] {
            let mut at = Some(parent);
            let mut cycle = false;
            let mut steps = 0;
            while let Some(id) = at {
                steps += 1;
                if id == place.id || steps > tree.links.len() {
                    cycle = true;
                    break;
                }
                at = links.get(&id).copied().or(match tree.links.get(&id) {
                    Some(Inside::Place(p)) => Some(*p),
                    _ => None,
                });
            }
            if !cycle {
                links.insert(place.id, parent);
            }
        }
    }
    let mut out: Vec<(EntityId, EntityId)> = links.into_iter().collect();
    out.sort_unstable();
    out
}

/// The parents the facts give `place` (the rules of `inferred_links`).
/// `facts` holds each `location` fact whose value names one Place: its
/// subject, that Place, and the words of its text (`normalize::
/// name_words`), split once per fact. The subject rule wins: the Places
/// that the facts about `place` itself name. Else the text rule: the
/// Places named by each fact whose text names `place`, by any of `names`,
/// as a run of whole words. `place` itself is never a parent.
fn fact_parents<K: Copy + Ord>(
    place: K,
    names: &[&str],
    facts: &[(K, K, Vec<String>)],
) -> BTreeSet<K> {
    let by_subject: BTreeSet<K> = facts
        .iter()
        .filter(|(subject, q, _)| *subject == place && *q != place)
        .map(|(_, q, _)| *q)
        .collect();
    if !by_subject.is_empty() {
        return by_subject;
    }
    let names: Vec<Vec<String>> = names
        .iter()
        .map(|n| place_words(n))
        .filter(|w| !w.is_empty())
        .collect();
    facts
        .iter()
        .filter(|(_, q, _)| *q != place)
        .filter(|(_, _, words)| names.iter().any(|n| has_run(words, n)))
        .map(|(_, q, _)| *q)
        .collect()
}

/// One Place for `doubted_places`: a key, its names (main and other) and
/// the Inside Link proposed for it.
#[derive(Clone, Debug)]
pub struct TreeNode<K> {
    pub id: K,
    pub names: Vec<String>,
    pub inside: Inside<K>,
}

/// One `location` fact for `doubted_places`. `subject` is the key of any
/// entry, a Place or not.
#[derive(Clone, Copy, Debug)]
pub struct PlaceFact<'a, K> {
    pub subject: K,
    pub value: &'a str,
    pub text: &'a str,
}

/// The fact check of a proposed place tree (ADR 0008, the trust boundary
/// of 2026-11-16): the Places whose proposed link the facts do not
/// support. The caller sets each one to `Unknown`, so the check only moves
/// a link toward doubt, never toward `Top`. Two rules:
///
/// 1. The rules of `inferred_links` (subject rule, else text rule) give
///    the parents the facts point to. A Place is doubted when one of them
///    is not on the Place's proposed chain (its parent, the parent's
///    parent, and so on): a `Top` link, another parent, or a broken chain.
/// 2. A Place `A` named, as a run of whole words, in the value or the text
///    of a `location` fact about another Place `B` is doubted when neither
///    sits on the other's proposed chain. "Ostley Crossing lies on the
///    river Sallow." with both at top level doubts the Sallow.
///
/// A value names a Place as `PlaceTree::resolve` reads it: the words of
/// exactly one Place name. It reads free text, so it can doubt a link the
/// facts never meant to touch; a doubt only asks the Player to choose.
pub fn doubted_places<K: Copy + Ord + std::hash::Hash>(
    nodes: &[TreeNode<K>],
    facts: &[PlaceFact<K>],
) -> BTreeSet<K> {
    let links: HashMap<K, Inside<K>> = nodes.iter().map(|n| (n.id, n.inside)).collect();
    // The proposed ancestors of a Place, as far as the links are known.
    let above = |place: K| -> Vec<K> {
        let mut up = Vec::new();
        let mut at = place;
        while let Some(Inside::Place(parent)) = links.get(&at) {
            if *parent == place || up.contains(parent) {
                break;
            }
            up.push(*parent);
            at = *parent;
        }
        up
    };
    let resolved = resolve_facts(nodes, facts);
    let mut out = BTreeSet::new();
    for node in nodes {
        let names: Vec<&str> = node.names.iter().map(String::as_str).collect();
        let up = above(node.id);
        if fact_parents(node.id, &names, &resolved)
            .iter()
            .any(|q| !up.contains(q))
        {
            out.insert(node.id);
        }
    }
    for fact in facts.iter().filter(|f| links.contains_key(&f.subject)) {
        let b = fact.subject;
        let value = normalize::name_words(fact.value);
        let text = normalize::name_words(fact.text);
        for a in nodes.iter().filter(|n| n.id != b) {
            let named = a
                .names
                .iter()
                .map(|n| place_words(n))
                .any(|n| !n.is_empty() && (has_run(&value, &n) || has_run(&text, &n)));
            if named && !above(a.id).contains(&b) && !above(b).contains(&a.id) {
                out.insert(a.id);
            }
        }
    }
    out
}

/// The Places whose proposed link a fact backs (ADR 0008, 2026-11-16):
/// the link is `Inside::Place(p)`, and the rules of `inferred_links`
/// (subject rule, else text rule) give `p` itself as a parent. An
/// ancestor of `p` does not back the link. `Top` is never backed: no fact
/// can say that a Place is inside nothing. The Seed review starts a link
/// that is not backed on "Choose…", so the Player must choose it.
pub fn backed_places<K: Copy + Ord + std::hash::Hash>(
    nodes: &[TreeNode<K>],
    facts: &[PlaceFact<K>],
) -> BTreeSet<K> {
    let resolved = resolve_facts(nodes, facts);
    nodes
        .iter()
        .filter(|node| {
            let Inside::Place(parent) = node.inside else {
                return false;
            };
            let names: Vec<&str> = node.names.iter().map(String::as_str).collect();
            fact_parents(node.id, &names, &resolved).contains(&parent)
        })
        .map(|node| node.id)
        .collect()
}

/// Each fact whose value names exactly one Place of `nodes`: its subject,
/// that Place and the words of its text. A value names a Place as `PlaceTree::resolve`
/// reads it: the words of exactly one Place name.
fn resolve_facts<K: Copy + Ord + std::hash::Hash>(
    nodes: &[TreeNode<K>],
    facts: &[PlaceFact<K>],
) -> Vec<(K, K, Vec<String>)> {
    let mut index: HashMap<Vec<String>, Vec<K>> = HashMap::new();
    for node in nodes {
        for name in &node.names {
            let words = place_words(name);
            if !words.is_empty() {
                let ids = index.entry(words).or_default();
                if !ids.contains(&node.id) {
                    ids.push(node.id);
                }
            }
        }
    }
    let resolve = |value: &str| match index.get(&place_words(value)).map(Vec::as_slice) {
        Some([one]) => Some(*one),
        _ => None,
    };
    facts
        .iter()
        .filter_map(|f| resolve(f.value).map(|q| (f.subject, q, normalize::name_words(f.text))))
        .collect()
}

/// `doubted_places` over a Campaign: its Places with their stored links,
/// and every live `location` fact whose subject is not a character. The
/// gate calls it on each `room` (`PlaceTree::checked`).
///
/// A fact about a character is left out. Its text often names the place
/// the character left or passes through ("Pell went from the inn to the
/// ferry house."), and the text rule would read that as "the inn is in the
/// ferry house". The subject rule and rule 2 never read such a fact, since
/// its subject is no Place. So a fact about a character that contradicts a
/// link does not doubt it in play (ADR 0008, Limits).
pub fn doubted_in(view: &WorldView) -> BTreeSet<EntityId> {
    let (nodes, mut facts) = tree_in(view);
    let characters: HashSet<EntityId> = view
        .entities
        .iter()
        .filter(|e| is_character(e))
        .map(|e| e.id)
        .collect();
    facts.retain(|f| !characters.contains(&f.subject));
    doubted_places(&nodes, &facts)
}

/// `backed_places` over a Campaign, as `doubted_in`. For the QA probes.
pub fn backed_in(view: &WorldView) -> BTreeSet<EntityId> {
    let (nodes, facts) = tree_in(view);
    backed_places(&nodes, &facts)
}

/// The Places of a Campaign with their stored links, and every live
/// `location` fact.
fn tree_in(view: &WorldView) -> (Vec<TreeNode<EntityId>>, Vec<PlaceFact<'_, EntityId>>) {
    let nodes: Vec<TreeNode<EntityId>> = view
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::Place)
        .map(|e| TreeNode {
            id: e.id,
            names: std::iter::once(&e.name)
                .chain(&e.other_names)
                .cloned()
                .collect(),
            inside: e.inside,
        })
        .collect();
    let facts: Vec<PlaceFact<EntityId>> = view
        .live_facts
        .iter()
        .filter(|f| f.slot.key == "location")
        .map(|f| PlaceFact {
            subject: f.slot.subject,
            value: &f.slot.value,
            text: &f.text,
        })
        .collect();
    (nodes, facts)
}

/// True when `words` holds `run` as consecutive items.
pub(crate) fn has_run(words: &[String], run: &[String]) -> bool {
    !run.is_empty() && words.windows(run.len()).any(|w| w == run)
}

/// The words of a location value for `near_place`: `normalize::name_words`
/// (lower case, no accents, split on every character that is not a letter
/// or a digit), with a leading article left out.
pub(crate) fn place_words(value: &str) -> Vec<String> {
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
