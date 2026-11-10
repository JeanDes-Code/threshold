//! The shell's command layer, driven without a window: `Session` is what
//! every Tauri command calls.

use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

mod support;

use serde_json::{Value, json};
use support::{allowed, entity, narration, session, town, town_dir, with_fact};
use threshold_app::{
    drafts::{DraftView, Drafts},
    session::{AppError, Session},
};
use threshold_core::{
    Gm, GmError, Prompt, RecordedGm,
    build::{BuildCheck, BuildFailure, DraftState, GapKind, Progress, Stage},
    domain::Handle,
    rules::D10,
    store::{Store, TurnKind},
    turn::Engine,
};

#[test]
fn get_debug_refused_when_debug_off() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![allowed()], vec!["The hall is quiet.".into()]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let turn = s
        .submit_intent("op-1".into(), "I look around.", &mut |_| {})
        .unwrap();

    assert!(matches!(s.get_debug(turn.turn_id), Err(AppError::DebugOff)));

    s.set_debug(true);
    let debug = s.get_debug(turn.turn_id).unwrap();
    assert_eq!(debug.op_id, "op-1");
    assert!(debug.entries.iter().any(|e| e.step == "propose"));
    assert!(debug.entries.iter().any(|e| e.step == "narrate"));

    s.set_debug(false);
    assert!(matches!(s.get_debug(turn.turn_id), Err(AppError::DebugOff)));
}

#[test]
fn journal_hides_gm_only_facts() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);

    let journal = s.get_journal().unwrap();
    let texts: Vec<&str> = journal
        .observations
        .iter()
        .chain(&journal.claims)
        .map(|e| e.text.as_str())
        .collect();
    // The PC witnessed who stands in the town hall at the start.
    assert!(
        journal
            .observations
            .iter()
            .any(|e| e.text.contains("Magistrate Orsel holds court")),
        "{texts:?}"
    );
    // The GM-only secret and the unheard rumor never show.
    assert!(texts.iter().all(|t| !t.contains("skims")), "{texts:?}");
    assert!(texts.iter().all(|t| !t.contains("scales")), "{texts:?}");
    assert!(journal.claims.is_empty());

    let character = s.get_character().unwrap();
    assert_eq!(character.name, "Wren");
    assert!(character.facts.iter().all(|f| !f.text.contains("skims")));
}

#[test]
fn journal_marks_open_items() {
    let dir = tempfile::tempdir().unwrap();
    let world = dir.path().join("world.toml");
    std::fs::write(
        &world,
        r#"
[[facts]]
subject = "Hessa"
key = "shopping_trip"
value = "the market"
text = "Hessa plans a shopping trip to the market."
known_by = ["Wren"]
open = true

[[facts]]
subject = "Wren"
key = "audience"
value = "the magistrate"
text = "Wren has an audience with the magistrate next week."
known_by = ["Wren"]
open = true
"#,
    )
    .unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    s.new_campaign(&town_dir().join("adventure-seed.toml"), &world, 11)
        .unwrap();

    // The Character view marks an open item about the PC as the Journal does.
    let character = s.get_character().unwrap();
    let audience = character
        .facts
        .iter()
        .find(|f| f.text.contains("audience"))
        .expect("the PC knows its open item");
    assert!(audience.open);
    assert!(
        character
            .facts
            .iter()
            .filter(|f| !f.text.contains("audience"))
            .all(|f| !f.open)
    );

    let journal = s.get_journal().unwrap();
    let trip = journal
        .observations
        .iter()
        .find(|e| e.text.contains("shopping trip"))
        .expect("the PC holds the open item");
    assert!(trip.open);
    assert!(
        journal
            .observations
            .iter()
            .filter(|e| !e.text.contains("shopping trip") && !e.text.contains("audience"))
            .all(|e| !e.open)
    );
}

#[test]
fn turn_streams_and_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![allowed()], vec!["The hall is quiet.".into()]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let mut chunks = Vec::new();
    let turn = s
        .submit_intent("op-1".into(), "I look around.", &mut |c| {
            chunks.push(c.to_owned())
        })
        .unwrap();
    assert_eq!(chunks.concat(), "The hall is quiet.");
    assert_eq!(turn.narration.as_deref(), Some("The hall is quiet."));
    assert_eq!(turn.roll, None);

    // A retry with the same op_id returns the stored Turn: no second Commit.
    let again = s
        .submit_intent("op-1".into(), "I look around.", &mut |_| {})
        .unwrap();
    assert_eq!(again.turn_id, turn.turn_id);

    let db = s.summary().unwrap().db_path;
    let mut reopened = session(dir.path(), &gm);
    let summary = reopened.open_campaign(Path::new(&db)).unwrap();
    assert_eq!(summary.pc_name, "Wren");
    let story = reopened.get_play_view().unwrap().story;
    assert_eq!(story.len(), 1);
    assert_eq!(story[0].intent, "I look around.");
    assert_eq!(narration(&story[0]).as_deref(), Some("The hall is quiet."));
}

