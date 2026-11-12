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

use serde::{Serialize, Serializer, ser::SerializeMap};
use threshold_core::{
    Gm,
    build::Gap,
    fixture::{load_adventure_seed, load_world_setup},
    model::GmError,
    pack::{self, ContentPack, PackProblem},
    player::{Marks, Note, PlayerStore, Target},
    rules::{D10, RollResult},
    setup::{create_campaign_file, remove_part},
    store::{self, CampaignId, DebugEntry, Store, StoreError, StoredTurn, TurnKind, WorldView},
    turn::{Engine, TurnControl, TurnError, TurnOutcome, stored_roll},
    view::{self, CharacterView, EntityCard, JournalView, PlayView},
    world::{PcSight, pc_sight},
};
use threshold_ollama::OllamaConfig;

/// The config file the app reads from its data dir.
pub const CONFIG_FILE: &str = "threshold.toml";

/// The folder of the data dir that holds the Content Packs, one folder each
/// (data shape 3.8).
pub const PACKS_DIR: &str = "packs";

/// The `op_id` of the debug row that names the loaded and refused packs
/// when a Campaign opens. It holds folder names, pack names and rules,
/// never a line of a pack.
pub const PACKS_OP_ID: &str = "packs";

/// One line of the home menu's pack list (data shape 3.11): a loaded pack by
/// its name, or a refused one by its folder and its problem. It holds no
/// fragment text: a Player may paste the list into a public issue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackLine {
    pub folder: String,
    pub name: Option<String>,
    /// The pack declares that it gives explicit description.
    pub explicit: bool,
    /// Shown, never applied.
    pub recommended_model: Option<String>,
    /// The fixed sentence of the rule that refused the pack, and where.
    pub problem: Option<String>,
}

/// The pack list of `packs` and `problems`, in folder order.
fn pack_lines(packs: &[ContentPack], problems: &[PackProblem]) -> Vec<PackLine> {
    let mut lines: Vec<PackLine> = packs
        .iter()
        .map(|p| PackLine {
            folder: p.folder.clone(),
            name: Some(p.name.clone()),
            explicit: p.explicit(),
            recommended_model: p.recommended_model.clone(),
            problem: None,
        })
        .chain(problems.iter().map(|p| PackLine {
            folder: p.folder.clone(),
            name: None,
            explicit: false,
            recommended_model: None,
            problem: Some(p.text()),
        }))
        .collect();
    lines.sort_by(|a, b| a.folder.cmp(&b.folder));
    lines
}

/// The one refusal of `get_entity_card`, for a hidden entity and for an id
/// that does not exist alike.
pub const UNKNOWN_ENTITY: &str = "the PC does not know this one";

/// Builds the model adapter for each Engine the session opens.
pub type MakeGm = Box<dyn Fn() -> Box<dyn Gm + Send> + Send + Sync>;

#[derive(Debug)]
pub enum AppError {
    NoCampaign,
    DebugOff,
    /// `approve_draft` on a draft that still has gaps: every gap.
    Gaps(Vec<Gap>),
    /// A Refusal (glossary): the Turn was not played, and it did not fail.
    /// The one fixed sentence of its kind (`view::refusal_text`).
    Refused(String),
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
            Self::Refused(text) | Self::Failed(text) => f.write_str(text),
        }
    }
}

pub(crate) fn failed(e: impl fmt::Display) -> AppError {
    AppError::Failed(e.to_string())
}

impl std::error::Error for AppError {}

/// An error reaches the UI as its text, except `Gaps`, the list of gaps,
/// and `Refused`, `{"refusal": <sentence>}`: a Refusal is its own result
/// kind, not a failed Turn (review M3).
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Gaps(gaps) => gaps.serialize(s),
            Self::Refused(text) => {
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry("refusal", text)?;
                map.end()
            }
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

/// The folder in the data dir that removed Campaign files move to.
pub const ARCHIVE_DIR: &str = "archive";

/// The problem line of a Campaign file of an older schema version.
pub const OLD_FILE: &str = "made by an older version of Threshold: import its Source Text again";

/// One Campaign file on the home screen (data shape 3.6). A card with a
/// `problem` cannot open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CampaignCard {
    /// The card's own id: its file name in the campaigns folder. The UI
    /// names a card by this id, never by a path.
    pub id: String,
    pub db_path: String,
    pub pc_name: String,
    pub premise: String,
    /// The change time of the file.
    pub last_played_ms: u64,
    /// The Play and Correction Turns that stand.
    pub turn_count: u32,
    pub needs_begin: bool,
    pub problem: Option<String>,
}

