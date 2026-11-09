use crate::domain::{ClockMin, EntityKind, EntityRef, FactSource, Knowing, LedgerOp};
use crate::normalize;
use crate::store::{Fact, WorldView};

/// Perception on arrival: the PC witnesses the live `location` fact of each
/// entity that truly stands at `location` in `view`.
///
/// Two kinds of presence stay unseen (amended ruling 4(b)):
/// - an entity the Adventure Seed declares `hidden`;
/// - a location fact placed out of the PC's sight on purpose: one written by
///   a proposal (or a Correction) with no PC witness, that is `seen = gm`.
///
/// Presence placed by the Adventure Seed or by a World Event is open. An
/// event's `seen_by` decides who witnesses the event when it fires; it does
/// not hide the NPC from a PC who arrives later.
///
/// Facts the PC already holds a knowledge row for are skipped. `at` is the
/// time the PC arrives; the ops must be committed after the clock reaches it.
pub fn arrival(view: &WorldView, location: &str, at: ClockMin) -> Vec<LedgerOp> {
    let Some(pc) = view.entities.iter().find(|e| e.kind == EntityKind::Pc) else {
        return Vec::new();
    };
    let location = normalize::value(location);
    view.entities
        .iter()
        .filter(|e| e.id != pc.id && !hidden(view, &e.norm_name))
        .filter_map(|e| view.live_fact(e.id, "location"))
        .filter(|f| normalize::value(&f.slot.value) == location)
        .filter(|f| !placed_unseen(f, pc.id))
        .filter(|f| {
            !view
                .knowledge
                .iter()
                .any(|k| k.holder == pc.id && k.fact_id == Some(f.id))
        })
        .map(|f| LedgerOp::Know {
            holder: EntityRef::Id(pc.id),
            fact_id: Some(f.id),
            claim_id: None,
            how: Knowing::Witnessed,
            arrives_at: at,
        })
        .collect()
}

/// True when the Adventure Seed declares the entity with this normalized
/// name hidden.
pub fn hidden(view: &WorldView, norm_name: &str) -> bool {
    view.adventure_seed.as_ref().is_some_and(|seed| {
        seed.entities
            .iter()
            .any(|s| s.hidden && normalize::name(&s.name) == norm_name)
    })
}

/// A location fact a proposal (or a Correction) wrote with no PC witness:
/// `seen = gm`. Adventure Seed and World Event presence is open by ruling.
fn placed_unseen(fact: &Fact, pc: u64) -> bool {
    !matches!(
        fact.source,
        FactSource::AdventureSeed | FactSource::WorldEvent
    ) && !fact.seen_by.contains(&pc)
}

/// The `location` slot an op writes, as `(subject, value)`.
fn location_write(op: &LedgerOp) -> Option<(EntityRef, &str)> {
    let slot = match op {
        LedgerOp::AddFact { slot, .. } => slot,
        LedgerOp::SupersedeFact { new, .. } => &new.slot,
        _ => return None,
    };
    (normalize::key(&slot.key) == "location").then_some((slot.subject, slot.value.as_str()))
}

/// Perception on arrival. When the ops move the PC to a new place, the PC
/// witnesses the location of each entity that openly stands there:
/// - an entity already there (and not moved by this Turn) gets a `Know` op,
///   added at the end, after the clock reaches `clock_after`;
/// - a World Event that moves an entity there this Turn gets the PC among
///   the witnesses of that location fact.
///
/// Presence placed by a World Event is open: the event's `seen_by` decides
/// who witnesses the event itself, not who sees the NPC later on arrival.
/// Hidden entities and locations placed by a proposal with `seen = gm` stay
/// unseen (`arrival`).
pub fn with_arrival(
    view: &WorldView,
    mut ops: Vec<LedgerOp>,
    clock_after: ClockMin,
) -> Vec<LedgerOp> {
    let Some(pc) = view.entities.iter().find(|e| e.kind == EntityKind::Pc) else {
        return ops;
    };
    let pc_ref = EntityRef::Id(pc.id);
    let Some(location) = ops
        .iter()
        .filter_map(location_write)
        .filter(|(subject, _)| *subject == pc_ref)
        .map(|(_, value)| normalize::value(value))
        .next_back()
    else {
        return ops;
    };
    let before = view
        .live_fact(pc.id, "location")
        .map(|f| normalize::value(&f.slot.value));
    if before.as_deref() == Some(location.as_str()) {
        return ops;
    }
    let moved: Vec<EntityRef> = ops
        .iter()
        .filter_map(location_write)
        .map(|(subject, _)| subject)
        .collect();
    for op in &mut ops {
        if let LedgerOp::SupersedeFact { new: fact, .. } = op
            && fact.source == FactSource::WorldEvent
            && normalize::key(&fact.slot.key) == "location"
            && normalize::value(&fact.slot.value) == location
            && fact.slot.subject != pc_ref
            && !hidden_ref(view, fact.slot.subject)
            && !fact.seen_by.contains(&pc_ref)
        {
            fact.seen_by.push(pc_ref);
        }
        if let LedgerOp::AddFact {
            slot,
            source: FactSource::WorldEvent,
            seen_by,
            ..
        } = op
            && normalize::key(&slot.key) == "location"
            && normalize::value(&slot.value) == location
            && slot.subject != pc_ref
            && !hidden_ref(view, slot.subject)
            && !seen_by.contains(&pc_ref)
        {
            seen_by.push(pc_ref);
        }
    }
    let arrivals: Vec<LedgerOp> = arrival(view, &location, clock_after)
        .into_iter()
        .filter(|op| match op {
            LedgerOp::Know {
                fact_id: Some(id), ..
            } => view
                .facts
                .iter()
                .find(|f| f.id == *id)
                .is_none_or(|f| !moved.contains(&EntityRef::Id(f.slot.subject))),
            _ => true,
        })
        .collect();
    ops.extend(arrivals);
    ops
}

fn hidden_ref(view: &WorldView, entity: EntityRef) -> bool {
    match entity {
        EntityRef::Id(id) => view
            .entities
            .iter()
            .find(|e| e.id == id)
            .is_some_and(|e| hidden(view, &e.norm_name)),
        EntityRef::New(_) => false,
    }
}
