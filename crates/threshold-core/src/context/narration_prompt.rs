//! The Narration prompt and the narration filter (design §6.1 step 7).
//!
//! The narrator gets only what the PC can perceive: the PC's place, who the
//! PC knows is here (`seen_present`), the PC's own Beliefs about them, their
//! Voice Cards, Content Pack fragments,
//! the committed ops the PC perceived, the texts of the World Events the PC
//! witnessed fire this Turn (`world::with_witnessed_events`, filtered by
//! `guard_events`), and short NPC reaction intents (filtered by
//! `guard_plans`). It never gets a fact by id, a hidden fact, a key list, a
//! goal, or a World Event that is still scheduled or that the PC did not
//! witness. This is the real protection against leaks.

use std::collections::HashSet;

use super::{Retrieval, one_line, recent_block, render_voice, seen_present, with_fragments};
use crate::{
    domain::{EntityId, EntityKind, EntityRef, Handle, Knowing, LedgerOp, Reaction, is_state_key},
    model::Prompt,
    normalize,
    pack::ContentPack,
    store::WorldView,
    world::{hidden, openly_here, pc_knowledge},
};

pub const NARRATION_RULES: &str = "\
You are the narrator of a single-player tabletop role-playing game. Write what the player character (PC) perceives, in the second person, in the story language.
Narrate only the outcome given under THIS TURN. Never add a fact that changes the world, and never reveal what the PC cannot perceive.
Never decide the PC's actions, words, or feelings beyond the player's intent.
NPC intentions tell you how to portray an NPC. Show them through words and manner; never state them as fact.
Give each NPC the voice of their Voice Card.
Never say that more time passed than THIS TURN states. If no minutes pass, time does not pass.
Write two short paragraphs at most. Plain prose, no headings, no lists.
";

/// The ops of one Turn that the PC perceives, in their order. A fact or a
/// supersession counts when the PC is among its witnesses; a Claim when the
/// PC hears it now; a new entity when the PC knows it is here after the
/// Commit (`seen_present`); a Roll
/// and the passing of time always. Everything else (hidden facts, scheduled
/// events, goals, attitudes, knowledge rows, Voice Cards) is dropped.
pub fn perceived_by_pc(ops: &[LedgerOp], view_after: &WorldView) -> Vec<LedgerOp> {
    let Some(pc) = view_after
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Pc)
        .map(|e| e.id)
    else {
        return Vec::new();
    };
    let pc_ref = EntityRef::Id(pc);
    let present = seen_present(view_after);
    ops.iter()
        .filter(|op| match op {
            LedgerOp::AddFact { seen_by, .. } => seen_by.contains(&pc_ref),
            LedgerOp::SupersedeFact { new, .. } => new.seen_by.contains(&pc_ref),
            LedgerOp::AddClaim {
                listeners,
                delay_min,
                ..
            } => *delay_min == 0 && listeners.contains(&pc_ref),
            LedgerOp::AddEntity { kind, name, .. } => {
                *kind != EntityKind::Pc
                    && view_after
                        .entities_by_name(name)
                        .any(|e| present.contains(&e.id))
            }
            LedgerOp::RecordRoll { .. } | LedgerOp::AdvanceClock { .. } => true,
            _ => false,
        })
        .cloned()
        .collect()
}

/// The reactions as the narrator may see them. A plan is model text and can
/// quote a GM-only fact, so a plan that names a secret is dropped; the NPC
/// and the stance stay.
///
/// A secret is a live fact on a key that is not a state key, with no PC
/// Belief on the same slot and value. A plan names it when the fact's key
/// or value appears in the plan as whole words, after normalization
/// (case, articles, `_` and punctuation count as word breaks). A key or
/// value that is an entity name is not a secret word: names are public.
pub fn guard_plans(view: &WorldView, reactions: &[Reaction]) -> Vec<Reaction> {
    let secrets = secret_terms(view);
    reactions
        .iter()
        .map(|r| {
            let mut r = r.clone();
            if quotes(&secrets, &r.plan) {
                r.plan.clear();
            }
            r
        })
        .collect()
}

