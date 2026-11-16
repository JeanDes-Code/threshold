//! The place tree of a Seed Draft (ADR 0008, Inside Links and the trust
//! boundary of 2026-11-16). The draft run asks the model for the Inside
//! Link of each Place (`draft_tree`), then the fact check
//! (`world::presence::doubted_places`) sets each link the facts do not
//! support to `Unknown`. A link that a fact backs
//! (`world::presence::backed_places`) is the review's starting choice;
//! any other link starts on "Choose…", with the model's guess as a hint.
//! The Seed review shows each link (`place_lines`);
//! the Player confirms or changes it (`confirm_inside`), and `gaps` blocks
//! approval until every Place has a confirmed link. The World Build uses
//! the confirmed links (`world_places`) and never asks the model again.

use std::collections::{BTreeSet, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    BuildCheck, BuildFailure, Control, SeedDraft, Stage,
    checks::{self, FactBook, Roster, TreeItem, is_top_level},
    draft::{EntryRef, ReviewError},
    stages::{Staged, report, run_stage},
    world_prompts::tree_call,
};
use crate::{
    domain::{EntityKind, FactEntry, Inside, PlaceEntry, TOP_LEVEL},
    model::Gm,
    normalize,
    world::presence::{PlaceFact, TreeNode, backed_places, doubted_places, has_run, place_words},
};

/// The Inside Link of one Place in a Seed Draft. A link counts for a Place
/// row only while the row holds a Place with the exact main name `place`,
/// and, for `Inside::Place`, while another row holds a Place with the
/// parent's exact main name. So an edit that renames either one, or turns
/// either into another kind, asks for a new choice. `rename` moves both
/// names, and asks the renamed Place for a new choice (`rename_links`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceLink {
    /// The Place's main name, exactly as written.
    pub place: String,
    /// The parent by exact main name, `Top`, or `Unknown` ("not sure").
    pub inside: Inside<String>,
    /// The fact check found the model's link at odds with the facts, and
    /// set it to `Unknown`.
    #[serde(default)]
    pub doubted: bool,
    /// A fact backs the model's link (`backed_places`): the review starts
    /// on it. An unconfirmed link that is not backed starts on "Choose…".
    /// False on a draft saved before the field existed, so its links start
    /// blank too.
    #[serde(default)]
    pub backed: bool,
    /// The Player confirmed `inside` through `confirm_inside`.
    #[serde(default)]
    pub confirmed: bool,
}

/// One Place's link as the review shows it. Derived, never stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceLine {
    /// The row: `seed.entities[index]`.
    pub index: usize,
    /// The main name, as the seed writes it.
    pub name: String,
    /// The review's starting choice: the confirmed link, else a backed
    /// link, else `Unknown` ("Choose…").
    pub inside: Inside<String>,
    pub doubted: bool,
    pub backed: bool,
    pub confirmed: bool,
    /// The model's link that no fact backs, for a hint only; `None` once
    /// confirmed, for a backed link, and for no proposal.
    pub suggested: Option<Inside<String>>,
}

impl PlaceLine {
    /// The entry of this line, for `confirm_inside`.
    pub fn entry(&self) -> EntryRef {
        EntryRef::entity(self.index, &self.name, EntityKind::Place)
    }
}

