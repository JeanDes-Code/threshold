//! What the Tauri commands do, without Tauri: tests drive it directly.

use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Serialize, Serializer};
use threshold_core::{
    Gm,
    build::Gap,
    fixture::{load_adventure_seed, load_world_setup},
    model::GmError,
    player::Marks,
    rules::{D10, RollResult},
    setup::{create_campaign_file, remove_part},
    store::{CampaignId, Store, StoredTurn, TurnKind, WorldView},
    turn::{Engine, TurnError, TurnOutcome, stored_roll},
    view::{self, CharacterView, JournalView, PlayView},
    world::{PcSight, pc_sight},
};
use threshold_ollama::OllamaConfig;

/// The config file the app reads from its data dir.
pub const CONFIG_FILE: &str = "threshold.toml";

/// Builds the model adapter for each Engine the session opens.
pub type MakeGm = Box<dyn Fn() -> Box<dyn Gm + Send> + Send + Sync>;

#[derive(Debug)]
pub enum AppError {
    NoCampaign,
    DebugOff,
    /// `approve_draft` on a draft that still has gaps: every gap.
    Gaps(Vec<Gap>),
    Failed(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("no campaign is open"),
            Self::DebugOff => f.write_str("debug mode is off"),
            Self::Gaps(gaps) => write!(
                f,
                "the draft cannot be approved: {} gap(s) to fix",
                gaps.len()
            ),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

pub(crate) fn failed(e: impl fmt::Display) -> AppError {
    AppError::Failed(e.to_string())
}

impl std::error::Error for AppError {}

/// An error reaches the UI as its text, except `Gaps`: the list of gaps.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Gaps(gaps) => gaps.serialize(s),
            _ => s.collect_str(self),
        }
    }
}

/// The Campaign just opened. Its story comes from `get_play_view`.
#[derive(Clone, Debug, Serialize)]
pub struct CampaignSummary {
    pub db_path: String,
    pub pc_name: String,
    pub premise: String,
    /// No Opening Turn and no play Turn yet: the Player still has to Begin.
    pub needs_begin: bool,
}

/// A committed Turn. `narration` is `None` when the Turn is committed but
/// its Narration failed; `narration_error` then says why, and the Turn can
/// be narrated again.
#[derive(Clone, Debug, Serialize)]
pub struct TurnSummary {
    pub turn_id: u64,
    pub roll: Option<RollResult>,
    pub narration: Option<String>,
    pub narration_error: Option<String>,
    pub total_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DebugLine {
    pub step: String,
    pub prompt: String,
    pub raw: String,
    pub ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DebugView {
    pub turn_id: u64,
    pub op_id: String,
    pub kind: String,
    pub intent: String,
    pub proposal_json: String,
    pub narration: Option<String>,
    pub entries: Vec<DebugLine>,
}

/// Reads `threshold.toml` from `dir`, and writes the default config there
/// first when the file is absent.
pub fn load_config(dir: &Path) -> Result<OllamaConfig, AppError> {
    let path = dir.join(CONFIG_FILE);
    if !path.exists() {
        fs::create_dir_all(dir).map_err(failed)?;
        let text = toml::to_string_pretty(&OllamaConfig::default()).map_err(failed)?;
        fs::write(&path, text).map_err(failed)?;
    }
    let text = fs::read_to_string(&path).map_err(failed)?;
    OllamaConfig::from_toml(&text).map_err(|e| failed(format!("{}: {e}", path.display())))
}

/// A fresh value from the system clock, for Roll seeds and OOC op ids.
pub fn clock_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

struct Open {
    engine: Engine,
    campaign: CampaignId,
    db_path: PathBuf,
}

/// Locks `m`, and takes the value of a poisoned lock: a panic in one
/// command must not lock the app for good.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A new Campaign file path in `dir`, which it creates.
pub(crate) fn new_campaign_path(dir: &Path) -> Result<PathBuf, AppError> {
    fs::create_dir_all(dir).map_err(failed)?;
    Ok(dir.join(format!("campaign-{}.sqlite", clock_nanos())))
}

/// Removes every `.part` file in `dir`, and its SQLite side files: what a
/// crash between the two Setup Turns leaves (data shape 4).
fn remove_part_files(dir: &Path) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "part") {
            remove_part(&path);
        }
    }
}

/// The open campaign, if any, and the debug toggle. One per app.
pub struct Session {
    data_dir: PathBuf,
    make_gm: MakeGm,
    open: Option<Open>,
    /// Shared with the shell, so toggling never waits for a running Turn.
    debug: Arc<AtomicBool>,
}

impl Session {
    /// A session on `data_dir`. It first deletes the `.part` files a crash
    /// left in `campaigns/`.
    pub fn new(data_dir: PathBuf, make_gm: MakeGm) -> Self {
        remove_part_files(&data_dir.join("campaigns"));
        Self {
            data_dir,
            make_gm,
            open: None,
            debug: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Where new campaign files go.
    pub fn campaigns_dir(&self) -> PathBuf {
        self.data_dir.join("campaigns")
    }

    /// The campaign files in the data dir, newest first.
    pub fn list_campaigns(&self) -> Vec<String> {
        let mut files: Vec<String> = fs::read_dir(self.campaigns_dir())
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "sqlite"))
            .map(|path| path.display().to_string())
            .collect();
        files.sort();
        files.reverse();
        files
    }