#[test]
fn commands_need_a_campaign() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    assert!(matches!(s.get_journal(), Err(AppError::NoCampaign)));
    assert!(matches!(
        s.submit_intent("op".into(), "hi", &mut |_| {}),
        Err(AppError::NoCampaign)
    ));
    assert!(s.open_campaign(&dir.path().join("missing.sqlite")).is_err());
}

#[test]
fn config_is_created_once() {
    let dir = tempfile::tempdir().unwrap();
    let config = threshold_app::session::load_config(dir.path()).unwrap();
    assert_eq!(config, threshold_ollama::OllamaConfig::default());
    let path = dir.path().join("threshold.toml");
    std::fs::write(&path, "model = \"other:tag\"\n").unwrap();
    let config = threshold_app::session::load_config(dir.path()).unwrap();
    assert_eq!(config.model, "other:tag");
}

#[test]
fn journal_lists_claims_with_their_source() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec!["Hessa leans in.".into()]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;
    let (hessa, tobin) = (entity(&db, "Hessa"), entity(&db, "Tobin"));
    gm.push_proposal(with_fact(tobin, "cheats", "the scales", Some(hessa)));
    s.submit_intent("op-1".into(), "I ask Hessa about the miller.", &mut |_| {})
        .unwrap();

    let journal = s.get_journal().unwrap();
    let claim = journal
        .claims
        .iter()
        .find(|c| c.subject == "Tobin")
        .expect("the told Belief is a Claim");
    assert_eq!(claim.source.as_deref(), Some("Hessa"));
    assert!(
        journal
            .observations
            .iter()
            .all(|o| o.subject != "Tobin" || !o.text.contains("cheats"))
    );
}

#[test]
fn open_campaign_refuses_a_foreign_file() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);

    let empty = dir.path().join("empty.sqlite");
    std::fs::write(&empty, b"").unwrap();
    assert!(s.open_campaign(&empty).is_err());
    // Refused before any migration: the file is still empty.
    assert_eq!(std::fs::metadata(&empty).unwrap().len(), 0);

    let text = dir.path().join("notes.sqlite");
    std::fs::write(&text, b"just some notes, not a database at all").unwrap();
    assert!(s.open_campaign(&text).is_err());
    assert!(matches!(s.summary(), Err(AppError::NoCampaign)));
}

#[test]
fn rejected_turn_error_hides_gm_only_facts() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;
    let orsel = entity(&db, "Orsel");
    // Both attempts try to bend the GM-only secret.
    let bend = with_fact(orsel, "skims", "nothing at all", None);
    gm.push_proposal(bend.clone());
    gm.push_proposal(bend);

    let err = s
        .submit_intent("op-1".into(), "I accuse Orsel.", &mut |_| {})
        .unwrap_err()
        .to_string();
    assert!(err.contains("fact bends"), "{err}");
    for secret in ["skims", "grain tithe"] {
        assert!(!err.contains(secret), "{err}");
    }
    // No handle either: a fact handle reveals that a hidden fact exists, and
    // its id.
    let skims = {
        let store = Store::open(Path::new(&db)).unwrap();
        let campaign = *store.campaigns().unwrap().last().unwrap();
        store
            .snapshot(campaign)
            .unwrap()
            .live_fact(orsel, "skims")
            .unwrap()
            .id
    };
    assert!(!err.contains(&Handle::Fact(skims).to_string()), "{err}");
    assert!(!err.contains(&Handle::Entity(orsel).to_string()), "{err}");
    // The raw reason did name the secret's key: the formatter removed it.
    s.set_debug(true);
    let raw = Store::open(Path::new(&db))
        .unwrap()
        .debug_log("op-1")
        .unwrap();
    assert!(
        raw.iter()
            .any(|e| e.step == "validate" && e.raw.contains("skims"))
    );
}

