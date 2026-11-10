//! `PcSight`: the world as the PC knows it. It is the one door from
//! `WorldView` to a Player screen: a view made from it cannot show a hidden
//! fact, because it does not hold one (play experience data shape 2).
//!
//! Five rules live here: who the PC can name (3.5), who is here and the
//! Moment of each Turn (3.3), the state of each Open Item (3.4), and which
//! Turn is a decision of the PC (3.2).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::beliefs::{Belief, belief, held};
use crate::{
    context::seen_present,
    domain::{
        ActionKind, Canon, ClockMin, EntityId, EntityKind, FactSource, Key, Knowing, Verdict,
        is_alias_key, is_open_key, outcome_key,
    },
    normalize,
    rules::RollResult,
    store::{Fact, Knowledge, StoredTurn, TurnId, TurnKind, WorldView},
    turn::{stored_action, stored_roll},
};

/// A Turn that moves the World Clock by more than this starts a new Scene.
pub const SCENE_GAP_MIN: ClockMin = 240;

/// One Opening, Play or Correction Turn that stands, as the PC lived it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moment {
    pub turn: TurnId,
    pub kind: TurnKind,
    /// The PC's `location` after the Turn.
    pub place: Option<String>,
    pub clock_after: ClockMin,
    /// The `seen_present` rule at this Turn, the PC included.
    pub present: Vec<EntityId>,
    /// The PC's `knowledge` rows that arrived in this Turn. No witnessed
    /// `location` of another entity: that is presence.
    pub learned: Vec<Belief>,
    pub decision: bool,
}

/// An entity the PC perceives or holds a Belief about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    pub id: EntityId,
    pub kind: EntityKind,
    /// The main name, or `None`: perceived, not named.
    pub name: Option<String>,
    /// The other names the PC holds an `alias_` Belief for, and no other.
    pub other_names: Vec<String>,
}

/// A PC Belief on an open key (ADR 0005), and the Turn that closed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenItem {
    pub subject: EntityId,
    /// The open key, `pending_` prefix included.
    pub key: Key,
    pub text: String,
    /// The Turn of the first PC row from a fact on the outcome slot, learned
    /// after the open one. A Claim never closes an item.
    pub closed_in: Option<TurnId>,
}

/// One Opening, Play or Correction Turn, reverted or not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SightTurn {
    pub turn: TurnId,
    pub kind: TurnKind,
    pub intent: String,
    pub roll: Option<RollResult>,
    pub reverted: bool,
    pub narration: Option<String>,
}

/// The world as the PC knows it. Each field comes from PC Knowledge, from
/// what the PC perceives, or from Turn rows the Player already read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PcSight {
    pub pc: EntityId,
    pub pc_name: String,
    pub concept: String,
    pub clock_min: ClockMin,
    /// The place entity the PC can name, if one has the location's name, and
    /// the location text.
    pub place: Option<(Option<EntityId>, String)>,
    /// `seen_present` now, the PC included.
    pub present: Vec<EntityId>,
    /// The PC, each entity of a `present` list, each subject or speaker of a
    /// kept Belief, and the named place. Sorted by id.
    pub known: Vec<Known>,
    /// PC Beliefs now, Setup rows included, none about an unnamed entity.
    pub beliefs: Vec<Belief>,
    /// "Known before the Campaign": the PC rows of Setup Turns.
    pub before: Vec<Belief>,
    /// One per standing Opening, Play and Correction Turn, in `seq` order.
    pub moments: Vec<Moment>,
    pub open_items: Vec<OpenItem>,
    /// Opening, Play and Correction Turns, reverted ones included.
    pub story: Vec<SightTurn>,
    /// A reverted Turn and the Correction Turn that replaced it.
    pub replaced_by: BTreeMap<TurnId, TurnId>,
    /// No Opening Turn and no Play Turn yet.
    pub needs_begin: bool,
    /// The latest Turn is a standing Play Turn (`Engine::correct_last`).
    pub can_correct: bool,
}

impl PcSight {
    /// The Moments cut into Scenes: a Scene ends where the PC's place
    /// changes, or before a Turn that moves the clock by more than
    /// `SCENE_GAP_MIN`.
    pub fn scenes(&self) -> Vec<&[Moment]> {
        let mut scenes = Vec::new();
        let mut start = 0;
        for (i, pair) in self.moments.windows(2).enumerate() {
            let (before, moment) = (&pair[0], &pair[1]);
            if moment.place != before.place
                || moment.clock_after.saturating_sub(before.clock_after) > SCENE_GAP_MIN
            {
                scenes.push(&self.moments[start..=i]);
                start = i + 1;
            }
        }
        if start < self.moments.len() {
            scenes.push(&self.moments[start..]);
        }
        scenes
    }
}

