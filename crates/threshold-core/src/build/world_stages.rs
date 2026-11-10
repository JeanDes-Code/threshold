//! The four World Build stages: hidden facts, the hidden state of each NPC,
//! World Events and the start state (design 6.3, data shape 3.6). Each
//! stage output is a private type with length caps; code checks it
//! (`checks`) and turns it into a `WorldSetup`, which it never writes.

use schemars::JsonSchema;
use serde::Deserialize;

use super::{
    BuildCheck, BuildFailure, Control, DraftState, SeedDraft, Stage,
    checks::{self, FactBook, Member, NpcItem, Roster, fact_entry, location},
    stages::{Need, StageCall, Staged, essential, report, run_stage, schema_of, sections},
};
use crate::{
    checks::PC_INVOLUNTARY_KEYS,
    domain::{
        AttitudeEntry, EffectEntry, EntityKind, EventEntry, FactEntry, GoalEntry, VoiceCardEntry,
        WorldSetup,
    },
    model::{Gm, Prompt},
    normalize,
};

/// The highest share of all NPC goals and secrets that may name the PC. A
/// world above it turns around the PC (design 5). The first value is a
/// guess; the live evaluation sets it.
pub const PC_CENTRED_MAX_SHARE: f32 = 0.5;

/// The hidden state of one NPC, as the World Setup holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NpcState {
    /// One to three.
    pub goals: Vec<GoalEntry>,
    /// Held by this NPC alone.
    pub secret: Option<FactEntry>,
    pub attitudes: Vec<AttitudeEntry>,
    /// Two to four sample lines.
    pub voice_card: VoiceCardEntry,
}

/// Where an NPC who is not present at the start is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub npc: String,
    pub place: String,
}

/// Builds the hidden world of an approved draft: hidden facts, the state of
/// each NPC, World Events and the start state. It calls the model through
/// `gm` only and writes nothing. A failed check drops an optional item, or
/// fails the build when the item is essential.
pub fn build_world(
    draft: &SeedDraft,
    gm: &dyn Gm,
    ctl: &mut Control,
) -> Result<WorldSetup, BuildFailure> {
    if draft.state != DraftState::Approved {
        return Err(fail(Stage::HiddenFacts, BuildCheck::NotApproved));
    }
    let roster = Roster::of(&draft.seed);
    let mut book = draft_facts(draft, &roster)?;

    hidden_facts(draft, gm, ctl, &roster, &mut book)?;
    let states = npc_states(draft, gm, ctl, &roster, &book)?;
    for secret in states.iter().filter_map(|s| s.secret.clone()) {
        book.add(secret);
    }
    let events = if states.is_empty() {
        vec![]
    } else {
        world_events(draft, gm, ctl, &roster, &book, &states)?
    };
    for placement in start_state(draft, gm, ctl, &roster, &book)? {
        let npc = placement.npc.clone();
        book.add(location(&npc, &placement.place, vec![npc.clone()]));
    }

    let mut world = WorldSetup {
        facts: book.into_facts(),
        events,
        ..WorldSetup::default()
    };
    for state in states {
        world.goals.extend(state.goals);
        world.attitudes.extend(state.attitudes);
        world.voice_cards.push(state.voice_card);
    }
    Ok(world)
}

fn fail(stage: Stage, check: BuildCheck) -> BuildFailure {
    BuildFailure { stage, check }
}

