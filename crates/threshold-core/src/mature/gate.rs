//! The adults-only rule: who is cleared for an intimate Turn, the Commit
//! check of an intimate proposal (G2), and the same rule over who stands at
//! the PC's place once the Turn's ops are committed (G3). It reads the world
//! only: stored ages, Age Confirmations and who stands at the PC's place. No prompt and no
//! model output reach it, and it names nothing outside the core's world.
//! A source test in `tests/mature.rs` pins that.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::{
    checks::{CheckKind, Rejection},
    context::seen_present,
    domain::{
        Cancel, Canon, Cause, CheckDraft, ClockMin, DecisionProposal, DetailLevel, Effect,
        EntityId, EntityKind, EntityRef, EventDraft, FactSource, Factor, Handle, Knowing, Leave,
        LedgerOp, NO_ITEM, NewFact, NewNpc, OPEN_KEY_PREFIX, Reaction, Settle, Slot, SlotDraft,
        Stance, Trigger, is_adult, is_state_key,
    },
    normalize,
    store::{Claim, Entity, Fact, Knowledge, WorldView},
    world::pc_knowledge,
};

/// The proof that every character of a place is cleared. Only `clearance`
/// makes one: the field is private to this file. It is neither `Clone` nor
/// `Copy` and is never stored, so a later step must ask `clearance` again.
#[derive(Debug)]
pub struct Cleared(());

/// The characters who are not cleared, sorted, each once. For the debug log
/// and the core's own rules: a Player screen never shows it raw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blocked {
    pub who: Vec<EntityId>,
}

/// Clears the characters at the PC's place as `checked_here` gives them
/// (world truth, hidden ones included, and everyone the PC believes is
/// there), plus the ids in `also`.
///
/// A character is cleared when all of these hold:
/// - its kind is PC or NPC;
/// - its age is confirmed through the core (`Entity::age_confirmed`);
/// - its stored age is `domain::ADULT_AGE` or more;
/// - no PC or NPC that fails one of the three rules above shares one of its
///   names, main or other, through `normalize::name`. Nothing stops a new
///   NPC from taking the name of a minor of the roster.
///
/// An id of `also` that names no PC or NPC is not cleared either. The
/// Adventure Seed's `min_age` and the Detail Level change nothing here.
pub fn clearance(view: &WorldView, also: &[EntityId]) -> Result<Cleared, Blocked> {
    let failing: HashSet<String> = view
        .entities
        .iter()
        .filter(|e| is_character(e) && !holds_alone(e))
        .flat_map(names)
        .collect();
    let cleared = |id: EntityId| {
        view.entities.iter().find(|e| e.id == id).is_some_and(|e| {
            is_character(e) && holds_alone(e) && names(e).all(|n| !failing.contains(&n))
        })
    };
    let characters_here = checked_here(view)
        .into_iter()
        .filter(|id| view.entities.iter().any(|e| e.id == *id && is_character(e)));
    let who: BTreeSet<EntityId> = characters_here
        .chain(also.iter().copied())
        .filter(|id| !cleared(*id))
        .collect();
    if who.is_empty() {
        Ok(Cleared(()))
    } else {
        Err(Blocked {
            who: who.into_iter().collect(),
        })
    }
}