#[test]
fn debug_flag_is_shared_outside_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let s = session(dir.path(), &gm);
    let flag = s.debug_flag();
    flag.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(s.debug());
}

/// Final fix 1: a narration that fails after the Commit keeps the Turn. The
/// result says so, the Play view lists the Turn without Narration, and a retry
/// under the same op_id narrates it without a second Turn row.
#[test]
fn narration_failure_keeps_the_committed_turn() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![allowed()], vec![]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let turn = s
        .submit_intent("op-1".into(), "I look around.", &mut |_| {})
        .unwrap();
    assert_eq!(turn.narration, None);
    assert!(turn.narration_error.is_some());
    let story = s.get_play_view().unwrap().story;
    assert_eq!(story.len(), 1);
    assert_eq!(story[0].turn_id, turn.turn_id);
    assert_eq!(narration(&story[0]), None);

    gm.push_narration("The hall is quiet.".into());
    let again = s
        .submit_intent("op-1".into(), "I look around.", &mut |_| {})
        .unwrap();
    assert_eq!(again.turn_id, turn.turn_id);
    assert_eq!(again.narration.as_deref(), Some("The hall is quiet."));
    assert_eq!(again.narration_error, None);
    let story = s.get_play_view().unwrap().story;
    assert_eq!(story.len(), 1, "exactly one Turn row");
    assert_eq!(narration(&story[0]).as_deref(), Some("The hall is quiet."));
}

/// Final fix 4: the Play view marks the Turn a Correction reverted.
#[test]
fn summary_marks_reverted_turns() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(
        vec![allowed(), allowed()],
        vec!["You look.".into(), "You listen.".into()],
    );
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let played = s
        .submit_intent("op-1".into(), "I look around.", &mut |_| {})
        .unwrap();
    let corrected = s
        .correct_last("op-2".into(), "I listen.", "I meant listening", &mut |_| {})
        .unwrap();
    let lines = s.get_play_view().unwrap().story;
    let line = |id: u64| lines.iter().find(|l| l.turn_id == id).unwrap();
    assert!(line(played.turn_id).reverted);
    assert!(!line(corrected.turn_id).reverted);
}

/// The opening Narration lives on its Opening Turn, so the Play view of a
/// reopened file shows it again.
#[test]
fn opening_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec!["The hall is quiet at dawn.".into()]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    let db = s.summary().unwrap().db_path;
    // No `begin` command yet: drive the Engine on the file directly.
    {
        let store = Store::open_existing(Path::new(&db)).unwrap();
        let campaign = *store.campaigns().unwrap().last().unwrap();
        let mut engine = Engine::new(store, Box::new(gm.clone()), Box::new(D10), vec![], campaign);
        engine.begin(&mut |_| {}).unwrap();
    }
    drop(s);

    let mut reopened = session(dir.path(), &gm);
    reopened.open_campaign(Path::new(&db)).unwrap();
    let story = reopened.get_play_view().unwrap().story;
    assert_eq!(story.len(), 1, "{story:?}");
    assert_eq!(story[0].kind, TurnKind::Opening);
    assert_eq!(story[0].intent, "");
    assert_eq!(
        narration(&story[0]).as_deref(),
        Some("The hall is quiet at dawn.")
    );
    assert!(!story[0].reverted);
}

// ---- Campaign from a document ------------------------------------------------

/// Something a test runs inside a model call of the draft or the build.
type Hook = Arc<dyn Fn() + Send + Sync>;

/// A `RecordedGm` that runs the test's hook after each proposal.
#[derive(Clone)]
struct Hooked {
    inner: RecordedGm,
    hook: Arc<Mutex<Option<Hook>>>,
}

impl Gm for Hooked {
    fn propose(&self, prompt: &Prompt, schema: &Value) -> Result<String, GmError> {
        let out = self.inner.propose(prompt, schema);
        // Called without the slot's lock, so a hook that waits never blocks
        // another job's call.
        let hook = self.hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook();
        }
        out
    }

    fn narrate(&self, prompt: &Prompt, on_chunk: &mut dyn FnMut(&str)) -> Result<String, GmError> {
        self.inner.narrate(prompt, on_chunk)
    }
}

