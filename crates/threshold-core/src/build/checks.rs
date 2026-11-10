//! The checks of design 6.5 on each World Build stage output, the merge
//! rule of data shape 3.6, and how a World Build repair note is written.
//!
//! The Player never reads the World Build, so these checks are its only
//! reviewer. A repair note never quotes model text: see `Note`.

use std::{collections::HashSet, sync::LazyLock};

use super::{
    BuildCheck,
    draft_stages::open_key,
    stages::{Problem, StageItem, Step},
    world_stages::{EventsOut, FactOut, GoalOut, HiddenOut, NpcOut, NpcState, StartOut},
};
use crate::{
    checks::{PC_INVOLUNTARY_KEYS, is_involuntary_key},
    domain::{AdventureSeed, EntityKind, FactEntry, OtherName, alias_key, is_open_key},
    normalize,
};

// ---- Roster ------------------------------------------------------------------

/// One entry of the approved roster.
pub(super) struct Member {
    pub main: String,
    pub kind: EntityKind,
    pub age: Option<u16>,
    pub others: Vec<String>,
    pub hidden: bool,
}

/// The approved roster: the PC first, then `seed.entities`. A name resolves
/// as `WorldView::entity_by_name` does: main or other name, through
/// `normalize::name`, the first entry on a tie.
pub(super) struct Roster {
    members: Vec<Member>,
    names: Vec<(String, usize)>,
    /// The word sequences that name the PC (`pc_terms`), built once.
    pc_terms: Vec<Vec<String>>,
}

impl Roster {
    pub fn of(seed: &AdventureSeed) -> Self {
        let others = |names: &[OtherName]| names.iter().map(|o| o.name.clone()).collect();
        let pc = Member {
            main: seed.pc.name.clone(),
            kind: EntityKind::Pc,
            age: Some(seed.pc.age),
            others: others(&seed.pc.other_names),
            hidden: false,
        };
        let members: Vec<Member> = std::iter::once(pc)
            .chain(seed.entities.iter().map(|e| Member {
                main: e.name.clone(),
                kind: e.kind,
                age: e.age,
                others: others(&e.other_names),
                hidden: e.hidden,
            }))
            .collect();
        let names = members
            .iter()
            .enumerate()
            .flat_map(|(i, m)| {
                std::iter::once(&m.main)
                    .chain(&m.others)
                    .map(move |n| (normalize::name(n), i))
            })
            .filter(|(n, _)| !n.is_empty())
            .collect();
        let rest: Vec<&str> = members[1..]
            .iter()
            .flat_map(|m| std::iter::once(&m.main).chain(&m.others))
            .map(String::as_str)
            .collect();
        let pc_terms = pc_terms(&members[0].main, &members[0].others, &rest);
        Self {
            members,
            names,
            pc_terms,
        }
    }

    pub fn find(&self, name: &str) -> Option<&Member> {
        let name = normalize::name(name);
        self.names
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, i)| &self.members[*i])
    }

    pub fn pc(&self) -> &Member {
        &self.members[0]
    }

    pub fn members(&self) -> &[Member] {
        &self.members
    }

    pub fn npcs(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|m| m.kind == EntityKind::Npc)
    }

    /// The main name that `name` stands for, or `name` itself.
    pub fn main(&self, name: &str) -> String {
        self.find(name)
            .map_or_else(|| name.to_owned(), |m| m.main.clone())
    }

    /// True when `text` names the PC: see `names_pc`.
    pub fn names_pc(&self, text: &str) -> bool {
        holds(&self.pc_terms, text)
    }

    /// True when a goal or a secret centres on the PC: see `centres_on_pc`.
    pub fn centres_on_pc(&self, text: &str, target: Option<&str>) -> bool {
        let pc = self.pc();
        self.names_pc(text) || target.is_some_and(|t| is_pc_name(&pc.main, &pc.others, t))
    }
}

/// True when a goal or a secret of an NPC centres on the PC whose main name
/// is `main`: its text names the PC (`names_pc`), or its `target` is a name
/// of the PC after `normalize::name`. The target is a goal's `against` or a
/// secret's subject, so "Ruin him" against the PC counts. The WorldAutonomy
/// check and the evaluation count PC-centred goals and secrets with it.
pub fn centres_on_pc(
    main: &str,
    others: &[String],
    roster: &[&str],
    text: &str,
    target: Option<&str>,
) -> bool {
    names_pc(main, others, roster, text) || target.is_some_and(|t| is_pc_name(main, others, t))
}

