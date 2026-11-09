//! The Decision Proposal prompt. The GM model sees every fact it retrieves,
//! hidden ones included, marked as such.

use super::{Retrieval, one_line, recent_block, render_voice, with_fragments};
use crate::{
    HandleContext,
    domain::{Canon, EntityId, Handle, Trigger},
    model::Prompt,
    pack::ContentPack,
    store::{Entity, Fact},
};

/// The rules part of the system prompt. Its first lines are the wording
/// measured by the live probe (`threshold-ollama/examples/probe.rs`); the
/// VERDICT and LATER ACTIONS parts answer two failures that probe found: a
/// Check on every proposal, and no scheduled event for a delayed action.
pub const PROPOSAL_RULES: &str = "\
You are the Game Master for a single-player tabletop role-playing game. You decide what the world and its NPCs do; the player decides what the player character (PC) does.
Never decide the player character's actions, words, or feelings.
Return only one-line minified JSON with no spaces or newlines, matching the supplied schema. Omit optional fields that are empty.
Reference entities, facts, events and goals only by their handles (E*, F*, W*, G*). A new NPC uses N1.
Stake values and reaction plans must be short snake_case or at most 6 words.
Facts marked hidden are known to the GM only: they may drive NPCs, but never show them to the PC.
VERDICT
- allowed: the action simply happens. This is the usual verdict. Talking, greeting, asking, insulting, moving, looking and ordinary actions are allowed; NPCs answer through reactions.
- Words are never a check: whatever the PC says, even an insult or a threat, is said. The consequence is the NPCs' reactions and their later actions.
- impossible: the world prevents it outright. List the facts that block it in blockers.
- check: only when BOTH are true: the outcome is truly uncertain, AND failure has real stakes (danger, an NPC who actively opposes the PC, an open obstacle). Then include check with win and lose stakes as slot writes, and factors that cite existing facts.
- If either part is missing, do not use check: choose allowed or impossible. Include check only when verdict is check. Never set a numeric difficulty.
LATER ACTIONS
- reactions: what each affected NPC intends right now, as a short plan, not prose.
- When an NPC decides to act later (revenge, a report, a visit, fetching someone), use schedule: actor = that NPC, in_minutes = minutes until it happens, text = what happens, effects = the slot writes it causes. Never record such a plan as a fact.
- An NPC who is insulted, threatened or wronged, and who would hit back later rather than now, gets a schedule.
- facts: only named or significant new details, each with seen = pc, present or gm. Reuse the keys listed for each entity.
- A slot is subject (a handle), key (a plain word such as mood or suspicion, never a handle) and value. On the PC, only write condition, injury, fatigue, carrying, reputation, standing or debt; put other consequences on NPCs, places or things.
- This holds for schedule effects too. A grudge or a complaint lives on the NPC: subject = the NPC's handle, key = grudge, value = the PC's name.
- minutes: the game time this action takes.
";

/// The prompt and the handles it shows. The per-Turn schema
/// (`proposal_schema_for`) must use exactly these handles.
#[derive(Clone, Debug)]
pub struct ProposalContext {
    pub prompt: Prompt,
    pub handles: HandleContext,
}

pub fn proposal_prompt(
    view: &crate::store::WorldView,
    packs: &[ContentPack],
    intent: &str,
) -> Prompt {
    proposal_context(view, packs, intent).prompt
}

/// The handles that `proposal_prompt(view, _, intent)` shows. Packs never
/// change them.
pub fn handle_context(view: &crate::store::WorldView, intent: &str) -> HandleContext {
    handles(&Retrieval::new(view, intent))
}

pub fn proposal_context(
    view: &crate::store::WorldView,
    packs: &[ContentPack],
    intent: &str,
) -> ProposalContext {
    let r = Retrieval::new(view, intent);
    ProposalContext {
        prompt: Prompt {
            system: system(packs),
            stable: stable(&r),
            scene: scene(&r),
            recent: recent(&r, intent),
        },
        handles: handles(&r),
    }
}

fn handles(r: &Retrieval<'_>) -> HandleContext {
    let mut entities: Vec<EntityId> = r.scene_entities.clone();
    entities.extend(&r.mentioned);
    entities.sort_unstable();
    entities.dedup();
    let mut facts: Vec<u64> = r
        .scene_facts
        .iter()
        .chain(&r.obstacles)
        .chain(&r.mentioned_facts)
        .map(|f| f.id)
        .collect();
    facts.sort_unstable();
    facts.dedup();
    HandleContext {
        entities: entities.into_iter().map(Handle::Entity).collect(),
        facts: facts.into_iter().map(Handle::Fact).collect(),
        events: r.events.iter().map(|w| Handle::WorldEvent(w.id)).collect(),
        goals: r.goals.iter().map(|g| Handle::Goal(g.id)).collect(),
        new_slots: 1,
    }
}

fn system(packs: &[ContentPack]) -> String {
    with_fragments(PROPOSAL_RULES, packs)
}

