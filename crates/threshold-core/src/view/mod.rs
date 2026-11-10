//! The screen types and the functions that make them.
//!
//! The functions here still read `WorldView`. `PcSight` replaces that input
//! later, so a view cannot see a hidden fact.

use serde::Serialize;

use crate::{
    domain::{EntityKind, Knowing, is_open_key},
    store::WorldView,
    world::pc_knowledge,
};

#[derive(Clone, Debug, Serialize)]
pub struct CharacterView {
    pub name: String,
    pub concept: String,
    pub clock_min: u64,
    pub facts: Vec<CharacterFact>,
}

/// One thing the PC knows about itself.
#[derive(Clone, Debug, Serialize)]
pub struct CharacterFact {
    pub text: String,
    /// An open item (ADR 0005): planned or pending, not done.
    pub open: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct JournalEntry {
    pub subject: String,
    pub text: String,
    pub source: Option<String>,
    /// An open item (ADR 0005): planned or pending, not done.
    pub open: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct JournalView {
    pub observations: Vec<JournalEntry>,
    pub claims: Vec<JournalEntry>,
}

/// The PC as the PC knows itself: only its own Beliefs about itself. `None`
/// when the campaign has no PC.
pub fn character_view(view: &WorldView) -> Option<CharacterView> {
    let pc = view.entities.iter().find(|e| e.kind == EntityKind::Pc)?;
    let concept = view
        .adventure_seed
        .as_ref()
        .map(|s| s.pc.concept.clone())
        .unwrap_or_default();
    let facts = pc_knowledge(view)
        .into_iter()
        .filter(|b| b.subject == pc.id)
        .map(|b| CharacterFact {
            open: is_open_key(&b.key),
            text: b.text,
        })
        .collect();
    Some(CharacterView {
        name: pc.name.clone(),
        concept,
        clock_min: view.clock_min,
        facts,
    })
}

/// PC Knowledge, split: what the PC witnessed, and what it was told. Built
/// from the PC's Beliefs only, so a fact no one told the PC (a GM-only fact)
/// cannot appear.
pub fn legacy_journal(view: &WorldView) -> JournalView {
    let name = |id: u64| {
        view.entities
            .iter()
            .find(|e| e.id == id)
            .map_or_else(|| "someone".to_owned(), |e| e.name.clone())
    };
    let mut journal = JournalView {
        observations: Vec::new(),
        claims: Vec::new(),
    };
    for belief in pc_knowledge(view) {
        let entry = JournalEntry {
            subject: name(belief.subject),
            text: belief.text,
            source: belief.source.map(name),
            open: is_open_key(&belief.key),
        };
        match belief.how {
            Knowing::Witnessed => journal.observations.push(entry),
            Knowing::Told => journal.claims.push(entry),
        }
    }
    journal
}
