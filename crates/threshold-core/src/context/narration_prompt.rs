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
//! witness. This is the real protection against leaks. The opening prompt
//! (Begin) gets the same blocks, with no Turn: the start's situation and
//! time take the place of the outcome.

use std::collections::HashSet;

use super::{
    NamesAudience, Retrieval, adventure_block, fact_text, names_block, one_line, recent_block,
    render_voice, seen_present, with_fragments,
};
use crate::{
    domain::{
        ActionKind, EntityId, EntityKind, EntityRef, Handle, Knowing, LedgerOp, Reaction, Verdict,
        is_open_key, is_state_key, outcome_key,
    },
    model::Prompt,
    normalize,
    pack::ContentPack,
    store::WorldView,
    world::{hidden, openly_here, pc_can_name, pc_knowledge},
};

/// The narrator's rules. The last line is `speech::RULE`, the speech mark.
pub const NARRATION_RULES: &str = concat!(
    "\
You are the narrator of a single-player tabletop role-playing game. Write what the player character (PC) perceives, in the second person, in the story language.
Narrate only the outcome given under THIS TURN. Never add a fact that changes the world, and never reveal what the PC cannot perceive.
Never decide the PC's actions, words, or feelings beyond the player's intent.
NPC intentions tell you how to portray an NPC. Show them through words and manner; never state them as fact.
Give each NPC the voice of their Voice Card.
Never say that more time passed than the time line of THIS TURN states. Do not open with the time; tell it only when it matters.
Write two short paragraphs at most. Plain prose, no headings, no lists.
",
    crate::speech::rule_text!(),
    "\n"
);

/// The lines that follow `NARRATION_RULES` in the opening prompt.
pub const OPENING_RULES: &str = "\
This is the opening of the adventure: there is no THIS TURN. Narrate the start given under OPENING.
Describe where the PC is and what the PC perceives as play begins.
Do not decide anything the PC does, says, or feels.
Do not move time forward.
End without asking what the PC does.
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
/// A secret is a live fact on a key that is not a state key (or that is an
/// open key), with no PC
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
/// event text is model text, like a plan, and it is the visible part of the
/// event: it is told unless it names something the PC must not learn. A
/// text that holds a secret value (`secret_values`, the same whole-word
/// match as `guard_plans`) is dropped; a secret key alone does not drop it
/// ("Ludo counts the silver for his debt" is seen, review M1). So is a text
/// that names an entity the PC must not hear of (`names_unheard`).
pub fn guard_events(view: &WorldView, events: &[String]) -> Vec<String> {
    let mut forbidden = secret_values(view);
    forbidden.extend(unheard_names(view));
    events
        .iter()
        .filter(|text| !quotes(&forbidden, text))
        .cloned()
        .collect()
}

/// The line the narrator gets when a World Event stopped the clock and
/// every part of it is guarded away: its text and its effects (review M1).
/// The outcome is then never empty.
pub const QUIET_EVENT_LINE: &str = "Nothing the PC notices happens.";

/// True when `text` names an entity the PC must not hear of: an NPC the
/// Adventure Seed hides, or one that neither stands openly here
/// (`world::openly_here`) nor is known to the PC (seen here, or the subject
/// of a PC Belief); or a place, a Faction or a thing the PC cannot name
/// (`world::pc_can_name`, review B1: "Orsel slips down to the Smugglers
/// Cellar"). `guard_events` drops such a text, and the PC learns no effect
/// of an event whose text it is (`world::with_witnessed_events`).
pub fn names_unheard(view: &WorldView, text: &str) -> bool {
    quotes(&unheard_names(view), text)
}

fn unheard_names(view: &WorldView) -> Vec<Vec<String>> {
    let present = seen_present(view);
    let known: HashSet<EntityId> = pc_knowledge(view).into_iter().map(|b| b.subject).collect();
    let standing: Vec<&crate::store::StoredTurn> = view.recent_turns.iter().collect();
    let named = pc_can_name(view, &standing);
    view.entities
        .iter()
        .filter(|e| match e.kind {
            EntityKind::Npc => {
                hidden(view, e.id)
                    || !(openly_here(view, e.id)
                        || present.contains(&e.id)
                        || known.contains(&e.id))
            }
            EntityKind::Place | EntityKind::Faction | EntityKind::Item => !named.contains(&e.id),
            EntityKind::Pc => false,
        })
        .map(|e| words(&e.name))
        .filter(|name| !name.is_empty())
        .collect()
}

