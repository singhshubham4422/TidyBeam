//! Saved organize rules: previews, reviewed moves and opt-in background moves.

use crate::{
    platform::{self, FileEntry},
    transfer::{self, Conflict, QuietObserver, TransferSummary},
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeRule {
    pub id: String,
    pub name: String,
    pub source: String,
    pub destination: String,
    pub extensions: Vec<String>,
    pub min_age_days: u64,
    #[serde(default)]
    pub automatic: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RulePreview {
    pub rule: OrganizeRule,
    pub files: Vec<FileEntry>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RuleRunActivity {
    rule_id: String,
    rule_name: String,
    completed: usize,
}

type ActiveRules = HashMap<String, (PathBuf, RecommendedWatcher)>;

#[derive(Default)]
pub struct RuleWatcher {
    active: Mutex<ActiveRules>,
    pub execution: Arc<Mutex<()>>,
}

fn rules_file(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    Ok(directory.join("organize-rules.json"))
}

pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Can't find the settings folder")?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(
        ".tidybeam-settings-{}-{stamp}.tmp",
        std::process::id()
    ));
    let result = (|| -> Result<(), String> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(contents).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub fn read_rules(app: &AppHandle) -> Result<Vec<OrganizeRule>, String> {
    let path = rules_file(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("Couldn't read organize rules: {e}"))
}

fn write_rules(app: &AppHandle, rules: &[OrganizeRule]) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(rules).map_err(|e| e.to_string())?;
    atomic_write(&rules_file(app)?, &json)
}

pub fn validate(mut rule: OrganizeRule) -> Result<OrganizeRule, String> {
    rule.name = rule.name.trim().to_string();
    if rule.name.is_empty() || rule.name.chars().count() > 80 {
        return Err("Rule names need 1–80 characters".into());
    }
    let source = platform::canonical(Path::new(rule.source.trim()))
        .map_err(|e| format!("Source folder: {e}"))?;
    let destination = platform::canonical(Path::new(rule.destination.trim()))
        .map_err(|e| format!("Destination folder: {e}"))?;
    if !source.is_dir() || !destination.is_dir() {
        return Err("Choose existing source and destination folders".into());
    }
    if platform::same_or_descendant(&destination, &source) {
        return Err("The destination can't be inside the source folder".into());
    }
    rule.source = platform::path_string(&source);
    rule.destination = platform::path_string(&destination);
    let mut extensions = rule
        .extensions
        .into_iter()
        .map(|ext| ext.trim().trim_start_matches('.').to_lowercase())
        .filter(|ext| !ext.is_empty())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    extensions.sort();
    if extensions.iter().any(|ext| {
        ext.len() > 32
            || !ext
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    }) {
        return Err(
            "Extensions may contain letters, numbers, dots, underscores and hyphens".into(),
        );
    }
    rule.extensions = extensions;
    rule.min_age_days = rule.min_age_days.min(36_500);
    if rule.automatic && rule.min_age_days == 0 {
        return Err("Automatic rules need files to be at least one day old".into());
    }
    if rule.id.trim().is_empty() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        rule.id = format!("rule-{stamp}");
    }
    Ok(rule)
}

pub fn save(app: &AppHandle, rule: OrganizeRule) -> Result<Vec<OrganizeRule>, String> {
    let rule = validate(rule)?;
    let mut rules = read_rules(app)?;
    match rules.iter_mut().find(|existing| existing.id == rule.id) {
        Some(existing) => *existing = rule,
        None => rules.push(rule),
    }
    write_rules(app, &rules)?;
    Ok(rules)
}

pub fn delete(app: &AppHandle, id: &str) -> Result<Vec<OrganizeRule>, String> {
    let mut rules = read_rules(app)?;
    rules.retain(|rule| rule.id != id);
    write_rules(app, &rules)?;
    Ok(rules)
}

pub fn preview(app: &AppHandle, id: &str) -> Result<RulePreview, String> {
    let rule = read_rules(app)?
        .into_iter()
        .find(|rule| rule.id == id)
        .ok_or("Rule not found")?;
    let source = PathBuf::from(&rule.source);
    if !source.is_dir() || !Path::new(&rule.destination).is_dir() {
        return Err("The rule's source or destination folder is unavailable".into());
    }
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(
            rule.min_age_days.saturating_mul(86_400),
        ))
        .unwrap_or(UNIX_EPOCH);
    let extensions = rule.extensions.iter().cloned().collect::<HashSet<_>>();
    let mut files = Vec::new();
    for item in fs::read_dir(&source)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
    {
        let path = item.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_file() {
            continue;
        }
        let extension = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if !extensions.is_empty() && !extensions.contains(&extension) {
            continue;
        }
        if metadata.modified().map(|m| m > cutoff).unwrap_or(true) {
            continue;
        }
        files.push(platform::entry_from_metadata(&path, &metadata));
    }
    files.sort_by(|a, b| {
        crate::listing::natural_cmp(&a.name.to_lowercase(), &b.name.to_lowercase())
    });
    Ok(RulePreview { rule, files })
}