/// A data dir, the shared session, the draft commands, the build model and
/// the play model.
struct Rig {
    dir: tempfile::TempDir,
    session: Arc<Mutex<Session>>,
    drafts: Drafts,
    build: RecordedGm,
    play: RecordedGm,
    hook: Arc<Mutex<Option<Hook>>>,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let build = RecordedGm::new(vec![], vec![]);
        let play = RecordedGm::new(vec![], vec![]);
        let hook: Arc<Mutex<Option<Hook>>> = Arc::new(Mutex::new(None));
        let session = Arc::new(Mutex::new(session(dir.path(), &play)));
        let hooked = Hooked {
            inner: build.clone(),
            hook: hook.clone(),
        };
        let drafts = Drafts::new(
            dir.path(),
            session.clone(),
            Box::new(move || Box::new(hooked.clone()) as Box<dyn Gm + Send>),
        )
        .unwrap();
        Self {
            dir,
            session,
            drafts,
            build,
            play,
            hook,
        }
    }

    fn push(&self, outputs: &[Value]) {
        for out in outputs {
            self.build.push_proposal(out.to_string());
        }
    }

    fn set_hook(&self, hook: Option<Hook>) {
        *self.hook.lock().unwrap() = hook;
    }

    fn session(&self) -> std::sync::MutexGuard<'_, Session> {
        self.session.lock().unwrap()
    }

    /// The files in `campaigns/`, by name.
    fn campaign_files(&self) -> Vec<String> {
        std::fs::read_dir(self.dir.path().join("campaigns"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()
    }

    /// A draft from `doc_source()`, its age confirmed, approved.
    fn approved_draft(&self) -> String {
        self.push(&draft_outputs());
        let view = self
            .drafts
            .draft_from_text(&doc_source(), &mut |_| {})
            .unwrap();
        let view = self
            .drafts
            .save_draft(&view.draft_id, view.seed, view.knowledge, vec![], vec![])
            .unwrap();
        let view = self.drafts.approve_draft(&view.draft_id).unwrap();
        assert_eq!(view.state, DraftState::Approved);
        view.draft_id
    }
}

/// A section of about 450 characters under a level-one heading.
fn doc_section(title: &str, sentence: &str) -> String {
    let mut body = String::new();
    while body.chars().count() < 450 {
        body.push_str(sentence);
        body.push(' ');
    }
    format!("# {title}\n\n{body}\n\n")
}

/// An invented Source Text with two sections.
fn doc_source() -> String {
    [
        doc_section(
            "The harbour",
            "Mira Holt keeps the lighthouse at Gull Point.",
        ),
        doc_section("The council", "The Harbour Council meets in the Tide Hall."),
    ]
    .concat()
}

fn doc_npc(name: &str, age: u16, stated: bool) -> Value {
    json!({"name": name, "age": age, "age_stated": stated, "other_names": []})
}

fn doc_thing(name: &str, kind: &str) -> Value {
    json!({"name": name, "kind": kind, "other_names": []})
}

fn doc_fact(subject: &str, key: &str, value: &str, open: bool) -> Value {
    json!({
        "subject": subject,
        "key": key,
        "value": value,
        "text": format!("{subject}: {key} is {value}."),
        "open": open,
        "pc_knows": true
    })
}

/// The roster with the PC (one proposed age), the seed and the PC Knowledge
/// of each section of `doc_source()`.
fn draft_outputs() -> Vec<Value> {
    vec![
        json!({
            "pc": {
                "name": "Mira Holt",
                "age": 31,
                "age_stated": true,
                "other_names": [{"name": "the Keeper", "known_to": ["Mira Holt", "Tobin Reed"]}],
                "concept": "A careful lighthouse keeper."
            },
            "characters": [doc_npc("Tobin Reed", 58, true), doc_npc("Ada Venn", 40, false)],
            "others": [
                doc_thing("Gull Point", "place"),
                doc_thing("Tide Hall", "place"),
                doc_thing("Harbour Council", "faction"),
            ]
        }),
        json!({
            "premise": "A lighthouse keeper faces the storm season.",
            "story_language": "en",
            "themes": ["duty", "weather"],
            "gm_agreement": "Keep the pace slow.",
            "resolution": "Roll when the outcome is uncertain.",
            "min_age": null,
            "start": {
                "place": "Gull Point",
                "present": ["Tobin Reed"],
                "situation": "Night falls on the point.",
                "when": "An autumn evening"
            },
            "open_details": []
        }),
        json!({"facts": [doc_fact("Mira Holt", "occupation", "lighthouse keeper", false)]}),
        json!({"facts": [doc_fact("Harbour Council", "vote", "new pier", true)]}),
    ]
}

/// What the hidden world holds: none of it may reach a draft view or a
/// Player error.
const HIDDEN_TEXT: [&str; 7] = [
    "forty coins",
    "debt",
    "quiet winter",
    "tin box",
    "savings",
    "short lines",
    "harbour chains",
];

fn npc_state(name: &str) -> Value {
    json!({
        "goals": [{"text": format!("{name} wants a quiet winter."), "against": null}],
        "secret": {
            "subject": name,
            "key": "savings",
            "value": "a tin box",
            "text": format!("{name} hides savings in a tin box."),
            "open": false
        },
        "attitudes": [],
        "voice_card": {
            "habits": format!("{name} speaks in short lines."),
            "vocabulary": "tide, rope, lamp",
            "samples": ["Mind the rope.", "The tide turns soon."]
        }
    })
}

/// Every output of a full World Build of the approved draft, in call order.
fn build_outputs() -> Vec<Value> {
    vec![
        json!({"facts": []}),
        json!({"facts": [{
            "subject": "Ada Venn",
            "key": "debt",
            "value": "forty coins",
            "text": "Ada Venn owes forty coins.",
            "open": false,
            "known_by": ["Ada Venn"]
        }]}),
        npc_state("Tobin Reed"),
        npc_state("Ada Venn"),
        json!({"events": [{
            "in_minutes": 60,
            "actor": "Tobin Reed",
            "text": "Tobin Reed checks the harbour chains.",
            "effects": [],
            "seen_by": ["Ada Venn"]
        }]}),
        json!({"placements": [{"npc": "Ada Venn", "place": "Tide Hall"}]}),
    ]
}

/// A build that fails at the NPC stage: Ada Venn's Voice Card has one
/// sample, in the answer and in the repair.
fn failing_build_outputs() -> Vec<Value> {
    let mut bad = npc_state("Ada Venn");
    bad["voice_card"]["samples"] = json!(["Mind the rope."]);
    let outputs = build_outputs();
    vec![
        outputs[0].clone(),
        outputs[1].clone(),
        outputs[2].clone(),
        bad.clone(),
        bad,
    ]
}

fn stages(progress: &[Progress]) -> Vec<(Stage, u32, u32)> {
    progress
        .iter()
        .map(|p| (p.stage, p.done, p.total))
        .collect()
}

#[test]
fn draft_flow_end_to_end() {
    let rig = Rig::new();
    rig.push(&draft_outputs());
    let mut progress = Vec::new();
    let view = rig
        .drafts
        .draft_from_text(&doc_source(), &mut |p| progress.push(p))
        .unwrap();
    assert_eq!(view.state, DraftState::Draft);
    assert_eq!(view.seed.pc.name, "Mira Holt");
    // Every age waits for the Player; the model's claim is information.
    assert_eq!(
        view.ages_to_confirm,
        ["Mira Holt", "Tobin Reed", "Ada Venn"]
    );
    assert_eq!(view.ages_stated, ["Mira Holt", "Tobin Reed"]);
    for at in ["pc.age", "entities[0].age", "entities[1].age"] {
        assert!(
            view.gaps
                .iter()
                .any(|g| g.kind == GapKind::AgeUnconfirmed && g.at == at),
            "{at}: {:?}",
            view.gaps
        );
    }
    assert!(stages(&progress).contains(&(Stage::DraftSeed, 0, 1)));
    assert!(stages(&progress).contains(&(Stage::DraftKnowledge, 2, 2)));
    let id = view.draft_id.clone();

    // Approval refuses a draft with gaps and returns them.
    match rig.drafts.approve_draft(&id) {
        Err(e @ AppError::Gaps(_)) => {
            let AppError::Gaps(gaps) = &e else {
                unreachable!()
            };
            assert_eq!(*gaps, view.gaps);
            // The UI receives the list itself.
            assert_eq!(
                serde_json::to_value(&e).unwrap(),
                serde_json::to_value(&view.gaps).unwrap()
            );
        }
        other => panic!("{other:?}"),
    }
    // The Player confirms every age: the draft has no gap left.
    let saved = rig
        .drafts
        .save_draft(
            &id,
            view.seed.clone(),
            view.knowledge.clone(),
            vec![],
            vec![],
        )
        .unwrap();
    assert_eq!(saved.state, DraftState::Draft);
    assert!(saved.gaps.is_empty(), "{:?}", saved.gaps);
    let approved = rig.drafts.approve_draft(&id).unwrap();
    assert_eq!(approved.state, DraftState::Approved);
    let lines = rig.drafts.list_drafts().unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].draft_id, id);
    assert_eq!(lines[0].state, DraftState::Approved);
    assert!(lines[0].title.starts_with("A lighthouse keeper"));

    rig.push(&build_outputs());
    let mut progress = Vec::new();
    let summary = rig
        .drafts
        .start_build(&id, &mut |p| progress.push(p))
        .unwrap();
    assert!(summary.needs_begin);
    assert_eq!(summary.pc_name, "Mira Holt");
    assert!(rig.session().get_play_view().unwrap().story.is_empty());
    let seen = stages(&progress);
    assert_eq!(seen.first(), Some(&(Stage::HiddenFacts, 0, 2)));
    assert_eq!(
        seen[seen.len() - 2..],
        [(Stage::Commit, 0, 1), (Stage::Commit, 1, 1)]
    );
    assert!(Path::new(&summary.db_path).exists());
    assert_eq!(
        rig.session().list_campaigns(),
        vec![summary.db_path.clone()]
    );
    assert_eq!(rig.drafts.get_draft(&id).unwrap().state, DraftState::Built);

    // The Campaign is the open one: Begin plays its Opening Turn.
    rig.play
        .push_narration("The lamp burns over Gull Point.".into());
    let mut chunks = Vec::new();
    let opening = rig
        .session()
        .begin(&mut |c| chunks.push(c.to_owned()))
        .unwrap();
    assert_eq!(
        opening.narration.as_deref(),
        Some("The lamp burns over Gull Point.")
    );
    assert_eq!(chunks.concat(), "The lamp burns over Gull Point.");
    let summary = rig.session().summary().unwrap();
    assert!(!summary.needs_begin);
    let story = rig.session().get_play_view().unwrap().story;
    assert_eq!(story.len(), 1);
    assert_eq!(story[0].kind, TurnKind::Opening);
}

