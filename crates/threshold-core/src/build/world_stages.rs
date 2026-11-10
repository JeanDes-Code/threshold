//! The four World Build stages: hidden facts, the hidden state of each NPC,
//! World Events and the start state (design 6.3, data shape 3.6). Each
//! stage output is a private type with length caps; code checks it
//! (`checks`) and turns it into a `WorldSetup`, which it never writes.

use schemars::JsonSchema;
use serde::Deserialize;

use super::{
    BuildCheck, BuildFailure, Control, DraftState, SeedDraft, Stage,
    checks::{self, FactBook, Member, NpcItem, Roster, fact_entry, location},
    draft::gaps,
    stages::{Staged, essential, report, run_stage, sections},
    world_prompts::{events_call, hidden_call, npc_call, start_call},
};
use crate::{
    domain::{
        AttitudeEntry, EffectEntry, EntityKind, EventEntry, FactEntry, GoalEntry, VoiceCardEntry,
        WorldSetup,
    },
    model::Gm,
    normalize,
};

/// The highest share of all NPC goals and secrets that may name the PC. A
/// world above it turns around the PC (design 5). The live evaluation of
/// 2026-11-10 measured 3% on average and 5% at most over four built worlds
/// (`evals/reports/2026-11-world-build.md`), so this limit is a wide guard.
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
///
/// The state alone is not enough: a draft with any gap now (an old file
/// that says `approved` with no age confirmation) fails as `NotApproved`
/// before any call. The caller runs `approve` to get the gaps and to put
/// the draft back to `Draft`.
pub fn build_world(
    draft: &SeedDraft,
    gm: &dyn Gm,
    ctl: &mut Control,
) -> Result<WorldSetup, BuildFailure> {
    if draft.state != DraftState::Approved || !gaps(draft).is_empty() {
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
/// marked hidden gets no PC holder. A known fact on a taken slot fails the
/// build at `HiddenFacts`, the stage that loads the known facts; the start
/// place fails it at `StartState`.
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
        book.known(FactEntry {
            subject: roster.main(&known.subject),
            key: normalize::key(&known.key),
            value: known.value.clone(),
            text: known.text.clone(),
            known_by: vec![pc.clone()],
            open: known.open,
        })
        .map_err(|check| fail(Stage::HiddenFacts, check))?;
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
        if let Some(Staged { output, drop, .. }) = staged {
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
pub(super) struct NpcCall<'a> {
    pub npc: &'a Member,
    /// The NPC's place in the roster, from 1, and the NPC count.
    pub at: (usize, usize),
    /// The states of the other NPCs written so far.
    pub others: &'a [&'a NpcState],
    /// For a WorldAutonomy call: the items of the NPC's state that name the
    /// PC. Such a call is optional.
    pub autonomy: Option<&'a [String]>,
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
    let Some(Staged { output, drop, .. }) = staged else {
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

/// One call, when an NPC is absent at the start and a place other than the
/// start place exists. The call is essential; a placement that fails its
/// repair is dropped, and that NPC's place stays unknown.
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
    // An absent NPC is never at the start place.
    let start = roster.main(&draft.seed.start.place);
    let places: Vec<&str> = roster
        .members()
        .iter()
        .filter(|m| m.kind == EntityKind::Place && m.main != start)
        .map(|m| m.main.as_str())
        .collect();
    if absent.is_empty() || places.is_empty() {
        return Ok(vec![]);
    }
    report(ctl, Stage::StartState, 0, 1);
    let call = start_call(draft, roster, book, &present, &absent, &places);
    let staged = run_stage(gm, ctl, &draft.id, &call, |out| {
        checks::start_state(roster, book, &draft.seed.start.place, out)
    })?;
    let Staged { output, drop, .. } = essential(Stage::StartState, staged)?;
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
    let Staged { output, drop, .. } = staged;
    let keep = |item| !drop.contains(&item);
    NpcState {
        goals: output
            .goals
            .into_iter()
            .enumerate()
            .filter(|(i, _)| keep(NpcItem::Goal(*i)))
            .map(|(i, g)| GoalEntry {
                npc: npc.main.clone(),
                against: checks::against(&g)
                    .filter(|_| keep(NpcItem::Against(i)))
                    .map(|a| roster.main(a)),
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