/// True when `name` is the main name or an other name of the PC.
fn is_pc_name(main: &str, others: &[String], name: &str) -> bool {
    let name = normalize::name(name);
    !name.is_empty()
        && std::iter::once(main)
            .chain(others.iter().map(String::as_str))
            .any(|n| normalize::name(n) == name)
}

/// True when `text` names the PC whose main name is `main`: it holds, as
/// whole words after `normalize::name`, the full main name, a full name of
/// `others`, or the first word of the main name ("Teodor" names "Teodor
/// Brandt"). The first word counts alone only when no name of `roster` (the
/// main and other names of every other entity) holds it as a whole word:
/// with an NPC "Teodor Vane", "Teodor" no longer points at the PC. A
/// possessive "'s" still names the PC; a longer or hyphenated word does not
/// ("Theodora", "Teodor-Brandtson"). The WorldAutonomy check and the
/// evaluation count PC-centred goals and secrets with it.
pub fn names_pc(main: &str, others: &[String], roster: &[&str], text: &str) -> bool {
    holds(&pc_terms(main, others, roster), text)
}

/// The word sequences that name the PC: see `names_pc`.
fn pc_terms(main: &str, others: &[String], roster: &[&str]) -> Vec<Vec<String>> {
    let main = words(main);
    let first = main
        .first()
        .filter(|first| !roster.iter().any(|name| words(name).contains(first)))
        .map(|first| vec![first.clone()]);
    std::iter::once(main.clone())
        .chain(others.iter().map(|n| words(n)))
        .chain(first)
        .filter(|term| !term.is_empty())
        .collect()
}

/// True when `text` holds one of `terms` as whole words.
fn holds(terms: &[Vec<String>], text: &str) -> bool {
    let said = words(text);
    terms
        .iter()
        .any(|term| said.windows(term.len()).any(|w| w == term.as_slice()))
}