#[test]
fn old_campaign_reports_needs_begin() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![allowed()], vec!["The hall is quiet.".into()]);
    let mut s = session(dir.path(), &gm);
    town(&mut s);
    assert!(s.summary().unwrap().needs_begin);
    s.submit_intent("op-1".into(), "I look around.", &mut |_| {})
        .unwrap();
    assert!(!s.summary().unwrap().needs_begin);
}

#[test]
fn failed_build_leaves_no_campaign_and_keeps_the_draft() {
    let rig = Rig::new();
    let id = rig.approved_draft();
    rig.push(&failing_build_outputs());
    assert!(rig.drafts.start_build(&id, &mut |_| {}).is_err());

    let files = rig.campaign_files();
    assert!(
        files
            .iter()
            .all(|f| !f.ends_with(".sqlite") && !f.contains(".part")),
        "{files:?}"
    );
    let view = rig.drafts.get_draft(&id).unwrap();
    assert_eq!(view.state, DraftState::Approved);
    assert_eq!(
        view.last_failure,
        Some(BuildFailure {
            stage: Stage::NpcState,
            check: BuildCheck::Completeness
        })
    );
    assert!(matches!(rig.session().summary(), Err(AppError::NoCampaign)));

    // A retry needs no new review: no save, no approval.
    rig.push(&build_outputs());
    let summary = rig.drafts.start_build(&id, &mut |_| {}).unwrap();
    assert!(summary.needs_begin);
    let view = rig.drafts.get_draft(&id).unwrap();
    assert_eq!(view.state, DraftState::Built);
    assert_eq!(view.last_failure, None);
}

