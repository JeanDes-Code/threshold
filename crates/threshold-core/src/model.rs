use std::{collections::VecDeque, error::Error, fmt, sync::Mutex};

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
}

impl fmt::Display for GmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(message) => write!(f, "game model unreachable: {message}"),
            Self::Timeout => f.write_str("game model request timed out"),
            Self::BadStatus(status, message) => {
                write!(f, "game model returned HTTP {status}: {message}")
            }
        }
    }
}

impl Error for GmError {}

pub trait Gm {
    fn propose(&self, prompt: &Prompt, schema: &serde_json::Value) -> Result<String, GmError>;
    fn narrate(&self, prompt: &Prompt, on_chunk: &mut dyn FnMut(&str)) -> Result<String, GmError>;
}

#[derive(Debug)]
pub struct RecordedGm {
    proposals: Mutex<VecDeque<String>>,
    narrations: Mutex<VecDeque<String>>,
}

impl RecordedGm {
    pub fn new(proposals: Vec<String>, narrations: Vec<String>) -> Self {
        Self {
            proposals: Mutex::new(proposals.into()),
            narrations: Mutex::new(narrations.into()),
        }
    }
}

impl Gm for RecordedGm {
    fn propose(&self, _prompt: &Prompt, _schema: &serde_json::Value) -> Result<String, GmError> {
        self.proposals
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .ok_or_else(|| GmError::Unreachable("no recorded proposal remains".into()))
    }

    fn narrate(&self, _prompt: &Prompt, on_chunk: &mut dyn FnMut(&str)) -> Result<String, GmError> {
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
