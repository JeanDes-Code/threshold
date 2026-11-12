use std::{
    collections::{HashMap, HashSet},
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use super::BuildFailure;
use crate::{
    context::{ADVENTURE_BLOCK_MAX_CHARS, adventure_block_chars},
    domain::{
        AdventureSeed, ConfirmedAges, EntityKind, OtherName, SeedEntity, alias_key, is_open_key,
    },
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
    /// The age mark of each roster row: what the draft gave, and what the
    /// Player confirmed. Only `confirm_age` writes a confirmation. A draft
    /// file from before this field (its marks were under `ages`, keyed by
    /// name) loads with no mark, so every age needs a new confirmation.
    #[serde(default)]
    pub age_marks: AgeRows,
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
            age_marks: AgeRows::default(),
            cut: vec![],
            last_failure: None,
            campaign_file: None,
        }
    }
}

/// The age marks of a draft, by roster row: the PC, then one slot for each
/// row of `seed.entities`, in order. A mark counts for a row only while the
/// row has the mark's exact main name and kind (data shape 3.8).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgeRows {
    #[serde(default)]
    pub pc: Option<AgeMark>,
    /// One slot for each row of `seed.entities`; a missing slot is no mark.
    #[serde(default)]
    pub entities: Vec<Option<AgeMark>>,
}

impl AgeRows {
    /// The mark in the slot of a row, whatever entry the row now holds.
    pub fn get(&self, row: Option<usize>) -> Option<&AgeMark> {
        match row {
            None => self.pc.as_ref(),
            Some(i) => self.entities.get(i).and_then(Option::as_ref),
        }
    }

    /// The slot of a row, made when it is missing.
    pub fn slot(&mut self, row: Option<usize>) -> &mut Option<AgeMark> {
        match row {
            None => &mut self.pc,
            Some(i) => {
                if self.entities.len() <= i {
                    self.entities.resize(i + 1, None);
                }
                &mut self.entities[i]
            }
        }
    }
}

/// The age of one character entry (data shape 3.8). An age is confirmed
/// when the entry still has `name` and `kind` and its age equals
/// `confirmed`. So a change of the age, of the exact name or of the kind
/// removes the confirmation, whatever path changed it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgeMark {
    /// The entry's main name, exactly as written, when the mark was made or
    /// when `rename` renamed the entry.
    pub name: String,
    /// The entry's kind: `Pc` or `Npc`.
    pub kind: EntityKind,
    /// The age the draft stage gave; `None` for an entry the Player added.
    pub drafted: Option<u16>,
    /// The model says the Source Text states the age. Information only.
    pub stated: bool,
    /// The age the Player confirmed through `confirm_age`.
    pub confirmed: Option<u16>,
}

impl AgeMark {
    /// True while the row holds an entry with this exact main name and kind,
    /// and that entry is a character.
    fn fits(&self, name: &str, kind: EntityKind) -> bool {
        is_character(kind) && self.name == name && self.kind == kind
    }
}

/// One roster entry, as a review command names it: its row (`None` for the
/// PC, `Some(i)` for `seed.entities[i]`), and, as a check, its exact main
/// name and its kind. A command whose entry does not match the row changes
/// nothing: the screen is out of date.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryRef {
    pub index: Option<usize>,
    pub name: String,
    pub kind: EntityKind,
}

impl EntryRef {
    /// The entry of the PC with this main name.
    pub fn pc(name: &str) -> Self {
        Self {
            index: None,
            name: name.to_owned(),
            kind: EntityKind::Pc,
        }
    }

    /// The entry `seed.entities[index]` with this main name and kind.
    pub fn entity(index: usize, name: &str, kind: EntityKind) -> Self {
        Self {
            index: Some(index),
            name: name.to_owned(),
            kind,
        }
    }
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
    /// The row: `None` for the PC, `Some(i)` for `seed.entities[i]`.
    pub index: Option<usize>,
    pub kind: EntityKind,
    pub source: AgeSource,
    pub confirmed: bool,
}

