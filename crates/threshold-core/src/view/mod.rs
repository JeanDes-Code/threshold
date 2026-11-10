//! The screen types and the functions that make them.
//!
//! Every function here reads `PcSight` only, never the world, so a view
//! cannot show a hidden fact: its input does not hold one.

use serde::Serialize;

use crate::{
    domain::{EntityId, Knowing, is_open_key},
    world::PcSight,
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

/// The PC as the PC knows itself: only its own Beliefs about itself.
pub fn character_view(sight: &PcSight) -> CharacterView {
    let facts = sight
        .beliefs
        .iter()
        .filter(|b| b.subject == sight.pc)
        .map(|b| CharacterFact {
            open: is_open_key(&b.key),
            text: b.text.clone(),
        })
        .collect();
    CharacterView {
        name: sight.pc_name.clone(),
        concept: sight.concept.clone(),
        clock_min: sight.clock_min,
        facts,
    }
}

/// PC Knowledge, split: what the PC witnessed, and what it was told. A
/// subject or a speaker the PC cannot name reads "someone".
pub fn legacy_journal(sight: &PcSight) -> JournalView {
    let name = |id: EntityId| {
        sight
            .known
            .iter()
            .find(|k| k.id == id)
            .and_then(|k| k.name.clone())
            .unwrap_or_else(|| "someone".to_owned())
    };
    let mut journal = JournalView {
        observations: Vec::new(),
        claims: Vec::new(),
    };
    for belief in &sight.beliefs {
        let entry = JournalEntry {
            subject: name(belief.subject),
            text: belief.text.clone(),
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
