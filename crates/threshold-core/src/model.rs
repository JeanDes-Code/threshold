use std::{
    collections::VecDeque,
    error::Error,
    fmt,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prompt {
    pub system: String,
    pub stable: String,
    pub scene: String,
    pub recent: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GmError {
    Unreachable(String),
    Timeout,
    BadStatus(u16, String),
    /// The server answered, then reported an error in the body (a
    /// mid-stream `{"error": ...}` line, for example).
    Aborted(String),
}

impl fmt::Display for GmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(message) => write!(f, "game model unreachable: {message}"),
            Self::Timeout => f.write_str("game model request timed out"),
            Self::BadStatus(status, message) => {
                write!(f, "game model returned HTTP {status}: {message}")
            }
            Self::Aborted(message) => write!(f, "game model stopped: {message}"),
        }
    }
}

impl Error for GmError {}

pub trait Gm {
    fn propose(&self, prompt: &Prompt, schema: &serde_json::Value) -> Result<String, GmError>;
    fn narrate(&self, prompt: &Prompt, on_chunk: &mut dyn FnMut(&str)) -> Result<String, GmError>;
    /// The prompt tokens the model reported for the last call, if the adapter
    /// knows them.
    fn last_prompt_tokens(&self) -> Option<u64> {
        None
    }
}

/// Which `Gm` method a recorded call went to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallKind {
    Propose,
    Narrate,
}

/// One call a `RecordedGm` received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedCall {
    pub kind: CallKind,
    pub prompt: Prompt,
}

/// A `Gm` that replays recorded responses in order and records every call.
/// Clones share the same queues and call log, so a test can keep one clone
/// and hand another to the engine.
#[derive(Clone, Debug)]
pub struct RecordedGm {
    proposals: Arc<Mutex<VecDeque<String>>>,
    narrations: Arc<Mutex<VecDeque<String>>>,
    calls: Arc<Mutex<Vec<RecordedCall>>>,
}

impl RecordedGm {
    pub fn new(proposals: Vec<String>, narrations: Vec<String>) -> Self {
        Self {
            proposals: Arc::new(Mutex::new(proposals.into())),
            narrations: Arc::new(Mutex::new(narrations.into())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Queues one more proposal, after the recorded ones. A test uses it when
    /// a proposal needs a handle that exists only after earlier Turns.
    pub fn push_proposal(&self, raw: String) {
        self.proposals
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back(raw);
    }

    /// Queues one more narration, after the recorded ones.
    pub fn push_narration(&self, raw: String) {
        self.narrations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back(raw);
    }

    /// Every call received so far, in order.
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn record(&self, kind: CallKind, prompt: &Prompt) {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(RecordedCall {
                kind,
                prompt: prompt.clone(),
            });
    }
}

impl Gm for RecordedGm {
    fn propose(&self, prompt: &Prompt, _schema: &serde_json::Value) -> Result<String, GmError> {
        self.record(CallKind::Propose, prompt);
        self.proposals
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .ok_or_else(|| GmError::Unreachable("no recorded proposal remains".into()))
    }

    fn narrate(&self, prompt: &Prompt, on_chunk: &mut dyn FnMut(&str)) -> Result<String, GmError> {
        self.record(CallKind::Narrate, prompt);
        let narration = self
            .narrations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .ok_or_else(|| GmError::Unreachable("no recorded narration remains".into()))?;
        on_chunk(&narration);
        Ok(narration)
    }
}
