//! Repairs the core makes to a parsed proposal before the Commit checks
//! (rules changed after QA, 2026-11-10). Each repair only removes a PC act
//! or respells a place, so it never adds a voluntary act of the PC: the
//! Agency rule holds. Every repair returns one note for the debug log.

use super::pc;
use crate::{
    domain::{DecisionProposal, EntityKind, Handle, SlotDraft},
    normalize,
    store::WorldView,
};

/// The proposal after the repairs, and one note per repair.
pub fn settle(mut p: DecisionProposal, view: &WorldView) -> (DecisionProposal, Vec<String>) {
    let mut notes = Vec::new();
    if let Some(pc) = pc(view).map(Handle::Entity) {
        drop_pc_acts(&mut p, pc, &mut notes);
    }
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

/// A `location` value that names a known place, by its handle or by one of
/// its names, becomes that place's main name. Presence and the Scene place
/// then read it like every other location (`WorldView::present`,
/// `entities_by_name`): one spelling per place.
fn spell_places(p: &mut DecisionProposal, view: &WorldView, notes: &mut Vec<String>) {
    let mut slots: Vec<&mut SlotDraft> = p.facts.iter_mut().map(|f| &mut f.slot).collect();
    if let Some(c) = p.check.as_mut() {
        slots.push(&mut c.win);
        slots.push(&mut c.lose);
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
