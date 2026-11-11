//! Contests (ADR 0007). The proposal says in `contested` whether an NPC
//! actively opposes the act and the result is uncertain. A contested act
//! needs a Check: the first attempt without one gets one repair note; on
//! the retry the core builds a default Check instead of rejecting. When the
//! verdict is already `check`, the core builds it on the first attempt.

use super::{CheckKind, Rejection};
use crate::{
    domain::{
        CheckDraft, DecisionProposal, EntityId, EntityKind, Handle, Stance, Verdict, is_alias_key,
    },
    normalize,
    store::WorldView,
};

/// True when the proposal needs a Check it does not have: it is contested,
/// or its verdict is `check`, and it carries no Check. An impossible act
/// needs none.
fn lacks_check(p: &DecisionProposal) -> bool {
    p.check.is_none()
        && match p.verdict {
            Verdict::Check => true,
            Verdict::Allowed => p.contested,
            Verdict::Impossible => false,
        }
}

/// The repair note of a contested act with no Check, for the first attempt.
pub fn contest_note(p: &DecisionProposal) -> Option<Rejection> {
    (p.contested && lacks_check(p)).then(|| {
        Rejection::new(
            CheckKind::Contest,
            None,
            "contested is true, so this act needs a Check: set verdict check and add check with against = the opposing NPC, win and lose stakes and factors",
        )
    })
}

/// True when the core builds the default Check on the first attempt: the
/// act is contested and the verdict is already `check`, but the Check is
/// missing. The model has said "roll" twice; a repair round would only add
/// stakes, so the core spends no retry on it (changed after the live run
/// of cdefde8). Every other missing Check keeps its repair note.
pub fn checks_now(p: &DecisionProposal) -> bool {
    p.contested && p.verdict == Verdict::Check && p.check.is_none()
}

/// The NPC a contested act opposes (review M3), in order: the present NPC
/// the PC perceives that the Intent names (`named_here`), when exactly one;
/// else the only NPC the PC perceives here whose reaction is below
/// neutral; else none. Never an NPC the PC cannot perceive: the lowest
/// stance alone picked a hidden NPC, or a hostile bystander the Intent did
/// not address. A new NPC of this proposal stands before the PC, so it
/// counts as perceived.
fn opponent(p: &DecisionProposal, view: &WorldView, intent: &str) -> Option<Handle> {
    let here = perceived_npcs(view);
    if let [one] = named_here(view, intent, &here)[..] {
        return Some(Handle::Entity(one));
    }
    let mut hostile: Vec<Handle> = p
        .reactions
        .iter()
        .filter(|r| r.stance < Stance::Neutral)
        .filter(|r| match r.npc {
            Handle::Entity(id) => here.contains(&id),
            Handle::NewNpc(_) => p.new_npcs.iter().any(|n| n.id == r.npc),
            _ => false,
        })
        .map(|r| r.npc)
        .collect();
    hostile.dedup();
    match hostile[..] {
        [one] => Some(one),
        _ => None,
    }
}

/// The NPCs the PC perceives here (`context::seen_present`, the PC left
/// out).
fn perceived_npcs(view: &WorldView) -> Vec<EntityId> {
    crate::context::seen_present(view)
        .into_iter()
        .filter(|id| {
            view.entities
                .iter()
                .any(|e| e.id == *id && e.kind == EntityKind::Npc)
        })
        .collect()
}

/// Of `here`, the NPCs that `intent` names by a name the PC knows: the main
/// name or its first word, or an other name the PC holds an `alias_` Belief
/// for, as whole words, whatever the case. Only NPCs the PC can name
/// (`world::pc_can_name`) count.
fn named_here(view: &WorldView, intent: &str, here: &[EntityId]) -> Vec<EntityId> {
    let standing: Vec<&crate::store::StoredTurn> = view.recent_turns.iter().collect();
    let nameable = crate::world::pc_can_name(view, &standing);
    let said = normalize::name_words(intent);
    let says = |name: &str| {
        let words = normalize::name_words(&normalize::name(name));
        !words.is_empty() && said.windows(words.len()).any(|w| w == words.as_slice())
    };
    let aliases = crate::world::pc_knowledge(view);
    here.iter()
        .copied()
        .filter(|id| nameable.contains(id))
        .filter(|id| {
            let Some(e) = view.entities.iter().find(|e| e.id == *id) else {
                return false;
            };
            let first = e.name.split_whitespace().next().unwrap_or_default();
            says(&e.name)
                || says(first)
                || aliases
                    .iter()
                    .filter(|b| b.subject == *id && is_alias_key(&b.key))
                    .any(|b| says(&b.value))
        })
        .collect()
}

/// A contested Check with no `against` opposes the NPC of `opponent`
/// (bug pass 3, changed after review M3): the QA arm-wrestle came with
/// `against: null`, and the narrator could not name the opponent. With no
/// such NPC, `against` stays empty and the Check still rolls. Returns the
/// note for the debug log. It adds no PC act.
pub fn oppose(p: &mut DecisionProposal, view: &WorldView, intent: &str) -> Option<String> {
    if !p.contested {
        return None;
    }
    let against = opponent(p, view, intent)?;
    let check = p.check.as_mut().filter(|c| c.against.is_none())?;
    check.against = Some(against);
    Some(format!(
        "set the Check against {against}: the act is contested and named no opponent"
    ))
}

/// On the retry, or on the first attempt when `checks_now`: when the
/// proposal still lacks the Check it needs, the core builds one and
/// returns the note for the debug log. The default Check opposes the NPC
/// of `opponent` (none when no NPC fits). It has no stakes and no factors, so the Ruleset gives
/// its normal difficulty, and the narrator gets only the result. The
/// proposal's facts then apply on a won Roll only.
pub fn default_check(p: &mut DecisionProposal, view: &WorldView, intent: &str) -> Option<String> {
    if !lacks_check(p) {
        return None;
    }
    let against = opponent(p, view, intent);
    p.verdict = Verdict::Check;
    // The facts were written for an act that simply happens: with the Roll
    // they apply on a win only (`to_ops`).
    p.facts_on_win = true;
    p.check = Some(CheckDraft {
        against,
        win: None,
        lose: None,
        factors: Vec::new(),
    });
    Some(match against {
        Some(npc) => {
            format!(
                "built a default Check against {npc}: the act is contested and has no check; its facts apply on a win only"
            )
        }
        None => {
            "built a default Check against nobody: the act is contested and has no check; its facts apply on a win only".into()
        }
    })
}