    /// Creates a campaign file from an Adventure Seed and a world file, and
    /// opens it. A failed setup leaves no file.
    pub fn new_campaign(
        &mut self,
        seed_path: &Path,
        world_path: &Path,
        rng_seed: u64,
    ) -> Result<CampaignSummary, AppError> {
        let seed = load_adventure_seed(seed_path).map_err(failed)?;
        let world = load_world_setup(world_path).map_err(failed)?;
        let db_path = new_campaign_path(&self.campaigns_dir())?;
        create_campaign_file(&db_path, &seed, &world, rng_seed).map_err(failed)?;
        self.open_campaign(&db_path)
    }

    /// Opens an existing campaign file: its last campaign.
    pub fn open_campaign(&mut self, db_path: &Path) -> Result<CampaignSummary, AppError> {
        let store = Store::open_existing(db_path).map_err(failed)?;
        let campaign = *store
            .campaigns()
            .map_err(failed)?
            .last()
            .ok_or_else(|| failed("the file holds no campaign"))?;
        self.attach(store, campaign, db_path.to_path_buf())
    }

    fn attach(
        &mut self,
        store: Store,
        campaign: CampaignId,
        db_path: PathBuf,
    ) -> Result<CampaignSummary, AppError> {
        let engine = Engine::new(store, (self.make_gm)(), Box::new(D10), vec![], campaign);
        self.open = Some(Open {
            engine,
            campaign,
            db_path,
        });
        self.summary()
    }

    fn open(&self) -> Result<&Open, AppError> {
        self.open.as_ref().ok_or(AppError::NoCampaign)
    }

    fn view(&self) -> Result<WorldView, AppError> {
        let open = self.open()?;
        open.engine.store().snapshot(open.campaign).map_err(failed)
    }

    /// The open campaign: its file, its PC and premise, and whether Begin
    /// is still due.
    pub fn summary(&self) -> Result<CampaignSummary, AppError> {
        let open = self.open()?;
        let view = self.view()?;
        let seed = view.adventure_seed.as_ref();
        let stored = open.engine.store().turns(open.campaign).map_err(failed)?;
        let needs_begin = !stored
            .iter()
            .any(|t| matches!(t.record.kind, TurnKind::Opening | TurnKind::Play));
        Ok(CampaignSummary {
            db_path: open.db_path.display().to_string(),
            pc_name: seed.map(|s| s.pc.name.clone()).unwrap_or_default(),
            premise: seed.map(|s| s.premise.clone()).unwrap_or_default(),
            needs_begin,
        })
    }