#[test]
fn build_error_names_stage_and_check_only() {
    let rig = Rig::new();
    let id = rig.approved_draft();
    rig.push(&failing_build_outputs());
    let err = rig
        .drafts
        .start_build(&id, &mut |_| {})
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "The world build stopped at the characters step: a completeness check failed."
    );
    for text in HIDDEN_TEXT
        .iter()
        .chain(&["Mind the rope", "Ada Venn", "Tobin Reed"])
    {
        assert!(!err.contains(text), "{err}");
    }
}

#[test]
fn source_text_is_checked_before_the_model() {
    let rig = Rig::new();
    let empty = rig
        .drafts
        .draft_from_text("  \n", &mut |_| {})
        .unwrap_err()
        .to_string();
    assert!(empty.contains("empty"), "{empty}");
    let long = "a".repeat(threshold_core::build::SOURCE_TEXT_MAX_CHARS + 1);
    let err = rig
        .drafts
        .draft_from_text(&long, &mut |_| {})
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(&threshold_core::build::SOURCE_TEXT_MAX_CHARS.to_string()),
        "{err}"
    );
    assert!(rig.build.calls().is_empty());
}

#[test]
fn one_build_at_a_time() {
    let rig = Rig::new();
    let id = rig.approved_draft();
    rig.push(&build_outputs());
    // The first model call of the build waits for the test; later calls
    // pass, so a missing guard fails this test instead of hanging it.
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let first_call = AtomicBool::new(true);
    let gate = Mutex::new((entered_tx, release_rx));
    rig.set_hook(Some(Arc::new(move || {
        if first_call.swap(false, Ordering::SeqCst) {
            let gate = gate.lock().unwrap();
            let _ = gate.0.send(());
            let _ = gate.1.recv();
        }
    })));

    let first = {
        let drafts = rig.drafts.clone();
        let id = id.clone();
        thread::spawn(move || drafts.start_build(&id, &mut |_| {}))
    };
    entered_rx.recv().unwrap();

    let second = rig.drafts.start_build(&id, &mut |_| {}).unwrap_err();
    assert!(second.to_string().contains("running"), "{second}");
    let draft = rig
        .drafts
        .draft_from_text(&doc_source(), &mut |_| {})
        .unwrap_err();
    assert!(draft.to_string().contains("running"), "{draft}");
    // The draft under build cannot change.
    let view = rig.drafts.get_draft(&id).unwrap();
    assert!(
        rig.drafts
            .save_draft(&id, view.seed, view.knowledge, vec![], vec![])
            .is_err()
    );
    assert!(rig.drafts.delete_draft(&id).is_err());
    // The build does not hold the session lock while the model runs.
    assert!(rig.session.try_lock().is_ok());

    drop(release_tx);
    let summary = first.join().unwrap().unwrap();
    assert!(summary.needs_begin);
    assert_eq!(rig.build.calls().len(), 4 + 6);
    // The guard is free again: the next refusal is about the draft.
    let again = rig.drafts.start_build(&id, &mut |_| {}).unwrap_err();
    assert!(!again.to_string().contains("running"), "{again}");
}