/// The texts of witnessed World Events as the narrator may see them. An
/// event text is model text, like a plan: a text that names a secret (the
/// same whole-word match as `guard_plans`) is dropped. So is a text that
/// names an NPC the PC must not know of: one the Adventure Seed hides, or
/// one that neither stands openly here (`world::openly_here`) nor is known
/// to the PC (seen here, or the subject of a PC Belief).
pub fn guard_events(view: &WorldView, events: &[String]) -> Vec<String> {
    let mut forbidden = secret_terms(view);
    let present = seen_present(view);
    let known: HashSet<EntityId> = pc_knowledge(view).into_iter().map(|b| b.subject).collect();
    forbidden.extend(
        view.entities
            .iter()
            .filter(|e| e.kind == EntityKind::Npc)
            .filter(|e| {
                hidden(view, e.id)
                    || !(openly_here(view, e.id)
                        || present.contains(&e.id)
                        || known.contains(&e.id))
            })
            .map(|e| words(&e.name))
            .filter(|name| !name.is_empty()),
    );
    events
        .iter()
        .filter(|text| !quotes(&forbidden, text))
        .cloned()
        .collect()
}

/// The words of every live fact the PC holds no Belief about, on a key that
/// is not a state key: its key and its value. Entity names are public, so a
/// key or value that is an entity name is left out.
fn secret_terms(view: &WorldView) -> Vec<Vec<String>> {
    let known: HashSet<(EntityId, String, String)> = pc_knowledge(view)
        .into_iter()
        .map(|b| (b.subject, b.key, normalize::value(&b.value)))
        .collect();
    let names: HashSet<Vec<String>> = view.entities.iter().map(|e| words(&e.name)).collect();
    view.live_facts
        .iter()
        .filter(|f| !is_state_key(&f.slot.key))
        .filter(|f| {
            !known.contains(&(
                f.slot.subject,
                f.slot.key.clone(),
                normalize::value(&f.slot.value),
            ))
        })
        .flat_map(|f| [words(&f.slot.key), words(&f.slot.value)])
        .filter(|term| !term.is_empty() && !names.contains(term))
        .collect()
}

/// True when `text` contains one of `terms` as whole words.
fn quotes(terms: &[Vec<String>], text: &str) -> bool {
    let text = words(text);
    terms.iter().any(|term| {
        text.windows(term.len())
            .any(|window| window == term.as_slice())
    })
}

