//! The context builder (design §6.1 step 2, ADR 0001).
//!
//! Each prompt has a fixed order: `system` (rules and Content Pack fragments,
//! stable for the campaign), `stable` (the agreed parts of the Adventure
//! Seed, `adventure_block`), `scene` (place,
//! present entities with their Voice Cards, their facts and keys, open
//! obstacles; stable within a Scene), then `recent` (clock, entities named in
//! the Intent or the last 3 Turns, recent Turns, the Intent; changes every
//! Turn). Anything that depends on the Intent or on the clock stays in
//! `recent`, so the prefix stays byte-identical and Ollama can reuse it.
//!
//! Retrieval is deterministic: no embeddings, ids in ascending order.

mod narration_prompt;
mod proposal_prompt;

pub use narration_prompt::{
    FAILED_CHECK_LINE, HERE_AFTER_LINE, IMPLIED_LINE, IMPOSSIBLE_LINE, INTIMATE_RULE,
    NARRATION_RULES, NO_STAKE_LINE, NOBODY_HERE_LINE, NOT_MOVED_LINE, PC_PLACE_LINE,
    QUIET_EVENT_LINE, TIME_LINE, WON_CHECK_LINE, guard_events, guard_plans, names_unheard,
    narration_prompt, narration_prompt_with, opening_prompt, perceived_by_pc,
};
pub use proposal_prompt::{
    INTIMATE_FIELD_LINE, PROPOSAL_RULES, ProposalContext, handle_context, proposal_context,
    proposal_prompt,
};

use std::collections::BTreeSet;

use crate::{
    domain::{AdventureSeed, Canon, EntityId, EntityKind, Trigger, alias_key, is_open_key},
    normalize,
    pack::{ContentPack, InsertPoint},
    store::{Entity, Fact, Goal, StoredTurn, TurnKind, VoiceCard, WorldEvent, WorldView},
    world::{hidden, holders_of, pc_can_name, pc_knowledge},
};

/// The fewest past Turns a prompt holds, when that many exist (ADR 0001,
/// bug pass 5, M4).
pub const TURNS_WINDOW_MIN: u64 = 3;
/// The window of past Turns moves by this many Turns at once: its first
/// Turn stays the same for this many Turns in a row, so the prompt of the
/// next Turn differs from the cached one only near its end. The window
/// holds 3 to 5 Turns.
pub const TURNS_WINDOW_STEP: u64 = 3;
/// The longest window of past Turns, in chars (about 1,250 tokens). Over
/// it, the window starts one step later.
pub const TURNS_WINDOW_MAX_CHARS: usize = 5_000;
/// The longest list of what the PC knows here in the narrator's scene, in
/// chars (about 750 tokens). Over it, the oldest lines go, `PC_KNOWS_STEP`
/// at a time, so the list keeps its start for a while.
pub const PC_KNOWS_MAX_CHARS: usize = 3_000;
/// See `PC_KNOWS_MAX_CHARS`.
pub const PC_KNOWS_STEP: usize = 5;
/// The longest FACTS block of the proposal prompt, in chars (about 1,100
/// tokens). Over it, the oldest facts that are not Persistent, not a
/// `location` and not an open item go first.
pub const GM_FACTS_MAX_CHARS: usize = 4_500;
/// Entities named in this many of the latest Turns are retrieved.
pub const NAMED_TURNS: usize = 3;
/// The longest stable block an Adventure Seed may give (data shape 3.1),
/// about 600 tokens. See `adventure_block_chars`.
pub const ADVENTURE_BLOCK_MAX_CHARS: usize = 2_400;

/// The marker before the text of an open fact (ADR 0005).
const OPEN_MARKER: &str = "(open, not done)";

