//! The stage runner that every draft and World Build stage shares, and the
//! split of a Source Text into sections (data shape 2, design 6.3).
//!
//! A stage sends one schema-constrained call, checks the output in code,
//! and on a problem sends one repair call that carries the check names and
//! notes that code wrote. After the repair, an optional item that still
//! fails is dropped; an essential part that still fails fails the stage.
//! The runner applies in code the list and text caps that a stage schema
//! declares: a list is cut to its cap and reported, a text above its cap is
//! a `Schema` problem of its item.

use std::{ops::Range, time::Instant};

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{BuildCheck, BuildFailure, Control, DebugEntry, Progress, Stage};
use crate::model::{Gm, GmError, Prompt};

/// How many times a call that timed out is sent again, with the same
/// prompt, before the stage fails as `Model`. About one live call in ten
/// times out. Any other model error fails the stage at once.
pub(crate) const TIMEOUT_RETRIES: usize = 2;

/// The model timeout of draft and build calls, in seconds, that the app and
/// the evaluation set. They are much slower than a Turn: one spike call took
/// 117 s. A timeout is retried `TIMEOUT_RETRIES` times, so one stalled call
/// waits 12 minutes at most, and a Cancel waits for the running call.
pub const BUILD_TIMEOUT_S: u64 = 240;

/// The longest raw answer, in bytes, that a stage reads. The schema caps
/// every list and text, but only the model server reads it; a longer answer
/// fails as `Schema` before it is parsed.
pub(crate) const RAW_ANSWER_MAX_BYTES: usize = 200_000;

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

/// One step of a path into a stage output: a field name or a list index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Field(String),
    Index(usize),
}

/// The item of a stage output that a problem at a path drops. `None`: the
/// path is in an essential part, so the problem is essential.
pub(crate) trait StageItem: Sized {
    fn at(path: &[Step]) -> Option<Self>;
}

/// A stage with no optional item: every problem is essential.
impl StageItem for () {
    fn at(_: &[Step]) -> Option<Self> {
        None
    }
}

/// A stage whose items are the entries of its one top-level list, by
/// index: `facts[3].text` is item 3.
impl StageItem for usize {
    fn at(path: &[Step]) -> Option<Self> {
        match path {
            [Step::Field(_), Step::Index(i), ..] => Some(*i),
            _ => None,
        }
    }
}

/// A path as a note shows it: `facts[3].text`.
pub(crate) fn show(path: &[Step]) -> String {
    let mut out = String::new();
    for step in path {
        match step {
            Step::Field(name) if out.is_empty() => out.push_str(name),
            Step::Field(name) => out.push_str(&format!(".{name}")),
            Step::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    out
}

/// A path into a stage output and a cap.
pub(crate) type CapAt = (Vec<Step>, u64);

/// What `apply_caps` found: each text above its cap, and each list it cut.
#[derive(Default)]
struct Caps {
    long: Vec<CapAt>,
    cut: Vec<CapAt>,
}

/// Applies the caps that `schema` declares to `value`, as the model server
/// does when it reads the schema; serde ignores them. A list above its
/// `maxItems` is cut to that length and returned with its path and its cap.
/// Each text above its `maxLength`, in characters, is returned with its path
/// and its cap.
fn apply_caps(schema: &Value, value: &mut Value) -> Caps {
    let mut caps = Caps::default();
    walk_caps(schema, schema, value, &mut vec![], &mut caps);
    caps
}

fn walk_caps(root: &Value, node: &Value, value: &mut Value, path: &mut Vec<Step>, caps: &mut Caps) {
    if let Some(def) = node
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix('#'))
        .and_then(|pointer| root.pointer(pointer))
    {
        walk_caps(root, def, value, path, caps);
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        for branch in node
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            walk_caps(root, branch, value, path, caps);
        }
    }
    match value {
        Value::String(text) => {
            if let Some(max) = node.get("maxLength").and_then(Value::as_u64)
                && text.chars().count() as u64 > max
            {
                caps.long.push((path.clone(), max));
            }
        }
        Value::Array(list) => {
            if let Some(max) = node.get("maxItems").and_then(Value::as_u64)
                && list.len() as u64 > max
            {
                list.truncate(usize::try_from(max).unwrap_or(usize::MAX));
                caps.cut.push((path.clone(), max));
            }
            if let Some(items) = node.get("items").filter(|i| i.is_object()) {
                for (i, item) in list.iter_mut().enumerate() {
                    path.push(Step::Index(i));
                    walk_caps(root, items, item, path, caps);
                    path.pop();
                }
            }
        }
        Value::Object(fields) => {
            for (name, sub) in node
                .get("properties")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                if let Some(field) = fields.get_mut(name) {
                    path.push(Step::Field(name.clone()));
                    walk_caps(root, sub, field, path, caps);
                    path.pop();
                }
            }
        }
        _ => {}
    }
}

