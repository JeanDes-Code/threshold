//! Repairs the core makes to a parsed proposal before the Commit checks
//! (rules changed after QA, 2026-11-10, and ADR 0007). Each repair only
//! removes model output about the PC or respells a place, so it never adds a
//! voluntary act of the PC: the Agency rule holds. Every repair returns one
//! note for the debug log.

use super::{closed, is_involuntary_key, pc, references::is_known_place, same_key};
use crate::{
    domain::{ActionKind, DecisionProposal, EntityKind, Handle, NO_ITEM, SlotDraft, Verdict},
    mature::SCENE_CLOSE_MAX_MIN,
    normalize,
    store::WorldView,
    world,
};

/// The proposal after the repairs, and one note per repair.
pub fn settle(mut p: DecisionProposal, view: &WorldView) -> (DecisionProposal, Vec<String>) {
    let mut notes = Vec::new();
    drop_age_keys(&mut p, &mut notes);
    if let Some(pc) = pc(view).map(Handle::Entity) {
        drop_pc_acts(&mut p, pc, &mut notes);
        drop_pc_location(&mut p, pc, &mut notes);
        drop_pc_stakes(&mut p, pc, &mut notes);
        drop_pc_blockers(&mut p, view, pc, &mut notes);
    }
    if let Some(place) = p.move_to.filter(|h| !is_known_place(view, *h)) {
        p.move_to = None;
        notes.push(format!("removed move_to {place}: not a place the PC knows"));
    }
    if let Some(place) = p.move_to.filter(|_| p.action != ActionKind::Move) {
        p.move_to = None;
        notes.push(format!(
            "removed move_to {place}: the action is {:?}, not move",
            p.action
        ));
    }
    settle_closes(&mut p, view, &mut notes);
    settle_leaves(&mut p, view, &mut notes);
    spell_places(&mut p, view, &mut notes);
    (p, notes)
}

/// The repairs of a Closing Turn's proposal (mature content data shape 3.7
/// step 3): the pins of `closing_schema_for` first, then `settle`. Each pin
/// only takes model output away or fixes a value the core owns, so it adds
/// no PC act. `minutes` is clamped to 1 to `mature::SCENE_CLOSE_MAX_MIN`.
/// A Closing Turn moves nobody (review minor, 2026-11-12): `move_to`,
/// `leaves`, the scheduled event and new NPCs go, and so does every fact on
/// the key `location`, on any subject. One note per change.
pub fn settle_closing(
    mut p: DecisionProposal,
    view: &WorldView,
) -> (DecisionProposal, Vec<String>) {
    let mut notes = Vec::new();
    let mut note = |what: &str| notes.push(format!("closing: {what}"));
    if p.action != ActionKind::Wait {
        note(&format!("action {:?} set to wait", p.action));
        p.action = ActionKind::Wait;
    }
    if p.verdict != Verdict::Allowed {
        note(&format!("verdict {:?} set to allowed", p.verdict));
        p.verdict = Verdict::Allowed;
    }
    if p.contested {
        note("contested set to false");
        p.contested = false;
    }
    if p.intimate {
        note("intimate set to false");
        p.intimate = false;
    }
    if let Some(place) = p.move_to.take() {
        note(&format!("removed move_to {place}"));
    }
    if p.check.take().is_some() {
        note("removed the check");
    }
    if p.settles.take().is_some() {
        note("removed settles");
    }
    if p.schedule.take().is_some() {
        note("removed the scheduled event");
    }
    for (field, len) in [
        ("new_npcs", std::mem::take(&mut p.new_npcs).len()),
        ("cancel", std::mem::take(&mut p.cancel).len()),
        ("leaves", std::mem::take(&mut p.leaves).len()),
        ("blockers", std::mem::take(&mut p.blockers).len()),
    ] {
        if len > 0 {
            note(&format!("removed {len} {field}"));
        }
    }
    for (field, len, max) in [
        ("reactions", p.reactions.len(), CLOSING_MAX_REACTIONS),
        ("facts", p.facts.len(), CLOSING_MAX_FACTS),
    ] {
        if len > max {
            note(&format!("kept the first {max} of {len} {field}"));
        }
    }
    p.facts.retain(|f| {
        let moves = super::same_key(&f.slot.key, "location");
        if moves {
            note(&format!("removed the location fact on {}", f.slot.subject));
        }
        !moves
    });
    p.reactions.truncate(CLOSING_MAX_REACTIONS);
    p.facts.truncate(CLOSING_MAX_FACTS);
    let minutes = p.minutes.clamp(1, SCENE_CLOSE_MAX_MIN);
    if minutes != p.minutes {
        note(&format!("minutes {} set to {minutes}", p.minutes));
        p.minutes = minutes;
    }
    let (p, more) = settle(p, view);
    notes.extend(more);
    (p, notes)
}

