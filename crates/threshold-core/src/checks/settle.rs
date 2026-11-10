//! Repairs the core makes to a parsed proposal before the Commit checks
//! (rules changed after QA, 2026-11-10, and ADR 0007). Each repair only
//! removes model output about the PC or respells a place, so it never adds a
//! voluntary act of the PC: the Agency rule holds. Every repair returns one
//! note for the debug log.

use std::collections::HashSet;

use super::{closed, is_involuntary_key, pc, references::is_known_place, same_key};
use crate::{
    domain::{DecisionProposal, EntityKind, Handle, SlotDraft},
    normalize,
    store::WorldView,
    world,
};

/// The proposal after the repairs, and one note per repair.
pub fn settle(mut p: DecisionProposal, view: &WorldView) -> (DecisionProposal, Vec<String>) {
    let mut notes = Vec::new();
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
    settle_closes(&mut p, view, &mut notes);
    spell_places(&mut p, view, &mut notes);
    (p, notes)
}

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

/// A `closes` item that names no Open Item the PC holds, or names one a
/// second time, goes. So does a fact on the outcome slot of an item that
/// `closes` names: the core writes that slot itself (ADR 0007).
fn settle_closes(p: &mut DecisionProposal, view: &WorldView, notes: &mut Vec<String>) {
    let held = world::pc_open_items(view);
    let mut seen = HashSet::new();
    p.closes.retain(|close| {
        let known = held
            .iter()
            .any(|(subject, key)| world::open_item_ref(*subject, key) == close.item);
        let keep = known && seen.insert(close.item.clone());
        if !keep {
            notes.push(format!(
                "removed closes {}: not an open item the PC holds, or named twice",
                close.item
            ));
        }
        keep
    });
    let slots: Vec<(Handle, String)> = closed(p, view)
        .into_iter()
        .map(|(subject, key, _)| (Handle::Entity(subject), key.to_owned()))
        .collect();
    let before = p.facts.len();
    p.facts.retain(|f| {
        f.said_by.is_some()
            || !slots
                .iter()
                .any(|(subject, key)| f.slot.subject == *subject && same_key(&f.slot.key, key))
    });
    if p.facts.len() < before {
        notes.push("removed a fact on the outcome of an item in closes: closes writes it".into());
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
