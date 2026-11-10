use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use super::BuildFailure;
use crate::{
    context::{ADVENTURE_BLOCK_MAX_CHARS, adventure_block_chars},
    domain::{AdventureSeed, EntityKind, OtherName, alias_key, is_open_key},
    normalize,
};

/// Only three states are stored (data shape 3.5). A failed or cancelled
/// build is `Approved` with `last_failure` set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftState {
    Draft,
    Approved,
    Built,
}

/// What the Player reviews before the World Build. The roster is `seed.pc`
/// and `seed.entities`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedDraft {
    /// `draft-<nanos>`, also the file stem in the draft store.
    pub id: String,
    pub state: DraftState,
    pub source_text: String,
    pub seed: AdventureSeed,
    /// PC Knowledge: the PC is the implied holder of each fact.
    pub knowledge: Vec<KnownFact>,
    /// Main names whose age the Player has not confirmed yet. A new draft
    /// lists every character, the PC included: an age never passes on the
    /// model's word.
    pub ages_to_confirm: Vec<String>,
    /// Main names whose age the model says the Source Text states.
    /// Information for the Player only: it never removes a confirmation.
    #[serde(default)]
    pub ages_stated: Vec<String>,
    #[serde(default)]
    pub last_failure: Option<BuildFailure>,
    /// The Campaign file the build wrote; set when the state is `Built`.
    #[serde(default)]
    pub campaign_file: Option<String>,
}

impl SeedDraft {
    /// A new draft in state `Draft`, with a fresh id and no knowledge.
    pub fn new(source_text: String, seed: AdventureSeed) -> Self {
        Self {
            id: new_draft_id(),
            state: DraftState::Draft,
            source_text,
            seed,
            knowledge: vec![],
            ages_to_confirm: vec![],
            ages_stated: vec![],
            last_failure: None,
            campaign_file: None,
        }
    }
}

/// A fresh draft id, `draft-<nanos>`, from the system clock.
pub(crate) fn new_draft_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("draft-{nanos}")
}

/// A fact the PC knows. `open` marks an open item (ADR 0005): `setup` adds
/// the `pending_` prefix, so `key` never carries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownFact {
    pub subject: String,
    pub key: String,
    pub value: String,
    pub text: String,
    #[serde(default)]
    pub open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    /// An essential text is empty.
    Missing,
    /// A character has no age.
    AgeMissing,
    /// The Player has not confirmed the age yet. A new draft lists every
    /// character, whatever the model says about the text.
    AgeUnconfirmed,
    /// An age is below the seed's `min_age`.
    AgeBelowMin,
    /// A name that no roster entry carries, or one of the wrong kind.
    UnknownName,
    /// A name used twice, main or other, after `normalize::name`; or the
    /// reserved name "world".
    DuplicateName,
    /// A roster entry carries the PC's name or one of its other names.
    PcInRoster,
    /// A field that the entry's kind does not allow: an age on a
    /// non-character, a second PC, a known fact on a `pending_` key.
    WrongKind,
    /// The stable block is above `ADVENTURE_BLOCK_MAX_CHARS`.
    SeedBlockTooLong,
    /// A known fact on a slot that another fact holds: a second known fact
    /// on the same subject, key and open state; a known fact on the slot of
    /// a `decided` fact or of an other name; or a settled and an open known
    /// fact on the same subject and key. `at` names the later entry.
    SlotConflict,
}

/// One thing to fix before approval.
///
/// `at` locates the field, in the draft's own field names: a seed field
/// without the `seed.` prefix (`premise`, `story_language`, `pc.name`,
/// `pc.age`, `start.place`), an index in brackets for a list
/// (`entities[3].age`, `entities[0].other_names[1]`,
/// `entities[0].other_names[1].known_to[0]`, `start.present[2]`,
/// `decided[0].subject`), `knowledge[5].subject` for PC Knowledge, and
/// `seed` for the stable block as a whole.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    pub kind: GapKind,
    pub at: String,
}