/// What code derives with no call: the known facts, held by the PC, and the
/// location of the PC and of each character present at the start. An entity
/// marked hidden gets no PC holder.
fn draft_facts(draft: &SeedDraft, roster: &Roster) -> Result<FactBook, BuildFailure> {
    let start = |check| fail(Stage::StartState, check);
    let place = roster
        .find(&draft.seed.start.place)
        .ok_or(start(BuildCheck::References))?;
    if place.kind != EntityKind::Place {
        return Err(start(BuildCheck::Kinds));
    }
    let pc = roster.pc().main.clone();
    let mut book = FactBook::of_seed(&draft.seed, roster);
    for known in &draft.knowledge {
        book.put(FactEntry {
            subject: roster.main(&known.subject),
            key: normalize::key(&known.key),
            value: known.value.clone(),
            text: known.text.clone(),
            known_by: vec![pc.clone()],
            open: known.open,
        });
    }
    book.put(location(&pc, &place.main, vec![pc.clone()]));
    for name in &draft.seed.start.present {
        let member = roster.find(name).ok_or(start(BuildCheck::References))?;
        match member.kind {
            EntityKind::Pc => {}
            EntityKind::Npc => {
                let holders = if member.hidden {
                    vec![]
                } else {
                    vec![pc.clone()]
                };
                book.put(location(&member.main, &place.main, holders));
            }
            _ => return Err(start(BuildCheck::Kinds)),
        }
    }
    Ok(book)
}

// ---- Stages --------------------------------------------------------------------

/// One call per section. A fact that fails its repair is dropped.
fn hidden_facts(
    draft: &SeedDraft,
    gm: &dyn Gm,
    ctl: &mut Control,
    roster: &Roster,
    book: &mut FactBook,
) -> Result<(), BuildFailure> {
    let parts = sections(&draft.source_text);
    report(ctl, Stage::HiddenFacts, 0, parts.len());
    for (i, part) in parts.iter().enumerate() {
        let call = hidden_call(draft, roster, book, (i + 1, parts.len()), part);
        let staged = run_stage(gm, ctl, &draft.id, &call, |out| {
            checks::hidden_facts(roster, book, out)
        })?;
        if let Some(Staged { output, drop }) = staged {
            for (j, fact) in output.facts.iter().enumerate() {
                if !drop.contains(&j) {
                    book.add(fact_entry(roster, fact, &fact.known_by));
                }
            }
        }
        report(ctl, Stage::HiddenFacts, i + 1, parts.len());
    }
    Ok(())
}

/// One call per NPC, then the WorldAutonomy check over all of them.
fn npc_states(
    draft: &SeedDraft,
    gm: &dyn Gm,
    ctl: &mut Control,
    roster: &Roster,
    book: &FactBook,
) -> Result<Vec<NpcState>, BuildFailure> {
    let npcs: Vec<&Member> = roster.npcs().collect();
    let of = npcs.len();
    if of == 0 {
        return Ok(vec![]);
    }
    report(ctl, Stage::NpcState, 0, of);
    let mut states: Vec<NpcState> = Vec::new();
    for (i, npc) in npcs.iter().enumerate() {
        let others: Vec<&NpcState> = states.iter().collect();
        let call = NpcCall {
            npc,
            at: (i + 1, of),
            others: &others,
            autonomy: None,
        };
        let state = npc_state(draft, gm, ctl, roster, book, &call)?;
        states.push(state.ok_or(fail(Stage::NpcState, BuildCheck::Schema))?);
        report(ctl, Stage::NpcState, i + 1, of);
    }
    autonomy(draft, gm, ctl, roster, book, &npcs, &mut states)?;
    Ok(states)
}

/// What one NpcState call is about.
struct NpcCall<'a> {
    npc: &'a Member,
    /// The NPC's place in the roster, from 1, and the NPC count.
    at: (usize, usize),
    /// The states of the other NPCs written so far.
    others: &'a [&'a NpcState],
    /// For a WorldAutonomy call: the items of the NPC's state that name the
    /// PC. Such a call is optional.
    autonomy: Option<&'a [String]>,
}

fn npc_state(
    draft: &SeedDraft,
    gm: &dyn Gm,
    ctl: &mut Control,
    roster: &Roster,
    book: &FactBook,
    call: &NpcCall,
) -> Result<Option<NpcState>, BuildFailure> {
    // A secret is checked against the secrets of the other NPCs too.
    let mut seen = book.clone();
    for secret in call.others.iter().filter_map(|s| s.secret.clone()) {
        seen.add(secret);
    }
    let stage_call = npc_call(draft, roster, book, call);
    let staged = run_stage(gm, ctl, &draft.id, &stage_call, |out| {
        checks::npc_state(roster, &seen, call.npc, out)
    })?;
    Ok(staged.map(|staged| npc_state_of(roster, call.npc, staged)))
}

