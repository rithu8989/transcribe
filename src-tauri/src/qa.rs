use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub word: String,
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub interpolated: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QaReport {
    pub pass: bool,
    pub word_count: usize,
    pub issues: Vec<String>,
    pub interpolated_words: usize,
    pub low_confidence_words: usize,
    pub max_gap_seconds: f64,
    pub duration_delta_seconds: f64,
}

/// Validate the word timeline before the user is allowed to trust an export.
///
/// `media_duration` comes from ffprobe on the ORIGINAL file, independent of
/// the engine, so a drifting transcript cannot self-certify.
pub fn validate(words: &[Word], media_duration: f64, engine_duration: f64, interpolated: usize) -> QaReport {
    let mut issues = Vec::new();
    let tolerance = 0.05; // 50ms slack for rounding

    let mut prev_end = 0.0_f64;
    let mut max_gap = 0.0_f64;
    let mut low_confidence = 0usize;

    for (i, w) in words.iter().enumerate() {
        if w.end < w.start - 1e-6 {
            issues.push(format!(
                "word {} ('{}') ends before it starts ({:.3} -> {:.3})",
                i, w.word, w.start, w.end
            ));
        }
        if w.start < prev_end - tolerance {
            issues.push(format!(
                "word {} ('{}') starts at {:.3}, before previous word ended at {:.3} (non-monotonic)",
                i, w.word, w.start, prev_end
            ));
        }
        if w.end > media_duration + tolerance {
            issues.push(format!(
                "word {} ('{}') ends at {:.3}, beyond media duration {:.3}",
                i, w.word, w.end, media_duration
            ));
        }
        if i > 0 {
            let gap = w.start - prev_end;
            if gap > max_gap {
                max_gap = gap;
            }
        }
        if w.confidence.unwrap_or(1.0) < 0.5 {
            low_confidence += 1;
        }
        prev_end = prev_end.max(w.end);
    }

    if words.is_empty() {
        issues.push("no words were transcribed".to_string());
    }

    let duration_delta = (engine_duration - media_duration).abs();
    if duration_delta > 1.0 {
        issues.push(format!(
            "engine saw {engine_duration:.2}s of audio but the media is {media_duration:.2}s (delta {duration_delta:.2}s)"
        ));
    }

    QaReport {
        pass: issues.is_empty(),
        word_count: words.len(),
        issues,
        interpolated_words: interpolated,
        low_confidence_words: low_confidence,
        max_gap_seconds: (max_gap * 1000.0).round() / 1000.0,
        duration_delta_seconds: (duration_delta * 1000.0).round() / 1000.0,
    }
}