/// The most reactions and facts a Closing Turn's proposal keeps, as
/// `closing_schema_for` says.
const CLOSING_MAX_REACTIONS: usize = 2;
const CLOSING_MAX_FACTS: usize = 1;

/// The key `age` is reserved (mature content module, data shape 3.3): the
/// adults-only rule reads the stored age of an entity only, never a fact,
/// and a fact that states another age would only mislead the narrator. So
/// each proposal fact, Check stake and scheduled effect on `age`, on any
/// subject, goes, with one note each. A removal only takes model output
/// away.
fn drop_age_keys(p: &mut DecisionProposal, notes: &mut Vec<String>) {
    let is_age = |slot: &SlotDraft| same_key(&slot.key, AGE_KEY);
    p.facts.retain(|f| {
        let age = is_age(&f.slot);
        if age {
            notes.push(format!(
                "removed the fact on key age of {}: the core reads the stored age only",
                f.slot.subject
            ));
        }
        !age
    });
    if let Some(c) = p.check.as_mut() {
        for (name, stake) in [("win", &mut c.win), ("lose", &mut c.lose)] {
            if let Some(slot) = stake.as_ref().filter(|slot| is_age(slot)) {
                notes.push(format!(
                    "removed the {name} stake on key age of {}: the core reads the stored age only",
                    slot.subject
                ));
                *stake = None;
            }
        }
    }
    if let Some(s) = p.schedule.as_mut() {
        s.effects.retain(|slot| {
            let age = is_age(slot);
            if age {
                notes.push(format!(
                    "removed the scheduled effect on key age of {}: the core reads the stored age only",
                    slot.subject
                ));
            }
            !age
        });
    }
}

/// The reserved key of `drop_age_keys`.
const AGE_KEY: &str = "age";

/// Three shapes that put the PC where only NPCs belong: a reaction of the
/// PC, a scheduled event whose actor is the PC, and the PC among an event's
/// witnesses. The element goes; the rest of the proposal stays.
fn drop_pc_acts(p: &mut DecisionProposal, pc: Handle, notes: &mut Vec<String>) {
    let before = p.reactions.len();
    p.reactions.retain(|r| r.npc != pc);
    if p.reactions.len() < before {
        notes.push(format!("removed the reaction of the PC ({pc})"));
    }
    if p.schedule.as_ref().is_some_and(|s| s.actor == pc) {
        p.schedule = None;
        notes.push(format!(
            "removed the scheduled event whose actor is the PC ({pc})"
        ));
    }
    if let Some(s) = p.schedule.as_mut()
        && s.seen_by.contains(&pc)
    {
        s.seen_by.retain(|h| *h != pc);
        notes.push(format!(
            "removed the PC ({pc}) from the scheduled event's seen_by"
        ));
    }
}

