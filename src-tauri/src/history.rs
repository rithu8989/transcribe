use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

/// Serializes access to index.json across concurrent commands.
#[derive(Default)]
pub struct HistoryState(pub Mutex<()>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMeta {
    pub id: String,
    /// Stable id shared by every version of the same source file.
    #[serde(default)]
    pub group_id: String,
    /// 1-based version number within the group.
    #[serde(default = "default_version")]
    pub version: u32,
    pub name: String,
    pub created_at: u64,
    #[serde(default)]
    pub deleted_at: Option<u64>,
    pub engine: String,
    pub duration: f64,
    pub word_count: usize,
    pub source_file: String,
    pub qa_pass: bool,
}

fn default_version() -> u32 {
    1
}

/// One history row in the UI: a source file with its version timeline.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryGroup {
    pub group_id: String,
    pub name: String,
    pub source_file: String,
    pub deleted_at: Option<u64>,
    /// Newest first.
    pub versions: Vec<HistoryMeta>,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn history_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("no app data dir: {e}"))?
        .join("history");
    fs::create_dir_all(dir.join("records")).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn record_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    if id.contains('/') || id.contains("..") {
        return Err("invalid record id".to_string());
    }
    Ok(history_dir(app)?.join("records").join(format!("{id}.json")))
}

fn load_index_raw(app: &AppHandle) -> Result<Vec<HistoryMeta>, String> {
    let path = history_dir(app)?.join("index.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("corrupt history index: {e}"))
}

fn save_index(app: &AppHandle, index: &[HistoryMeta]) -> Result<(), String> {
    let path = history_dir(app)?.join("index.json");
    let raw = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| e.to_string())
}

/// Backfill group_id / version and merge same-source entries into version chains.
fn migrate_index(mut index: Vec<HistoryMeta>) -> (Vec<HistoryMeta>, bool) {
    let mut changed = false;

    // Fill missing group_id (legacy entries used id alone).
    for m in index.iter_mut() {
        if m.group_id.is_empty() {
            m.group_id = m.id.clone();
            changed = true;
        }
        if m.version == 0 {
            m.version = 1;
            changed = true;
        }
    }

    // Merge separate groups that share the same source file + deleted state.
    // Keep the oldest group_id; assign versions by created_at ascending.
    let mut buckets: HashMap<(String, bool), Vec<usize>> = HashMap::new();
    for (i, m) in index.iter().enumerate() {
        let key = (m.source_file.clone(), m.deleted_at.is_some());
        buckets.entry(key).or_default().push(i);
    }

    for indices in buckets.values() {
        if indices.len() < 2 {
            continue;
        }
        let mut sorted = indices.clone();
        sorted.sort_by_key(|&i| index[i].created_at);
        let canonical = index[sorted[0]].group_id.clone();
        let shared_name = index[sorted[0]].name.clone();
        for (v, &i) in sorted.iter().enumerate() {
            let ver = (v + 1) as u32;
            if index[i].group_id != canonical
                || index[i].version != ver
                || index[i].name != shared_name
            {
                index[i].group_id = canonical.clone();
                index[i].version = ver;
                // Prefer an existing custom name if the first is just the filename.
                if v == 0 {
                    // keep
                } else if index[i].name != shared_name {
                    // If a later entry was renamed differently, keep the newest custom name for the group.
                }
                changed = true;
            }
        }
        // Use the most recently renamed-looking name: prefer non-stem mismatches from newest.
        let newest = *sorted.last().unwrap();
        let name = index[newest].name.clone();
        for &i in &sorted {
            if index[i].name != name {
                index[i].name = name.clone();
                changed = true;
            }
        }
    }

    (index, changed)
}

fn load_index(app: &AppHandle) -> Result<Vec<HistoryMeta>, String> {
    let raw = load_index_raw(app)?;
    let (index, changed) = migrate_index(raw);
    if changed {
        save_index(app, &index)?;
    }
    Ok(index)
}