/// The values of `secret_facts`: what an event text must not state.
fn secret_values(view: &WorldView) -> Vec<Vec<String>> {
    let names: HashSet<Vec<String>> = view.entities.iter().map(|e| words(&e.name)).collect();
    secret_facts(view)
        .map(|f| words(&f.slot.value))
        .filter(|term| !term.is_empty() && !names.contains(term))
        .collect()
}

/// Every live fact the PC holds no Belief about (same slot, same value), on
/// a key that is not a state key or that is an open key. An open key is a
/// state key only so that its fact stays replaceable; a plan the PC does
/// not know stays a secret.
fn secret_facts(view: &WorldView) -> impl Iterator<Item = &crate::store::Fact> {
    let known: HashSet<(EntityId, String, String)> = pc_knowledge(view)
        .into_iter()
        .map(|b| (b.subject, b.key, normalize::value(&b.value)))
        .collect();
    view.live_facts
        .iter()
        .filter(|f| !is_state_key(&f.slot.key) || is_open_key(&f.slot.key))
        .filter(move |f| {
            !known.contains(&(
                f.slot.subject,
                f.slot.key.clone(),
                normalize::value(&f.slot.value),
            ))
        })
}

/// The words of every secret fact (`secret_facts`): its key (an open key
/// without its prefix, `outcome_key`) and its value. Entity names are
/// public, so a key or value that is an entity name is left out.
fn secret_terms(view: &WorldView) -> Vec<Vec<String>> {
    let names: HashSet<Vec<String>> = view.entities.iter().map(|e| words(&e.name)).collect();
    secret_facts(view)
        .flat_map(|f| {
            // An open key names its outcome; the bare prefix names nothing.
            let key = &f.slot.key;
            let term = if is_open_key(key) {
                outcome_key(key).unwrap_or_default()
            } else {
                key
            };
            [words(term), words(&f.slot.value)]
        })
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
    turn: Option<(ActionKind, Verdict)>,
) -> Prompt {
    let r = Retrieval::new(view_after, "");
    Prompt {
        system: with_fragments(NARRATION_RULES, packs),
        stable: format!(
            "{}{}",
            adventure_block(view_after, None),
            names_block(view_after, NamesAudience::Narrator)
        ),
        scene: scene(&r),
        recent: recent(&r, perceived, events, reactions, intent, turn),
    }
}

/// The outcome line of an impossible attempt (changed after QA,
/// 2026-11-10). It carries the verdict only: the blockers can be GM-only
/// facts, so their text never reaches the narrator.
pub const IMPOSSIBLE_LINE: &str = "- The attempt does not happen: the world prevents it. Nothing changes beyond what is listed here.\n";

/// The outcome line that says where the PC is after the Turn, from the
/// Commit (ADR 0007). The place's name follows.
pub const PC_PLACE_LINE: &str = "- After this Turn the PC is at ";

/// The outcome line of a move that the Commit did not make: the PC does not
/// arrive (ADR 0007). The place's name follows.
pub const NOT_MOVED_LINE: &str =
    "- The move does not happen: the PC does not arrive. After this Turn the PC is still at ";

/// The words after a lost Check's result line.
pub const FAILED_CHECK_LINE: &str =
    " Tell the attempt and how it fails. Nothing changes beyond what is listed here.";

/// The words after a won Check's result line.
pub const WON_CHECK_LINE: &str = " Tell the attempt and how it succeeds.";

/// The line that gives the narrator the Turn's game time, after the
/// outcome and never as an outcome item: as the first item, the narrator
/// opened most Narrations with it (bug pass 3). The minutes follow.
pub const TIME_LINE: &str = "Time this Turn takes: ";