/// What the context builder shows for one Turn.
pub(crate) struct Retrieval<'a> {
    pub view: &'a WorldView,
    pub pc: Option<EntityId>,
    /// The PC's live `location` value.
    pub location: Option<String>,
    /// A Place entity whose name is the PC's location, if one exists.
    pub place: Option<EntityId>,
    /// Present entities, the PC included (world truth, `WorldView::present`).
    pub present: Vec<EntityId>,
    /// The entities the PC knows are here: see `seen_present`. Everything
    /// the narrator gets, and every `pc knows` marker, uses this list.
    pub seen_present: Vec<EntityId>,
    /// Place, present entities, the subjects of open obstacles, and every
    /// entity a shown goal or event names. Only ids that resolve to an entity.
    pub scene_entities: Vec<EntityId>,
    /// Live facts whose subject is the place or a present entity.
    pub scene_facts: Vec<&'a Fact>,
    /// The facts of open obstacles.
    pub obstacles: Vec<&'a Fact>,
    /// Active goals of present NPCs.
    pub goals: Vec<&'a Goal>,
    /// Scheduled World Events with an actor among `scene_entities`.
    pub events: Vec<&'a WorldEvent>,
    /// Entities named in the Intent or the last 3 Turns, not in the scene.
    pub mentioned: Vec<EntityId>,
    /// Live facts of `mentioned` entities.
    pub mentioned_facts: Vec<&'a Fact>,
}

impl<'a> Retrieval<'a> {
    pub fn new(view: &'a WorldView, intent: &str) -> Self {
        let pc = view
            .entities
            .iter()
            .find(|e| e.kind == EntityKind::Pc)
            .map(|e| e.id);
        let location = pc
            .and_then(|pc| view.live_fact(pc, "location"))
            .map(|f| f.slot.value.clone());
        let place = location.as_ref().and_then(|location| {
            view.entities_by_name(location)
                .filter(|e| e.kind == EntityKind::Place)
                .min_by_key(|e| e.id)
                .map(|e| e.id)
        });
        let exists = |id: &EntityId| view.entities.iter().any(|e| e.id == *id);
        let present: BTreeSet<EntityId> = view.present().into_iter().collect();
        let mut room: BTreeSet<EntityId> = present.clone();
        room.extend(place);
        let scene_facts: Vec<&Fact> = view
            .live_facts
            .iter()
            .filter(|f| room.contains(&f.slot.subject) && may_become_canon(view, f))
            .collect();
        let obstacles: Vec<&Fact> = view
            .obstacles_open
            .iter()
            .filter_map(|o| view.facts.iter().find(|f| f.id == o.fact_id))
            .filter(|f| exists(&f.slot.subject))
            .collect();
        let mut scene_entities = room.clone();
        scene_entities.extend(obstacles.iter().map(|f| f.slot.subject));
        let goals: Vec<&Goal> = view
            .goals_active
            .iter()
            .filter(|g| present.contains(&g.npc_id))
            .collect();
        let events: Vec<&WorldEvent> = view
            .events_scheduled
            .iter()
            .filter(|w| w.actors.iter().any(|a| scene_entities.contains(a)))
            .filter(|w| event_entities(w).all(|id| exists(&id)))
            .collect();
        // Every entity a shown goal or event names gets a line of its own and
        // a place in the schema. A goal target that does not resolve is
        // dropped by `retain` and rendered without a handle.
        scene_entities.extend(goals.iter().filter_map(|g| g.against_id));
        scene_entities.extend(events.iter().flat_map(|w| event_entities(w)));
        scene_entities.retain(|id| exists(id));

        let mut named_text = vec![intent.to_owned()];
        let turns = &view.recent_turns;
        for turn in &turns[turns.len().saturating_sub(NAMED_TURNS)..] {
            named_text.push(turn.record.intent.clone());
            named_text.extend(turn.narration.clone());
        }
        let texts: Vec<Vec<String>> = named_text.iter().map(|t| words(t)).collect();
        let mentioned: Vec<EntityId> = view
            .entities
            .iter()
            .filter(|e| !scene_entities.contains(&e.id))
            .filter(|e| {
                let name = words(&e.name);
                !name.is_empty()
                    && texts
                        .iter()
                        .any(|text| text.windows(name.len()).any(|w| w == name.as_slice()))
            })
            .map(|e| e.id)
            .collect();
        let mentioned_facts = view
            .live_facts
            .iter()
            .filter(|f| mentioned.contains(&f.slot.subject) && may_become_canon(view, f))
            .collect();

        Self {
            view,
            pc,
            location,
            place,
            present: present.into_iter().collect(),
            seen_present: seen_present(view),
            scene_entities: scene_entities.into_iter().collect(),
            scene_facts,
            obstacles,
            goals,
            events,
            mentioned,
            mentioned_facts,
        }
    }

