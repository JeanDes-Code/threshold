//! The three draft stages: the roster (with the PC), the seed and PC
//! Knowledge, drafted from a Source Text (design 6.2, data shape 3.5). Each
//! stage output is a private type with length caps; it converts to the
//! public draft types. Once the roster is known, each field of a later
//! stage that names a roster entry is an `enum` of roster names in its
//! schema.

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::Deserialize;

use super::{
    BuildCheck, BuildFailure, Control, DraftState, KnownFact, SeedDraft, Stage,
    draft::new_draft_id,
    stages::{
        Need, Problem, StageCall, essential, name_field, name_list, report, run_stage, schema_of,
        sections,
    },
};
use crate::{
    domain::{
        AdventureSeed, EntityKind, OPEN_KEY_PREFIX, OpenDetail, OpenMode, OtherName, PcSeed,
        SeedEntity, StartSeed,
    },
    model::{Gm, Prompt},
    normalize,
};

/// The longest Source Text, in characters, that a draft accepts. The seed
/// and roster calls read the whole text, so this bounds their prompt.
pub const SOURCE_TEXT_MAX_CHARS: usize = 60_000;

/// Drafts a Seed Draft from a Source Text: the roster with the PC, the seed,
/// then the PC Knowledge of each section. An empty or too long text is
/// refused before any call, as `Completeness`. A failed roster or seed fails
/// the draft; a known fact that fails its repair is dropped.
pub fn draft_from_text(
    text: &str,
    gm: &dyn Gm,
    ctl: &mut Control,
) -> Result<SeedDraft, BuildFailure> {
    if text.trim().is_empty() || text.chars().count() > SOURCE_TEXT_MAX_CHARS {
        return Err(BuildFailure {
            stage: Stage::DraftSeed,
            check: BuildCheck::Completeness,
        });
    }
    let id = new_draft_id();

    report(ctl, Stage::DraftRoster, 0, 1);
    let roster = run_stage(gm, ctl, &id, &roster_call(text), check_roster)?;
    let roster = essential(Stage::DraftRoster, roster)?.output;
    report(ctl, Stage::DraftRoster, 1, 1);

    report(ctl, Stage::DraftSeed, 0, 1);
    let call = seed_call(text, &roster);
    let seed = run_stage(gm, ctl, &id, &call, |out| check_seed(&roster, out))?;
    let seed = essential(Stage::DraftSeed, seed)?.output;
    report(ctl, Stage::DraftSeed, 1, 1);

    let mut draft = assemble(id, text, seed, roster);
    let names = Names::of(&draft.seed);
    let parts = sections(text);
    report(ctl, Stage::DraftKnowledge, 0, parts.len());
    // One known fact per slot: subject, key and open state. The same value
    // is a duplicate; another value is the later state, so a fact from a
    // later section, or later in a section, replaces the earlier one.
    let mut slots: Vec<(String, String, bool)> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let call = knowledge_call(&draft.seed, (i + 1, parts.len()), part);
        let staged = run_stage(gm, ctl, &draft.id, &call, |out| {
            check_knowledge(&names, out)
        })?;
        if let Some(staged) = staged {
            for (j, fact) in staged.output.facts.into_iter().enumerate() {
                if !fact.pc_knows || staged.drop.contains(&j) {
                    continue;
                }
                let fact = known_fact(&names, fact);
                let slot = (normalize::name(&fact.subject), fact.key.clone(), fact.open);
                match slots.iter().position(|s| *s == slot) {
                    None => {
                        slots.push(slot);
                        draft.knowledge.push(fact);
                    }
                    Some(at) => {
                        let earlier = &draft.knowledge[at];
                        if normalize::value(&earlier.value) != normalize::value(&fact.value) {
                            draft.knowledge[at] = fact;
                        }
                    }
                }
            }
        }
        report(ctl, Stage::DraftKnowledge, i + 1, parts.len());
    }
    Ok(draft)
}

// ---- Stage outputs -------------------------------------------------------