/// The start of the op_id that a Narration told again streams under. The UI
/// holds the same text in `ui/src/api.ts`.
pub const RENARRATE_OP_PREFIX: &str = "__renarrate__";

/// The op_id of the `narration_chunk` events of `renarrate(turn_id)`. It is
/// not the Turn's own op_id, so the Play screen can tell these chunks from
/// those of any Turn.
pub fn renarrate_op_id(turn_id: u64) -> String {
    format!("{RENARRATE_OP_PREFIX}{turn_id}")
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
    /// The Player faded the Narration (Fade to Black): `narration` holds
    /// the Neutral Line.
    pub faded: bool,
}

/// What `fade_to_black` did: `pending` when a Turn was running, so the
/// running command finishes the fade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct FadeResult {
    pub pending: bool,
}

/// "Fade to black" (data shape 3.12): raises the fade flag with no lock,
/// then tries the session lock. A running Turn holds it: the result is
/// `pending`, and that Turn reads the flag at its next chunk and finishes
/// the fade. When the lock is free, the flag goes down again and the
/// Engine fades with no Turn running (`Session::fade`). The shell runs this
/// in a blocking task.
pub fn fade_to_black(
    shared: &Mutex<Session>,
    flag: &AtomicBool,
    op_id: String,
) -> Result<FadeResult, AppError> {
    flag.store(true, Ordering::SeqCst);
    let mut session = match shared.try_lock() {
        Ok(session) => session,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return Ok(FadeResult { pending: true }),
    };
    session.fade(op_id)
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

/// The open Campaign. `player` holds the Notes and Pins on a connection of
/// its own; the `Engine` never sees it, so no Note reaches a prompt.
struct Open {
    engine: Engine,
    campaign: CampaignId,
    db_path: PathBuf,
    player: PlayerStore,
}

/// Milliseconds since the Unix epoch, for the times of a Note.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
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
    /// The fade flag (Fade to Black): `fade_to_black` raises it with no
    /// lock; each Turn command clears it when it starts and reads it
    /// through `TurnControl`.
    fade: Arc<AtomicBool>,
    /// The Content Packs of `<data_dir>/packs`, loaded once at start and
    /// given to every Engine (data shape 3.8).
    packs: Vec<ContentPack>,
    /// The packs the loader refused, by folder and rule.
    pack_problems: Vec<PackProblem>,
}

impl Session {
    /// A session on `data_dir`. It first deletes the `.part` files a crash
    /// left in `campaigns/`, then loads the Content Packs of
    /// `<data_dir>/packs`. A refused pack loads nothing and the app starts:
    /// the pack list shows its folder and rule, and one line on the
    /// standard error names them for the owner, never a line of the pack.
    pub fn new(data_dir: PathBuf, make_gm: MakeGm) -> Self {
        remove_part_files(&data_dir.join("campaigns"));
        let (packs, pack_problems) = pack::load_all(&data_dir.join(PACKS_DIR));
        for problem in &pack_problems {
            eprintln!(
                "threshold: Content Pack folder \"{}\" refused: {}",
                problem.folder,
                problem.text()
            );
        }
        Self {
            data_dir,
            make_gm,
            open: None,
            debug: Arc::new(AtomicBool::new(false)),
            fade: Arc::new(AtomicBool::new(false)),
            packs,
            pack_problems,
        }
    }

    /// The home menu's pack list: every folder of `<data_dir>/packs`, in
    /// name order. No lock is needed by the shell: the list never changes
    /// after start.
    pub fn list_packs(&self) -> Vec<PackLine> {
        pack_lines(&self.packs, &self.pack_problems)
    }

    /// The debug row of the packs, written when a Campaign opens: one line
    /// per loaded pack and per refused folder, with names and rules only.
    /// Nothing when the packs folder held nothing.
    fn log_packs(&self, store: &Store) -> Result<(), AppError> {
        let lines: Vec<String> = self
            .list_packs()
            .into_iter()
            .map(|line| match (&line.name, &line.problem) {
                (_, Some(problem)) => format!("refused: folder {}: {problem}", line.folder),
                (name, None) => format!(
                    "loaded: {} (folder {}), explicit description: {}",
                    name.as_deref().unwrap_or_default(),
                    line.folder,
                    if line.explicit { "yes" } else { "no" }
                ),
            })
            .collect();
        if lines.is_empty() {
            return Ok(());
        }
        store
            .log_debug(DebugEntry {
                op_id: PACKS_OP_ID.into(),
                step: "packs".into(),
                prompt: String::new(),
                raw: lines.join("\n"),
                ms: 0,
            })
            .map_err(failed)
    }