    pub fn entity(&self, id: EntityId) -> Option<&'a Entity> {
        self.view.entities.iter().find(|e| e.id == id)
    }

    /// The entity of an id that `Retrieval` lists. Every list it builds keeps
    /// only ids that resolve, so this never fails on its own output.
    pub fn known(&self, id: EntityId) -> &'a Entity {
        self.entity(id)
            .expect("Retrieval lists only entity ids that resolve")
    }

    pub fn name(&self, id: EntityId) -> String {
        self.entity(id)
            .map_or_else(|| format!("E{id}"), |e| e.name.clone())
    }

    /// Voice Cards of the NPCs in `ids` (present entities), in id order.
    pub fn voice_cards(&self, ids: &[EntityId]) -> Vec<&'a VoiceCard> {
        ids.iter()
            .filter(|id| Some(**id) != self.pc)
            .filter_map(|id| self.view.voice_card(*id))
            .collect()
    }

    /// The distinct live keys of an entity, sorted.
    pub fn live_keys(&self, id: EntityId) -> Vec<&'a str> {
        let keys: BTreeSet<&str> = self
            .view
            .live_facts
            .iter()
            .filter(|f| f.slot.subject == id)
            .map(|f| f.slot.key.as_str())
            .collect();
        keys.into_iter().collect()
    }
}

/// The entities the PC knows are here: the PC, plus each entity whose
/// `location`, as the PC believes it (`pc_knowledge`), equals the PC's live
/// location. A location fact the PC never witnessed or heard of does not
/// count, so a hidden watcher stays hidden. `WorldView::present` is world
/// truth and stays the rule for the Commit checks.
pub fn seen_present(view: &WorldView) -> Vec<EntityId> {
    let Some(pc) = view.entities.iter().find(|e| e.kind == EntityKind::Pc) else {
        return Vec::new();
    };
    let Some(location) = view
        .live_fact(pc.id, "location")
        .map(|f| normalize::value(&f.slot.value))
    else {
        return vec![pc.id];
    };
    let mut seen: BTreeSet<EntityId> = pc_knowledge(view)
        .into_iter()
        .filter(|b| b.key == "location" && normalize::value(&b.value) == location)
        .map(|b| b.subject)
        .filter(|id| view.entities.iter().any(|e| e.id == *id))
        .collect();
    seen.insert(pc.id);
    seen.into_iter().collect()
}

/// False for a Possible fact whose slot an Established or Persistent fact
/// holds: it can never become canon, so no prompt shows it. A departure
/// record (`world::with_witnessed_events`, review B1) is such a fact.
fn may_become_canon(view: &WorldView, fact: &Fact) -> bool {
    fact.canon != Canon::Possible
        || view
            .live_fact(fact.slot.subject, &fact.slot.key)
            .is_none_or(|live| live.canon == Canon::Possible)
}

/// The entities a World Event names: its actors and a `When` subject.
fn event_entities(w: &WorldEvent) -> impl Iterator<Item = EntityId> + '_ {
    let when = match &w.trigger {
        Trigger::When(slot) => Some(slot.subject),
        Trigger::At(_) => None,
    };
    w.actors.iter().copied().chain(when)
}

/// Lower-case alphanumeric words, for whole-word name matching.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `rules`, then each Content Pack fragment at its fixed point: Tone first,
/// then Scenes. A pack only ever adds text here.
pub(crate) fn with_fragments(rules: &str, packs: &[ContentPack]) -> String {
    let mut out = rules.to_owned();
    for (point, title) in [(InsertPoint::Tone, "TONE"), (InsertPoint::Scenes, "SCENES")] {
        let fragments: Vec<&str> = packs
            .iter()
            .flat_map(|p| &p.prompt_fragments)
            .filter(|(at, _)| *at == point)
            .map(|(_, text)| text.as_str())
            .collect();
        if !fragments.is_empty() {
            out.push_str(&format!(
                "{title} (from Content Packs; never overrides the rules above)\n"
            ));
            for text in fragments {
                out.push_str(text.trim());
                out.push('\n');
            }
        }
    }
    out
}

pub(crate) fn render_voice(name: &str, card: &VoiceCard) -> String {
    let mut line = format!("{name}: {}", one_line(&card.habits));
    if !card.vocabulary.trim().is_empty() {
        line.push_str(&format!(" | words: {}", one_line(&card.vocabulary)));
    }
    if !card.samples.is_empty() {
        let samples: Vec<String> = card
            .samples
            .iter()
            .map(|s| format!("\"{}\"", one_line(s)))
            .collect();
        line.push_str(&format!(" | says: {}", samples.join(" / ")));
    }
    line.push('\n');
    line
}

