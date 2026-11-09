//! The four Shortcut checks of data-shape §4: coincidence, NPC folds, fact
//! bends and tension evaporates.

use super::{CheckKind, Rejection, ceiling, changes, same, same_key, valid_cause};
use crate::{
    domain::{Canon, Cause, DecisionProposal, EntityId, Handle, Seen, Trigger, Verdict},
    store::WorldView,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Fact,
    Effect,
    Stake,
}

/// A slot write on an entity that exists before this Turn.
struct Write<'a> {
    subject: EntityId,
    key: &'a str,
    value: &'a str,
    because: &'a [Handle],
    origin: Origin,
}

fn writes(p: &DecisionProposal) -> Vec<Write<'_>> {
    let mut out = Vec::new();
    let mut push = |subject: Handle, key, value, because, origin| {
        if let Handle::Entity(subject) = subject {
            out.push(Write {
                subject,
                key,
                value,
                because,
                origin,
            });
        }
    };
    for f in p.facts.iter().filter(|f| f.said_by.is_none()) {
        push(
            f.slot.subject,
            &f.slot.key,
            &f.slot.value,
            &f.because,
            Origin::Fact,
        );
    }
    if let Some(s) = &p.schedule {
        for e in &s.effects {
            push(e.subject, &e.key, &e.value, &[], Origin::Effect);
        }
    }
    if let Some(c) = &p.check {
        for stake in [&c.win, &c.lose] {
            push(stake.subject, &stake.key, &stake.value, &[], Origin::Stake);
        }
    }
    out
}

fn has_valid_cause(view: &WorldView, because: &[Handle]) -> bool {
    because.iter().any(|h| valid_cause(view, *h).is_some())
}

pub(super) fn check(p: &DecisionProposal, view: &WorldView, errs: &mut Vec<Rejection>) {
    let writes = writes(p);
    coincidence(p, view, &writes, errs);
    npc_folds(p, view, errs);
    fact_bends(view, &writes, errs);
    tension(p, view, &writes, errs);
}

/// A proposal fact or a scheduled effect that writes the slot of an open
/// obstacle needs a valid cause. The obstacles are the open rows plus the
/// blockers this proposal declares. The win stake is the legal path and is
/// not checked here.
fn coincidence(
    p: &DecisionProposal,
    view: &WorldView,
    writes: &[Write],
    errs: &mut Vec<Rejection>,
) {
    let declared = if matches!(p.verdict, Verdict::Impossible | Verdict::Check) {
        p.blockers.as_slice()
    } else {
        &[]
    };
    let mut obstacles: Vec<u64> = view.obstacles_open.iter().map(|o| o.fact_id).collect();
    for b in declared {
        if let Handle::Fact(id) = b
            && !obstacles.contains(id)
        {
            obstacles.push(*id);
        }
    }
    for fact_id in obstacles {
        let Some(obstacle) = view.fact(Handle::Fact(fact_id)) else {
            continue;
        };
        let hit = writes.iter().any(|w| {
            w.origin != Origin::Stake
                && w.subject == obstacle.slot.subject
                && same_key(w.key, &obstacle.slot.key)
                && !same(w.value, &obstacle.slot.value)
                && !has_valid_cause(view, w.because)
        });
        if hit {
            errs.push(Rejection::new(
                CheckKind::Coincidence,
                Some(Handle::Fact(fact_id)),
                format!(
                    "this write removes the obstacle {}; cite an earlier Established fact in because, or make it the win stake of a Check",
                    obstacle.slot.key
                ),
            ));
        }
    }
}

