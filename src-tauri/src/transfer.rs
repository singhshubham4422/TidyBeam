//! Copy and move with conflict handling, metadata preservation, pause/resume/cancel
//! and safe cross-device moves.

use crate::platform;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs::{self, File, FileTimes},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// Files up to this size use the OS copy routine (reflinks, CopyFileEx, fcopyfile).
const FAST_COPY_LIMIT: u64 = 16 * 1024 * 1024;
const CHUNK: usize = 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conflict {
    Skip,
    KeepBoth,
    Replace,
}

impl Conflict {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "skip" => Ok(Self::Skip),
            "keep-both" => Ok(Self::KeepBoth),
            "replace" => Ok(Self::Replace),
            _ => Err("Choose how to handle items that already exist".into()),
        }
    }
}

#[derive(Serialize, Default, Debug)]
pub struct TransferSummary {
    pub completed: usize,
    pub skipped: usize,
    pub skipped_links: usize,
}

#[derive(Serialize)]
pub struct TransferConflict {
    pub source: String,
    pub destination: String,
    pub name: String,
    pub is_dir: bool,
}

/// Receives progress and is asked whether to continue between chunks.
pub trait CopyObserver {
    fn checkpoint(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn bytes(&mut self, _count: u64) {}
    fn item_started(&mut self, _name: &str) {}
    fn item_finished(&mut self) {}
    fn skipped_link(&mut self) {}
}

#[derive(Default)]
pub struct QuietObserver {
    pub skipped_links: usize,
}

impl CopyObserver for QuietObserver {
    fn skipped_link(&mut self) {
        self.skipped_links += 1;
    }
}

#[derive(Default)]
pub struct TaskControl {
    pub paused: Mutex<bool>,
    pub wake: Condvar,
    pub cancelled: AtomicBool,
}

impl TaskControl {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Blocks while paused. Returns `Ok(true)` if the task had been paused.
    pub fn wait_if_paused(&self) -> Result<bool, String> {
        if self.is_cancelled() {
            return Err(CANCELLED.into());
        }
        let mut paused = self.paused.lock().map_err(|e| e.to_string())?;
        let was_paused = *paused;
        while *paused && !self.is_cancelled() {
            paused = self.wake.wait(paused).map_err(|e| e.to_string())?;
        }
        if self.is_cancelled() {
            return Err(CANCELLED.into());
        }
        Ok(was_paused)
    }
}

pub const CANCELLED: &str = "Transfer cancelled";

#[derive(Default)]
pub struct TransferTasks {
    pub controls: Arc<Mutex<HashMap<String, Arc<TaskControl>>>>,
}

pub fn preview(sources: &[String], destination: &Path) -> Result<Vec<TransferConflict>, String> {
    if !destination.is_dir() {
        return Err("Destination folder is unavailable".into());
    }
    Ok(sources
        .iter()
        .filter_map(|raw| {
            let source = PathBuf::from(raw);
            fs::symlink_metadata(&source).ok()?;
            let target = destination.join(source.file_name()?);
            if platform::same_existing_path(&target, &source)
                || fs::symlink_metadata(&target).is_err()
            {
                return None;
            }
            Some(TransferConflict {
                source: platform::path_string(&source),
                destination: platform::path_string(&target),
                name: target.file_name()?.to_string_lossy().into_owned(),
                is_dir: source.is_dir(),
            })
        })
        .collect())
}

pub fn keep_both_path(target: &Path, is_dir: bool) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let as_path = Path::new(&name);
    let stem = as_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let extension = as_path
        .extension()
        .map(|e| e.to_string_lossy().into_owned());
    for index in 1u32.. {
        let suffix = if index == 1 {
            " (copy)".to_string()
        } else {
            format!(" (copy {index})")
        };
        let renamed = match (&extension, is_dir) {
            (Some(ext), false) if !stem.is_empty() => format!("{stem}{suffix}.{ext}"),
            _ => format!("{name}{suffix}"),
        };
        let candidate = parent.join(renamed);
        if fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
    }
    unreachable!("an unused name always exists")
}