/// Every gap of `draft`, in field order. Derived, never stored. It mirrors
/// the checks that `Store::create_campaign` runs on a seed, so the Player
/// sees them at review, before any commit. It never panics on an edited
/// draft.
pub fn gaps(draft: &SeedDraft) -> Vec<Gap> {
    let seed = &draft.seed;
    let mut out = Vec::new();
    let mut gap = |kind, at: String| out.push(Gap { kind, at });
    let below_min = |age: u16| seed.min_age.is_some_and(|min| age < min);
    let to_confirm: HashSet<String> = draft
        .ages_to_confirm
        .iter()
        .map(|n| normalize::name(n))
        .collect();
    let roster = Roster::of(seed);

    for (text, at) in [
        (&seed.premise, "premise"),
        (&seed.pc.name, "pc.name"),
        (&seed.pc.concept, "pc.concept"),
        (&seed.story_language, "story_language"),
    ] {
        if text.trim().is_empty() {
            gap(GapKind::Missing, at.to_owned());
        }
    }

    // The PC.
    let pc = &seed.pc;
    if below_min(pc.age) {
        gap(GapKind::AgeBelowMin, "pc.age".into());
    }
    if to_confirm.contains(&normalize::name(&pc.name)) {
        gap(GapKind::AgeUnconfirmed, "pc.age".into());
    }
    let pc_names: HashSet<String> = names(&pc.name, &pc.other_names)
        .map(|(n, _)| normalize::name(n))
        .filter(|n| !n.is_empty())
        .collect();
    let mut seen = HashSet::new();
    for (name, at) in names(&pc.name, &pc.other_names) {
        let at = at.map_or_else(|| "pc.name".to_owned(), |j| format!("pc.other_names[{j}]"));
        let norm = normalize::name(name);
        if norm.is_empty() {
            if at != "pc.name" {
                gap(GapKind::Missing, at);
            }
        } else if norm == "world" || !seen.insert(norm) {
            gap(GapKind::DuplicateName, at);
        }
    }
    for (j, other) in pc.other_names.iter().enumerate() {
        for (k, holder) in other.known_to.iter().enumerate() {
            if !roster.has(holder) {
                gap(
                    GapKind::UnknownName,
                    format!("pc.other_names[{j}].known_to[{k}]"),
                );
            }
        }
    }

    // The other roster entries.
    for (i, entity) in seed.entities.iter().enumerate() {
        let at = |field: &str| format!("entities[{i}].{field}");
        if entity.kind == EntityKind::Pc {
            gap(GapKind::WrongKind, at("kind"));
        }
        for (name, j) in names(&entity.name, &entity.other_names) {
            let at = j.map_or_else(|| at("name"), |j| at(&format!("other_names[{j}]")));
            let norm = normalize::name(name);
            if norm.is_empty() {
                gap(GapKind::Missing, at);
            } else if pc_names.contains(&norm) {
                gap(GapKind::PcInRoster, at);
            } else if norm == "world" || !seen.insert(norm) {
                gap(GapKind::DuplicateName, at);
            }
        }
        for (j, other) in entity.other_names.iter().enumerate() {
            for (k, holder) in other.known_to.iter().enumerate() {
                if !roster.has(holder) {
                    gap(
                        GapKind::UnknownName,
                        at(&format!("other_names[{j}].known_to[{k}]")),
                    );
                }
            }
        }
        match (is_character(entity.kind), entity.age) {
            (true, None) => gap(GapKind::AgeMissing, at("age")),
            (true, Some(age)) => {
                if below_min(age) {
                    gap(GapKind::AgeBelowMin, at("age"));
                }
                if to_confirm.contains(&normalize::name(&entity.name)) {
                    gap(GapKind::AgeUnconfirmed, at("age"));
                }
            }
            (false, Some(_)) => gap(GapKind::WrongKind, at("age")),
            (false, None) => {}
        }
    }

    // PC Knowledge.
    for (i, fact) in draft.knowledge.iter().enumerate() {
        let at = |field: &str| format!("knowledge[{i}].{field}");
        if fact.subject.trim().is_empty() {
            gap(GapKind::Missing, at("subject"));
        } else if !roster.has(&fact.subject) {
            gap(GapKind::UnknownName, at("subject"));
        }
        if fact.key.trim().is_empty() {
            gap(GapKind::Missing, at("key"));
        } else if is_open_key(&fact.key) {
            gap(GapKind::WrongKind, at("key"));
        }
        for (text, field) in [(&fact.value, "value"), (&fact.text, "text")] {
            if text.trim().is_empty() {
                gap(GapKind::Missing, at(field));
            }
        }
    }

    // One fact per slot (data shape 3.6). A known fact may not share its
    // subject and key with an earlier known fact, open or settled, nor take
    // a slot of the seed's Setup Turn.
    let taken = seed_slots(seed, &roster);
    let mut known: Vec<(usize, String)> = Vec::new();
    for (i, fact) in draft.knowledge.iter().enumerate() {
        let (Some(owner), key) = (roster.owner(&fact.subject), normalize::key(&fact.key)) else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        let slot = (owner, key);
        if taken.contains(&slot) || known.contains(&slot) {
            gap(GapKind::SlotConflict, format!("knowledge[{i}]"));
        }
        known.push(slot);
    }

    // Where play starts.
    if seed.start.place.trim().is_empty() {
        gap(GapKind::Missing, "start.place".into());
    } else if !roster.has_kind(&seed.start.place, |kind| kind == EntityKind::Place) {
        gap(GapKind::UnknownName, "start.place".into());
    }
    for (k, name) in seed.start.present.iter().enumerate() {
        if !roster.has_kind(name, is_character) {
            gap(GapKind::UnknownName, format!("start.present[{k}]"));
        }
    }

    // Seed facts: a roster name, or "world" (`Store::create_campaign`).
    for (facts, list) in [(&seed.decided, "decided"), (&seed.proposed, "proposed")] {
        for (i, fact) in facts.iter().enumerate() {
            if !roster.has(&fact.subject) && normalize::name(&fact.subject) != "world" {
                gap(GapKind::UnknownName, format!("{list}[{i}].subject"));
            }
        }
    }

    if adventure_block_chars(seed) > ADVENTURE_BLOCK_MAX_CHARS {
        gap(GapKind::SeedBlockTooLong, "seed".into());
    }
    out
}

