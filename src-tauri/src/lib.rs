//! TidyBeam: Tauri command layer. Heavy work runs on blocking worker threads.

mod analysis;
mod archive;
mod index;
mod listing;
mod platform;
mod rules;
mod transfer;

use index::{IndexDb, IndexWatchers};
use listing::{DirectorySnapshots, ListRequest, SortKey};
use platform::FileEntry;
use serde::Serialize;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use transfer::{CopyObserver, TaskControl, TransferTasks};

type CmdResult<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

fn home(app: &AppHandle) -> PathBuf {
    app.path().home_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

fn resolve(app: &AppHandle, raw: &str) -> CmdResult<PathBuf> {
    listing::resolve_input(raw, &home(app))
}

// ---------- Platform and places ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformInfo {
    os: &'static str,
    trash_name: &'static str,
    separator: char,
    home: String,
    version: String,
}

#[tauri::command]
fn platform_info(app: AppHandle) -> PlatformInfo {
    PlatformInfo {
        os: if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        },
        trash_name: platform::trash_name(),
        separator: std::path::MAIN_SEPARATOR,
        home: platform::path_string(&home(&app)),
        version: app.package_info().version.to_string(),
    }
}

#[derive(Serialize)]
struct Place {
    name: String,
    path: String,
    kind: &'static str,
}

#[tauri::command]
fn user_places(app: AppHandle) -> Vec<Place> {
    let resolver = app.path();
    let candidates: [(&str, &'static str, Option<PathBuf>); 6] = [
        ("Home", "home", resolver.home_dir().ok()),
        ("Desktop", "desktop", resolver.desktop_dir().ok()),
        ("Documents", "documents", resolver.document_dir().ok()),
        ("Downloads", "downloads", resolver.download_dir().ok()),
        ("Pictures", "pictures", resolver.picture_dir().ok()),
        ("Music", "music", resolver.audio_dir().ok()),
    ];
    candidates
        .into_iter()
        .filter_map(|(name, kind, path)| {
            let path = path?;
            path.is_dir().then(|| Place {
                name: name.into(),
                path: platform::path_string(&path),
                kind,
            })
        })
        .collect()
}

#[derive(Serialize)]
struct DriveInfo {
    name: String,
    path: String,
    total: u64,
    available: u64,
    file_system: String,
    removable: bool,
}

#[tauri::command]
async fn list_drives() -> CmdResult<Vec<DriveInfo>> {
    blocking(|| {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let mut seen = std::collections::HashSet::new();
        Ok(disks
            .iter()
            .filter_map(|disk| {
                let mount = platform::path_string(disk.mount_point());
                let file_system = disk.file_system().to_string_lossy().into_owned();
                if !platform::is_user_visible_mount(&mount, &file_system)
                    || !seen.insert(mount.clone())
                {
                    return None;
                }
                let label = disk.name().to_string_lossy().trim().to_string();
                let name = if mount == "/" {
                    if cfg!(target_os = "macos") {
                        "Macintosh HD".into()
                    } else {
                        "System".into()
                    }
                } else if cfg!(windows) {
                    let letter = mount.trim_end_matches('\\');
                    if label.is_empty() {
                        format!("Local Disk ({letter})")
                    } else {
                        format!("{label} ({letter})")
                    }
                } else {
                    Path::new(&mount)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or(label)
                };
                Some(DriveInfo {
                    name,
                    path: mount,
                    total: disk.total_space(),
                    available: disk.available_space(),
                    file_system,
                    removable: disk.is_removable(),
                })
            })
            .collect())
    })
    .await
}

#[tauri::command]
fn path_info(app: AppHandle, path: String) -> CmdResult<listing::PathInfo> {
    Ok(listing::path_info(&resolve(&app, &path)?))
}

// ---------- Listing ----------

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn list_entries(
    app: AppHandle,
    snapshots: State<'_, DirectorySnapshots>,
    path: String,
    offset: usize,
    limit: usize,
    show_hidden: bool,
    sort: String,
    descending: bool,
    filter: String,
    force: bool,
) -> CmdResult<listing::ListResult> {
    let snapshots = snapshots.inner().clone();
    let directory = resolve(&app, &path)?;
    blocking(move || {
        listing::list_entries(
            &snapshots,
            &directory,
            ListRequest {
                offset,
                limit,
                show_hidden,
                sort: SortKey::parse(&sort),
                descending,
                filter,
                force,
            },
        )
    })
    .await
}

#[tauri::command]
async fn compare_folders(
    left: String,
    right: String,
    show_hidden: bool,
) -> CmdResult<listing::FolderComparison> {
    blocking(move || listing::compare_folders(Path::new(&left), Path::new(&right), show_hidden))
        .await
}

// ---------- Preview and opening ----------

#[derive(Serialize)]
struct TextPreview {
    text: String,
    truncated: bool,
}

#[tauri::command]
async fn read_text_preview(path: String) -> CmdResult<TextPreview> {
    blocking(move || {
        const MAX: u64 = 256 * 1024;
        let file = PathBuf::from(path);
        let metadata = fs::metadata(&file).map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            return Err("Preview is available for files only".into());
        }
        let mut bytes = Vec::with_capacity(metadata.len().min(MAX) as usize);
        fs::File::open(&file)
            .map_err(|e| e.to_string())?
            .take(MAX)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.iter().take(4096).any(|&b| b == 0) {
            return Err("This looks like a binary file".into());
        }
        Ok(TextPreview {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            truncated: metadata.len() > MAX,
        })
    })
    .await
}