    pub fn submit_intent(
        &mut self,
        op_id: String,
        text: &str,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnSummary, AppError> {
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        let played = open.engine.play_turn(op_id, text, on_chunk);
        committed(&open.engine, played)
    }

    /// Begin: the Opening Turn of the open Campaign. A second call returns
    /// the stored Turn.
    pub fn begin(&mut self, on_chunk: &mut dyn FnMut(&str)) -> Result<TurnSummary, AppError> {
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        let played = open.engine.begin(on_chunk);
        committed(&open.engine, played)
    }

    pub fn correct_last(
        &mut self,
        op_id: String,
        clarified: &str,
        why: &str,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnSummary, AppError> {
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        let played = open.engine.correct_last(op_id, clarified, why, on_chunk);
        committed(&open.engine, played)
    }

    pub fn ask_ooc(&mut self, text: &str) -> Result<String, AppError> {
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        open.engine
            .ooc(format!("ooc-{}", clock_nanos()), text)
            .map_err(|e| player_error(&e))
    }

    /// The `op_id` a Turn was played under.
    pub fn turn_op_id(&self, turn_id: u64) -> Result<String, AppError> {
        Ok(self.turn(turn_id)?.record.op_id)
    }

    pub fn renarrate(
        &mut self,
        turn_id: u64,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, AppError> {
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        open.engine
            .renarrate(turn_id, on_chunk)
            .map_err(|e| player_error(&e))
    }

    /// The open campaign as the PC knows it: the only input of a view.
    fn sight(&self) -> Result<PcSight, AppError> {
        let open = self.open()?;
        let store = open.engine.store();
        let view = store.snapshot(open.campaign).map_err(failed)?;
        let turns = store.turns(open.campaign).map_err(failed)?;
        let reverted = store
            .reverted_turns(open.campaign)
            .map_err(failed)?
            .into_iter()
            .collect();
        pc_sight(&view, &turns, &reverted).ok_or_else(|| failed("the campaign has no PC"))
    }

    /// The PC as the PC knows itself: only its own Beliefs about itself.
    pub fn get_character(&self) -> Result<CharacterView, AppError> {
        Ok(view::character_view(&self.sight()?))
    }

    /// The Play screen: the story and the side panel, from PC Knowledge
    /// only. The UI calls it after a Turn ends, never during one.
    pub fn get_play_view(&self) -> Result<PlayView, AppError> {
        Ok(view::play_view(&self.sight()?, &Marks::default()))
    }

    /// PC Knowledge, split: what the PC witnessed, and what it was told.
    pub fn get_journal(&self) -> Result<JournalView, AppError> {
        Ok(view::legacy_journal(&self.sight()?))
    }

    pub fn set_debug(&mut self, on: bool) {
        self.debug.store(on, Ordering::SeqCst);
    }

    pub fn debug(&self) -> bool {
        self.debug.load(Ordering::SeqCst)
    }

    /// The debug toggle itself, for the shell to read and set without
    /// locking the session.
    pub fn debug_flag(&self) -> Arc<AtomicBool> {
        self.debug.clone()
    }

    /// Prompts, raw model output and timings of one Turn. Refused unless
    /// debug mode is on: it shows GM-only facts.
    pub fn get_debug(&self, turn_id: u64) -> Result<DebugView, AppError> {
        if !self.debug() {
            return Err(AppError::DebugOff);
        }
        let turn = self.turn(turn_id)?;
        let entries = self
            .open()?
            .engine
            .store()
            .debug_log(&turn.record.op_id)
            .map_err(failed)?
            .into_iter()
            .map(|e| DebugLine {
                step: e.step,
                prompt: e.prompt,
                raw: e.raw,
                ms: e.ms,
            })
            .collect();
        Ok(DebugView {
            turn_id,
            op_id: turn.record.op_id,
            kind: kind_name(turn.record.kind),
            intent: turn.record.intent,
            proposal_json: turn.record.proposal_json,
            narration: turn.narration,
            entries,
        })
    }

    fn turn(&self, turn_id: u64) -> Result<StoredTurn, AppError> {
        let open = self.open()?;
        open.engine
            .store()
            .turn(turn_id)
            .map_err(failed)?
            .filter(|t| t.record.campaign_id == open.campaign)
            .ok_or_else(|| failed(format!("no turn {turn_id}")))
    }
}

/// A Turn result as the app reports it. A Turn whose Narration failed after
/// the Commit is a committed Turn without Narration, not an error.
fn committed(
    engine: &Engine,
    played: Result<TurnOutcome, TurnError>,
) -> Result<TurnSummary, AppError> {
    match played {
        Ok(outcome) => Ok(turn_summary(outcome)),
        Err(TurnError::NarrationFailed { turn_id, error }) => {
            let stored = engine
                .store()
                .turn(turn_id)
                .map_err(failed)?
                .ok_or_else(|| failed(format!("no turn {turn_id}")))?;
            Ok(TurnSummary {
                turn_id,
                roll: stored_roll(&stored.record.proposal_json),
                narration: None,
                narration_error: Some(model_error(&error)),
                total_ms: 0,
            })
        }
        Err(e) => Err(player_error(&e)),
    }
}

/// The text of a model error that the Player may see: never the server's
/// message, which can echo the prompt.
fn model_error(e: &GmError) -> String {
    match e {
        GmError::Unreachable(_) => "the game model is unreachable".into(),
        GmError::Timeout => "the game model request timed out".into(),
        GmError::BadStatus(status, _) => format!("the game model returned HTTP {status}"),
        GmError::Aborted(_) => "the game model stopped with an error".into(),
    }
}

/// The text of a Turn error that the Player may see. Rejection reasons and
/// model or store messages can quote GM-only facts (a Persistent key, a
/// model's echo), so only the check kind gets through. A handle would reveal
/// that a hidden fact exists, and its id. The full reasons stay in the debug
/// log.
pub fn player_error(e: &TurnError) -> AppError {
    let text = match e {
        TurnError::EmptyIntent
        | TurnError::IntentTooLong
        | TurnError::NotCorrectable(_)
        | TurnError::PlayStarted => e.to_string(),
        TurnError::Model(error) => model_error(error),
        TurnError::NarrationFailed { error, .. } => format!(
            "the Turn is committed, but its narration failed: {}",
            model_error(error)
        ),
        TurnError::Rejected(errs) => {
            let mut text = String::from("the proposal was rejected twice:");
            for r in errs {
                text.push_str(&format!("\n- {}", r.check));
            }
            text.push_str("\n(details in the debug view)");
            text
        }
        TurnError::Store(_) => {
            "a storage error stopped the Turn (details in the debug view)".into()
        }
    };
    AppError::Failed(text)
}

fn kind_name(kind: TurnKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn turn_summary(outcome: TurnOutcome) -> TurnSummary {
    TurnSummary {
        turn_id: outcome.turn_id,
        roll: outcome.roll,
        narration: Some(outcome.narration),
        narration_error: None,
        total_ms: outcome.timings.total_ms,
    }
}