// The PC comes from the roster stage.
#[derive(Deserialize, JsonSchema)]
struct SeedOut {
    #[schemars(length(max = 500))]
    premise: String,
    #[schemars(length(max = 12))]
    story_language: String,
    #[schemars(length(max = 8), inner(length(max = 40)))]
    themes: Vec<String>,
    #[schemars(length(max = 1200))]
    gm_agreement: String,
    #[schemars(length(max = 600))]
    resolution: String,
    #[schemars(range(max = 999))]
    min_age: Option<u16>,
    start: StartOut,
    #[schemars(length(max = 8))]
    open_details: Vec<OpenDetailOut>,
}

#[derive(Deserialize, JsonSchema)]
struct PcOut {
    #[schemars(length(max = 60))]
    name: String,
    #[schemars(range(max = 999))]
    age: u16,
    // False when the model proposed the age.
    age_stated: bool,
    #[schemars(length(max = 4))]
    other_names: Vec<NameOut>,
    #[schemars(length(max = 300))]
    concept: String,
}

#[derive(Clone, Deserialize, JsonSchema)]
struct NameOut {
    #[schemars(length(max = 60))]
    name: String,
    #[schemars(length(max = 8), inner(length(max = 60)))]
    known_to: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct StartOut {
    // A place of the roster: an enum in the schema.
    #[schemars(length(max = 60))]
    place: String,
    // Characters of the roster, not the PC: an enum in the schema.
    #[schemars(length(max = 8), inner(length(max = 60)))]
    present: Vec<String>,
    #[schemars(length(max = 400))]
    situation: String,
    #[schemars(length(max = 80))]
    when: String,
}

#[derive(Deserialize, JsonSchema)]
struct OpenDetailOut {
    #[schemars(length(max = 200))]
    text: String,
    mode: OpenMode,
}

// The roster in two lists, so that a character without an age and an age
// on a place cannot be written. A doc comment would reach the schema.
#[derive(Deserialize, JsonSchema)]
struct RosterOut {
    pc: PcOut,
    #[schemars(length(max = 30))]
    characters: Vec<CharacterOut>,
    #[schemars(length(max = 30))]
    others: Vec<OtherOut>,
}

#[derive(Deserialize, JsonSchema)]
struct CharacterOut {
    #[schemars(length(max = 60))]
    name: String,
    #[schemars(range(min = 0, max = 999))]
    age: u16,
    // False when the model proposed the age.
    age_stated: bool,
    #[schemars(length(max = 4))]
    other_names: Vec<NameOut>,
}

#[derive(Deserialize, JsonSchema)]
struct OtherOut {
    #[schemars(length(max = 60))]
    name: String,
    kind: OtherKind,
    #[schemars(length(max = 4))]
    other_names: Vec<NameOut>,
}

// Every roster kind with no age.
#[derive(Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OtherKind {
    Place,
    Faction,
    Item,
}

impl From<OtherKind> for EntityKind {
    fn from(kind: OtherKind) -> Self {
        match kind {
            OtherKind::Place => Self::Place,
            OtherKind::Faction => Self::Faction,
            OtherKind::Item => Self::Item,
        }
    }
}

impl RosterOut {
    /// Each entry but the PC: main name, kind and other names.
    fn entries(&self) -> impl Iterator<Item = (&str, EntityKind, &[NameOut])> {
        let characters = self
            .characters
            .iter()
            .map(|c| (c.name.as_str(), EntityKind::Npc, c.other_names.as_slice()));
        let others = self
            .others
            .iter()
            .map(|o| (o.name.as_str(), o.kind.into(), o.other_names.as_slice()));
        characters.chain(others)
    }

    /// The main name and kind of the entry that `name` names, main or
    /// other, through `normalize::name`; the PC first.
    fn find(&self, name: &str) -> Option<(&str, EntityKind)> {
        let norm = normalize::name(name);
        let pc = (
            self.pc.name.as_str(),
            EntityKind::Pc,
            self.pc.other_names.as_slice(),
        );
        std::iter::once(pc)
            .chain(self.entries())
            .find(|(main, _, others)| {
                std::iter::once(*main)
                    .chain(others.iter().map(|o| o.name.as_str()))
                    .any(|n| normalize::name(n) == norm)
            })
            .map(|(main, kind, _)| (main, kind))
    }

    /// The main names of the entries of `kind`.
    fn names_of(&self, kind: EntityKind) -> Vec<&str> {
        self.entries()
            .filter(|(_, k, _)| *k == kind)
            .map(|(name, _, _)| name)
            .collect()
    }
}

