use crate::domain::{ClockMin, EntityKind, EntityRef, FactSource, Knowing, LedgerOp};
use crate::normalize;
use crate::store::{Fact, WorldView};

/// Perception on arrival: the PC witnesses the live `location` fact of each
/// entity that truly stands at `location` in `view`.
///
/// Two kinds of presence stay unseen:
/// - an entity the Adventure Seed declares `hidden`;
/// - a location fact placed out of the PC's sight on purpose: one written by
///   a proposal (or a Correction) with no PC witness, that is `seen = gm`.
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

/// A location fact written with no PC witness on purpose (`seen = gm`).
/// Adventure Seed facts and World Event effects carry no such choice.
fn placed_unseen(fact: &Fact, pc: u64) -> bool {
    !matches!(
        fact.source,
        FactSource::AdventureSeed | FactSource::WorldEvent
    ) && !fact.seen_by.contains(&pc)
}