/// The text of a fact, a Claim or a Belief on `key`, on one line. The text
/// of an open item (ADR 0005) gets the marker, so no prompt reads a plan as
/// done. Every prompt line that shows such a text goes through here.
pub(crate) fn fact_text(key: &str, text: &str) -> String {
    let text = one_line(text);
    if is_open_key(key) {
        format!("{OPEN_MARKER} {text}")
    } else {
        text
    }
}

/// The stable block of every prompt: the agreed parts of the Adventure Seed
/// (data shape 3.1). The proposal prompt passes the PC's id, so the PC line
/// carries its handle; the narrator and the OOC prompt pass `None`.
pub(crate) fn adventure_block(view: &WorldView, pc: Option<EntityId>) -> String {
    view.adventure_seed
        .as_ref()
        .map_or_else(|| "ADVENTURE\n".to_owned(), |seed| seed_block(seed, pc))
}

/// The length in chars of the stable block that `seed` gives the narrator,
/// without the PC's handle and with its NAMES block (review M5).
/// `ADVENTURE_BLOCK_MAX_CHARS` caps it.
pub fn adventure_block_chars(seed: &AdventureSeed) -> usize {
    seed_block(seed, None).chars().count() + seed_names_block(seed).chars().count()
}

/// Premise, the PC line with age and other names, story language,
/// boundaries, the `min_age` line, themes and the GM agreement. Never
/// `resolution` (the Ruleset owns dice, ADR 0002), `start` or
/// `open_details`.
fn seed_block(seed: &AdventureSeed, pc: Option<EntityId>) -> String {
    let mut out = String::from("ADVENTURE\n");
    out.push_str(&format!("Premise: {}\n", one_line(&seed.premise)));
    let handle = pc.map_or_else(String::new, |id| format!("E{id} "));
    let mut about = format!("age {}", seed.pc.age);
    if !seed.pc.other_names.is_empty() {
        let names: Vec<String> = seed
            .pc
            .other_names
            .iter()
            .map(|n| one_line(&n.name))
            .collect();
        about.push_str(&format!("; also called {}", names.join(", ")));
    }
    out.push_str(&format!(
        "PC: {handle}{} ({about}), {}\n",
        one_line(&seed.pc.name),
        one_line(&seed.pc.concept)
    ));
    out.push_str(&format!("Story language: {}\n", seed.story_language));
    if seed.boundaries.is_empty() {
        out.push_str("Boundaries: none beyond the rules\n");
    } else {
        out.push_str("Boundaries (always hold):\n");
        for b in &seed.boundaries {
            out.push_str(&format!("- {}\n", one_line(b)));
        }
    }
    if let Some(min_age) = seed.min_age {
        out.push_str(&format!(
            "Every character is at least {min_age} years old.\n"
        ));
    }
    if !seed.themes.is_empty() {
        let themes: Vec<String> = seed.themes.iter().map(|t| one_line(t)).collect();
        out.push_str(&format!("Themes: {}\n", themes.join(", ")));
    }
    let agreement = seed.gm_agreement.trim();
    if !agreement.is_empty() {
        out.push_str(&format!("GM agreement:\n{agreement}\n"));
    }
    out
}

/// Collapses whitespace, so one item stays on one line.
pub(crate) fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One recent Turn. The Opening Turn has no Player line: the PC did nothing
/// in it.
fn render_turn(turn: &StoredTurn) -> String {
    let mut out = if turn.record.kind == TurnKind::Opening {
        format!("Turn {} (opening)\n", turn.seq)
    } else {
        format!(
            "Turn {}\nPlayer: {}\n",
            turn.seq,
            one_line(&turn.record.intent)
        )
    };
    if let Some(narration) = turn.narration.as_deref().filter(|n| !n.trim().is_empty()) {
        out.push_str(&format!("Narration: {}\n", narration.trim()));
    }
    out
}

