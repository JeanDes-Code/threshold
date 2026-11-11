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
/// event's text, and the effects that `perceive` accepts. Which effects are
/// visible is data, declared by the event's `public` flag, never read from
/// its text (review of bug pass 4: a word rule taught "Orsel paid no
/// attention to the crowd" as `debt_status = paid`):
/// - a public event (an announcement, a public act): every effect on an
///   entity the PC can name and that is not hidden;
/// - any event: an entity that comes or goes at the PC's place, and what
///   happens to the PC (`checks::PC_INVOLUNTARY_KEYS`).
///
/// The PC joins the witnesses of those facts only, and they become
/// Persistent, as for any fact the PC sees (the store keeps state keys
/// Established). Every other effect stays with the witnesses the event
/// declares (bug pass 4, B1: "Ludo counts his silver" made the PC hold
/// "Ludo's debt is now partially paid", a secret seen by Agathe only).
///
/// An entity that leaves for a place the PC cannot name is seen to leave,
/// not to arrive (review B1): the PC gets a departure record
/// (`departure`), never the destination.
///
/// Returns the ops and the texts of the witnessed events, in firing order,
/// for the narrator. The texts are model text: the narrator receives them
/// only through `context::guard_events`.
pub fn with_witnessed_events(view: &WorldView, ops: Vec<LedgerOp>) -> (Vec<LedgerOp>, Vec<String>) {
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
    let standing: Vec<&crate::store::StoredTurn> = view.recent_turns.iter().collect();
    let sight = Sight {
        view,
        pc: pc.id,
        here: here.as_deref(),
        named: super::sight::pc_can_name(view, &standing),
        items: super::beliefs::pc_open_items(view)
            .into_iter()
            .filter_map(|(subject, key)| {
                crate::domain::outcome_key(&key).map(|k| (subject, k.to_owned()))
            })
            .collect(),
    };
    let mut texts = Vec::new();
    let mut out = Vec::with_capacity(ops.len());
    // The witnessed event whose effect ops follow, if the PC witnesses it.
    // `advance` puts each event's effect ops right after its `FireEvent`.
    let mut witnessed: Option<&WorldEvent> = None;
    for mut op in ops {
        let mut extra = None;
        match &mut op {
            LedgerOp::FireEvent { event_id } => {
                witnessed = view
                    .events_scheduled
                    .iter()
                    .find(|e| e.id == *event_id)
                    .filter(|e| pc_witnesses(view, e));
                texts.extend(witnessed.map(|e| e.text.clone()));
            }
            LedgerOp::AddFact {
                source: FactSource::WorldEvent,
                slot,
                text,
                seen_by,
                canon,
                ..
            } if let Some(event) = witnessed => match sight.perceive(slot, text, event) {
                Perceived::Effect => {
                    if !seen_by.contains(&pc_ref) {
                        seen_by.push(pc_ref);
                    }
                    *canon = Canon::Persistent;
                }
                Perceived::Departure => extra = Some(sight.departure(slot.subject)),
                Perceived::Nothing => {}
            },
            LedgerOp::SupersedeFact { new, .. }
                if let Some(event) = witnessed
                    && new.source == FactSource::WorldEvent =>
            {
                match sight.perceive(&new.slot, &new.text, event) {
                    Perceived::Effect => {
                        if !new.seen_by.contains(&pc_ref) {
                            new.seen_by.push(pc_ref);
                        }
                        new.canon = Canon::Persistent;
                    }
                    Perceived::Departure => extra = Some(sight.departure(new.slot.subject)),
                    Perceived::Nothing => {}
                }
            }
            LedgerOp::ShiftAttitude { .. } | LedgerOp::AddClaim { .. } => {}
            _ => witnessed = None,
        }
        out.push(op);
        out.extend(extra);
    }
    (out, texts)
}

/// What the PC perceives of one effect of a witnessed event.
enum Perceived {
    /// The PC witnesses the fact itself.
    Effect,
    /// The PC sees the entity leave, and not where to (review B1).
    Departure,
    Nothing,
}

/// What `with_witnessed_events` reads to judge one effect.
struct Sight<'a> {
    view: &'a WorldView,
    pc: EntityId,
    /// The PC's place, normalized.
    here: Option<&'a str>,
    /// The entities the PC can name (`world::pc_can_name`).
    named: std::collections::HashSet<EntityId>,
    /// The outcome slots of the Open Items open for the PC, as `(subject,
    /// outcome key)`.
    items: Vec<(EntityId, String)>,
}

