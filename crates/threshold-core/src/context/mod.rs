//! The context builder (design §6.1 step 2, ADR 0001).
//!
//! Each prompt has a fixed order: `system` (rules and Content Pack fragments,
//! stable for the campaign), `stable` (the Adventure Seed), `scene` (place,
//! present entities with their Voice Cards, their facts and keys, open
//! obstacles; stable within a Scene), then `recent` (clock, entities named in
//! the Intent or the last 3 Turns, recent Turns, the Intent; changes every
//! Turn). Anything that depends on the Intent or on the clock stays in
//! `recent`, so the prefix stays byte-identical and Ollama can reuse it.
//!
//! Retrieval is deterministic: no embeddings, ids in ascending order.

mod narration_prompt;
mod proposal_prompt;

pub(crate) use narration_prompt::adventure_block;
pub use narration_prompt::{guard_plans, narration_prompt, perceived_by_pc};
pub use proposal_prompt::{ProposalContext, handle_context, proposal_context, proposal_prompt};

use std::collections::BTreeSet;

use crate::{
    domain::{EntityId, EntityKind, Trigger},
    normalize,
    pack::{ContentPack, InsertPoint},
    store::{Entity, Fact, Goal, StoredTurn, VoiceCard, WorldEvent, WorldView},
    world::pc_knowledge,
};

/// At most this many recent Turns go into a prompt.
pub const RECENT_TURNS: usize = 6;
/// Oldest Turns are cut until the `recent` block is at most this long.
pub const RECENT_MAX_CHARS: usize = 6_000;
/// Entities named in this many of the latest Turns are retrieved.
pub const NAMED_TURNS: usize = 3;

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
            let location = normalize::name(location);
            view.entities
                .iter()
                .find(|e| e.kind == EntityKind::Place && e.norm_name == location)
                .map(|e| e.id)
        });
        let exists = |id: &EntityId| view.entities.iter().any(|e| e.id == *id);
        let present: BTreeSet<EntityId> = view.present().into_iter().collect();
        let mut room: BTreeSet<EntityId> = present.clone();
        room.extend(place);
        let scene_facts: Vec<&Fact> = view
            .live_facts
            .iter()
            .filter(|f| room.contains(&f.slot.subject))
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
            .filter(|f| mentioned.contains(&f.slot.subject))
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
pub(crate) fn seen_present(view: &WorldView) -> Vec<EntityId> {
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

/// Collapses whitespace, so one item stays on one line.
pub(crate) fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_turn(turn: &StoredTurn) -> String {
    let mut out = format!(
        "Turn {}\nPlayer: {}\n",
        turn.seq,
        one_line(&turn.record.intent)
    );
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