#[derive(Deserialize, JsonSchema)]
struct KnowledgeOut {
    #[schemars(length(max = 40))]
    facts: Vec<FactOut>,
}

#[derive(Deserialize, JsonSchema)]
struct FactOut {
    #[schemars(length(max = 60))]
    subject: String,
    #[schemars(length(max = 32))]
    key: String,
    #[schemars(length(max = 40))]
    value: String,
    #[schemars(length(max = 200))]
    text: String,
    // Planned, proposed or pending: it has not happened.
    open: bool,
    // False for what the PC does not know; such a fact leaves the draft.
    pc_knows: bool,
}

// ---- Prompts ---------------------------------------------------------------

const ROSTER_SYSTEM: &str = "\
You list the roster of a Source Text for a role-playing game, as JSON.
Rules:
- Use only what the text states. Write each name as the text writes it, with no final period.
- pc: the player character (PC). The PC is one person, even when the text gives the PC several names. Put the main name in pc.name and each other name in pc.other_names. concept: one sentence about who the PC is.
- characters: every other named person. Never the PC, under any of the PC's names. One person is one entry, even with several names: the main name in name, the others in other_names.
- others: every named place, group and significant object. kind is place for a location, faction for a group of people (a family, a crew, a council, a company), item for an object that matters. A group is never a character. List the place where play starts.
- other_names: list only the other names that the PC knows. known_to lists the characters who know that this name means this person.
- age: the PC and every character have an age in years. When the text states it, age_stated is true. When it does not, propose an age that fits the text and set age_stated to false.";