/// The whole words of `text` after `normalize::name`. A word holds letters,
/// digits, and inner apostrophes and hyphens; a final "'s" is dropped.
fn words(text: &str) -> Vec<String> {
    let inner = |c: char| c == '\'' || c == '-';
    normalize::name(text)
        .split(|c: char| !c.is_alphanumeric() && !inner(c))
        .map(|w| w.trim_matches(inner))
        .map(|w| w.strip_suffix("'s").unwrap_or(w))
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

// ---- Repair notes ------------------------------------------------------------

/// A repair note of a World Build stage. It is made only of field names and
/// rules that code wrote (`&'static str`), item indexes, and main names from
/// the approved roster. So it can never quote a hidden fact, a secret, a
/// key, a value or a name that the model invented.
pub(super) struct Note(String);

impl Note {
    /// An item of a list of the answer: `facts[3]`.
    pub fn item(list: &'static str, index: usize) -> Self {
        Self(format!("{list}[{index}]"))
    }

    /// A part of the answer that is not in a list: `voice_card`.
    pub fn part(name: &'static str) -> Self {
        Self(name.to_owned())
    }

    /// A field of the item: `facts[3].subject`.
    pub fn field(mut self, name: &'static str) -> Self {
        self.0.push('.');
        self.0.push_str(name);
        self
    }

    /// An index inside the field: `facts[3].known_by[1]`.
    pub fn at(mut self, index: usize) -> Self {
        self.0.push_str(&format!("[{index}]"));
        self
    }

    /// The roster name that the item is about.
    pub fn of(mut self, member: &Member) -> Self {
        self.0.push_str(&format!(" ({})", member.main));
        self
    }

    pub fn path(self) -> String {
        self.0
    }

    pub fn says(self, rule: &'static str) -> String {
        format!("{}: {rule}", self.0)
    }
}

// ---- The merge rule ----------------------------------------------------------

/// Every fact the build has accepted, at most one per store slot. A slot is
/// the subject, the key and whether the item is open: `setup` writes an open
/// item on `pending_<key>`.
#[derive(Clone, Default)]
pub(super) struct FactBook {
    entries: Vec<Booked>,
}

#[derive(Clone)]
struct Booked {
    entry: FactEntry,
    /// A fact that the seed's Setup Turn writes (an other name, a decided
    /// fact). It holds its slot, but the World Setup never carries it.
    fixed: bool,
}

enum Fit {
    New,
    Same(usize),
    Clash(BuildCheck),
}

impl FactBook {
    /// A book that holds the slots the seed's Setup Turn writes.
    pub fn of_seed(seed: &AdventureSeed, roster: &Roster) -> Self {
        let mut book = Self::default();
        let mut fixed = |entry| book.entries.push(Booked { entry, fixed: true });
        let members = roster.members().iter();
        for member in members {
            for other in &member.others {
                let key = alias_key(other);
                fixed(fact(&member.main, &key, other, vec![]));
            }
        }
        for decided in &seed.decided {
            if let Some(member) = roster.find(&decided.subject) {
                fixed(fact(&member.main, &decided.key, &decided.value, vec![]));
            }
        }
        book
    }

    /// The check that `entry` fails against the accepted facts, if any.
    pub fn clash(&self, entry: &FactEntry) -> Option<BuildCheck> {
        match self.fit(entry) {
            Fit::Clash(check) => Some(check),
            _ => None,
        }
    }

    /// Accepts `entry`. The same slot and value adds its holders and adds no
    /// fact. A clash adds nothing and returns false.
    pub fn add(&mut self, entry: FactEntry) -> bool {
        match self.fit(&entry) {
            Fit::New => self.entries.push(Booked {
                entry,
                fixed: false,
            }),
            Fit::Same(i) if !self.entries[i].fixed => {
                let known_by = &mut self.entries[i].entry.known_by;
                for holder in entry.known_by {
                    if !known_by.iter().any(|h| same_name(h, &holder)) {
                        known_by.push(holder);
                    }
                }
            }
            Fit::Same(_) => {}
            Fit::Clash(_) => return false,
        }
        true
    }

    /// Accepts a known fact of the approved draft. A known fact on a slot of
    /// the seed's Setup Turn fails `SettledFacts`; a clash with an earlier
    /// known fact fails its check. `gaps` reports both as `SlotConflict`, so
    /// `approve` refuses them: this guards a draft approved some other way.
    pub fn known(&mut self, entry: FactEntry) -> Result<(), BuildCheck> {
        let seed_slot = self.entries.iter().any(|b| {
            b.fixed
                && same_name(&b.entry.subject, &entry.subject)
                && normalize::key(&b.entry.key) == normalize::key(&entry.key)
        });
        if seed_slot {
            return Err(BuildCheck::SettledFacts);
        }
        match self.clash(&entry) {
            Some(check) => Err(check),
            None => {
                self.add(entry);
                Ok(())
            }
        }
    }

    /// Writes a `location` that code derives from the start state. It
    /// replaces a known fact on its slot: the start state is the latest
    /// state. A slot of the seed's Setup Turn stays.
    pub fn put(&mut self, entry: FactEntry) {
        match self
            .entries
            .iter()
            .position(|b| same_slot(&b.entry, &entry))
        {
            Some(i) if self.entries[i].fixed => {}
            Some(i) => self.entries[i].entry = entry,
            None => self.entries.push(Booked {
                entry,
                fixed: false,
            }),
        }
    }

    /// The accepted facts, in the order they came, without the seed's.
    pub fn facts(&self) -> impl Iterator<Item = &FactEntry> {
        self.entries.iter().filter(|b| !b.fixed).map(|b| &b.entry)
    }

    pub fn into_facts(self) -> Vec<FactEntry> {
        self.entries
            .into_iter()
            .filter(|b| !b.fixed)
            .map(|b| b.entry)
            .collect()
    }

    /// A same subject and key decides: the same openness and value is the
    /// same fact; another value fails `SettledFacts`; another openness fails
    /// `OpenItems`.
    fn fit(&self, entry: &FactEntry) -> Fit {
        let on_key = |b: &&Booked| {
            same_name(&b.entry.subject, &entry.subject)
                && normalize::key(&b.entry.key) == normalize::key(&entry.key)
        };
        let mut other_openness = false;
        for (i, booked) in self.entries.iter().enumerate() {
            if !on_key(&booked) {
                continue;
            }
            if booked.entry.open != entry.open {
                other_openness = true;
            } else if normalize::value(&booked.entry.value) == normalize::value(&entry.value) {
                return Fit::Same(i);
            } else {
                return Fit::Clash(BuildCheck::SettledFacts);
            }
        }
        if other_openness {
            Fit::Clash(BuildCheck::OpenItems)
        } else {
            Fit::New
        }
    }
}

fn same_name(a: &str, b: &str) -> bool {
    normalize::name(a) == normalize::name(b)
}

fn same_slot(a: &FactEntry, b: &FactEntry) -> bool {
    same_name(&a.subject, &b.subject)
        && normalize::key(&a.key) == normalize::key(&b.key)
        && a.open == b.open
}

fn fact(subject: &str, key: &str, value: &str, known_by: Vec<String>) -> FactEntry {
    FactEntry {
        subject: subject.to_owned(),
        key: key.to_owned(),
        value: value.to_owned(),
        text: String::new(),
        known_by,
        open: false,
    }
}

// ---- Facts: hidden facts and secrets -----------------------------------------

/// A model fact as a `FactEntry`: main names, the key without a `pending_`
/// prefix, and `holders` (main names, once each) as `known_by`.
pub(super) fn fact_entry(roster: &Roster, out: &FactOut, holders: &[String]) -> FactEntry {
    let (key, open) = open_key(&out.key, out.open);
    let mut known_by: Vec<String> = Vec::new();
    for holder in holders.iter().map(|h| roster.main(h)) {
        if !known_by.iter().any(|h| same_name(h, &holder)) {
            known_by.push(holder);
        }
    }
    FactEntry {
        subject: roster.main(&out.subject),
        key,
        value: out.value.clone(),
        text: out.text.clone(),
        known_by,
        open,
    }
}

/// Schema, References, Kinds, Secrecy, Agency, then the merge rule, on one
/// fact. `note` locates the fact in the answer.
fn fact_problems(
    roster: &Roster,
    book: &FactBook,
    out: &FactOut,
    holders: &[String],
    note: &dyn Fn() -> Note,
) -> Vec<(BuildCheck, String)> {
    let mut found = vec![];
    let empty = [&out.subject, &out.value, &out.text]
        .iter()
        .any(|t| t.trim().is_empty())
        || open_key(&out.key, out.open).0.is_empty();
    if empty {
        let rule = "needs a subject, a key, a value and a text";
        return vec![(BuildCheck::Schema, note().says(rule))];
    }
    let subject = roster.find(&out.subject);
    if subject.is_none() {
        let rule = "is not a name from the ROSTER";
        found.push((BuildCheck::References, note().field("subject").says(rule)));
    }
    for (j, holder) in holders.iter().enumerate() {
        let at = || note().field("known_by").at(j);
        match roster.find(holder) {
            None => found.push((
                BuildCheck::References,
                at().says("is not a name from the ROSTER"),
            )),
            Some(m) if m.kind == EntityKind::Pc => found.push((
                BuildCheck::Secrecy,
                at().says("is the PC; the PC never holds a hidden fact"),
            )),
            Some(m) if m.kind != EntityKind::Npc => found.push((
                BuildCheck::Kinds,
                at().of(m)
                    .says("is not a character; only an NPC holds a fact"),
            )),
            Some(_) => {}
        }
    }
    let Some(subject) = subject else {
        return found;
    };
    let key = open_key(&out.key, out.open).0;
    if subject.kind == EntityKind::Pc && !is_involuntary_key(&key) {
        found.push((
            BuildCheck::Agency,
            note().field("key").of(subject).says(&PC_KEYS_RULE),
        ));
    }
    if found.is_empty()
        && let Some(check) = book.clash(&fact_entry(roster, out, holders))
    {
        let rule = match check {
            BuildCheck::OpenItems => {
                "an accepted fact has this subject and key with the other open state"
            }
            _ => "an accepted fact has this subject and key with another value",
        };
        found.push((check, note().of(subject).says(rule)));
    }
    found
}

/// The rule of `checks::agency` on the keys of a write on the PC.
static PC_KEYS_RULE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "a fact or an effect on the PC may only use the keys {}",
        PC_INVOLUNTARY_KEYS.join(", ")
    )
});

