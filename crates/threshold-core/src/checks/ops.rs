//! Turns a validated proposal and its Roll into ledger operations.

use std::collections::HashSet;

use super::{handle_ceiling, pc};
use crate::{
    domain::{
        ActionKind, Canon, Cause, ClaimDraft, DecisionProposal, Effect, EntityKind, EntityRef,
        FactDraft, FactSource, Handle, Knowing, LedgerOp, NewFact, Reaction, Seen, Slot, SlotDraft,
        Trigger, Verdict,
    },
    normalize,
    rules::RollResult,
    store::{Fact, WorldView},
};

/// The result of `to_ops`: the ops of one Commit, and the final reactions
/// after any clamp. A stance is Turn output, not world state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub ops: Vec<LedgerOp>,
    pub reactions: Vec<Reaction>,
}

/// Converts a proposal that passed `validate` into the ops of one Commit.
///
/// `roll` is the Roll of the proposal's Check, if any. With a Check and no
/// Roll, no stake applies and every `Cause::Check` reaction is clamped.
/// Every fact in `view` predates the Turn.
pub fn to_ops(
    valid: super::Validated<'_>,
    roll: Option<&RollResult>,
    view: &WorldView,
) -> Resolved {
    let p = valid.proposal();
    let success = roll.is_some_and(|r| r.success);
    // ADR 0007: the core moves the PC to `move_to` when the verdict lets the
    // action happen: allowed, or a Check that succeeds.
    let lets_happen = match p.verdict {
        Verdict::Allowed => true,
        Verdict::Check => success,
        Verdict::Impossible => false,
    };
    // `move_to` moves the PC only with the move action (changed after
    // review); `settle` drops it otherwise.
    let moved_to = p
        .move_to
        .filter(|_| lets_happen && p.action == ActionKind::Move)
        .and_then(|h| view.entity(h))
        .map(|place| place.name.clone());
    let mut b = Builder {
        p,
        view,
        ops: Vec::new(),
        promoted: HashSet::new(),
        cleared: HashSet::new(),
        moved_to: moved_to.clone(),
    };

    b.new_npcs();
    b.promote_cited();
    if let (Some(check), Some(roll)) = (&p.check, roll) {
        b.ops.push(LedgerOp::RecordRoll {
            seed: roll.seed,
            difficulty: i8::try_from(roll.difficulty).expect("a Ruleset difficulty is at most 10"),
            against: check.against.map(entity_ref),
            stakes: serde_json::json!({ "win": check.win, "lose": check.lose }),
            factors: serde_json::to_value(&check.factors)
                .expect("factors are plain data and always serialize"),
            value: i32::from(roll.value),
            outcome: if roll.success { "win" } else { "lose" }.into(),
        });
        let stake = if roll.success {
            &check.win
        } else {
            &check.lose
        };
        if let Some(stake) = stake {
            let holders = b.holders(Seen::Present);
            b.write(stake, FactSource::Stake, holders, Vec::new());
        }
    }
    // ADR 0007: the Open Item that `settles` closes gets its outcome fact,
    // witnessed by everyone present, the PC included, when the verdict lets
    // the action happen (changed after review). The core builds the value
    // and the text from the outcome word and the item's text as the PC
    // holds it (review B3), never the live text.
    //
    // An item whose outcome slot the world already filled, unseen by the PC
    // (review M2): the core does not overwrite the world's fact for those
    // who know it. The PC's outcome is a Claim of the PC, stored as a told
    // Claim is: it closes the item for the PC (`world::pc_open_items`), and
    // the slot keeps its fact. It is the PC's own stated decision, so the
    // NPCs the PC perceives here hear it too (review of bug pass 4, m6).
    // Only a canon fact fills the slot: a Possible fact is a rumor nobody
    // committed (`Builder::current`; m7).
    for close in super::closed(p, view).into_iter().filter(|_| lets_happen) {
        let text = close.outcome.text(&close.text);
        let filled = b.current(close.subject, close.key).is_some();
        if let (true, Some(pc)) = (filled, pc(view)) {
            let listeners = std::iter::once(EntityRef::Id(pc))
                .chain(
                    view.entities
                        .iter()
                        .filter(|e| {
                            e.kind == EntityKind::Npc && crate::world::openly_here(view, e.id)
                        })
                        .map(|e| EntityRef::Id(e.id)),
                )
                .collect();
            b.ops.push(LedgerOp::AddClaim {
                claim: ClaimDraft {
                    speaker: EntityRef::Id(pc),
                    slot: Slot {
                        subject: EntityRef::Id(close.subject),
                        key: close.key.to_owned(),
                        value: close.outcome.word().to_owned(),
                    },
                    text,
                },
                listeners,
                delay_min: 0,
            });
            continue;
        }
        let holders = b.holders(Seen::Present);
        let outcome = SlotDraft {
            subject: Handle::Entity(close.subject),
            key: close.key.to_owned(),
            value: close.outcome.word().to_owned(),
        };
        b.write_text(&outcome, text, FactSource::Proposal, holders, Vec::new());
    }
    if let (Some(place), Some(pc)) = (&moved_to, pc(view)) {
        let holders = b.holders(Seen::Pc);
        let location = SlotDraft {
            subject: Handle::Entity(pc),
            key: "location".into(),
            value: place.clone(),
        };
        b.write(&location, FactSource::Proposal, holders, Vec::new());
    }
    // The facts of a proposal whose Check the core built were written for a
    // success: they apply on a won Roll only (changed after review).
    let facts: &[NewFact] = if p.facts_on_win && !success {
        &[]
    } else {
        &p.facts
    };
    for fact in facts {
        let holders = b.holders(fact.seen);
        if let Some(speaker) = fact.said_by {
            b.ops.push(LedgerOp::AddClaim {
                claim: ClaimDraft {
                    speaker: entity_ref(speaker),
                    slot: slot(&fact.slot),
                    text: b.text(&fact.slot),
                },
                listeners: holders,
                delay_min: 0,
            });
        } else {
            let causes = fact.because.iter().filter_map(|h| fact_id(*h)).collect();
            b.write(&fact.slot, FactSource::Proposal, holders, causes);
        }
    }
    b.open_blockers();
    for cancel in &p.cancel {
        if let (Handle::WorldEvent(event_id), Some(cause)) = (cancel.event, fact_id(cancel.because))
        {
            b.ops.push(LedgerOp::CancelEvent {
                event_id,
                cause_fact_id: cause,
            });
        }
    }
    if let Some(s) = &p.schedule {
        let at = view.clock_min + u64::from(p.minutes) + u64::from(s.in_minutes);
        let effects = s
            .effects
            .iter()
            .map(|e| Effect::SetFact {
                slot: slot(e),
                text: b.text(e),
                seen_by: Vec::new(),
            })
            .collect();
        b.ops.push(LedgerOp::ScheduleEvent {
            text: s.text.clone(),
            trigger: Trigger::At(at),
            actors: vec![entity_ref(s.actor)],
            goal_id: match s.goal {
                Some(Handle::Goal(g)) => Some(g),
                _ => None,
            },
            cause_fact_id: None,
            effects,
            seen_by: s.seen_by.iter().map(|h| entity_ref(*h)).collect(),
            public: s.public,
        });
    }
    if p.minutes > 0 {
        b.ops.push(LedgerOp::AdvanceClock { minutes: p.minutes });
    }

    let reactions = p
        .reactions
        .iter()
        .map(|r| {
            let mut r = r.clone();
            if r.cause == Some(Cause::Check) && !success {
                let limit = handle_ceiling(view, r.npc);
                r.stance = r.stance.min(limit);
            }
            r
        })
        .collect();
    Resolved {
        ops: b.ops,
        reactions,
    }
}

