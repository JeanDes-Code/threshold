//! Content Packs, format 2 (mature content data shape 3.8, ADR 0009): a
//! folder of data in `<data_dir>/packs/`. Prompt fragments that each name
//! one prompt and one insert point, an optional capability, a recommended
//! model that the app shows and never applies, and two lists kept as they
//! were. A pack holds no code, and nothing here reaches `checks`: loading
//! or removing a pack never changes a Commit check, the age rule, the Detail
//! Level a Campaign stores, a Refusal sentence or a Neutral Line.
//!
//! `pack.toml` at the root of the folder:
//!
//! ```toml
//! format = 2                              # required: any other value is refused
//! name = "my-pack"                        # required, 1 to 60 chars, one line
//! provides = ["explicit_description"]     # optional; the one capability
//! recommended_model = "some-tag"          # optional, 1 to 80 chars, no space
//! seed_options = ["..."]                  # optional, kept as before
//! model_hints = ["..."]                   # optional, kept as before
//!
//! [[fragments]]
//! prompt = "narration"                    # "proposal" or "narration"
//! at = "intimate_scene"                   # "tone", "scenes" or "intimate_scene"
//! when = "explicit"                       # optional: "always" (default) or "explicit"
//! file = "scene.md"                       # a plain file name in the pack folder
//!
//! [interview]                             # reserved for project 3: read and ignored
//! ```
//!
//! `load_all` refuses a whole pack on the first rule it breaks, and its
//! `PackProblem` names the folder (`shown_folder`), the rule and a line or
//! fragment number: never a line of the pack and never a file name the pack
//! chose. A fragment holds no control or invisible character and no line
//! that starts with a heading the core writes (`forges_a_heading`).
//! `docs/content-pack-format.md` is the same format, written for the author
//! of a private pack.

use std::{
    collections::HashSet,
    fs,
    path::{Component, Path},
};

use serde::Deserialize;

use crate::{context::CORE_HEADINGS, mature::Cleared, normalize};

/// The one pack format this code reads.
pub const FORMAT: i64 = 2;

/// The longest fragment, in chars after trimming the blanks at its start
/// and its end (about 300 tokens).
pub const FRAGMENT_MAX_CHARS: usize = 1_200;

/// The largest fragment file the loader reads, in bytes, before it trims
/// anything: a file over it is refused unread (rule 9). It leaves room for
/// `FRAGMENT_MAX_CHARS` chars of four bytes each and blank padding.
pub const FRAGMENT_MAX_BYTES: u64 = 16 * 1024;

/// The longest folder name the pack list shows, in chars; a longer one is
/// cut (`shown_folder`).
pub const FOLDER_SHOWN_MAX_CHARS: usize = 60;

/// The longest total of the `intimate_scene` fragments of one prompt, over
/// all loaded packs, in chars (about 400 tokens). They sit in the `recent`
/// block, which the model reads again on every scene Turn (ADR 0009).
pub const SCENE_FRAGMENTS_MAX_CHARS: usize = 1_600;

/// The longest total of the `tone` and `scenes` fragments of one prompt,
/// over all loaded packs, in chars (about 600 tokens, the size of the
/// Adventure block). They sit in the cached system block (ADR 0009).
pub const TONE_FRAGMENTS_MAX_CHARS: usize = 2_400;

/// The longest pack name, in chars.
pub const NAME_MAX_CHARS: usize = 60;

/// The largest word list file the loader reads, in bytes (rule 14): a
/// larger one is refused unread.
pub const WORDS_MAX_BYTES: u64 = 8 * 1024;

/// The most terms a word list holds (rule 14).
pub const WORDS_MAX_TERMS: usize = 200;

/// The longest term of a word list, in chars after trimming (rule 14).
pub const WORD_MAX_CHARS: usize = 60;

/// The longest recommended model, in chars.
pub const MODEL_MAX_CHARS: usize = 80;

/// File extensions that mark code. A pack folder that holds one is refused,
/// whatever the file contains.
pub const CODE_EXTENSIONS: &[&str] = &[
    "js", "mjs", "cjs", "ts", "wasm", "so", "dylib", "dll", "exe", "rs", "py", "sh", "bat", "ps1",
    "lua",
];