/// When more than `PC_CENTRED_MAX_SHARE` of the goals and secrets name the
/// PC, sends one more call for each NPC with PC-naming items, most first,
/// until the share is at or below the limit. A new state replaces the old
/// one only when it names the PC less.
fn autonomy(
    draft: &SeedDraft,
    gm: &dyn Gm,
    ctl: &mut Control,
    roster: &Roster,
    book: &FactBook,
    npcs: &[&Member],
    states: &mut [NpcState],
) -> Result<(), BuildFailure> {
    let naming = |state: &NpcState| checks::pc_centred(roster, state).0;
    let centred =
        |states: &[NpcState]| checks::pc_centred_share(roster, states) > PC_CENTRED_MAX_SHARE;
    if !centred(states) {
        return Ok(());
    }
    let mut order: Vec<usize> = (0..states.len())
        .filter(|&i| naming(&states[i]) > 0)
        .collect();
    order.sort_by_key(|&i| std::cmp::Reverse(naming(&states[i])));
    for i in order {
        if !centred(states) {
            break;
        }
        let items = checks::pc_centred_items(roster, &states[i]);
        let again = {
            let others: Vec<&NpcState> = states
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, s)| s)
                .collect();
            let call = NpcCall {
                npc: npcs[i],
                at: (i + 1, npcs.len()),
                others: &others,
                autonomy: Some(&items),
            };
            npc_state(draft, gm, ctl, roster, book, &call)?
        };
        if let Some(state) = again
            && naming(&state) < naming(&states[i])
        {
            states[i] = state;
        }
    }
    if centred(states) {
        return Err(fail(Stage::NpcState, BuildCheck::WorldAutonomy));
    }
    Ok(())
}

/// One call. An event that fails its repair is dropped; zero events is a
/// valid world.
fn world_events(
    draft: &SeedDraft,
    gm: &dyn Gm,
    ctl: &mut Control,
    roster: &Roster,
    book: &FactBook,
    states: &[NpcState],
) -> Result<Vec<EventEntry>, BuildFailure> {
    report(ctl, Stage::WorldEvents, 0, 1);
    let call = events_call(draft, roster, book, states);
    let staged = run_stage(gm, ctl, &draft.id, &call, |out| {
        checks::world_events(roster, out)
    })?;
    report(ctl, Stage::WorldEvents, 1, 1);
    let Some(Staged { output, drop }) = staged else {
        return Ok(vec![]);
    };
    Ok(output
        .events
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !drop.contains(i))
        .map(|(_, e)| EventEntry {
            at: e.in_minutes,
            actor: roster.main(&e.actor),
            text: e.text,
            effects: e
                .effects
                .into_iter()
                .map(|f| EffectEntry {
                    subject: roster.main(&f.subject),
                    key: f.key,
                    value: f.value,
                })
                .collect(),
            seen_by: e.seen_by.iter().map(|n| roster.main(n)).collect(),
        })
        .collect())
}

/// One call, when an NPC is absent at the start. The call is essential; a
/// placement that fails its repair is dropped, and that NPC's place stays
/// unknown.
fn start_state(
    draft: &SeedDraft,
    gm: &dyn Gm,
    ctl: &mut Control,
    roster: &Roster,
    book: &FactBook,
) -> Result<Vec<Placement>, BuildFailure> {
    let present: Vec<&str> = draft
        .seed
        .start
        .present
        .iter()
        .filter_map(|n| roster.find(n))
        .map(|m| m.main.as_str())
        .collect();
    let absent: Vec<&Member> = roster
        .npcs()
        .filter(|m| !present.contains(&m.main.as_str()))
        .collect();
    if absent.is_empty() {
        return Ok(vec![]);
    }
    report(ctl, Stage::StartState, 0, 1);
    let call = start_call(draft, roster, book, &present, &absent);
    let staged = run_stage(gm, ctl, &draft.id, &call, |out| {
        checks::start_state(roster, book, out)
    })?;
    let Staged { output, drop } = essential(Stage::StartState, staged)?;
    report(ctl, Stage::StartState, 1, 1);
    Ok(output
        .placements
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !drop.contains(i))
        .map(|(_, p)| Placement {
            npc: roster.main(&p.npc),
            place: roster.main(&p.place),
        })
        .collect())
}