/// The words of a text after normalization: lowercase, article dropped, and
/// split on everything that is not a letter or a digit.
fn words(text: &str) -> Vec<String> {
    let text = text.replace('_', " ");
    normalize::value(&text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn narration_prompt(
    view_after: &WorldView,
    packs: &[ContentPack],
    perceived: &[LedgerOp],
    events: &[String],
    reactions: &[Reaction],
    intent: &str,
) -> Prompt {
    let r = Retrieval::new(view_after, "");
    Prompt {
        system: with_fragments(NARRATION_RULES, packs),
        stable: adventure_block(view_after),
        scene: scene(&r),
        recent: recent(&r, perceived, events, reactions, intent),
    }
}

/// The stable block: the Adventure Seed's premise, PC, story language and
/// boundaries. The OOC prompt uses it too.
pub(crate) fn adventure_block(view: &WorldView) -> String {
    let mut out = String::from("ADVENTURE\n");
    if let Some(seed) = &view.adventure_seed {
        out.push_str(&format!("Premise: {}\n", one_line(&seed.premise)));
        out.push_str(&format!(
            "PC: {}, {}\n",
            one_line(&seed.pc.name),
            one_line(&seed.pc.concept)
        ));
        out.push_str(&format!("Story language: {}\n", seed.story_language));
        for b in &seed.boundaries {
            out.push_str(&format!("Boundary: {}\n", one_line(b)));
        }
    }
    out
}

fn scene(r: &Retrieval<'_>) -> String {
    let mut out = String::from("SCENE\n");
    out.push_str(&format!(
        "Place: {}\n",
        r.location.as_deref().unwrap_or("unknown")
    ));
    // Only who the PC knows is here (`seen_present`), never world truth: a
    // hidden watcher must not reach the narrator.
    let others: Vec<String> = r
        .seen_present
        .iter()
        .filter(|id| Some(**id) != r.pc)
        .map(|id| r.name(*id))
        .collect();
    if !others.is_empty() {
        out.push_str(&format!("Present: {}\n", others.join(", ")));
    }
    let voices = r.voice_cards(&r.seen_present);
    if !voices.is_empty() {
        out.push_str("VOICES\n");
        for card in voices {
            out.push_str(&render_voice(&r.name(card.npc), card));
        }
    }
    let mut shown: Vec<EntityId> = r.seen_present.clone();
    shown.extend(r.place);
    let known: Vec<String> = pc_knowledge(r.view)
        .into_iter()
        .filter(|b| shown.contains(&b.subject))
        .map(|b| match (b.how, b.source) {
            (Knowing::Told, Some(speaker)) => {
                format!("- {} (told by {})\n", one_line(&b.text), r.name(speaker))
            }
            _ => format!("- {}\n", one_line(&b.text)),
        })
        .collect();
    if !known.is_empty() {
        out.push_str("WHAT THE PC KNOWS HERE\n");
        out.push_str(&known.concat());
    }
    out
}

fn entity_name(r: &Retrieval<'_>, entity: &EntityRef, perceived: &[LedgerOp]) -> String {
    match entity {
        EntityRef::Id(id) => r.name(*id),
        EntityRef::New(local) => new_name(*local, perceived),
    }
}

fn new_name(local: u8, perceived: &[LedgerOp]) -> String {
    perceived
        .iter()
        .find_map(|op| match op {
            LedgerOp::AddEntity {
                local: Some(l),
                name,
                ..
            } if *l == local => Some(name.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "a newcomer".into())
}

fn outcome_line(r: &Retrieval<'_>, op: &LedgerOp, perceived: &[LedgerOp]) -> Option<String> {
    Some(match op {
        LedgerOp::AddFact { text, .. } => format!("- {}\n", one_line(text)),
        LedgerOp::SupersedeFact { new, .. } => format!("- {}\n", one_line(&new.text)),
        LedgerOp::AddClaim { claim, .. } => format!(
            "- {} says: {}\n",
            entity_name(r, &claim.speaker, perceived),
            one_line(&claim.text)
        ),
        LedgerOp::AddEntity { name, .. } => format!("- {} is here.\n", one_line(name)),
        LedgerOp::RecordRoll { outcome, .. } => format!("- The Check result: {outcome}.\n"),
        LedgerOp::AdvanceClock { minutes } => format!("- {minutes} minutes pass.\n"),
        _ => return None,
    })
}

fn recent(
    r: &Retrieval<'_>,
    perceived: &[LedgerOp],
    events: &[String],
    reactions: &[Reaction],
    intent: &str,
) -> String {
    let mut tail = format!("THIS TURN\nPlayer intent: {}\n", one_line(intent));
    let outcome: Vec<String> = perceived
        .iter()
        .filter_map(|op| outcome_line(r, op, perceived))
        .chain(events.iter().map(|text| format!("- {}\n", one_line(text))))
        .collect();
    if !outcome.is_empty() {
        tail.push_str("Outcome:\n");
        tail.push_str(&outcome.concat());
    }
    if !reactions.is_empty() {
        tail.push_str("NPC intentions (portray, do not state):\n");
        for reaction in reactions {
            let name = match reaction.npc {
                Handle::Entity(id) => r.name(id),
                Handle::NewNpc(n) => {
                    u8::try_from(n).map_or_else(|_| "a newcomer".into(), |n| new_name(n, perceived))
                }
                _ => continue,
            };
            let stance = serde_json::to_value(reaction.stance)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let plan = one_line(&reaction.plan);
            if plan.is_empty() {
                tail.push_str(&format!("- {name} ({stance})\n"));
            } else {
                tail.push_str(&format!("- {name} ({stance}): {plan}\n"));
            }
        }
    }
    recent_block(r.view, "", &tail)
}
