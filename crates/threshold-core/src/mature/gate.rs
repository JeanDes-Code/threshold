//! The adults-only rule: who is cleared for an intimate Turn, and the Commit
//! check of an intimate proposal (G2). It reads the world only: stored ages,
//! Age Confirmations and who stands at the PC's place. No prompt and no
//! model output reach it, and it names nothing outside the core's world.
//! A source test in `tests/mature.rs` pins that.

use std::collections::{BTreeSet, HashSet};

use crate::{
    checks::{CheckKind, Rejection},
    domain::{DecisionProposal, DetailLevel, EntityId, EntityKind, Handle, Stance, is_adult},
    normalize,
    store::{Entity, WorldView},
};

/// The proof that every character of a place is cleared. Only `clearance`
/// makes one: the field is private to this file. It is neither `Clone` nor
/// `Copy` and is never stored, so a later step must ask `clearance` again.
#[derive(Debug)]
pub struct Cleared(());

/// The characters who are not cleared, sorted, each once. For the debug log
/// and the core's own rules: a Player screen never shows it raw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blocked {
    pub who: Vec<EntityId>,
}

/// Clears the characters at the PC's place, world truth (hidden ones
/// included, `WorldView::present`), plus the ids in `also`.
///
/// A character is cleared when all of these hold:
/// - its kind is PC or NPC;
/// - its age is confirmed through the core (`Entity::age_confirmed`);
/// - its stored age is `domain::ADULT_AGE` or more;
/// - no PC or NPC that fails one of the three rules above shares one of its
///   names, main or other, through `normalize::name`. Nothing stops a new
///   NPC from taking the name of a minor of the roster.
///
/// An id of `also` that names no PC or NPC is not cleared either. The
/// Adventure Seed's `min_age` and the Detail Level change nothing here.
pub fn clearance(view: &WorldView, also: &[EntityId]) -> Result<Cleared, Blocked> {
    let failing: HashSet<String> = view
        .entities
        .iter()
        .filter(|e| is_character(e) && !holds_alone(e))
        .flat_map(names)
        .collect();
    let cleared = |id: EntityId| {
        view.entities.iter().find(|e| e.id == id).is_some_and(|e| {
            is_character(e) && holds_alone(e) && names(e).all(|n| !failing.contains(&n))
        })
    };
    let characters_here = view
        .present()
        .into_iter()
        .filter(|id| view.entities.iter().any(|e| e.id == *id && is_character(e)));
    let who: BTreeSet<EntityId> = characters_here
        .chain(also.iter().copied())
        .filter(|id| !cleared(*id))
        .collect();
    if who.is_empty() {
        Ok(Cleared(()))
    } else {
        Err(Blocked {
            who: who.into_iter().collect(),
        })
    }
}

/// G2: the rules of an intimate proposal, in order (data shape 3.3). A
/// proposal without `intimate` passes untouched.
///
/// 1. At Detail Level `excluded`: one `DetailExcluded` rejection, and
///    nothing else is checked.
/// 2. Each new NPC: one `AdultsOnly` rejection with its `N` handle. A new
///    NPC has no Age Confirmation.
/// 3. `clearance` over the place and the reacting NPCs: one `AdultsOnly`
///    rejection per blocked character, with its handle.
/// 4. No reaction, or a reacting NPC below comply: `Unwilling`. Not final:
///    it keeps the one retry.
pub fn check(p: &DecisionProposal, view: &WorldView, errs: &mut Vec<Rejection>) {
    if !p.intimate {
        return;
    }
    if view.detail == DetailLevel::Excluded {
        errs.push(Rejection::new(
            CheckKind::DetailExcluded,
            None,
            "this Campaign's Detail Level excludes an intimate Turn",
        ));
        return;
    }
    for npc in &p.new_npcs {
        errs.push(Rejection::new(
            CheckKind::AdultsOnly,
            Some(npc.id),
            "a new NPC cannot be part of an intimate Turn",
        ));
    }
    let reacting: Vec<EntityId> = p
        .reactions
        .iter()
        .filter_map(|r| match r.npc {
            Handle::Entity(id) => Some(id),
            _ => None,
        })
        .collect();
    if let Err(blocked) = clearance(view, &reacting) {
        for id in blocked.who {
            errs.push(Rejection::new(
                CheckKind::AdultsOnly,
                Some(Handle::Entity(id)),
                "an intimate Turn needs every character here to be a confirmed adult",
            ));
        }
    }
    if p.reactions.is_empty() {
        errs.push(Rejection::new(
            CheckKind::Unwilling,
            None,
            "an intimate Turn needs at least one NPC who reacts with comply or help",
        ));
    }
    for r in p.reactions.iter().filter(|r| r.stance < Stance::Comply) {
        errs.push(Rejection::new(
            CheckKind::Unwilling,
            Some(r.npc),
            "in an intimate Turn each reacting NPC complies or helps",
        ));
    }
}

fn is_character(e: &Entity) -> bool {
    matches!(e.kind, EntityKind::Pc | EntityKind::Npc)
}

/// The first three rules of a cleared character: confirmed, with an adult
/// stored age. The kind is checked apart.
fn holds_alone(e: &Entity) -> bool {
    e.age_confirmed && is_adult(e.age)
}

/// Every name of `e`, main and other, in normal form.
fn names(e: &Entity) -> impl Iterator<Item = String> + '_ {
    std::iter::once(normalize::name(&e.name))
        .chain(e.other_names.iter().map(|n| normalize::name(n)))
}