fn cleanup_partial(target: &Path) {
    if let Ok(metadata) = fs::symlink_metadata(target) {
        let _ = if metadata.is_dir() {
            fs::remove_dir_all(target)
        } else {
            fs::remove_file(target)
        };
    }
}

fn file_times(source: &fs::Metadata) -> FileTimes {
    let mut times = FileTimes::new();
    if let Ok(modified) = source.modified() {
        times = times.set_modified(modified);
    }
    if let Ok(accessed) = source.accessed() {
        times = times.set_accessed(accessed);
    }
    times
}

fn copy_times(source: &fs::Metadata, destination: &Path) {
    // Best effort: folders can't be opened for writing on Windows, and the OS copy
    // routine already keeps times on most platforms.
    if let Ok(file) = File::options().write(true).open(destination) {
        let _ = file.set_times(file_times(source));
    }
}

fn copy_file(
    source: &Path,
    destination: &Path,
    metadata: &fs::Metadata,
    observer: &mut dyn CopyObserver,
) -> Result<(), String> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(format!("{} already exists", destination.display()));
    }
    observer.checkpoint()?;
    let result = (|| -> Result<(), String> {
        if metadata.len() > FAST_COPY_LIMIT {
            let mut input = File::open(source).map_err(|e| format!("{}: {e}", source.display()))?;
            let mut output = File::options()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|e| format!("{}: {e}", destination.display()))?;
            let mut buffer = vec![0u8; CHUNK];
            loop {
                observer.checkpoint()?;
                let read = input.read(&mut buffer).map_err(|e| e.to_string())?;
                if read == 0 {
                    break;
                }
                output
                    .write_all(&buffer[..read])
                    .map_err(|e| e.to_string())?;
                observer.bytes(read as u64);
            }
            output.flush().map_err(|e| e.to_string())?;
            let _ = output.set_times(file_times(metadata));
            drop(output);
            fs::set_permissions(destination, metadata.permissions()).map_err(|e| e.to_string())?;
        } else {
            fs::copy(source, destination).map_err(|e| format!("{}: {e}", source.display()))?;
            observer.bytes(metadata.len());
            copy_times(metadata, destination);
        }
        Ok(())
    })();
    if result.is_err() {
        cleanup_partial(destination);
    }
    result
}

/// Copies a file or folder tree. Symbolic links are skipped and reported to the observer.
pub fn copy_tree(
    source: &Path,
    destination: &Path,
    observer: &mut dyn CopyObserver,
) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(source).map_err(|e| format!("{}: {e}", source.display()))?;
    if metadata.file_type().is_symlink() {
        observer.skipped_link();
        return Ok(());
    }
    if !metadata.is_dir() {
        return copy_file(source, destination, &metadata, observer);
    }
    if fs::symlink_metadata(destination).is_ok() {
        return Err(format!("{} already exists", destination.display()));
    }
    // Iterative walk: deeply nested trees can't overflow the stack.
    let result = (|| -> Result<(), String> {
        let mut folders = Vec::<(PathBuf, fs::Metadata)>::new();
        for item in walkdir::WalkDir::new(source).follow_links(false) {
            let item = item.map_err(|e| e.to_string())?;
            let relative = item
                .path()
                .strip_prefix(source)
                .map_err(|e| e.to_string())?;
            let target = destination.join(relative);
            let file_type = item.file_type();
            if file_type.is_symlink() {
                observer.skipped_link();
                continue;
            }
            let item_meta = item.metadata().map_err(|e| e.to_string())?;
            if file_type.is_dir() {
                fs::create_dir(&target).map_err(|e| format!("{}: {e}", target.display()))?;
                folders.push((target, item_meta));
            } else if file_type.is_file() {
                copy_file(item.path(), &target, &item_meta, observer)?;
            }
        }
        // Restore folder permissions and times last, deepest first, so read-only
        // folders don't block copying their contents.
        for (folder, meta) in folders.iter().rev() {
            copy_times(meta, folder);
            let _ = fs::set_permissions(folder, meta.permissions());
        }
        Ok(())
    })();
    if result.is_err() {
        cleanup_partial(destination);
    }
    result
}

