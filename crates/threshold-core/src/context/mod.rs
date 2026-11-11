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
    FAILED_CHECK_LINE, IMPOSSIBLE_LINE, NARRATION_RULES, NOT_MOVED_LINE, PC_PLACE_LINE, TIME_LINE,
    WON_CHECK_LINE, guard_events, guard_plans, narration_prompt, opening_prompt, perceived_by_pc,
};
pub use proposal_prompt::{ProposalContext, handle_context, proposal_context, proposal_prompt};

use std::collections::BTreeSet;

use crate::{
    domain::{AdventureSeed, Canon, EntityId, EntityKind, Trigger, is_open_key},
    model::Prompt,
    normalize,
    pack::{ContentPack, InsertPoint},
    store::{Entity, Fact, Goal, StoredTurn, TurnKind, VoiceCard, WorldEvent, WorldView},
    world::pc_knowledge,
};

/// At most this many recent Turns go into a prompt.
pub const RECENT_TURNS: usize = 6;
/// Oldest Turns are cut until the `recent` block is at most this long.
pub const RECENT_MAX_CHARS: usize = 6_000;
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
/// without the PC's handle and with its other-name notes (bug pass 4).
/// `ADVENTURE_BLOCK_MAX_CHARS` caps it.
pub fn adventure_block_chars(seed: &AdventureSeed) -> usize {
    let notes = seed_alias_notes(seed, AliasAudience::Narrator);
    mark_aliases(&seed_block(seed, None), &notes)
        .chars()
        .count()
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

/// Builds the `recent` block: `head`, the recent Turns, then `tail`. The
/// last `RECENT_TURNS` Turns are kept, then the oldest are cut until the
/// block is at most `RECENT_MAX_CHARS` long (or no Turn is left).
pub(crate) fn recent_block(view: &WorldView, head: &str, tail: &str) -> String {
    let turns = &view.recent_turns;
    let mut rendered: Vec<String> = turns[turns.len().saturating_sub(RECENT_TURNS)..]
        .iter()
        .map(render_turn)
        .collect();
    let build = |rendered: &[String]| {
        let mut out = head.to_owned();
        if !rendered.is_empty() {
            out.push_str("RECENT TURNS\n");
            out.push_str(&rendered.concat());
        }
        out.push_str(tail);
        out
    };
    let mut out = build(&rendered);
    while out.len() > RECENT_MAX_CHARS && !rendered.is_empty() {
        rendered.remove(0);
        out = build(&rendered);
    }
    out
}

/// Who may say an other name, for the prompts (bug pass 4, MA). QA: Ludo
/// Brenn called the PC "Wick" in three Narrations, a name the Adventure Seed
/// gives to Agathe Morrow and Sefa Lind only. The name stood unmarked in the
/// premise, on the PC line, in the PC's Beliefs and in the recent Turns, and
/// one prompt serves every NPC of the Scene.
#[derive(Clone, Debug)]
pub(crate) struct AliasNote {
    /// The other name's words (`normalize::name_words`), article left out.
    words: Vec<String>,
    /// The note that follows each line that uses the name.
    note: String,
}

/// Which prompt the notes are for. The narrator gets only the other names
/// the PC knows, and never a hidden Adventure Seed entry as a knower.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AliasAudience {
    Gm,
    Narrator,
}

/// One note per other name in the Adventure Seed: the PC's, and each
/// entry's. Its knowers are the entry's `known_to`, without the bearer. The
/// narrator's notes leave out a hidden entry (as a bearer and as a knower)
/// and an NPC's other name that the PC does not know (no `known_to` entry
/// for the PC): the narrator must not learn that name, or who knows it.
pub(crate) fn alias_notes(view: &WorldView, audience: AliasAudience) -> Vec<AliasNote> {
    view.adventure_seed
        .as_ref()
        .map_or_else(Vec::new, |seed| seed_alias_notes(seed, audience))
}

/// `alias_notes` from the Adventure Seed alone: an entry is hidden when the
/// seed says so, as `world::hidden` reads it.
fn seed_alias_notes(seed: &AdventureSeed, audience: AliasAudience) -> Vec<AliasNote> {
    let is_pc = |name: &str| normalize::name(name) == normalize::name(&seed.pc.name);
    let hidden_name = |name: &str| {
        seed.entities
            .iter()
            .any(|e| e.hidden && normalize::name(&e.name) == normalize::name(name))
    };
    let narrator = audience == AliasAudience::Narrator;
    let bearers = std::iter::once((seed.pc.name.as_str(), &seed.pc.other_names, false)).chain(
        seed.entities
            .iter()
            .map(|e| (e.name.as_str(), &e.other_names, e.hidden)),
    );
    let mut notes = Vec::new();
    for (bearer, other_names, bearer_hidden) in bearers {
        if narrator && bearer_hidden {
            continue;
        }
        for other in other_names {
            let pc_knows = is_pc(bearer) || other.known_to.iter().any(|k| is_pc(k));
            if narrator && !pc_knows {
                continue;
            }
            let words = normalize::name_words(&normalize::name(&other.name));
            if words.is_empty() {
                continue;
            }
            let knowers: Vec<String> = other
                .known_to
                .iter()
                .filter(|k| normalize::name(k) != normalize::name(bearer))
                .filter(|k| !(narrator && hidden_name(k)))
                .map(|k| one_line(k))
                .collect();
            let name = one_line(&other.name);
            let note = if knowers.is_empty() {
                format!("[{name}: a name no other character knows; nobody says it]")
            } else {
                let verb = if knowers.len() == 1 { "knows" } else { "know" };
                format!(
                    "[{name}: a name only {} {verb}; nobody else says it]",
                    knowers.join(" and ")
                )
            };
            notes.push(AliasNote { words, note });
        }
    }
    notes
}

/// `text` with the note of each other name it uses after the line that uses
/// it, once per line. A line that already holds the note keeps it as it is.
pub(crate) fn mark_aliases(text: &str, notes: &[AliasNote]) -> String {
    if notes.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let (body, end) = match line.strip_suffix('\n') {
            Some(body) => (body, "\n"),
            None => (line, ""),
        };
        out.push_str(body);
        let words = normalize::name_words(body);
        for note in notes {
            let uses = words
                .windows(note.words.len())
                .any(|w| w == note.words.as_slice());
            if uses && !body.contains(&note.note) {
                out.push(' ');
                out.push_str(&note.note);
            }
        }
        out.push_str(end);
    }
    out
}

/// `prompt` with every use of an other name marked (`mark_aliases`), in
/// each block but the rules.
pub(crate) fn with_alias_notes(
    mut prompt: Prompt,
    view: &WorldView,
    audience: AliasAudience,
) -> Prompt {
    let notes = alias_notes(view, audience);
    prompt.stable = mark_aliases(&prompt.stable, &notes);
    prompt.scene = mark_aliases(&prompt.scene, &notes);
    prompt.recent = mark_aliases(&prompt.recent, &notes);
    prompt
}