/// The hidden facts of one section. Each problem drops one fact. A fact is
/// checked against the accepted facts and the facts before it that passed.
pub(super) fn hidden_facts(
    roster: &Roster,
    book: &FactBook,
    out: &HiddenOut,
) -> Vec<Problem<usize>> {
    let mut book = book.clone();
    let mut problems = vec![];
    for (i, fact) in out.facts.iter().enumerate() {
        let found = fact_problems(roster, &book, fact, &fact.known_by, &|| {
            Note::item("facts", i)
        });
        if found.is_empty() {
            book.add(fact_entry(roster, fact, &fact.known_by));
        }
        problems.extend(
            found
                .into_iter()
                .map(|(check, note)| Problem::optional(check, i, note)),
        );
    }
    problems
}

// ---- NpcState ----------------------------------------------------------------

/// The name a goal works against; an empty name means none.
pub(super) fn against(goal: &GoalOut) -> Option<&str> {
    goal.against.as_deref().filter(|a| !a.trim().is_empty())
}

/// An optional item of an NpcState answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NpcItem {
    Goal(usize),
    /// Only the `against` of a goal: the goal stays.
    Against(usize),
    Attitude(usize),
    Secret,
}

/// The item a problem at a path of an NpcState answer drops. A text of the
/// Voice Card is essential.
impl StageItem for NpcItem {
    fn at(path: &[Step]) -> Option<Self> {
        let field = |step: &Step, name: &str| matches!(step, Step::Field(f) if f == name);
        match path {
            [goals, Step::Index(i), against, ..]
                if field(goals, "goals") && field(against, "against") =>
            {
                Some(Self::Against(*i))
            }
            [goals, Step::Index(i), ..] if field(goals, "goals") => Some(Self::Goal(*i)),
            [attitudes, Step::Index(i), ..] if field(attitudes, "attitudes") => {
                Some(Self::Attitude(*i))
            }
            [secret, ..] if field(secret, "secret") => Some(Self::Secret),
            _ => None,
        }
    }
}