fn stable(r: &Retrieval<'_>) -> String {
    let mut out = String::from("ADVENTURE\n");
    if let Some(seed) = &r.view.adventure_seed {
        out.push_str(&format!("Premise: {}\n", one_line(&seed.premise)));
        let pc = r.pc.map_or_else(String::new, |id| format!("E{id} "));
        out.push_str(&format!(
            "PC: {pc}{}, {}\n",
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
    }
    out
}

/// One entity line. It takes the `Entity` itself, so an unknown id can never
/// be rendered as an empty line.
fn entity_line(r: &Retrieval<'_>, e: &Entity) -> String {
    let id = e.id;
    let kind = serde_json::to_value(e.kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut line = format!("E{id} {} ({kind}", e.name);
    match e.adult {
        Some(true) => line.push_str(", adult"),
        Some(false) => line.push_str(", minor"),
        None => {}
    }
    line.push(')');
    if let Some(pc) = r.pc.filter(|pc| *pc != id)
        && let Some(a) = r.view.attitudes.iter().find(|a| a.from == id && a.to == pc)
    {
        line.push_str(&format!("; attitude to PC {}", a.attitude));
    }
    let keys = r.live_keys(id);
    if !keys.is_empty() {
        line.push_str(&format!("; keys: {}", keys.join(", ")));
    }
    line.push('\n');
    line
}

fn fact_line(r: &Retrieval<'_>, f: &Fact) -> String {
    let canon = match f.canon {
        Canon::Possible => "possible",
        Canon::Established => "established",
        Canon::Persistent => "persistent",
    };
    // The location of an entity the PC knows is here is not hidden, even
    // when the PC learned it from another fact (`seen_present`).
    let in_view = f.slot.key == "location" && r.seen_present.contains(&f.slot.subject);
    let seen = if in_view || r.pc.is_some_and(|pc| f.seen_by.contains(&pc)) {
        "pc knows"
    } else {
        "hidden"
    };
    format!(
        "F{} E{} {} = {} ({canon}, {seen})\n",
        f.id, f.slot.subject, f.slot.key, f.slot.value
    )
}

fn scene(r: &Retrieval<'_>) -> String {
    let mut out = String::from("SCENE\n");
    match (&r.location, r.place) {
        (Some(location), Some(place)) => out.push_str(&format!("Place: {location} (E{place})\n")),
        (Some(location), None) => out.push_str(&format!("Place: {location}\n")),
        _ => out.push_str("Place: unknown\n"),
    }
    out.push_str("ENTITIES\n");
    for id in &r.scene_entities {
        out.push_str(&entity_line(r, r.known(*id)));
    }
    let voices = r.voice_cards(&r.present);
    if !voices.is_empty() {
        out.push_str("VOICES\n");
        for card in voices {
            out.push_str(&render_voice(&format!("E{}", card.npc), card));
        }
    }
    if !r.scene_facts.is_empty() {
        out.push_str("FACTS\n");
        for f in &r.scene_facts {
            out.push_str(&fact_line(r, f));
        }
    }
    if !r.obstacles.is_empty() {
        out.push_str("OPEN OBSTACLES\n");
        for f in &r.obstacles {
            out.push_str(&fact_line(r, f));
        }
    }
    if !r.goals.is_empty() {
        out.push_str("NPC GOALS\n");
        for g in &r.goals {
            let against = match (g.against_id, r.pc) {
                (Some(a), Some(pc)) if a == pc => " (against PC)".to_owned(),
                (Some(a), _) if r.scene_entities.contains(&a) => format!(" (against E{a})"),
                _ => String::new(),
            };
            out.push_str(&format!(
                "G{} E{}: {}{against}\n",
                g.id,
                g.npc_id,
                one_line(&g.text)
            ));
        }
    }
    if !r.events.is_empty() {
        out.push_str("SCHEDULED EVENTS\n");
        for w in &r.events {
            let when = match &w.trigger {
                Trigger::At(minute) => format!("at minute {minute}"),
                Trigger::When(slot) => {
                    format!("when E{} {} = {}", slot.subject, slot.key, slot.value)
                }
            };
            let actors: Vec<String> = w.actors.iter().map(|a| format!("E{a}")).collect();
            out.push_str(&format!(
                "W{} {when}, actor {}: {}\n",
                w.id,
                actors.join(" "),
                one_line(&w.text)
            ));
        }
    }
    out
}

fn recent(r: &Retrieval<'_>, intent: &str) -> String {
    let mut head = format!("NOW\nMinute {}\n", r.view.clock_min);
    if !r.mentioned.is_empty() {
        head.push_str("MENTIONED ENTITIES (not present)\n");
        for id in &r.mentioned {
            head.push_str(&entity_line(r, r.known(*id)));
        }
        if !r.mentioned_facts.is_empty() {
            head.push_str("MENTIONED FACTS\n");
            for f in &r.mentioned_facts {
                head.push_str(&fact_line(r, f));
            }
        }
    }
    let tail = format!("INTENT\n{}\n", intent.trim());
    recent_block(r.view, &head, &tail)
}