fn is_story(kind: TurnKind) -> bool {
    matches!(
        kind,
        TurnKind::Opening | TurnKind::Play | TurnKind::Correction
    )
}

/// The PC's sight of `view`. `turns` are the campaign's Turn rows (Setup
/// Turns included) and `reverted` the Turns a Correction reverted. `None`
/// when the campaign has no PC.
pub fn pc_sight(
    view: &WorldView,
    turns: &[StoredTurn],
    reverted: &HashSet<TurnId>,
) -> Option<PcSight> {
    let pc = view.entities.iter().find(|e| e.kind == EntityKind::Pc)?;
    let mut ordered: Vec<&StoredTurn> = turns.iter().collect();
    ordered.sort_by_key(|t| t.seq);
    let seq: HashMap<TurnId, u64> = ordered.iter().map(|t| (t.id, t.seq)).collect();
    let standing: Vec<&StoredTurn> = ordered
        .iter()
        .copied()
        .filter(|t| is_story(t.record.kind) && !reverted.contains(&t.id))
        .collect();
    // The PC's rows in the order they were written, with that order.
    let rows: Vec<&Knowledge> = view
        .knowledge
        .iter()
        .filter(|r| r.holder == pc.id)
        .collect();
    let order: HashMap<*const Knowledge, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| (std::ptr::from_ref(*row), i))
        .collect();
    let at = |row: &Knowledge| (row.learned_at, order[&std::ptr::from_ref(row)]);
    let now = held(view, rows.iter().copied(), view.clock_min);

    let named = named(view, pc.id, &now, &standing);
    let kept = |b: &Belief| named.contains(&b.subject);

    let beliefs: Vec<Belief> = now
        .iter()
        .map(|(b, _)| b)
        .filter(|b| kept(b))
        .cloned()
        .collect();
    let setup: HashSet<TurnId> = ordered
        .iter()
        .filter(|t| t.record.kind == TurnKind::Setup)
        .map(|t| t.id)
        .collect();
    let arrived = || {
        rows.iter()
            .copied()
            .filter(|r| r.arrives_at <= view.clock_min)
    };
    let before: Vec<Belief> = arrived()
        .filter(|r| setup.contains(&r.turn))
        .filter_map(|r| belief(view, r))
        .filter(|b| kept(b))
        .collect();

    let open_items: Vec<OpenItem> = now
        .iter()
        .filter(|(b, _)| kept(b) && is_open_key(&b.key))
        .map(|(b, open)| {
            let closed_in = outcome_key(&b.key).and_then(|outcome| {
                arrived()
                    .filter(|r| at(r) > at(open))
                    .find(|r| {
                        r.fact_id
                            .and_then(|id| view.facts.iter().find(|f| f.id == id))
                            .is_some_and(|f| f.slot.subject == b.subject && f.slot.key == outcome)
                    })
                    .map(|r| r.turn)
            });
            OpenItem {
                subject: b.subject,
                key: b.key.clone(),
                text: b.text.clone(),
                closed_in,
            }
        })
        .collect();

    let mut moments: Vec<Moment> = standing
        .iter()
        .map(|t| {
            let s = seq[&t.id];
            let place = location_at(view, pc.id, s, &seq);
            let then = held(
                view,
                rows.iter()
                    .copied()
                    .filter(|r| seq.get(&r.turn).is_some_and(|q| *q <= s)),
                t.record.clock_after,
            );
            Moment {
                turn: t.id,
                kind: t.record.kind,
                present: present_at(view, pc.id, place.as_deref(), &then),
                place,
                clock_after: t.record.clock_after,
                learned: Vec::new(),
                decision: decision(t, view, &rows, &open_items),
            }
        })
        .collect();
    for row in arrived().filter(|r| !setup.contains(&r.turn)) {
        let Some(b) = belief(view, row) else {
            continue;
        };
        let presence = row.how == Knowing::Witnessed && b.key == "location" && b.subject != pc.id;
        if presence || !kept(&b) {
            continue;
        }
        let written = seq.get(&row.turn).copied().unwrap_or(0);
        if let Some(m) = moments
            .iter_mut()
            .find(|m| seq[&m.turn] >= written && m.clock_after >= row.arrives_at)
        {
            m.learned.push(b);
        }
    }

    let present = seen_present(view);
    let place = view.live_fact(pc.id, "location").map(|f| {
        let name = normalize::name(&f.slot.value);
        let entity = view
            .entities
            .iter()
            .filter(|e| e.kind == EntityKind::Place && e.norm_name == name)
            .map(|e| e.id)
            .min()
            .filter(|id| named.contains(id));
        (entity, f.slot.value.clone())
    });

    let mut ids: BTreeSet<EntityId> = BTreeSet::from([pc.id]);
    ids.extend(present.iter().copied());
    ids.extend(moments.iter().flat_map(|m| m.present.iter().copied()));
    ids.extend(place.as_ref().and_then(|(entity, _)| *entity));
    let held_by_pc = beliefs
        .iter()
        .chain(&before)
        .chain(moments.iter().flat_map(|m| &m.learned));
    for b in held_by_pc {
        ids.insert(b.subject);
        ids.extend(b.source);
    }
    let known = ids
        .into_iter()
        .filter_map(|id| view.entities.iter().find(|e| e.id == id))
        .map(|e| {
            let mut other_names: Vec<String> = Vec::new();
            for b in beliefs
                .iter()
                .filter(|b| b.subject == e.id && is_alias_key(&b.key))
            {
                let spelled = spelled(&b.text, &b.value);
                if !other_names.contains(&spelled) {
                    other_names.push(spelled);
                }
            }
            Known {
                id: e.id,
                kind: e.kind,
                name: named.contains(&e.id).then(|| e.name.clone()),
                other_names,
            }
        })
        .collect();

    let story: Vec<SightTurn> = ordered
        .iter()
        .filter(|t| is_story(t.record.kind))
        .map(|t| SightTurn {
            turn: t.id,
            kind: t.record.kind,
            intent: t.record.intent.clone(),
            roll: stored_roll(&t.record.proposal_json),
            reverted: reverted.contains(&t.id),
            narration: t.narration.clone(),
        })
        .collect();
    let replaced_by = story
        .iter()
        .filter(|t| t.reverted)
        .filter_map(|old| {
            standing
                .iter()
                .find(|t| t.record.kind == TurnKind::Correction && t.seq > seq[&old.turn])
                .map(|t| (old.turn, t.id))
        })
        .collect();

    Some(PcSight {
        pc: pc.id,
        pc_name: pc.name.clone(),
        concept: view
            .adventure_seed
            .as_ref()
            .map(|s| s.pc.concept.clone())
            .unwrap_or_default(),
        clock_min: view.clock_min,
        place,
        present,
        known,
        beliefs,
        before,
        moments,
        open_items,
        story,
        replaced_by,
        needs_begin: !ordered
            .iter()
            .any(|t| matches!(t.record.kind, TurnKind::Opening | TurnKind::Play)),
        can_correct: ordered
            .last()
            .is_some_and(|t| t.record.kind == TurnKind::Play && !reverted.contains(&t.id)),
    })
}