/// The most goals an NPC keeps.
const MAX_GOALS: usize = 3;
/// The fewest sample lines of a Voice Card.
const MIN_SAMPLES: usize = 2;

/// The checks on the hidden state of `npc`. Each goal is an optional item;
/// the NPC fails `Completeness` when no goal survives, or when its Voice
/// Card is incomplete. `book` holds the accepted facts and the secrets of
/// the other NPCs.
pub(super) fn npc_state(
    roster: &Roster,
    book: &FactBook,
    npc: &Member,
    out: &NpcOut,
) -> Vec<Problem<NpcItem>> {
    let mut problems = vec![];
    let mut kept = 0;
    for (i, goal) in out.goals.iter().enumerate() {
        let mut drop = |check, rule| {
            let note = Note::item("goals", i).says(rule);
            problems.push(Problem::optional(check, NpcItem::Goal(i), note));
        };
        if goal.text.trim().is_empty() {
            drop(BuildCheck::Schema, "goal text is empty");
        } else if i >= MAX_GOALS {
            drop(BuildCheck::Schema, "an NPC keeps at most three goals");
        } else {
            kept += 1;
        }
        if against(goal).is_some_and(|a| roster.find(a).is_none()) {
            problems.push(Problem::optional(
                BuildCheck::References,
                NpcItem::Against(i),
                Note::item("goals", i)
                    .field("against")
                    .says("is not a name from the ROSTER"),
            ));
        }
    }
    if kept == 0 {
        problems.push(Problem::essential(
            BuildCheck::Completeness,
            Note::part("goals").says("an NPC needs at least one goal"),
        ));
    }

    let card = &out.voice_card;
    let samples = card.samples.iter().filter(|s| !s.trim().is_empty()).count();
    if samples < MIN_SAMPLES || card.habits.trim().is_empty() || card.vocabulary.trim().is_empty() {
        problems.push(Problem::essential(
            BuildCheck::Completeness,
            Note::part("voice_card").says("needs habits, vocabulary and two to four sample lines"),
        ));
    }

    let mut toward = HashSet::new();
    for (i, attitude) in out.attitudes.iter().enumerate() {
        let note = || Note::item("attitudes", i);
        let problem = match roster.find(&attitude.to) {
            None => Some((
                BuildCheck::References,
                note().field("to").says("is not a name from the ROSTER"),
            )),
            Some(m) if m.main == npc.main => Some((
                BuildCheck::References,
                note().field("to").says("is the NPC itself"),
            )),
            Some(m) if !toward.insert(normalize::name(&m.main)) => Some((
                BuildCheck::Schema,
                note().of(m).says("a second attitude toward the same name"),
            )),
            Some(_) if !(-5..=5).contains(&attitude.attitude) => Some((
                BuildCheck::Schema,
                note().field("attitude").says("is outside -5 to 5"),
            )),
            Some(_) => None,
        };
        if let Some((check, note)) = problem {
            problems.push(Problem::optional(check, NpcItem::Attitude(i), note));
        }
    }

    if let Some(secret) = &out.secret {
        let holders = [npc.main.clone()];
        let secret = secret.fact();
        let found = fact_problems(roster, book, &secret, &holders, &|| Note::part("secret"));
        problems.extend(
            found
                .into_iter()
                .map(|(check, note)| Problem::optional(check, NpcItem::Secret, note)),
        );
    }
    problems
}

