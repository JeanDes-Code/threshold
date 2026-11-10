//! The Tauri shell: thin commands over `session::Session`.
//!
//! Engine work runs in `spawn_blocking`, so a Turn never blocks the async
//! runtime. Narration streams out as `narration_chunk` events, and draft and
//! build progress as `build_progress` events. The app binds no network port;
//! it only calls the local Ollama server.

pub mod drafts;
pub mod session;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use drafts::{DraftLine, DraftView, Drafts};
use serde::Serialize;
use session::{
    AppError, CampaignCard, CampaignSummary, DebugLine, DebugView, Session, TurnSummary,
    clock_nanos, load_config, lock,
};
use tauri::{AppHandle, Emitter, Manager, State};
use threshold_core::{
    build::{BUILD_TIMEOUT_S, KnownFact, Progress},
    domain::AdventureSeed,
    player::{Note, Target},
    rules::RollResult,
    turn::BEGIN_OP_ID,
    view::{CharacterView, EntityCard, JournalView, PlayView},
};
use threshold_ollama::OllamaGm;

/// The session every command shares.
struct Shared(Arc<Mutex<Session>>);

/// The debug toggle, shared with the session but set without its lock, so
/// toggling never waits for a running Turn.
struct DebugFlag(Arc<AtomicBool>);

/// The model's warm-up state: "warming", "ready", or the error text.
struct ModelStatus(Arc<Mutex<String>>);

#[derive(Clone, Serialize)]
struct NarrationChunk {
    op_id: String,
    text: String,
}

#[derive(Clone, Serialize)]
struct TurnCommitted {
    turn_id: u64,
    roll: Option<RollResult>,
}

#[derive(Clone, Serialize)]
struct TurnFailed {
    op_id: String,
    reason: String,
}

#[derive(Serialize)]
struct Defaults {
    seed_path: String,
    world_path: String,
    data_dir: String,
}

/// Runs `f` on the session in a blocking task.
async fn blocking<T, F>(state: &State<'_, Shared>, f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(&mut Session) -> Result<T, AppError> + Send + 'static,
{
    let shared = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || f(&mut lock(&shared)))
        .await
        .map_err(|e| AppError::Failed(e.to_string()))?
}

/// Runs `f` on the draft commands in a blocking task. It takes no session
/// lock: a job locks the session only to open the finished Campaign.
async fn drafting<T, F>(drafts: &State<'_, Drafts>, f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(&Drafts) -> Result<T, AppError> + Send + 'static,
{
    let drafts = drafts.inner().clone();
    tauri::async_runtime::spawn_blocking(move || f(&drafts))
        .await
        .map_err(|e| AppError::Failed(e.to_string()))?
}

/// A sink that sends draft and build progress to the webview.
fn progress_sink(app: &AppHandle) -> impl FnMut(Progress) + Send + 'static {
    let app = app.clone();
    move |progress: Progress| {
        let _ = app.emit("build_progress", progress);
    }
}

/// A sink that sends narration chunks of `op_id` to the webview.
fn chunk_sink(app: &AppHandle, op_id: &str) -> impl FnMut(&str) + Send + 'static {
    let app = app.clone();
    let op_id = op_id.to_owned();
    move |text: &str| {
        let _ = app.emit(
            "narration_chunk",
            NarrationChunk {
                op_id: op_id.clone(),
                text: text.to_owned(),
            },
        );
    }
}

fn report(app: &AppHandle, op_id: &str, result: &Result<TurnSummary, AppError>) {
    let _ = match result {
        Ok(turn) => app.emit(
            "turn_committed",
            TurnCommitted {
                turn_id: turn.turn_id,
                roll: turn.roll,
            },
        ),
        Err(e) => app.emit(
            "turn_failed",
            TurnFailed {
                op_id: op_id.to_owned(),
                reason: e.to_string(),
            },
        ),
    };
}

#[tauri::command(rename_all = "snake_case")]
fn defaults(app: AppHandle) -> Result<Defaults, AppError> {
    // The town demo, found next to the sources in a dev build.
    let town = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/town");
    let town = town.canonicalize().unwrap_or(town);
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Failed(e.to_string()))?;
    Ok(Defaults {
        seed_path: town.join("adventure-seed.toml").display().to_string(),
        world_path: town.join("world.toml").display().to_string(),
        data_dir: data_dir.display().to_string(),
    })
}

#[tauri::command(rename_all = "snake_case")]
fn model_status(status: State<'_, ModelStatus>) -> String {
    lock(&status.0).clone()
}

#[tauri::command(rename_all = "snake_case")]
async fn list_campaigns(state: State<'_, Shared>) -> Result<Vec<CampaignCard>, AppError> {
    blocking(&state, |s| Ok(s.list_campaigns())).await
}

