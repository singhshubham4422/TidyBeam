//! Disk usage analysis, duplicate detection and temporary-file cleanup review.

use crate::platform::{self, FileEntry};
use jwalk::{Parallelism, WalkDir as ParallelWalkDir};
use rayon::prelude::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;
use xxhash_rust::xxh3::xxh3_64;

#[derive(Serialize)]
pub struct FolderSize {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub files: u64,
    pub is_dir: bool,
}

#[derive(Serialize)]
pub struct Analysis {
    pub path: String,
    pub total_bytes: u64,
    pub file_count: u64,
    pub folder_count: u64,
    pub largest: Vec<FileEntry>,
    pub folders: Vec<FolderSize>,
}

pub fn analyze(root: &Path, mut progress: impl FnMut(u64, u64)) -> Result<Analysis, String> {
    if !root.is_dir() {
        return Err("Choose an available folder".into());
    }
    let workers = std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 8))
        .unwrap_or(2);
    let (mut total_bytes, mut file_count, mut folder_count) = (0u64, 0u64, 0u64);
    let mut largest: Vec<FileEntry> = Vec::new();
    let mut children = HashMap::<PathBuf, (u64, u64)>::new();
    let mut last = Instant::now();
    for item in ParallelWalkDir::new(root)
        .follow_links(false)
        .skip_hidden(false)
        .parallelism(Parallelism::RayonNewPool(workers))
    {
        let Ok(item) = item else { continue };
        let file_type = item.file_type();
        let path = item.path();
        let first = path
            .strip_prefix(root)
            .ok()
            .and_then(|rel| rel.components().next())
            .map(|c| PathBuf::from(c.as_os_str()));
        if file_type.is_dir() {
            folder_count += 1;
            if let Some(first) = first {
                children.entry(first).or_insert((0, 0));
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Ok(metadata) = item.metadata() else {
            continue;
        };
        let size = metadata.len();
        total_bytes = total_bytes.saturating_add(size);
        file_count += 1;
        let smallest = largest
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.size)
            .map(|(i, e)| (i, e.size));
        if largest.len() < 10 {
            largest.push(platform::entry_from_metadata(&path, &metadata));
        } else if let Some((index, smallest_size)) = smallest
            && size > smallest_size
        {
            largest[index] = platform::entry_from_metadata(&path, &metadata);
        }
        if let Some(first) = first {
            let stat = children.entry(first).or_insert((0, 0));
            stat.0 = stat.0.saturating_add(size);
            stat.1 += 1;
        }
        if last.elapsed() >= Duration::from_millis(250) {
            progress(file_count, folder_count.saturating_sub(1));
            last = Instant::now();
        }
    }
    largest.sort_by_key(|e| std::cmp::Reverse(e.size));
    let mut folders = children
        .into_iter()
        .map(|(name, (bytes, files))| {
            let child = root.join(&name);
            FolderSize {
                name: name.to_string_lossy().into_owned(),
                path: platform::path_string(&child),
                is_dir: child.is_dir(),
                bytes,
                files,
            }
        })
        .collect::<Vec<_>>();
    folders.sort_by_key(|f| std::cmp::Reverse(f.bytes));
    let folder_count = folder_count.saturating_sub(1);
    progress(file_count, folder_count);
    Ok(Analysis {
        path: platform::path_string(root),
        total_bytes,
        file_count,
        folder_count,
        largest,
        folders,
    })
}

#[derive(Serialize)]
pub struct DuplicateGroup {
    pub hash: String,
    pub size: u64,
    pub files: Vec<FileEntry>,
}

const SAMPLE: usize = 16 * 1024;

/// Cheap fingerprint: the first and last 16 KiB.
fn sample_hash(path: &Path, size: u64) -> Option<u64> {
    let mut file = fs::File::open(path).ok()?;
    let mut buffer = vec![0u8; SAMPLE * 2];
    let head = file.read(&mut buffer[..SAMPLE]).ok()?;
    let mut used = head;
    if size > (SAMPLE * 2) as u64 {
        file.seek(SeekFrom::End(-(SAMPLE as i64))).ok()?;
        let mut tail = 0;
        while tail < SAMPLE {
            let read = file.read(&mut buffer[SAMPLE + tail..]).ok()?;
            if read == 0 {
                break;
            }
            tail += read;
        }
        buffer.copy_within(SAMPLE..SAMPLE + tail, head);
        used = head + tail;
    }
    Some(xxh3_64(&buffer[..used]))
}