/// How many goals and secrets of `state` centre on the PC
/// (`centres_on_pc`), and how many it has.
pub(super) fn pc_centred(roster: &Roster, state: &NpcState) -> (usize, usize) {
    let items = state
        .goals
        .iter()
        .map(|g| (g.text.as_str(), g.against.as_deref()))
        .chain(
            state
                .secret
                .iter()
                .map(|s| (s.text.as_str(), Some(s.subject.as_str()))),
        );
    items.fold((0, 0), |(naming, total), (text, target)| {
        (
            naming + usize::from(roster.centres_on_pc(text, target)),
            total + 1,
        )
    })
}

/// The share of all NPC goals and secrets that centre on the PC.
pub(super) fn pc_centred_share(roster: &Roster, states: &[NpcState]) -> f32 {
    let (naming, total) = states
        .iter()
        .map(|s| pc_centred(roster, s))
        .fold((0, 0), |(n, t), (a, b)| (n + a, t + b));
    if total == 0 {
        0.0
    } else {
        naming as f32 / total as f32
    }
}

/// The items of `state` that centre on the PC, for a WorldAutonomy repair.
pub(super) fn pc_centred_items(roster: &Roster, state: &NpcState) -> Vec<String> {
    let goals = state
        .goals
        .iter()
        .enumerate()
        .filter(|(_, g)| roster.centres_on_pc(&g.text, g.against.as_deref()))
        .map(|(i, _)| Note::item("goals", i).path());
    let secret = state
        .secret
        .iter()
        .filter(|s| roster.centres_on_pc(&s.text, Some(&s.subject)))
        .map(|_| Note::part("secret").path());
    goals.chain(secret).collect()
}

// ---- World Events --------------------------------------------------------------

/// The first and last minute of a World Event, from the start: two weeks.
pub(super) const EVENT_MINUTES: std::ops::RangeInclusive<u64> = 10..=20_160;
/// The longest event text, in characters: one short line.
pub(super) const EVENT_TEXT_MAX_CHARS: usize = 80;
const MAX_EFFECTS: usize = 2;
const MAX_WITNESSES: usize = 2;