const SEED_SYSTEM: &str = "\
You read a Source Text that a Player wrote for a role-playing game. You draft its Adventure Seed as JSON. The PC and the roster are given.
Rules:
- Use only what the text states. Never invent a past event. Never invent a decision of the Player or of the player character (PC).
- premise: two or three sentences about the situation where play starts.
- story_language: the short code of the language of the text, such as en or fr. Write every text field in that language.
- themes: at most 8, a few words each. Mature content in the text becomes a theme or a neutral statement. Do not remove it and do not refuse.
- gm_agreement: how the text asks the Game Master to run the game. Empty when it does not say.
- resolution: how the text says uncertain actions are resolved. Empty when it does not say.
- min_age: the lowest age a character may have, only when the text states such a constraint (\"every character is an adult\" gives 18). Otherwise null.
- start: where play starts, one name from PLACES, spelled as written there; the characters present there, by names from CHARACTERS (not the PC); what is going on; and when, in words.
- open_details: what the text leaves open. mode is delegated when the text leaves it to the Game Master, discover when the PC finds it out in play, to_agree when the Player and the Game Master settle it before play.
- Something planned, proposed or pending has not happened yet. Never write it as done.";

const KNOWLEDGE_SYSTEM: &str = "\
You extract what the player character (PC) knows from one section of a Source Text for a role-playing game, as JSON.
Rules:
- Use only what this section states. Never invent a past event, a link between two entities, or a decision of the Player or of the PC.
- Extract every fact the section states, one fact per item: who someone is, where someone or something is, who owns what, how two characters relate, what happened, what is planned.
- subject: one name from the ROSTER, spelled as written there. The PC is one subject: never use another name of the PC as a subject.
- key: a short English word in snake_case, such as role, location, owner, occupation. value: a few words. text: one sentence that states the fact, in the story language.
- open: true when the item is planned, proposed, promised or pending, so it has not happened. false when the text says that it happened or that it is true.
- pc_knows: true when the PC knows the fact. false for the private feeling of another character, a hidden cause, or anything the PC has not seen or been told.
- Mature content becomes neutral facts. Do not remove it and do not refuse.";

fn roster_call(text: &str) -> StageCall {
    StageCall {
        stage: Stage::DraftRoster,
        step: "draft_roster".into(),
        prompt: Prompt {
            system: ROSTER_SYSTEM.into(),
            recent: format!("SOURCE TEXT\n{text}"),
            ..Prompt::default()
        },
        schema: schema_of::<RosterOut>(),
        need: Need::Essential,
    }
}

fn seed_call(text: &str, roster: &RosterOut) -> StageCall {
    let places = roster.names_of(EntityKind::Place);
    let characters = roster.names_of(EntityKind::Npc);
    // One line per entry, with its other names, as `knowledge_call` writes
    // them; the enums hold main names only.
    let list = |kind: EntityKind| {
        let lines: Vec<String> = roster
            .entries()
            .filter(|(_, k, _)| *k == kind)
            .map(|(name, _, others)| {
                let mut line = format!("- {name}");
                if !others.is_empty() {
                    let others: Vec<&str> = others.iter().map(|o| o.name.as_str()).collect();
                    line.push_str(&format!("; also called {}", others.join(", ")));
                }
                line
            })
            .collect();
        if lines.is_empty() {
            "none".to_owned()
        } else {
            lines.join("\n")
        }
    };
    let others: Vec<&str> = roster
        .pc
        .other_names
        .iter()
        .map(|o| o.name.as_str())
        .collect();
    let mut schema = schema_of::<SeedOut>();
    name_field(
        &mut schema,
        "/$defs/StartOut/properties/place",
        &places,
        false,
    );
    name_list(
        &mut schema,
        "/$defs/StartOut/properties/present",
        &characters,
    );
    StageCall {
        stage: Stage::DraftSeed,
        step: "draft_seed".into(),
        prompt: Prompt {
            system: SEED_SYSTEM.into(),
            recent: format!(
                "THE PC\nName: {}\nOther names: {}\n\nPLACES\n{}\n\nCHARACTERS\n{}\n\nSOURCE TEXT\n{text}",
                roster.pc.name,
                if others.is_empty() {
                    "none".into()
                } else {
                    others.join(", ")
                },
                list(EntityKind::Place),
                list(EntityKind::Npc),
            ),
            ..Prompt::default()
        },
        schema,
        need: Need::Essential,
    }
}

fn knowledge_call(seed: &AdventureSeed, (n, of): (usize, usize), part: &str) -> StageCall {
    let line = |name: &str, what: &str, others: &[OtherName]| {
        let mut line = format!("- {name}: {what}");
        if !others.is_empty() {
            let others: Vec<&str> = others.iter().map(|o| o.name.as_str()).collect();
            line.push_str(&format!("; also called {}", others.join(", ")));
        }
        line
    };
    let mut roster = vec![line(
        &seed.pc.name,
        "the player character (PC)",
        &seed.pc.other_names,
    )];
    for entity in &seed.entities {
        roster.push(line(
            &entity.name,
            kind_word(entity.kind),
            &entity.other_names,
        ));
    }
    let subjects: Vec<&str> = std::iter::once(&seed.pc.name)
        .chain(seed.entities.iter().map(|e| &e.name))
        .map(String::as_str)
        .collect();
    let mut schema = schema_of::<KnowledgeOut>();
    name_field(
        &mut schema,
        "/$defs/FactOut/properties/subject",
        &subjects,
        false,
    );
    StageCall {
        stage: Stage::DraftKnowledge,
        step: format!("draft_knowledge {n}/{of}"),
        prompt: Prompt {
            system: KNOWLEDGE_SYSTEM.into(),
            recent: format!(
                "PREMISE\n{}\n\nSTORY LANGUAGE\n{}\n\nROSTER\n{}\n\nSECTION {n} OF {of}\n{part}",
                seed.premise,
                seed.story_language,
                roster.join("\n"),
            ),
            ..Prompt::default()
        },
        schema,
        need: Need::Optional,
    }
}

fn kind_word(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Pc => "pc",
        EntityKind::Npc => "npc",
        EntityKind::Faction => "faction",
        EntityKind::Place => "place",
        EntityKind::Item => "item",
    }
}

// ---- Checks ----------------------------------------------------------------