/// Everyone the gate checks at the PC's place (ADR 0008, changed after
/// review): the union of
/// - world truth at the PC's place (`WorldView::present`, hidden
///   characters included);
/// - everyone the PC believes is at the PC's place, through the very
///   function the narrator's `Present` line reads (`context::seen_present`),
///   so a stale Belief or a Claim counts: the narrator would tell that
///   character as here;
/// - each character whose live location, or whose location as the PC
///   believes it, is the PC's place by the gate's own place rule
///   (`same_place`), and each character with no live location: unknown is
///   not cleared. When the PC's own place is unknown, every character;
/// - the PC.
///
/// The narrator's `Present` line is a subset of this set by construction;
/// the test `the_narrator_present_line_is_a_subset_of_the_checked_set`
/// pins it. Things and places are in the set too; `clearance` checks the
/// characters among them. The place rule here is for the gate only: play
/// keeps `normalize::value` equality.
pub fn checked_here(view: &WorldView) -> BTreeSet<EntityId> {
    let mut here: BTreeSet<EntityId> = view.present().into_iter().collect();
    here.extend(seen_present(view));
    let pc = view.entities.iter().find(|e| e.kind == EntityKind::Pc);
    here.extend(pc.map(|pc| pc.id));
    let pc_place = pc
        .and_then(|pc| view.live_fact(pc.id, "location"))
        .map(|f| f.slot.value.as_str())
        .unwrap_or_default();
    let at_pc_place = |value: &str| same_place(view, pc_place, value);
    here.extend(
        view.entities
            .iter()
            .filter(|e| is_character(e))
            .filter(|e| {
                view.live_fact(e.id, "location")
                    .is_none_or(|f| at_pc_place(&f.slot.value))
            })
            .map(|e| e.id),
    );
    here.extend(
        pc_knowledge(view)
            .into_iter()
            .filter(|b| b.key == "location" && at_pc_place(&b.value))
            .map(|b| b.subject)
            .filter(|id| view.entities.iter().any(|e| e.id == *id && is_character(e))),
    );
    here
}

/// The gate's place rule (review M1): two location values name one place
/// when, after the strongest normal form the core has (`place_words`:
/// case, accents, punctuation, a leading article), their words are equal,
/// or one is a word-prefix of the other ("gate" and "gate square, by the
/// well"), or both name one Place entity through any of its names. A value
/// with no words is unknown and counts as the same place: the gate fails
/// closed.
fn same_place(view: &WorldView, a: &str, b: &str) -> bool {
    let (wa, wb) = (place_words(a), place_words(b));
    if wa.is_empty() || wb.is_empty() || wa.starts_with(&wb) || wb.starts_with(&wa) {
        return true;
    }
    let places = |value: &str| -> HashSet<EntityId> {
        view.entities_by_name(value)
            .filter(|e| e.kind == EntityKind::Place)
            .map(|e| e.id)
            .collect()
    };
    !places(a).is_disjoint(&places(b))
}

/// The words of a location value for `same_place`: `normalize::name_words`
/// (lower case, no accents, split on every character that is not a letter
/// or a digit), with a leading article left out.
fn place_words(value: &str) -> Vec<String> {
    let mut words = normalize::name_words(value);
    if words.len() > 1 && ["the", "a", "an"].contains(&words[0].as_str()) {
        words.remove(0);
    }
    words
}

/// Who is at the PC's place once the ops of one Turn are committed (data
/// shape 3.4): `truth` is world truth, hidden characters included, as
/// `WorldView::present` will read it; `seen` the characters the PC knows
/// are there, as `context::seen_present` will read it; `checked` the set
/// the gate checks, as `checked_here` will read it; `pc_place` the PC's
/// location value, in normal form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Presence {
    pub truth: BTreeSet<EntityId>,
    pub seen: BTreeSet<EntityId>,
    pub checked: BTreeSet<EntityId>,
    pub pc_place: Option<String>,
}

/// The presence after `ops`, the ops of one Turn before its Commit (the
/// proposal's, the World Clock's and its due World Events', perception on
/// arrival), on the state `view` before the Turn. It replays, in memory,
/// what the store's projection does with the ops that decide a place and
/// who knows it: new entities, location facts and their supersessions, the
/// witnesses of a fact, `Know` rows, Claims, the World Clock and the
/// promotion of a witnessed Possible fact. Then it reads the result with
/// the same two functions the next snapshot is read with
/// (`WorldView::present`, `context::seen_present`). The oracle test
/// `presence_after_matches_the_next_snapshot` pins the two together.
///
/// A new entity gets the next id above the view's: the id the store gives
/// it in a file where no Turn was reverted. A wrong guess only blocks:
/// `clearance` never clears an id that the view does not hold.
pub fn after(view: &WorldView, ops: &[LedgerOp]) -> Presence {
    let mut sim = Replay::new(view);
    for op in ops {
        sim.apply(op);
    }
    let world = sim.world;
    let pc_place = world
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Pc)
        .and_then(|pc| world.live_fact(pc.id, "location"))
        .map(|f| normalize::value(&f.slot.value));
    Presence {
        truth: world.present().into_iter().collect(),
        seen: seen_present(&world).into_iter().collect(),
        checked: checked_here(&world),
        pc_place,
    }
}

