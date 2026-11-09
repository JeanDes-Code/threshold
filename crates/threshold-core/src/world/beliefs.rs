use std::collections::BTreeMap;

use crate::domain::{ClockMin, EntityId, EntityKind, Key, Knowing, Value};
use crate::store::WorldView;

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
/// has arrived (`arrives_at <= clock`). Latest means the highest `arrives_at`;
/// on a tie, the row written last. Sorted by subject, then key.
pub fn beliefs(view: &WorldView, holder: EntityId) -> Vec<Belief> {
    let mut latest: BTreeMap<(EntityId, Key), (ClockMin, Belief)> = BTreeMap::new();
    for row in view
        .knowledge
        .iter()
        .filter(|row| row.holder == holder && row.arrives_at <= view.clock_min)
    {
        let belief = if let Some(id) = row.fact_id {
            let Some(fact) = view.facts.iter().find(|fact| fact.id == id) else {
                continue;
            };
            Belief {
                subject: fact.slot.subject,
                key: fact.slot.key.clone(),
                value: fact.slot.value.clone(),
                text: fact.text.clone(),
                how: row.how,
                source: None,
            }
        } else if let Some(id) = row.claim_id {
            let Some(claim) = view.claims.iter().find(|claim| claim.id == id) else {
                continue;
            };
            Belief {
                subject: claim.claim.slot.subject,
                key: claim.claim.slot.key.clone(),
                value: claim.claim.slot.value.clone(),
                text: claim.claim.text.clone(),
                how: row.how,
                source: Some(claim.claim.speaker),
            }
        } else {
            continue;
        };
        let key = (belief.subject, belief.key.clone());
        if latest
            .get(&key)
            .is_none_or(|(arrived, _)| row.arrives_at >= *arrived)
        {
            latest.insert(key, (row.arrives_at, belief));
        }
    }
    latest.into_values().map(|(_, belief)| belief).collect()
}

/// PC Knowledge: the PC's Beliefs. Empty when the campaign has no PC.
pub fn pc_knowledge(view: &WorldView) -> Vec<Belief> {
    view.entities
        .iter()
        .find(|entity| entity.kind == EntityKind::Pc)
        .map(|pc| beliefs(view, pc.id))
        .unwrap_or_default()
}