/// The link of each Place row, in roster order. A row with no link that
/// counts (`PlaceLink`) shows `Unknown`, not confirmed. A confirmed link
/// whose chain loops is shown as not confirmed.
pub fn place_lines(draft: &SeedDraft) -> Vec<PlaceLine> {
    let rows: Vec<(usize, &str)> = place_rows(draft);
    let is_place = |name: &str| rows.iter().any(|(_, n)| *n == name);
    let mut lines: Vec<PlaceLine> = rows
        .iter()
        .map(|&(index, name)| {
            let link = draft
                .place_links
                .iter()
                .find(|l| l.place == name)
                .filter(|l| match &l.inside {
                    Inside::Place(parent) => parent != name && is_place(parent),
                    _ => true,
                });
            let stored = link.map_or(Inside::Unknown, |l| l.inside.clone());
            let backed = link.is_some_and(|l| l.backed);
            let confirmed = link.is_some_and(|l| l.confirmed);
            let (inside, suggested) = if confirmed || backed {
                (stored, None)
            } else if stored == Inside::Unknown {
                (Inside::Unknown, None)
            } else {
                (Inside::Unknown, Some(stored))
            };
            PlaceLine {
                index,
                name: name.to_owned(),
                inside,
                doubted: link.is_some_and(|l| l.doubted),
                backed,
                confirmed,
                suggested,
            }
        })
        .collect();
    let looped: Vec<usize> = (0..lines.len())
        .filter(|&i| loops(&lines, &lines[i].name))
        .collect();
    for i in looped {
        lines[i].confirmed = false;
    }
    lines
}

/// True when the chain of links from `name` comes back to `name`.
fn loops(lines: &[PlaceLine], name: &str) -> bool {
    let parent = |at: &str| match lines.iter().find(|l| l.name == at).map(|l| &l.inside) {
        Some(Inside::Place(p)) => Some(p.clone()),
        _ => None,
    };
    let mut at = parent(name);
    let mut steps = 0;
    while let Some(p) = at {
        if p == name {
            return true;
        }
        steps += 1;
        if steps > lines.len() {
            return true;
        }
        at = parent(&p);
    }
    false
}

/// The row and exact main name of each Place of the seed.
fn place_rows(draft: &SeedDraft) -> Vec<(usize, &str)> {
    draft
        .seed
        .entities
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == EntityKind::Place)
        .map(|(i, e)| (i, e.name.as_str()))
        .collect()
}

/// The paths of the Place rows whose link waits for the Player:
/// `entities[i].inside`. A Place with no name is a `Missing` gap: its link
/// waits for the name.
pub(super) fn unconfirmed(draft: &SeedDraft) -> Vec<usize> {
    place_lines(draft)
        .into_iter()
        .filter(|l| !l.confirmed && !normalize::name(&l.name).is_empty())
        .map(|l| l.index)
        .collect()
}

/// Confirms the Inside Link of one Place: `inside` is another Place by
/// exact main name, `Top`, or `Unknown` ("not sure"). The only writer of
/// `PlaceLink.confirmed`. The entry must match its row exactly
/// (`EntryRef`) and be a Place. A parent that is no other Place of the
/// draft is refused (`UnknownName`), and so is a link that makes a loop
/// (`Loop`); the draft does not change.
/// A confirmation is an edit (`draft::edited`): a built draft goes back to
/// `Draft`.
pub fn confirm_inside(
    draft: &mut SeedDraft,
    entry: &EntryRef,
    inside: Inside<String>,
) -> Result<(), ReviewError> {
    let row = entry.index.ok_or(ReviewError::UnknownName)?;
    match draft.seed.entities.get(row) {
        Some(e)
            if e.kind == EntityKind::Place
                && entry.kind == EntityKind::Place
                && e.name == entry.name
                && !normalize::name(&e.name).is_empty() => {}
        _ => return Err(ReviewError::UnknownName),
    }
    let name = entry.name.clone();
    let mut lines = place_lines(draft);
    if let Inside::Place(parent) = &inside {
        if *parent == name {
            return Err(ReviewError::Loop);
        }
        if !lines.iter().any(|l| l.name == *parent) {
            return Err(ReviewError::UnknownName);
        }
    }
    if let Some(line) = lines.iter_mut().find(|l| l.name == name) {
        line.inside = inside.clone();
    }
    if loops(&lines, &name) {
        return Err(ReviewError::Loop);
    }
    let doubted = draft
        .place_links
        .iter()
        .find(|l| l.place == name)
        .is_some_and(|l| l.doubted);
    draft.place_links.retain(|l| l.place != name);
    draft.place_links.push(PlaceLink {
        place: name,
        inside,
        doubted,
        backed: false,
        confirmed: true,
    });
    super::draft::edited(draft);
    Ok(())
}