/// Each problem drops one event.
pub(super) fn world_events(roster: &Roster, out: &EventsOut) -> Vec<Problem<usize>> {
    let mut problems = vec![];
    for (i, event) in out.events.iter().enumerate() {
        let note = || Note::item("events", i);
        let mut found: Vec<(BuildCheck, String)> = vec![];
        let mut push = |check, note: String| found.push((check, note));
        if !EVENT_MINUTES.contains(&event.in_minutes) {
            push(
                BuildCheck::Schema,
                note().field("in_minutes").says("is outside 10 to 20160"),
            );
        }
        let chars = event.text.trim().chars().count();
        if chars == 0 || chars > EVENT_TEXT_MAX_CHARS {
            push(
                BuildCheck::Schema,
                note()
                    .field("text")
                    .says("needs one line of at most 80 characters"),
            );
        }
        if event.effects.len() > MAX_EFFECTS || event.seen_by.len() > MAX_WITNESSES {
            push(
                BuildCheck::Schema,
                note().says("at most two effects and two witnesses"),
            );
        }
        let actor = roster.find(&event.actor);
        match actor {
            None => push(
                BuildCheck::References,
                note().field("actor").says("is not a name from the ROSTER"),
            ),
            Some(m) if m.kind == EntityKind::Pc => push(
                BuildCheck::Agency,
                note()
                    .field("actor")
                    .says("is the PC; an event never makes the PC act"),
            ),
            Some(m) if m.kind != EntityKind::Npc => push(
                BuildCheck::Kinds,
                note().field("actor").of(m).says("is not an NPC"),
            ),
            Some(_) => {}
        }
        for (j, witness) in event.seen_by.iter().enumerate() {
            let at = || note().field("seen_by").at(j);
            match roster.find(witness) {
                None => push(
                    BuildCheck::References,
                    at().says("is not a name from the ROSTER"),
                ),
                Some(m) if m.kind == EntityKind::Pc => push(
                    BuildCheck::Agency,
                    at().says("is the PC; seen_by lists NPCs only"),
                ),
                Some(m) if m.kind != EntityKind::Npc => {
                    push(BuildCheck::Kinds, at().of(m).says("is not an NPC"))
                }
                Some(m) if actor.is_some_and(|a| a.main == m.main) => push(
                    BuildCheck::References,
                    at().of(m).says("is the actor; the actor is no witness"),
                ),
                Some(_) => {}
            }
        }
        for (j, effect) in event.effects.iter().enumerate() {
            let at = || note().field("effects").at(j);
            if effect.key.trim().is_empty() || effect.value.trim().is_empty() {
                push(
                    BuildCheck::Schema,
                    at().says("needs a subject, a key and a value"),
                );
            }
            // ADR 0005: only `setup` writes an open item, from `open`.
            if is_open_key(&effect.key) {
                push(
                    BuildCheck::OpenItems,
                    at().field("key")
                        .says("starts with pending_; an event never writes an open item"),
                );
            }
            match roster.find(&effect.subject) {
                None => push(
                    BuildCheck::References,
                    at().field("subject").says("is not a name from the ROSTER"),
                ),
                Some(m) if m.kind == EntityKind::Pc && !is_involuntary_key(&effect.key) => {
                    push(
                        BuildCheck::Agency,
                        at().field("key").of(m).says(&PC_KEYS_RULE),
                    );
                }
                Some(_) => {}
            }
        }
        problems.extend(
            found
                .into_iter()
                .map(|(check, note)| Problem::optional(check, i, note)),
        );
    }
    problems
}

// ---- Start state ---------------------------------------------------------------

/// The `location` fact of a placement, held by the NPC.
pub(super) fn location(npc: &str, place: &str, known_by: Vec<String>) -> FactEntry {
    FactEntry {
        text: format!("{npc} is at {place}."),
        ..fact(npc, "location", place, known_by)
    }
}

/// Each problem drops one placement. A placement is checked against the
/// accepted facts and the placements before it that passed, and may not
/// put an NPC at the start place.
pub(super) fn start_state(
    roster: &Roster,
    book: &FactBook,
    start_place: &str,
    out: &StartOut,
) -> Vec<Problem<usize>> {
    let mut book = book.clone();
    let mut problems = vec![];
    for (i, placement) in out.placements.iter().enumerate() {
        let note = || Note::item("placements", i);
        let npc = roster.find(&placement.npc);
        let place = roster.find(&placement.place);
        let problem = match (npc, place) {
            (None, _) => Some((
                BuildCheck::References,
                note().field("npc").says("is not a name from the ROSTER"),
            )),
            (_, None) => Some((
                BuildCheck::References,
                note().field("place").says("is not a name from the ROSTER"),
            )),
            (Some(n), _) if n.kind == EntityKind::Pc => Some((
                BuildCheck::OnePc,
                note().field("npc").says("is the PC; code places the PC"),
            )),
            (Some(n), _) if n.kind != EntityKind::Npc => Some((
                BuildCheck::Kinds,
                note().field("npc").of(n).says("is not an NPC"),
            )),
            (_, Some(p)) if p.kind != EntityKind::Place => Some((
                BuildCheck::Kinds,
                note().field("place").of(p).says("is not a place"),
            )),
            // An absent NPC is not at the start place: the approved start
            // state lists who is there.
            (Some(n), Some(p)) if same_name(&p.main, start_place) => Some((
                BuildCheck::SettledFacts,
                note()
                    .of(n)
                    .says("this NPC is absent at the start, so not at the start place"),
            )),
            (Some(n), Some(p)) => {
                let entry = location(&n.main, &p.main, vec![n.main.clone()]);
                match book.clash(&entry) {
                    Some(check) => Some((
                        check,
                        note()
                            .of(n)
                            .says("an accepted fact puts this NPC elsewhere"),
                    )),
                    None => {
                        book.add(entry);
                        None
                    }
                }
            }
        };
        if let Some((check, note)) = problem {
            problems.push(Problem::optional(check, i, note));
        }
    }
    problems
}