/// G3: for an intimate proposal, `clearance` over every character the gate
/// checks at the PC's place once the Turn's ops are committed
/// (`after.checked`: world truth, so an arrival the PC does not see counts,
/// and everyone the PC then believes is here, so a Claim of this Turn
/// counts), on top of the place before the Turn that `clearance` always
/// covers. G2 read the place before the Turn; both must pass. Any other
/// proposal passes.
///
/// A thing or a place in `after.checked` is no character and blocks nothing,
/// as before the Turn. An id the view does not hold (an entity this Turn
/// creates) stays in, and `clearance` blocks it.
pub fn check_after(
    p: &DecisionProposal,
    view: &WorldView,
    after: &Presence,
) -> Result<(), Blocked> {
    if !p.intimate {
        return Ok(());
    }
    let here: Vec<EntityId> = after
        .checked
        .iter()
        .copied()
        .filter(|id| {
            view.entities
                .iter()
                .find(|e| e.id == *id)
                .is_none_or(is_character)
        })
        .collect();
    clearance(view, &here).map(|_| ())
}

/// The in-memory replay of one Turn's ops for `after`. It mirrors
/// `store::project::apply` and `Store::commit` for the ops that change a
/// place or what the PC knows of one; every other op changes nothing here.
struct Replay {
    world: WorldView,
    pc: Option<EntityId>,
    locals: HashMap<u8, EntityId>,
    next_entity: EntityId,
    next_fact: u64,
    next_claim: u64,
}

impl Replay {
    fn new(view: &WorldView) -> Self {
        Self {
            world: view.clone(),
            pc: view
                .entities
                .iter()
                .find(|e| e.kind == EntityKind::Pc)
                .map(|e| e.id),
            locals: HashMap::new(),
            next_entity: view.entities.iter().map(|e| e.id).max().unwrap_or(0),
            next_fact: view.facts.iter().map(|f| f.id).max().unwrap_or(0),
            next_claim: view.claims.iter().map(|c| c.id).max().unwrap_or(0),
        }
    }