/// After `rename` of a Place from `old` to `to`: each link of `old`, and
/// each link to `old` as a parent, now names `to`. The renamed Place asks
/// for a new choice (ADR 0008; QA run 4): its own link loses its
/// confirmation and its backing, so the review starts it on "Choose…",
/// with the old link as a hint. A link to it as a parent keeps its
/// confirmation: the parent is the same Place under a new name.
pub(super) fn rename_links(draft: &mut SeedDraft, old: &str, to: &str) {
    for link in &mut draft.place_links {
        if link.place == old {
            to.clone_into(&mut link.place);
            link.confirmed = false;
            link.backed = false;
        }
        if let Inside::Place(parent) = &mut link.inside
            && parent == old
        {
            to.clone_into(parent);
        }
    }
}

/// The draft stage of the place tree: one call when the roster holds a
/// Place. The model gets the Source Text, the roster and the known facts.
/// The call is optional: a failed answer, a dropped link or a Place the
/// answer leaves out keeps an `Unknown` link, which the Player must
/// choose. A model error or a timeout does not fail the draft either: the
/// draft is whole, and every Place waits for a choice. A cancel stops the
/// draft. Then the fact check (`propose`) runs.
pub(super) fn draft_tree(
    draft: &mut SeedDraft,
    gm: &dyn Gm,
    ctl: &mut Control,
) -> Result<(), BuildFailure> {
    let roster = Roster::of(&draft.seed);
    let places: Vec<&str> = roster
        .members()
        .iter()
        .filter(|m| m.kind == EntityKind::Place && !normalize::name(&m.main).is_empty())
        .map(|m| m.main.as_str())
        .collect();
    if places.is_empty() {
        return Ok(());
    }
    let mut book = FactBook::of_seed(&draft.seed, &roster);
    let pc = roster.pc().main.clone();
    for known in &draft.knowledge {
        // A known fact that clashes is a gap the review shows; the prompt
        // goes without it.
        let _ = book.known(FactEntry {
            subject: roster.main(&known.subject),
            key: normalize::key(&known.key),
            value: known.value.clone(),
            text: known.text.clone(),
            known_by: vec![pc.clone()],
            open: known.open,
        });
    }
    report(ctl, Stage::PlaceTree, 0, 1);
    let call = tree_call(draft, &roster, &book, &places);
    let staged = match run_stage(gm, ctl, &draft.id, &call, |out| {
        checks::place_tree(&roster, out)
    }) {
        Ok(staged) => staged,
        Err(failure) if failure.check == BuildCheck::Cancelled => return Err(failure),
        // The draft is whole without the proposal: every Place waits for
        // the Player's choice.
        Err(_) => None,
    };
    report(ctl, Stage::PlaceTree, 1, 1);
    let proposed: Vec<(String, Inside<String>)> = staged
        .map(|Staged { output, drop, .. }| {
            output
                .places
                .into_iter()
                .enumerate()
                .filter(|(i, _)| !drop.contains(&TreeItem::Link(*i)))
                .map(|(_, link)| {
                    let inside = if is_top_level(&link.inside) {
                        Inside::Top
                    } else {
                        Inside::Place(roster.main(&link.inside))
                    };
                    (roster.main(&link.place), inside)
                })
                .collect()
        })
        .unwrap_or_default();
    let facts = draft_location_facts(draft);
    let evidence = source_facts(&roster, &proposed, &draft.source_text);
    draft.place_links = propose(&roster, &proposed, &facts, &evidence);
    Ok(())
}