#[test]
fn cancel_build_stops_draft_and_build() {
    let rig = Rig::new();
    let cancel_after_each_call = || {
        let drafts = rig.drafts.clone();
        rig.set_hook(Some(Arc::new(move || drafts.cancel_build())));
    };

    // A draft, cancelled during its first call, leaves nothing.
    rig.push(&draft_outputs()[..1]);
    cancel_after_each_call();
    let err = rig
        .drafts
        .draft_from_text(&doc_source(), &mut |_| {})
        .unwrap_err()
        .to_string();
    assert_eq!(err, "The draft stopped at the seed step: it was cancelled.");
    assert_eq!(rig.build.calls().len(), 1);
    assert!(rig.drafts.list_drafts().unwrap().is_empty());
    let left: Vec<_> = std::fs::read_dir(rig.dir.path().join("drafts"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(left.is_empty(), "{left:?}");

    // The flag resets when the next job starts: this draft runs to its end.
    rig.set_hook(None);
    let id = rig.approved_draft();

    // A build, cancelled during its first call, keeps the approved draft.
    rig.push(&build_outputs()[..1]);
    cancel_after_each_call();
    let err = rig
        .drafts
        .start_build(&id, &mut |_| {})
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "The world build stopped at the hidden facts step: it was cancelled."
    );
    let view = rig.drafts.get_draft(&id).unwrap();
    assert_eq!(view.state, DraftState::Approved);
    assert_eq!(
        view.last_failure,
        Some(BuildFailure {
            stage: Stage::HiddenFacts,
            check: BuildCheck::Cancelled
        })
    );
    assert!(rig.campaign_files().is_empty());

    rig.set_hook(None);
    rig.push(&build_outputs());
    assert!(
        rig.drafts
            .start_build(&id, &mut |_| {})
            .unwrap()
            .needs_begin
    );
}

#[test]
fn a_cancel_during_the_last_call_writes_no_campaign() {
    let rig = Rig::new();
    let id = rig.approved_draft();
    let before = rig.build.calls().len();
    let outputs = build_outputs();
    let last = outputs.len();
    rig.push(&outputs);
    let drafts = rig.drafts.clone();
    let calls = Arc::new(Mutex::new(0));
    rig.set_hook(Some(Arc::new(move || {
        let mut n = calls.lock().unwrap();
        *n += 1;
        if *n == last {
            drafts.cancel_build();
        }
    })));

    let err = rig
        .drafts
        .start_build(&id, &mut |_| {})
        .unwrap_err()
        .to_string();
    assert_eq!(rig.build.calls().len() - before, last);
    assert_eq!(
        err,
        "The world build stopped at the campaign file step: it was cancelled."
    );
    let view = rig.drafts.get_draft(&id).unwrap();
    assert_eq!(view.state, DraftState::Approved);
    assert_eq!(
        view.last_failure,
        Some(BuildFailure {
            stage: Stage::Commit,
            check: BuildCheck::Cancelled
        })
    );
    assert!(rig.campaign_files().is_empty());
}

#[test]
fn part_files_are_deleted_at_start() {
    let dir = tempfile::tempdir().unwrap();
    let gm = RecordedGm::new(vec![], vec![]);
    let kept = {
        let mut s = session(dir.path(), &gm);
        town(&mut s);
        s.summary().unwrap().db_path
    };
    let campaigns = dir.path().join("campaigns");
    let part = "campaign-2.sqlite.part";
    for side in ["", "-journal", "-wal", "-shm"] {
        std::fs::write(campaigns.join(format!("{part}{side}")), b"half").unwrap();
    }

    let s = session(dir.path(), &gm);
    let mut left: Vec<String> = std::fs::read_dir(&campaigns)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".part"))
        .collect();
    left.sort();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(s.list_campaigns(), vec![kept]);
}

