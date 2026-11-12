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
    /// The caller cancelled the call before it ended.
    Cancelled,
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
            Self::Cancelled => f.write_str("game model call cancelled"),
        }
    }
}

impl Error for GmError {}

/// What the model server reported for one call (bug pass 5, M4). With a
/// prompt cache, `prompt_tokens` counts only the tokens the server read
/// fresh: Ollama's `prompt_eval_count` leaves out the cached prefix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CallStats {
    /// Prompt tokens read fresh (`prompt_eval_count`).
    pub prompt_tokens: u64,
    /// Time to read them (`prompt_eval_duration`), in milliseconds.
    pub prompt_ms: u64,
    /// Tokens generated (`eval_count`).
    pub eval_tokens: u64,
    /// Time to generate them (`eval_duration`), in milliseconds.
    pub eval_ms: u64,
}

impl fmt::Display for CallStats {
    /// One line of `key=value` pairs, for the debug log's `metrics` rows.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "prompt_eval_count={} prompt_eval_ms={} eval_count={} eval_ms={}",
            self.prompt_tokens, self.prompt_ms, self.eval_tokens, self.eval_ms
        )
    }
}

impl CallStats {
    /// Reads a line that `Display` wrote; `None` when a field is missing.
    pub fn parse(line: &str) -> Option<Self> {
        let field = |name: &str| -> Option<u64> {
            line.split_whitespace()
                .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))?
                .parse()
                .ok()
        };
        Some(Self {
            prompt_tokens: field("prompt_eval_count")?,
            prompt_ms: field("prompt_eval_ms")?,
            eval_tokens: field("eval_count")?,
            eval_ms: field("eval_ms")?,
        })
    }
}

/// What the reader of a narration stream wants after a chunk (data shape
/// 3.6): the next chunk, or no more text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    More,
    /// The adapter stops reading, drops the response and returns the text
    /// received so far.
    Stop,
}

pub trait Gm {
    fn propose(&self, prompt: &Prompt, schema: &serde_json::Value) -> Result<String, GmError>;
    /// One streamed narration call. `max_tokens` caps the generated tokens
    /// when set. After each chunk, `on_chunk` says whether to read on; on
    /// `Stream::Stop` the call returns the text received so far.
    fn narrate(
        &self,
        prompt: &Prompt,
        max_tokens: Option<u32>,
        on_chunk: &mut dyn FnMut(&str) -> Stream,
    ) -> Result<String, GmError>;
    /// The prompt tokens the model reported for the last call, if the adapter
    /// knows them.
    fn last_prompt_tokens(&self) -> Option<u64> {
        None
    }
    /// The server's numbers for the last call, if the adapter knows them.
    /// The engine writes them to the debug log (step `metrics`).
    fn last_call_stats(&self) -> Option<CallStats> {
        None
    }
    /// Why the model stopped generating in the last call (`stop`,
    /// `length`), if the adapter knows. The engine logs any reason other
    /// than `stop` with the raw proposal, so a cut answer shows its cause.
    fn last_done_reason(&self) -> Option<String> {
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
    /// The JSON schema of a `propose` call; `Null` for `narrate`.
    pub schema: serde_json::Value,
    /// The token cap of a `narrate` call; `None` for `propose`.
    pub max_tokens: Option<u32>,
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

    fn record(
        &self,
        kind: CallKind,
        prompt: &Prompt,
        schema: &serde_json::Value,
        max_tokens: Option<u32>,
    ) {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(RecordedCall {
                kind,
                prompt: prompt.clone(),
                schema: schema.clone(),
                max_tokens,
            });
    }
}

impl Gm for RecordedGm {
    fn propose(&self, prompt: &Prompt, schema: &serde_json::Value) -> Result<String, GmError> {
        self.record(CallKind::Propose, prompt, schema, None);
        self.proposals
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .ok_or_else(|| GmError::Unreachable("no recorded proposal remains".into()))
    }

    fn narrate(
        &self,
        prompt: &Prompt,
        max_tokens: Option<u32>,
        on_chunk: &mut dyn FnMut(&str) -> Stream,
    ) -> Result<String, GmError> {
        self.record(
            CallKind::Narrate,
            prompt,
            &serde_json::Value::Null,
            max_tokens,
        );
        let narration = self
            .narrations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .ok_or_else(|| GmError::Unreachable("no recorded narration remains".into()))?;
        let _ = on_chunk(&narration);
        Ok(narration)
    }
}