    fn apply(&mut self, op: &LedgerOp) {
        if let LedgerOp::AddEntity {
            local: Some(local), ..
        } = op
        {
            self.locals.insert(*local, self.next_entity + 1);
        }
        let locals = &self.locals;
        let Ok(op) = op.clone().try_map(&mut |r| match r {
            EntityRef::Id(id) => Ok(id),
            EntityRef::New(n) => locals.get(&n).copied().ok_or(()),
        }) else {
            return;
        };
        match op {
            LedgerOp::AddEntity {
                kind,
                name,
                age,
                other_names,
                age_confirmed,
                ..
            } => {
                self.next_entity += 1;
                self.world.entities.push(Entity {
                    id: self.next_entity,
                    kind,
                    norm_name: normalize::name(&name),
                    name,
                    age,
                    other_names,
                    age_confirmed,
                });
            }
            LedgerOp::AddFact {
                slot,
                text,
                canon,
                source,
                seen_by,
                ..
            } => self.add_fact(slot, text, canon, source, seen_by),
            LedgerOp::SupersedeFact { old, new } => {
                let replacement = self.next_fact + 1;
                self.update_fact(old, |f| f.superseded_by = Some(replacement));
                self.world.live_facts.retain(|f| f.id != old);
                self.add_fact(new.slot, new.text, new.canon, new.source, new.seen_by);
            }
            LedgerOp::EstablishFact { fact_id } => self.update_fact(fact_id, |f| {
                if f.canon == Canon::Possible && f.superseded_by.is_none() {
                    f.canon = Canon::Established;
                }
            }),
            LedgerOp::Know {
                holder,
                fact_id,
                claim_id,
                how,
                arrives_at,
            } => self.know(holder, fact_id, claim_id, how, arrives_at),
            LedgerOp::AddClaim {
                claim,
                listeners,
                delay_min,
            } => {
                self.next_claim += 1;
                let claim_id = self.next_claim;
                self.world.claims.push(Claim {
                    id: claim_id,
                    claim,
                    turn_id: 0,
                });
                let arrives_at = self
                    .world
                    .clock_min
                    .saturating_add(ClockMin::from(delay_min));
                let mut unique = HashSet::new();
                for holder in listeners.into_iter().filter(|h| unique.insert(*h)) {
                    self.know(holder, None, Some(claim_id), Knowing::Told, arrives_at);
                }
            }
            LedgerOp::AdvanceClock { minutes } => {
                let before = self.world.clock_min;
                self.world.clock_min = before.saturating_add(ClockMin::from(minutes));
                self.promote_seen(Some(before), None);
            }
            _ => {}
        }
    }

    /// `apply` of `AddFact`, then the `Know` row of each witness (the rows
    /// `Store::commit` adds for `seen_by`).
    fn add_fact(
        &mut self,
        slot: Slot<EntityId>,
        text: String,
        canon: Canon,
        source: FactSource,
        seen_by: Vec<EntityId>,
    ) {
        let key = normalize::key(&slot.key);
        let pc_seen = self.pc.is_some_and(|pc| seen_by.contains(&pc));
        let mut canon = if matches!(
            source,
            FactSource::Proposal | FactSource::Stake | FactSource::Correction
        ) && canon != Canon::Possible
        {
            if pc_seen {
                Canon::Persistent
            } else {
                Canon::Established
            }
        } else {
            canon
        };
        let open = format!("{OPEN_KEY_PREFIX}{key}");
        let outcome_slot = self
            .world
            .facts
            .iter()
            .any(|f| f.slot.subject == slot.subject && f.slot.key == open);
        if canon == Canon::Persistent && (is_state_key(&key) || outcome_slot) {
            canon = Canon::Established;
        }
        self.next_fact += 1;
        let id = self.next_fact;
        let mut holders = Vec::new();
        for holder in seen_by {
            if !holders.contains(&holder) {
                holders.push(holder);
            }
        }
        let fact = Fact {
            id,
            slot: Slot {
                subject: slot.subject,
                value: normalize::value(&slot.value),
                key,
            },
            text,
            canon,
            source,
            created_turn: 0,
            superseded_by: None,
            seen_by: holders.clone(),
        };
        // A canon write retires a live departure record on its slot.
        if canon != Canon::Possible && fact.slot.key == "location" {
            let retired: Vec<u64> = self
                .world
                .live_facts
                .iter()
                .filter(|f| {
                    f.slot.subject == fact.slot.subject
                        && f.slot.key == "location"
                        && f.slot.value.is_empty()
                        && f.canon == Canon::Possible
                })
                .map(|f| f.id)
                .collect();
            for old in retired {
                self.update_fact(old, |f| f.superseded_by = Some(id));
                self.world.live_facts.retain(|f| f.id != old);
            }
        }
        self.world.facts.push(fact.clone());
        self.world.live_facts.push(fact);
        let clock = self.world.clock_min;
        for holder in holders {
            self.know(holder, Some(id), None, Knowing::Witnessed, clock);
        }
    }