/// The entities the PC can name (data shape 3.5): the PC; an Adventure Seed
/// entry that is not hidden; an entity the PC holds a Belief about that is
/// not a witnessed `location`; an entity whose main name is in the Narration
/// of a standing Turn, as whole words.
fn named(
    view: &WorldView,
    pc: EntityId,
    now: &[(Belief, &Knowledge)],
    standing: &[&StoredTurn],
) -> HashSet<EntityId> {
    let mut named = HashSet::from([pc]);
    if let Some(seed) = &view.adventure_seed {
        for entry in seed.entities.iter().filter(|entry| !entry.hidden) {
            let name = normalize::name(&entry.name);
            named.extend(
                view.entities
                    .iter()
                    .filter(|e| e.norm_name == name)
                    .map(|e| e.id)
                    .min(),
            );
        }
    }
    named.extend(
        now.iter()
            .map(|(b, _)| b)
            .filter(|b| !(b.how == Knowing::Witnessed && b.key == "location"))
            .map(|b| b.subject),
    );
    let narrations: Vec<Vec<String>> = standing
        .iter()
        .filter_map(|t| t.narration.as_deref())
        .map(normalize::name_words)
        .collect();
    for entity in &view.entities {
        let words = normalize::name_words(&normalize::name(&entity.name));
        if !words.is_empty()
            && narrations
                .iter()
                .any(|text| text.windows(words.len()).any(|w| w == words.as_slice()))
        {
            named.insert(entity.id);
        }
    }
    named
}