fn full_hash(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let before = file.metadata().ok()?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; 256 * 1024];
    loop {
        let read = file.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    let after = file.metadata().ok()?;
    // Skip files that changed while being read.
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return None;
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

pub fn duplicates(root: &Path) -> Result<Vec<DuplicateGroup>, String> {
    if !root.is_dir() {
        return Err("Choose an available folder".into());
    }
    let mut by_size = HashMap::<u64, Vec<PathBuf>>::new();
    for item in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !item.file_type().is_file() {
            continue;
        }
        if let Ok(metadata) = item.metadata()
            && metadata.len() > 0
        {
            by_size
                .entry(metadata.len())
                .or_default()
                .push(item.into_path());
        }
    }
    let candidates = by_size
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .collect::<Vec<_>>();
    let mut groups = candidates
        .into_par_iter()
        .flat_map_iter(|(size, files)| {
            let mut samples = HashMap::<u64, Vec<PathBuf>>::new();
            for (hash, file) in files
                .into_par_iter()
                .filter_map(|f| sample_hash(&f, size).map(|h| (h, f)))
                .collect::<Vec<_>>()
            {
                samples.entry(hash).or_default().push(file);
            }
            let mut out = Vec::new();
            for files in samples.into_values().filter(|f| f.len() > 1) {
                let mut full = HashMap::<String, Vec<PathBuf>>::new();
                for (hash, file) in files
                    .into_par_iter()
                    .filter_map(|f| full_hash(&f).map(|h| (h, f)))
                    .collect::<Vec<_>>()
                {
                    full.entry(hash).or_default().push(file);
                }
                for (hash, files) in full {
                    let entries = files
                        .iter()
                        .filter_map(|f| platform::entry_from_path(f).ok())
                        .collect::<Vec<_>>();
                    if entries.len() > 1 {
                        out.push(DuplicateGroup {
                            hash,
                            size,
                            files: entries,
                        });
                    }
                }
            }
            out
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|g| std::cmp::Reverse(g.size.saturating_mul(g.files.len() as u64 - 1)));
    Ok(groups)
}

#[derive(Serialize)]
pub struct CleanupCandidate {
    pub name: String,
    pub path: String,
    pub category: String,
    pub bytes: u64,
    pub item_count: u64,
    pub modified: u64,
}

/// Returns true when the item belongs to the current user. On shared temp folders
/// such as Linux `/tmp`, other users' files must never be offered for cleanup.
fn owned_by_current_user(metadata: &fs::Metadata, home: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match fs::metadata(home) {
            Ok(home_meta) => home_meta.uid() == metadata.uid(),
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (metadata, home);
        true
    }
}

pub fn cleanup_candidates(home: &Path) -> Vec<CleanupCandidate> {
    let mut roots = vec![("Temporary files".to_string(), std::env::temp_dir())];
    if cfg!(target_os = "linux") {
        roots.push((
            "Thumbnail cache".into(),
            home.join(".cache").join("thumbnails"),
        ));
    }
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(7 * 24 * 60 * 60))
        .unwrap_or(UNIX_EPOCH);
    let mut candidates = Vec::new();
    for (category, root) in roots {
        let Ok(items) = fs::read_dir(&root) else {
            continue;
        };
        for item in items.filter_map(Result::ok) {
            let path = item.path();
            let name = item.file_name().to_string_lossy().into_owned();
            // macOS keeps live system state here.
            if cfg!(target_os = "macos")
                && (name.starts_with("com.apple") || name == "TemporaryItems")
            {
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() || !owned_by_current_user(&metadata, home) {
                continue;
            }
            let (mut bytes, mut count) = (0u64, 0u64);
            let mut newest = metadata.modified().unwrap_or(UNIX_EPOCH);
            if metadata.is_file() {
                bytes = metadata.len();
                count = 1;
            } else if metadata.is_dir() {
                for child in WalkDir::new(&path)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(Result::ok)
                {
                    if child.file_type().is_file()
                        && let Ok(meta) = child.metadata()
                    {
                        bytes = bytes.saturating_add(meta.len());
                        count += 1;
                        if let Ok(time) = meta.modified() {
                            newest = newest.max(time);
                        }
                    }
                }
            } else {
                continue;
            }
            if count == 0 || newest > cutoff {
                continue;
            }
            candidates.push(CleanupCandidate {
                name,
                path: platform::path_string(&path),
                category: category.clone(),
                bytes,
                item_count: count,
                modified: newest
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            });
        }
    }
    candidates.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates_use_full_content() {
        let dir = std::env::temp_dir().join(format!("tidybeam-dupes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("first.bin"), b"identical content").unwrap();
        fs::write(dir.join("second.bin"), b"identical content").unwrap();
        fs::write(dir.join("different.bin"), b"different content").unwrap();
        let groups = duplicates(&dir).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].files.len(), 2);
        assert_eq!(groups[0].hash.len(), 64);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn analysis_counts_files() {
        let dir = std::env::temp_dir().join(format!("tidybeam-analyze-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("a.bin"), vec![0u8; 100]).unwrap();
        fs::write(dir.join("b.bin"), vec![0u8; 50]).unwrap();
        let analysis = analyze(&dir, |_, _| {}).unwrap();
        assert_eq!(analysis.file_count, 2);
        assert_eq!(analysis.total_bytes, 150);
        assert_eq!(analysis.folders[0].name, "sub");
        let _ = fs::remove_dir_all(&dir);
    }
}
