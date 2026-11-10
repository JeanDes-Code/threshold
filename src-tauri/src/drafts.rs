//! The draft commands and their jobs: draft, review, approve, build.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use serde::Serialize;
use threshold_core::{
    build::{
        self, BuildCheck, BuildFailure, Control, DebugEntry, DraftState, DraftStore,
        DraftStoreError, Gap, KnownFact, Progress, SOURCE_TEXT_MAX_CHARS, SeedDraft, Stage,
    },
    domain::AdventureSeed,
    setup::{SetupError, create_campaign_file},
};

use crate::session::{
    AppError, CampaignSummary, DebugLine, MakeGm, Session, clock_nanos, failed, lock,
    new_campaign_path,
};

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
