//! The Tauri shell: thin commands over `session::Session`.
//!
//! Engine work runs in `spawn_blocking`, so a Turn never blocks the async
//! runtime. Narration streams out as `narration_chunk` events. The app binds
//! no network port; it only calls the local Ollama server.

pub mod session;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use serde::Serialize;
use session::{
    AppError, CampaignSummary, CharacterView, DebugView, JournalView, Session, TurnSummary,
    clock_nanos, load_config,
};
use tauri::{AppHandle, Emitter, Manager, State};
use threshold_core::rules::RollResult;
use threshold_ollama::OllamaGm;

/// The session every command shares.
struct Shared(Arc<Mutex<Session>>);

/// The debug toggle, shared with the session but set without its lock, so
/// toggling never waits for a running Turn.
struct DebugFlag(Arc<AtomicBool>);

/// The model's warm-up state: "warming", "ready", or the error text.
struct ModelStatus(Arc<Mutex<String>>);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

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
async fn list_campaigns(state: State<'_, Shared>) -> Result<Vec<String>, AppError> {
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

#[tauri::command(rename_all = "snake_case")]
async fn get_journal(state: State<'_, Shared>) -> Result<JournalView, AppError> {
    blocking(&state, |s| s.get_journal()).await
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

    let session = Session::new(
        data_dir,
        Box::new(move || Box::new(OllamaGm::new(config.clone()))),
    );
    app.manage(DebugFlag(session.debug_flag()));
    app.manage(Shared(Arc::new(Mutex::new(session))));
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
            submit_intent,
            correct_last,
            ask_ooc,
            renarrate,
            get_character,
            get_journal,
            set_debug,
            get_debug,
        ])
        .run(tauri::generate_context!())
        .expect("Threshold failed to start");
}