fn to_groups(index: &[HistoryMeta]) -> Vec<HistoryGroup> {
    let mut order: Vec<String> = Vec::new();
    let mut map: HashMap<String, Vec<HistoryMeta>> = HashMap::new();

    for m in index {
        if !map.contains_key(&m.group_id) {
            order.push(m.group_id.clone());
        }
        map.entry(m.group_id.clone()).or_default().push(m.clone());
    }

    // Sort groups by newest version created_at.
    order.sort_by(|a, b| {
        let a_max = map[a].iter().map(|m| m.created_at).max().unwrap_or(0);
        let b_max = map[b].iter().map(|m| m.created_at).max().unwrap_or(0);
        b_max.cmp(&a_max)
    });

    order
        .into_iter()
        .filter_map(|gid| {
            let mut versions = map.remove(&gid)?;
            versions.sort_by(|a, b| b.version.cmp(&a.version));
            let head = versions.first()?;
            Some(HistoryGroup {
                group_id: gid,
                name: head.name.clone(),
                source_file: head.source_file.clone(),
                deleted_at: head.deleted_at,
                versions,
            })
        })
        .collect()
}

/// Persist a finished transcription; returns its metadata.
///
/// Same source file → new version under the existing group.
/// Near-simultaneous duplicate (≤30s) → overwrite the latest version instead.
pub fn save_record(app: &AppHandle, state: &HistoryState, result: &Value) -> Result<HistoryMeta, String> {
    let _guard = state.0.lock().unwrap();

    let source_file = result["sourceFile"].as_str().unwrap_or("").to_string();
    let engine = result["engine"].as_str().unwrap_or("unknown").to_string();
    let name_from_file = PathBuf::from(&source_file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Untitled".to_string());

    let created = now_millis();
    let mut index = load_index(app)?;

    const DEDUPE_WINDOW_MS: u64 = 30_000;

    // Active versions for this source file (not in bin).
    let mut active: Vec<usize> = index
        .iter()
        .enumerate()
        .filter(|(_, m)| m.deleted_at.is_none() && m.source_file == source_file)
        .map(|(i, _)| i)
        .collect();
    active.sort_by_key(|&i| index[i].version);

    if let Some(&latest_i) = active.last() {
        let latest = &index[latest_i];
        // Duplicate fire → overwrite latest version in place.
        if created.saturating_sub(latest.created_at) < DEDUPE_WINDOW_MS {
            let id = latest.id.clone();
            let group_id = latest.group_id.clone();
            let version = latest.version;
            let name = if latest.name != name_from_file && !latest.name.is_empty() {
                latest.name.clone()
            } else {
                name_from_file
            };
            let meta = HistoryMeta {
                id: id.clone(),
                group_id,
                version,
                name,
                created_at: latest.created_at,
                deleted_at: None,
                engine,
                duration: result["mediaDuration"].as_f64().unwrap_or(0.0),
                word_count: result["words"].as_array().map(|a| a.len()).unwrap_or(0),
                source_file,
                qa_pass: result["qa"]["pass"].as_bool().unwrap_or(false),
            };
            let raw = serde_json::to_string(result).map_err(|e| e.to_string())?;
            fs::write(record_path(app, &id)?, raw).map_err(|e| e.to_string())?;
            index[latest_i] = meta.clone();
            save_index(app, &index)?;
            return Ok(meta);
        }

        // Intentional re-run → next version in the same group.
        let group_id = latest.group_id.clone();
        let name = latest.name.clone();
        let version = latest.version + 1;
        let id = format!("{created:x}-v{version}");
        let meta = HistoryMeta {
            id: id.clone(),
            group_id,
            version,
            name,
            created_at: created,
            deleted_at: None,
            engine,
            duration: result["mediaDuration"].as_f64().unwrap_or(0.0),
            word_count: result["words"].as_array().map(|a| a.len()).unwrap_or(0),
            source_file,
            qa_pass: result["qa"]["pass"].as_bool().unwrap_or(false),
        };
        let raw = serde_json::to_string(result).map_err(|e| e.to_string())?;
        fs::write(record_path(app, &id)?, raw).map_err(|e| e.to_string())?;
        index.insert(0, meta.clone());
        save_index(app, &index)?;
        return Ok(meta);
    }

    // Brand new source file → group v1.
    let id = format!("{created:x}-v1");
    let meta = HistoryMeta {
        id: id.clone(),
        group_id: id.clone(),
        version: 1,
        name: name_from_file,
        created_at: created,
        deleted_at: None,
        engine,
        duration: result["mediaDuration"].as_f64().unwrap_or(0.0),
        word_count: result["words"].as_array().map(|a| a.len()).unwrap_or(0),
        source_file,
        qa_pass: result["qa"]["pass"].as_bool().unwrap_or(false),
    };
    let raw = serde_json::to_string(result).map_err(|e| e.to_string())?;
    fs::write(record_path(app, &id)?, raw).map_err(|e| e.to_string())?;
    index.insert(0, meta.clone());
    save_index(app, &index)?;
    Ok(meta)
}

/// Remove accidental near-duplicates within a group (same version window).
fn cleanup_duplicates(app: &AppHandle) -> Result<usize, String> {
    let index = load_index_raw(app)?;
    let (mut index, migrated) = migrate_index(index);
    const WINDOW_MS: u64 = 30_000;
    let mut remove_ids: Vec<String> = Vec::new();

    // Per group, drop older entries created within WINDOW of a newer one with same engine.
    let mut by_group: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, m) in index.iter().enumerate() {
        if m.deleted_at.is_some() {
            continue;
        }
        by_group.entry(m.group_id.clone()).or_default().push(i);
    }
    for indices in by_group.values() {
        let mut sorted = indices.clone();
        sorted.sort_by_key(|&i| std::cmp::Reverse(index[i].created_at));
        let mut keepers: Vec<usize> = Vec::new();
        for &i in &sorted {
            let is_dup = keepers.iter().any(|&k| {
                index[k].engine == index[i].engine
                    && index[k].created_at.saturating_sub(index[i].created_at) < WINDOW_MS
            });
            if is_dup {
                remove_ids.push(index[i].id.clone());
            } else {
                keepers.push(i);
            }
        }
    }

    if remove_ids.is_empty() {
        if migrated {
            save_index(app, &index)?;
        }
        return Ok(0);
    }

    for id in &remove_ids {
        let path = record_path(app, id)?;
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }
    index.retain(|m| !remove_ids.contains(&m.id));
    // Re-number versions in each group after cleanup.
    renumber_versions(&mut index);
    save_index(app, &index)?;
    Ok(remove_ids.len())
}

