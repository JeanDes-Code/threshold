//! The draft of QA run 5 (2026-11-16), replayed from the live model's
//! answers (`fixtures/qa-run5`, from `draft-1794858127245652288`). Two
//! bugs of that run:
//!
//! - The first Draft failed at the roster step, three runs in a row. The
//!   live model wrote the roster's keys in schema order (`characters`
//!   before `pc`), and listed the PC under `characters` as well.
//! - "The Lantern Inn stands in Ashford, beside the old well." did not
//!   back "the Lantern Inn inside Ashford": the sentence names a third
//!   Place.

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    fs,
    path::PathBuf,
};

use serde_json::Value;
use threshold_core::{
    Gm, GmError, Prompt, Stream,
    build::{Control, SeedDraft, draft_from_text},
};

/// A model that answers each draft stage from its own queue, so a test
/// does not depend on how many calls an earlier stage made. The stage is
/// read from the first words of the system prompt.
struct ReplayGm {
    answers: RefCell<HashMap<&'static str, VecDeque<String>>>,
    calls: RefCell<Vec<&'static str>>,
}

const STAGES: [(&str, &str); 4] = [
    ("roster", "You list the roster"),
    ("seed", "You read a Source Text"),
    ("knowledge", "You extract what the player character"),
    ("tree", "You state which place holds"),
];

impl ReplayGm {
    fn new(answers: &[(&'static str, Vec<Value>)]) -> Self {
        let answers = answers
            .iter()
            .map(|(stage, list)| (*stage, list.iter().map(Value::to_string).collect()))
            .collect();
        Self {
            answers: RefCell::new(answers),
            calls: RefCell::new(Vec::new()),
        }
    }

    /// The stage of each call, in order.
    fn calls(&self) -> Vec<&'static str> {
        self.calls.borrow().clone()
    }
}

impl Gm for ReplayGm {
    fn propose(&self, prompt: &Prompt, _schema: &Value) -> Result<String, GmError> {
        let (stage, _) = STAGES
            .iter()
            .find(|(_, head)| prompt.system.starts_with(head))
            .unwrap_or_else(|| panic!("no stage for {}", &prompt.system[..40]));
        self.calls.borrow_mut().push(stage);
        self.answers
            .borrow_mut()
            .get_mut(stage)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| GmError::Aborted(format!("no answer left for {stage}")))
    }

    fn narrate(
        &self,
        _prompt: &Prompt,
        _max_tokens: Option<u32>,
        _on_chunk: &mut dyn FnMut(&str) -> Stream,
    ) -> Result<String, GmError> {
        panic!("a draft never narrates")
    }
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/qa-run5")
}

/// A raw answer of the live model, as the debug log holds it.
fn answer(name: &str) -> Value {
    let path = dir().join(format!("{name}.json"));
    serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap()
}

fn source() -> String {
    fs::read_to_string(dir().join("source.txt")).unwrap()
}

fn draft(gm: &ReplayGm) -> Result<SeedDraft, threshold_core::build::BuildFailure> {
    let mut progress = |_| {};
    let mut log = |_| {};
    let mut ctl = Control::new(&mut progress, &|| false, &mut log);
    draft_from_text(&source(), gm, &mut ctl)
}

/// The run 5 answers after the roster: seed, known facts, and the place
/// tree with its repair.
fn after_roster() -> Vec<(&'static str, Vec<Value>)> {
    vec![
        ("seed", vec![answer("seed")]),
        ("knowledge", vec![answer("knowledge")]),
        (
            "tree",
            vec![answer("place-tree"), answer("place-tree-repair")],
        ),
    ]
}

/// QA run 5 (403): the live model listed the PC, "Bran Holt", both in `pc`
/// and in `characters`. The one-PC check sent a repair; a model that
/// repeats the copy failed the draft ("The draft stopped at the roster
/// step: the one-PC check failed."). A copy of the PC with no other name
/// is the PC: the draft drops it and goes on, with no repair call.
#[test]
fn a_roster_that_lists_the_pc_again_drafts_on_the_first_try() {
    let roster = answer("roster");
    assert_eq!(roster["characters"][0]["name"], "Bran Holt");
    let mut answers = vec![("roster", vec![roster.clone(), roster])];
    answers.extend(after_roster());
    let gm = ReplayGm::new(&answers);
    let draft = draft(&gm).expect("the run 5 roster drafts");
    assert_eq!(gm.calls().iter().filter(|s| **s == "roster").count(), 1);
    assert_eq!(draft.seed.pc.name, "Bran Holt");
    let names: Vec<&str> = draft
        .seed
        .entities
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert!(!names.contains(&"Bran Holt"), "{names:?}");
    for name in ["Wren Holt", "Ada Miller", "Pip Miller", "Old Tamsin"] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }
}