/// A checked NpcState answer, without its dropped items.
fn npc_state_of(roster: &Roster, npc: &Member, staged: Staged<NpcOut, NpcItem>) -> NpcState {
    let Staged { output, drop } = staged;
    let keep = |item| !drop.contains(&item);
    NpcState {
        goals: output
            .goals
            .into_iter()
            .enumerate()
            .filter(|(i, _)| keep(NpcItem::Goal(*i)))
            .map(|(_, g)| GoalEntry {
                npc: npc.main.clone(),
                against: checks::against(&g).map(|a| roster.main(a)),
                text: g.text,
            })
            .collect(),
        secret: output
            .secret
            .filter(|_| keep(NpcItem::Secret))
            .map(|s| fact_entry(roster, &s.fact(), std::slice::from_ref(&npc.main))),
        attitudes: output
            .attitudes
            .into_iter()
            .enumerate()
            .filter(|(i, _)| keep(NpcItem::Attitude(*i)))
            .map(|(_, a)| AttitudeEntry {
                from: npc.main.clone(),
                to: roster.main(&a.to),
                attitude: a.attitude,
            })
            .collect(),
        voice_card: VoiceCardEntry {
            npc: npc.main.clone(),
            habits: output.voice_card.habits,
            vocabulary: output.voice_card.vocabulary,
            samples: output
                .voice_card
                .samples
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .take(4)
                .collect(),
        },
    }
}

// ---- Prompts -------------------------------------------------------------------

const HIDDEN_SYSTEM: &str = "\
You extract the hidden side of a role-playing world from one section of a Source Text, as JSON. The Player never reads it.
Rules:
- Use only what this section states. Never invent a past event, a link between two entities, or a decision of the Player or of the player character (PC).
- Extract what the PC does not know: the private feelings and plans of other characters, hidden causes, what happened where the PC was not, who knows what. One fact per item.
- Never repeat a fact listed under WHAT THE PC KNOWS or HIDDEN FACTS ALREADY FOUND.
- subject: one name from the ROSTER, spelled as written there. key: a short English word in snake_case, such as role, debt, fear, plan. value: a few words. text: one sentence that states the fact, in the story language.
- known_by: the names of the characters from the ROSTER who know the fact. Never the PC and never another name of the PC. Empty when only the Game Master knows.
- open: true when the item is planned, proposed, promised or pending, so it has not happened. false when the text says that it happened or that it is true.
- Mature content becomes neutral facts. Do not remove it and do not refuse.";

const NPC_SYSTEM: &str = "\
You are the Game Master. You prepare the hidden side of one non-player character (NPC) before play starts, as JSON. The Player never reads it.
Rules:
- Stay consistent with the WORLD FACTS. Never contradict a fact. Never invent a past event. Never decide what the player character (PC) does, says, feels or wants.
- Give this NPC their own life, one that does not turn around the PC: their work, their family, their money, their fears, their other relations. Many NPCs care little about the PC. Name the PC in at most one goal or in the secret, and only when the WORLD FACTS give a reason.
- goals: one to three things this NPC wants now, each in one sentence. against: the ROSTER name of the character the goal works against, or null.
- secret: one thing this NPC hides, plausible and not melodramatic, as a fact. subject: a name from the ROSTER, often the NPC. key: a short English word in snake_case. value: a few words. text: one sentence. open: true when it is planned or pending, not done. It must be new: never restate a WORLD FACT. null when the NPC hides nothing.
- attitudes: how this NPC feels toward other names from the ROSTER, from -5 (hostile) to 5 (devoted).
- voice_card: speech habits, typical words, and two to four short sample lines. Make this voice clearly different from every voice under VOICES ALREADY WRITTEN.
- Write every text in the story language.";