#[test]
fn get_build_debug_refused_when_debug_off() {
    let rig = Rig::new();
    let id = rig.approved_draft();
    assert!(matches!(
        rig.drafts.get_build_debug(&id),
        Err(AppError::DebugOff)
    ));
    rig.session().set_debug(true);
    let lines = rig.drafts.get_build_debug(&id).unwrap();
    assert_eq!(lines.len(), 4, "one line per draft call");
    assert!(
        lines
            .iter()
            .all(|l| !l.prompt.is_empty() && !l.raw.is_empty())
    );

    rig.push(&build_outputs());
    rig.drafts.start_build(&id, &mut |_| {}).unwrap();
    // A build replaces the records of the draft.
    let lines = rig.drafts.get_build_debug(&id).unwrap();
    assert_eq!(lines.len(), 6, "one line per build call");

    rig.session().set_debug(false);
    assert!(matches!(
        rig.drafts.get_build_debug(&id),
        Err(AppError::DebugOff)
    ));
    rig.drafts.delete_draft(&id).unwrap();
    assert!(rig.drafts.list_drafts().unwrap().is_empty());
}

#[test]
fn draft_view_never_holds_hidden_content() {
    let rig = Rig::new();
    let id = rig.approved_draft();
    rig.push(&build_outputs());
    rig.drafts.start_build(&id, &mut |_| {}).unwrap();

    let view: DraftView = rig.drafts.get_draft(&id).unwrap();
    let text = serde_json::to_string(&view).unwrap();
    for hidden in HIDDEN_TEXT {
        assert!(!text.contains(hidden), "{hidden} in {text}");
    }
    let lines = serde_json::to_string(&rig.drafts.list_drafts().unwrap()).unwrap();
    for hidden in HIDDEN_TEXT {
        assert!(!lines.contains(hidden), "{hidden} in {lines}");
    }
}

#[test]
fn journal_marks_open_facts() {
    let rig = Rig::new();
    let id = rig.approved_draft();
    rig.push(&build_outputs());
    rig.drafts.start_build(&id, &mut |_| {}).unwrap();

    let journal = rig.session().get_journal().unwrap();
    let entries: Vec<_> = journal.observations.iter().chain(&journal.claims).collect();
    let vote = entries
        .iter()
        .find(|e| e.text.contains("new pier"))
        .expect("the PC knows the open item");
    assert!(vote.open);
    let job = entries
        .iter()
        .find(|e| e.text.contains("lighthouse keeper"))
        .expect("the PC knows the settled fact");
    assert!(!job.open);
    let texts: Vec<&str> = entries.iter().map(|e| e.text.as_str()).collect();
    for hidden in HIDDEN_TEXT {
        assert!(texts.iter().all(|t| !t.contains(hidden)), "{texts:?}");
    }
}