/// Approves a draft with no gap. On a gap, returns every gap and leaves the
/// draft as it was. A `Built` draft stays `Built`.
pub fn approve(draft: &mut SeedDraft) -> Result<(), Vec<Gap>> {
    let found = gaps(draft);
    if !found.is_empty() {
        return Err(found);
    }
    if draft.state == DraftState::Draft {
        draft.state = DraftState::Approved;
    }
    Ok(())
}

/// A Player edit of the draft (the `save_draft` command): it replaces the
/// seed, the knowledge, the ages to confirm and the ages the model calls
/// stated (under the rows' current names), and returns the draft to
/// `Draft`. The last failure and the Campaign file belong to the build of
/// the content it replaces, so both are cleared. The caller then saves.
pub fn apply_edit(
    draft: &mut SeedDraft,
    seed: AdventureSeed,
    knowledge: Vec<KnownFact>,
    ages_to_confirm: Vec<String>,
    ages_stated: Vec<String>,
) {
    draft.seed = seed;
    draft.knowledge = knowledge;
    draft.ages_to_confirm = ages_to_confirm;
    draft.ages_stated = ages_stated;
    draft.state = DraftState::Draft;
    draft.last_failure = None;
    draft.campaign_file = None;
}

fn is_character(kind: EntityKind) -> bool {
    matches!(kind, EntityKind::Pc | EntityKind::Npc)
}

/// The main name (`None`), then each other name with its index.
fn names<'a>(
    main: &'a str,
    others: &'a [OtherName],
) -> impl Iterator<Item = (&'a str, Option<usize>)> {
    std::iter::once((main, None)).chain(
        others
            .iter()
            .enumerate()
            .map(|(j, o)| (o.name.as_str(), Some(j))),
    )
}

/// Every name of the roster, main or other, normalized, with its kind and
/// the index of its entry: 0 for the PC, `i + 1` for `entities[i]`.
struct Roster(Vec<(String, EntityKind, usize)>);

impl Roster {
    fn of(seed: &AdventureSeed) -> Self {
        let pc = names(&seed.pc.name, &seed.pc.other_names).map(|(n, _)| (n, EntityKind::Pc, 0));
        let entities = seed.entities.iter().enumerate().flat_map(|(i, e)| {
            names(&e.name, &e.other_names).map(move |(n, _)| (n, e.kind, i + 1))
        });
        Self(
            pc.chain(entities)
                .map(|(n, kind, owner)| (normalize::name(n), kind, owner))
                .filter(|(n, _, _)| !n.is_empty())
                .collect(),
        )
    }

    fn has(&self, name: &str) -> bool {
        self.has_kind(name, |_| true)
    }

    fn has_kind(&self, name: &str, kind: impl Fn(EntityKind) -> bool) -> bool {
        let name = normalize::name(name);
        self.0.iter().any(|(n, k, _)| *n == name && kind(*k))
    }

    /// The entry that `name` names; the first one on a tie.
    fn owner(&self, name: &str) -> Option<usize> {
        let name = normalize::name(name);
        self.0
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, _, owner)| *owner)
    }
}

/// The slots of the seed's Setup Turn that a known fact may not take: one
/// `alias_` fact per other name, and each `decided` fact. A slot is a roster
/// entry and a normalized key.
fn seed_slots(seed: &AdventureSeed, roster: &Roster) -> Vec<(usize, String)> {
    let pc = std::iter::once((0, &seed.pc.other_names));
    let entities = seed
        .entities
        .iter()
        .enumerate()
        .map(|(i, e)| (i + 1, &e.other_names));
    let aliases = pc.chain(entities).flat_map(|(owner, others)| {
        others.iter().map(move |o| {
            let key = alias_key(&o.name);
            (owner, normalize::key(&key))
        })
    });
    let decided = seed.decided.iter().filter_map(|f| {
        roster
            .owner(&f.subject)
            .map(|o| (o, normalize::key(&f.key)))
    });
    aliases.chain(decided).collect()
}