/// Opens an item with its default app. Programs and scripts need explicit confirmation.
#[tauri::command]
async fn open_item(app: AppHandle, path: String, confirmed: bool) -> CmdResult<()> {
    if platform::needs_launch_confirmation(Path::new(&path))? && !confirmed {
        return Err("confirm-launch".into());
    }
    blocking(move || {
        app.opener()
            .open_path(path, None::<&str>)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn reveal_item(app: AppHandle, path: String) -> CmdResult<()> {
    blocking(move || {
        app.opener()
            .reveal_item_in_dir(PathBuf::from(path))
            .map_err(|e| e.to_string())
    })
    .await
}

// ---------- Basic file operations ----------

#[tauri::command]
fn create_folder(path: String, name: String) -> CmdResult<String> {
    let name = platform::validate_name(&name)?;
    let target = PathBuf::from(path).join(&name);
    fs::create_dir(&target).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => format!("“{name}” already exists"),
        _ => e.to_string(),
    })?;
    Ok(platform::path_string(&target))
}

#[tauri::command]
fn rename_path(path: String, new_name: String) -> CmdResult<String> {
    let old = PathBuf::from(path);
    let name = platform::validate_name(&new_name)?;
    let parent = old.parent().ok_or("This location can't be renamed")?;
    let new_path = parent.join(&name);
    if new_path == old {
        return Ok(platform::path_string(&old));
    }
    // fs::rename replaces existing files, so refuse unless the target is this same
    // item under a different letter case (allowed on case-insensitive volumes).
    if fs::symlink_metadata(&new_path).is_ok() && !platform::same_existing_path(&new_path, &old) {
        return Err(format!("“{name}” already exists in this folder"));
    }
    fs::rename(&old, &new_path).map_err(|e| e.to_string())?;
    Ok(platform::path_string(&new_path))
}

#[tauri::command]
fn undo_created_folder(path: String) -> CmdResult<()> {
    let folder = PathBuf::from(path);
    let metadata = fs::symlink_metadata(&folder)
        .map_err(|_| "The created folder no longer exists".to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("The created folder is no longer a regular folder".into());
    }
    if fs::read_dir(&folder)
        .map_err(|e| e.to_string())?
        .next()
        .is_some()
    {
        return Err("The folder isn't empty anymore, so it was kept".into());
    }
    fs::remove_dir(&folder).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct FailedItem {
    path: String,
    error: String,
}

#[derive(Serialize)]
struct TrashResult {
    trashed: usize,
    failed: Vec<FailedItem>,
}

/// Paths that just failed to go to the trash. Permanent deletion is only allowed for
/// these, so the interface can't be used to delete arbitrary paths outright.
#[derive(Default)]
struct TrashFailures(Mutex<std::collections::HashSet<String>>);

#[tauri::command]
async fn trash_paths(
    failures: State<'_, TrashFailures>,
    paths: Vec<String>,
) -> CmdResult<TrashResult> {
    let result = blocking(move || {
        let mut result = TrashResult {
            trashed: 0,
            failed: Vec::new(),
        };
        for raw in paths {
            let path = PathBuf::from(&raw);
            if fs::symlink_metadata(&path).is_err() {
                continue;
            }
            match platform::trash_one(&path) {
                Ok(()) => result.trashed += 1,
                Err(error) => result.failed.push(FailedItem { path: raw, error }),
            }
        }
        Ok(result)
    })
    .await?;
    let mut set = failures.0.lock().map_err(|e| e.to_string())?;
    set.clear();
    set.extend(
        result
            .failed
            .iter()
            .filter(|item| !platform::is_protected(Path::new(&item.path)))
            .map(|item| item.path.clone()),
    );
    Ok(result)
}

/// Only called after the user confirmed that items which couldn't go to the trash
/// should be deleted for good.
#[tauri::command]
async fn delete_permanently(
    failures: State<'_, TrashFailures>,
    paths: Vec<String>,
) -> CmdResult<usize> {
    let allowed = {
        let mut set = failures.0.lock().map_err(|e| e.to_string())?;
        let allowed = paths
            .into_iter()
            .filter(|p| set.contains(p))
            .collect::<Vec<_>>();
        set.clear();
        allowed
    };
    if allowed.is_empty() {
        return Err("Nothing to delete. Try moving the items to the trash again.".into());
    }
    blocking(move || {
        let mut deleted = 0;
        let mut errors = Vec::new();
        for raw in allowed {
            match platform::delete_permanently(Path::new(&raw)) {
                Ok(()) => deleted += 1,
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(deleted)
        } else {
            Err(errors.join("\n"))
        }
    })
    .await
}

// ---------- Transfers ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TransferProgress {
    task_id: String,
    status: String,
    completed_bytes: u64,
    total_bytes: u64,
    completed_items: usize,
    total_items: usize,
    current_name: String,
    speed_bytes_per_second: u64,
    message: Option<String>,
}

struct TransferReporter {
    app: AppHandle,
    control: Arc<TaskControl>,
    progress: TransferProgress,
    last_emit: Instant,
    last_bytes: u64,
}

impl TransferReporter {
    fn emit(&mut self, status: &str, message: Option<String>) {
        let elapsed = self.last_emit.elapsed().as_secs_f64();
        self.progress.speed_bytes_per_second = if status == "running" && elapsed > 0.05 {
            (self
                .progress
                .completed_bytes
                .saturating_sub(self.last_bytes) as f64
                / elapsed) as u64
        } else {
            self.progress.speed_bytes_per_second
        };
        self.progress.status = status.into();
        self.progress.message = message;
        let _ = self.app.emit("transfer-progress", self.progress.clone());
        self.last_emit = Instant::now();
        self.last_bytes = self.progress.completed_bytes;
    }
}

impl CopyObserver for TransferReporter {
    fn checkpoint(&mut self) -> Result<(), String> {
        let paused = *self.control.paused.lock().map_err(|e| e.to_string())?;
        if paused {
            self.emit("paused", None);
        }
        if self.control.wait_if_paused()? {
            self.last_emit = Instant::now();
            self.emit("running", None);
        }
        Ok(())
    }
    fn bytes(&mut self, count: u64) {
        self.progress.completed_bytes = self.progress.completed_bytes.saturating_add(count);
        if self.last_emit.elapsed() >= Duration::from_millis(200) {
            self.emit("running", None);
        }
    }
    fn item_started(&mut self, name: &str) {
        self.progress.current_name = name.into();
        self.emit("running", None);
    }
    fn item_finished(&mut self) {
        self.progress.completed_items += 1;
    }
}

#[derive(Serialize)]
struct TransferPreview {
    conflicts: Vec<transfer::TransferConflict>,
}

#[tauri::command]
async fn preview_transfer(sources: Vec<String>, destination: String) -> CmdResult<TransferPreview> {
    blocking(move || {
        Ok(TransferPreview {
            conflicts: transfer::preview(&sources, Path::new(&destination))?,
        })
    })
    .await
}

#[tauri::command]
fn start_transfer(
    app: AppHandle,
    tasks: State<'_, TransferTasks>,
    task_id: String,
    sources: Vec<String>,
    destination: String,
    move_items: bool,
    conflict: String,
) -> CmdResult<()> {
    if task_id.is_empty() || sources.is_empty() {
        return Err("A transfer needs at least one item".into());
    }
    let conflict = transfer::Conflict::parse(&conflict)?;
    let control = Arc::new(TaskControl::default());
    let controls = tasks.controls.clone();
    {
        let mut active = controls.lock().map_err(|e| e.to_string())?;
        if active.contains_key(&task_id) {
            return Err("That transfer is already running".into());
        }
        active.insert(task_id.clone(), control.clone());
    }
    let store = controls.clone();
    let key = task_id.clone();
    let spawned = std::thread::Builder::new()
        .name("tidybeam-transfer".into())
        .spawn(move || {
            let mut reporter = TransferReporter {
                app,
                control: control.clone(),
                progress: TransferProgress {
                    task_id: task_id.clone(),
                    status: "running".into(),
                    completed_bytes: 0,
                    total_bytes: 0,
                    completed_items: 0,
                    total_items: sources.len(),
                    current_name: "Preparing…".into(),
                    speed_bytes_per_second: 0,
                    message: None,
                },
                last_emit: Instant::now(),
                last_bytes: 0,
            };
            reporter.emit("running", None);
            reporter.progress.total_bytes = sources
                .iter()
                .map(|s| transfer::path_size(Path::new(s)))
                .fold(0, u64::saturating_add);
            let result = transfer::run(
                &sources,
                Path::new(&destination),
                move_items,
                conflict,
                &mut reporter,
            );
            match result {
                Ok(summary) => {
                    let mut message = format!("{} done", summary.completed);
                    if summary.skipped > 0 {
                        message.push_str(&format!(" · {} skipped", summary.skipped));
                    }
                    if summary.skipped_links > 0 {
                        message.push_str(&format!(
                            " · {} symbolic links not copied",
                            summary.skipped_links
                        ));
                    }
                    reporter.emit("completed", Some(message));
                }
                Err(_) if control.is_cancelled() => reporter.emit("cancelled", None),
                Err(error) => reporter.emit("failed", Some(error)),
            }
            if let Ok(mut tasks) = store.lock() {
                tasks.remove(&task_id);
            }
        });
    if let Err(error) = spawned {
        if let Ok(mut active) = controls.lock() {
            active.remove(&key);
        }
        return Err(error.to_string());
    }
    Ok(())
}

fn task(tasks: &TransferTasks, id: &str) -> CmdResult<Arc<TaskControl>> {
    tasks
        .controls
        .lock()
        .map_err(|e| e.to_string())?
        .get(id)
        .cloned()
        .ok_or_else(|| "That transfer has finished".into())
}

#[tauri::command]
fn pause_transfer(tasks: State<'_, TransferTasks>, task_id: String) -> CmdResult<()> {
    *task(&tasks, &task_id)?
        .paused
        .lock()
        .map_err(|e| e.to_string())? = true;
    Ok(())
}

#[tauri::command]
fn resume_transfer(tasks: State<'_, TransferTasks>, task_id: String) -> CmdResult<()> {
    let control = task(&tasks, &task_id)?;
    *control.paused.lock().map_err(|e| e.to_string())? = false;
    control.wake.notify_all();
    Ok(())
}

#[tauri::command]
fn cancel_transfer(tasks: State<'_, TransferTasks>, task_id: String) -> CmdResult<()> {
    let control = task(&tasks, &task_id)?;
    control
        .cancelled
        .store(true, std::sync::atomic::Ordering::Release);
    *control.paused.lock().map_err(|e| e.to_string())? = false;
    control.wake.notify_all();
    Ok(())
}

// ---------- Rules ----------

#[tauri::command]
async fn list_rules(app: AppHandle) -> CmdResult<Vec<rules::OrganizeRule>> {
    blocking(move || rules::read_rules(&app)).await
}

#[tauri::command]
async fn save_rule(
    app: AppHandle,
    watcher: State<'_, rules::RuleWatcher>,
    rule: rules::OrganizeRule,
) -> CmdResult<Vec<rules::OrganizeRule>> {
    let (write_app, execution) = (app.clone(), watcher.execution.clone());
    let saved = blocking(move || {
        let _guard = execution.lock().map_err(|e| e.to_string())?;
        rules::save(&write_app, rule)
    })
    .await?;
    rules::reconcile(&app, &watcher, &saved)
        .map_err(|e| format!("Rule saved, but its background watcher couldn't start: {e}"))?;
    Ok(saved)
}

#[tauri::command]
async fn preview_rule(app: AppHandle, id: String) -> CmdResult<rules::RulePreview> {
    blocking(move || rules::preview(&app, &id)).await
}

#[tauri::command]
async fn apply_rule(
    app: AppHandle,
    watcher: State<'_, rules::RuleWatcher>,
    id: String,
    selected: Vec<String>,
) -> CmdResult<transfer::TransferSummary> {
    let execution = watcher.execution.clone();
    blocking(move || {
        let _guard = execution.lock().map_err(|e| e.to_string())?;
        rules::apply(&app, &id, selected)
    })
    .await
}

#[tauri::command]
async fn delete_rule(
    app: AppHandle,
    watcher: State<'_, rules::RuleWatcher>,
    id: String,
) -> CmdResult<Vec<rules::OrganizeRule>> {
    let (write_app, execution) = (app.clone(), watcher.execution.clone());
    let saved = blocking(move || {
        let _guard = execution.lock().map_err(|e| e.to_string())?;
        rules::delete(&write_app, &id)
    })
    .await?;
    rules::reconcile(&app, &watcher, &saved)?;
    Ok(saved)
}

// ---------- Analysis ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AnalysisProgress {
    scan_id: String,
    files_scanned: u64,
    folders_scanned: u64,
}

#[tauri::command]
async fn analyze_folder(
    app: AppHandle,
    path: String,
    scan_id: String,
) -> CmdResult<analysis::Analysis> {
    let root = resolve(&app, &path)?;
    blocking(move || {
        analysis::analyze(&root, |files, folders| {
            let _ = app.emit(
                "analysis-progress",
                AnalysisProgress {
                    scan_id: scan_id.clone(),
                    files_scanned: files,
                    folders_scanned: folders,
                },
            );
        })
    })
    .await
}

#[tauri::command]
async fn find_duplicates(app: AppHandle, path: String) -> CmdResult<Vec<analysis::DuplicateGroup>> {
    let root = resolve(&app, &path)?;
    blocking(move || analysis::duplicates(&root)).await
}

#[tauri::command]
async fn scan_cleanup(app: AppHandle) -> CmdResult<Vec<analysis::CleanupCandidate>> {
    let home = home(&app);
    blocking(move || Ok(analysis::cleanup_candidates(&home))).await
}

// ---------- Archives ----------

#[tauri::command]
async fn list_archive(path: String) -> CmdResult<Vec<archive::ArchiveEntry>> {
    blocking(move || archive::list(Path::new(&path))).await
}

#[tauri::command]
async fn extract_archive(path: String, destination: String) -> CmdResult<usize> {
    blocking(move || archive::extract(Path::new(&path), Path::new(&destination))).await
}

#[tauri::command]
async fn create_zip(
    paths: Vec<String>,
    destination: String,
    archive_name: String,
) -> CmdResult<String> {
    blocking(move || archive::create(&paths, Path::new(&destination), &archive_name)).await
}

// ---------- Search index ----------

struct IndexState {
    db: Mutex<Option<Arc<IndexDb>>>,
    watchers: Arc<IndexWatchers>,
}

fn index_db(app: &AppHandle) -> CmdResult<Arc<IndexDb>> {
    let state = app.state::<IndexState>();
    let mut slot = state.db.lock().map_err(|e| e.to_string())?;
    if let Some(db) = slot.as_ref() {
        return Ok(db.clone());
    }
    let directory = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let db = Arc::new(IndexDb::open(&directory.join("search-index.sqlite3"))?);
    *slot = Some(db.clone());
    Ok(db)
}

#[tauri::command]
async fn index_folder(app: AppHandle, path: String) -> CmdResult<index::IndexStatus> {
    let root = platform::canonical(&resolve(&app, &path)?).map_err(|e| e.to_string())?;
    if !root.is_dir() {
        return Err("Choose an available folder".into());
    }
    let db = index_db(&app)?;
    let watchers = app.state::<IndexState>().watchers.clone();
    blocking(move || {
        let indexed = db.index_root(&root)?;
        db.set_watch(&platform::path_string(&root), true)?;
        let watch_error = watchers.start(db.clone(), root.clone()).err();
        Ok(index::IndexStatus {
            root: platform::path_string(&root),
            indexed,
            watching: watch_error.is_none(),
            watch_error,
        })
    })
    .await
}

#[tauri::command]
async fn index_status(app: AppHandle, path: String) -> CmdResult<index::IndexStatus> {
    let root = platform::canonical(&resolve(&app, &path)?).map_err(|e| e.to_string())?;
    let db = index_db(&app)?;
    let watchers = app.state::<IndexState>().watchers.clone();
    blocking(move || db.status(&root, &watchers)).await
}

#[tauri::command]
async fn list_index_roots(app: AppHandle) -> CmdResult<Vec<index::IndexRoot>> {
    let db = index_db(&app)?;
    let watchers = app.state::<IndexState>().watchers.clone();
    blocking(move || db.roots(&watchers)).await
}

#[tauri::command]
async fn remove_index_root(app: AppHandle, root: String) -> CmdResult<()> {
    let db = index_db(&app)?;
    let watchers = app.state::<IndexState>().watchers.clone();
    blocking(move || {
        watchers.stop(Path::new(&root));
        db.remove_root(&root)
    })
    .await
}

#[tauri::command]
async fn set_index_watch(app: AppHandle, root: String, watch: bool) -> CmdResult<()> {
    let db = index_db(&app)?;
    let watchers = app.state::<IndexState>().watchers.clone();
    blocking(move || {
        db.set_watch(&root, watch)?;
        if watch {
            watchers.start(db.clone(), PathBuf::from(&root))
        } else {
            watchers.stop(Path::new(&root));
            Ok(())
        }
    })
    .await
}

#[tauri::command]
async fn search_index(
    app: AppHandle,
    path: Option<String>,
    query: String,
    limit: usize,
) -> CmdResult<Vec<FileEntry>> {
    let within = match path {
        Some(path) => Some(platform::path_string(
            &platform::canonical(&resolve(&app, &path)?).map_err(|e| e.to_string())?,
        )),
        None => None,
    };
    let db = index_db(&app)?;
    blocking(move || db.search(within.as_deref(), &query, limit)).await
}

// ---------- App ----------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(DirectorySnapshots::default())
        .manage(IndexState {
            db: Mutex::new(None),
            watchers: Arc::new(IndexWatchers::default()),
        })
        .manage(rules::RuleWatcher::default())
        .manage(TransferTasks::default())
        .manage(TrashFailures::default())
        .setup(|app| {
            let handle = app.handle().clone();
            // Restoring watchers can take a while on large trees, so it never blocks startup.
            std::thread::spawn(move || {
                if let Ok(saved) = rules::read_rules(&handle)
                    && let Err(error) =
                        rules::reconcile(&handle, &handle.state::<rules::RuleWatcher>(), &saved)
                {
                    eprintln!("TidyBeam couldn't restore automatic rules: {error}");
                }
                match index_db(&handle) {
                    Ok(db) => index::restore(db, handle.state::<IndexState>().watchers.clone()),
                    Err(error) => eprintln!("TidyBeam couldn't open the search index: {error}"),
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            platform_info,
            user_places,
            list_drives,
            path_info,
            list_entries,
            compare_folders,
            read_text_preview,
            open_item,
            reveal_item,
            create_folder,
            rename_path,
            undo_created_folder,
            trash_paths,
            delete_permanently,
            preview_transfer,
            start_transfer,
            pause_transfer,
            resume_transfer,
            cancel_transfer,
            list_rules,
            save_rule,
            preview_rule,
            apply_rule,
            delete_rule,
            analyze_folder,
            find_duplicates,
            scan_cleanup,
            list_archive,
            extract_archive,
            create_zip,
            index_folder,
            index_status,
            list_index_roots,
            remove_index_root,
            set_index_watch,
            search_index
        ])
        .run(tauri::generate_context!())
        .expect("error while running TidyBeam");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_refuses_to_overwrite() {
        let dir = std::env::temp_dir().join(format!("tidybeam-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), b"a").unwrap();
        fs::write(dir.join("b.txt"), b"b").unwrap();
        assert!(rename_path(dir.join("a.txt").to_string_lossy().into(), "b.txt".into()).is_err());
        assert_eq!(fs::read(dir.join("b.txt")).unwrap(), b"b");
        let renamed =
            rename_path(dir.join("a.txt").to_string_lossy().into(), "c.txt".into()).unwrap();
        assert!(renamed.ends_with("c.txt"));
        // Case-only renames work everywhere.
        let upper =
            rename_path(dir.join("c.txt").to_string_lossy().into(), "C.txt".into()).unwrap();
        assert!(upper.ends_with("C.txt"));
        assert_eq!(fs::read(&upper).unwrap(), b"a");
        let _ = fs::remove_dir_all(&dir);
    }
}
