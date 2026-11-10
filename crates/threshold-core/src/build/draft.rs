use std::{
    collections::{BTreeMap, HashSet},
    fmt,
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
    /// The age of each character, under `normalize::name` of its main
    /// name: what the draft gave, and what the Player confirmed. Only
    /// `confirm_age` writes a confirmation, so a draft file from before this
    /// field loads with every age unconfirmed.
    #[serde(default)]
    pub ages: BTreeMap<String, AgeMark>,
    /// The lists that the draft run cut at their cap.
    #[serde(default)]
    pub cut: Vec<Cut>,
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
            ages: BTreeMap::new(),
            cut: vec![],
            last_failure: None,
            campaign_file: None,
        }
    }
}

/// The age of one character (data shape 3.8). An age is confirmed when
/// `confirmed` holds the current age, so any change of the age removes the
/// confirmation, whatever path changed it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgeMark {
    /// The age the draft stage gave; `None` for an entry the Player added.
    pub drafted: Option<u16>,
    /// The model says the Source Text states the age. Information only.
    pub stated: bool,
    /// The age the Player confirmed through `confirm_age`.
    pub confirmed: Option<u16>,
}

/// Where the current age of a character comes from. Derived, never stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgeSource {
    /// The draft's age, untouched, and the model says the text states it.
    FromText,
    /// The draft's age, untouched, and the model proposed it.
    Proposed,
    /// The Player changed the age, or added the entry.
    Edited,
}

/// One character's age as the review shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgeLine {
    /// The main name, as the seed writes it.
    pub name: String,
    pub source: AgeSource,
    pub confirmed: bool,
}

/// A list that the draft run cut at its cap. `at` has the path form of
/// `Gap.at`: `entities`, `knowledge`, `entities[2].other_names`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cut {
    pub at: String,
    pub cap: u32,
}

/// Why a review action changed nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewError {
    /// No entry of the right kind has this main name.
    UnknownName,
    /// Another entry has this name, main or other, or it is "world".
    NameInUse,
    /// The new name is empty, or the character has no age to confirm.
    Empty,
}

impl fmt::Display for ReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownName => "no entry of the draft has that name",
            Self::NameInUse => "another entry has that name",
            Self::Empty => "the value is empty",
        })
    }
}

