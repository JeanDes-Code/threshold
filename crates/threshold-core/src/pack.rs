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
//! `PackProblem` names the folder, the rule and a file name or a line
//! number: never a line of the pack. `docs/content-pack-format.md` is the
//! same format, written for the author of a private pack.

use std::{
    collections::HashSet,
    fs,
    path::{Component, Path},
};

use serde::Deserialize;

use crate::{mature::Cleared, normalize};

/// The one pack format this code reads.
pub const FORMAT: i64 = 2;

/// The longest fragment, in chars (about 300 tokens).
pub const FRAGMENT_MAX_CHARS: usize = 1_200;

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
}

impl ContentPack {
    /// True when the pack declares `explicit_description`.
    pub fn explicit(&self) -> bool {
        self.provides.contains(&Capability::ExplicitDescription)
    }
}

/// Why `load_all` refused a pack: one variant per validation rule of data
/// shape 3.8, the first split in two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackRule {
    /// Rule 1: a file with a code extension.
    Code,
    /// Rule 1: a symbolic link.
    Symlink,
    /// Rule 2: `pack.toml` is missing, is not UTF-8 TOML, has a key the
    /// format does not name, or a value of the wrong kind.
    Manifest,
    /// Rule 3: `format` is not 2.
    Format,
    /// Rule 4: the name is empty, too long, not one line, or the name of a
    /// pack loaded before it.
    Name,
    /// Rule 5: a fragment has no `prompt`, an unknown `prompt`, `at` or
    /// `when`, or a `file` that is not a plain file in the pack folder.
    Fragment,
    /// Rule 6: an explicit-only fragment outside `intimate_scene`.
    ExplicitOutsideScene,
    /// Rule 7: a pack that declares `explicit_description` holds a fragment
    /// outside `intimate_scene`.
    CapableOutsideScene,
    /// Rule 8: an explicit-only fragment in a pack without the capability.
    ExplicitWithoutCapability,
    /// Rule 9: a fragment file is empty, is not UTF-8 text, or is longer
    /// than `FRAGMENT_MAX_CHARS`.
    FragmentText,
    /// Rule 10: with this pack, the fragments of one prompt pass
    /// `SCENE_FRAGMENTS_MAX_CHARS` (scene) or `TONE_FRAGMENTS_MAX_CHARS`
    /// (tone and scenes).
    TooLong,
    /// Rule 11: `provides` holds an unknown word.
    Capability,
}

impl PackRule {
    /// The one fixed sentence the pack list shows for this rule. It names
    /// no pack text.
    pub fn sentence(self) -> &'static str {
        match self {
            Self::Code => "Refused: a Content Pack holds no code file.",
            Self::Symlink => "Refused: a Content Pack holds no symbolic link.",
            Self::Manifest => {
                "Refused: pack.toml is missing, is not valid TOML, or has a key or value that format 2 does not have."
            }
            Self::Format => "Refused: this pack is not in Content Pack format 2.",
            Self::Name => {
                "Refused: the pack name is empty, longer than 60 characters, or already used by another pack."
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
            Self::ExplicitWithoutCapability => {
                "Refused: an explicit-only fragment needs provides = [\"explicit_description\"]."
            }
            Self::FragmentText => {
                "Refused: a fragment file is empty, is not UTF-8 text, or is longer than 1,200 characters."
            }
            Self::TooLong => {
                "Refused: with this pack, the fragments of one prompt are too long in total."
            }
            Self::Capability => "Refused: provides names a capability that does not exist.",
        }
    }
}

/// A refused pack: its folder, the rule, and where (a file name inside the
/// pack, or a line of `pack.toml`). Never a line of the pack: a Player may
/// paste the pack list into a public issue.
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
        match load(&folder, &path, &packs) {
            Ok(pack) => packs.push(pack),
            Err((rule, at)) => problems.push(PackProblem { folder, rule, at }),
        }
    }
    (packs, problems)
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
    refuse_code(dir, dir)?;
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
        || name.chars().any(char::is_control)
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
    // Rule 8.
    if !capable
        && let Some(n) = entries
            .iter()
            .position(|(_, _, when, _)| *when == When::Explicit)
    {
        return refused(
            PackRule::ExplicitWithoutCapability,
            format!("pack.toml, fragment {}", n + 1),
        );
    }
    // Rule 9.
    let mut fragments = Vec::new();
    for (prompt, at, when, file) in entries {
        let text = fragment_text(&dir.join(file))
            .ok_or((PackRule::FragmentText, Some(file.to_owned())))?;
        fragments.push(Fragment {
            prompt,
            at,
            when,
            text,
        });
    }
    // Rule 10.
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
    // Rule 11.
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
    Ok(ContentPack {
        folder: folder.to_owned(),
        name,
        fragments,
        provides,
        recommended_model: model.map(str::to_owned),
        seed_options: raw.seed_options,
        model_hints: raw.model_hints,
    })
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

/// The trimmed text of a fragment file: `None` when it cannot be read, is
/// not UTF-8, is empty, or is longer than `FRAGMENT_MAX_CHARS`. A file far
/// over the limit is not read at all.
fn fragment_text(path: &Path) -> Option<String> {
    let len = fs::metadata(path).ok()?.len();
    if len > (FRAGMENT_MAX_CHARS as u64) * 4 + 64 {
        return None;
    }
    let text = String::from_utf8(fs::read(path).ok()?).ok()?;
    let text = text.trim();
    (!text.is_empty() && text.chars().count() <= FRAGMENT_MAX_CHARS).then(|| text.to_owned())
}

/// Walks `dir` and refuses any symbolic link (file or folder: it could point
/// outside the pack) and any entry whose extension is in `CODE_EXTENSIONS`
/// (case does not matter). `at` is the path inside the pack, `root` first.
fn refuse_code(root: &Path, dir: &Path) -> Result<(), Refused> {
    let inside = |path: &Path| {
        path.strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    };
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
            return refused(PackRule::Code, inside(&path));
        }
        let Ok(kind) = entry.file_type() else {
            return refused(PackRule::Manifest, None);
        };
        if kind.is_symlink() {
            return refused(PackRule::Symlink, inside(&path));
        }
        if kind.is_dir() {
            refuse_code(root, &path)?;
        }
    }
    Ok(())
}