/// The prompt a fragment enters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Proposal,
    Narration,
}

/// Where a fragment goes in its prompt. `Tone` and `Scenes` go in the system
/// block, on every Turn; `IntimateScene` at the head of the `recent` block,
/// only with a `Cleared` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertPoint {
    Tone,
    Scenes,
    IntimateScene,
}

/// When an `IntimateScene` fragment enters its prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// In every scene Turn of a cleared open scene.
    Always,
    /// Only when the effective Detail Level is `explicit`.
    Explicit,
}

/// What a pack may declare that it gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// The pack gives explicit description: with it, a Campaign stored at
    /// `explicit` plays at `explicit` (`mature::effective`).
    ExplicitDescription,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment {
    pub prompt: Target,
    pub at: InsertPoint,
    pub when: When,
    /// The file's text, trimmed. It reaches a prompt and the debug log only.
    pub text: String,
}

/// The terms of a pack's word list (rule 14, ADR 0008), each as its folded
/// words (`mature::folded`). They are never shown and never in a prompt:
/// the debug form says how many terms there are, never which.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct WordList(Vec<Vec<String>>);

impl WordList {
    /// The terms, as folded words.
    pub(crate) fn terms(&self) -> &[Vec<String>] {
        &self.0
    }

    /// How many terms the list holds.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for WordList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WordList({} terms)", self.0.len())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentPack {
    /// The pack's folder under `packs/`, for the pack list.
    pub folder: String,
    pub name: String,
    pub fragments: Vec<Fragment>,
    pub provides: Vec<Capability>,
    /// Shown in the pack list; the app never switches the model by itself.
    pub recommended_model: Option<String>,
    pub seed_options: Vec<String>,
    pub model_hints: Vec<String>,
    /// The pack's word list (rule 14): an Intent that holds one of its
    /// terms is at least `intimate` for the gate
    /// (`mature::intimacy_floor`). Never shown, never in a prompt.
    pub intimate_words: WordList,
}

impl ContentPack {
    /// True when the pack declares `explicit_description`.
    pub fn explicit(&self) -> bool {
        self.provides.contains(&Capability::ExplicitDescription)
    }
}

/// Why `load_all` refused a pack: one variant per validation rule of data
/// shape 3.8 and `docs/content-pack-format.md`, the first split in two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackRule {
    /// Rule 1: a file with a code extension.
    Code,
    /// Rule 1: a symbolic link, or a file with more than one hard link
    /// (review minor, 2026-11-12): either can point outside the pack.
    Link,
    /// Rule 2: `pack.toml` is missing, is not UTF-8 TOML, has a key the
    /// format does not name, or a value of the wrong kind.
    Manifest,
    /// Rule 3: `format` is not 2.
    Format,
    /// Rule 4: the name is empty, too long, holds a control or invisible
    /// character or a blank other than a space, or is the name of a pack
    /// loaded before it.
    Name,
    /// Rule 5: a fragment has no `prompt`, an unknown `prompt`, `at` or
    /// `when`, or a `file` that is not a plain file in the pack folder.
    Fragment,
    /// Rule 6: an explicit-only fragment outside `intimate_scene`.
    ExplicitOutsideScene,
    /// Rule 7: a pack that declares `explicit_description` holds a fragment
    /// outside `intimate_scene`.
    CapableOutsideScene,
    /// Rule 8: an `intimate_scene` fragment, explicit-only or not, in a
    /// pack without the capability (review M1, 2026-11-12).
    SceneWithoutCapability,
    /// Rule 9: a fragment file is over `FRAGMENT_MAX_BYTES`, is empty, is
    /// not UTF-8 text, is longer than `FRAGMENT_MAX_CHARS`, or holds a
    /// control character other than a line end or a tab, or an invisible
    /// character.
    FragmentText,
    /// Rule 10: a line of a fragment starts with a heading the core writes
    /// in a prompt (`forges_a_heading`, review M1, 2026-11-12).
    CoreHeading,
    /// Rule 11: with this pack, the fragments of one prompt pass
    /// `SCENE_FRAGMENTS_MAX_CHARS` (scene) or `TONE_FRAGMENTS_MAX_CHARS`
    /// (tone and scenes).
    TooLong,
    /// Rule 12: `provides` holds an unknown word.
    Capability,
    /// Rule 13: a pack that declares `explicit_description` holds no
    /// narration `intimate_scene` fragment with `when = "explicit"` (review
    /// M1, 2026-11-12): it would turn explicit telling on with the core's
    /// explicit line alone.
    CapableWithoutExplicit,
    /// Rule 14: `intimate_words` names a file that is not a plain file in
    /// the pack folder, or a file over `WORDS_MAX_BYTES`, that is not UTF-8
    /// text, that holds a control character other than a line end or a tab
    /// or an invisible character, no term, more than `WORDS_MAX_TERMS`
    /// terms, a term longer than `WORD_MAX_CHARS` or a term with no letter
    /// or digit (live evaluation of 2026-11-12).
    Words,
}