fn renumber_versions(index: &mut [HistoryMeta]) {
    let mut by_group: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, m) in index.iter().enumerate() {
        by_group.entry(m.group_id.clone()).or_default().push(i);
    }
    for indices in by_group.values() {
        let mut sorted = indices.clone();
        sorted.sort_by_key(|&i| index[i].created_at);
        for (v, &i) in sorted.iter().enumerate() {
            index[i].version = (v + 1) as u32;
        }
    }
}

pub fn list(app: &AppHandle, state: &HistoryState) -> Result<Vec<HistoryGroup>, String> {
    let _guard = state.0.lock().unwrap();
    let _ = cleanup_duplicates(app);
    let index = load_index(app)?;
    Ok(to_groups(&index))
}

pub fn get(app: &AppHandle, id: &str) -> Result<Value, String> {
    let path = record_path(app, id)?;
    let raw = fs::read_to_string(&path).map_err(|e| format!("record not found: {e}"))?;
    let mut value: Value = serde_json::from_str(&raw).map_err(|e| format!("corrupt record: {e}"))?;

    // Attach version meta from the index when available.
    if let Ok(index) = load_index_raw(app) {
        if let Some(m) = index.iter().find(|m| m.id == id) {
            value["historyId"] = Value::String(m.id.clone());
            value["name"] = Value::String(m.name.clone());
            value["version"] = Value::from(m.version);
            value["groupId"] = Value::String(m.group_id.clone());
        }
    }
    Ok(value)
}

