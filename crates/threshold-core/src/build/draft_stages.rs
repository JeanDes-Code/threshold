//! The three draft stages: the seed, the roster and PC Knowledge, drafted
//! from a Source Text (design 6.2, data shape 3.5). Each stage output is a
//! private type with length caps; it converts to the public draft types.

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::Deserialize;

use super::{
    BuildCheck, BuildFailure, Control, DraftState, KnownFact, SeedDraft, Stage,
    draft::new_draft_id,
    stages::{Need, Problem, StageCall, Staged, report, run_stage, schema_of, sections},
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

/// Drafts a Seed Draft from a Source Text: the seed, the roster, then the
/// PC Knowledge of each section. An empty or too long text is refused before
/// any call, as `Completeness`. A failed seed or roster fails the draft; a
/// known fact that fails its repair is dropped.
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

    report(ctl, Stage::DraftSeed, 0, 1);
    let seed = run_stage(gm, ctl, &id, &seed_call(text), check_seed)?;
    let seed = essential(Stage::DraftSeed, seed)?;
    report(ctl, Stage::DraftSeed, 1, 1);

    report(ctl, Stage::DraftRoster, 0, 1);
    let call = roster_call(text, &seed);
    let roster = run_stage(gm, ctl, &id, &call, |out| check_roster(&seed, out))?;
    let roster = essential(Stage::DraftRoster, roster)?;
    report(ctl, Stage::DraftRoster, 1, 1);

    let mut draft = assemble(id, text, seed, roster);
    let names = Names::of(&draft.seed);
    let parts = sections(text);
    report(ctl, Stage::DraftKnowledge, 0, parts.len());
    let mut seen = HashSet::new();
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
                let slot = (
                    normalize::name(&fact.subject),
                    fact.key.clone(),
                    normalize::name(&fact.value),
                );
                if seen.insert(slot) {
                    draft.knowledge.push(fact);
                }
            }
        }
        report(ctl, Stage::DraftKnowledge, i + 1, parts.len());
    }
    Ok(draft)
}

/// The output of an essential call. `run_stage` returns `None` only for an
/// optional call, so the fallback never fires.
fn essential<T, I>(stage: Stage, staged: Option<Staged<T, I>>) -> Result<T, BuildFailure> {
    staged.map(|s| s.output).ok_or(BuildFailure {
        stage,
        check: BuildCheck::Schema,
    })
}

// ---- Stage outputs -------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
struct SeedOut {
    #[schemars(length(max = 500))]
    premise: String,
    #[schemars(length(max = 12))]
    story_language: String,
    pc: PcOut,
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
    #[schemars(length(max = 60))]
    place: String,
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

#[derive(Deserialize, JsonSchema)]
struct RosterOut {
    #[schemars(length(max = 40))]
    entities: Vec<RosterEntry>,
}

#[derive(Deserialize, JsonSchema)]
struct RosterEntry {
    #[schemars(length(max = 60))]
    name: String,
    kind: RosterKind,
    // A character's age in years; null for anything else.
    #[schemars(range(max = 999))]
    age: Option<u16>,
    // False when the model proposed the age.
    age_stated: bool,
    #[schemars(length(max = 4))]
    other_names: Vec<NameOut>,
}

// Every roster kind but the PC: the PC comes from the seed stage.
#[derive(Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RosterKind {
    Npc,
    Place,
    Faction,
    Item,
}

