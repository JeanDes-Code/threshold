//! Helpers that the shell tests share.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use threshold_app::session::Session;
use threshold_core::{
    Gm, RecordedGm,
    domain::{ActionKind, DecisionProposal, Handle, NewFact, Seen, SlotDraft, Verdict},
    store::Store,
    view::{Block, Part, StoryTurn, TurnBody},
};

pub fn town_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/town")
}

pub fn allowed() -> String {
    serde_json::to_string(&base()).unwrap()
}

pub fn base() -> DecisionProposal {
    DecisionProposal {
        action: ActionKind::Observe,
        verdict: Verdict::Allowed,
        blockers: vec![],
        check: None,
        reactions: vec![],
        new_npcs: vec![],
        facts: vec![],
        cancel: vec![],
        schedule: None,
        minutes: 0,
        intimate: false,
    }
}

/// The id of a town entity, read from the campaign file.
pub fn entity(db: &str, name: &str) -> u64 {
    let store = Store::open(Path::new(db)).unwrap();
    let campaign = *store.campaigns().unwrap().last().unwrap();
    let view = store.snapshot(campaign).unwrap();
    view.entities.iter().find(|e| e.name == name).unwrap().id
}

/// A proposal with one new fact about `subject`.
pub fn with_fact(subject: u64, key: &str, value: &str, said_by: Option<u64>) -> String {
    let mut p = base();
    p.facts = vec![NewFact {
        slot: SlotDraft {
            subject: Handle::Entity(subject),
            key: key.into(),
            value: value.into(),
        },
        seen: Seen::Present,
        said_by: said_by.map(Handle::Entity),
        because: vec![],
    }];
    serde_json::to_string(&p).unwrap()
}

/// A session on a fresh data dir whose every Engine shares `gm`.
pub fn session(dir: &Path, gm: &RecordedGm) -> Session {
    let gm = gm.clone();
    Session::new(
        dir.to_path_buf(),
        Box::new(move || Box::new(gm.clone()) as Box<dyn Gm + Send>),
    )
}

pub fn town(session: &mut Session) {
    let dir = town_dir();
    session
        .new_campaign(
            &dir.join("adventure-seed.toml"),
            &dir.join("world.toml"),
            11,
        )
        .unwrap();
}

/// The Narration of a story Turn as the Player reads it: its blocks joined
/// by a blank line. `None` when the Narration is missing.
pub fn narration(turn: &StoryTurn) -> Option<String> {
    let TurnBody::Narration { blocks } = &turn.body else {
        return None;
    };
    let text = |parts: &[Part]| -> String {
        parts
            .iter()
            .map(|p| match p {
                Part::Text { text } | Part::Name { text, .. } => text.as_str(),
            })
            .collect()
    };
    Some(
        blocks
            .iter()
            .map(|b| match b {
                Block::Prose { parts } | Block::Speech { parts, .. } => text(parts),
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    )
}