    /// Where new campaign files go.
    pub fn campaigns_dir(&self) -> PathBuf {
        self.data_dir.join("campaigns")
    }

    /// A card for each Campaign file in the data dir, the latest played
    /// first. Each file is read with `store::peek`, which writes nothing; a
    /// file that cannot open gets a card with its problem.
    pub fn list_campaigns(&self) -> Vec<CampaignCard> {
        let mut cards: Vec<CampaignCard> = fs::read_dir(self.campaigns_dir())
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "sqlite"))
            .map(|path| campaign_card(&path))
            .collect();
        cards.sort_by(|a, b| {
            b.last_played_ms
                .cmp(&a.last_played_ms)
                .then_with(|| b.db_path.cmp(&a.db_path))
        });
        cards
    }

    /// The folder where "Remove from list" moves a Campaign file.
    pub fn archive_dir(&self) -> PathBuf {
        self.data_dir.join(ARCHIVE_DIR)
    }

    /// "Remove from list" (QA, 2026-11-10): moves the file of the card
    /// `card_id` into `archive_dir`, never deletes it. `card_id` is a card's
    /// own id as `list_campaigns` gives it, never a path: only a file the
    /// list shows can move, and only one the list reports with a problem
    /// (changed after review: the shell enforces it, not the UI). The open
    /// Campaign cannot. Its SQLite side files move with it; a side file
    /// that cannot move is reported. Returns the new path.
    pub fn remove_campaign(&mut self, card_id: &str) -> Result<PathBuf, AppError> {
        let card = self
            .list_campaigns()
            .into_iter()
            .find(|c| c.id == card_id)
            .ok_or_else(|| failed("this Campaign is not in the list"))?;
        let from = PathBuf::from(&card.db_path);
        if self.open.as_ref().is_some_and(|o| o.db_path == from) {
            return Err(failed("close this Campaign before you remove it"));
        }
        if card.problem.is_none() {
            return Err(failed(
                "this Campaign opens, so it stays in the list: only a file that cannot open can be removed",
            ));
        }
        let archive = self.archive_dir();
        fs::create_dir_all(&archive).map_err(failed)?;
        let stem = from
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut to = archive.join(&card.id);
        let mut n = 1;
        while to.exists() {
            to = archive.join(format!("{stem}-{n}.sqlite"));
            n += 1;
        }
        fs::rename(&from, &to).map_err(failed)?;
        let mut stuck = Vec::new();
        for side in ["-wal", "-shm", "-journal"] {
            let file = PathBuf::from(format!("{}{side}", from.display()));
            if file.exists()
                && let Err(e) = fs::rename(&file, PathBuf::from(format!("{}{side}", to.display())))
            {
                stuck.push(format!("{} ({e})", file.display()));
            }
        }
        if !stuck.is_empty() {
            return Err(failed(format!(
                "the Campaign file moved to {}, but these side files did not: {}",
                to.display(),
                stuck.join(", ")
            )));
        }
        Ok(to)
    }

    /// Creates a campaign file from an Adventure Seed and a world file, and
    /// opens it. A failed setup leaves no file. Nobody confirmed the ages of
    /// such a file, so it gives no `ConfirmedAges`: every age stays
    /// unconfirmed.
    pub fn new_campaign(
        &mut self,
        seed_path: &Path,
        world_path: &Path,
        rng_seed: u64,
    ) -> Result<CampaignSummary, AppError> {
        let seed = load_adventure_seed(seed_path).map_err(failed)?;
        let world = load_world_setup(world_path).map_err(failed)?;
        let db_path = new_campaign_path(&self.campaigns_dir())?;
        create_campaign_file(&db_path, &seed, &world, rng_seed, None).map_err(failed)?;
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
        let player = PlayerStore::open(&db_path).map_err(failed)?;
        self.log_packs(&store)?;
        let engine = Engine::new(
            store,
            (self.make_gm)(),
            Box::new(D10),
            self.packs.clone(),
            campaign,
        );
        self.open = Some(Open {
            engine,
            campaign,
            db_path,
            player,
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
        let needs_begin = !stored.iter().any(|t| t.record.kind.tells());
        Ok(CampaignSummary {
            db_path: open.db_path.display().to_string(),
            pc_name: seed.map(|s| s.pc.name.clone()).unwrap_or_default(),
            premise: seed.map(|s| s.premise.clone()).unwrap_or_default(),
            needs_begin,
        })
    }

    /// The fade flag, cleared: a Turn command calls it when it starts.
    fn fresh_fade(&self) -> Arc<AtomicBool> {
        self.fade.store(false, Ordering::SeqCst);
        self.fade.clone()
    }

    /// The fade flag itself, for the shell to raise without locking the
    /// session.
    pub fn fade_flag(&self) -> Arc<AtomicBool> {
        self.fade.clone()
    }

    pub fn submit_intent(
        &mut self,
        op_id: String,
        text: &str,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnSummary, AppError> {
        let debug = self.debug();
        let fade = self.fresh_fade();
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        let mut ctl = TurnControl {
            on_chunk,
            faded: &|| fade.load(Ordering::SeqCst),
        };
        let played = open.engine.play_turn_with(op_id, text, &mut ctl);
        committed(&open.engine, played, debug)
    }

    /// Begin: the Opening Turn of the open Campaign. A second call returns
    /// the stored Turn.
    pub fn begin(&mut self, on_chunk: &mut dyn FnMut(&str)) -> Result<TurnSummary, AppError> {
        let debug = self.debug();
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        let played = open.engine.begin(on_chunk);
        committed(&open.engine, played, debug)
    }

    pub fn correct_last(
        &mut self,
        op_id: String,
        clarified: &str,
        why: &str,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnSummary, AppError> {
        let debug = self.debug();
        let fade = self.fresh_fade();
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        let mut ctl = TurnControl {
            on_chunk,
            faded: &|| fade.load(Ordering::SeqCst),
        };
        let played = open
            .engine
            .correct_last_with(op_id, clarified, why, &mut ctl);
        committed(&open.engine, played, debug)
    }

    /// "End scene": the Closing Turn of the open Intimate Scene. Its short
    /// Narration streams like an Intent's.
    pub fn end_scene(
        &mut self,
        op_id: String,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<TurnSummary, AppError> {
        let debug = self.debug();
        let fade = self.fresh_fade();
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        let mut ctl = TurnControl {
            on_chunk,
            faded: &|| fade.load(Ordering::SeqCst),
        };
        let played = open.engine.end_scene(op_id, &mut ctl);
        committed(&open.engine, played, debug)
    }

    /// "Fade to black" with no Turn running (`fade_to_black` holds the
    /// lock): with a scene open, a Closing Turn born faded; with none,
    /// nothing, and no model call.
    pub fn fade(&mut self, op_id: String) -> Result<FadeResult, AppError> {
        let debug = self.debug();
        self.fresh_fade();
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        match open.engine.fade(op_id) {
            Ok(_) | Err(TurnError::NothingToFade) => Ok(FadeResult { pending: false }),
            Err(e) => Err(player_error(&e, debug)),
        }
    }

    pub fn ask_ooc(&mut self, text: &str) -> Result<String, AppError> {
        let debug = self.debug();
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        open.engine
            .ooc(format!("ooc-{}", clock_nanos()), text)
            .map_err(|e| player_error(&e, debug))
    }

    pub fn renarrate(
        &mut self,
        turn_id: u64,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, AppError> {
        let debug = self.debug();
        let fade = self.fresh_fade();
        let open = self.open.as_mut().ok_or(AppError::NoCampaign)?;
        let mut ctl = TurnControl {
            on_chunk,
            faded: &|| fade.load(Ordering::SeqCst),
        };
        open.engine
            .renarrate_with(turn_id, &mut ctl)
            .map_err(|e| player_error(&e, debug))
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

    /// The Player's Notes and Pins on the open Campaign.
    fn marks(&self) -> Result<Marks, AppError> {
        self.open()?.player.marks().map_err(failed)
    }

    /// A new Note, free or about `about`.
    pub fn add_note(&self, body: &str, about: Option<Target>) -> Result<Note, AppError> {
        self.open()?
            .player
            .add_note(body, about, now_ms())
            .map_err(failed)
    }

    pub fn edit_note(&self, note_id: u64, body: &str) -> Result<Note, AppError> {
        self.open()?
            .player
            .edit_note(note_id, body, now_ms())
            .map_err(failed)
    }

    /// Removes a Note and its Pin.
    pub fn delete_note(&self, note_id: u64) -> Result<(), AppError> {
        self.open()?.player.delete_note(note_id).map_err(failed)
    }

    /// Pins `target`, or removes its Pin. The screens show a Pin where its
    /// target stands now, so removing it also removes each stored Pin that
    /// shows as `target` (a Pin on a Turn that a Correction replaced).
    pub fn set_pin(&self, target: &Target, on: bool) -> Result<(), AppError> {
        let player = &self.open()?.player;
        if on {
            return player.set_pin(target, true).map_err(failed);
        }
        let shown = view::pins_resolving_to(&self.sight()?, &self.marks()?, target);
        for pin in std::iter::once(target).chain(&shown) {
            player.set_pin(pin, false).map_err(failed)?;
        }
        Ok(())
    }

    /// The PC as the PC knows itself: only its own Beliefs about itself.
    pub fn get_character(&self) -> Result<CharacterView, AppError> {
        Ok(view::character_view(&self.sight()?))
    }

    /// The Play screen: the story and the side panel, from PC Knowledge
    /// only. The UI calls it after a Turn ends, never during one.
    pub fn get_play_view(&self) -> Result<PlayView, AppError> {
        Ok(view::play_view_with(
            &self.sight()?,
            &self.marks()?,
            &self.packs,
        ))
    }

    /// The card of one person, place or Faction the PC knows. Any other id
    /// gets the same refusal, so the Player cannot learn that a hidden
    /// entity exists by probing ids.
    pub fn get_entity_card(&self, entity_id: u64) -> Result<EntityCard, AppError> {
        view::entity_card(&self.sight()?, &self.marks()?, entity_id)
            .ok_or_else(|| failed(UNKNOWN_ENTITY))
    }

    /// The Journal: one entry per Scene, the Open Items, and what the PC
    /// knew before the Campaign, from PC Knowledge only.
    pub fn get_journal(&self) -> Result<JournalView, AppError> {
        Ok(view::journal_view(&self.sight()?, &self.marks()?))
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

/// The home card of one Campaign file.
fn campaign_card(path: &Path) -> CampaignCard {
    let last_played_ms = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64);
    let mut card = CampaignCard {
        id: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        db_path: path.display().to_string(),
        pc_name: String::new(),
        premise: String::new(),
        last_played_ms,
        turn_count: 0,
        needs_begin: false,
        problem: None,
    };
    match store::peek(path) {
        Ok(peek) => {
            card.pc_name = peek.pc_name;
            card.premise = peek.premise;
            card.turn_count = peek.turn_count;
            card.needs_begin = peek.needs_begin;
        }
        Err(StoreError::Version(found)) if found < store::SCHEMA_VERSION => {
            card.problem = Some(OLD_FILE.into())
        }
        Err(e) => card.problem = Some(format!("this file cannot be opened: {e}")),
    }
    card
}

/// A Turn result as the app reports it. A Turn whose Narration failed after
/// the Commit is a committed Turn without Narration, not an error.
fn committed(
    engine: &Engine,
    played: Result<TurnOutcome, TurnError>,
    debug: bool,
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
                faded: false,
            })
        }
        Err(e) => Err(player_error(&e, debug)),
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
        GmError::Cancelled => "the game model call was cancelled".into(),
    }
}

/// The Player's text for a Turn the checks rejected twice: one plain
/// sentence, no check name (changed after QA, 2026-11-10).
pub const TURN_NOT_SETTLED: &str = "The GM could not settle this Turn. Try to say it another way.";

/// The text of a Turn error that the Player may see. Rejection reasons and
/// model or store messages can quote GM-only facts (a Persistent key, a
/// model's echo), so a rejection shows `TURN_NOT_SETTLED` only; in debug
/// mode the check kinds follow it. A handle would reveal that a hidden fact
/// exists, and its id. The full reasons stay in the debug log.
pub fn player_error(e: &TurnError, debug: bool) -> AppError {
    let text = match e {
        TurnError::EmptyIntent
        | TurnError::IntentTooLong
        | TurnError::NotCorrectable(_)
        | TurnError::PlayStarted
        | TurnError::NoScene
        | TurnError::NothingToFade
        | TurnError::Faded => e.to_string(),
        TurnError::Model(error) => model_error(error),
        TurnError::NarrationFailed { error, .. } => format!(
            "the Turn is committed, but its narration failed: {}",
            model_error(error)
        ),
        TurnError::Rejected(errs) => {
            let mut text = String::from(TURN_NOT_SETTLED);
            if debug {
                text.push_str("\nRejected twice by:");
                for r in errs {
                    text.push_str(&format!("\n- {}", r.check));
                }
                text.push_str("\n(details in the debug view)");
            }
            text
        }
        TurnError::Store(_) => {
            "a storage error stopped the Turn (details in the debug view)".into()
        }
        // A Refusal shows its one fixed sentence, in debug mode too: no
        // name, no id, no age (mature content module, Task 1). It is its
        // own result kind, not a failure (review M3).
        TurnError::Refused { kind, .. } => {
            return AppError::Refused(view::refusal_text(*kind).to_owned());
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
        faded: outcome.faded,
    }
}