impl From<RosterKind> for EntityKind {
    fn from(kind: RosterKind) -> Self {
        match kind {
            RosterKind::Npc => Self::Npc,
            RosterKind::Place => Self::Place,
            RosterKind::Faction => Self::Faction,
            RosterKind::Item => Self::Item,
        }
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

const SEED_SYSTEM: &str = "\
You read a Source Text that a Player wrote for a role-playing game. You draft its Adventure Seed as JSON.
Rules:
- Use only what the text states. Never invent a past event. Never invent a decision of the Player or of the player character (PC).
- The PC is one person, even when the text gives the PC several names. Put the main name in pc.name and each other name in pc.other_names. known_to lists the characters who know that this name means the PC.
- pc.age is the age in years. When the text states it, age_stated is true. When it does not, propose an age that fits the text and set age_stated to false.
- premise: two or three sentences about the situation where play starts.
- story_language: the short code of the language of the text, such as en or fr. Write every text field in that language.
- themes: at most 8, a few words each. Mature content in the text becomes a theme or a neutral statement. Do not remove it and do not refuse.
- gm_agreement: how the text asks the Game Master to run the game. Empty when it does not say.
- resolution: how the text says uncertain actions are resolved. Empty when it does not say.
- min_age: the lowest age a character may have, only when the text states such a constraint (\"every character is an adult\" gives 18). Otherwise null.
- start: the place where play starts, the characters present there by name (not the PC), what is going on, and when, in words.
- open_details: what the text leaves open. mode is delegated when the text leaves it to the Game Master, discover when the PC finds it out in play, to_agree when the Player and the Game Master settle it before play.
- Something planned, proposed or pending has not happened yet. Never write it as done.";

const ROSTER_SYSTEM: &str = "\
You list the roster of a Source Text for a role-playing game, as JSON.
Rules:
- entities: every named character, place, group and significant object in the text. Use only what the text states.
- Never list the player character (PC). The PC and every name of the PC are given below. None of them is an entity.
- kind: npc for one person. faction for a group of people (a family, a crew, a council, a company), never npc. place for a location. item for an object that matters.
- One person is one entity, even with several names. Put the main name in name and the others in other_names. List only the other names that the PC knows. known_to lists the characters who know that this name means this entity.
- age: every npc has an age in years. A place, a faction or an item has no age (null). When the text states the age, age_stated is true. When it does not, propose an age that fits the text and set age_stated to false.
- The roster holds every name under NAMES THE ROSTER MUST HOLD, spelled as given.
- Write each name as the text writes it.";

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

fn seed_call(text: &str) -> StageCall {
    StageCall {
        stage: Stage::DraftSeed,
        step: "draft_seed".into(),
        prompt: Prompt {
            system: SEED_SYSTEM.into(),
            recent: format!("SOURCE TEXT\n{text}"),
            ..Prompt::default()
        },
        schema: schema_of::<SeedOut>(),
        need: Need::Essential,
    }
}

fn roster_call(text: &str, seed: &SeedOut) -> StageCall {
    let pc_names = pc_names(&seed.pc);
    let others: Vec<&str> = seed
        .pc
        .other_names
        .iter()
        .map(|o| o.name.as_str())
        .collect();
    let mut needed = vec![];
    if !seed.start.place.trim().is_empty() {
        needed.push(format!("- {} (the start place)", seed.start.place));
    }
    let present = seed
        .start
        .present
        .iter()
        .map(|n| (n, "present at the start"));
    let holders = seed
        .pc
        .other_names
        .iter()
        .flat_map(|o| &o.known_to)
        .map(|n| (n, "knows another name of the PC"));
    let mut listed = HashSet::new();
    for (name, why) in present.chain(holders) {
        let norm = normalize::name(name);
        if !norm.is_empty() && !pc_names.contains(&norm) && listed.insert(norm) {
            needed.push(format!("- {name} ({why})"));
        }
    }
    StageCall {
        stage: Stage::DraftRoster,
        step: "draft_roster".into(),
        prompt: Prompt {
            system: ROSTER_SYSTEM.into(),
            recent: format!(
                "THE PC (not a roster entity)\nName: {}\nOther names: {}\n\n\
                 NAMES THE ROSTER MUST HOLD\n{}\n\nSOURCE TEXT\n{text}",
                seed.pc.name,
                if others.is_empty() {
                    "none".into()
                } else {
                    others.join(", ")
                },
                if needed.is_empty() {
                    "none".into()
                } else {
                    needed.join("\n")
                },
            ),
            ..Prompt::default()
        },
        schema: schema_of::<RosterOut>(),
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
        schema: schema_of::<KnowledgeOut>(),
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

fn check_seed(out: &SeedOut) -> Vec<Problem<usize>> {
    [
        (&out.premise, "premise"),
        (&out.pc.name, "pc.name"),
        (&out.story_language, "story_language"),
    ]
    .into_iter()
    .filter(|(text, _)| text.trim().is_empty())
    .map(|(_, field)| Problem::essential(BuildCheck::Schema, format!("{field} is empty")))
    .collect()
}

/// Schema, OnePc, Kinds and References on the roster. Every problem is
/// essential: the roster is.
fn check_roster(seed: &SeedOut, out: &RosterOut) -> Vec<Problem<usize>> {
    let mut problems = vec![];
    let mut problem = |check, note: String| problems.push(Problem::essential(check, note));
    let pc_names = pc_names(&seed.pc);

    for (i, entry) in out.entities.iter().enumerate() {
        if entry.name.trim().is_empty() {
            problem(BuildCheck::Schema, format!("entities[{i}].name is empty"));
            continue;
        }
        let names = std::iter::once(&entry.name).chain(entry.other_names.iter().map(|o| &o.name));
        for name in names {
            if pc_names.contains(&normalize::name(name)) {
                problem(
                    BuildCheck::OnePc,
                    format!("\"{name}\" is the PC; the PC is never a roster entity"),
                );
            }
        }
        match (entry.kind == RosterKind::Npc, entry.age) {
            (true, None) => problem(
                BuildCheck::Kinds,
                format!("\"{}\" is an npc and needs an age", entry.name),
            ),
            (false, Some(_)) => problem(
                BuildCheck::Kinds,
                format!("\"{}\" is not an npc, so its age is null", entry.name),
            ),
            _ => {}
        }
    }

    let kind_of = |name: &str| {
        let norm = normalize::name(name);
        if pc_names.contains(&norm) {
            return Some(EntityKind::Pc);
        }
        out.entities
            .iter()
            .find(|e| {
                std::iter::once(&e.name)
                    .chain(e.other_names.iter().map(|o| &o.name))
                    .any(|n| normalize::name(n) == norm)
            })
            .map(|e| e.kind.into())
    };
    let place = &seed.start.place;
    if !place.trim().is_empty() {
        match kind_of(place) {
            None => problem(
                BuildCheck::References,
                format!("the start place \"{place}\" is not in the roster"),
            ),
            Some(EntityKind::Place) => {}
            Some(_) => problem(
                BuildCheck::Kinds,
                format!("\"{place}\" is the start place, so its kind is place"),
            ),
        }
    }
    for name in &seed.start.present {
        match kind_of(name) {
            None => problem(
                BuildCheck::References,
                format!("\"{name}\" is present at the start but not in the roster"),
            ),
            Some(EntityKind::Pc | EntityKind::Npc) => {}
            Some(_) => problem(
                BuildCheck::Kinds,
                format!("\"{name}\" is present at the start, so it is an npc"),
            ),
        }
    }
    let holders = seed
        .pc
        .other_names
        .iter()
        .chain(out.entities.iter().flat_map(|e| &e.other_names))
        .flat_map(|o| &o.known_to);
    for name in holders {
        if kind_of(name).is_none() {
            problem(
                BuildCheck::References,
                format!("\"{name}\" is in a known_to list but not in the roster"),
            );
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
    let pc_names = pc_names(&seed.pc);
    let mut ages_to_confirm = vec![];
    if !seed.pc.age_stated {
        ages_to_confirm.push(seed.pc.name.clone());
    }
    let entities = roster
        .entities
        .into_iter()
        .map(|entry| {
            if entry.kind == RosterKind::Npc && !entry.age_stated {
                ages_to_confirm.push(entry.name.clone());
            }
            SeedEntity {
                other_names: entry
                    .other_names
                    .into_iter()
                    .map(|o| known_to_pc(o, &seed.pc.name, &pc_names))
                    .collect(),
                name: entry.name,
                kind: entry.kind.into(),
                age: entry.age,
                hidden: false,
            }
        })
        .collect();
    let present = seed
        .start
        .present
        .into_iter()
        .filter(|n| !pc_names.contains(&normalize::name(n)))
        .collect();
    SeedDraft {
        id,
        state: DraftState::Draft,
        source_text: text.to_owned(),
        seed: AdventureSeed {
            premise: seed.premise,
            pc: PcSeed {
                name: seed.pc.name,
                age: seed.pc.age,
                other_names: seed.pc.other_names.into_iter().map(other_name).collect(),
                concept: seed.pc.concept,
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
                place: seed.start.place,
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
fn open_key(key: &str, open: bool) -> (String, bool) {
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