/// The past Turns before Turn `before_seq`, as the first block of a
/// prompt's `recent` part (ADR 0001, bug pass 5, M4).
///
/// The model's attention layers with a sliding window (about 512 tokens on
/// `gemma4:26b-a4b-it-qat`) let Ollama reuse a cached prompt only when the
/// new prompt parts from it within the last ~500 tokens of the cached
/// sequence, the answer included. The old window dropped its oldest Turn on
/// every Turn and sat after the clock line, so every proposal and every
/// narration was read again in full (QA replay: about 4,700 ms and 3,000
/// ms of prompt reading per Turn). Now the clock and the Intent come after
/// the Turns, and the window's first Turn is the latest step boundary
/// (`seq` 1, 4, 7, …, `TURNS_WINDOW_STEP` apart) that leaves at least
/// `TURNS_WINDOW_MIN` Turns: it keeps its start for three Turns in a row,
/// so the next prompt differs only after the newest Turn. Over
/// `TURNS_WINDOW_MAX_CHARS` it starts one step later, down to the last
/// Turn alone.
pub(crate) fn turns_window(view: &WorldView, before_seq: u64) -> String {
    let past: Vec<&StoredTurn> = view
        .recent_turns
        .iter()
        .filter(|t| t.seq < before_seq)
        .collect();
    let Some(last) = past.last().map(|t| t.seq) else {
        return String::new();
    };
    let mut first = if last > TURNS_WINDOW_MIN {
        ((last - TURNS_WINDOW_MIN) / TURNS_WINDOW_STEP) * TURNS_WINDOW_STEP + 1
    } else {
        1
    };
    loop {
        let rendered: String = past
            .iter()
            .filter(|t| t.seq >= first)
            .map(|t| render_turn(t))
            .collect();
        if rendered.len() <= TURNS_WINDOW_MAX_CHARS || first >= last {
            return if rendered.is_empty() {
                String::new()
            } else {
                format!("RECENT TURNS\n{rendered}")
            };
        }
        first = (first + TURNS_WINDOW_STEP).min(last);
    }
}

/// `lines` with its oldest lines left out, `step` at a time, until it is
/// at most `max` chars long or one line is left (bug pass 5, M4).
pub(crate) fn bounded(lines: &[String], max: usize, step: usize) -> &[String] {
    let mut from = 0;
    while lines.len() - from > 1 && lines[from..].concat().len() > max {
        from = (from + step).min(lines.len() - 1);
    }
    &lines[from..]
}

/// Who reads a NAMES block (review M5). The narrator reads only the other
/// names the PC knows, of a bearer the Adventure Seed does not hide, and
/// only the knowers the PC can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NamesAudience {
    Gm,
    Narrator,
}

/// One restricted other name: how it is spelled, its bearer's main name,
/// and the main names of the others who may say it.
struct NameLine {
    other: String,
    bearer: String,
    knowers: Vec<String>,
}