impl PackRule {
    /// The one fixed sentence the pack list shows for this rule. It names
    /// no pack text.
    pub fn sentence(self) -> &'static str {
        match self {
            Self::Code => "Refused: a Content Pack holds no code file.",
            Self::Link => {
                "Refused: a Content Pack holds no symbolic link and no file with more than one hard link."
            }
            Self::Manifest => {
                "Refused: pack.toml is missing, is not valid TOML, or has a key or value that format 2 does not have."
            }
            Self::Format => "Refused: this pack is not in Content Pack format 2.",
            Self::Name => {
                "Refused: the pack name is empty, longer than 60 characters, holds an invisible character, or is already used by another pack."
            }
            Self::Fragment => {
                "Refused: a fragment needs a known prompt, insert point and when, and a file in the pack folder."
            }
            Self::ExplicitOutsideScene => {
                "Refused: an explicit-only fragment must be an intimate_scene fragment."
            }
            Self::CapableOutsideScene => {
                "Refused: a pack that gives explicit description may hold intimate_scene fragments only."
            }
            Self::SceneWithoutCapability => {
                "Refused: an intimate_scene fragment needs provides = [\"explicit_description\"]."
            }
            Self::FragmentText => {
                "Refused: a fragment file is empty, is not UTF-8 text, is longer than 1,200 characters, or holds a control or invisible character."
            }
            Self::CoreHeading => {
                "Refused: a line of a fragment starts with a heading that the app writes in its prompts."
            }
            Self::TooLong => {
                "Refused: with this pack, the fragments of one prompt are too long in total."
            }
            Self::Capability => "Refused: provides names a capability that does not exist.",
            Self::CapableWithoutExplicit => {
                "Refused: a pack that gives explicit description needs a narration intimate_scene fragment with when = \"explicit\"."
            }
            Self::Words => {
                "Refused: the intimate_words file is missing, empty, too long, not UTF-8 text, or holds a control or invisible character or a term with no letter."
            }
        }
    }
}

/// A refused pack: its folder as the pack list shows it (`shown_folder`),
/// the rule, and where: a line of `pack.toml`, a fragment table by its
/// number, or a fixed name. Never a line of the pack and never a name the
/// pack chose, such as a file name: a Player may paste the pack list into a
/// public issue (review minor, 2026-11-12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackProblem {
    pub folder: String,
    pub rule: PackRule,
    pub at: Option<String>,
}

impl PackProblem {
    /// The line the pack list shows: the rule's sentence, then where.
    pub fn text(&self) -> String {
        match &self.at {
            Some(at) => format!("{} ({at})", self.rule.sentence()),
            None => self.rule.sentence().to_owned(),
        }
    }
}

