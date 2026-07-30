mod engine;
mod history;
mod icon;
mod media;
mod qa;

use std::path::PathBuf;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use engine::EngineState;

fn emit_stage(app: &AppHandle, stage: &str) {
    let _ = app.emit("engine-event", json!({ "type": "stage", "stage": stage }));
}

#[tauri::command]
async fn transcribe(
    app: AppHandle,
    state: State<'_, EngineState>,
    path: String,
    engine: String,
    model: Option<String>,
) -> Result<Value, String> {
    let input = PathBuf::from(&path);
    if !input.exists() {
        return Err(format!("file not found: {path}"));
    }

    // 1. Probe the true media duration (independent of the engine).
    emit_stage(&app, "extract");
    let media_duration = media::probe_duration(&input)?;

    // 2. Extract 16kHz mono WAV.
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("no cache dir: {e}"))?;
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let wav_path = cache.join("current.wav");
    let input_clone = input.clone();
    let wav_clone = wav_path.clone();
    tauri::async_runtime::spawn_blocking(move || media::extract_wav(&input_clone, &wav_clone))
        .await
        .map_err(|e| e.to_string())??;

    // 3. Run the engine, streaming progress events.
    let app_clone = app.clone();
    let wav_str = wav_path.to_string_lossy().to_string();
    let engine_name = engine.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app_clone.state::<EngineState>();
        engine::run(&app_clone, &state, &wav_str, &engine_name, model.as_deref())
    })
    .await
    .map_err(|e| e.to_string())??;

    // 4. QA gate: validate the timeline against the independent duration.
    emit_stage(&app, "qa");
    let words: Vec<qa::Word> = serde_json::from_value(result["words"].clone())
        .map_err(|e| format!("bad words payload from engine: {e}"))?;
    let engine_duration = result["duration"].as_f64().unwrap_or(0.0);
    let interpolated = result["interpolatedWords"].as_u64().unwrap_or(0) as usize;
    let report = qa::validate(&words, media_duration, engine_duration, interpolated);

    let mut out = result;
    out["qa"] = serde_json::to_value(&report).map_err(|e| e.to_string())?;
    out["sourceFile"] = json!(path);
    out["mediaDuration"] = json!(media_duration);

    // 5. Persist to history.
    let hist = app.state::<history::HistoryState>();
    match history::save_record(&app, &hist, &out) {
        Ok(meta) => {
            out["historyId"] = json!(meta.id);
            out["name"] = json!(meta.name);
            out["version"] = json!(meta.version);
            out["groupId"] = json!(meta.group_id);
        }
        Err(e) => eprintln!("failed to save history record: {e}"),
    }

    let _ = state; // state used via app handle inside spawn_blocking
    Ok(out)
}

#[tauri::command]
fn history_list(
    app: AppHandle,
    state: State<'_, history::HistoryState>,
) -> Result<Vec<history::HistoryGroup>, String> {
    history::list(&app, &state)
}

#[tauri::command]
fn history_get(app: AppHandle, id: String) -> Result<Value, String> {
    history::get(&app, &id)
}

#[tauri::command]
fn history_rename(
    app: AppHandle,
    state: State<'_, history::HistoryState>,
    group_id: String,
    name: String,
) -> Result<(), String> {
    history::rename(&app, &state, &group_id, &name)
}

#[tauri::command]
fn history_delete(
    app: AppHandle,
    state: State<'_, history::HistoryState>,
    group_id: String,
) -> Result<(), String> {
    history::delete(&app, &state, &group_id)
}

#[tauri::command]
fn history_restore(
    app: AppHandle,
    state: State<'_, history::HistoryState>,
    group_id: String,
) -> Result<(), String> {
    history::restore(&app, &state, &group_id)
}

#[tauri::command]
fn history_hard_delete(
    app: AppHandle,
    state: State<'_, history::HistoryState>,
    group_id: String,
) -> Result<(), String> {
    history::hard_delete_group(&app, &state, &group_id)
}

#[tauri::command]
fn history_hard_delete_version(
    app: AppHandle,
    state: State<'_, history::HistoryState>,
    id: String,
) -> Result<(), String> {
    history::hard_delete_version(&app, &state, &id)
}

#[tauri::command]
fn history_empty_bin(
    app: AppHandle,
    state: State<'_, history::HistoryState>,
) -> Result<usize, String> {
    history::empty_bin(&app, &state)
}

#[tauri::command]
fn cancel_transcription(state: State<'_, EngineState>) -> bool {
    engine::cancel(&state)
}

#[tauri::command]
fn write_text_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, contents).map_err(|e| format!("could not write {path}: {e}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(EngineState::default())
        .manage(history::HistoryState::default())
        .invoke_handler(tauri::generate_handler![
            transcribe,
            cancel_transcription,
            write_text_file,
            history_list,
            history_get,
            history_rename,
            history_delete,
            history_restore,
            history_hard_delete,
            history_hard_delete_version,
            history_empty_bin,
            icon::set_app_icon_theme
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
