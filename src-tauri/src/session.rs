//! What the Tauri commands do, without Tauri: tests drive it directly.

use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Serialize, Serializer};
use threshold_core::{
    Gm,
    domain::{EntityKind, Knowing},
    fixture::{load_adventure_seed, load_world},
    model::GmError,
    rules::{D10, RollResult},
    store::{CampaignId, Store, StoredTurn, TurnKind, WorldView},
    turn::{Engine, TurnError, TurnOutcome, create_campaign},
    world::pc_knowledge,
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
    Failed(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("no campaign is open"),
            Self::DebugOff => f.write_str("debug mode is off"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

fn failed(e: impl fmt::Display) -> AppError {
    AppError::Failed(e.to_string())
}

impl std::error::Error for AppError {}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CampaignSummary {
    pub db_path: String,
    pub pc_name: String,
    pub premise: String,
    pub turns: Vec<TurnLine>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TurnLine {
    pub turn_id: u64,
    pub kind: String,
    pub intent: String,
    pub narration: Option<String>,
    pub roll: Option<RollResult>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TurnSummary {
    pub turn_id: u64,
    pub roll: Option<RollResult>,
    pub narration: String,
    pub total_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CharacterView {
    pub name: String,
    pub concept: String,
    pub clock_min: u64,
    pub facts: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct JournalEntry {
    pub subject: String,
    pub text: String,
    pub source: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct JournalView {
    pub observations: Vec<JournalEntry>,
    pub claims: Vec<JournalEntry>,
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

/// The open campaign, if any, and the debug toggle. One per app.
pub struct Session {
    data_dir: PathBuf,
    make_gm: MakeGm,
    open: Option<Open>,
    /// Shared with the shell, so toggling never waits for a running Turn.
    debug: Arc<AtomicBool>,
}

impl Session {
    pub fn new(data_dir: PathBuf, make_gm: MakeGm) -> Self {
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
    /// opens it. A file that fails to set up is removed.
    pub fn new_campaign(
        &mut self,
        seed_path: &Path,
        world_path: &Path,
        rng_seed: u64,
    ) -> Result<CampaignSummary, AppError> {
        let seed = load_adventure_seed(seed_path).map_err(failed)?;
        let dir = self.campaigns_dir();
        fs::create_dir_all(&dir).map_err(failed)?;
        let db_path = dir.join(format!("campaign-{}.sqlite", clock_nanos()));
        let setup = || -> Result<(Store, CampaignId), AppError> {
            let mut store = Store::open(&db_path).map_err(failed)?;
            let campaign = create_campaign(&mut store, &seed, rng_seed).map_err(failed)?;
            load_world(&mut store, campaign, world_path).map_err(failed)?;
            Ok((store, campaign))
        };
        match setup() {
            Ok((store, campaign)) => self.attach(store, campaign, db_path),
            Err(e) => {
                let _ = fs::remove_file(&db_path);
                Err(e)
            }
        }
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

    /// The open campaign, with every play and Correction Turn.
    pub fn summary(&self) -> Result<CampaignSummary, AppError> {
        let open = self.open()?;
        let view = self.view()?;
        let seed = view.adventure_seed.as_ref();
        let turns = open
            .engine
            .store()
            .turns(open.campaign)
            .map_err(failed)?
            .into_iter()
            .filter(|t| matches!(t.record.kind, TurnKind::Play | TurnKind::Correction))
            .map(turn_line)
            .collect();
        Ok(CampaignSummary {
            db_path: open.db_path.display().to_string(),
            pc_name: seed.map(|s| s.pc.name.clone()).unwrap_or_default(),
            premise: seed.map(|s| s.premise.clone()).unwrap_or_default(),
            turns,
        })
    }

    pub fn submit_intent(
        &mut self,
        op_id: String,
        text: &str,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnSummary, AppError> {
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        let outcome = open
            .engine
            .play_turn(op_id, text, on_chunk)
            .map_err(|e| player_error(&e))?;
        Ok(turn_summary(outcome))
    }

    pub fn correct_last(
        &mut self,
        op_id: String,
        clarified: &str,
        why: &str,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnSummary, AppError> {
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        let outcome = open
            .engine
            .correct_last(op_id, clarified, why, on_chunk)
            .map_err(|e| player_error(&e))?;
        Ok(turn_summary(outcome))
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

    /// The PC as the PC knows itself: only its own Beliefs about itself.
    pub fn get_character(&self) -> Result<CharacterView, AppError> {
        let view = self.view()?;
        let pc = view
            .entities
            .iter()
            .find(|e| e.kind == EntityKind::Pc)
            .ok_or_else(|| failed("the campaign has no PC"))?;
        let concept = view
            .adventure_seed
            .as_ref()
            .map(|s| s.pc.concept.clone())
            .unwrap_or_default();
        let facts = pc_knowledge(&view)
            .into_iter()
            .filter(|b| b.subject == pc.id)
            .map(|b| b.text)
            .collect();
        Ok(CharacterView {
            name: pc.name.clone(),
            concept,
            clock_min: view.clock_min,
            facts,
        })
    }

    /// PC Knowledge, split: what the PC witnessed, and what it was told.
    /// Built from the PC's Beliefs only, so a fact no one told the PC (a
    /// GM-only fact) cannot appear.
    pub fn get_journal(&self) -> Result<JournalView, AppError> {
        let view = self.view()?;
        let name = |id: u64| {
            view.entities
                .iter()
                .find(|e| e.id == id)
                .map_or_else(|| "someone".to_owned(), |e| e.name.clone())
        };
        let mut journal = JournalView {
            observations: Vec::new(),
            claims: Vec::new(),
        };
        for belief in pc_knowledge(&view) {
            let entry = JournalEntry {
                subject: name(belief.subject),
                text: belief.text,
                source: belief.source.map(name),
            };
            match belief.how {
                Knowing::Witnessed => journal.observations.push(entry),
                Knowing::Told => journal.claims.push(entry),
            }
        }
        Ok(journal)
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

/// The text of a Turn error that the Player may see. Rejection reasons and
/// model or store messages can quote GM-only facts (a Persistent key, a
/// model's echo), so only the check kind and its handle get through. The
/// full reasons stay in the debug log.
pub fn player_error(e: &TurnError) -> AppError {
    let text = match e {
        TurnError::EmptyIntent | TurnError::IntentTooLong | TurnError::NotCorrectable(_) => {
            e.to_string()
        }
        TurnError::Model(GmError::Unreachable(_)) => "the game model is unreachable".into(),
        TurnError::Model(GmError::Timeout) => "the game model request timed out".into(),
        TurnError::Model(GmError::BadStatus(status, _)) => {
            format!("the game model returned HTTP {status}")
        }
        TurnError::Rejected(errs) => {
            let mut text = String::from("the proposal was rejected twice:");
            for r in errs {
                text.push_str(&format!("\n- {}", r.check));
                if let Some(handle) = r.handle {
                    text.push_str(&format!(" {handle}"));
                }
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

fn stored_roll(proposal_json: &str) -> Option<RollResult> {
    let record: serde_json::Value = serde_json::from_str(proposal_json).ok()?;
    serde_json::from_value(record.get("roll")?.clone()).ok()
}

fn turn_line(t: StoredTurn) -> TurnLine {
    TurnLine {
        turn_id: t.id,
        kind: kind_name(t.record.kind),
        roll: stored_roll(&t.record.proposal_json),
        intent: t.record.intent,
        narration: t.narration,
    }
}

fn turn_summary(outcome: TurnOutcome) -> TurnSummary {
    TurnSummary {
        turn_id: outcome.turn_id,
        roll: outcome.roll,
        narration: outcome.narration,
        total_ms: outcome.timings.total_ms,
    }
}