#[tauri::command(rename_all = "snake_case")]
async fn new_campaign(
    state: State<'_, Shared>,
    seed_path: String,
    world_path: String,
) -> Result<CampaignSummary, AppError> {
    blocking(&state, move |s| {
        s.new_campaign(
            &PathBuf::from(seed_path),
            &PathBuf::from(world_path),
            clock_nanos(),
        )
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn open_campaign(
    state: State<'_, Shared>,
    db_path: String,
) -> Result<CampaignSummary, AppError> {
    blocking(&state, move |s| s.open_campaign(&PathBuf::from(db_path))).await
}

/// The Play screen. The UI calls it after `open_campaign` and after each
/// Turn ends: a running Turn holds the session lock, so it never runs
/// inside one.
#[tauri::command(rename_all = "snake_case")]
async fn get_play_view(state: State<'_, Shared>) -> Result<PlayView, AppError> {
    blocking(&state, |s| s.get_play_view()).await
}

#[tauri::command(rename_all = "snake_case")]
async fn submit_intent(
    app: AppHandle,
    state: State<'_, Shared>,
    op_id: String,
    text: String,
) -> Result<TurnSummary, AppError> {
    let mut sink = chunk_sink(&app, &op_id);
    let id = op_id.clone();
    let result = blocking(&state, move |s| s.submit_intent(id, &text, &mut sink)).await;
    report(&app, &op_id, &result);
    result
}

#[tauri::command(rename_all = "snake_case")]
async fn correct_last(
    app: AppHandle,
    state: State<'_, Shared>,
    op_id: String,
    clarified: String,
    why: String,
) -> Result<TurnSummary, AppError> {
    let mut sink = chunk_sink(&app, &op_id);
    let id = op_id.clone();
    let result = blocking(&state, move |s| {
        s.correct_last(id, &clarified, &why, &mut sink)
    })
    .await;
    report(&app, &op_id, &result);
    result
}

#[tauri::command(rename_all = "snake_case")]
async fn ask_ooc(state: State<'_, Shared>, text: String) -> Result<String, AppError> {
    blocking(&state, move |s| s.ask_ooc(&text)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn renarrate(
    app: AppHandle,
    state: State<'_, Shared>,
    turn_id: u64,
) -> Result<String, AppError> {
    blocking(&state, move |s| {
        let mut sink = chunk_sink(&app, &s.turn_op_id(turn_id)?);
        s.renarrate(turn_id, &mut sink)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn get_character(state: State<'_, Shared>) -> Result<CharacterView, AppError> {
    blocking(&state, |s| s.get_character()).await
}

/// One person, place or Faction the PC knows; refused with one fixed text
/// for any other id.
#[tauri::command(rename_all = "snake_case")]
async fn get_entity_card(state: State<'_, Shared>, entity_id: u64) -> Result<EntityCard, AppError> {
    blocking(&state, move |s| s.get_entity_card(entity_id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn get_journal(state: State<'_, Shared>) -> Result<JournalView, AppError> {
    blocking(&state, |s| s.get_journal()).await
}

/// A new Note. The shell stamps its time; no Note reaches the `Engine`.
#[tauri::command(rename_all = "snake_case")]
async fn add_note(
    state: State<'_, Shared>,
    body: String,
    about: Option<Target>,
) -> Result<Note, AppError> {
    blocking(&state, move |s| s.add_note(&body, about)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn edit_note(state: State<'_, Shared>, note_id: u64, body: String) -> Result<Note, AppError> {
    blocking(&state, move |s| s.edit_note(note_id, &body)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn delete_note(state: State<'_, Shared>, note_id: u64) -> Result<(), AppError> {
    blocking(&state, move |s| s.delete_note(note_id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn set_pin(state: State<'_, Shared>, target: Target, on: bool) -> Result<(), AppError> {
    blocking(&state, move |s| s.set_pin(&target, on)).await
}

#[tauri::command(rename_all = "snake_case")]
fn set_debug(flag: State<'_, DebugFlag>, on: bool) -> bool {
    flag.0.store(on, Ordering::SeqCst);
    flag.0.load(Ordering::SeqCst)
}

#[tauri::command(rename_all = "snake_case")]
async fn get_debug(state: State<'_, Shared>, turn_id: u64) -> Result<DebugView, AppError> {
    blocking(&state, move |s| s.get_debug(turn_id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn draft_from_text(
    app: AppHandle,
    drafts: State<'_, Drafts>,
    text: String,
) -> Result<DraftView, AppError> {
    let mut sink = progress_sink(&app);
    drafting(&drafts, move |d| d.draft_from_text(&text, &mut sink)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn list_drafts(drafts: State<'_, Drafts>) -> Result<Vec<DraftLine>, AppError> {
    drafting(&drafts, |d| d.list_drafts()).await
}

#[tauri::command(rename_all = "snake_case")]
async fn get_draft(drafts: State<'_, Drafts>, draft_id: String) -> Result<DraftView, AppError> {
    drafting(&drafts, move |d| d.get_draft(&draft_id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn save_draft(
    drafts: State<'_, Drafts>,
    draft_id: String,
    seed: AdventureSeed,
    knowledge: Vec<KnownFact>,
) -> Result<DraftView, AppError> {
    drafting(&drafts, move |d| d.save_draft(&draft_id, seed, knowledge)).await
}

/// The Player confirms one named age. The core owns every confirmation.
#[tauri::command(rename_all = "snake_case")]
async fn confirm_age(
    drafts: State<'_, Drafts>,
    draft_id: String,
    name: String,
) -> Result<DraftView, AppError> {
    drafting(&drafts, move |d| d.confirm_age(&draft_id, &name)).await
}

/// A rename that follows every reference of the draft.
#[tauri::command(rename_all = "snake_case")]
async fn rename_draft_entity(
    drafts: State<'_, Drafts>,
    draft_id: String,
    from: String,
    to: String,
) -> Result<DraftView, AppError> {
    drafting(&drafts, move |d| {
        d.rename_draft_entity(&draft_id, &from, &to)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn approve_draft(drafts: State<'_, Drafts>, draft_id: String) -> Result<DraftView, AppError> {
    drafting(&drafts, move |d| d.approve_draft(&draft_id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn start_build(
    app: AppHandle,
    drafts: State<'_, Drafts>,
    draft_id: String,
) -> Result<CampaignSummary, AppError> {
    let mut sink = progress_sink(&app);
    drafting(&drafts, move |d| d.start_build(&draft_id, &mut sink)).await
}

#[tauri::command(rename_all = "snake_case")]
fn cancel_build(drafts: State<'_, Drafts>) {
    drafts.cancel_build();
}

#[tauri::command(rename_all = "snake_case")]
async fn delete_draft(drafts: State<'_, Drafts>, draft_id: String) -> Result<(), AppError> {
    drafting(&drafts, move |d| d.delete_draft(&draft_id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn get_build_debug(
    drafts: State<'_, Drafts>,
    draft_id: String,
) -> Result<Vec<DebugLine>, AppError> {
    drafting(&drafts, move |d| d.get_build_debug(&draft_id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn begin(app: AppHandle, state: State<'_, Shared>) -> Result<TurnSummary, AppError> {
    let mut sink = chunk_sink(&app, BEGIN_OP_ID);
    let result = blocking(&state, move |s| s.begin(&mut sink)).await;
    report(&app, BEGIN_OP_ID, &result);
    result
}

/// Loads the config, starts the warm-up call without blocking the window,
/// and installs the session.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    let config = load_config(&data_dir)?;

    let status = Arc::new(Mutex::new(String::from("warming")));
    app.manage(ModelStatus(status.clone()));
    let handle = app.handle().clone();
    let warm = config.clone();
    std::thread::spawn(move || {
        let now = match OllamaGm::new(warm).warm_up() {
            Ok(()) => "ready".to_owned(),
            Err(e) => e.to_string(),
        };
        *lock(&status) = now.clone();
        let _ = handle.emit("model_status", now);
    });

    // Drafts and builds get their own model, with a longer timeout; play
    // keeps the config as it is.
    let mut build_config = config.clone();
    build_config.timeout_s = BUILD_TIMEOUT_S;

    let session = Session::new(
        data_dir.clone(),
        Box::new(move || Box::new(OllamaGm::new(config.clone()))),
    );
    app.manage(DebugFlag(session.debug_flag()));
    let session = Arc::new(Mutex::new(session));
    let drafts = Drafts::new(
        &data_dir,
        session.clone(),
        Box::new(move || Box::new(OllamaGm::new(build_config.clone()))),
    )?;
    app.manage(drafts);
    app.manage(Shared(session));
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            defaults,
            model_status,
            list_campaigns,
            new_campaign,
            open_campaign,
            get_play_view,
            submit_intent,
            correct_last,
            ask_ooc,
            renarrate,
            get_character,
            get_entity_card,
            get_journal,
            add_note,
            edit_note,
            delete_note,
            set_pin,
            set_debug,
            get_debug,
            draft_from_text,
            list_drafts,
            get_draft,
            save_draft,
            confirm_age,
            rename_draft_entity,
            approve_draft,
            start_build,
            cancel_build,
            begin,
            delete_draft,
            get_build_debug,
        ])
        .run(tauri::generate_context!())
        .expect("Threshold failed to start");
}
