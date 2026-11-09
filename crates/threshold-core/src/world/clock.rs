use std::collections::HashSet;

use crate::domain::{
    Canon, ClockMin, Effect, EntityId, EntityKind, EntityRef, FactDraft, FactSource, Key, LedgerOp,
    Slot, Trigger,
};
use crate::normalize;
use crate::store::{WorldEvent, WorldView};

/// The most World Events that fire in one Turn. Due events past the cap stay
/// scheduled and fire on a later Turn, in the same order.
pub const MAX_EVENTS_PER_TURN: usize = 3;

/// Moves the clock by `minutes` and fires the due World Events.
///
/// Returns `AdvanceClock` (omitted when `minutes` is 0), then one `FireEvent`
/// plus its effect ops per fired event. Due events fire in trigger order:
/// time, then id. An `At(t)` event is due when `t` is at or before the new
/// clock; a `When(slot)` event is due when the live slot holds that value, and
/// its time is the new clock.
///
/// Every event in `view` was scheduled in a committed Turn, so it is always
/// earlier than the Turn these ops commit in. Its `SetFact` may therefore
/// supersede a Persistent fact (data shape section 4, Fact bends).
pub fn advance(view: &WorldView, minutes: u32) -> Vec<LedgerOp> {
    let clock = view.clock_min.saturating_add(ClockMin::from(minutes));
    let mut ops = Vec::new();
    if minutes > 0 {
        ops.push(LedgerOp::AdvanceClock { minutes });
    }
    let mut due: Vec<(ClockMin, &WorldEvent)> = view
        .events_scheduled
        .iter()
        .filter_map(|event| match &event.trigger {
            Trigger::At(at) => (*at <= clock).then_some((*at, event)),
            Trigger::When(slot) => slot_holds(view, slot).then_some((clock, event)),
        })
        .collect();
    due.sort_by_key(|(at, event)| (*at, event.id));

    let pc = view
        .entities
        .iter()
        .find(|entity| entity.kind == EntityKind::Pc)
        .map(|entity| entity.id);
    let mut written: HashSet<(EntityId, Key)> = HashSet::new();
    for (_, event) in due.into_iter().take(MAX_EVENTS_PER_TURN) {
        let effects = last_write_per_slot(&event.effects);
        // Two events writing one slot in one Turn would supersede the same
        // live fact twice. The later event waits for the next Turn, and so do
        // the events after it, to keep trigger order.
        let slots: Vec<(EntityId, Key)> = effects.iter().filter_map(|e| set_fact_slot(e)).collect();
        if slots.iter().any(|target| written.contains(target)) {
            break;
        }
        written.extend(slots);
        ops.push(LedgerOp::FireEvent { event_id: event.id });
        ops.extend(
            effects
                .into_iter()
                .map(|effect| effect_op(view, event, effect, pc)),
        );
    }
    ops
}

fn set_fact_slot(effect: &Effect<EntityId>) -> Option<(EntityId, Key)> {
    match effect {
        Effect::SetFact { slot, .. } => Some((slot.subject, normalize::key(&slot.key))),
        _ => None,
    }
}

/// The event's effects with only the last `SetFact` per slot kept, in their
/// order. Checks reject such an event at proposal time; one already scheduled
/// still fires instead of blocking the queue.
fn last_write_per_slot(effects: &[Effect<EntityId>]) -> Vec<&Effect<EntityId>> {
    effects
        .iter()
        .enumerate()
        .filter(|(i, effect)| {
            set_fact_slot(effect).is_none_or(|target| {
                !effects[i + 1..]
                    .iter()
                    .any(|later| set_fact_slot(later).as_ref() == Some(&target))
            })
        })
        .map(|(_, effect)| effect)
        .collect()
}

fn slot_holds(view: &WorldView, slot: &Slot<EntityId>) -> bool {
    view.live_fact(slot.subject, &slot.key)
        .is_some_and(|fact| normalize::value(&fact.slot.value) == normalize::value(&slot.value))
}

fn id_ref(slot: &Slot<EntityId>) -> Slot {
    Slot {
        subject: EntityRef::Id(slot.subject),
        key: slot.key.clone(),
        value: slot.value.clone(),
    }
}

fn effect_op(
    view: &WorldView,
    event: &WorldEvent,
    effect: &Effect<EntityId>,
    pc: Option<EntityId>,
) -> LedgerOp {
    match effect {
        Effect::SetFact {
            slot,
            text,
            seen_by,
        } => {
            // The effect's own witnesses, then the event's, without repeats.
            let mut holders = Vec::new();
            for holder in seen_by.iter().chain(&event.seen_by) {
                if !holders.contains(holder) {
                    holders.push(*holder);
                }
            }
            // Same rule as a proposal fact: Persistent when the PC sees it.
            // The store keeps state keys Established.
            let canon = if pc.is_some_and(|pc| holders.contains(&pc)) {
                Canon::Persistent
            } else {
                Canon::Established
            };
            let draft = FactDraft {
                slot: id_ref(slot),
                text: text.clone(),
                canon,
                source: FactSource::WorldEvent,
                seen_by: holders.into_iter().map(EntityRef::Id).collect(),
                causes: event.cause_fact_id.into_iter().collect(),
            };
            match view.live_fact(slot.subject, &slot.key) {
                Some(live) => LedgerOp::SupersedeFact {
                    old: live.id,
                    new: draft,
                },
                None => LedgerOp::AddFact {
                    slot: draft.slot,
                    text: draft.text,
                    canon: draft.canon,
                    source: draft.source,
                    seen_by: draft.seen_by,
                    causes: draft.causes,
                },
            }
        }
        Effect::ShiftAttitude { from, to, delta } => LedgerOp::ShiftAttitude {
            from: EntityRef::Id(*from),
            to: EntityRef::Id(*to),
            delta: *delta,
        },
        Effect::Tell {
            to,
            claim,
            delay_min,
        } => LedgerOp::AddClaim {
            claim: crate::domain::ClaimDraft {
                speaker: EntityRef::Id(claim.speaker),
                slot: id_ref(&claim.slot),
                text: claim.text.clone(),
            },
            listeners: vec![EntityRef::Id(*to)],
            delay_min: *delay_min,
        },
    }
}