impl Sight<'_> {
    /// What the PC, who witnesses `event`, perceives of this effect, whose
    /// fact text is `text`:
    /// - the `location` of an entity that is not hidden, when the entity
    ///   comes to the PC's place (the PC sees it come), or openly stands at
    ///   the PC's place and goes to a place the PC can name (the PC sees
    ///   where it goes). One that goes to a place the PC cannot name is a
    ///   departure: the PC sees it leave (review B1);
    /// - a slot of the PC itself on a key of what happens to the PC
    ///   (`checks::PC_INVOLUNTARY_KEYS`): the event acts on the PC in the
    ///   PC's presence;
    /// - when the event is `public`: the outcome slot of an Open Item the
    ///   PC holds, on the PC (never another key of the PC: a public event
    ///   gives the PC no feeling and no choice); and any other slot of an
    ///   entity the PC can name, when the fact's text and value name
    ///   nothing the PC must not hear of (`context::names_unheard`).
    ///
    /// Any other effect (a debt, a plan, a feeling, a slot of an absent
    /// entity, every effect of a private event) is hidden state: only the
    /// event's declared witnesses learn the fact (bug pass 4, B1).
    fn perceive(&self, slot: &Slot, text: &str, event: &WorldEvent) -> Perceived {
        let EntityRef::Id(subject) = slot.subject else {
            return Perceived::Nothing;
        };
        let key = normalize::key(&slot.key);
        if subject == self.pc {
            let involuntary = crate::checks::PC_INVOLUNTARY_KEYS.contains(&key.as_str());
            let outcome = self.items.iter().any(|(s, k)| *s == subject && *k == key);
            return if involuntary || (event.public && outcome) {
                Perceived::Effect
            } else {
                Perceived::Nothing
            };
        }
        if hidden(self.view, subject) {
            return Perceived::Nothing;
        }
        if key != "location" {
            let public = event.public
                && self.named.contains(&subject)
                && !crate::context::names_unheard(self.view, text)
                && !crate::context::names_unheard(self.view, &slot.value);
            return if public {
                Perceived::Effect
            } else {
                Perceived::Nothing
            };
        }
        let to = normalize::value(&slot.value);
        if self.here == Some(to.as_str()) {
            return Perceived::Effect;
        }
        if !openly_here(self.view, subject) {
            return Perceived::Nothing;
        }
        if self.can_name_place(&slot.value) {
            Perceived::Effect
        } else {
            Perceived::Departure
        }
    }

    /// True when `location` is the name of a place the PC can name.
    fn can_name_place(&self, location: &str) -> bool {
        self.view
            .entities_by_name(location)
            .any(|e| e.kind == EntityKind::Place && self.named.contains(&e.id))
    }

    /// The departure record of `subject` (review B1): a fact on its
    /// `location` with no place, the text "<name> leaves.", witnessed by
    /// the PC alone. It is Possible, so it never takes the slot from the
    /// event's own fact, which stays world truth; the store never makes it
    /// Established while that fact holds the slot, and no GM prompt shows
    /// it (`context::Retrieval`). The PC's Belief on it puts the entity
    /// nowhere the PC knows: the side panel's Here and the narrator's
    /// Present leave it out, and the narrator's outcome reads "<name>
    /// leaves.". An entity the PC cannot name is "Someone".
    fn departure(&self, subject: EntityRef) -> LedgerOp {
        let name = match subject {
            EntityRef::Id(id) if self.named.contains(&id) => self
                .view
                .entities
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.name.clone()),
            _ => None,
        };
        LedgerOp::AddFact {
            slot: Slot {
                subject,
                key: "location".into(),
                value: String::new(),
            },
            text: format!("{} leaves.", name.as_deref().unwrap_or("Someone")),
            canon: Canon::Possible,
            source: FactSource::WorldEvent,
            seen_by: vec![EntityRef::Id(self.pc)],
            causes: Vec::new(),
        }
    }
}

fn hidden_ref(view: &WorldView, entity: EntityRef) -> bool {
    match entity {
        EntityRef::Id(id) => hidden(view, id),
        EntityRef::New(_) => false,
    }
}
