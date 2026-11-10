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
    build::{
        self, BuildCheck, BuildFailure, Control, DebugEntry, DraftState, DraftStore,
        DraftStoreError, Gap, KnownFact, Progress, SOURCE_TEXT_MAX_CHARS, SeedDraft, Stage,
    },
    domain::{AdventureSeed, EntityKind, Knowing, is_open_key},
    fixture::{load_adventure_seed, load_world_setup},
    model::GmError,
    rules::{D10, RollResult},
    setup::{SetupError, create_campaign_file, remove_part},
    store::{CampaignId, Store, StoredTurn, TurnKind, WorldView},
    turn::{Engine, TurnError, TurnOutcome, stored_roll},
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

fn failed(e: impl fmt::Display) -> AppError {
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

#[derive(Clone, Debug, Serialize)]
pub struct CampaignSummary {
    pub db_path: String,
    pub pc_name: String,
    pub premise: String,
    pub turns: Vec<TurnLine>,
    /// No Opening Turn and no play Turn yet: the Player still has to Begin.
    pub needs_begin: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TurnLine {
    pub turn_id: u64,
    pub kind: String,
    pub intent: String,
    pub narration: Option<String>,
    pub roll: Option<RollResult>,
    /// A later Correction reverted this Turn: its outcome no longer holds.
    pub reverted: bool,
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
pub struct CharacterView {
    pub name: String,
    pub concept: String,
    pub clock_min: u64,
    pub facts: Vec<CharacterFact>,
}

/// One thing the PC knows about itself.
#[derive(Clone, Debug, Serialize)]
pub struct CharacterFact {
    pub text: String,
    /// An open item (ADR 0005): planned or pending, not done.
    pub open: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct JournalEntry {
    pub subject: String,
    pub text: String,
    pub source: Option<String>,
    /// An open item (ADR 0005): planned or pending, not done.
    pub open: bool,
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

/// Locks `m`, and takes the value of a poisoned lock: a panic in one
/// command must not lock the app for good.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A new Campaign file path in `dir`, which it creates.
fn new_campaign_path(dir: &Path) -> Result<PathBuf, AppError> {
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

    /// The open campaign, with its Opening Turn and every play and
    /// Correction Turn.
    pub fn summary(&self) -> Result<CampaignSummary, AppError> {
        let open = self.open()?;
        let view = self.view()?;
        let seed = view.adventure_seed.as_ref();
        let store = open.engine.store();
        let reverted = store.reverted_turns(open.campaign).map_err(failed)?;
        let stored = store.turns(open.campaign).map_err(failed)?;
        let needs_begin = !stored
            .iter()
            .any(|t| matches!(t.record.kind, TurnKind::Opening | TurnKind::Play));
        let turns = stored
            .into_iter()
            .filter(|t| {
                matches!(
                    t.record.kind,
                    TurnKind::Play | TurnKind::Correction | TurnKind::Opening
                )
            })
            .map(|t| {
                let reverted = reverted.contains(&t.id);
                turn_line(t, reverted)
            })
            .collect();
        Ok(CampaignSummary {
            db_path: open.db_path.display().to_string(),
            pc_name: seed.map(|s| s.pc.name.clone()).unwrap_or_default(),
            premise: seed.map(|s| s.premise.clone()).unwrap_or_default(),
            turns,
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
            .map(|b| CharacterFact {
                open: is_open_key(&b.key),
                text: b.text,
            })
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
                open: is_open_key(&belief.key),
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

/// A draft as the review screen shows it: what the draft holds, and its
/// gaps. It holds no goal, secret, hidden fact or event, because a draft
/// never stores one.
#[derive(Clone, Debug, Serialize)]
pub struct DraftView {
    pub draft_id: String,
    pub state: DraftState,
    pub seed: AdventureSeed,
    pub knowledge: Vec<KnownFact>,
    pub ages_to_confirm: Vec<String>,
    /// Information only: the names whose age the model says the text states.
    pub ages_stated: Vec<String>,
    pub gaps: Vec<Gap>,
    pub last_failure: Option<BuildFailure>,
}

impl DraftView {
    fn of(draft: SeedDraft) -> Self {
        Self {
            gaps: build::gaps(&draft),
            draft_id: draft.id,
            state: draft.state,
            seed: draft.seed,
            knowledge: draft.knowledge,
            ages_to_confirm: draft.ages_to_confirm,
            ages_stated: draft.ages_stated,
            last_failure: draft.last_failure,
        }
    }
}

/// One line of the draft list.
#[derive(Clone, Debug, Serialize)]
pub struct DraftLine {
    pub draft_id: String,
    pub title: String,
    pub state: DraftState,
}

/// The job that runs. A build names its draft: no command may change that
/// draft until the build ends.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Job {
    Draft,
    Build(String),
}

/// Frees the job slot when the job ends, also on a panic.
struct JobSlot(Arc<Mutex<Option<Job>>>);

impl Drop for JobSlot {
    fn drop(&mut self) {
        *lock(&self.0) = None;
    }
}

const BUSY: &str = "another draft or build is running: wait for it to end, or cancel it";
const UNDER_BUILD: &str = "this draft is being built: wait for the build to end, or cancel it";

/// The draft commands and their jobs: the draft store, the one-job guard,
/// the cancel flag and the model of drafts and builds. Clones share all of
/// it.
///
/// One job runs at a time: a second `draft_from_text` or `start_build`
/// fails at once and never waits. A job does not hold the `Session` lock
/// while the model runs; it locks the session only to open the finished
/// Campaign. Every draft read and save takes the store lock, so two saves
/// of one draft never run at once.
#[derive(Clone)]
pub struct Drafts {
    store: Arc<Mutex<DraftStore>>,
    session: Arc<Mutex<Session>>,
    make_gm: Arc<MakeGm>,
    job: Arc<Mutex<Option<Job>>>,
    cancel: Arc<AtomicBool>,
    debug: Arc<AtomicBool>,
    campaigns_dir: PathBuf,
}

impl Drafts {
    /// The drafts in `<data_dir>/drafts/`, which it creates. `make_gm`
    /// builds the model of each job.
    pub fn new(
        data_dir: &Path,
        session: Arc<Mutex<Session>>,
        make_gm: MakeGm,
    ) -> Result<Self, AppError> {
        let store = DraftStore::open(&data_dir.join("drafts")).map_err(failed)?;
        let (debug, campaigns_dir) = {
            let s = lock(&session);
            (s.debug_flag(), s.campaigns_dir())
        };
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            session,
            make_gm: Arc::new(make_gm),
            job: Arc::new(Mutex::new(None)),
            cancel: Arc::new(AtomicBool::new(false)),
            debug,
            campaigns_dir,
        })
    }

    /// Drafts a Seed Draft from a Source Text and saves it. The text is
    /// checked here first, so an empty or too long text costs no model call
    /// and gets a message that names the limit.
    pub fn draft_from_text(
        &self,
        text: &str,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<DraftView, AppError> {
        if text.trim().is_empty() {
            return Err(failed("the Source Text is empty"));
        }
        if text.chars().count() > SOURCE_TEXT_MAX_CHARS {
            return Err(failed(format!(
                "the Source Text is too long: the limit is {SOURCE_TEXT_MAX_CHARS} characters"
            )));
        }
        let _slot = self.start_job(Job::Draft)?;
        let gm = (self.make_gm)();
        // The core picks the draft id, and each log entry carries it: the
        // debug file has the new draft's name from the first call on.
        let mut logged_to: Option<String> = None;
        let drafted = {
            let mut log = |entry: DebugEntry| {
                self.append_debug(&entry);
                logged_to = Some(entry.op_id);
            };
            let cancelled = || self.cancelled();
            let mut ctl = Control {
                progress: on_progress,
                cancelled: &cancelled,
                log: &mut log,
            };
            build::draft_from_text(text, gm.as_ref(), &mut ctl)
        };
        match drafted {
            Ok(draft) => {
                lock(&self.store).save(&draft).map_err(draft_error)?;
                Ok(DraftView::of(draft))
            }
            Err(failure) => {
                // No draft exists to hold these records.
                if let Some(id) = logged_to {
                    let _ = lock(&self.store).clear_debug(&id);
                }
                Err(job_error(failure))
            }
        }
    }

    /// Every draft, the newest first.
    pub fn list_drafts(&self) -> Result<Vec<DraftLine>, AppError> {
        let lines = lock(&self.store).list().map_err(draft_error)?;
        Ok(lines
            .into_iter()
            .map(|line| DraftLine {
                draft_id: line.id,
                title: line.title,
                state: line.state,
            })
            .collect())
    }

    pub fn get_draft(&self, draft_id: &str) -> Result<DraftView, AppError> {
        let draft = lock(&self.store).load(draft_id).map_err(draft_error)?;
        Ok(DraftView::of(draft))
    }

    /// A Player edit: it replaces the seed, the knowledge, the ages to
    /// confirm and the ages the model calls stated, and returns the draft to
    /// `Draft`.
    pub fn save_draft(
        &self,
        draft_id: &str,
        seed: AdventureSeed,
        knowledge: Vec<KnownFact>,
        ages_to_confirm: Vec<String>,
        ages_stated: Vec<String>,
    ) -> Result<DraftView, AppError> {
        let store = lock(&self.store);
        self.refuse_under_build(draft_id)?;
        let mut draft = store.load(draft_id).map_err(draft_error)?;
        build::apply_edit(&mut draft, seed, knowledge, ages_to_confirm, ages_stated);
        store.save(&draft).map_err(draft_error)?;
        Ok(DraftView::of(draft))
    }

    /// Approves a draft with no gap; on a gap, returns every gap.
    pub fn approve_draft(&self, draft_id: &str) -> Result<DraftView, AppError> {
        let store = lock(&self.store);
        self.refuse_under_build(draft_id)?;
        let mut draft = store.load(draft_id).map_err(draft_error)?;
        build::approve(&mut draft).map_err(AppError::Gaps)?;
        store.save(&draft).map_err(draft_error)?;
        Ok(DraftView::of(draft))
    }

    /// Builds the world of an approved draft, writes its Campaign file and
    /// opens it. A failed or cancelled build writes no file; the draft stays
    /// `Approved` with `last_failure` set, so a retry needs no new review.
    pub fn start_build(
        &self,
        draft_id: &str,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<CampaignSummary, AppError> {
        let _slot = self.start_job(Job::Build(draft_id.to_owned()))?;
        let draft = lock(&self.store).load(draft_id).map_err(draft_error)?;
        match draft.state {
            DraftState::Approved => {}
            DraftState::Draft => return Err(failed("the draft is not approved")),
            DraftState::Built => return Err(failed("the draft is already built")),
        }
        lock(&self.store)
            .clear_debug(draft_id)
            .map_err(draft_error)?;
        let gm = (self.make_gm)();
        let built = {
            let mut log = |entry: DebugEntry| self.append_debug(&entry);
            let cancelled = || self.cancelled();
            let mut ctl = Control {
                progress: &mut *on_progress,
                cancelled: &cancelled,
                log: &mut log,
            };
            build::build_world(&draft, gm.as_ref(), &mut ctl)
        };
        let world = built.map_err(|failure| self.build_failed(draft_id, failure))?;
        // A cancel that came during the last model call still stops the
        // build: no Campaign file is written after it.
        if self.cancelled() {
            let failure = BuildFailure {
                stage: Stage::Commit,
                check: BuildCheck::Cancelled,
            };
            return Err(self.build_failed(draft_id, failure));
        }

        on_progress(Progress {
            stage: Stage::Commit,
            done: 0,
            total: 1,
        });
        let path = new_campaign_path(&self.campaigns_dir)
            .map_err(|_| self.build_failed(draft_id, commit_failure(None)))?;
        create_campaign_file(&path, &draft.seed, &world, clock_nanos())
            .map_err(|e| self.build_failed(draft_id, commit_failure(Some(&e))))?;
        on_progress(Progress {
            stage: Stage::Commit,
            done: 1,
            total: 1,
        });
        self.update(draft_id, |d| {
            d.state = DraftState::Built;
            d.campaign_file = Some(path.display().to_string());
            d.last_failure = None;
        })?;
        lock(&self.session).open_campaign(&path)
    }

    /// Stops the running draft or build between two model calls. It sets a
    /// flag and takes no lock.
    pub fn cancel_build(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Removes a draft and its debug records. Its Campaign file stays.
    pub fn delete_draft(&self, draft_id: &str) -> Result<(), AppError> {
        let store = lock(&self.store);
        self.refuse_under_build(draft_id)?;
        store.delete(draft_id).map_err(draft_error)
    }

    /// The prompts and raw outputs of the draft's last run. Refused unless
    /// debug mode is on: they reveal the hidden world.
    pub fn get_build_debug(&self, draft_id: &str) -> Result<Vec<DebugLine>, AppError> {
        if !self.debug.load(Ordering::SeqCst) {
            return Err(AppError::DebugOff);
        }
        let entries = lock(&self.store)
            .read_debug(draft_id)
            .map_err(draft_error)?;
        Ok(entries
            .into_iter()
            .map(|e| DebugLine {
                step: e.step,
                prompt: e.prompt,
                raw: e.raw,
                ms: e.ms,
            })
            .collect())
    }

    /// Takes the one job slot and clears the cancel flag, or fails at once.
    fn start_job(&self, job: Job) -> Result<JobSlot, AppError> {
        let mut slot = lock(&self.job);
        if slot.is_some() {
            return Err(failed(BUSY));
        }
        *slot = Some(job);
        self.cancel.store(false, Ordering::SeqCst);
        Ok(JobSlot(self.job.clone()))
    }

    fn refuse_under_build(&self, draft_id: &str) -> Result<(), AppError> {
        match &*lock(&self.job) {
            Some(Job::Build(id)) if id == draft_id => Err(failed(UNDER_BUILD)),
            _ => Ok(()),
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Debug records help, but a failed write must not stop a job.
    fn append_debug(&self, entry: &DebugEntry) {
        let _ = lock(&self.store).append_debug(&entry.op_id, entry);
    }

    /// Loads, changes and saves a draft under one store lock.
    fn update(&self, draft_id: &str, change: impl FnOnce(&mut SeedDraft)) -> Result<(), AppError> {
        let store = lock(&self.store);
        let mut draft = store.load(draft_id).map_err(draft_error)?;
        change(&mut draft);
        store.save(&draft).map_err(draft_error)
    }

    /// Records the failure on the draft, and returns the Player's error.
    fn build_failed(&self, draft_id: &str, failure: BuildFailure) -> AppError {
        // The build error matters more than a failed save of it.
        let _ = self.update(draft_id, |d| d.last_failure = Some(failure));
        job_error(failure)
    }
}

/// A draft store error as the Player sees it.
fn draft_error(e: DraftStoreError) -> AppError {
    match e {
        DraftStoreError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
            failed("no such draft")
        }
        e => failed(e),
    }
}

/// The failure of the Campaign file write; `None` when the folder could not
/// be made. A setup message can quote a name of the hidden world, so only a
/// check name remains. `BuildCheck` has no storage check: a disk or database
/// error counts as `Completeness`, since the file could not be completed.
fn commit_failure(e: Option<&SetupError>) -> BuildFailure {
    let check = match e {
        Some(SetupError::Invalid(_)) => BuildCheck::References,
        Some(SetupError::Io(_) | SetupError::Store(_)) | None => BuildCheck::Completeness,
    };
    BuildFailure {
        stage: Stage::Commit,
        check,
    }
}

/// The Player's text for a failed draft or build: the stage and the check
/// in plain words, built from the two names only. Never model text.
fn job_error(failure: BuildFailure) -> AppError {
    let (job, step) = match failure.stage {
        Stage::DraftSeed => ("The draft", "the seed step"),
        Stage::DraftRoster => ("The draft", "the roster step"),
        Stage::DraftKnowledge => ("The draft", "the known facts step"),
        Stage::HiddenFacts => ("The world build", "the hidden facts step"),
        Stage::NpcState => ("The world build", "the characters step"),
        Stage::WorldEvents => ("The world build", "the world events step"),
        Stage::StartState => ("The world build", "the start step"),
        Stage::Commit => ("The world build", "the campaign file step"),
    };
    let check = match failure.check {
        BuildCheck::Model => "the model call failed",
        BuildCheck::Schema => "the model answer did not fit its form",
        BuildCheck::References => "a reference check failed",
        BuildCheck::OnePc => "the one-PC check failed",
        BuildCheck::Kinds => "a kind check failed",
        BuildCheck::Ages => "an age check failed",
        BuildCheck::Completeness => "a completeness check failed",
        BuildCheck::Agency => "a PC agency check failed",
        BuildCheck::SettledFacts => "a settled facts check failed",
        BuildCheck::OpenItems => "an open items check failed",
        BuildCheck::WorldAutonomy => "a world autonomy check failed",
        BuildCheck::Secrecy => "a secrecy check failed",
        BuildCheck::Cancelled => "it was cancelled",
        BuildCheck::NotApproved => "the draft is not approved",
    };
    AppError::Failed(format!("{job} stopped at {step}: {check}."))
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

fn turn_line(t: StoredTurn, reverted: bool) -> TurnLine {
    TurnLine {
        reverted,
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
        narration: Some(outcome.narration),
        narration_error: None,
        total_ms: outcome.timings.total_ms,
    }
}