impl NameLine {
    fn render(&self) -> String {
        let (other, bearer) = (one_line(&self.other), one_line(&self.bearer));
        match self.knowers.as_slice() {
            [] => format!("- \"{other}\": nobody in this scene calls {bearer} this.\n"),
            knowers => {
                let names = match knowers {
                    [one] => one.clone(),
                    [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
                    [] => unreachable!("matched above"),
                };
                let verb = if knowers.len() == 1 { "calls" } else { "call" };
                format!(
                    "- \"{other}\": only {names} {verb} {bearer} this. Everyone else says \"{bearer}\".\n"
                )
            }
        }
    }
}

/// The header of the NAMES block.
const NAMES_HEADER: &str = "NAMES (an other name, and who may say it)\n";

fn render_names(lines: &[NameLine]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::from(NAMES_HEADER);
    for line in lines {
        out.push_str(&line.render());
    }
    out
}

/// The NAMES block (review M5, after bug pass 4 MA): one line per
/// restricted other name of the PC or of an NPC, with who may say it. QA:
/// Ludo Brenn called the PC "Wick", a name only Agathe and Sefa use. The
/// block sits at the end of the stable part, so it changes only when the
/// knowers change (ADR 0001); no other prompt line is marked, so the model
/// has no note to copy into the story.
///
/// The knowers are data: every holder of a Belief on the name's `alias_`
/// slot (`world::holders_of`), the Adventure Seed's `known_to` and anyone
/// who learned it in play, each resolved to its entity, so a knower the
/// seed cites by an other name ("the Grey Lady") is its entity (review B2).
/// The bearer and the PC are not listed. A name every other NPC knows has
/// no restriction and no line. The narrator's block leaves out a hidden
/// bearer, an other name the PC does not know, and each knower that is
/// hidden or that the PC cannot name (`world::pc_can_name`); and it counts
/// only the NPCs the PC can name when it decides that a name is
/// restricted, so its lines are the same with and without a hidden NPC.
pub(crate) fn names_block(view: &WorldView, audience: NamesAudience) -> String {
    let Some(pc) = view.entities.iter().find(|e| e.kind == EntityKind::Pc) else {
        return String::new();
    };
    let narrator = audience == NamesAudience::Narrator;
    let named = pc_can_name(view);
    // Who counts when the block decides that a name is restricted: every
    // NPC for the GM; for the narrator only the NPCs the PC can name, so a
    // hidden NPC never makes a line appear (review of bug pass 4, m3).
    let npcs: BTreeSet<EntityId> = view
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::Npc)
        .filter(|e| !narrator || (named.contains(&e.id) && !hidden(view, e.id)))
        .map(|e| e.id)
        .collect();
    let mut bearers: Vec<&Entity> = view
        .entities
        .iter()
        .filter(|e| matches!(e.kind, EntityKind::Pc | EntityKind::Npc))
        .filter(|e| !(narrator && hidden(view, e.id)))
        .collect();
    bearers.sort_by_key(|e| e.id);
    let mut lines = Vec::new();
    for bearer in bearers {
        for other in &bearer.other_names {
            let holders = holders_of(view, bearer.id, &alias_key(other));
            let pc_knows = bearer.id == pc.id || holders.contains(&pc.id);
            if narrator && !pc_knows {
                continue;
            }
            let knowers: Vec<EntityId> = holders
                .into_iter()
                .filter(|id| *id != bearer.id && *id != pc.id)
                .collect();
            let others: BTreeSet<EntityId> =
                npcs.iter().copied().filter(|id| *id != bearer.id).collect();
            if !others.is_empty() && others.iter().all(|id| knowers.contains(id)) {
                continue;
            }
            let knowers = knowers
                .into_iter()
                .filter(|id| !(narrator && (hidden(view, *id) || !named.contains(id))))
                .filter_map(|id| view.entities.iter().find(|e| e.id == id))
                .map(|e| e.name.clone())
                .collect();
            lines.push(NameLine {
                other: other.clone(),
                bearer: bearer.name.clone(),
                knowers,
            });
        }
    }
    render_names(&lines)
}

/// The narrator's NAMES block as the Adventure Seed alone gives it, for
/// `adventure_block_chars`: each `known_to` resolved to its entry by any of
/// its names, a hidden entry left out.
fn seed_names_block(seed: &AdventureSeed) -> String {
    let entry = |name: &str| {
        let name = normalize::name(name);
        seed.entities.iter().find(|e| {
            normalize::name(&e.name) == name
                || e.other_names
                    .iter()
                    .any(|o| normalize::name(&o.name) == name)
        })
    };
    let is_pc = |name: &str| normalize::name(name) == normalize::name(&seed.pc.name);
    let bearers = std::iter::once((seed.pc.name.as_str(), &seed.pc.other_names, false, true))
        .chain(
            seed.entities
                .iter()
                .filter(|e| e.kind == EntityKind::Npc)
                .map(|e| (e.name.as_str(), &e.other_names, e.hidden, false)),
        );
    let mut lines = Vec::new();
    for (bearer, other_names, bearer_hidden, pc) in bearers {
        if bearer_hidden {
            continue;
        }
        for other in other_names {
            if !(pc || other.known_to.iter().any(|k| is_pc(k))) {
                continue;
            }
            let mut knowers: Vec<String> = Vec::new();
            for known in &other.known_to {
                if let Some(e) = entry(known).filter(|e| !e.hidden && e.name != bearer)
                    && !knowers.contains(&e.name)
                {
                    knowers.push(e.name.clone());
                }
            }
            // As `names_block`: a name every visible NPC knows has no line.
            let mut others = seed
                .entities
                .iter()
                .filter(|e| e.kind == EntityKind::Npc && !e.hidden && e.name != bearer)
                .peekable();
            if others.peek().is_some() && others.all(|e| knowers.contains(&e.name)) {
                continue;
            }
            lines.push(NameLine {
                other: other.name.clone(),
                bearer: bearer.to_owned(),
                knowers,
            });
        }
    }
    render_names(&lines)
}