/// Rename every version in the group.
pub fn rename(app: &AppHandle, state: &HistoryState, group_id: &str, name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    let _guard = state.0.lock().unwrap();
    let mut index = load_index(app)?;
    let mut found = false;
    for m in index.iter_mut() {
        if m.group_id == group_id {
            m.name = trimmed.to_string();
            found = true;
        }
    }
    if !found {
        return Err("history entry not found".to_string());
    }
    save_index(app, &index)
}

/// Soft-delete the whole group (all versions → bin).
pub fn delete(app: &AppHandle, state: &HistoryState, group_id: &str) -> Result<(), String> {
    let _guard = state.0.lock().unwrap();
    let mut index = load_index(app)?;
    let now = now_millis();
    let mut found = false;
    for m in index.iter_mut() {
        if m.group_id == group_id {
            m.deleted_at = Some(now);
            found = true;
        }
    }
    if !found {
        return Err("history entry not found".to_string());
    }
    save_index(app, &index)
}

pub fn restore(app: &AppHandle, state: &HistoryState, group_id: &str) -> Result<(), String> {
    let _guard = state.0.lock().unwrap();
    let mut index = load_index(app)?;
    let mut found = false;
    for m in index.iter_mut() {
        if m.group_id == group_id {
            m.deleted_at = None;
            found = true;
        }
    }
    if !found {
        return Err("history entry not found".to_string());
    }
    save_index(app, &index)
}

/// Hard-delete one version. If it was the last version in the group, the group disappears.
pub fn hard_delete_version(app: &AppHandle, state: &HistoryState, id: &str) -> Result<(), String> {
    let _guard = state.0.lock().unwrap();
    let mut index = load_index(app)?;
    let before = index.len();
    let group_id = index
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.group_id.clone());
    index.retain(|m| m.id != id);
    if index.len() == before {
        return Err("history entry not found".to_string());
    }
    let path = record_path(app, id)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    if let Some(gid) = group_id {
        let remaining: Vec<_> = index.iter().filter(|m| m.group_id == gid).collect();
        if !remaining.is_empty() {
            renumber_versions(&mut index);
        }
    }
    save_index(app, &index)
}

/// Hard-delete an entire group (all versions).
pub fn hard_delete_group(app: &AppHandle, state: &HistoryState, group_id: &str) -> Result<(), String> {
    let _guard = state.0.lock().unwrap();
    let mut index = load_index(app)?;
    let ids: Vec<String> = index
        .iter()
        .filter(|m| m.group_id == group_id)
        .map(|m| m.id.clone())
        .collect();
    if ids.is_empty() {
        return Err("history entry not found".to_string());
    }
    for id in &ids {
        let path = record_path(app, id)?;
        if path.exists() {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    index.retain(|m| m.group_id != group_id);
    save_index(app, &index)
}

/// Hard delete everything currently in the bin.
pub fn empty_bin(app: &AppHandle, state: &HistoryState) -> Result<usize, String> {
    let _guard = state.0.lock().unwrap();
    let mut index = load_index(app)?;
    let (binned, kept): (Vec<_>, Vec<_>) = index.drain(..).partition(|m| m.deleted_at.is_some());
    for meta in &binned {
        let path = record_path(app, &meta.id)?;
        if path.exists() {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    save_index(app, &kept)?;
    // Count unique groups removed.
    let mut groups = std::collections::HashSet::new();
    for m in &binned {
        groups.insert(m.group_id.clone());
    }
    Ok(groups.len())
}
