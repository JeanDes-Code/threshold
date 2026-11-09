use std::{
    error::Error,
    fmt,
    io::{self, BufRead, BufReader},
    sync::Mutex,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use threshold_core::{Gm, GmError, Prompt};
use ureq::{Agent, Response};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OllamaConfig {
    pub base_url: String,
    pub model: String,
    pub num_ctx: u32,
    pub keep_alive: String,
    pub timeout_s: u64,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".into(),
            model: "gemma4:26b-a4b-it-qat".into(),
            num_ctx: 16_384,
            keep_alive: "30m".into(),
            timeout_s: 60,
        }
    }
}

impl OllamaConfig {
    pub fn from_toml(input: &str) -> Result<Self, ConfigError> {
        toml::from_str(input).map_err(ConfigError)
    }
}

#[derive(Debug)]
pub struct ConfigError(toml::de::Error);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallMetrics {
    pub prompt_tokens: u64,
    pub eval_tokens: u64,
    pub eval_duration_ns: u64,
    pub prompt_eval_duration_ns: u64,
    pub total_ms: u64,
}

#[derive(Debug)]
pub struct OllamaGm {
    config: OllamaConfig,
    agent: Agent,
    last_metrics: Mutex<Option<CallMetrics>>,
}

impl OllamaGm {
    pub fn new(config: OllamaConfig) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(config.timeout_s))
            .build();
        Self {
            config,
            agent,
            last_metrics: Mutex::new(None),
        }
    }

    pub fn warm_up(&self) -> Result<(), GmError> {
        let body = json!({
            "model": self.config.model,
            "prompt": "",
            "stream": false,
            "think": false,
            "options": { "num_ctx": self.config.num_ctx },
            "keep_alive": self.config.keep_alive,
        });
        let response = self.post("/api/generate", &body)?;
        response.into_string().map_err(map_io_error)?;
        Ok(())
    }

    pub fn last_metrics(&self) -> Option<CallMetrics> {
        *self
            .last_metrics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn post(&self, path: &str, body: &Value) -> Result<Response, GmError> {
        let url = format!("{}{}", self.config.base_url.trim_end_matches('/'), path);
        self.agent
            .post(&url)
            .send_json(body)
            .map_err(map_ureq_error)
    }

    fn chat_body(&self, prompt: &Prompt, stream: bool, schema: Option<&Value>) -> Value {
        let mut body = json!({
            "model": self.config.model,
            "messages": [
                {
                    "role": "system",
                    "content": format!("{}{}{}", prompt.system, prompt.stable, prompt.scene),
                },
                { "role": "user", "content": prompt.recent },
            ],
            "stream": stream,
            "think": false,
            "options": { "num_ctx": self.config.num_ctx },
            "keep_alive": self.config.keep_alive,
        });
        if let Some(schema) = schema {
            body["format"] = schema.clone();
        }
        body
    }

    fn clear_metrics(&self) {
        *self
            .last_metrics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    fn record_metrics(&self, response: &ChatResponse) {
        if response.prompt_eval_count.is_some()
            || response.eval_count.is_some()
            || response.total_duration.is_some()
        {
            *self
                .last_metrics
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(CallMetrics {
                prompt_tokens: response.prompt_eval_count.unwrap_or(0),
                eval_tokens: response.eval_count.unwrap_or(0),
                eval_duration_ns: response.eval_duration.unwrap_or(0),
                prompt_eval_duration_ns: response.prompt_eval_duration.unwrap_or(0),
                total_ms: response.total_duration.unwrap_or(0) / 1_000_000,
            });
        }
    }
}

impl Gm for OllamaGm {
    fn propose(&self, prompt: &Prompt, schema: &Value) -> Result<String, GmError> {
        self.clear_metrics();
        let body = self.chat_body(prompt, false, Some(schema));
        let response = self.post("/api/chat", &body)?;
        let status = response.status();
        let raw = response.into_string().map_err(map_io_error)?;
        let parsed = parse_chat(&raw, status)?;
        self.record_metrics(&parsed);
        Ok(parsed.message.content)
    }

    fn narrate(&self, prompt: &Prompt, on_chunk: &mut dyn FnMut(&str)) -> Result<String, GmError> {
        self.clear_metrics();
        let body = self.chat_body(prompt, true, None);
        let response = self.post("/api/chat", &body)?;
        let status = response.status();
        let mut narration = String::new();
        for line in BufReader::new(response.into_reader()).lines() {
            let line = line.map_err(map_io_error)?;
            if line.is_empty() {
                continue;
            }
            let chunk = parse_chat(&line, status)?;
            self.record_metrics(&chunk);
            on_chunk(&chunk.message.content);
            narration.push_str(&chunk.message.content);
        }
        Ok(narration)
    }

    fn last_prompt_tokens(&self) -> Option<u64> {
        self.last_metrics().map(|metrics| metrics.prompt_tokens)
    }
}

/// One response body or stream line. An `{"error": ...}` object is the
/// server reporting a failure after it answered (Ollama does this mid-stream
/// when the runner stops), so it maps to `GmError::Aborted`.
fn parse_chat(raw: &str, status: u16) -> Result<ChatResponse, GmError> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|error| GmError::BadStatus(status, format!("invalid response: {error}")))?;
    if let Some(error) = value.get("error") {
        let message = error
            .as_str()
            .map_or_else(|| error.to_string(), str::to_owned);
        return Err(GmError::Aborted(message));
    }
    serde_json::from_value(value)
        .map_err(|error| GmError::BadStatus(status, format!("invalid response: {error}")))
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: ChatMessage,
    prompt_eval_count: Option<u64>,
    eval_count: Option<u64>,
    eval_duration: Option<u64>,
    prompt_eval_duration: Option<u64>,
    total_duration: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

fn map_ureq_error(error: ureq::Error) -> GmError {
    match error {
        ureq::Error::Status(status, response) => {
            let message = response
                .into_string()
                .unwrap_or_else(|error| format!("could not read error response: {error}"));
            GmError::BadStatus(status, message)
        }
        ureq::Error::Transport(transport) => map_transport_error(&transport),
    }
}

fn map_transport_error(error: &ureq::Transport) -> GmError {
    if error.source().is_some_and(is_timeout_source) {
        GmError::Timeout
    } else {
        GmError::Unreachable(error.to_string())
    }
}

fn map_io_error(error: io::Error) -> GmError {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        GmError::Timeout
    } else {
        GmError::Unreachable(error.to_string())
    }
}

fn is_timeout_source(source: &(dyn Error + 'static)) -> bool {
    source.downcast_ref::<io::Error>().is_some_and(|error| {
        matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        )
    })
}
