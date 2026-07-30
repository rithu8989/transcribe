use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::media::find_binary;

/// Holds the currently running engine process so it can be cancelled.
/// `busy` prevents two transcriptions from starting at once (e.g. duplicate
/// drop listeners in React StrictMode).
#[derive(Default)]
pub struct EngineState {
    pub child: Mutex<Option<Child>>,
    pub busy: Mutex<bool>,
}

fn engine_dir() -> PathBuf {
    // Dev layout: <repo>/src-tauri (this crate) next to <repo>/engine.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine")
}

/// Spawn the Python engine and stream its NDJSON output.
///
/// Every `stage`/`progress` line is forwarded to the frontend as an
/// `engine-event`. The final `result` object is returned.
pub fn run(
    app: &AppHandle,
    state: &EngineState,
    wav_path: &str,
    engine: &str,
    model: Option<&str>,
) -> Result<Value, String> {
    {
        let mut busy = state.busy.lock().unwrap();
        if *busy {
            return Err("a transcription is already running".to_string());
        }
        *busy = true;
    }

    let result = run_inner(app, state, wav_path, engine, model);

    *state.busy.lock().unwrap() = false;
    result
}

fn run_inner(
    app: &AppHandle,
    state: &EngineState,
    wav_path: &str,
    engine: &str,
    model: Option<&str>,
) -> Result<Value, String> {
    let uv = find_binary("uv").ok_or("uv not found; install it with: brew install uv")?;
    let dir = engine_dir();
    let script = dir.join("engine.py");
    if !script.exists() {
        return Err(format!("engine script not found at {}", script.display()));
    }

    let mut cmd = Command::new(uv);
    cmd.arg("run")
        .arg("--project")
        .arg(&dir)
        .arg(&script)
        .args(["--audio", wav_path, "--engine", engine]);
    if let Some(m) = model {
        cmd.args(["--model", m]);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start engine: {e}"))?;
    let stdout = child.stdout.take().ok_or("engine stdout unavailable")?;

    *state.child.lock().unwrap() = Some(child);

    let reader = BufReader::new(stdout);
    let mut result: Option<Value> = None;
    let mut engine_error: Option<String> = None;

    for line in reader.lines() {
        let line = line.map_err(|e| format!("engine read error: {e}"))?;
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue; // ignore stray non-JSON output
        };
        match msg.get("type").and_then(Value::as_str) {
            Some("result") => result = Some(msg),
            Some("error") => {
                engine_error = Some(
                    msg.get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown engine error")
                        .to_string(),
                );
            }
            _ => {
                let _ = app.emit("engine-event", &msg);
            }
        }
    }

    let status = {
        let mut guard = state.child.lock().unwrap();
        let status = guard.as_mut().map(|c| c.wait());
        *guard = None;
        status
    };

    if let Some(err) = engine_error {
        return Err(err);
    }
    match status {
        Some(Ok(s)) if !s.success() => {
            return Err(format!("engine exited with status {s}"));
        }
        Some(Err(e)) => return Err(format!("engine wait failed: {e}")),
        _ => {}
    }

    result.ok_or_else(|| "engine produced no result (was it cancelled?)".to_string())
}

pub fn cancel(state: &EngineState) -> bool {
    let mut guard = state.child.lock().unwrap();
    if let Some(child) = guard.as_mut() {
        let _ = child.kill();
        *guard = None;
        *state.busy.lock().unwrap() = false;
        true
    } else {
        false
    }
}