/// A `settles` that names an item the PC does not hold goes, and so does
/// one that names an item this Turn did not offer (review M1: the Intent
/// named no item, the schema had no `settles`, and the model's close still
/// committed). A fact on the outcome slot of the item that closes goes too:
/// the core writes that slot itself (ADR 0007).
fn settle_closes(p: &mut DecisionProposal, view: &WorldView, notes: &mut Vec<String>) {
    let held = world::pc_open_items(view);
    if let Some(s) = p.settles.as_ref()
        && s.item != NO_ITEM
    {
        let why = if !held
            .iter()
            .any(|(subject, key)| world::open_item_ref(*subject, key) == s.item)
        {
            Some("not an open item the PC holds")
        } else if !p.offered.contains(&s.item) {
            Some("not offered this Turn: the Intent does not name it")
        } else {
            None
        };
        if let Some(why) = why {
            notes.push(format!("removed settles {}: {why}", s.item));
            p.settles = None;
        }
    }
    let slots: Vec<(Handle, String)> = closed(p, view)
        .into_iter()
        .map(|close| (Handle::Entity(close.subject), close.key.to_owned()))
        .collect();
    let before = p.facts.len();
    p.facts.retain(|f| {
        f.said_by.is_some()
            || !slots
                .iter()
                .any(|(subject, key)| f.slot.subject == *subject && same_key(&f.slot.key, key))
    });
    if p.facts.len() < before {
        notes.push(
            "removed a fact on the outcome of the item that settles closes: the core writes it"
                .into(),
        );
    }
}

/// `leaves` (bug pass 5, M1): an entry whose NPC the PC does not perceive
/// here, that names the NPC twice, whose `to` is no place or `away`, or
/// that sends the NPC where it already is goes; so does a
/// `moves_actor_to` that is no place. A fact on the `location` of an NPC who
/// leaves goes too: the core writes that slot itself. Each removal only
/// takes model output away.
fn settle_leaves(p: &mut DecisionProposal, view: &WorldView, notes: &mut Vec<String>) {
    let leavers = super::leavers(view);
    let mut listed = Vec::new();
    p.leaves.retain(|leave| {
        let why = match (leave.npc, super::destination(view, &leave.to)) {
            (Handle::Entity(id), _) if !leavers.contains(&id) => {
                Some("not an NPC the PC perceives here")
            }
            (_, None) => Some("not a place handle or away"),
            (npc, _) if listed.contains(&npc) => Some("the NPC is listed twice"),
            (Handle::Entity(id), Some(to)) => {
                let now = view
                    .live_fact(id, "location")
                    .map(|f| normalize::value(&f.slot.value));
                (now.as_deref() == Some(normalize::value(super::location_value(to)).as_str()))
                    .then_some("the NPC is already there")
            }
            _ => Some("not an NPC"),
        };
        match why {
            Some(why) => {
                notes.push(format!(
                    "removed leaves {} to {}: {why}",
                    leave.npc, leave.to
                ));
                false
            }
            None => {
                listed.push(leave.npc);
                true
            }
        }
    });
    if p.leaves.len() > 2 {
        notes.push(format!(
            "removed {} leaves past the cap of 2",
            p.leaves.len() - 2
        ));
        p.leaves.truncate(2);
    }
    if let Some(s) = p.schedule.as_mut()
        && let Some(to) = s.moves_actor_to.clone()
        && super::destination(view, &to).is_none()
    {
        s.moves_actor_to = None;
        notes.push(format!(
            "removed schedule.moves_actor_to {to}: not a place handle or away"
        ));
    }
    let before = p.facts.len();
    p.facts.retain(|f| {
        f.said_by.is_some()
            || !(listed.contains(&f.slot.subject) && same_key(&f.slot.key, "location"))
    });
    if p.facts.len() < before {
        notes.push("removed a location fact of an NPC who leaves: the core writes it".into());
    }
}

