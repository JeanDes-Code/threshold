//! References check: every handle exists or is declared by this proposal, each
//! field takes the right handle prefix, and lists respect their caps. Also the
//! shape rule (check present iff verdict is check) and one write per slot.

use std::collections::HashSet;

use super::{CheckKind, Rejection};
use crate::{
    domain::{Cause, DecisionProposal, Handle},
    normalize,
    store::WorldView,
};

const CAPS: [(&str, usize); 7] = [
    ("blockers", 2),
    ("reactions", 2),
    ("new_npcs", 1),
    ("facts", 2),
    ("cancel", 1),
    ("check.factors", 3),
    ("schedule.effects", 2),
];

pub(super) fn check(p: &DecisionProposal, view: &WorldView) -> Vec<Rejection> {
    let mut errs = Vec::new();
    let mut err = |handle: Option<Handle>, reason: String| {
        errs.push(Rejection::new(CheckKind::References, handle, reason));
    };

    let lengths = [
        p.blockers.len(),
        p.reactions.len(),
        p.new_npcs.len(),
        p.facts.len(),
        p.cancel.len(),
        p.check.as_ref().map_or(0, |c| c.factors.len()),
        p.schedule.as_ref().map_or(0, |s| s.effects.len()),
    ];
    for ((field, cap), len) in CAPS.iter().zip(lengths) {
        if len > *cap {
            err(None, format!("{field} has {len} items; the cap is {cap}"));
        }
    }

    let mut declared = HashSet::new();
    for npc in &p.new_npcs {
        match npc.id {
            Handle::NewNpc(n) if u8::try_from(n).is_err() || n == 0 => {
                err(Some(npc.id), "new NPC number is out of range".into());
            }
            Handle::NewNpc(n) => {
                if !declared.insert(n) {
                    err(Some(npc.id), "new NPC handle is declared twice".into());
                }
            }
            other => err(Some(other), "new_npcs ids take an N handle".into()),
        }
    }

    let mut entity = |handle: Handle, field: &str| match handle {
        Handle::Entity(_) if view.entity(handle).is_some() => {}
        Handle::Entity(_) => err(Some(handle), format!("{field}: no such entity")),
        Handle::NewNpc(n) if declared.contains(&n) => {}
        Handle::NewNpc(_) => err(
            Some(handle),
            format!("{field}: N handle is not declared in new_npcs"),
        ),
        _ => err(Some(handle), format!("{field} takes an E or N handle")),
    };
    for r in &p.reactions {
        entity(r.npc, "reactions.npc");
    }
    if let Some(c) = &p.check {
        if let Some(against) = c.against {
            entity(against, "check.against");
        }
        entity(c.win.subject, "check.win.subject");
        entity(c.lose.subject, "check.lose.subject");
    }
    for fact in &p.facts {
        entity(fact.slot.subject, "facts.slot.subject");
        if let Some(speaker) = fact.said_by {
            entity(speaker, "facts.said_by");
        }
    }
    if let Some(s) = &p.schedule {
        entity(s.actor, "schedule.actor");
        for effect in &s.effects {
            entity(effect.subject, "schedule.effects.subject");
        }
    }

    let mut fact = |handle: Handle, field: &str| match handle {
        Handle::Fact(_) if view.fact(handle).is_some() => {}
        Handle::Fact(_) => err(
            Some(handle),
            format!(
                "{field}: no such fact before this Turn; a fact of this proposal is never a cause"
            ),
        ),
        _ => err(Some(handle), format!("{field} takes an F handle")),
    };
    for b in &p.blockers {
        fact(*b, "blockers");
    }
    if let Some(c) = &p.check {
        for factor in &c.factors {
            fact(factor.fact, "check.factors.fact");
        }
    }
    for r in &p.reactions {
        if let Some(Cause::Fact(f)) = r.cause {
            fact(f, "reactions.cause");
        }
    }
    for f in &p.facts {
        for b in &f.because {
            fact(*b, "facts.because");
        }
    }
    for c in &p.cancel {
        fact(c.because, "cancel.because");
    }

    for c in &p.cancel {
        match c.event {
            Handle::WorldEvent(id) if view.events_scheduled.iter().any(|e| e.id == id) => {}
            Handle::WorldEvent(_) => err(
                Some(c.event),
                "cancel.event: no such scheduled event".into(),
            ),
            other => err(Some(other), "cancel.event takes a W handle".into()),
        }
    }
    if let Some(goal) = p.schedule.as_ref().and_then(|s| s.goal) {
        match goal {
            Handle::Goal(id) if view.goals_active.iter().any(|g| g.id == id) => {}
            Handle::Goal(_) => err(Some(goal), "schedule.goal: no such active goal".into()),
            other => err(Some(other), "schedule.goal takes a G handle".into()),
        }
    }

    // One write per slot: two proposal facts, or a fact and a stake, on the
    // same slot would make the Commit order decide the result.
    let key = |subject: Handle, key: &str| (subject, normalize::key(key));
    let mut written = HashSet::new();
    for f in p.facts.iter().filter(|f| f.said_by.is_none()) {
        if !written.insert(key(f.slot.subject, &f.slot.key)) {
            err(
                Some(f.slot.subject),
                format!("facts write the slot {} twice", f.slot.key),
            );
        }
    }
    if let Some(c) = &p.check {
        for stake in [&c.win, &c.lose] {
            if written.contains(&key(stake.subject, &stake.key)) {
                err(
                    Some(stake.subject),
                    format!("a fact and a stake both write the slot {}", stake.key),
                );
            }
        }
    }

    if let Err(shape) = crate::validate_shape(p) {
        errs.push(Rejection::new(CheckKind::Schema, None, shape.to_string()));
    }
    errs
}