/// The opening prompt (data shape 3.7): the narration rules with the
/// opening lines after them, the same stable and scene blocks as a
/// Narration, then the start's situation and time. It shows no Turn and no
/// outcome: Begin commits nothing for the PC. An empty situation or time
/// leaves its line out.
pub fn opening_prompt(view: &WorldView, packs: &[ContentPack]) -> Prompt {
    let r = Retrieval::new(view, "");
    let mut recent = String::from("OPENING\n");
    if let Some(start) = view.adventure_seed.as_ref().map(|seed| &seed.start) {
        for (label, text) in [("Situation", &start.situation), ("When", &start.when)] {
            let text = one_line(text);
            if !text.is_empty() {
                recent.push_str(&format!("{label}: {text}\n"));
            }
        }
    }
    Prompt {
        system: with_fragments(&format!("{NARRATION_RULES}{OPENING_RULES}"), packs),
        stable: format!(
            "{}{}",
            adventure_block(view, None),
            names_block(view, NamesAudience::Narrator)
        ),
        scene: scene(&r),
        recent,
    }
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
            (Knowing::Told, Some(speaker)) if Some(speaker) != r.pc => {
                format!(
                    "- {} (told by {})\n",
                    fact_text(&b.key, &b.text),
                    r.name(speaker)
                )
            }
            _ => format!("- {}\n", fact_text(&b.key, &b.text)),
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
        LedgerOp::AddFact { slot, text, .. } => format!("- {}\n", fact_text(&slot.key, text)),
        LedgerOp::SupersedeFact { new, .. } => {
            format!("- {}\n", fact_text(&new.slot.key, &new.text))
        }
        // The PC's own Claim is the outcome of a `settles` (review M2): an
        // outcome line, never words the PC says.
        LedgerOp::AddClaim { claim, .. }
            if r.pc.is_some_and(|pc| claim.speaker == EntityRef::Id(pc)) =>
        {
            format!("- {}\n", fact_text(&claim.slot.key, &claim.text))
        }
        LedgerOp::AddClaim { claim, .. } => format!(
            "- {} says: {}\n",
            entity_name(r, &claim.speaker, perceived),
            fact_text(&claim.slot.key, &claim.text)
        ),
        LedgerOp::AddEntity { name, .. } => format!("- {} is here.\n", one_line(name)),
        LedgerOp::RecordRoll {
            outcome, against, ..
        } => {
            // The opponent is named only when the PC sees them here.
            let against = against
                .as_ref()
                .filter(|a| match a {
                    EntityRef::Id(id) => r.seen_present.contains(id),
                    EntityRef::New(_) => true,
                })
                .map(|a| format!(" against {}", entity_name(r, a, perceived)))
                .unwrap_or_default();
            if outcome == "lose" {
                format!("- Check{against}: the PC loses.{FAILED_CHECK_LINE}\n")
            } else {
                format!("- Check{against}: the PC wins.{WON_CHECK_LINE}\n")
            }
        }
        _ => return None,
    })
}

/// Where the PC is after the Turn, from the Commit (`view_after`). A move
/// whose Commit wrote no PC `location` did not happen.
fn place_line(
    r: &Retrieval<'_>,
    perceived: &[LedgerOp],
    action: Option<ActionKind>,
) -> Option<String> {
    let pc = EntityRef::Id(r.pc?);
    let place = r
        .place
        .map_or_else(|| r.location.clone(), |id| Some(r.name(id)))?;
    let moved = perceived.iter().any(|op| {
        let slot = match op {
            LedgerOp::AddFact { slot, .. } => slot,
            LedgerOp::SupersedeFact { new, .. } => &new.slot,
            _ => return false,
        };
        slot.subject == pc && normalize::key(&slot.key) == "location"
    });
    let line = if action == Some(ActionKind::Move) && !moved {
        NOT_MOVED_LINE
    } else {
        PC_PLACE_LINE
    };
    Some(format!("{line}{}.\n", one_line(&place)))
}

fn recent(
    r: &Retrieval<'_>,
    perceived: &[LedgerOp],
    events: &[String],
    reactions: &[Reaction],
    intent: &str,
    turn: Option<(ActionKind, Verdict)>,
) -> String {
    let mut tail = format!("THIS TURN\nPlayer intent: {}\n", one_line(intent));
    let verdict = turn.map(|(_, verdict)| verdict);
    let impossible = (verdict == Some(Verdict::Impossible)).then(|| IMPOSSIBLE_LINE.to_owned());
    let mut outcome: Vec<String> = impossible
        .into_iter()
        .chain(
            perceived
                .iter()
                .filter_map(|op| outcome_line(r, op, perceived)),
        )
        .chain(events.iter().map(|text| format!("- {}\n", one_line(text))))
        .collect();
    if let Some(line) = place_line(r, perceived, turn.map(|(action, _)| action)) {
        outcome.push(line);
    }
    if !outcome.is_empty() {
        tail.push_str("Outcome:\n");
        tail.push_str(&outcome.concat());
    }
    let minutes: u32 = perceived
        .iter()
        .filter_map(|op| match op {
            LedgerOp::AdvanceClock { minutes } => Some(*minutes),
            _ => None,
        })
        .sum();
    tail.push_str(&format!("{TIME_LINE}{minutes} minutes.\n"));
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