/// The core writes the PC's `location` from `move_to` (ADR 0007), so a PC
/// `location` in a fact or a stake goes.
fn drop_pc_location(p: &mut DecisionProposal, pc: Handle, notes: &mut Vec<String>) {
    let is_pc_location = |slot: &SlotDraft| slot.subject == pc && same_key(&slot.key, "location");
    let before = p.facts.len();
    p.facts
        .retain(|f| f.said_by.is_some() || !is_pc_location(&f.slot));
    if p.facts.len() < before {
        notes.push(format!(
            "removed the PC's ({pc}) location fact: move_to moves the PC"
        ));
    }
    if let Some(c) = p.check.as_mut() {
        for (name, stake) in [("win", &mut c.win), ("lose", &mut c.lose)] {
            if stake.as_ref().is_some_and(is_pc_location) {
                *stake = None;
                notes.push(format!(
                    "removed the {name} stake on the PC's ({pc}) location: move_to moves the PC"
                ));
            }
        }
    }
}

/// A stake on the PC on a key the agency rule refuses (anything but
/// `PC_INVOLUNTARY_KEYS`) goes. Dropping a stake adds no PC act; the Check
/// still rolls (ADR 0007).
fn drop_pc_stakes(p: &mut DecisionProposal, pc: Handle, notes: &mut Vec<String>) {
    let Some(c) = p.check.as_mut() else {
        return;
    };
    for (name, stake) in [("win", &mut c.win), ("lose", &mut c.lose)] {
        if let Some(slot) = stake.as_ref()
            && slot.subject == pc
            && !is_involuntary_key(&slot.key)
        {
            notes.push(format!(
                "removed the {name} stake on the PC ({pc}) key {}: a stake on the PC may only set an involuntary key",
                slot.key
            ));
            *stake = None;
        }
    }
}

/// A blocker is something in the PC's way, never the PC's own place or
/// identity: a blocker that is a fact on the PC, on a key other than
/// `PC_INVOLUNTARY_KEYS` (an injury may block; `location` or `age` never
/// does), goes (ADR 0007).
fn drop_pc_blockers(
    p: &mut DecisionProposal,
    view: &WorldView,
    pc: Handle,
    notes: &mut Vec<String>,
) {
    p.blockers.retain(|blocker| {
        let own = view.fact(*blocker).is_some_and(|f| {
            Handle::Entity(f.slot.subject) == pc && !is_involuntary_key(&f.slot.key)
        });
        if own {
            notes.push(format!(
                "removed the blocker {blocker}: a fact about the PC itself never blocks"
            ));
        }
        !own
    });
}

/// A `location` value that names a known place, by its handle or by one of
/// its names, becomes that place's main name. Presence and the Scene place
/// then read it like every other location (`WorldView::present`,
/// `entities_by_name`): one spelling per place.
fn spell_places(p: &mut DecisionProposal, view: &WorldView, notes: &mut Vec<String>) {
    let mut slots: Vec<&mut SlotDraft> = p.facts.iter_mut().map(|f| &mut f.slot).collect();
    if let Some(c) = p.check.as_mut() {
        slots.extend(c.win.as_mut());
        slots.extend(c.lose.as_mut());
    }
    if let Some(s) = p.schedule.as_mut() {
        slots.extend(s.effects.iter_mut());
    }
    for slot in slots {
        if normalize::key(&slot.key) != "location" {
            continue;
        }
        let Some(name) = place_name(view, &slot.value) else {
            continue;
        };
        if normalize::value(&slot.value) != normalize::value(name) {
            notes.push(format!(
                "location value {:?} is the place {name:?}",
                slot.value
            ));
        }
        slot.value = name.to_owned();
    }
}

/// The main name of the place that `value` names: a handle of a place
/// entity, or a main or other name of one (lowest id first).
fn place_name<'a>(view: &'a WorldView, value: &str) -> Option<&'a str> {
    let by_handle = Handle::parse(value.trim())
        .ok()
        .and_then(|h| view.entity(h))
        .filter(|e| e.kind == EntityKind::Place);
    by_handle
        .or_else(|| {
            view.entities_by_name(value)
                .filter(|e| e.kind == EntityKind::Place)
                .min_by_key(|e| e.id)
        })
        .map(|e| e.name.as_str())
}