/// Loads every pack folder of `dir`, in folder name order. A folder whose
/// name starts with `.` and a plain file are not packs. A refused pack
/// loads nothing and gives one `PackProblem`. No `dir` is no pack: not an
/// error.
pub fn load_all(dir: &Path) -> (Vec<ContentPack>, Vec<PackProblem>) {
    let mut folders: Vec<(String, std::path::PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            )
        })
        .filter(|(name, _)| !name.starts_with('.'))
        // The folder itself may be a link (the owner keeps his pack in a
        // repository of his own): `metadata` follows it. Inside a pack, a
        // link is refused (rule 1).
        .filter(|(_, path)| fs::metadata(path).is_ok_and(|m| m.is_dir()))
        .collect();
    folders.sort();
    let mut packs: Vec<ContentPack> = Vec::new();
    let mut problems = Vec::new();
    for (folder, path) in folders {
        let folder = shown_folder(&folder);
        match load(&folder, &path, &packs) {
            Ok(pack) => packs.push(pack),
            Err((rule, at)) => problems.push(PackProblem { folder, rule, at }),
        }
    }
    (packs, problems)
}

/// A folder name as the pack list, the standard error and the debug log
/// show it: each control or invisible character becomes `?`, and a name
/// over `FOLDER_SHOWN_MAX_CHARS` chars is cut and ends with `...`. The
/// folder name is chosen by whoever made the folder.
pub fn shown_folder(name: &str) -> String {
    let mut shown: String = name
        .chars()
        .map(|c| {
            if c.is_control() || invisible(c) {
                '?'
            } else {
                c
            }
        })
        .take(FOLDER_SHOWN_MAX_CHARS)
        .collect();
    if name.chars().count() > FOLDER_SHOWN_MAX_CHARS {
        shown.push_str("...");
    }
    shown
}

/// True for a character that shows nothing or changes how the text around
/// it shows: a soft hyphen, a zero-width space or joiner, a direction mark
/// or override, a word joiner, a variation selector, a byte order mark, a
/// Hangul filler, an interlinear annotation mark, a tag character.
pub fn invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}'
        | '\u{034F}'
        | '\u{061C}'
        | '\u{115F}'
        | '\u{1160}'
        | '\u{17B4}'
        | '\u{17B5}'
        | '\u{180B}'..='\u{180F}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}'
        | '\u{3164}'
        | '\u{FE00}'..='\u{FE0F}'
        | '\u{FEFF}'
        | '\u{FFA0}'
        | '\u{FFF0}'..='\u{FFFB}'
        | '\u{1BCA0}'..='\u{1BCA3}'
        | '\u{1D173}'..='\u{1D17A}'
        | '\u{E0000}'..='\u{E0FFF}')
}

/// True when `line` starts with a heading the core writes in a proposal or
/// a narration prompt (`context::CORE_HEADINGS`), in any case (review M1,
/// 2026-11-12). Before the match, the line loses what comes before its
/// first letter or digit (blanks, `#`, `*`, `-`, `>`, quote marks), every
/// run of blanks becomes one space, and a blank before a colon goes. A
/// heading that ends with a colon matches up to its colon; any other
/// heading matches when no letter or digit follows it. A line that starts
/// with a handle the core writes (`E12`, `F3`, `G1`, `W2`, `N1`) matches too.
pub fn forges_a_heading(line: &str) -> bool {
    let start = line.trim_start_matches(|c: char| !c.is_alphanumeric());
    let mut norm = String::with_capacity(start.len());
    for word in start.split_whitespace() {
        if !norm.is_empty() && !word.starts_with(':') {
            norm.push(' ');
        }
        norm.push_str(&word.to_lowercase());
    }
    let handle = {
        let mut chars = norm.chars();
        matches!(chars.next(), Some('e' | 'f' | 'g' | 'w' | 'n'))
            && chars.next().is_some_and(|c| c.is_ascii_digit())
    };
    handle
        || CORE_HEADINGS.iter().any(|heading| {
            let heading = heading.to_lowercase();
            norm.strip_prefix(heading.as_str()).is_some_and(|rest| {
                heading.ends_with(':') || !rest.starts_with(char::is_alphanumeric)
            })
        })
}

