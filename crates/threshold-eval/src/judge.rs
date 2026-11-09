//! The judge: a model that picks one answer from a fixed list.
//!
//! The default judge is the local game model itself, asked through the `Gm`
//! proposal call with a JSON schema that allows only the listed choices. A
//! frontier judge is not built in this slice; it would sit behind `--judge`
//! and stay out of the app crate.

use std::fmt;

use serde_json::json;
use threshold_core::{Gm, Prompt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JudgeError(pub String);

impl fmt::Display for JudgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "judge failed: {}", self.0)
    }
}

impl std::error::Error for JudgeError {}

pub trait Judge {
    /// Answers `task` with exactly one of `choices`.
    fn choose(&self, task: &str, choices: &[&str]) -> Result<String, JudgeError>;
}

pub const JUDGE_RULES: &str = "\
You evaluate the output of a game master for a single-player tabletop role-playing game.
Read the task carefully. Judge only what the text says, not what it might imply.
Answer with exactly one of the allowed choices, as JSON: {\"answer\": \"<choice>\"}.
";

/// A judge that asks a `Gm` (the local Ollama model by default).
pub struct GmJudge<G> {
    gm: G,
}

impl<G: Gm> GmJudge<G> {
    pub fn new(gm: G) -> Self {
        Self { gm }
    }
}

impl<G: Gm> Judge for GmJudge<G> {
    fn choose(&self, task: &str, choices: &[&str]) -> Result<String, JudgeError> {
        let prompt = Prompt {
            system: JUDGE_RULES.to_owned(),
            stable: String::new(),
            scene: String::new(),
            recent: format!("TASK\n{task}\n\nAllowed choices: {}\n", choices.join(", ")),
        };
        let schema = json!({
            "type": "object",
            "properties": { "answer": { "type": "string", "enum": choices } },
            "required": ["answer"],
            "additionalProperties": false,
        });
        let raw = self
            .gm
            .propose(&prompt, &schema)
            .map_err(|e| JudgeError(e.to_string()))?;
        let value: serde_json::Value = serde_json::from_str(raw.trim())
            .map_err(|e| JudgeError(format!("invalid JSON: {e}")))?;
        let answer = value
            .get("answer")
            .and_then(|a| a.as_str())
            .ok_or_else(|| JudgeError("no answer field".into()))?;
        choices
            .iter()
            .find(|c| c.eq_ignore_ascii_case(answer.trim()))
            .map(|c| (*c).to_owned())
            .ok_or_else(|| JudgeError(format!("answer {answer:?} is not an allowed choice")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use threshold_core::RecordedGm;

    #[test]
    fn gm_judge_returns_the_allowed_choice() {
        let gm = RecordedGm::new(vec![r#"{"answer":"yes"}"#.into()], vec![]);
        let judge = GmJudge::new(gm);
        assert_eq!(judge.choose("Is it?", &["yes", "no"]).unwrap(), "yes");
    }

    #[test]
    fn gm_judge_rejects_an_answer_outside_the_list() {
        let gm = RecordedGm::new(vec![r#"{"answer":"maybe"}"#.into()], vec![]);
        let judge = GmJudge::new(gm);
        assert!(judge.choose("Is it?", &["yes", "no"]).is_err());
    }
}