    /// `apply` of `Know`: one row, learned now; a PC witness that has
    /// arrived promotes the fact.
    fn know(
        &mut self,
        holder: EntityId,
        fact_id: Option<u64>,
        claim_id: Option<u64>,
        how: Knowing,
        arrives_at: ClockMin,
    ) {
        self.world.knowledge.push(Knowledge {
            holder,
            fact_id,
            claim_id,
            how,
            learned_at: self.world.clock_min,
            arrives_at,
            turn: 0,
        });
        if how == Knowing::Witnessed
            && fact_id.is_some()
            && Some(holder) == self.pc
            && arrives_at <= self.world.clock_min
        {
            self.promote_seen(None, fact_id);
        }
    }

    /// The store's `promote_seen`: a live Possible fact that the PC
    /// witnessed, with an arrived row (after `after`, when given), becomes
    /// Established when it is the only such fact on its slot and no live
    /// canon fact holds the slot.
    fn promote_seen(&mut self, after: Option<ClockMin>, fact_id: Option<u64>) {
        let Some(pc) = self.pc else {
            return;
        };
        let clock = self.world.clock_min;
        let witnessed: HashSet<u64> = self
            .world
            .knowledge
            .iter()
            .filter(|k| k.holder == pc && k.how == Knowing::Witnessed)
            .filter(|k| k.arrives_at <= clock && after.is_none_or(|a| k.arrives_at > a))
            .filter_map(|k| k.fact_id)
            .filter(|id| fact_id.is_none_or(|f| f == *id))
            .collect();
        let candidates: Vec<(u64, EntityId, String)> = self
            .world
            .live_facts
            .iter()
            .filter(|f| f.canon == Canon::Possible && witnessed.contains(&f.id))
            .map(|f| (f.id, f.slot.subject, f.slot.key.clone()))
            .collect();
        for (id, subject, key) in &candidates {
            let alone = candidates
                .iter()
                .filter(|(_, s, k)| s == subject && k == key)
                .count()
                == 1;
            let held = self.world.live_facts.iter().any(|f| {
                f.slot.subject == *subject && f.slot.key == *key && f.canon != Canon::Possible
            });
            if alone && !held {
                self.update_fact(*id, |f| f.canon = Canon::Established);
            }
        }
    }

    /// Changes the fact `id` in both lists of the view.
    fn update_fact(&mut self, id: u64, change: impl Fn(&mut Fact)) {
        for list in [&mut self.world.facts, &mut self.world.live_facts] {
            if let Some(f) = list.iter_mut().find(|f| f.id == id) {
                change(f);
            }
        }
    }
}

/// G2: the rules of an intimate proposal, in order (data shape 3.3). A
/// proposal without `intimate` passes untouched.
///
/// 1. At Detail Level `excluded`: one `DetailExcluded` rejection, and
///    nothing else is checked.
/// 2. Every handle the proposal names, anywhere (`named`): a character it
///    reaches, directly or through a fact, a World Event or a goal, must be
///    cleared, here or not. A new NPC (an `N` handle) has no Age
///    Confirmation, and a handle that reaches nothing the world holds is
///    not cleared either: one `AdultsOnly` rejection each, with its handle
///    (review B2).
/// 3. `clearance` over the place (`checked_here`) and the characters of
///    rule 2: one `AdultsOnly` rejection per blocked character, with its
///    handle.
/// 4. No reaction, or a reacting NPC below comply: `Unwilling`. Not final:
///    it keeps the one retry.
pub fn check(p: &DecisionProposal, view: &WorldView, errs: &mut Vec<Rejection>) {
    if !p.intimate {
        return;
    }
    if view.detail == DetailLevel::Excluded {
        errs.push(Rejection::new(
            CheckKind::DetailExcluded,
            None,
            "this Campaign's Detail Level excludes an intimate Turn",
        ));
        return;
    }
    let mut also: Vec<EntityId> = Vec::new();
    let mut refused: Vec<Option<Handle>> = Vec::new();
    for (handle, field) in named(p) {
        let reach = handle.map_or(Reach::Nothing, |h| reach(h, view));
        let reason = match reach {
            Reach::Characters(ids) => {
                also.extend(ids);
                continue;
            }
            Reach::NewNpc => "a new NPC cannot be part of an intimate Turn".to_owned(),
            Reach::Nothing => {
                format!("{field} names nothing the world holds, so nobody can be cleared")
            }
        };
        if !refused.contains(&handle) {
            refused.push(handle);
            errs.push(Rejection::new(CheckKind::AdultsOnly, handle, reason));
        }
    }
    if let Err(blocked) = clearance(view, &also) {
        for id in blocked.who {
            errs.push(Rejection::new(
                CheckKind::AdultsOnly,
                Some(Handle::Entity(id)),
                "an intimate Turn needs every character here, and every character it names, to be a confirmed adult",
            ));
        }
    }
    if p.reactions.is_empty() {
        errs.push(Rejection::new(
            CheckKind::Unwilling,
            None,
            "an intimate Turn needs at least one NPC who reacts with comply or help",
        ));
    }
    for r in p.reactions.iter().filter(|r| r.stance < Stance::Comply) {
        errs.push(Rejection::new(
            CheckKind::Unwilling,
            Some(r.npc),
            "in an intimate Turn each reacting NPC complies or helps",
        ));
    }
}