pub fn path_size(path: &Path) -> u64 {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => meta.len(),
        Ok(meta) if meta.is_dir() => walkdir::WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter_map(|entry| entry.metadata().ok())
            .map(|meta| meta.len())
            .fold(0u64, u64::saturating_add),
        _ => 0,
    }
}

/// Works out the final target for one source, handling conflicts. `None` means skip.
fn plan_target(
    source: &Path,
    destination: &Path,
    canonical_destination: &Path,
    conflict: Conflict,
    moving: bool,
) -> Result<Option<PathBuf>, String> {
    let Ok(source_meta) = fs::symlink_metadata(source) else {
        return Ok(None);
    };
    let canonical_source = platform::canonical(source).unwrap_or_else(|_| source.to_path_buf());
    if source_meta.is_dir()
        && platform::same_or_descendant(canonical_destination, &canonical_source)
    {
        return Err(if moving {
            "A folder can't be moved into itself"
        } else {
            "A folder can't be copied into itself"
        }
        .into());
    }
    let name = source
        .file_name()
        .ok_or("This location can't be transferred")?;
    let mut target = destination.join(name);
    if platform::same_existing_path(&target, source) {
        return Ok(None);
    }
    if fs::symlink_metadata(&target).is_ok() {
        match conflict {
            Conflict::Skip => return Ok(None),
            Conflict::KeepBoth => target = keep_both_path(&target, source_meta.is_dir()),
            Conflict::Replace => {
                let canonical_target =
                    platform::canonical(&target).unwrap_or_else(|_| target.clone());
                if platform::same_or_descendant(&canonical_source, &canonical_target) {
                    return Err(format!(
                        "“{}” can't replace a folder that contains it",
                        name.to_string_lossy()
                    ));
                }
                platform::trash_one(&target).map_err(|e| {
                    format!(
                        "Couldn't move the existing item to the {}: {e}",
                        platform::trash_name()
                    )
                })?;
            }
        }
    }
    Ok(Some(target))
}