const EVENTS_SYSTEM: &str = "\
You are the Game Master. You schedule the World Events: what the non-player characters (NPCs) do offscreen after play starts, whether or not the player character (PC) is there. Answer as JSON.
Rules:
- Three to six events. Each follows from an open item, a goal or a secret given below.
- actor: the ROSTER name of one NPC. Never the PC, a place or a group.
- text: one short line about what the actor does, at most 80 characters. Never narrate a scene of the PC.
- effects: zero to two changes to the world, as data. subject: a name from the ROSTER. key: a short English word in snake_case. value: a few words.
- seen_by: at most two NPC names who witness the event. Never the PC, never the actor. Empty when nobody sees it.
- in_minutes: minutes after play starts, from 10 to 20160 (two weeks). Spread them: some within a day, some later.
- Never schedule what must wait for a choice of the Player. Never decide what the PC does.
- Write every text in the story language.";

const START_SYSTEM: &str = "\
You place the non-player characters (NPCs) who are not present when play starts, as JSON.
Rules:
- placements: for each NPC under ABSENT NPCS whose place the WORLD FACTS suggest, the NPC name and one name from PLACES, spelled as written there.
- Leave an NPC out when you do not know where they are. Never invent a reason.
- Never place the PC or an NPC listed as present.";

/// `base`, plus the rule on the keys of a fact about the PC (`checks::agency`).
fn with_pc_keys(base: &str) -> String {
    format!(
        "{base}\n- A fact or an effect whose subject is the PC may only use these keys: {}. Never a choice, a word, a feeling or a motive of the PC.",
        PC_INVOLUNTARY_KEYS.join(", ")
    )
}

/// The premise, the story language and the roster.
fn context(draft: &SeedDraft, roster: &Roster) -> String {
    let lines: Vec<String> = roster
        .members()
        .iter()
        .map(|m| {
            let mut line = format!("- {}: ", m.main);
            line.push_str(match m.kind {
                EntityKind::Pc => "the player character (PC)",
                EntityKind::Npc => "npc",
                EntityKind::Place => "place",
                EntityKind::Faction => "group",
                EntityKind::Item => "item",
            });
            if let Some(age) = m.age {
                line.push_str(&format!(", age {age}"));
            }
            if !m.others.is_empty() {
                line.push_str(&format!("; also called {}", m.others.join(", ")));
            }
            line
        })
        .collect();
    format!(
        "PREMISE\n{}\n\nSTORY LANGUAGE\n{}\n\nROSTER\n{}",
        draft.seed.premise,
        draft.seed.story_language,
        lines.join("\n")
    )
}

/// One line per fact. An open item always carries its marker, and never
/// its `pending_` key.
fn fact_lines<'a>(facts: impl Iterator<Item = &'a FactEntry>, holders: bool) -> String {
    let lines: Vec<String> = facts
        .map(|f| {
            let marker = if f.open { "(open, not done) " } else { "" };
            let mut line = format!(
                "- {} {} = {}: {marker}{}",
                f.subject, f.key, f.value, f.text
            );
            if holders {
                if f.known_by.is_empty() {
                    line.push_str(" [Game Master only]");
                } else {
                    line.push_str(&format!(" [known by {}]", f.known_by.join(", ")));
                }
            }
            line
        })
        .collect();
    if lines.is_empty() {
        "none".into()
    } else {
        lines.join("\n")
    }
}

fn open_details(draft: &SeedDraft) -> String {
    let lines: Vec<String> = draft
        .seed
        .open_details
        .iter()
        .map(|d| format!("- (open, not done) {}", d.text))
        .collect();
    if lines.is_empty() {
        "none".into()
    } else {
        lines.join("\n")
    }
}

fn prompt(system: String, recent: String) -> Prompt {
    Prompt {
        system,
        recent,
        ..Prompt::default()
    }
}