struct Builder<'a> {
    p: &'a DecisionProposal,
    view: &'a WorldView,
    ops: Vec<LedgerOp>,
    /// Possible facts this Commit makes Established.
    promoted: HashSet<u64>,
    /// Obstacles this Commit clears.
    cleared: HashSet<u64>,
    /// The place the PC moves to in this Commit (`move_to`), if any.
    moved_to: Option<String>,
}

impl Builder<'_> {
    /// Each new NPC: the entity, its Voice Card from the one-line voice seed,
    /// and a `location` fact equal to the PC's location after this Turn, so
    /// the NPC is present. The NPC is introduced into the Scene, so everyone
    /// present witnesses that location, the PC included (as `Seen::Present`).
    /// On a move, the others present stay behind: only the PC and the new
    /// NPCs witness it.
    fn new_npcs(&mut self) {
        let pc_id = pc(self.view);
        let moved_to = self.moved_to.as_deref().map(normalize::value);
        let witnesses = if moved_to.is_some() {
            pc_id
                .map(EntityRef::Id)
                .into_iter()
                .chain(
                    self.p
                        .new_npcs
                        .iter()
                        .map(|n| EntityRef::New(new_local(n.id))),
                )
                .collect()
        } else {
            self.holders(Seen::Present)
        };
        let pc_location = moved_to.or_else(|| {
            pc_id
                .and_then(|pc| self.view.live_fact(pc, "location"))
                .map(|f| f.slot.value.clone())
        });
        for npc in &self.p.new_npcs {
            let local = new_local(npc.id);
            self.ops.push(LedgerOp::AddEntity {
                local: Some(local),
                kind: EntityKind::Npc,
                name: npc.name.clone(),
                age: Some(npc.age),
                other_names: Vec::new(),
            });
            self.ops.push(LedgerOp::AddVoiceCard {
                npc: EntityRef::New(local),
                habits: npc.voice.clone(),
                vocabulary: String::new(),
                samples: Vec::new(),
            });
            if let Some(location) = &pc_location {
                self.ops.push(LedgerOp::AddFact {
                    slot: Slot {
                        subject: EntityRef::New(local),
                        key: "location".into(),
                        value: location.clone(),
                    },
                    text: format!("{} is at {location}.", npc.name),
                    canon: Canon::Established,
                    source: FactSource::Proposal,
                    seen_by: witnesses.clone(),
                    causes: Vec::new(),
                });
            }
        }
    }

    /// A Possible fact that the proposal cites (blocker, factor, cause,
    /// `because`) becomes Established.
    fn promote_cited(&mut self) {
        let p = self.p;
        let cited = p
            .blockers
            .iter()
            .chain(
                p.check
                    .iter()
                    .flat_map(|c| c.factors.iter().map(|f| &f.fact)),
            )
            .chain(p.reactions.iter().filter_map(|r| match &r.cause {
                Some(Cause::Fact(f)) => Some(f),
                _ => None,
            }))
            .chain(p.facts.iter().flat_map(|f| &f.because))
            .chain(p.cancel.iter().map(|c| &c.because));
        for handle in cited {
            if let Some(fact) = self.view.fact(*handle) {
                self.promote(fact);
            }
        }
    }

    /// Emits `EstablishFact` for a live Possible fact, unless its slot already
    /// has a live Established or Persistent value (the store allows one).
    fn promote(&mut self, fact: &Fact) -> bool {
        let live = self.view.live_facts.iter().any(|f| f.id == fact.id);
        let slot_taken = self.current(fact.slot.subject, &fact.slot.key).is_some();
        if fact.canon != Canon::Possible || !live || slot_taken {
            return false;
        }
        if self.promoted.insert(fact.id) {
            self.ops.push(LedgerOp::EstablishFact { fact_id: fact.id });
        }
        true
    }

    /// The live non-Possible fact of a slot, counting promotions made by this
    /// Commit.
    fn current(&self, subject: u64, key: &str) -> Option<&Fact> {
        let key = normalize::key(key);
        self.view.live_facts.iter().find(|f| {
            f.slot.subject == subject
                && f.slot.key == key
                && (f.canon != Canon::Possible || self.promoted.contains(&f.id))
        })
    }

    /// Each blocker opens an obstacle when the verdict is impossible or check,
    /// unless it is already open or a write of this Commit changes its slot.
    /// A fact about the PC itself never gets here: `settle` drops it (ADR
    /// 0007, changed after review: an impossible verdict opens its real
    /// blockers again).
    fn open_blockers(&mut self) {
        if !matches!(self.p.verdict, Verdict::Impossible | Verdict::Check) {
            return;
        }
        let mut opened = HashSet::new();
        for handle in &self.p.blockers {
            let Some(fact) = self.view.fact(*handle) else {
                continue;
            };
            let open = self
                .view
                .obstacles_open
                .iter()
                .any(|o| o.fact_id == fact.id);
            let removed = self
                .view
                .live_fact(fact.slot.subject, &fact.slot.key)
                .is_none_or(|live| {
                    normalize::value(&live.slot.value) != normalize::value(&fact.slot.value)
                })
                || self.ops.iter().any(|op| writes_slot(op, &fact.slot));
            if !open && !removed && opened.insert(fact.id) {
                self.ops.push(LedgerOp::OpenObstacle { fact_id: fact.id });
            }
        }
    }

    /// Writes one slot. A value equal to the live one writes no new fact: the
    /// holders witness the existing fact, and a Possible one is Established.
    /// A changed value supersedes the live fact and clears the obstacles on
    /// that slot.
    fn write(
        &mut self,
        draft: &SlotDraft,
        source: FactSource,
        seen_by: Vec<EntityRef>,
        causes: Vec<u64>,
    ) {
        let text = self.text(draft);
        self.write_text(draft, text, source, seen_by, causes);
    }

    /// `write` with a text the core built itself.
    fn write_text(
        &mut self,
        draft: &SlotDraft,
        text: String,
        source: FactSource,
        seen_by: Vec<EntityRef>,
        causes: Vec<u64>,
    ) {
        let new = FactDraft {
            slot: slot(draft),
            text,
            canon: Canon::Established,
            source,
            seen_by,
            causes,
        };
        let Handle::Entity(subject) = draft.subject else {
            self.ops.push(add_fact(new));
            return;
        };
        let value = normalize::value(&draft.value);
        let same_possible = self
            .view
            .live_facts
            .iter()
            .find(|f| {
                f.slot.subject == subject
                    && f.slot.key == normalize::key(&draft.key)
                    && f.canon == Canon::Possible
                    && f.slot.value == value
            })
            .cloned();
        match self.current(subject, &draft.key).cloned() {
            Some(live) if live.slot.value == value => self.witness(live.id, &new.seen_by),
            Some(live) => {
                self.clear_obstacles(subject, &draft.key);
                self.ops.push(LedgerOp::SupersedeFact { old: live.id, new });
            }
            None => match same_possible {
                Some(possible) if self.promote(&possible) => {
                    self.witness(possible.id, &new.seen_by)
                }
                _ => {
                    self.clear_obstacles(subject, &draft.key);
                    self.ops.push(add_fact(new));
                }
            },
        }
    }

    fn witness(&mut self, fact_id: u64, holders: &[EntityRef]) {
        for holder in holders {
            self.ops.push(LedgerOp::Know {
                holder: *holder,
                fact_id: Some(fact_id),
                claim_id: None,
                how: Knowing::Witnessed,
                arrives_at: self.view.clock_min,
            });
        }
    }

    fn clear_obstacles(&mut self, subject: u64, key: &str) {
        let key = normalize::key(key);
        for o in &self.view.obstacles_open {
            let on_slot = self
                .view
                .fact(Handle::Fact(o.fact_id))
                .is_some_and(|f| f.slot.subject == subject && f.slot.key == key);
            if on_slot && self.cleared.insert(o.id) {
                self.ops.push(LedgerOp::ClearObstacle { obstacle_id: o.id });
            }
        }
    }

    /// `Seen::Pc`: the PC. `Seen::Present`: every present holder plus the new
    /// NPCs, who stand at the PC's location. `Seen::Gm`: nobody.
    fn holders(&self, seen: Seen) -> Vec<EntityRef> {
        match seen {
            Seen::Gm => Vec::new(),
            Seen::Pc => pc(self.view).map(EntityRef::Id).into_iter().collect(),
            Seen::Present => {
                // Factions hear and remember (holders), but are not persons, so boundary.rs skips them.
                let present = self.view.present();
                self.view
                    .entities
                    .iter()
                    .filter(|e| {
                        present.contains(&e.id)
                            && matches!(
                                e.kind,
                                EntityKind::Pc | EntityKind::Npc | EntityKind::Faction
                            )
                    })
                    .map(|e| EntityRef::Id(e.id))
                    .chain(
                        self.p
                            .new_npcs
                            .iter()
                            .map(|n| EntityRef::New(new_local(n.id))),
                    )
                    .collect()
            }
        }
    }

    fn text(&self, draft: &SlotDraft) -> String {
        let name = match draft.subject {
            Handle::Entity(_) => self.view.entity(draft.subject).map(|e| e.name.clone()),
            Handle::NewNpc(_) => self
                .p
                .new_npcs
                .iter()
                .find(|n| n.id == draft.subject)
                .map(|n| n.name.clone()),
            _ => None,
        }
        .unwrap_or_else(|| draft.subject.to_string());
        crate::domain::slot_sentence(&name, &draft.key, &draft.value)
    }
}