/// The `intimate_scene` fragments of `packs` for `prompt`: the `always`
/// ones, and the `explicit` ones too when `explicit`. `cleared` is the
/// proof that every character the gate checks at the PC's place is
/// cleared (ADR 0009): only `mature::clearance` makes one, so a caller that
/// skipped the gate cannot ask for a scene fragment.
pub fn scene_fragments<'a>(
    packs: &'a [ContentPack],
    prompt: Target,
    _cleared: &Cleared,
    explicit: bool,
) -> Vec<&'a str> {
    packs
        .iter()
        .flat_map(|p| &p.fragments)
        .filter(|f| f.prompt == prompt && f.at == InsertPoint::IntimateScene)
        .filter(|f| f.when == When::Always || explicit)
        .map(|f| f.text.as_str())
        .collect()
}

/// The `tone` and `scenes` fragments of `packs` for `prompt`, at `at`: the
/// system block of that prompt.
pub(crate) fn rule_fragments(packs: &[ContentPack], prompt: Target, at: InsertPoint) -> Vec<&str> {
    packs
        .iter()
        .flat_map(|p| &p.fragments)
        .filter(|f| f.prompt == prompt && f.at == at && f.when == When::Always)
        .map(|f| f.text.as_str())
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    format: Option<toml::Value>,
    name: Option<String>,
    #[serde(default)]
    provides: Vec<String>,
    recommended_model: Option<String>,
    #[serde(default)]
    seed_options: Vec<String>,
    #[serde(default)]
    model_hints: Vec<String>,
    #[serde(default)]
    fragments: Vec<RawFragment>,
    /// The word list file (rule 14).
    intimate_words: Option<String>,
    /// Reserved for project 3: read and ignored.
    #[serde(default, rename = "interview")]
    _interview: Option<toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFragment {
    prompt: Option<String>,
    at: Option<String>,
    when: Option<String>,
    file: Option<String>,
}

type Refused = (PackRule, Option<String>);

fn refused<T>(rule: PackRule, at: impl Into<Option<String>>) -> Result<T, Refused> {
    Err((rule, at.into()))
}

/// One pack, checked against the packs loaded before it. The rules run in
/// their order, and the first one broken refuses the whole pack.
fn load(folder: &str, dir: &Path, before: &[ContentPack]) -> Result<ContentPack, Refused> {
    // Rule 1.
    refuse_code(dir)?;
    // Rule 2.
    let manifest = dir.join("pack.toml");
    let Ok(bytes) = fs::read(&manifest) else {
        return refused(PackRule::Manifest, "pack.toml".to_owned());
    };
    let Ok(source) = String::from_utf8(bytes) else {
        return refused(PackRule::Manifest, "pack.toml".to_owned());
    };
    let raw: RawManifest = match toml::from_str(&source) {
        Ok(raw) => raw,
        // The error's own text quotes the line: only its line number goes.
        Err(e) => {
            let at = e.span().map_or_else(
                || "pack.toml".to_owned(),
                |span| format!("pack.toml, line {}", line_of(&source, span.start)),
            );
            return refused(PackRule::Manifest, at);
        }
    };
    let model = raw.recommended_model.as_deref().map(str::trim);
    if model.is_some_and(|m| {
        m.is_empty()
            || m.chars().count() > MODEL_MAX_CHARS
            || m.chars().any(|c| c.is_whitespace() || c.is_control())
    }) {
        return refused(PackRule::Manifest, "recommended_model".to_owned());
    }
    // Rule 3.
    if raw.format.as_ref().and_then(toml::Value::as_integer) != Some(FORMAT) {
        return refused(PackRule::Format, None);
    }
    // Rule 4.
    let name = raw.name.as_deref().unwrap_or_default().trim().to_owned();
    let taken = before
        .iter()
        .any(|p| normalize::name(&p.name) == normalize::name(&name));
    if name.is_empty()
        || name.chars().count() > NAME_MAX_CHARS
        || name
            .chars()
            .any(|c| c.is_control() || invisible(c) || (c.is_whitespace() && c != ' '))
        || taken
    {
        return refused(PackRule::Name, None);
    }
    // Rule 5.
    let mut entries = Vec::new();
    for (n, entry) in raw.fragments.iter().enumerate() {
        let at = || format!("pack.toml, fragment {}", n + 1);
        let prompt = match entry.prompt.as_deref() {
            Some("proposal") => Target::Proposal,
            Some("narration") => Target::Narration,
            _ => return refused(PackRule::Fragment, at()),
        };
        let point = match entry.at.as_deref() {
            Some("tone") => InsertPoint::Tone,
            Some("scenes") => InsertPoint::Scenes,
            Some("intimate_scene") => InsertPoint::IntimateScene,
            _ => return refused(PackRule::Fragment, at()),
        };
        let when = match entry.when.as_deref() {
            None | Some("always") => When::Always,
            Some("explicit") => When::Explicit,
            Some(_) => return refused(PackRule::Fragment, at()),
        };
        let Some(file) = entry.file.as_deref().filter(|f| plain_file_name(f)) else {
            return refused(PackRule::Fragment, at());
        };
        if !fs::symlink_metadata(dir.join(file)).is_ok_and(|m| m.is_file()) {
            return refused(PackRule::Fragment, at());
        }
        entries.push((prompt, point, when, file));
    }
    let capable = raw.provides.iter().any(|p| p == "explicit_description");
    // Rule 6.
    if let Some(n) = entries
        .iter()
        .position(|(_, at, when, _)| *when == When::Explicit && *at != InsertPoint::IntimateScene)
    {
        return refused(
            PackRule::ExplicitOutsideScene,
            format!("pack.toml, fragment {}", n + 1),
        );
    }
    // Rule 7.
    if capable
        && let Some(n) = entries
            .iter()
            .position(|(_, at, _, _)| *at != InsertPoint::IntimateScene)
    {
        return refused(
            PackRule::CapableOutsideScene,
            format!("pack.toml, fragment {}", n + 1),
        );
    }
    // Rule 8: an `intimate_scene` fragment, whatever its `when`, needs
    // the capability (review M1).
    if !capable
        && let Some(n) = entries
            .iter()
            .position(|(_, at, _, _)| *at == InsertPoint::IntimateScene)
    {
        return refused(
            PackRule::SceneWithoutCapability,
            format!("pack.toml, fragment {}", n + 1),
        );
    }
    // Rule 9. Where: the fragment's number, never its file name.
    let mut fragments = Vec::new();
    for (n, (prompt, at, when, file)) in entries.into_iter().enumerate() {
        let at_fragment = || Some(format!("pack.toml, fragment {}", n + 1));
        let text = fragment_text(&dir.join(file)).ok_or((PackRule::FragmentText, at_fragment()))?;
        // Rule 10.
        if text.lines().any(forges_a_heading) {
            return Err((PackRule::CoreHeading, at_fragment()));
        }
        fragments.push(Fragment {
            prompt,
            at,
            when,
            text,
        });
    }
    // Rule 11.
    for prompt in [Target::Proposal, Target::Narration] {
        let total = |scene: bool| -> usize {
            before
                .iter()
                .flat_map(|p| &p.fragments)
                .chain(&fragments)
                .filter(|f| f.prompt == prompt && (f.at == InsertPoint::IntimateScene) == scene)
                .map(|f| f.text.chars().count())
                .sum()
        };
        if total(true) > SCENE_FRAGMENTS_MAX_CHARS || total(false) > TONE_FRAGMENTS_MAX_CHARS {
            return refused(PackRule::TooLong, None);
        }
    }
    // Rule 12.
    let mut provides = Vec::new();
    let mut seen = HashSet::new();
    for word in &raw.provides {
        match word.as_str() {
            "explicit_description" if seen.insert(word) => {
                provides.push(Capability::ExplicitDescription);
            }
            "explicit_description" => {}
            _ => return refused(PackRule::Capability, None),
        }
    }
    // Rule 13: a capable pack holds an explicit-only narration fragment.
    let explicit_narration = fragments.iter().any(|f| {
        f.prompt == Target::Narration
            && f.at == InsertPoint::IntimateScene
            && f.when == When::Explicit
    });
    if capable && !explicit_narration {
        return refused(PackRule::CapableWithoutExplicit, None);
    }
    // Rule 14. Where: the key, never the file name.
    let intimate_words = match raw.intimate_words.as_deref() {
        None => WordList::default(),
        Some(file) => word_list(dir, file).ok_or((
            PackRule::Words,
            Some("pack.toml, intimate_words".to_owned()),
        ))?,
    };
    Ok(ContentPack {
        folder: folder.to_owned(),
        name,
        fragments,
        provides,
        recommended_model: model.map(str::to_owned),
        seed_options: raw.seed_options,
        model_hints: raw.model_hints,
        intimate_words,
    })
}

/// The word list `file` of the pack in `dir` (rule 14): one term per line,
/// blank lines left out, each term trimmed and folded as the gate reads an
/// Intent (`mature::folded`). `None` when the rule refuses it.
fn word_list(dir: &Path, file: &str) -> Option<WordList> {
    use std::io::Read;
    if !plain_file_name(file) {
        return None;
    }
    let path = dir.join(file);
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > WORDS_MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .ok()?
        .take(WORDS_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > WORDS_MAX_BYTES {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    if text
        .chars()
        .any(|c| invisible(c) || (c.is_control() && c != '\n' && c != '\t'))
    {
        return None;
    }
    let mut terms = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let words = crate::mature::folded(line);
        if words.is_empty() || line.chars().count() > WORD_MAX_CHARS {
            return None;
        }
        terms.push(words);
    }
    (!terms.is_empty() && terms.len() <= WORDS_MAX_TERMS).then_some(WordList(terms))
}

/// The 1-based line of byte `at` in `source`.
fn line_of(source: &str, at: usize) -> usize {
    source
        .get(..at.min(source.len()))
        .map_or(1, |head| head.matches('\n').count() + 1)
}

/// True for a plain file name: one normal path part, no folder.
fn plain_file_name(file: &str) -> bool {
    let mut parts = Path::new(file).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    )
}

/// The text of a fragment file, trimmed of the blanks at its start and its
/// end: `None` when the file is over `FRAGMENT_MAX_BYTES` (it is not read
/// then), cannot be read, is not UTF-8, is empty, is longer than
/// `FRAGMENT_MAX_CHARS` chars once trimmed, or holds a control character
/// other than a line end or a tab, or an invisible character (`invisible`).
fn fragment_text(path: &Path) -> Option<String> {
    use std::io::Read;
    if fs::metadata(path).ok()?.len() > FRAGMENT_MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(FRAGMENT_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > FRAGMENT_MAX_BYTES {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    let text = text.trim();
    let clean = text
        .chars()
        .all(|c| !(invisible(c) || (c.is_control() && c != '\n' && c != '\t')));
    (clean && !text.is_empty() && text.chars().count() <= FRAGMENT_MAX_CHARS)
        .then(|| text.to_owned())
}

/// Walks `dir` and refuses any symbolic link (file or folder: it could point
/// outside the pack), any file with more than one hard link (the same), and
/// any entry whose extension is in `CODE_EXTENSIONS` (case does not
/// matter). The refusal names no file: a file name is the pack's own text.
fn refuse_code(dir: &Path) -> Result<(), Refused> {
    let Ok(entries) = fs::read_dir(dir) else {
        return refused(PackRule::Manifest, None);
    };
    for entry in entries {
        let Ok(entry) = entry else {
            return refused(PackRule::Manifest, None);
        };
        let path = entry.path();
        let is_code = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| CODE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()));
        if is_code {
            return refused(PackRule::Code, None);
        }
        let Ok(kind) = entry.file_type() else {
            return refused(PackRule::Manifest, None);
        };
        if kind.is_symlink() || (kind.is_file() && hard_linked(&entry)) {
            return refused(PackRule::Link, None);
        }
        if kind.is_dir() {
            refuse_code(&path)?;
        }
    }
    Ok(())
}

/// True when the file of `entry` has more than one hard link: another name
/// of it may sit outside the pack. A file whose links cannot be read counts
/// as linked. Only Unix reports the count; elsewhere the answer is false.
fn hard_linked(entry: &fs::DirEntry) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        entry.metadata().map_or(true, |m| m.nlink() > 1)
    }
    #[cfg(not(unix))]
    {
        let _ = entry;
        false
    }
}