impl AgeLine {
    /// The entry of this line, for `confirm_age` or `rename`.
    pub fn entry(&self) -> EntryRef {
        EntryRef {
            index: self.index,
            name: self.name.clone(),
            kind: self.kind,
        }
    }
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
    /// The row holds no entry of this exact main name and kind.
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
    /// fact on the same subject and key. `at` names the later entry, `with`
    /// the first entry that holds the slot; the first gets no gap of its own.
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
///
/// `with` names the other entry of a pair, in the same path form, so the
/// review can show both. Only two kinds set it. `DuplicateName`: the entry
/// that holds the name first (`entities[0].name`, `pc.other_names[1]`); none
/// for the reserved name "world". `SlotConflict`: the first entry that holds
/// the slot, a seed slot before any known fact (`decided[2]`,
/// `entities[0].other_names[1]`, `knowledge[0]`). With three facts on one
/// slot, the second and the third each name the first. A gap without
/// `with` serializes as before.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    pub kind: GapKind,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with: Option<String>,
}

/// Every gap of `draft`, in field order. Derived, never stored. It mirrors
/// the checks that `Store::create_campaign` runs on a seed, so the Player
/// sees them at review, before any commit. It never panics on an edited
/// draft.
pub fn gaps(draft: &SeedDraft) -> Vec<Gap> {
    let seed = &draft.seed;
    let mut out = Vec::new();
    let mut gap = |kind, at: String| {
        out.push(Gap {
            kind,
            at,
            with: None,
        })
    };
    // The other entry of each pair gap, by kind and `at`; set at the end.
    let mut pairs: Vec<(GapKind, String, String)> = Vec::new();
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
    if age_waits(draft, None, Some(pc.age)) {
        gap(GapKind::AgeUnconfirmed, "pc.age".into());
    }
    let pc_names: HashSet<String> = names(&pc.name, &pc.other_names)
        .map(|(n, _)| normalize::name(n))
        .filter(|n| !n.is_empty())
        .collect();
    // Each name, normalized, and the path of the entry that holds it first.
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut duplicate = |norm: String, at: String| -> bool {
        if norm == "world" {
            return true;
        }
        match seen.get(&norm) {
            Some(first) => {
                pairs.push((GapKind::DuplicateName, at, first.clone()));
                true
            }
            None => {
                seen.insert(norm, at);
                false
            }
        }
    };
    for (name, at) in names(&pc.name, &pc.other_names) {
        let at = at.map_or_else(|| "pc.name".to_owned(), |j| format!("pc.other_names[{j}]"));
        let norm = normalize::name(name);
        if norm.is_empty() {
            if at != "pc.name" {
                gap(GapKind::Missing, at);
            }
        } else if duplicate(norm, at.clone()) {
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
            } else if duplicate(norm, at.clone()) {
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
                if age_waits(draft, Some(i), Some(age)) {
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
    let mut holders = seed_slots(seed, &roster);
    for (i, fact) in draft.knowledge.iter().enumerate() {
        let (Some(owner), key) = (roster.owner(&fact.subject), normalize::key(&fact.key)) else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        let slot = (owner, key);
        let at = format!("knowledge[{i}]");
        if let Some((_, first)) = holders.iter().find(|(s, _)| *s == slot) {
            pairs.push((GapKind::SlotConflict, at.clone(), first.clone()));
            gap(GapKind::SlotConflict, at.clone());
        }
        holders.push((slot, at));
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
    for (kind, at, with) in pairs {
        if let Some(g) = out.iter_mut().find(|g| g.kind == kind && g.at == at) {
            g.with = Some(with);
        }
    }
    out
}

/// Approves a draft with no gap. On a gap, returns every gap; an
/// `Approved` draft goes back to `Draft`, so a file that says `approved`
/// with an age the Player never confirmed cannot stay approved. A `Built`
/// draft stays `Built`.
pub fn approve(draft: &mut SeedDraft) -> Result<(), Vec<Gap>> {
    let found = gaps(draft);
    if !found.is_empty() {
        if draft.state == DraftState::Approved {
            draft.state = DraftState::Draft;
        }
        return Err(found);
    }
    if draft.state == DraftState::Draft {
        draft.state = DraftState::Approved;
    }
    Ok(())
}

/// The ages the Player confirmed, for the Campaign file (data shape 3.2).
/// It computes the gaps again and fails on any gap, whatever state the
/// draft file says: an old file that says `approved` gives no confirmed
/// age. Each entry is a character's kind, exact main name and the age the
/// Player confirmed for it.
pub fn confirmed_ages(draft: &SeedDraft) -> Result<ConfirmedAges, Vec<Gap>> {
    let found = gaps(draft);
    if !found.is_empty() {
        return Err(found);
    }
    let entries = characters(&draft.seed)
        .filter_map(|(row, name, kind, age)| {
            let age = age.filter(|_| age_confirmed(draft, row, age))?;
            Some((kind, name.to_owned(), age))
        })
        .collect();
    Ok(ConfirmedAges::new(entries))
}

/// A Player edit of the draft (the `save_draft` command): it replaces the
/// seed and the knowledge, and returns the draft to `Draft`. It cannot
/// confirm an age: it does not take the age marks. It keeps the mark of a
/// row only while the row holds a character with the mark's exact main name
/// and kind, so a renamed entry, an entry that became a place, or a name
/// that comes back is a new entry and needs a new confirmation.
///
/// Age marks follow rows by position. A save that removes exactly one row and
/// leaves every other row as it was moves the marks of the later rows up
/// by one. Any other change of the row count keeps the positions, so a
/// later row whose entry changed loses its mark. The last failure and the
/// Campaign file belong to the build of the content it replaces, so both
/// are cleared. `cut` stays. The caller then saves.
pub fn apply_edit(draft: &mut SeedDraft, seed: AdventureSeed, knowledge: Vec<KnownFact>) {
    let old = std::mem::replace(&mut draft.seed, seed);
    draft.knowledge = knowledge;
    let from = old_rows(&old.entities, &draft.seed.entities);
    let from_rows = from.clone();
    let marks = std::mem::take(&mut draft.age_marks);
    let pc = &draft.seed.pc;
    draft.age_marks = AgeRows {
        pc: marks.pc.filter(|m| m.fits(&pc.name, EntityKind::Pc)),
        entities: draft
            .seed
            .entities
            .iter()
            .zip(from)
            .map(|(entity, from)| {
                from.and_then(|j| marks.entities.get(j).cloned().flatten())
                    .filter(|m| m.fits(&entity.name, entity.kind))
            })
            .collect(),
    };
    // Only a row whose age changed in this edit, or whose age the Player
    // confirmed, rewrites its age facts (changed after review): an edit of
    // another field never puts an unconfirmed age over an age fact that
    // the Source Text states.
    let pc_changed = old.pc.age != draft.seed.pc.age || old.pc.name != draft.seed.pc.name;
    let rows: Vec<(Option<usize>, bool)> = characters(&draft.seed)
        .map(|(row, name, kind, age)| {
            let changed = match row {
                None => pc_changed,
                Some(k) => from_rows[k]
                    .and_then(|j| old.entities.get(j))
                    .is_none_or(|was| was.age != age || was.name != name || was.kind != kind),
            };
            (row, changed)
        })
        .collect();
    for (row, changed) in rows {
        let age = row_of(&draft.seed, row).and_then(|(_, _, age)| age);
        if changed {
            set_age_facts(draft, row, true);
        } else if age_confirmed(draft, row, age) {
            set_age_facts(draft, row, false);
        }
    }
    edited(draft);
}

/// The draft keeps one truth for an age (QA, 2026-11-10): each known fact
/// with key `age` about the entry of `row` gets the entry's age as its
/// value, and a plain sentence as its text. `always` rewrites them all;
/// else only a fact whose value is not the age. An entry with no age, or
/// with no name, changes nothing.
fn set_age_facts(draft: &mut SeedDraft, row: Option<usize>, always: bool) {
    let (name, others, age) = match row {
        None => (
            &draft.seed.pc.name,
            &draft.seed.pc.other_names,
            Some(draft.seed.pc.age),
        ),
        Some(i) => match draft.seed.entities.get(i) {
            Some(e) if is_character(e.kind) => (&e.name, &e.other_names, e.age),
            _ => return,
        },
    };
    let Some(age) = age else {
        return;
    };
    let names: Vec<String> = std::iter::once(name.as_str())
        .chain(others.iter().map(|o| o.name.as_str()))
        .map(normalize::name)
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        return;
    }
    let (value, text) = (age.to_string(), format!("{name} is {age} years old."));
    for fact in &mut draft.knowledge {
        let about = names.contains(&normalize::name(&fact.subject));
        if about && normalize::key(&fact.key) == "age" && (always || fact.value.trim() != value) {
            fact.value.clone_from(&value);
            fact.text.clone_from(&text);
        }
    }
}

/// For each new row, the old row whose mark it may keep. A pure removal of
/// one row shifts the later rows; any other edit keeps the positions.
fn old_rows(old: &[SeedEntity], new: &[SeedEntity]) -> Vec<Option<usize>> {
    if new.len() + 1 == old.len() {
        let i = (0..new.len())
            .find(|&k| new[k] != old[k])
            .unwrap_or(new.len());
        if old[..i] == new[..i] && old[i + 1..] == new[i..] {
            return (0..new.len())
                .map(|k| Some(if k < i { k } else { k + 1 }))
                .collect();
        }
    }
    (0..new.len())
        .map(|k| (k < old.len()).then_some(k))
        .collect()
}

/// Confirms the current age of one character entry. The only writer of
/// `AgeMark.confirmed`. The entry must match its row exactly (`EntryRef`).
pub fn confirm_age(draft: &mut SeedDraft, entry: &EntryRef) -> Result<(), ReviewError> {
    let row = resolve(&draft.seed, entry)?;
    let (name, kind, age) = row_of(&draft.seed, row).ok_or(ReviewError::UnknownName)?;
    if !is_character(kind) || normalize::name(name).is_empty() {
        return Err(ReviewError::UnknownName);
    }
    let age = age.ok_or(ReviewError::Empty)?;
    let name = name.to_owned();
    let slot = draft.age_marks.slot(row);
    match slot {
        Some(mark) if mark.fits(&name, kind) => mark.confirmed = Some(age),
        _ => {
            *slot = Some(AgeMark {
                name,
                kind,
                drafted: None,
                stated: false,
                confirmed: Some(age),
            });
        }
    }
    set_age_facts(draft, row, true);
    Ok(())
}

/// Renames one entry to `to`, in every field that names it: the main name,
/// each `known_to`, `start.place`, `start.present`, the subject of each
/// known, decided and proposed fact, and the name in its own age mark. A
/// reference matches the old name through `normalize::name`. The old name,
/// with the same spelling and as whole words, also changes in the text of
/// each known fact and in `start.situation`. A name that another entry has,
/// main or other, is refused, and the draft does not change. A change of
/// case or accent of the same name is allowed. Only the mark of this very
/// row moves to the new name.
pub fn rename(draft: &mut SeedDraft, entry: &EntryRef, to: &str) -> Result<(), ReviewError> {
    let to = to.trim();
    let owner = resolve(&draft.seed, entry)?;
    let from_key = normalize::name(&entry.name);
    let to_key = normalize::name(to);
    if from_key.is_empty() {
        return Err(ReviewError::UnknownName);
    }
    if to_key.is_empty() {
        return Err(ReviewError::Empty);
    }
    let seed = &draft.seed;
    let pc = std::iter::once((None, &seed.pc.name, &seed.pc.other_names));
    let entities = seed
        .entities
        .iter()
        .enumerate()
        .map(|(i, e)| (Some(i), &e.name, &e.other_names));
    let taken = pc.chain(entities).any(|(row, main, others)| {
        let main_taken = row != owner && normalize::name(main) == to_key;
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
    if draft
        .age_marks
        .get(owner)
        .is_some_and(|m| m.fits(&old, entry.kind))
        && let Some(mark) = draft.age_marks.slot(owner)
    {
        mark.name = to.to_owned();
    }
    set_age_facts(draft, owner, true);
    edited(draft);
    Ok(())
}

/// The age of each character, the PC first, in roster order.
pub fn age_lines(draft: &SeedDraft) -> Vec<AgeLine> {
    characters(&draft.seed)
        .map(|(index, name, kind, age)| {
            let source = match mark_of(draft, index) {
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
                index,
                kind,
                source,
                confirmed: age_confirmed(draft, index, age),
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

/// The row of `entry`, when the row holds an entry with that exact main
/// name and kind.
fn resolve(seed: &AdventureSeed, entry: &EntryRef) -> Result<Option<usize>, ReviewError> {
    match row_of(seed, entry.index) {
        Some((name, kind, _)) if name == entry.name && kind == entry.kind => Ok(entry.index),
        _ => Err(ReviewError::UnknownName),
    }
}

/// The main name, kind and age of a row: `None` for the PC.
fn row_of(seed: &AdventureSeed, row: Option<usize>) -> Option<(&str, EntityKind, Option<u16>)> {
    match row {
        None => Some((&seed.pc.name, EntityKind::Pc, Some(seed.pc.age))),
        Some(i) => seed
            .entities
            .get(i)
            .map(|e| (e.name.as_str(), e.kind, e.age)),
    }
}

/// The mark of a row, while it belongs to the entry the row holds.
fn mark_of(draft: &SeedDraft, row: Option<usize>) -> Option<&AgeMark> {
    let (name, kind, _) = row_of(&draft.seed, row)?;
    draft.age_marks.get(row).filter(|m| m.fits(name, kind))
}

/// True when the age of a row waits for the Player. A character with no
/// name is a `Missing` gap: its age waits for the name.
fn age_waits(draft: &SeedDraft, row: Option<usize>, age: Option<u16>) -> bool {
    row_of(&draft.seed, row).is_some_and(|(name, _, _)| !normalize::name(name).is_empty())
        && !age_confirmed(draft, row, age)
}

/// True when the Player confirmed `age` for the entry the row holds.
fn age_confirmed(draft: &SeedDraft, row: Option<usize>, age: Option<u16>) -> bool {
    age.is_some() && mark_of(draft, row).is_some_and(|m| m.confirmed == age)
}

/// The row, main name, kind and age of each character, the PC first. An
/// entry of kind `Pc` counts: it is a gap, and its age still needs a
/// confirmation.
fn characters(
    seed: &AdventureSeed,
) -> impl Iterator<Item = (Option<usize>, &str, EntityKind, Option<u16>)> {
    let pc = std::iter::once((
        None,
        seed.pc.name.as_str(),
        EntityKind::Pc,
        Some(seed.pc.age),
    ));
    let entities = seed
        .entities
        .iter()
        .enumerate()
        .filter(|(_, e)| is_character(e.kind))
        .map(|(i, e)| (Some(i), e.name.as_str(), e.kind, e.age));
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
/// entry and a normalized key; each comes with the `Gap.at` path of the
/// field that holds it.
fn seed_slots(seed: &AdventureSeed, roster: &Roster) -> Vec<((usize, String), String)> {
    let pc = std::iter::once((0, "pc".to_owned(), &seed.pc.other_names));
    let entities = seed
        .entities
        .iter()
        .enumerate()
        .map(|(i, e)| (i + 1, format!("entities[{i}]"), &e.other_names));
    let aliases = pc.chain(entities).flat_map(|(owner, path, others)| {
        others.iter().enumerate().map(move |(j, o)| {
            let key = alias_key(&o.name);
            (
                (owner, normalize::key(&key)),
                format!("{path}.other_names[{j}]"),
            )
        })
    });
    let decided = seed.decided.iter().enumerate().filter_map(|(k, f)| {
        roster
            .owner(&f.subject)
            .map(|o| ((o, normalize::key(&f.key)), format!("decided[{k}]")))
    });
    aliases.chain(decided).collect()
}
