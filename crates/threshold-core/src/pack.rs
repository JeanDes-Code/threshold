//! Content Packs (design §6.4): a folder of data. Prompt fragments added at
//! fixed points of the system prompt, extra Adventure Seed options and model
//! hints. A pack holds no code, and nothing here reaches `checks`: loading or
//! removing a pack never changes a Commit check.
//!
//! Layout: `pack.toml` at the root of the folder.
//!
//! ```toml
//! name = "my-pack"
//! seed_options = ["..."]
//! model_hints = ["..."]
//!
//! [[fragments]]
//! at = "tone"        # or "scenes"
//! file = "prompt.md" # a file name in the pack folder
//! ```

use std::{
    fmt, fs,
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;

/// File extensions that mark code. A pack folder that holds one is refused,
/// whatever the file contains.
pub const CODE_EXTENSIONS: &[&str] = &["js", "wasm", "so", "dylib", "dll", "rs", "py", "sh"];

/// Where a fragment goes in the system prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertPoint {
    Tone,
    Scenes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentPack {
    pub name: String,
    pub prompt_fragments: Vec<(InsertPoint, String)>,
    pub seed_options: Vec<String>,
    pub model_hints: Vec<String>,
}

#[derive(Debug)]
pub enum PackError {
    /// The folder holds a file with a code extension (`CODE_EXTENSIONS`).
    CodeNotAllowed(PathBuf),
    Io(String),
    Invalid(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CodeNotAllowed(path) => {
                write!(f, "content packs hold no code: {}", path.display())
            }
            Self::Io(message) | Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PackError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    name: String,
    #[serde(default)]
    seed_options: Vec<String>,
    #[serde(default)]
    model_hints: Vec<String>,
    #[serde(default)]
    fragments: Vec<FragmentEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FragmentEntry {
    at: InsertPoint,
    file: String,
}

impl ContentPack {
    /// Loads the pack in `dir`. The whole folder is scanned for code first,
    /// so a refused pack loads nothing.
    pub fn load(dir: &Path) -> Result<ContentPack, PackError> {
        refuse_code(dir)?;
        let manifest = read(&dir.join("pack.toml"))?;
        let manifest: Manifest =
            toml::from_str(&manifest).map_err(|e| PackError::Invalid(format!("pack.toml: {e}")))?;
        if manifest.name.trim().is_empty() {
            return Err(PackError::Invalid("pack.toml: empty name".into()));
        }
        let mut prompt_fragments = Vec::new();
        for entry in manifest.fragments {
            let mut parts = Path::new(&entry.file).components();
            let inside = matches!(
                (parts.next(), parts.next()),
                (Some(Component::Normal(_)), None)
            );
            if !inside {
                return Err(PackError::Invalid(format!(
                    "fragment file must be a file name in the pack folder: {}",
                    entry.file
                )));
            }
            let text = read(&dir.join(&entry.file))?;
            prompt_fragments.push((entry.at, text.trim().to_owned()));
        }
        Ok(ContentPack {
            name: manifest.name,
            prompt_fragments,
            seed_options: manifest.seed_options,
            model_hints: manifest.model_hints,
        })
    }
}

fn read(path: &Path) -> Result<String, PackError> {
    fs::read_to_string(path).map_err(|e| PackError::Io(format!("{}: {e}", path.display())))
}

/// Walks `dir` without following symbolic links and refuses any entry whose
/// extension is in `CODE_EXTENSIONS` (case does not matter).
fn refuse_code(dir: &Path) -> Result<(), PackError> {
    let entries =
        fs::read_dir(dir).map_err(|e| PackError::Io(format!("{}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| PackError::Io(e.to_string()))?;
        let path = entry.path();
        let is_code = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| CODE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()));
        if is_code {
            return Err(PackError::CodeNotAllowed(path));
        }
        let kind = entry
            .file_type()
            .map_err(|e| PackError::Io(format!("{}: {e}", path.display())))?;
        if kind.is_dir() {
            refuse_code(&path)?;
        }
    }
    Ok(())
}