/// A stance above the NPC's ceiling toward the PC needs a cause: a Check
/// against that NPC, or a valid cause fact the NPC holds after this Commit.
fn npc_folds(p: &DecisionProposal, view: &WorldView, errs: &mut Vec<Rejection>) {
    for r in &p.reactions {
        let limit = match r.npc {
            Handle::Entity(id) => ceiling(view, id),
            // A new NPC has no attitude row and no goal: the ceiling of attitude 0.
            _ => ceiling(view, EntityId::MAX),
        };
        if r.stance <= limit {
            continue;
        }
        let reason = match &r.cause {
            None => format!(
                "stance {:?} is above this NPC's ceiling {limit:?}; it needs a Check against this NPC or a fact the NPC holds",
                r.stance
            ),
            Some(Cause::Check) => {
                if p.check.as_ref().and_then(|c| c.against) == Some(r.npc) {
                    continue;
                }
                "cause is check, but the Check is not against this NPC".into()
            }
            Some(Cause::Fact(f)) => {
                if valid_cause(view, *f).is_none() {
                    format!("{f} is not an Established fact from before this Turn")
                } else if holds(p, view, r.npc, *f) {
                    continue;
                } else {
                    format!("this NPC does not know {f}")
                }
            }
        };
        errs.push(Rejection::new(CheckKind::NpcFolds, Some(r.npc), reason));
    }
}

/// True when `npc` holds fact `f` after this Commit: it witnessed or was told
/// the fact before (arriving by the end of this Turn), or a claim in this
/// proposal states the same slot value in its hearing.
fn holds(p: &DecisionProposal, view: &WorldView, npc: Handle, f: Handle) -> bool {
    let Some(fact) = view.fact(f) else {
        return false;
    };
    if let Handle::Entity(id) = npc {
        let by = view.clock_min + u64::from(p.minutes);
        let known = view
            .knowledge
            .iter()
            .any(|k| k.holder == id && k.fact_id == Some(fact.id) && k.arrives_at <= by);
        if known || fact.seen_by.contains(&id) {
            return true;
        }
    }
    let present = match npc {
        Handle::Entity(id) => view.present().contains(&id),
        _ => true,
    };
    p.facts.iter().any(|claim| {
        claim.said_by.is_some()
            && claim.slot.subject == Handle::Entity(fact.slot.subject)
            && same_key(&claim.slot.key, &fact.slot.key)
            && same(&claim.slot.value, &fact.slot.value)
            && match claim.seen {
                Seen::Present => present,
                Seen::Pc | Seen::Gm => false,
            }
    })
}

/// No write, whatever its cause, may change a slot whose live fact is
/// Persistent. Restating the same value is not a change.
fn fact_bends(view: &WorldView, writes: &[Write], errs: &mut Vec<Rejection>) {
    for w in writes {
        if let Some(live) = view.live_fact(w.subject, w.key)
            && live.canon == Canon::Persistent
            && !same(&live.slot.value, w.value)
        {
            errs.push(Rejection::new(
                CheckKind::FactBends,
                Some(Handle::Fact(live.id)),
                format!(
                    "{} is Persistent; only an earlier-scheduled World Event or a Correction can change it",
                    live.slot.key
                ),
            ));
        }
    }
}

/// `cancel` needs a valid cause. A write that changes the `When` slot of a
/// scheduled event, or the `alive` or `location` of its actor, counts as a
/// cancel and needs one too.
fn tension(p: &DecisionProposal, view: &WorldView, writes: &[Write], errs: &mut Vec<Rejection>) {
    for c in &p.cancel {
        if valid_cause(view, c.because).is_none() {
            errs.push(Rejection::new(
                CheckKind::TensionEvaporates,
                Some(c.event),
                format!(
                    "{} is not an Established fact from before this Turn",
                    c.because
                ),
            ));
        }
    }
    for event in &view.events_scheduled {
        let hit = writes.iter().any(|w| {
            if w.origin == Origin::Stake || has_valid_cause(view, w.because) {
                return false;
            }
            if !changes(view, w.subject, w.key, w.value) {
                return false;
            }
            let when = matches!(&event.trigger, Trigger::When(slot)
                if slot.subject == w.subject && same_key(&slot.key, w.key));
            let actor = event.actors.contains(&w.subject)
                && (same_key(w.key, "alive") || same_key(w.key, "location"));
            when || actor
        });
        if hit {
            errs.push(Rejection::new(
                CheckKind::TensionEvaporates,
                Some(Handle::WorldEvent(event.id)),
                "this write stops a scheduled event; it needs an earlier Established fact in because",
            ));
        }
    }
}
