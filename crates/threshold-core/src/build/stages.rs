//! The stage runner that every draft and World Build stage shares, and the
//! split of a Source Text into sections (data shape 2, design 6.3).
//!
//! A stage sends one schema-constrained call, checks the output in code,
//! and on a problem sends one repair call that carries the check names and
//! notes that code wrote. After the repair, an optional item that still
//! fails is dropped; an essential part that still fails fails the stage.

use std::{ops::Range, time::Instant};

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{BuildCheck, BuildFailure, Control, DebugEntry, Progress, Stage};
use crate::model::{Gm, Prompt};

/// A section shorter than this, in characters, merges into the next one.
pub(crate) const SECTION_MIN_CHARS: usize = 400;
/// The most characters one section call reads. A longer block is cut at a
/// paragraph break.
pub(crate) const SECTION_MAX_CHARS: usize = 6_000;

/// One schema-constrained call of a stage.
pub(crate) struct StageCall {
    pub stage: Stage,
    /// The debug label: the stage and the item, such as `draft_knowledge 2/5`.
    pub step: String,
    pub prompt: Prompt,
    pub schema: Value,
    /// What an output that stays unusable after the repair does.
    pub need: Need,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Need {
    /// The stage fails.
    Essential,
    /// The call yields nothing, and the stage goes on.
    Optional,
}

/// What a check found in a stage output.
pub(crate) struct Problem<I> {
    pub check: BuildCheck,
    /// `None`: an essential part of the output. `Some(item)`: an optional
    /// item, dropped when the repair does not fix it.
    pub item: Option<I>,
    /// Written by code, for the repair call and the debug log. It names a
    /// field, and at most a name; never the text or value of a fact, so a
    /// World Build note never carries a hidden fact.
    pub note: String,
}

impl<I> Problem<I> {
    pub fn essential(check: BuildCheck, note: impl Into<String>) -> Self {
        Self {
            check,
            item: None,
            note: note.into(),
        }
    }

    pub fn optional(check: BuildCheck, item: I, note: impl Into<String>) -> Self {
        Self {
            check,
            item: Some(item),
            note: note.into(),
        }
    }
}

/// A stage output that passed, and the optional items to drop from it.
pub(crate) struct Staged<T, I> {
    pub output: T,
    pub drop: Vec<I>,
}

/// Sends `call`, checks its output with `check`, and sends one repair call
/// when a check finds a problem. Returns `None` only for an `Optional` call
/// whose output stays unusable. Every call is logged under `op_id`.
pub(crate) fn run_stage<T, I>(
    gm: &dyn Gm,
    ctl: &mut Control,
    op_id: &str,
    call: &StageCall,
    check: impl Fn(&T) -> Vec<Problem<I>>,
) -> Result<Option<Staged<T, I>>, BuildFailure>
where
    T: DeserializeOwned,
{
    let first = attempt(gm, ctl, op_id, call, &call.prompt, &call.step, &check)?;
    if first.problems.is_empty() {
        return Ok(first.into_staged());
    }
    let repair = repair_prompt(&call.prompt, &first.problems);
    let step = format!("{} repair", call.step);
    let second = attempt(gm, ctl, op_id, call, &repair, &step, &check)?;

    // The repaired output wins unless it broke an essential part that the
    // first output had right.
    let failed = second.essential_failure();
    if failed.is_none() {
        return Ok(second.into_staged());
    }
    if first.essential_failure().is_none() {
        return Ok(first.into_staged());
    }
    match call.need {
        Need::Optional => Ok(None),
        Need::Essential => Err(BuildFailure {
            stage: call.stage,
            check: failed.unwrap_or(BuildCheck::Schema),
        }),
    }
}

/// Reports how far `stage` is.
pub(crate) fn report(ctl: &mut Control, stage: Stage, done: usize, total: usize) {
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    (ctl.progress)(Progress {
        stage,
        done: count(done),
        total: count(total),
    });
}

/// The JSON schema of a stage output, closed: each object takes only the
/// properties that its type names, and needs all of them. An `Option`
/// field stays nullable, so the model writes `null` instead of leaving it out.
pub(crate) fn schema_of<T: JsonSchema>() -> Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(T)).expect("a stage schema serializes");
    close(&mut schema);
    if let Some(defs) = schema.get_mut("$defs").and_then(Value::as_object_mut) {
        defs.values_mut().for_each(close);
    }
    schema
}

