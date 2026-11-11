use crate::domain::{
    Canon, ClockMin, EntityId, EntityKind, EntityRef, FactSource, Knowing, LedgerOp, Slot,
};
use crate::normalize;
use crate::store::{Fact, WorldEvent, WorldView};

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
        .filter(|e| e.id != pc.id && !hidden(view, e.id))
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

/// True when the Adventure Seed declares this entity hidden.
pub fn hidden(view: &WorldView, entity: EntityId) -> bool {
    view.adventure_seed.as_ref().is_some_and(|seed| {
        seed.entities
            .iter()
            .any(|s| s.hidden && view.entity_by_name(&s.name).is_some_and(|e| e.id == entity))
    })
}

/// The places the PC knows: every place entity the PC can name, by the
/// rule of the Player screens (`world::pc_can_name`), in id order. The
/// standing Turns are the view's recent ones. A Decision Proposal moves
/// the PC only to one of them (`move_to`, ADR 0007, changed after review).
pub fn known_places(view: &WorldView) -> Vec<EntityId> {
    let standing: Vec<&crate::store::StoredTurn> = view.recent_turns.iter().collect();
    let named = super::sight::pc_can_name(view, &standing);
    let mut places: Vec<EntityId> = view
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::Place && named.contains(&e.id))
        .map(|e| e.id)
        .collect();
    places.sort_unstable();
    places
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

/// True when the PC would witness `event` if it fired now: the event is not
/// GM-only (its `seen_by` is not empty), and one of its actors openly stands
/// at the PC's place (not hidden, not placed with `seen = gm`).
///
/// `with_witnessed_events` and the engine-owned wait (`wait_minutes`) share
/// this rule, so a wait never ends at an event the PC does not see.
pub fn pc_witnesses(view: &WorldView, event: &WorldEvent) -> bool {
    !event.seen_by.is_empty() && event.actors.iter().any(|a| openly_here(view, *a))
}

/// True when `entity` openly stands at the PC's place: it is not the PC, the
/// Adventure Seed does not hide it, and its location was not placed with
/// `seen = gm` (`arrival` uses the same two rules).
pub fn openly_here(view: &WorldView, entity: EntityId) -> bool {
    let Some(pc) = view.entities.iter().find(|e| e.kind == EntityKind::Pc) else {
        return false;
    };
    let Some(here) = view
        .live_fact(pc.id, "location")
        .map(|f| normalize::value(&f.slot.value))
    else {
        return false;
    };
    entity != pc.id
        && view
            .entities
            .iter()
            .find(|e| e.id == entity)
            .is_some_and(|e| !hidden(view, e.id))
        && view
            .live_fact(entity, "location")
            .is_some_and(|f| normalize::value(&f.slot.value) == here && !placed_unseen(f, pc.id))
}

/// Perception of a World Event: the PC witnesses an event that fires in
/// `ops` when the PC stays at its place this Turn and `pc_witnesses` holds
/// for the event before the Turn.
///
/// What the PC perceives of a witnessed event is its visible part: the
/// event's text, and the effects that `perceptible_effect` accepts (an
/// entity that comes or goes at the PC's place, what happens to the PC).
/// The PC joins the witnesses of those facts only, and they become
/// Persistent, as for any fact the PC sees (the store keeps state keys
/// Established). Every other effect stays with the witnesses the event
/// declares (bug pass 4, B1: "Ludo counts his silver" made the PC hold
/// "Ludo's debt is now partially paid", a secret seen by Agathe only).
/// Returns the ops and the texts of the witnessed events, in firing order,
/// for the narrator. The texts are model text: the narrator receives them
/// only through `context::guard_events`.
pub fn with_witnessed_events(
    view: &WorldView,
    mut ops: Vec<LedgerOp>,
) -> (Vec<LedgerOp>, Vec<String>) {
    let Some(pc) = view.entities.iter().find(|e| e.kind == EntityKind::Pc) else {
        return (ops, Vec::new());
    };
    let pc_ref = EntityRef::Id(pc.id);
    let here = view
        .live_fact(pc.id, "location")
        .map(|f| normalize::value(&f.slot.value));
    let moves = ops
        .iter()
        .filter_map(location_write)
        .any(|(subject, value)| subject == pc_ref && Some(normalize::value(value)) != here);
    if moves {
        return (ops, Vec::new());
    }
    let mut texts = Vec::new();
    // Whether the event whose effect ops follow is witnessed. `advance` puts
    // each event's effect ops right after its `FireEvent`.
    let mut witnessed = false;
    for op in &mut ops {
        match op {
            LedgerOp::FireEvent { event_id } => {
                let event = view.events_scheduled.iter().find(|e| e.id == *event_id);
                witnessed = event.is_some_and(|e| pc_witnesses(view, e));
                if let Some(event) = event.filter(|_| witnessed) {
                    texts.push(event.text.clone());
                }
            }
            LedgerOp::AddFact {
                source: FactSource::WorldEvent,
                slot,
                seen_by,
                canon,
                ..
            } if witnessed => {
                if perceptible_effect(view, pc.id, here.as_deref(), slot) {
                    if !seen_by.contains(&pc_ref) {
                        seen_by.push(pc_ref);
                    }
                    *canon = Canon::Persistent;
                }
            }
            LedgerOp::SupersedeFact { new, .. }
                if witnessed && new.source == FactSource::WorldEvent =>
            {
                if perceptible_effect(view, pc.id, here.as_deref(), &new.slot) {
                    if !new.seen_by.contains(&pc_ref) {
                        new.seen_by.push(pc_ref);
                    }
                    new.canon = Canon::Persistent;
                }
            }
            LedgerOp::ShiftAttitude { .. } | LedgerOp::AddClaim { .. } => {}
            _ => witnessed = false,
        }
    }
    (ops, texts)
}

/// The keys of the PC that an event the PC witnesses may change in front of
/// the PC: what happens to the PC (the same list as
/// `checks::PC_INVOLUNTARY_KEYS`). The town's "Orsel summons the scribe"
/// sets the PC's `standing` to summoned, and the PC is there to hear it.
const PC_FELT_KEYS: &[&str] = &[
    "condition",
    "injury",
    "fatigue",
    "carrying",
    "reputation",
    "standing",
    "debt",
];

/// True when the PC, who witnesses an event, perceives this effect of it:
/// - the `location` of an entity that is not hidden, when that entity
///   openly stands at the PC's place (the PC sees it leave) or the new
///   place is the PC's place (the PC sees it come);
/// - a slot of the PC itself on a key of what happens to the PC
///   (`PC_FELT_KEYS`): the event acts on the PC in the PC's presence.
///
/// Any other effect (a debt, a plan, a feeling, a slot of an absent entity)
/// is hidden state: the event's text may show its visible side, and only
/// the event's declared witnesses learn the fact.
fn perceptible_effect(view: &WorldView, pc: EntityId, here: Option<&str>, slot: &Slot) -> bool {
    let EntityRef::Id(subject) = slot.subject else {
        return false;
    };
    let key = normalize::key(&slot.key);
    if subject == pc {
        return PC_FELT_KEYS.contains(&key.as_str());
    }
    key == "location"
        && !hidden(view, subject)
        && (openly_here(view, subject)
            || here.is_some_and(|here| normalize::value(&slot.value) == here))
}

fn hidden_ref(view: &WorldView, entity: EntityRef) -> bool {
    match entity {
        EntityRef::Id(id) => hidden(view, id),
        EntityRef::New(_) => false,
    }
}