/// The `location` facts that the Source Text gives the proposed links
/// (QA run 4): for each link "`P` inside `Q`", the first sentence of the
/// text that names `P` and `Q`, and no other Place, as whole words. The
/// fact is `P`, location `Q`, with that sentence as its text. The live
/// model rarely writes a `location` fact about a Place: it gives its facts
/// to the PC, under keys such as `destination_location`. So a link that
/// the Player's own text states had nothing to back it.
///
/// These facts back a link only (`backed_places`). They never take part in
/// the doubt check, and the draft does not keep them: built from the
/// proposal, they would hide a fact that contradicts it, since the subject
/// rule wins over the text rule. A sentence with a third Place backs
/// nothing: "the inn in the crossing, on the river" does not say which
/// one holds the inn.
fn source_facts(
    roster: &Roster,
    proposed: &[(String, Inside<String>)],
    text: &str,
) -> Vec<(String, String, String)> {
    let places: Vec<(&str, Vec<Vec<String>>)> = roster
        .members()
        .iter()
        .filter(|m| m.kind == EntityKind::Place)
        .map(|m| {
            let names = std::iter::once(&m.main)
                .chain(&m.others)
                .map(|n| place_words(n))
                .filter(|w| !w.is_empty())
                .collect();
            (m.main.as_str(), names)
        })
        .collect();
    let sentences: Vec<(&str, Vec<&str>)> = text
        .split_inclusive(['.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|sentence| {
            let words = normalize::name_words(sentence);
            let named = places
                .iter()
                .filter(|(_, names)| names.iter().any(|n| has_run(&words, n)))
                .map(|(main, _)| *main)
                .collect();
            (sentence, named)
        })
        .collect();
    proposed
        .iter()
        .filter_map(|(place, inside)| {
            let Inside::Place(parent) = inside else {
                return None;
            };
            let (sentence, _) = sentences.iter().find(|(_, named)| {
                named.len() == 2
                    && named.contains(&place.as_str())
                    && named.contains(&parent.as_str())
            })?;
            Some((place.clone(), parent.clone(), (*sentence).to_owned()))
        })
        .collect()
}

/// The `location` facts of the draft for the fact check: each known fact,
/// decided fact and proposed fact on the key `location`.
fn draft_location_facts(draft: &SeedDraft) -> Vec<(String, String, String)> {
    let seed = &draft.seed;
    let known = draft
        .knowledge
        .iter()
        .map(|k| (&k.subject, &k.key, &k.value, &k.text));
    let settled = seed
        .decided
        .iter()
        .chain(&seed.proposed)
        .map(|f| (&f.subject, &f.key, &f.value, &f.text));
    known
        .chain(settled)
        .filter(|(_, key, _, _)| normalize::key(key) == "location")
        .map(|(s, _, v, t)| (s.clone(), v.clone(), t.clone()))
        .collect()
}

/// The fact check of a proposed tree over `roster`: each Place gets its
/// proposed link, or `Unknown` when the proposal has none or when
/// `doubted_places` finds it at odds with `facts` (subject, value, text).
/// A link that `facts` or `evidence` back starts on the model's choice.
/// No link is confirmed.
fn propose(
    roster: &Roster,
    proposed: &[(String, Inside<String>)],
    facts: &[(String, String, String)],
    evidence: &[(String, String, String)],
) -> Vec<PlaceLink> {
    let checked = check(roster, proposed, facts, evidence);
    roster
        .members()
        .iter()
        .filter(|m| m.kind == EntityKind::Place)
        .map(|m| {
            let model = proposed.iter().find(|(p, _)| *p == m.main).map(|(_, i)| i);
            let doubted = model.is_some() && checked.doubted.contains(&m.main);
            PlaceLink {
                place: m.main.clone(),
                inside: match model {
                    Some(inside) if !doubted => inside.clone(),
                    _ => Inside::Unknown,
                },
                doubted,
                backed: !doubted && checked.backed.contains(&m.main),
                confirmed: false,
            }
        })
        .collect()
}

/// What the fact check says of a tree, by main name.
struct Checked {
    /// The Places whose link the facts do not support (`doubted_places`).
    doubted: BTreeSet<String>,
    /// The Places whose link a fact backs (`backed_places`).
    backed: BTreeSet<String>,
}