fn close(schema: &mut Value) {
    if schema.get("type") != Some(&json!("object")) {
        return;
    }
    schema["additionalProperties"] = json!(false);
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        let all: Vec<Value> = properties.keys().map(|k| json!(k)).collect();
        schema["required"] = Value::Array(all);
    }
}

/// One call and what its checks found.
struct Attempt<T, I> {
    /// `None` when the output does not parse.
    output: Option<T>,
    problems: Vec<Problem<I>>,
}

impl<T, I> Attempt<T, I> {
    fn essential_failure(&self) -> Option<BuildCheck> {
        self.problems
            .iter()
            .find(|p| p.item.is_none())
            .map(|p| p.check)
    }

    fn into_staged(self) -> Option<Staged<T, I>> {
        let drop = self.problems.into_iter().filter_map(|p| p.item).collect();
        self.output.map(|output| Staged { output, drop })
    }
}

fn attempt<T, I>(
    gm: &dyn Gm,
    ctl: &mut Control,
    op_id: &str,
    call: &StageCall,
    prompt: &Prompt,
    step: &str,
    check: &impl Fn(&T) -> Vec<Problem<I>>,
) -> Result<Attempt<T, I>, BuildFailure>
where
    T: DeserializeOwned,
{
    let fail = |check| BuildFailure {
        stage: call.stage,
        check,
    };
    if (ctl.cancelled)() {
        return Err(fail(BuildCheck::Cancelled));
    }
    let started = Instant::now();
    let answer = gm.propose(prompt, &call.schema);
    (ctl.log)(DebugEntry {
        op_id: op_id.to_owned(),
        step: step.to_owned(),
        prompt: render(prompt),
        raw: match &answer {
            Ok(raw) => raw.clone(),
            Err(error) => format!("model error: {error}"),
        },
        ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    });
    let raw = answer.map_err(|_| fail(BuildCheck::Model))?;
    Ok(match serde_json::from_str::<T>(&raw) {
        Ok(output) => Attempt {
            problems: check(&output),
            output: Some(output),
        },
        Err(_) => Attempt {
            output: None,
            problems: vec![Problem::essential(
                BuildCheck::Schema,
                "the answer is not JSON that matches the schema",
            )],
        },
    })
}

/// The prompt as the model reads it: the system message, then the user
/// message.
fn render(prompt: &Prompt) -> String {
    format!(
        "[system]\n{}{}{}\n[user]\n{}",
        prompt.system, prompt.stable, prompt.scene, prompt.recent
    )
}

/// The same prompt, with the problems that code found. It carries no text
/// of the failed answer.
fn repair_prompt<I>(prompt: &Prompt, problems: &[Problem<I>]) -> Prompt {
    let mut recent = prompt.recent.clone();
    recent.push_str("\n\nYOUR LAST ANSWER FAILED THESE CHECKS\n");
    for problem in problems {
        recent.push_str(&format!("- {:?}: {}\n", problem.check, problem.note));
    }
    recent.push_str("Write the whole answer again, and avoid these errors.");
    Prompt {
        recent,
        ..prompt.clone()
    }
}

/// The sections of `text`, in order: each Markdown heading starts a block;
/// a block shorter than `SECTION_MIN_CHARS` merges into the next one; a
/// block longer than `SECTION_MAX_CHARS` is cut at paragraph breaks. A text
/// with no heading is one block, so it is cut into windows.
pub(crate) fn sections(text: &str) -> Vec<&str> {
    let mut starts = vec![0];
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if offset > 0 && is_heading(line) {
            starts.push(offset);
        }
        offset += line.len();
    }
    starts.push(text.len());

    let mut blocks: Vec<Range<usize>> = Vec::new();
    let mut short_from: Option<usize> = None;
    for pair in starts.windows(2) {
        let block = short_from.take().unwrap_or(pair[0])..pair[1];
        if text[block.clone()].chars().count() < SECTION_MIN_CHARS {
            short_from = Some(block.start);
        } else {
            blocks.push(block);
        }
    }
    if let Some(start) = short_from {
        match blocks.last_mut() {
            Some(last) => last.end = text.len(),
            None => blocks.push(start..text.len()),
        }
    }

    blocks
        .into_iter()
        .flat_map(|block| windows(&text[block]))
        .filter(|section| !section.trim().is_empty())
        .collect()
}