/// Schema, OnePc and References on the roster. Every problem is
/// essential: the roster is.
fn check_roster(out: &RosterOut) -> Vec<Problem<usize>> {
    let mut problems = vec![];
    let mut problem = |check, note: String| problems.push(Problem::essential(check, note));
    let pc_names = pc_names(&out.pc);

    if out.pc.name.trim().is_empty() {
        problem(BuildCheck::Schema, "pc.name is empty".into());
    }
    let characters = out.characters.iter().map(|c| (&c.name, &c.other_names));
    let others = out.others.iter().map(|o| (&o.name, &o.other_names));
    let lists = [
        ("characters", characters.collect::<Vec<_>>()),
        ("others", others.collect()),
    ];
    for (list, entries) in lists {
        for (i, (name, other_names)) in entries.into_iter().enumerate() {
            if name.trim().is_empty() {
                problem(BuildCheck::Schema, format!("{list}[{i}].name is empty"));
            }
            for name in std::iter::once(name).chain(other_names.iter().map(|o| &o.name)) {
                if pc_names.contains(&normalize::name(name)) {
                    problem(
                        BuildCheck::OnePc,
                        format!("\"{name}\" is the PC; the PC is only in pc"),
                    );
                }
            }
        }
    }

    let holders = out
        .pc
        .other_names
        .iter()
        .chain(out.entries().flat_map(|(_, _, others)| others))
        .flat_map(|o| &o.known_to);
    for name in holders {
        if out.find(name).is_none() {
            problem(
                BuildCheck::References,
                format!("\"{name}\" is in a known_to list but not in the roster"),
            );
        }
    }
    problems
}

/// Schema, References and Kinds on the seed. The schema allows only roster
/// names for the start; these checks guard a model that ignores it. Every
/// problem is essential: the seed is.
fn check_seed(roster: &RosterOut, out: &SeedOut) -> Vec<Problem<usize>> {
    let mut problems: Vec<Problem<usize>> = [
        (&out.premise, "premise"),
        (&out.story_language, "story_language"),
    ]
    .into_iter()
    .filter(|(text, _)| text.trim().is_empty())
    .map(|(_, field)| Problem::essential(BuildCheck::Schema, format!("{field} is empty")))
    .collect();
    let mut problem = |check, note: String| problems.push(Problem::essential(check, note));

    let place = &out.start.place;
    if !place.trim().is_empty() {
        match roster.find(place) {
            None => problem(
                BuildCheck::References,
                format!("start.place \"{place}\" is not a name from PLACES"),
            ),
            Some((_, EntityKind::Place)) => {}
            Some(_) => problem(
                BuildCheck::Kinds,
                format!("start.place \"{place}\" is not a place"),
            ),
        }
    }
    for name in &out.start.present {
        match roster.find(name) {
            None => problem(
                BuildCheck::References,
                format!("start.present \"{name}\" is not a name from CHARACTERS"),
            ),
            Some((_, EntityKind::Pc | EntityKind::Npc)) => {}
            Some(_) => problem(
                BuildCheck::Kinds,
                format!("start.present \"{name}\" is not a character"),
            ),
        }
    }
    problems
}

/// Schema and References on each fact that the PC knows. Each problem drops
/// one fact.
fn check_knowledge(names: &Names, out: &KnowledgeOut) -> Vec<Problem<usize>> {
    let mut problems = vec![];
    for (i, fact) in out.facts.iter().enumerate() {
        if !fact.pc_knows {
            continue;
        }
        let empty = [&fact.subject, &fact.value, &fact.text]
            .iter()
            .any(|t| t.trim().is_empty())
            || open_key(&fact.key, false).0.is_empty();
        if empty {
            problems.push(Problem::optional(
                BuildCheck::Schema,
                i,
                format!("facts[{i}] needs a subject, a key, a value and a text"),
            ));
        } else if names.main(&fact.subject).is_none() {
            problems.push(Problem::optional(
                BuildCheck::References,
                i,
                format!(
                    "facts[{i}].subject \"{}\" is not a name from the ROSTER",
                    fact.subject
                ),
            ));
        }
    }
    problems
}

// ---- Assembly --------------------------------------------------------------

