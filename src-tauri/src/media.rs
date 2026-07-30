use std::path::{Path, PathBuf};
use std::process::Command;

/// GUI apps launched from Finder don't inherit the shell PATH, so probe the
/// usual install locations explicitly.
pub fn find_binary(name: &str) -> Option<PathBuf> {
    let candidates = [
        format!("/opt/homebrew/bin/{name}"),
        format!("/usr/local/bin/{name}"),
        format!("/usr/bin/{name}"),
    ];
    for c in candidates {
        let p = PathBuf::from(&c);
        if p.exists() {
            return Some(p);
        }
    }
    // fall back to PATH
    which(name)
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|p| p.exists())
    })
}

/// Exact duration of the source media in seconds, via ffprobe.
pub fn probe_duration(input: &Path) -> Result<f64, String> {
    let ffprobe = find_binary("ffprobe").ok_or("ffprobe not found; install ffmpeg (brew install ffmpeg)")?;
    let out = Command::new(ffprobe)
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(input)
        .output()
        .map_err(|e| format!("ffprobe failed to start: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "ffprobe error: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .map_err(|e| format!("could not parse duration: {e}"))
}

/// Extract audio from any container to 16kHz mono WAV for the engine.
pub fn extract_wav(input: &Path, output: &Path) -> Result<(), String> {
    let ffmpeg = find_binary("ffmpeg").ok_or("ffmpeg not found; install ffmpeg (brew install ffmpeg)")?;
    let out = Command::new(ffmpeg)
        .args(["-y", "-i"])
        .arg(input)
        .args(["-vn", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(output)
        .output()
        .map_err(|e| format!("ffmpeg failed to start: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "ffmpeg could not extract audio: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}