/// A stage output that passed, the optional items to drop from it, and the
/// lists the runner cut at their cap.
pub(crate) struct Staged<T, I> {
    pub output: T,
    pub drop: Vec<I>,
    pub cut: Vec<CapAt>,
}

/// Sends `call`, checks its output with `check`, and sends one repair call
/// when a check finds a problem. Returns `None` only for an `Optional` call
/// whose output stays unusable. A call that times out is sent again, up to
/// `TIMEOUT_RETRIES` times. Every call is logged under `op_id`.
pub(crate) fn run_stage<T, I>(
    gm: &dyn Gm,
    ctl: &mut Control,
    op_id: &str,
    call: &StageCall,
    check: impl Fn(&T) -> Vec<Problem<I>>,
) -> Result<Option<Staged<T, I>>, BuildFailure>
where
    T: DeserializeOwned,
    I: StageItem,
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

/// The output of an essential call. `run_stage` returns `None` only for an
/// optional call, so the fallback never fires.
pub(crate) fn essential<T, I>(
    stage: Stage,
    staged: Option<Staged<T, I>>,
) -> Result<Staged<T, I>, BuildFailure> {
    staged.ok_or(BuildFailure {
        stage,
        check: BuildCheck::Schema,
    })
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

/// Constrains the string at `pointer` of a stage schema to one of `names`,
/// built per call from the roster, as the Turn pipeline does for handles
/// (`proposal_schema_for`). `nullable` adds `null`. With no name, only the
/// empty string fits, or only `null`.
pub(crate) fn name_field(schema: &mut Value, pointer: &str, names: &[&str], nullable: bool) {
    let mut values = name_values(names);
    *node(schema, pointer) = if nullable {
        values.push(Value::Null);
        json!({ "enum": values })
    } else {
        if values.is_empty() {
            values.push(json!(""));
        }
        json!({ "type": "string", "enum": values })
    };
}

/// Constrains each item of the list at `pointer` of a stage schema to one
/// of `names`. With no name, the list stays empty.
pub(crate) fn name_list(schema: &mut Value, pointer: &str, names: &[&str]) {
    let list = node(schema, pointer);
    let values = name_values(names);
    if values.is_empty() {
        list["maxItems"] = json!(0);
        list["items"] = json!({ "type": "string", "enum": [""] });
    } else {
        list["items"] = json!({ "type": "string", "enum": values });
    }
}

/// Each non-empty name once, in order.
fn name_values(names: &[&str]) -> Vec<Value> {
    let mut seen = Vec::new();
    for name in names {
        if !name.trim().is_empty() && !seen.contains(name) {
            seen.push(*name);
        }
    }
    seen.into_iter().map(|n| json!(n)).collect()
}

fn node<'a>(schema: &'a mut Value, pointer: &str) -> &'a mut Value {
    schema
        .pointer_mut(pointer)
        .unwrap_or_else(|| panic!("a stage schema has {pointer}"))
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
    cut: Vec<CapAt>,
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
        let cut = self.cut;
        self.output.map(|output| Staged { output, drop, cut })
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
    I: StageItem,
{
    let fail = |check| BuildFailure {
        stage: call.stage,
        check,
    };
    let mut retries = 0;
    let answer = loop {
        if (ctl.cancelled)() {
            return Err(fail(BuildCheck::Cancelled));
        }
        let started = Instant::now();
        let answer = gm.propose(prompt, &call.schema);
        (ctl.log)(DebugEntry {
            op_id: op_id.to_owned(),
            step: if retries == 0 {
                step.to_owned()
            } else {
                format!("{step} retry {retries}")
            },
            prompt: render(prompt),
            raw: match &answer {
                Ok(raw) => raw.clone(),
                Err(error) => format!("model error: {error}"),
            },
            ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        match answer {
            Err(GmError::Timeout) if retries < TIMEOUT_RETRIES => retries += 1,
            answer => break answer,
        }
    };
    let raw = answer.map_err(|_| fail(BuildCheck::Model))?;
    if raw.len() > RAW_ANSWER_MAX_BYTES {
        return Ok(Attempt {
            output: None,
            problems: vec![Problem::essential(
                BuildCheck::Schema,
                "the answer is far longer than the schema allows",
            )],
            cut: vec![],
        });
    }
    let parsed = serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|mut value| {
            let caps = apply_caps(&call.schema, &mut value);
            serde_json::from_value::<T>(value)
                .ok()
                .map(|out| (out, caps))
        });
    Ok(match parsed {
        Some((output, caps)) => {
            let mut problems: Vec<Problem<I>> = caps
                .long
                .into_iter()
                .map(|(path, max)| {
                    let note = format!("{} is longer than {max} characters", show(&path));
                    match I::at(&path) {
                        Some(item) => Problem::optional(BuildCheck::Schema, item, note),
                        None => Problem::essential(BuildCheck::Schema, note),
                    }
                })
                .collect();
            problems.extend(check(&output));
            Attempt {
                problems,
                output: Some(output),
                cut: caps.cut,
            }
        }
        None => Attempt {
            output: None,
            problems: vec![Problem::essential(
                BuildCheck::Schema,
                "the answer is not JSON that matches the schema",
            )],
            cut: vec![],
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

    /// A `Gm` that answers from a script of results, in order.
    struct ScriptedGm(std::sync::Mutex<std::collections::VecDeque<Result<String, GmError>>>);

    impl ScriptedGm {
        fn new(answers: Vec<Result<String, GmError>>) -> Self {
            Self(std::sync::Mutex::new(answers.into()))
        }

        fn left(&self) -> usize {
            self.0.lock().unwrap().len()
        }
    }

    impl Gm for ScriptedGm {
        fn propose(&self, _: &Prompt, _: &Value) -> Result<String, GmError> {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(GmError::Unreachable("script is empty".into())))
        }

        fn narrate(&self, _: &Prompt, _: &mut dyn FnMut(&str)) -> Result<String, GmError> {
            unreachable!("a stage never narrates")
        }
    }

    #[derive(serde::Deserialize)]
    struct Items {
        items: Vec<u8>,
    }

    /// Item 0 must be 1. Any other item above 1 is an optional problem.
    fn check_items(out: &Items) -> Vec<Problem<usize>> {
        let mut problems = vec![];
        if out.items.first() != Some(&1) {
            problems.push(Problem::essential(BuildCheck::Completeness, "items[0]"));
        }
        for (i, item) in out.items.iter().enumerate().skip(1) {
            if *item > 1 {
                problems.push(Problem::optional(BuildCheck::References, i, "item"));
            }
        }
        problems
    }

    fn call(need: Need) -> StageCall {
        StageCall {
            stage: Stage::HiddenFacts,
            step: "hidden_facts 1/1".into(),
            prompt: Prompt::default(),
            schema: json!({}),
            need,
        }
    }

    type Outcome = Result<Option<Staged<Items, usize>>, BuildFailure>;

    /// Runs `call(need)` against `gm` and returns the result and the
    /// logged steps.
    fn run(gm: &ScriptedGm, need: Need) -> (Outcome, Vec<String>) {
        let mut steps = vec![];
        let result = {
            let mut progress = |_| {};
            let mut log = |e: DebugEntry| steps.push(e.step);
            let mut ctl = Control {
                progress: &mut progress,
                cancelled: &|| false,
                log: &mut log,
            };
            run_stage(gm, &mut ctl, "draft-1", &call(need), check_items)
        };
        (result, steps)
    }

    fn ok(items: &str) -> Result<String, GmError> {
        Ok(format!("{{\"items\": {items}}}"))
    }

    #[test]
    fn one_stalled_call_waits_twelve_minutes_at_most() {
        // Two retries of a call that times out each time: three timeouts.
        assert_eq!(BUILD_TIMEOUT_S, 240);
        assert_eq!(TIMEOUT_RETRIES, 2);
        assert_eq!(BUILD_TIMEOUT_S * (TIMEOUT_RETRIES as u64 + 1), 12 * 60);
    }

    #[test]
    fn a_timeout_is_retried_twice_with_the_same_prompt() {
        let gm = ScriptedGm::new(vec![
            Err(GmError::Timeout),
            Err(GmError::Timeout),
            ok("[1]"),
        ]);
        let (result, steps) = run(&gm, Need::Essential);
        assert_eq!(result.unwrap().unwrap().output.items, vec![1]);
        assert_eq!(
            steps,
            vec![
                "hidden_facts 1/1",
                "hidden_facts 1/1 retry 1",
                "hidden_facts 1/1 retry 2",
            ]
        );
    }

    #[test]
    fn a_third_timeout_fails_the_stage_as_model() {
        let gm = ScriptedGm::new(vec![
            Err(GmError::Timeout),
            Err(GmError::Timeout),
            Err(GmError::Timeout),
            ok("[1]"),
        ]);
        let (result, steps) = run(&gm, Need::Optional);
        assert_eq!(
            result.err(),
            Some(BuildFailure {
                stage: Stage::HiddenFacts,
                check: BuildCheck::Model
            })
        );
        assert_eq!(steps.len(), 3);
        assert_eq!(gm.left(), 1);
    }

    #[test]
    fn another_model_error_fails_at_once() {
        let gm = ScriptedGm::new(vec![Err(GmError::BadStatus(500, "x".into())), ok("[1]")]);
        let (result, steps) = run(&gm, Need::Essential);
        assert_eq!(result.err().map(|f| f.check), Some(BuildCheck::Model));
        assert_eq!(steps.len(), 1);
        assert_eq!(gm.left(), 1);
    }

    #[test]
    fn a_repair_call_is_retried_on_timeout_too() {
        let gm = ScriptedGm::new(vec![ok("[0]"), Err(GmError::Timeout), ok("[1]")]);
        let (result, steps) = run(&gm, Need::Essential);
        assert_eq!(result.unwrap().unwrap().output.items, vec![1]);
        assert_eq!(
            steps,
            vec![
                "hidden_facts 1/1",
                "hidden_facts 1/1 repair",
                "hidden_facts 1/1 repair retry 1",
            ]
        );
    }

    #[test]
    fn cancel_stops_a_retry() {
        let gm = ScriptedGm::new(vec![Err(GmError::Timeout), ok("[1]")]);
        let calls = std::cell::Cell::new(0);
        let mut progress = |_| {};
        let mut log = |_| calls.set(calls.get() + 1);
        let mut ctl = Control {
            progress: &mut progress,
            cancelled: &|| calls.get() > 0,
            log: &mut log,
        };
        let result = run_stage(&gm, &mut ctl, "d", &call(Need::Essential), check_items);
        assert_eq!(result.err().map(|f| f.check), Some(BuildCheck::Cancelled));
        assert_eq!(gm.left(), 1);
    }

    #[test]
    fn the_first_answer_wins_when_the_repair_breaks_an_essential_part() {
        let gm = ScriptedGm::new(vec![ok("[1, 1, 7]"), Ok("not JSON".into())]);
        let (result, steps) = run(&gm, Need::Essential);
        let staged = result.unwrap().unwrap();
        assert_eq!(staged.output.items, vec![1, 1, 7]);
        assert_eq!(staged.drop, vec![2]);
        assert_eq!(steps.len(), 2);
    }

    #[derive(serde::Deserialize, JsonSchema)]
    struct Capped {
        #[schemars(length(max = 12))]
        title: String,
        #[schemars(length(max = 2))]
        notes: Vec<Line>,
    }

    #[derive(serde::Deserialize, JsonSchema)]
    struct Line {
        #[schemars(length(max = 5))]
        text: String,
        #[schemars(length(max = 1), inner(length(max = 3)))]
        tags: Vec<String>,
    }

    type CappedOutcome = Result<Option<Staged<Capped, usize>>, BuildFailure>;

    /// Runs one call whose schema is `Capped`'s, with no check of its own;
    /// each note is an optional item.
    fn run_capped(gm: &ScriptedGm, need: Need) -> (CappedOutcome, Vec<String>) {
        let mut steps = vec![];
        let result = {
            let mut progress = |_| {};
            let mut log = |e: DebugEntry| steps.push(e.step);
            let mut ctl = Control {
                progress: &mut progress,
                cancelled: &|| false,
                log: &mut log,
            };
            let call = StageCall {
                schema: schema_of::<Capped>(),
                ..call(need)
            };
            run_stage(gm, &mut ctl, "draft-1", &call, |_: &Capped| {
                Vec::<Problem<usize>>::new()
            })
        };
        (result, steps)
    }

    fn capped(title: &str, notes: &[(&str, &[&str])]) -> Result<String, GmError> {
        let notes: Vec<Value> = notes
            .iter()
            .map(|(text, tags)| json!({"text": text, "tags": tags}))
            .collect();
        Ok(json!({"title": title, "notes": notes}).to_string())
    }

    #[test]
    fn a_list_above_its_cap_is_cut_to_the_cap() {
        let gm = ScriptedGm::new(vec![capped(
            "Harbour",
            &[("a", &["x", "y"]), ("b", &[]), ("c", &[]), ("d", &[])],
        )]);
        let (result, steps) = run_capped(&gm, Need::Essential);
        let staged = result.unwrap().unwrap();
        assert_eq!(staged.output.title, "Harbour");
        let texts: Vec<&str> = staged
            .output
            .notes
            .iter()
            .map(|n| n.text.as_str())
            .collect();
        assert_eq!(texts, ["a", "b"]);
        assert_eq!(staged.output.notes[0].tags, ["x"]);
        assert!(staged.drop.is_empty());
        // Each cut list is reported with its path and its cap.
        let cut: Vec<(String, u64)> = staged.cut.iter().map(|(p, c)| (show(p), *c)).collect();
        assert_eq!(
            cut,
            [("notes".to_owned(), 2), ("notes[0].tags".to_owned(), 1)]
        );
        // Cutting a list needs no repair.
        assert_eq!(steps.len(), 1);
    }

    #[test]
    fn a_text_above_its_cap_drops_its_optional_item() {
        let answer = || capped("Harbour", &[("a", &[]), ("far too long", &[]), ("c", &[])]);
        let gm = ScriptedGm::new(vec![answer(), answer()]);
        let (result, steps) = run_capped(&gm, Need::Optional);
        let staged = result.unwrap().unwrap();
        assert_eq!(staged.drop, vec![1]);
        assert_eq!(steps, ["hidden_facts 1/1", "hidden_facts 1/1 repair"]);

        // A list item's text counts for its item too; the cap counts
        // characters, not bytes.
        let gm = ScriptedGm::new(vec![
            capped("Harbour", &[("ééééé", &["long"])]),
            capped("Harbour", &[("ééééé", &["long"])]),
        ]);
        let staged = run_capped(&gm, Need::Optional).0.unwrap().unwrap();
        assert_eq!(staged.drop, vec![0]);
        // A list at its cap is not cut.
        assert!(staged.cut.is_empty());

        // A text above its cap outside any item fails an essential call.
        let answer = || capped("A title far above twelve", &[]);
        let gm = ScriptedGm::new(vec![answer(), answer()]);
        let (result, _) = run_capped(&gm, Need::Essential);
        assert_eq!(
            result.err(),
            Some(BuildFailure {
                stage: Stage::HiddenFacts,
                check: BuildCheck::Schema
            })
        );
    }

    #[test]
    fn an_answer_above_the_size_limit_fails_as_schema() {
        assert_eq!(RAW_ANSWER_MAX_BYTES, 200_000);
        let body = json!({"title": "Harbour", "notes": []}).to_string();
        // Valid JSON, padded with spaces past the limit.
        let huge = format!("{body}{}", " ".repeat(RAW_ANSWER_MAX_BYTES));
        let gm = ScriptedGm::new(vec![Ok(huge.clone()), Ok(huge)]);
        let (result, steps) = run_capped(&gm, Need::Essential);
        assert_eq!(
            result.err(),
            Some(BuildFailure {
                stage: Stage::HiddenFacts,
                check: BuildCheck::Schema
            })
        );
        assert_eq!(steps.len(), 2);

        // At the limit, the answer is read.
        let at_limit = format!("{body}{}", " ".repeat(RAW_ANSWER_MAX_BYTES - body.len()));
        let gm = ScriptedGm::new(vec![Ok(at_limit)]);
        assert!(run_capped(&gm, Need::Essential).0.unwrap().is_some());
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