fn hidden_call(
    draft: &SeedDraft,
    roster: &Roster,
    book: &FactBook,
    (n, of): (usize, usize),
    part: &str,
) -> StageCall {
    let pc = &roster.pc().main;
    let (known, hidden): (Vec<&FactEntry>, Vec<&FactEntry>) =
        book.facts().partition(|f| f.known_by.contains(pc));
    StageCall {
        stage: Stage::HiddenFacts,
        step: format!("hidden_facts {n}/{of}"),
        prompt: prompt(
            with_pc_keys(HIDDEN_SYSTEM),
            format!(
                "{}\n\nWHAT THE PC KNOWS\n{}\n\nHIDDEN FACTS ALREADY FOUND\n{}\n\nSECTION {n} OF {of}\n{part}",
                context(draft, roster),
                fact_lines(known.into_iter(), false),
                fact_lines(hidden.into_iter(), true),
            ),
        ),
        schema: schema_of::<HiddenOut>(),
        need: Need::Optional,
    }
}

fn npc_call(draft: &SeedDraft, roster: &Roster, book: &FactBook, call: &NpcCall) -> StageCall {
    let voices: Vec<String> = call
        .others
        .iter()
        .map(|s| {
            let v = &s.voice_card;
            format!("- {}: {}; {}", v.npc, v.habits, v.vocabulary)
        })
        .collect();
    let npc = call.npc;
    let mut who = npc.main.clone();
    if let Some(age) = npc.age {
        who.push_str(&format!(", age {age}"));
    }
    let (n, of) = call.at;
    let mut recent = format!(
        "{}\n\nWORLD FACTS\n{}\n\nOPEN DETAILS\n{}\n\nVOICES ALREADY WRITTEN\n{}\n\nNPC TO PREPARE\n{who}",
        context(draft, roster),
        fact_lines(book.facts(), true),
        open_details(draft),
        if voices.is_empty() {
            "none".into()
        } else {
            voices.join("\n")
        },
    );
    let mut step = format!("npc_state {n}/{of}");
    let mut need = Need::Essential;
    if let Some(items) = call.autonomy {
        recent.push_str(&format!(
            "\n\nWORLD AUTONOMY\n- WorldAutonomy: in an earlier answer for this NPC, {} named the PC. \
             Too many goals and secrets in this world are about the PC. Write goals and a secret \
             about this NPC's own life, and name the PC in at most one of them.",
            items.join(", ")
        ));
        step.push_str(" autonomy");
        need = Need::Optional;
    }
    StageCall {
        stage: Stage::NpcState,
        step,
        prompt: prompt(with_pc_keys(NPC_SYSTEM), recent),
        schema: schema_of::<NpcOut>(),
        need,
    }
}

fn events_call(
    draft: &SeedDraft,
    roster: &Roster,
    book: &FactBook,
    states: &[NpcState],
) -> StageCall {
    let goals: Vec<String> = states
        .iter()
        .flat_map(|s| &s.goals)
        .map(|g| format!("- {}: {}", g.npc, g.text))
        .collect();
    StageCall {
        stage: Stage::WorldEvents,
        step: "world_events".into(),
        prompt: prompt(
            with_pc_keys(EVENTS_SYSTEM),
            format!(
                "{}\n\nWORLD FACTS\n{}\n\nOPEN DETAILS\n{}\n\nNPC GOALS\n{}",
                context(draft, roster),
                fact_lines(book.facts(), true),
                open_details(draft),
                goals.join("\n"),
            ),
        ),
        schema: schema_of::<EventsOut>(),
        need: Need::Optional,
    }
}

