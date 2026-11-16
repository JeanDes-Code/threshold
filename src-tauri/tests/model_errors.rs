//! A model error reaches the Player as plain copy (QA run 5, screens 439
//! and 455): never "HTTP 500", "timed out" or the server's message. The
//! raw error stays in the debug log of the Turn's operation.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

mod support;

use serde_json::Value;
use support::{allowed, town};
use threshold_app::session::{AppError, Session};
use threshold_core::{Gm, GmError, Prompt, RecordedGm, Stream, store::Store};

const SLOW: &str = "The game model took too long to answer. Try again.";
const DOWN: &str = "The game model is not responding. Check it is running, then try again.";

/// A `RecordedGm` that fails a call first when an error is queued for it.
#[derive(Clone)]
struct FailingGm {
    inner: RecordedGm,
    propose: Arc<Mutex<VecDeque<GmError>>>,
    narrate: Arc<Mutex<VecDeque<GmError>>>,
}

impl FailingGm {
    fn new(inner: RecordedGm) -> Self {
        Self {
            inner,
            propose: Arc::default(),
            narrate: Arc::default(),
        }
    }

    fn fail_propose(&self, e: GmError) {
        self.propose.lock().unwrap().push_back(e);
    }

    fn fail_narrate(&self, e: GmError) {
        self.narrate.lock().unwrap().push_back(e);
    }
}

impl Gm for FailingGm {
    fn propose(&self, prompt: &Prompt, schema: &Value) -> Result<String, GmError> {
        match self.propose.lock().unwrap().pop_front() {
            Some(e) => Err(e),
            None => self.inner.propose(prompt, schema),
        }
    }

    fn narrate(
        &self,
        prompt: &Prompt,
        max_tokens: Option<u32>,
        on_chunk: &mut dyn FnMut(&str) -> Stream,
    ) -> Result<String, GmError> {
        match self.narrate.lock().unwrap().pop_front() {
            Some(e) => Err(e),
            None => self.inner.narrate(prompt, max_tokens, on_chunk),
        }
    }
}

fn session(dir: &std::path::Path, gm: &FailingGm) -> Session {
    let gm = gm.clone();
    let mut s = Session::new(
        dir.to_path_buf(),
        Box::new(move || Box::new(gm.clone()) as Box<dyn Gm + Send>),
    );
    town(&mut s);
    s
}

fn text(e: AppError) -> String {
    e.to_string()
}

/// A server error with a body that echoes the prompt.
fn server_error() -> GmError {
    GmError::BadStatus(500, "model unloaded; prompt: the hidden key".into())
}

fn assert_plain(text: &str) {
    for raw in [
        "HTTP",
        "500",
        "timed out",
        "hidden key",
        "unloaded",
        "request",
    ] {
        assert!(!text.contains(raw), "{raw} in {text}");
    }
}

#[test]
fn a_failed_turn_shows_plain_copy_for_a_model_error() {
    let dir = tempfile::tempdir().unwrap();
    let gm = FailingGm::new(RecordedGm::new(vec![], vec![]));
    let mut s = session(dir.path(), &gm);
    for (error, copy) in [
        (GmError::Timeout, SLOW),
        (server_error(), DOWN),
        (GmError::Unreachable("connection refused".into()), DOWN),
    ] {
        gm.fail_propose(error);
        let err = text(
            s.submit_intent("op-1".into(), "I look around.", &mut |_| {})
                .unwrap_err(),
        );
        assert_eq!(err, copy);
        assert_plain(&err);
    }
}

/// The raw error stays in the debug log of the operation, for the debug
/// view: the Player copy drops it, the log does not.
#[test]
fn the_raw_model_error_stays_in_the_debug_log() {
    let dir = tempfile::tempdir().unwrap();
    let gm = FailingGm::new(RecordedGm::new(vec![], vec![]));
    let mut s = session(dir.path(), &gm);
    gm.fail_propose(server_error());
    s.submit_intent("op-9".into(), "I look around.", &mut |_| {})
        .unwrap_err();
    let db = s.summary().unwrap().db_path;
    let store = Store::open(std::path::Path::new(&db)).unwrap();
    let log = store.debug_log("op-9").unwrap();
    assert!(
        log.iter()
            .any(|e| e.raw.contains("HTTP 500") && e.raw.contains("model unloaded")),
        "{log:?}"
    );
}

#[test]
fn ask_the_gm_shows_plain_copy_for_a_model_error() {
    let dir = tempfile::tempdir().unwrap();
    let gm = FailingGm::new(RecordedGm::new(vec![], vec![]));
    let mut s = session(dir.path(), &gm);
    for (error, copy) in [(GmError::Timeout, SLOW), (server_error(), DOWN)] {
        gm.fail_propose(error.clone());
        gm.fail_narrate(error);
        let err = text(s.ask_ooc("Where is Ada?").unwrap_err());
        assert_eq!(err, copy);
    }
}

/// A narration that fails after the Commit, then "Narrate again" that
/// fails too: both say it in plain words.
#[test]
fn a_failed_narration_and_narrate_again_show_plain_copy() {
    let dir = tempfile::tempdir().unwrap();
    let gm = FailingGm::new(RecordedGm::new(vec![allowed()], vec![]));
    let mut s = session(dir.path(), &gm);
    gm.fail_narrate(GmError::Timeout);
    let turn = s
        .submit_intent("op-1".into(), "I look around.", &mut |_| {})
        .unwrap();
    assert_eq!(turn.narration, None);
    let notice = turn.narration_error.unwrap();
    assert_eq!(notice, SLOW);

    gm.fail_narrate(server_error());
    let err = text(s.renarrate(turn.turn_id, &mut |_| {}).unwrap_err());
    assert_eq!(err, DOWN);
    assert_plain(&err);
}