fn add_fact(new: FactDraft) -> LedgerOp {
    LedgerOp::AddFact {
        slot: new.slot,
        text: new.text,
        canon: new.canon,
        source: new.source,
        seen_by: new.seen_by,
        causes: new.causes,
    }
}

fn writes_slot(op: &LedgerOp, slot: &Slot<u64>) -> bool {
    let target = match op {
        LedgerOp::AddFact { slot, .. } => slot,
        LedgerOp::SupersedeFact { new, .. } => &new.slot,
        _ => return false,
    };
    target.subject == EntityRef::Id(slot.subject) && normalize::key(&target.key) == slot.key
}

fn new_local(handle: Handle) -> u8 {
    match handle {
        Handle::NewNpc(n) => u8::try_from(n).expect("validate rejects N handles above 255"),
        other => panic!("{other} is not an N handle; validate rejects it"),
    }
}

fn entity_ref(handle: Handle) -> EntityRef {
    match handle {
        Handle::Entity(id) => EntityRef::Id(id),
        Handle::NewNpc(_) => EntityRef::New(new_local(handle)),
        other => panic!("{other} is not an entity handle; validate rejects it"),
    }
}

fn fact_id(handle: Handle) -> Option<u64> {
    match handle {
        Handle::Fact(id) => Some(id),
        _ => None,
    }
}

fn slot(draft: &SlotDraft) -> Slot {
    Slot {
        subject: entity_ref(draft.subject),
        key: normalize::key(&draft.key),
        value: normalize::value(&draft.value),
    }
}