/// The PC's `location` after the Turn of `seq` `at`: the fact that was live
/// then, preferring one that is not Possible, as `WorldView::live_fact` does.
fn location_at(
    view: &WorldView,
    pc: EntityId,
    at: u64,
    seq: &HashMap<TurnId, u64>,
) -> Option<String> {
    let written = |f: &Fact| seq.get(&f.created_turn).copied();
    let live: Vec<&Fact> = view
        .facts
        .iter()
        .filter(|f| f.slot.subject == pc && f.slot.key == "location")
        .filter(|f| written(f).is_some_and(|s| s <= at))
        .filter(|f| {
            f.superseded_by.is_none_or(|next| {
                view.facts
                    .iter()
                    .find(|g| g.id == next)
                    .and_then(written)
                    .is_none_or(|s| s > at)
            })
        })
        .collect();
    live.iter()
        .find(|f| f.canon != Canon::Possible)
        .or(live.first())
        .map(|f| f.slot.value.clone())
}

/// The `seen_present` rule on the Beliefs the PC held then: the PC, and each
/// entity the PC believes stands at `place`.
fn present_at(
    view: &WorldView,
    pc: EntityId,
    place: Option<&str>,
    then: &[(Belief, &Knowledge)],
) -> Vec<EntityId> {
    let Some(place) = place.map(normalize::value) else {
        return vec![pc];
    };
    let mut seen: BTreeSet<EntityId> = then
        .iter()
        .map(|(b, _)| b)
        .filter(|b| b.key == "location" && normalize::value(&b.value) == place)
        .map(|b| b.subject)
        .filter(|id| view.entities.iter().any(|e| e.id == *id))
        .collect();
    seen.insert(pc);
    seen.into_iter().collect()
}

/// A decision of the PC (data shape 3.2): a standing Play or Correction
/// Turn whose stored action is `talk`, `move` or `act`, not impossible, that
/// has a Roll, wrote a Persistent fact from the proposal, its Check or a
/// Correction, or wrote or closed an Open Item the PC knows.
fn decision(
    turn: &StoredTurn,
    view: &WorldView,
    rows: &[&Knowledge],
    open_items: &[OpenItem],
) -> bool {
    if !matches!(turn.record.kind, TurnKind::Play | TurnKind::Correction) {
        return false;
    }
    let Some((action, verdict)) = stored_action(&turn.record.proposal_json) else {
        return false;
    };
    if !matches!(
        action,
        ActionKind::Talk | ActionKind::Move | ActionKind::Act
    ) || verdict == Verdict::Impossible
    {
        return false;
    }
    let written = || view.facts.iter().filter(|f| f.created_turn == turn.id);
    stored_roll(&turn.record.proposal_json).is_some()
        || written().any(|f| {
            f.canon == Canon::Persistent
                && matches!(
                    f.source,
                    FactSource::Proposal | FactSource::Stake | FactSource::Correction
                )
        })
        || written()
            .any(|f| is_open_key(&f.slot.key) && rows.iter().any(|r| r.fact_id == Some(f.id)))
        || open_items.iter().any(|i| i.closed_in == Some(turn.id))
}

/// `value` as `text` spells it: "the Toll-Wolf" in "Orsel is also called
/// the Toll-Wolf." for the stored value "toll-wolf". The longest run of
/// words whose `normalize::name` is the value's wins; `value` itself when
/// none is.
fn spelled(text: &str, value: &str) -> String {
    let target = normalize::name(value);
    let words: Vec<(usize, usize)> = text
        .split_whitespace()
        .map(|word| {
            let start = word.as_ptr() as usize - text.as_ptr() as usize;
            (start, start + word.len())
        })
        .collect();
    let mut best: Option<&str> = None;
    for (i, &(start, _)) in words.iter().enumerate() {
        for &(_, end) in &words[i..] {
            let run = &text[start..end];
            if normalize::name(run) == target {
                let run = run.trim_matches(|c: char| !c.is_alphanumeric());
                if best.is_none_or(|b| run.len() > b.len()) {
                    best = Some(run);
                }
            }
        }
    }
    best.map_or_else(|| value.to_owned(), str::to_owned)
}