/// The fact check of `links` over a roster. Keys are roster positions.
/// `facts` doubt and back a link; `evidence` (`source_facts`) only backs
/// one.
fn check(
    roster: &Roster,
    links: &[(String, Inside<String>)],
    facts: &[(String, String, String)],
    evidence: &[(String, String, String)],
) -> Checked {
    let members = roster.members();
    let position: HashMap<&str, usize> = members
        .iter()
        .enumerate()
        .map(|(i, m)| (m.main.as_str(), i))
        .collect();
    let key = |name: &str| {
        roster
            .find(name)
            .and_then(|m| position.get(m.main.as_str()).copied())
    };
    let nodes: Vec<TreeNode<usize>> = members
        .iter()
        .enumerate()
        .filter(|(_, m)| m.kind == EntityKind::Place)
        .map(|(i, m)| TreeNode {
            id: i,
            names: std::iter::once(&m.main).chain(&m.others).cloned().collect(),
            inside: match links.iter().find(|(p, _)| *p == m.main).map(|(_, l)| l) {
                Some(Inside::Top) => Inside::Top,
                Some(Inside::Place(parent)) => key(parent).map_or(Inside::Unknown, Inside::Place),
                _ => Inside::Unknown,
            },
        })
        .collect();
    let facts = place_facts(&key, facts);
    let mut backing = facts.clone();
    backing.extend(place_facts(&key, evidence));
    let names = |set: BTreeSet<usize>| set.into_iter().map(|i| members[i].main.clone()).collect();
    Checked {
        doubted: names(doubted_places(&nodes, &facts)),
        backed: names(backed_places(&nodes, &backing)),
    }
}

/// Each fact of `list` (subject, value, text) whose subject has a key.
fn place_facts<'a>(
    key: &impl Fn(&str) -> Option<usize>,
    list: &'a [(String, String, String)],
) -> Vec<PlaceFact<'a, usize>> {
    list.iter()
        .filter_map(|(subject, value, text)| {
            Some(PlaceFact {
                subject: key(subject)?,
                value,
                text,
            })
        })
        .collect()
}

/// The Inside Links the World Build writes: each confirmed link of the
/// draft, then the fact check again over the facts the World Build added
/// (`added`). A link those facts doubt is left out, so the Place keeps an
/// unknown link: the check moves a link toward doubt only. The facts of
/// the draft are not checked again: the Player saw them when choosing. A
/// link the Player set to "not sure" is left out too.
pub(super) fn world_places(
    draft: &SeedDraft,
    roster: &Roster,
    added: &[FactEntry],
) -> Vec<PlaceEntry> {
    let links: Vec<(String, Inside<String>)> = place_lines(draft)
        .into_iter()
        .filter(|l| l.confirmed)
        .map(|l| (l.name, l.inside))
        .collect();
    let facts: Vec<(String, String, String)> = added
        .iter()
        .filter(|f| normalize::key(&f.key) == "location")
        .map(|f| (f.subject.clone(), f.value.clone(), f.text.clone()))
        .collect();
    let doubted = check(roster, &links, &facts, &[]).doubted;
    links
        .into_iter()
        .filter(|(place, _)| !doubted.contains(place))
        .filter_map(|(place, inside)| {
            let inside = match inside {
                Inside::Unknown => return None,
                Inside::Top => TOP_LEVEL.to_owned(),
                Inside::Place(parent) => parent,
            };
            Some(PlaceEntry { place, inside })
        })
        .collect()
}

// ---- Stage output ----------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
pub(super) struct TreeOut {
    #[schemars(length(max = 80))]
    pub places: Vec<LinkOut>,
}

/// The Inside Link of one Place.
#[derive(Deserialize, JsonSchema)]
pub(super) struct LinkOut {
    #[schemars(length(max = 60))]
    pub place: String,
    /// The place from PLACES that directly holds `place`, or `none`.
    #[schemars(length(max = 60))]
    pub inside: String,
}