fn start_call(
    draft: &SeedDraft,
    roster: &Roster,
    book: &FactBook,
    present: &[&str],
    absent: &[&Member],
) -> StageCall {
    let list = |names: Vec<&str>| {
        if names.is_empty() {
            "none".into()
        } else {
            names
                .iter()
                .map(|n| format!("- {n}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };
    let places = roster
        .members()
        .iter()
        .filter(|m| m.kind == EntityKind::Place)
        .map(|m| m.main.as_str())
        .collect();
    StageCall {
        stage: Stage::StartState,
        step: "start_state".into(),
        prompt: prompt(
            START_SYSTEM.into(),
            format!(
                "{}\n\nSTART\nThe PC is at {}. Present there: {}.\n\nABSENT NPCS\n{}\n\nPLACES\n{}\n\nWORLD FACTS\n{}",
                context(draft, roster),
                roster.main(&draft.seed.start.place),
                if present.is_empty() {
                    "nobody".into()
                } else {
                    present.join(", ")
                },
                list(absent.iter().map(|m| m.main.as_str()).collect()),
                list(places),
                fact_lines(book.facts(), true),
            ),
        ),
        schema: schema_of::<StartOut>(),
        need: Need::Essential,
    }
}

// ---- Stage outputs -------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
pub(super) struct HiddenOut {
    #[schemars(length(max = 30))]
    pub facts: Vec<FactOut>,
}

/// A hidden fact, or a secret. A secret has no `known_by`: its NPC alone
/// holds it.
#[derive(Deserialize, JsonSchema)]
pub(super) struct FactOut {
    #[schemars(length(max = 60))]
    pub subject: String,
    #[schemars(length(max = 32))]
    pub key: String,
    #[schemars(length(max = 40))]
    pub value: String,
    #[schemars(length(max = 200))]
    pub text: String,
    // Planned, proposed or pending: it has not happened.
    pub open: bool,
    #[schemars(length(max = 8), inner(length(max = 60)))]
    pub known_by: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct NpcOut {
    #[schemars(length(min = 1, max = 3))]
    pub goals: Vec<GoalOut>,
    pub secret: Option<SecretOut>,
    #[schemars(length(max = 6))]
    pub attitudes: Vec<AttitudeOut>,
    pub voice_card: VoiceOut,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct GoalOut {
    #[schemars(length(max = 160))]
    pub text: String,
    #[schemars(length(max = 60))]
    pub against: Option<String>,
}

/// The same fields as `FactOut` without `known_by`, for the schema.
#[derive(Deserialize, JsonSchema)]
pub(super) struct SecretOut {
    #[schemars(length(max = 60))]
    subject: String,
    #[schemars(length(max = 32))]
    key: String,
    #[schemars(length(max = 40))]
    value: String,
    #[schemars(length(max = 200))]
    text: String,
    open: bool,
}

impl SecretOut {
    /// The secret as a fact with no holder: code adds its NPC.
    pub fn fact(&self) -> FactOut {
        FactOut {
            subject: self.subject.clone(),
            key: self.key.clone(),
            value: self.value.clone(),
            text: self.text.clone(),
            open: self.open,
            known_by: vec![],
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct AttitudeOut {
    #[schemars(length(max = 60))]
    pub to: String,
    #[schemars(range(min = -5, max = 5))]
    pub attitude: i8,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct VoiceOut {
    #[schemars(length(max = 200))]
    pub habits: String,
    #[schemars(length(max = 200))]
    pub vocabulary: String,
    #[schemars(length(min = 2, max = 4), inner(length(max = 160)))]
    pub samples: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct EventsOut {
    #[schemars(length(min = 3, max = 6))]
    pub events: Vec<EventOut>,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct EventOut {
    #[schemars(range(min = 10, max = 20_160))]
    pub in_minutes: u64,
    #[schemars(length(max = 60))]
    pub actor: String,
    #[schemars(length(max = 80))]
    pub text: String,
    #[schemars(length(max = 2))]
    pub effects: Vec<EffectOut>,
    #[schemars(length(max = 2), inner(length(max = 60)))]
    pub seen_by: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct EffectOut {
    #[schemars(length(max = 60))]
    pub subject: String,
    #[schemars(length(max = 32))]
    pub key: String,
    #[schemars(length(max = 40))]
    pub value: String,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct StartOut {
    #[schemars(length(max = 40))]
    pub placements: Vec<PlacementOut>,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct PlacementOut {
    #[schemars(length(max = 60))]
    pub npc: String,
    #[schemars(length(max = 60))]
    pub place: String,
}