impl std::error::Error for ReviewError {}

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
    /// The Player has not confirmed the current age through
    /// `confirm_age`, whatever the model says about the text.
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
    if age_waits(draft, &pc.name, Some(pc.age)) {
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
                if age_waits(draft, &entity.name, Some(age)) {
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
/// seed and the knowledge, and returns the draft to `Draft`. It cannot
/// confirm an age: it does not take the age marks. A mark whose character
/// left the roster is dropped, so a name that comes back is a new entry and
/// needs a new confirmation. The last failure and the Campaign file belong
/// to the build of the content it replaces, so both are cleared. `cut`
/// stays. The caller then saves.
pub fn apply_edit(draft: &mut SeedDraft, seed: AdventureSeed, knowledge: Vec<KnownFact>) {
    draft.seed = seed;
    draft.knowledge = knowledge;
    let names: HashSet<String> = characters(&draft.seed)
        .map(|(name, _)| normalize::name(name))
        .collect();
    draft.ages.retain(|name, _| names.contains(name));
    edited(draft);
}

/// Confirms the current age of the character whose main name is `name`
/// (through `normalize::name`). The only writer of `AgeMark.confirmed`.
pub fn confirm_age(draft: &mut SeedDraft, name: &str) -> Result<(), ReviewError> {
    let key = normalize::name(name);
    let (_, age) = characters(&draft.seed)
        .find(|(main, _)| !key.is_empty() && normalize::name(main) == key)
        .ok_or(ReviewError::UnknownName)?;
    let age = age.ok_or(ReviewError::Empty)?;
    draft.ages.entry(key).or_default().confirmed = Some(age);
    Ok(())
}

/// Renames the entry whose main name is `from` (through `normalize::name`)
/// to `to`, in every field that names it: the main name, each `known_to`,
/// `start.place`, `start.present`, the subject of each known, decided and
/// proposed fact, and the key of its age mark. The old name, with the same
/// spelling and as whole words, also changes in the text of each known fact
/// and in `start.situation`. A name that another entry has, main or other,
/// is refused, and the draft does not change. A change of case or accent
/// of the same name is allowed.
pub fn rename(draft: &mut SeedDraft, from: &str, to: &str) -> Result<(), ReviewError> {
    let to = to.trim();
    let (from_key, to_key) = (normalize::name(from), normalize::name(to));
    if to_key.is_empty() {
        return Err(ReviewError::Empty);
    }
    let seed = &draft.seed;
    // `None`: the PC; `Some(i)`: `entities[i]`.
    let owner = if !from_key.is_empty() && normalize::name(&seed.pc.name) == from_key {
        None
    } else {
        let found = seed
            .entities
            .iter()
            .position(|e| !from_key.is_empty() && normalize::name(&e.name) == from_key);
        Some(found.ok_or(ReviewError::UnknownName)?)
    };
    let pc = std::iter::once((None, &seed.pc.name, &seed.pc.other_names));
    let entities = seed
        .entities
        .iter()
        .enumerate()
        .map(|(i, e)| (Some(i), &e.name, &e.other_names));
    let taken = pc.chain(entities).any(|(entry, main, others)| {
        let main_taken = entry != owner && normalize::name(main) == to_key;
        main_taken || others.iter().any(|o| normalize::name(&o.name) == to_key)
    });
    if taken || to_key == "world" {
        return Err(ReviewError::NameInUse);
    }

    let old = match owner {
        None => std::mem::replace(&mut draft.seed.pc.name, to.to_owned()),
        Some(i) => std::mem::replace(&mut draft.seed.entities[i].name, to.to_owned()),
    };
    let follow = |name: &mut String| {
        if normalize::name(name) == from_key {
            *name = to.to_owned();
        }
    };
    let seed = &mut draft.seed;
    let others = seed.pc.other_names.iter_mut().chain(
        seed.entities
            .iter_mut()
            .flat_map(|e| e.other_names.iter_mut()),
    );
    others.flat_map(|o| o.known_to.iter_mut()).for_each(follow);
    follow(&mut seed.start.place);
    seed.start.present.iter_mut().for_each(follow);
    let facts = seed.decided.iter_mut().chain(seed.proposed.iter_mut());
    facts.for_each(|f| follow(&mut f.subject));
    seed.start.situation = replace_words(&seed.start.situation, &old, to);
    for fact in &mut draft.knowledge {
        follow(&mut fact.subject);
        fact.text = replace_words(&fact.text, &old, to);
    }
    if let Some(mark) = draft.ages.remove(&from_key) {
        draft.ages.insert(to_key, mark);
    }
    edited(draft);
    Ok(())
}

/// The age of each character, the PC first, in roster order.
pub fn age_lines(draft: &SeedDraft) -> Vec<AgeLine> {
    characters(&draft.seed)
        .map(|(name, age)| {
            let mark = draft.ages.get(&normalize::name(name));
            let source = match mark {
                Some(m) if m.drafted.is_some() && m.drafted == age => {
                    if m.stated {
                        AgeSource::FromText
                    } else {
                        AgeSource::Proposed
                    }
                }
                _ => AgeSource::Edited,
            };
            AgeLine {
                name: name.to_owned(),
                source,
                confirmed: age_confirmed(draft, name, age),
            }
        })
        .collect()
}

/// A Player edit returns the draft to `Draft`. The last failure and the
/// Campaign file belong to the content it replaces.
fn edited(draft: &mut SeedDraft) {
    draft.state = DraftState::Draft;
    draft.last_failure = None;
    draft.campaign_file = None;
}

/// True when the Player confirmed `age` for the character `name`. A
/// character with no name is a `Missing` gap: its age waits for the name.
fn age_waits(draft: &SeedDraft, name: &str, age: Option<u16>) -> bool {
    !normalize::name(name).is_empty() && !age_confirmed(draft, name, age)
}

/// True when the Player confirmed `age` for the character `name`.
fn age_confirmed(draft: &SeedDraft, name: &str, age: Option<u16>) -> bool {
    age.is_some()
        && draft
            .ages
            .get(&normalize::name(name))
            .is_some_and(|m| m.confirmed == age)
}

/// The main name and age of each character, the PC first. An entry of kind
/// `Pc` counts: it is a gap, and its age still needs a confirmation.
fn characters(seed: &AdventureSeed) -> impl Iterator<Item = (&str, Option<u16>)> {
    let pc = std::iter::once((seed.pc.name.as_str(), Some(seed.pc.age)));
    let entities = seed
        .entities
        .iter()
        .filter(|e| is_character(e.kind))
        .map(|e| (e.name.as_str(), e.age));
    pc.chain(entities)
}

/// `text` with each whole-word `old`, in the same spelling, replaced by
/// `new`. A word is whole when no letter or digit touches it.
fn replace_words(text: &str, old: &str, new: &str) -> String {
    if old.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = 0;
    for (at, _) in text.match_indices(old) {
        if at < rest {
            continue;
        }
        let before = text[..at].chars().next_back();
        let after = text[at + old.len()..].chars().next();
        if before.is_some_and(char::is_alphanumeric) || after.is_some_and(char::is_alphanumeric) {
            continue;
        }
        out.push_str(&text[rest..at]);
        out.push_str(new);
        rest = at + old.len();
    }
    out.push_str(&text[rest..]);
    out
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
