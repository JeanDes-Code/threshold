use std::collections::BTreeMap;

use crate::domain::{ClockMin, EntityId, EntityKind, Key, Knowing, Value, outcome_key};
use crate::store::{Knowledge, WorldView};

/// What a holder takes to be true about one slot. A Belief can be false: a
/// `Told` Belief comes from a Claim, and a Claim never writes a slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Belief {
    pub subject: EntityId,
    pub key: Key,
    pub value: Value,
    pub text: String,
    pub how: Knowing,
    /// The speaker of a Claim; `None` for a fact.
    pub source: Option<EntityId>,
}

/// The holder's Beliefs: the latest `knowledge` row per `(subject, key)` that
/// has arrived (`arrives_at <= clock`). Latest means the highest `learned_at`,
/// the time the information was true or said, so late news never overrides a
/// newer witness. On a tie, the row written last. Sorted by subject, then key.
pub fn beliefs(view: &WorldView, holder: EntityId) -> Vec<Belief> {
    held(
        view,
        view.knowledge.iter().filter(|row| row.holder == holder),
        view.clock_min,
    )
    .into_iter()
    .map(|(belief, _)| belief)
    .collect()
}

/// The Belief rule, the one copy of it: of `rows` (one holder's, in the
/// order they were written), the latest per `(subject, key)` among those
/// that arrived by `clock`, each with its row. `beliefs` and `pc_sight`
/// both call it.
pub(super) fn held<'a>(
    view: &WorldView,
    rows: impl IntoIterator<Item = &'a Knowledge>,
    clock: ClockMin,
) -> Vec<(Belief, &'a Knowledge)> {
    let mut latest: BTreeMap<(EntityId, Key), (Belief, &'a Knowledge)> = BTreeMap::new();
    for row in rows.into_iter().filter(|row| row.arrives_at <= clock) {
        let Some(belief) = belief(view, row) else {
            continue;
        };
        let key = (belief.subject, belief.key.clone());
        if latest
            .get(&key)
            .is_none_or(|(_, kept)| row.learned_at >= kept.learned_at)
        {
            latest.insert(key, (belief, row));
        }
    }
    latest.into_values().collect()
}

/// The Belief one `knowledge` row gives: its fact, or its Claim with the
/// speaker. `None` when the view does not hold the fact or the Claim.
pub(super) fn belief(view: &WorldView, row: &Knowledge) -> Option<Belief> {
    if let Some(id) = row.fact_id {
        let fact = view.facts.iter().find(|fact| fact.id == id)?;
        Some(Belief {
            subject: fact.slot.subject,
            key: fact.slot.key.clone(),
            value: fact.slot.value.clone(),
            text: fact.text.clone(),
            how: row.how,
            source: None,
        })
    } else {
        let id = row.claim_id?;
        let claim = view.claims.iter().find(|claim| claim.id == id)?;
        Some(Belief {
            subject: claim.claim.slot.subject,
            key: claim.claim.slot.key.clone(),
            value: claim.claim.slot.value.clone(),
            text: claim.claim.text.clone(),
            how: row.how,
            source: Some(claim.claim.speaker),
        })
    }
}

/// PC Knowledge: the PC's Beliefs. Empty when the campaign has no PC.
pub fn pc_knowledge(view: &WorldView) -> Vec<Belief> {
    view.entities
        .iter()
        .find(|entity| entity.kind == EntityKind::Pc)
        .map(|pc| beliefs(view, pc.id))
        .unwrap_or_default()
}

/// The outcome keys of the Open Items the PC holds about the PC: for each
/// live fact on the PC on a `pending_x` key that the PC believes, `x`. A
/// Turn whose Intent settles the item may write `x` on the PC, which closes
/// it (ADR 0005, changed after QA, 2026-11-10). Sorted, no duplicates.
pub fn pc_open_outcomes(view: &WorldView) -> Vec<String> {
    let Some(pc) = view.entities.iter().find(|e| e.kind == EntityKind::Pc) else {
        return Vec::new();
    };
    let mut keys: Vec<String> = beliefs(view, pc.id)
        .into_iter()
        .filter(|b| b.subject == pc.id && view.live_fact(pc.id, &b.key).is_some())
        .filter_map(|b| outcome_key(&b.key).map(str::to_owned))
        .filter(|k| !k.is_empty())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}
