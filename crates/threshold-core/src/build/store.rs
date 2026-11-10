use std::{
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::{DebugEntry, DraftState, SeedDraft};

/// The length of a draft's title in `DraftStore::list`.
const TITLE_CHARS: usize = 60;

#[derive(Debug)]
pub enum DraftStoreError {
    Io(io::Error),
    Json(serde_json::Error),
    /// The id is not `draft-<digits>`, so it cannot be a file stem.
    BadId(String),
}

impl fmt::Display for DraftStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Json(e) => e.fmt(f),
            Self::BadId(id) => write!(f, "not a draft id: {id}"),
        }
    }
}

impl std::error::Error for DraftStoreError {}

impl From<io::Error> for DraftStoreError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for DraftStoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// One line of the draft list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftLine {
    pub id: String,
    /// The first 60 chars of the premise, on one line.
    pub title: String,
    pub state: DraftState,
}

/// The drafts of one folder, apart from any Campaign file: `<id>.json` for
/// the draft and `<id>.debug.jsonl` for the model calls of its last run.
pub struct DraftStore {
    dir: PathBuf,
}

impl DraftStore {
    /// Opens the folder, and creates it when it does not exist.
    pub fn open(dir: &Path) -> Result<Self, DraftStoreError> {
        fs::create_dir_all(dir)?;
        Ok(Self { dir: dir.into() })
    }

    /// Every draft, the newest first.
    pub fn list(&self) -> Result<Vec<DraftLine>, DraftStoreError> {
        let mut lines = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let name = entry?.file_name();
            let Some(id) = name.to_str().and_then(|n| n.strip_suffix(".json")) else {
                continue;
            };
            if check_id(id).is_err() {
                continue;
            }
            let draft = self.load(id)?;
            let premise = draft.seed.premise.split_whitespace();
            lines.push(DraftLine {
                title: premise
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(TITLE_CHARS)
                    .collect(),
                id: draft.id,
                state: draft.state,
            });
        }
        lines.sort_by(|a, b| (b.id.len(), &b.id).cmp(&(a.id.len(), &a.id)));
        Ok(lines)
    }

    pub fn load(&self, id: &str) -> Result<SeedDraft, DraftStoreError> {
        let text = fs::read_to_string(self.draft_path(id)?)?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Writes the draft as it is: `save` changes no field. It writes a
    /// temporary file, then renames it over the draft, so a crash leaves
    /// the old draft or the new one, never half of one.
    pub fn save(&self, draft: &SeedDraft) -> Result<(), DraftStoreError> {
        let path = self.draft_path(&draft.id)?;
        let tmp = path.with_extension("json.tmp");
        let written = (|| -> Result<(), DraftStoreError> {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(&serde_json::to_vec_pretty(draft)?)?;
            file.sync_all()?;
            fs::rename(&tmp, &path)?;
            Ok(())
        })();
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        written
    }

    /// Removes the draft and its debug file. A missing file is no error.
    pub fn delete(&self, id: &str) -> Result<(), DraftStoreError> {
        remove(&self.draft_path(id)?)?;
        remove(&self.debug_path(id)?)
    }

    /// Starts a new run: removes the debug records of the last one.
    pub fn clear_debug(&self, id: &str) -> Result<(), DraftStoreError> {
        remove(&self.debug_path(id)?)
    }

    /// Appends one model call to the debug file, as one JSON line.
    pub fn append_debug(&self, id: &str, entry: &DebugEntry) -> Result<(), DraftStoreError> {
        let mut line = serde_json::to_string(entry)?;
        line.push('\n');
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.debug_path(id)?)?
            .write_all(line.as_bytes())?;
        Ok(())
    }

    /// The debug records of the last run; none when there is no file.
    pub fn read_debug(&self, id: &str) -> Result<Vec<DebugEntry>, DraftStoreError> {
        let text = match fs::read_to_string(self.debug_path(id)?) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            text => text?,
        };
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| Ok(serde_json::from_str(line)?))
            .collect()
    }

    fn draft_path(&self, id: &str) -> Result<PathBuf, DraftStoreError> {
        check_id(id)?;
        Ok(self.dir.join(format!("{id}.json")))
    }

    fn debug_path(&self, id: &str) -> Result<PathBuf, DraftStoreError> {
        check_id(id)?;
        Ok(self.dir.join(format!("{id}.debug.jsonl")))
    }
}

/// An id is `draft-` and digits, so it is always a plain file stem.
fn check_id(id: &str) -> Result<(), DraftStoreError> {
    match id.strip_prefix("draft-") {
        Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => Ok(()),
        _ => Err(DraftStoreError::BadId(id.to_owned())),
    }
}

fn remove(path: &Path) -> Result<(), DraftStoreError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}
