use std::collections::BTreeMap;

use crate::domain::{
    ClockMin, EntityId, EntityKind, Key, Knowing, Value, is_open_key, outcome_key,
};
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

/// The Open Items the PC holds that are still open for the PC, as
/// `(subject, open key)`: the rule of `pc_items` (review M2). A Turn whose
/// Intent settles one names it in a closing `settles`, and the core writes
/// its outcome (ADR 0005, ADR 0007). Sorted, no duplicates.
pub fn pc_open_items(view: &WorldView) -> Vec<(EntityId, Key)> {
    let Some(pc) = view.entities.iter().find(|e| e.kind == EntityKind::Pc) else {
        return Vec::new();
    };
    let rows: Vec<&Knowledge> = view
        .knowledge
        .iter()
        .filter(|r| r.holder == pc.id)
        .collect();
    let mut items: Vec<(EntityId, Key)> = pc_items(view, pc.id, &rows)
        .into_iter()
        .filter(|item| item.closing.is_none())
        .map(|item| (item.belief.subject, item.belief.key))
        .collect();
    items.sort();
    items.dedup();
    items
}

/// One Open Item as the PC holds it.
pub(super) struct ItemState<'a> {
    /// The PC's Belief on the open key now.
    pub belief: Belief,
    /// The place, among the PC's rows in the order they were written, of
    /// the first row on the fact or the Claim of that Belief.
    pub first: usize,
    /// The PC's row that closed the item for the PC, if one did.
    pub closing: Option<&'a Knowledge>,
}

/// Every Open Item the PC holds, open or closed for the PC: each PC Belief
/// on a `pending_` key, and the first later PC row that closes it
/// (`closes_item`). `rows` are the PC's `knowledge` rows in the order they
/// were written. The one rule of "open for the PC" (review M2): an item is
/// open while the PC holds the pending Belief and no row that closes it.
/// The world's state does not count: an outcome slot that an event wrote
/// without the PC leaves the item open for the PC, on the screens, in the
/// offer of `settles` and in `pc_open_items` alike. `pc_sight` reads it for
/// the screens.
pub(super) fn pc_items<'a>(
    view: &WorldView,
    pc: EntityId,
    rows: &[&'a Knowledge],
) -> Vec<ItemState<'a>> {
    let at = |i: usize| (rows[i].learned_at, i);
    held(view, rows.iter().copied(), view.clock_min)
        .into_iter()
        .filter(|(b, _)| is_open_key(&b.key))
        .filter_map(|(belief, open)| {
            // The first time the PC learned this open row. A later row on the
            // same fact (a proposal that restates it) must not re-open an
            // item that an outcome already closed.
            let first = (0..rows.len())
                .filter(|i| rows[*i].fact_id == open.fact_id && rows[*i].claim_id == open.claim_id)
                .min_by_key(|i| at(*i))?;
            let closing = outcome_key(&belief.key).and_then(|outcome| {
                (0..rows.len())
                    .filter(|i| rows[*i].arrives_at <= view.clock_min && at(*i) > at(first))
                    .map(|i| rows[i])
                    .find(|row| closes_item(view, pc, row, belief.subject, outcome))
            });
            Some(ItemState {
                belief,
                first,
                closing,
            })
        })
        .collect()
}

/// True when the PC's `row` closes an Open Item whose outcome slot is
/// `(subject, outcome)` for the PC: a row from a fact on that slot, or from
/// the PC's own Claim on it (a `settles` whose slot the world had already
/// filled, review M2). A Claim of anyone else never closes an item: it is
/// news the PC was told, and the item stays open.
fn closes_item(
    view: &WorldView,
    pc: EntityId,
    row: &Knowledge,
    subject: EntityId,
    outcome: &str,
) -> bool {
    if let Some(id) = row.fact_id {
        return view
            .facts
            .iter()
            .find(|f| f.id == id)
            .is_some_and(|f| f.slot.subject == subject && f.slot.key == outcome);
    }
    row.claim_id
        .and_then(|id| view.claims.iter().find(|c| c.id == id))
        .is_some_and(|c| {
            c.claim.speaker == pc && c.claim.slot.subject == subject && c.claim.slot.key == outcome
        })
}

/// The text of an Open Item as the PC holds it: the text of the PC's Belief
/// on `(subject, open key)`. `None` when the PC holds no Belief there or
/// its text is blank. The live fact may say more: a GM-only write can
/// replace it, and the PC never learns that text (review B3). The prompt's
/// offered items and the outcome fact read this text, never the live one.
pub fn pc_item_text(view: &WorldView, subject: EntityId, key: &str) -> Option<String> {
    pc_knowledge(view)
        .into_iter()
        .find(|b| b.subject == subject && b.key == key)
        .map(|b| b.text)
        .filter(|text| !text.trim().is_empty())
}

/// How the proposal prompt and `settles.item` name an Open Item: the
/// subject's handle, a space, and the open key, as a fact line shows them.
pub fn open_item_ref(subject: EntityId, key: &str) -> String {
    format!("E{subject} {key}")
}
