//! Helpers that the shell tests share.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use threshold_core::domain::Intimacy;

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
        contested: false,
        move_to: None,
        leaves: vec![],
        settles: None,
        finds: Default::default(),
        blockers: vec![],
        check: None,
        reactions: vec![],
        new_npcs: vec![],
        facts: vec![],
        cancel: vec![],
        schedule: None,
        minutes: 0,
        intimacy: Intimacy::None,
        facts_on_win: false,
        offered: vec![],
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

/// The town fixture with its seed at Detail Level `implied`. Every age of a
/// fixture Campaign is unconfirmed.
pub fn town_at_implied(s: &mut Session, dir: &Path) {
    let seed = std::fs::read_to_string(town_dir().join("adventure-seed.toml")).unwrap();
    let seed_path = dir.join("adventure-seed.toml");
    std::fs::write(&seed_path, format!("detail = \"implied\"\n{seed}")).unwrap();
    s.new_campaign(&seed_path, &town_dir().join("world.toml"), 11)
        .unwrap();
}

/// Confirms the age of every adult character of the open Campaign, through
/// the core's op, as the Player would in play (Task 8).
pub fn confirm_all(s: &Session) {
    use threshold_core::{
        domain::{EntityKind, EntityRef, LedgerOp},
        store::{TurnKind, TurnRecord},
    };
    let db = s.summary().unwrap().db_path;
    let mut store = Store::open(Path::new(&db)).unwrap();
    let campaign = *store.campaigns().unwrap().last().unwrap();
    let view = store.snapshot(campaign).unwrap();
    let ops: Vec<LedgerOp> = view
        .entities
        .iter()
        .filter(|e| matches!(e.kind, EntityKind::Pc | EntityKind::Npc))
        .filter(|e| e.age.is_some_and(|age| age >= 18))
        .map(|e| LedgerOp::ConfirmAge {
            entity: EntityRef::Id(e.id),
        })
        .collect();
    store
        .commit(
            TurnRecord {
                campaign_id: campaign,
                op_id: "confirm-all".into(),
                kind: TurnKind::Setup,
                intent: String::new(),
                proposal_json: "{}".into(),
                clock_before: view.clock_min,
                clock_after: view.clock_min,
            },
            ops,
        )
        .unwrap();
}

/// An intimate proposal in which Hessa, who stands in the town hall with
/// Wren, goes along.
pub fn intimate_with_hessa(db: &str) -> String {
    use threshold_core::domain::{Reaction, Stance, Willing};
    let mut p = base();
    p.intimacy = Intimacy::Intimate;
    p.minutes = 5;
    p.reactions = vec![Reaction {
        npc: Handle::Entity(entity(db, "Hessa")),
        stance: Stance::Comply,
        plan: "stays".into(),
        cause: None,
        willing: Some(Willing::Yes),
    }];
    serde_json::to_string(&p).unwrap()
}

/// The town at `implied`, every adult confirmed, and one intimate Turn that
/// opens an Intimate Scene.
pub fn town_in_a_scene(s: &mut Session, dir: &Path, gm: &RecordedGm) {
    town_at_implied(s, dir);
    confirm_all(s);
    let db = s.summary().unwrap().db_path;
    gm.push_proposal(intimate_with_hessa(&db));
    gm.push_narration("Hessa smiles.".into());
    s.submit_intent("scene-1".into(), "I sit with Hessa.", &mut |_| {})
        .unwrap();
}