/// An ATX heading: one to six `#` at the start of the line, then a space
/// or the end of the line.
fn is_heading(line: &str) -> bool {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(char::is_whitespace)
}

/// `block` cut into pieces of at most `SECTION_MAX_CHARS`, each cut at the
/// last paragraph break, else the last line break, in its second half.
fn windows(mut block: &str) -> Vec<&str> {
    let mut out = Vec::new();
    while let Some((limit, _)) = block.char_indices().nth(SECTION_MAX_CHARS) {
        let head = &block[..limit];
        let cut = [
            head.rfind("\n\n").map(|i| i + 2),
            head.rfind('\n').map(|i| i + 1),
        ]
        .into_iter()
        .flatten()
        .find(|&cut| cut >= limit / 2)
        .unwrap_or(limit);
        out.push(&block[..cut]);
        block = &block[cut..];
    }
    out.push(block);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> usize {
        s.chars().count()
    }

    fn paragraph(n: usize, len: usize) -> String {
        let mut p = format!("Paragraph {n}.");
        while chars(&p) < len {
            p.push_str(" Waves.");
        }
        p
    }

    #[test]
    fn each_heading_starts_a_section() {
        let a = format!("# One\n\n{}\n\n", paragraph(1, 500));
        let b = format!("## Two\n\n{}\n\n", paragraph(2, 500));
        let c = format!("### Three\n{}\n", paragraph(3, 500));
        let text = format!("{a}{b}{c}");
        assert_eq!(sections(&text), vec![a.as_str(), b.as_str(), c.as_str()]);
    }

    #[test]
    fn a_short_section_merges_into_the_next_one() {
        let intro = "A short preamble.\n\n";
        let a = "# One\nToo short.\n";
        let b = format!("# Two\n{}\n", paragraph(2, 500));
        let c = format!("# Three\n{}\n", paragraph(3, 500));
        let tail = "# Four\nShort tail.\n";
        let text = format!("{intro}{a}{b}{c}{tail}");
        assert_eq!(
            sections(&text),
            vec![format!("{intro}{a}{b}"), format!("{c}{tail}")]
        );
    }

    #[test]
    fn a_text_without_heading_is_cut_at_paragraph_breaks() {
        let paragraphs: Vec<String> = (0..15).map(|n| paragraph(n, 1_000)).collect();
        let text = paragraphs.join("\n\n");
        let found = sections(&text);
        assert!(found.len() >= 3, "{}", found.len());
        assert_eq!(found.concat(), text);
        for section in &found {
            assert!(chars(section) <= SECTION_MAX_CHARS);
            assert!(section.starts_with("Paragraph"), "{section}");
        }
    }

    #[test]
    fn a_long_heading_block_is_cut_too() {
        let body: Vec<String> = (0..10).map(|n| paragraph(n, 1_000)).collect();
        let text = format!("# Long\n\n{}", body.join("\n\n"));
        let found = sections(&text);
        assert!(found.len() >= 2);
        assert_eq!(found.concat(), text);
        assert!(found.iter().all(|s| chars(s) <= SECTION_MAX_CHARS));
    }

    #[test]
    fn a_text_without_any_break_is_cut_on_a_character_boundary() {
        let text = "é".repeat(SECTION_MAX_CHARS * 2 + 10);
        let found = sections(&text);
        assert_eq!(found.len(), 3);
        assert_eq!(found.concat(), text);
    }

    #[test]
    fn a_stage_schema_is_closed_and_needs_every_field() {
        #[derive(JsonSchema)]
        #[allow(dead_code)]
        struct Out {
            name: String,
            age: Option<u16>,
            inner: Vec<Inner>,
        }
        #[derive(JsonSchema)]
        #[allow(dead_code)]
        struct Inner {
            note: Option<String>,
        }
        let schema = schema_of::<Out>();
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["age", "inner", "name"]));
        assert_eq!(
            schema["properties"]["age"]["type"],
            json!(["integer", "null"])
        );
        let inner = &schema["$defs"]["Inner"];
        assert_eq!(inner["additionalProperties"], json!(false));
        assert_eq!(inner["required"], json!(["note"]));
    }

    #[test]
    fn a_hash_inside_a_line_is_no_heading() {
        let text = format!(
            "{}\n#hashtag and issue #4\n{}",
            paragraph(1, 500),
            paragraph(2, 500)
        );
        assert_eq!(sections(&text), vec![text.as_str()]);
    }
}