pub fn apply(app: &AppHandle, id: &str, selected: Vec<String>) -> Result<TransferSummary, String> {
    let preview = preview(app, id)?;
    let eligible = preview
        .files
        .into_iter()
        .map(|file| file.path)
        .collect::<HashSet<_>>();
    let sources = selected
        .into_iter()
        .filter(|path| eligible.contains(path))
        .collect::<Vec<_>>();
    transfer::run(
        &sources,
        Path::new(&preview.rule.destination),
        true,
        Conflict::KeepBoth,
        &mut QuietObserver::default(),
    )
}

fn run_automatic(app: &AppHandle, id: &str) -> Result<(), String> {
    let preview = preview(app, id)?;
    if !preview.rule.automatic || preview.files.is_empty() {
        return Ok(());
    }
    let name = preview.rule.name.clone();
    let selected = preview.files.into_iter().map(|f| f.path).collect();
    let summary = apply(app, id, selected)?;
    emit_activity(app, id, name, summary.completed);
    Ok(())
}

fn emit_activity(app: &AppHandle, id: &str, rule_name: String, completed: usize) {
    if completed > 0 {
        let _ = app.emit(
            "organize-rule-run",
            RuleRunActivity {
                rule_id: id.into(),
                rule_name,
                completed,
            },
        );
    }
}

fn run_watcher(
    app: AppHandle,
    id: String,
    receiver: mpsc::Receiver<notify::Result<notify::Event>>,
    execution: Arc<Mutex<()>>,
) {
    let run = |app: &AppHandle| {
        let _guard = execution
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let enabled = read_rules(app)
            .map(|rules| rules.iter().any(|r| r.id == id && r.automatic))
            .unwrap_or(false);
        if enabled && let Err(error) = run_automatic(app, &id) {
            eprintln!("TidyBeam automatic rule failed: {error}");
        }
    };
    run(&app);
    loop {
        match receiver.recv_timeout(Duration::from_secs(15 * 60)) {
            Ok(_) => {
                // Let bursts of events settle before sweeping.
                while receiver.recv_timeout(Duration::from_millis(500)).is_ok() {}
                run(&app);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => run(&app),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn start(app: &AppHandle, state: &RuleWatcher, rule: &OrganizeRule) -> Result<(), String> {
    let root = platform::canonical(Path::new(&rule.source)).map_err(|e| e.to_string())?;
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = sender.send(event);
    })
    .map_err(|e| format!("Couldn't start the rule watcher: {e}"))?;
    watcher
        .watch(&root, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Couldn't watch the source folder: {e}"))?;
    let (worker_app, id, execution) = (app.clone(), rule.id.clone(), state.execution.clone());
    std::thread::Builder::new()
        .name("tidybeam-organize-rule".into())
        .spawn(move || run_watcher(worker_app, id, receiver, execution))
        .map_err(|e| e.to_string())?;
    state
        .active
        .lock()
        .map_err(|e| e.to_string())?
        .insert(rule.id.clone(), (PathBuf::from(&rule.source), watcher));
    Ok(())
}

pub fn reconcile(
    app: &AppHandle,
    state: &RuleWatcher,
    rules: &[OrganizeRule],
) -> Result<(), String> {
    let desired = rules
        .iter()
        .filter(|rule| rule.automatic)
        .map(|rule| (rule.id.clone(), PathBuf::from(&rule.source)))
        .collect::<HashMap<_, _>>();
    let missing = {
        let mut active = state.active.lock().map_err(|e| e.to_string())?;
        active.retain(|id, (root, _)| desired.get(id) == Some(root));
        desired
            .keys()
            .filter(|id| !active.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>()
    };
    let errors = missing
        .iter()
        .filter_map(|id| rules.iter().find(|rule| &rule.id == id))
        .filter_map(|rule| {
            start(app, state, rule)
                .err()
                .map(|e| format!("{}: {e}", rule.name))
        })
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_replaced_atomically() {
        let dir = std::env::temp_dir().join(format!("tidybeam-rules-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("rules.json");
        fs::write(&file, b"old").unwrap();
        atomic_write(&file, b"new").unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"new");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rule_destination_inside_source_is_rejected() {
        let dir =
            std::env::temp_dir().join(format!("tidybeam-rule-validate-{}", std::process::id()));
        fs::create_dir_all(dir.join("inner")).unwrap();
        let rule = OrganizeRule {
            id: String::new(),
            name: "Test".into(),
            source: dir.to_string_lossy().into(),
            destination: dir.join("inner").to_string_lossy().into(),
            extensions: vec![".PDF".into()],
            min_age_days: 3,
            automatic: false,
        };
        assert!(validate(rule).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