/// What one named handle reaches, for the gate.
enum Reach {
    /// The characters it names (none for a place or a thing).
    Characters(Vec<EntityId>),
    /// A new NPC of this proposal: never cleared.
    NewNpc,
    /// Nothing the world holds, or a text the core cannot read: not
    /// cleared, so the gate fails closed.
    Nothing,
}

/// Every handle a proposal names that can reach a character, with its
/// field, in field order (review B2). `None` is a reference the core cannot
/// read (a `settles.item` that is neither `none` nor a handle and a key).
///
/// The proposal and each of its parts are destructured with no `..`: a new
/// field does not compile until its author decides whether it names a
/// character. The test `every_proposal_field_is_sorted_for_the_gate`
/// pins the same list against the proposal's JSON schema. Fields that name
/// a place only (`leaves.to`, `schedule.moves_actor_to`) are left to G3,
/// which reads who stands at the PC's place after the Turn's ops.
fn named(p: &DecisionProposal) -> Vec<(Option<Handle>, &'static str)> {
    let DecisionProposal {
        action: _,
        contested: _,
        verdict: _,
        move_to,
        leaves,
        settles,
        blockers,
        check,
        reactions,
        new_npcs,
        facts,
        cancel,
        schedule,
        minutes: _,
        intimate: _,
        facts_on_win: _,
        offered: _,
    } = p;
    let mut out: Vec<(Option<Handle>, &'static str)> = Vec::new();
    let mut add = |handle: Handle, field: &'static str| out.push((Some(handle), field));
    if let Some(place) = move_to {
        add(*place, "move_to");
    }
    for Leave { npc, to: _ } in leaves {
        add(*npc, "leaves.npc");
    }
    for blocker in blockers {
        add(*blocker, "blockers");
    }
    if let Some(CheckDraft {
        against,
        win,
        lose,
        factors,
    }) = check
    {
        if let Some(against) = against {
            add(*against, "check.against");
        }
        for (stake, field) in [(win, "check.win.subject"), (lose, "check.lose.subject")] {
            if let Some(SlotDraft {
                subject,
                key: _,
                value: _,
            }) = stake
            {
                add(*subject, field);
            }
        }
        for Factor { fact, helps: _ } in factors {
            add(*fact, "check.factors.fact");
        }
    }
    for Reaction {
        npc,
        stance: _,
        plan: _,
        cause,
    } in reactions
    {
        add(*npc, "reactions.npc");
        match cause {
            Some(Cause::Fact(fact)) => add(*fact, "reactions.cause"),
            Some(Cause::Check) | None => {}
        }
    }
    for NewNpc {
        id,
        name: _,
        age: _,
        voice: _,
    } in new_npcs
    {
        add(*id, "new_npcs.id");
    }
    for NewFact {
        slot: SlotDraft {
            subject,
            key: _,
            value: _,
        },
        seen: _,
        said_by,
        because,
    } in facts
    {
        add(*subject, "facts.slot.subject");
        if let Some(speaker) = said_by {
            add(*speaker, "facts.said_by");
        }
        for cause in because {
            add(*cause, "facts.because");
        }
    }
    for Cancel { event, because } in cancel {
        add(*event, "cancel.event");
        add(*because, "cancel.because");
    }
    if let Some(EventDraft {
        in_minutes: _,
        actor,
        goal,
        text: _,
        effects,
        seen_by,
        public: _,
        moves_actor_to: _,
    }) = schedule
    {
        add(*actor, "schedule.actor");
        if let Some(goal) = goal {
            add(*goal, "schedule.goal");
        }
        for SlotDraft {
            subject,
            key: _,
            value: _,
        } in effects
        {
            add(*subject, "schedule.effects.subject");
        }
        for witness in seen_by {
            add(*witness, "schedule.seen_by");
        }
    }
    if let Some(Settle { item, outcome: _ }) = settles
        && item.trim() != NO_ITEM
    {
        let handle = item
            .split_whitespace()
            .next()
            .and_then(|first| Handle::parse(first).ok());
        out.push((handle, "settles.item"));
    }
    out
}

/// The characters one handle reaches: an entity itself; the subject of a
/// fact; every entity a scheduled World Event names (actors, witnesses,
/// a `When` subject, its effects); the NPC and the target of a goal.
fn reach(handle: Handle, view: &WorldView) -> Reach {
    let mut ids: Vec<EntityId> = Vec::new();
    match handle {
        Handle::NewNpc(_) => return Reach::NewNpc,
        Handle::Entity(id) => ids.push(id),
        Handle::Fact(_) => match view.fact(handle) {
            Some(fact) => ids.push(fact.slot.subject),
            None => return Reach::Nothing,
        },
        Handle::WorldEvent(id) => match view.events_scheduled.iter().find(|w| w.id == id) {
            Some(event) => {
                ids.extend(&event.actors);
                ids.extend(&event.seen_by);
                if let Trigger::When(slot) = &event.trigger {
                    ids.push(slot.subject);
                }
                for effect in &event.effects {
                    match effect {
                        Effect::SetFact { slot, seen_by, .. } => {
                            ids.push(slot.subject);
                            ids.extend(seen_by);
                        }
                        Effect::ShiftAttitude { from, to, .. } => ids.extend([*from, *to]),
                        Effect::Tell { to, claim, .. } => {
                            ids.extend([*to, claim.speaker, claim.slot.subject]);
                        }
                    }
                }
            }
            None => return Reach::Nothing,
        },
        Handle::Goal(id) => match view.goals_active.iter().find(|g| g.id == id) {
            Some(goal) => ids.extend(std::iter::once(goal.npc_id).chain(goal.against_id)),
            None => return Reach::Nothing,
        },
    }
    let mut characters = Vec::new();
    for id in ids {
        match view.entities.iter().find(|e| e.id == id) {
            Some(e) if is_character(e) => characters.push(id),
            Some(_) => {}
            None => return Reach::Nothing,
        }
    }
    Reach::Characters(characters)
}

fn is_character(e: &Entity) -> bool {
    matches!(e.kind, EntityKind::Pc | EntityKind::Npc)
}

/// The first three rules of a cleared character: confirmed, with an adult
/// stored age. The kind is checked apart.
fn holds_alone(e: &Entity) -> bool {
    e.age_confirmed && is_adult(e.age)
}

/// Every name of `e`, main and other, in normal form.
fn names(e: &Entity) -> impl Iterator<Item = String> + '_ {
    std::iter::once(normalize::name(&e.name))
        .chain(e.other_names.iter().map(|n| normalize::name(n)))
}