fn assemble(id: String, text: &str, seed: SeedOut, roster: RosterOut) -> SeedDraft {
    let pc_names = pc_names(&roster.pc);
    let pc = &roster.pc.name;
    // The start names the roster entries by their main names.
    let main = |name: &str| roster.find(name).map(|(main, _)| main.to_owned());
    let place = main(&seed.start.place).unwrap_or(seed.start.place);
    let present = seed
        .start
        .present
        .iter()
        .filter(|n| !pc_names.contains(&normalize::name(n)))
        .map(|n| main(n).unwrap_or_else(|| n.clone()))
        .collect();

    let mut ages_to_confirm = vec![];
    if !roster.pc.age_stated {
        ages_to_confirm.push(pc.clone());
    }
    let others = |list: Vec<NameOut>| -> Vec<OtherName> {
        list.into_iter()
            .map(|o| known_to_pc(o, pc, &pc_names))
            .collect()
    };
    let mut entities = vec![];
    for character in roster.characters {
        if !character.age_stated {
            ages_to_confirm.push(character.name.clone());
        }
        entities.push(SeedEntity {
            other_names: others(character.other_names),
            name: character.name,
            kind: EntityKind::Npc,
            age: Some(character.age),
            hidden: false,
        });
    }
    for other in roster.others {
        entities.push(SeedEntity {
            other_names: others(other.other_names),
            name: other.name,
            kind: other.kind.into(),
            age: None,
            hidden: false,
        });
    }
    SeedDraft {
        id,
        state: DraftState::Draft,
        source_text: text.to_owned(),
        seed: AdventureSeed {
            premise: seed.premise,
            pc: PcSeed {
                name: roster.pc.name,
                age: roster.pc.age,
                other_names: roster.pc.other_names.into_iter().map(other_name).collect(),
                concept: roster.pc.concept,
            },
            entities,
            boundaries: vec![],
            decided: vec![],
            proposed: vec![],
            story_language: seed.story_language,
            themes: seed.themes,
            gm_agreement: seed.gm_agreement,
            resolution: seed.resolution,
            min_age: seed.min_age,
            start: StartSeed {
                place,
                present,
                situation: seed.start.situation,
                when: seed.start.when,
            },
            open_details: seed
                .open_details
                .into_iter()
                .map(|d| OpenDetail {
                    text: d.text,
                    mode: d.mode,
                })
                .collect(),
        },
        knowledge: vec![],
        ages_to_confirm,
        last_failure: None,
        campaign_file: None,
    }
}

fn other_name(out: NameOut) -> OtherName {
    OtherName {
        name: out.name,
        known_to: out.known_to,
    }
}

/// A draft lists only the other names that the PC knows (data shape 3.3),
/// so the PC is in `known_to`.
fn known_to_pc(out: NameOut, pc: &str, pc_names: &HashSet<String>) -> OtherName {
    let mut name = other_name(out);
    if !name
        .known_to
        .iter()
        .any(|n| pc_names.contains(&normalize::name(n)))
    {
        name.known_to.insert(0, pc.to_owned());
    }
    name
}

/// A checked fact the PC knows: the subject is its main name, and a
/// `pending_` prefix on the key becomes `open`.
fn known_fact(names: &Names, fact: FactOut) -> KnownFact {
    let (key, open) = open_key(&fact.key, fact.open);
    KnownFact {
        subject: names
            .main(&fact.subject)
            .map_or(fact.subject.clone(), str::to_owned),
        key,
        value: fact.value,
        text: fact.text,
        open,
    }
}

/// The normalized key without any `pending_` prefix, and whether the item is
/// open. A prefix that the model wrote means open: code alone adds it.
pub(super) fn open_key(key: &str, open: bool) -> (String, bool) {
    let mut key = normalize::key(key);
    let mut open = open;
    while let Some(rest) = key.strip_prefix(OPEN_KEY_PREFIX) {
        key = rest.to_owned();
        open = true;
    }
    (key, open)
}

fn pc_names(pc: &PcOut) -> HashSet<String> {
    std::iter::once(&pc.name)
        .chain(pc.other_names.iter().map(|o| &o.name))
        .map(|n| normalize::name(n))
        .filter(|n| !n.is_empty())
        .collect()
}

/// Every name of the PC and the roster, normalized, with the main name it
/// stands for.
struct Names(Vec<(String, String)>);

impl Names {
    fn of(seed: &AdventureSeed) -> Self {
        let pc = (&seed.pc.name, &seed.pc.other_names);
        let entities = seed.entities.iter().map(|e| (&e.name, &e.other_names));
        Self(
            std::iter::once(pc)
                .chain(entities)
                .flat_map(|(main, others)| {
                    std::iter::once(main)
                        .chain(others.iter().map(|o| &o.name))
                        .map(move |n| (normalize::name(n), main.clone()))
                })
                .filter(|(n, _)| !n.is_empty())
                .collect(),
        )
    }

    /// The main name that `name` stands for; the first entry on a tie.
    fn main(&self, name: &str) -> Option<&str> {
        let name = normalize::name(name);
        self.0
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, main)| main.as_str())
    }
}