fn move_item(
    source: &Path,
    target: &Path,
    observer: &mut CountingObserver<'_>,
) -> Result<(), String> {
    match fs::rename(source, target) {
        Ok(()) => {
            observer.inner.bytes(path_size(target));
            Ok(())
        }
        Err(error) if platform::is_cross_device(&error) => {
            let links_before = observer.skipped_links;
            copy_tree(source, target, observer)?;
            if observer.skipped_links > links_before {
                // Deleting the original would lose the links that weren't copied.
                return Err(format!(
                    "Copied “{}”, but kept the original because it contains symbolic links that can't be moved across drives",
                    source.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
            let removal = if source.is_dir() {
                fs::remove_dir_all(source)
            } else {
                fs::remove_file(source)
            };
            removal.map_err(|e| {
                format!(
                    "The copy of “{}” is complete, but the original couldn't be removed ({e}). Remove it manually.",
                    source.file_name().unwrap_or_default().to_string_lossy()
                )
            })
        }
        Err(error) => Err(format!("Couldn't move “{}”: {error}", source.display())),
    }
}

struct CountingObserver<'a> {
    inner: &'a mut dyn CopyObserver,
    skipped_links: usize,
}

impl CopyObserver for CountingObserver<'_> {
    fn checkpoint(&mut self) -> Result<(), String> {
        self.inner.checkpoint()
    }
    fn bytes(&mut self, count: u64) {
        self.inner.bytes(count)
    }
    fn skipped_link(&mut self) {
        self.skipped_links += 1;
        self.inner.skipped_link();
    }
}

pub fn run(
    sources: &[String],
    destination: &Path,
    moving: bool,
    conflict: Conflict,
    observer: &mut dyn CopyObserver,
) -> Result<TransferSummary, String> {
    if !destination.is_dir() {
        return Err("Destination folder is unavailable".into());
    }
    let canonical_destination = platform::canonical(destination).map_err(|e| e.to_string())?;
    let mut summary = TransferSummary::default();
    for raw in sources {
        observer.checkpoint()?;
        let source = PathBuf::from(raw);
        observer.item_started(&source.file_name().unwrap_or_default().to_string_lossy());
        let Some(target) = plan_target(
            &source,
            destination,
            &canonical_destination,
            conflict,
            moving,
        )?
        else {
            summary.skipped += 1;
            observer.item_finished();
            continue;
        };
        let mut counting = CountingObserver {
            inner: &mut *observer,
            skipped_links: 0,
        };
        if moving {
            move_item(&source, &target, &mut counting)?;
        } else {
            copy_tree(&source, &target, &mut counting)?;
        }
        summary.skipped_links += counting.skipped_links;
        summary.completed += 1;
        observer.item_finished();
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tidybeam-transfer-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn same_folder_is_not_a_conflict() {
        let dir = temp("same");
        let file = dir.join("keep.txt");
        fs::write(&file, b"keep").unwrap();
        assert!(
            preview(&[file.to_string_lossy().into()], &dir)
                .unwrap()
                .is_empty()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_keeps_both_and_preserves_contents() {
        let dir = temp("copy");
        let src = dir.join("src");
        let dst = dir.join("dst");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::write(src.join("nested").join("a.txt"), b"hello").unwrap();
        fs::write(dst.join("a.txt"), b"existing").unwrap();
        let sources = vec![
            src.join("nested")
                .join("a.txt")
                .to_string_lossy()
                .into_owned(),
            src.to_string_lossy().into_owned(),
        ];
        let summary = run(
            &sources,
            &dst,
            false,
            Conflict::KeepBoth,
            &mut QuietObserver::default(),
        )
        .unwrap();
        assert_eq!(summary.completed, 2);
        assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"existing");
        assert_eq!(fs::read(dst.join("a (copy).txt")).unwrap(), b"hello");
        assert_eq!(
            fs::read(dst.join("src").join("nested").join("a.txt")).unwrap(),
            b"hello"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn folder_cannot_move_into_itself() {
        let dir = temp("self");
        let inner = dir.join("a").join("b");
        fs::create_dir_all(&inner).unwrap();
        let result = run(
            &[dir.join("a").to_string_lossy().into_owned()],
            &inner,
            true,
            Conflict::Skip,
            &mut QuietObserver::default(),
        );
        assert!(result.is_err());
        assert!(inner.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_refuses_to_trash_a_folder_containing_the_source() {
        let dir = temp("replace");
        let outer = dir.join("foo");
        fs::create_dir_all(&outer).unwrap();
        let source = outer.join("foo");
        fs::write(&source, b"precious").unwrap();
        let result = run(
            &[source.to_string_lossy().into_owned()],
            &dir,
            true,
            Conflict::Replace,
            &mut QuietObserver::default(),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&source).unwrap(), b"precious");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_within_a_drive_renames() {
        let dir = temp("move");
        fs::create_dir_all(dir.join("dst")).unwrap();
        fs::write(dir.join("file.txt"), b"x").unwrap();
        run(
            &[dir.join("file.txt").to_string_lossy().into_owned()],
            &dir.join("dst"),
            true,
            Conflict::Skip,
            &mut QuietObserver::default(),
        )
        .unwrap();
        assert!(!dir.join("file.txt").exists());
        assert_eq!(fs::read(dir.join("dst").join("file.txt")).unwrap(), b"x");
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn copies_deeply_nested_trees_without_recursion() {
        let dir = std::env::temp_dir().join(format!("tidybeam-deep-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut nested = dir.join("src");
        for _ in 0..120 {
            nested = nested.join("d");
        }
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("leaf.txt"), b"leaf").unwrap();
        let dest = dir.join("dst");
        fs::create_dir_all(&dest).unwrap();
        run(
            &[dir.join("src").to_string_lossy().into_owned()],
            &dest,
            false,
            Conflict::Skip,
            &mut QuietObserver::default(),
        )
        .unwrap();
        let mut copied = dest.join("src");
        for _ in 0..120 {
            copied = copied.join("d");
        }
        assert_eq!(fs::read(copied.join("leaf.txt")).unwrap(), b"leaf");
        let _ = fs::remove_dir_all(&dir);
    }
}
