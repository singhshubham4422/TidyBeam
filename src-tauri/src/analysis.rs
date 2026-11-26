//! High-speed disk usage analysis (u32 arena tree + drive-aware rayon + OS bulk-stat),
//! 3-tier duplicate detection (4 KiB -> 32 KiB -> SIMD blake3 + SQLite cache),
//! and parallel temporary-file cleanup review.

use crate::platform::{self, FileEntry};
use jwalk::{Parallelism, WalkDir as ParallelWalkDir};
use rayon::prelude::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
#[cfg(unix)]
use std::collections::HashSet;
use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{DiskKind, Disks};
use walkdir::WalkDir;
use xxhash_rust::xxh3::xxh3_64;

const MAX_CACHED_TREES: usize = 4;
const CACHE_TTL: Duration = Duration::from_secs(600);
const LARGE_FILE_CACHE_THRESHOLD: u64 = 10 * 1024 * 1024;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CategoryBreakdown {
    pub media: u64,
    pub archives: u64,
    pub code: u64,
    pub documents: u64,
    pub app_data: u64,
    pub other: u64,
}

impl CategoryBreakdown {
    fn add(&mut self, ext: &str, bytes: u64) {
        match ext {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif" | "ico" | "heic"
            | "raw" | "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "mp3" | "wav" | "flac"
            | "aac" | "ogg" | "m4a" | "opus" => self.media = self.media.saturating_add(bytes),
            "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" | "iso" | "dmg" => {
                self.archives = self.archives.saturating_add(bytes)
            }
            "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "kt" | "c" | "cpp"
            | "h" | "hpp" | "cs" | "swift" | "rb" | "php" | "html" | "css" | "scss" | "json"
            | "yaml" | "yml" | "toml" | "xml" | "sql" | "sh" | "ps1" | "lock" | "wasm" => {
                self.code = self.code.saturating_add(bytes)
            }
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "csv"
            | "rtf" | "odt" | "pages" | "numbers" => {
                self.documents = self.documents.saturating_add(bytes)
            }
            "db" | "sqlite" | "sqlite3" | "log" | "cache" | "tmp" | "bak" | "dat" | "bin"
            | "dll" | "so" | "dylib" | "exe" | "msi" | "app" => {
                self.app_data = self.app_data.saturating_add(bytes)
            }
            _ => self.other = self.other.saturating_add(bytes),
        }
    }

    fn merge(&mut self, other: &CategoryBreakdown) {
        self.media = self.media.saturating_add(other.media);
        self.archives = self.archives.saturating_add(other.archives);
        self.code = self.code.saturating_add(other.code);
        self.documents = self.documents.saturating_add(other.documents);
        self.app_data = self.app_data.saturating_add(other.app_data);
        self.other = self.other.saturating_add(other.other);
    }
}

#[derive(Serialize, Clone)]
pub struct FolderSize {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub allocated_bytes: u64,
    pub files: u64,
    pub is_dir: bool,
}

#[derive(Serialize, Clone)]
pub struct Analysis {
    pub path: String,
    pub total_bytes: u64,
    pub allocated_bytes: u64,
    pub file_count: u64,
    pub folder_count: u64,
    pub scanned_at: u64,
    pub categories: CategoryBreakdown,
    pub largest: Vec<FileEntry>,
    pub folders: Vec<FolderSize>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisProgress {
    pub scan_id: String,
    pub files_scanned: u64,
    pub folders_scanned: u64,
    pub bytes_scanned: u64,
    pub top_folders: Vec<FolderSize>,
}

#[derive(Default)]
pub struct AnalysisTasks {
    pub controls: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl AnalysisTasks {
    pub fn register(&self, scan_id: &str) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut map) = self.controls.lock() {
            map.insert(scan_id.to_string(), cancel.clone());
        }
        cancel
    }

    pub fn cancel(&self, scan_id: &str) {
        if let Ok(map) = self.controls.lock()
            && let Some(flag) = map.get(scan_id)
        {
            flag.store(true, Ordering::Release);
        }
    }

    pub fn finish(&self, scan_id: &str) {
        if let Ok(mut map) = self.controls.lock() {
            map.remove(scan_id);
        }
    }
}

/// Flat u32-indexed arena node for sub-millisecond drill-downs and low RAM overhead.
struct DirNode {
    name: OsString,
    path: PathBuf,
    total_bytes: u64,
    total_allocated: u64,
    file_count: u64,
    folder_count: u64,
    children: Vec<u32>,
    direct_files: Vec<FolderSize>,
    top_files: Vec<FileEntry>,
    categories: CategoryBreakdown,
}

struct CachedTree {
    nodes: Vec<DirNode>,
    path_to_id: HashMap<String, u32>,
    scanned_at: u64,
    created: Instant,
    last_access: Instant,
}

#[derive(Default, Clone)]
pub struct AnalysisSnapshots(Arc<Mutex<Vec<CachedTree>>>);

impl AnalysisSnapshots {
    pub fn get(&self, path: &Path) -> Option<Analysis> {
        let key = platform::normalized_key(&platform::tidy(path));
        let mut cache = self.0.lock().ok()?;
        let now = Instant::now();
        cache.retain(|tree| now.duration_since(tree.created) < CACHE_TTL);
        for tree in cache.iter_mut() {
            if let Some(&node_id) = tree.path_to_id.get(&key) {
                tree.last_access = now;
                return Some(tree_to_analysis(tree, node_id));
            }
        }
        None
    }

    fn insert(&self, tree: CachedTree) {
        let Ok(mut cache) = self.0.lock() else {
            return;
        };
        let root_key = tree
            .nodes
            .first()
            .map(|n| platform::normalized_key(&n.path))
            .unwrap_or_default();
        cache.retain(|t| {
            t.nodes
                .first()
                .map(|n| platform::normalized_key(&n.path))
                .unwrap_or_default()
                != root_key
        });
        while cache.len() >= MAX_CACHED_TREES {
            if let Some(pos) = cache
                .iter()
                .enumerate()
                .min_by_key(|(_, t)| t.last_access)
                .map(|(i, _)| i)
            {
                cache.remove(pos);
            } else {
                break;
            }
        }
        cache.push(tree);
    }

    pub fn invalidate(&self, path: &Path) {
        let key = platform::normalized_key(&platform::tidy(path));
        if let Ok(mut cache) = self.0.lock() {
            cache.retain(|t| !t.path_to_id.contains_key(&key));
        }
    }
}

fn tree_to_analysis(tree: &CachedTree, node_id: u32) -> Analysis {
    let node = &tree.nodes[node_id as usize];
    let mut folders: Vec<FolderSize> = node
        .children
        .iter()
        .map(|&cid| {
            let child = &tree.nodes[cid as usize];
            FolderSize {
                name: child.name.to_string_lossy().into_owned(),
                path: platform::path_string(&child.path),
                bytes: child.total_bytes,
                allocated_bytes: child.total_allocated,
                files: child.file_count,
                is_dir: true,
            }
        })
        .collect();
    folders.extend(node.direct_files.iter().cloned());
    folders.sort_by_key(|f| std::cmp::Reverse(f.bytes));
    Analysis {
        path: platform::path_string(&node.path),
        total_bytes: node.total_bytes,
        allocated_bytes: node.total_allocated,
        file_count: node.file_count,
        folder_count: node.folder_count,
        scanned_at: tree.scanned_at,
        categories: node.categories.clone(),
        largest: node.top_files.clone(),
        folders,
    }
}

/// Persistent SQLite cache for duplicate large-file blake3 hashes and folder summaries.
pub struct AnalysisCacheDb {
    conn: Mutex<Connection>,
}

impl AnalysisCacheDb {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA temp_store=MEMORY;
             PRAGMA busy_timeout=3000;
             CREATE TABLE IF NOT EXISTS hash_cache (
                 path TEXT PRIMARY KEY,
                 size INTEGER NOT NULL,
                 modified INTEGER NOT NULL,
                 blake3 TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS folder_summaries (
                 root TEXT NOT NULL,
                 path TEXT NOT NULL,
                 total_bytes INTEGER NOT NULL,
                 allocated_bytes INTEGER NOT NULL,
                 file_count INTEGER NOT NULL,
                 folder_count INTEGER NOT NULL,
                 scanned_at INTEGER NOT NULL,
                 PRIMARY KEY(root, path)
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn get_hash(&self, path: &str, size: u64, modified: u64) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        conn.query_row(
            "SELECT blake3 FROM hash_cache WHERE path = ?1 AND size = ?2 AND modified = ?3",
            params![path, size, modified],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn put_hash(&self, path: &str, size: u64, modified: u64, blake3: &str) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO hash_cache(path, size, modified, blake3) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(path) DO UPDATE SET size = excluded.size, modified = excluded.modified, blake3 = excluded.blake3",
                params![path, size, modified, blake3],
            );
        }
    }

    pub fn save_summary(&self, root: &str, analysis: &Analysis) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO folder_summaries(root, path, total_bytes, allocated_bytes, file_count, folder_count, scanned_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(root, path) DO UPDATE SET total_bytes=excluded.total_bytes, allocated_bytes=excluded.allocated_bytes,
                 file_count=excluded.file_count, folder_count=excluded.folder_count, scanned_at=excluded.scanned_at",
                params![
                    root,
                    analysis.path,
                    analysis.total_bytes,
                    analysis.allocated_bytes,
                    analysis.file_count,
                    analysis.folder_count,
                    analysis.scanned_at
                ],
            );
        }
    }
}

/// Chooses drive-aware worker count: 2 workers on HDD/removable to avoid head thrashing,
/// 4..=12 on NVMe/SSD.
pub fn drive_worker_count(root: &Path) -> usize {
    let disks = Disks::new_with_refreshed_list();
    let mut best_len = 0usize;
    let mut kind = DiskKind::Unknown(-1);
    let mut removable = false;
    for disk in disks.iter() {
        let mount = disk.mount_point();
        if platform::same_or_descendant(root, mount) {
            let len = mount.as_os_str().len();
            if len >= best_len {
                best_len = len;
                kind = disk.kind();
                removable = disk.is_removable();
            }
        }
    }
    if removable || matches!(kind, DiskKind::HDD) {
        2
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get().clamp(4, 12))
            .unwrap_or(4)
    }
}

struct FastDirEntry {
    name: OsString,
    path: PathBuf,
    is_dir: bool,
    is_symlink: bool,
    is_cloud_placeholder: bool,
    size: u64,
    allocated: u64,
    modified: u64,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    nlink: u64,
}

#[cfg(windows)]
fn read_dir_fast(dir: &Path) -> Vec<FastDirEntry> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Foundation::{FILETIME, INVALID_HANDLE_VALUE, MAX_PATH};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FIND_FIRST_EX_LARGE_FETCH,
        FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW,
        WIN32_FIND_DATAW,
    };
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x0004_0000;

    fn filetime_to_secs(ft: FILETIME) -> u64 {
        let ticks = ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64);
        // Windows epoch 1601-01-01 to Unix epoch 1970-01-01 is 11644473600 seconds.
        (ticks / 10_000_000).saturating_sub(11_644_473_600)
    }

    let mut pattern: Vec<u16> = dir.as_os_str().encode_wide().collect();
    if !pattern.ends_with(&[b'\\' as u16, b'/' as u16]) {
        pattern.push(b'\\' as u16);
    }
    pattern.push(b'*' as u16);
    pattern.push(0);

    let mut out = Vec::new();
    let mut data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    let handle = unsafe {
        FindFirstFileExW(
            pattern.as_ptr(),
            FindExInfoBasic,
            &mut data as *mut _ as *mut _,
            FindExSearchNameMatch,
            std::ptr::null(),
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return out;
    }
    loop {
        let len = data
            .cFileName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(MAX_PATH as usize);
        let name_slice = &data.cFileName[..len];
        // Skip "." and ".."
        if !(name_slice == [b'.' as u16] || name_slice == [b'.' as u16, b'.' as u16]) {
            let name = OsString::from_wide(name_slice);
            let attrs = data.dwFileAttributes;
            let is_dir = (attrs & FILE_ATTRIBUTE_DIRECTORY) != 0;
            let is_symlink = (attrs & FILE_ATTRIBUTE_REPARSE_POINT) != 0;
            let is_cloud = (attrs
                & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_RECALL_ON_OPEN))
                != 0;
            let size = ((data.nFileSizeHigh as u64) << 32) | (data.nFileSizeLow as u64);
            let allocated = if is_dir || is_cloud {
                0
            } else {
                size.div_ceil(4096) * 4096
            };
            let modified = filetime_to_secs(data.ftLastWriteTime);
            out.push(FastDirEntry {
                path: dir.join(&name),
                name,
                is_dir,
                is_symlink,
                is_cloud_placeholder: is_cloud,
                size,
                allocated,
                modified,
            });
        }
        let ok = unsafe { FindNextFileW(handle, &mut data) };
        if ok == 0 {
            break;
        }
    }
    unsafe { FindClose(handle) };
    out
}

#[cfg(unix)]
fn read_dir_fast(dir: &Path) -> Vec<FastDirEntry> {
    use std::os::unix::fs::MetadataExt;
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let ft = meta.file_type();
        let size = if ft.is_file() { meta.len() } else { 0 };
        let allocated = if ft.is_file() {
            meta.blocks().saturating_mul(512)
        } else {
            0
        };
        #[cfg(target_os = "macos")]
        let is_cloud = {
            use std::os::macos::fs::MetadataExt as MacMeta;
            const SF_DATALESS: u32 = 0x4000_0000;
            (meta.st_flags() & SF_DATALESS) != 0
        };
        #[cfg(not(target_os = "macos"))]
        let is_cloud = false;
        out.push(FastDirEntry {
            name: entry.file_name(),
            path,
            is_dir: ft.is_dir(),
            is_symlink: ft.is_symlink(),
            is_cloud_placeholder: is_cloud,
            size,
            allocated,
            modified: platform::modified_secs(&meta),
            dev: meta.dev(),
            ino: meta.ino(),
            nlink: meta.nlink(),
        });
    }
    out
}

fn push_top_file(top: &mut Vec<FileEntry>, entry: FileEntry) {
    if top.len() < 10 {
        top.push(entry);
        return;
    }
    if let Some((idx, smallest)) = top
        .iter()
        .enumerate()
        .min_by_key(|(_, e)| e.size)
        .map(|(i, e)| (i, e.size))
        && entry.size > smallest
    {
        top[idx] = entry;
    }
}

fn merge_top_files(dst: &mut Vec<FileEntry>, src: &[FileEntry]) {
    for item in src {
        push_top_file(dst, item.clone());
    }
}

#[derive(Default)]
struct SubtreeBuild {
    nodes: Vec<DirNode>,
    root_idx: usize,
}

fn scan_subtree_recursive(
    path: &Path,
    name: OsString,
    cancel: &AtomicBool,
    files_counter: &AtomicU64,
    folders_counter: &AtomicU64,
    bytes_counter: &AtomicU64,
    #[cfg(unix)] seen_inodes: &Mutex<HashSet<(u64, u64)>>,
) -> Result<SubtreeBuild, String> {
    if cancel.load(Ordering::Acquire) {
        return Err("Cancelled".into());
    }
    let entries = read_dir_fast(path);
    let mut subdirs = Vec::new();
    let mut self_bytes = 0u64;
    let mut self_allocated = 0u64;
    let mut file_count = 0u64;
    let mut direct_files = Vec::new();
    let mut top_files = Vec::new();
    let mut categories = CategoryBreakdown::default();

    for entry in entries {
        if entry.is_symlink || entry.is_cloud_placeholder {
            continue;
        }
        if entry.is_dir {
            folders_counter.fetch_add(1, Ordering::Relaxed);
            subdirs.push((entry.name, entry.path));
            continue;
        }
        #[cfg(unix)]
        if entry.nlink > 1
            && let Ok(mut seen) = seen_inodes.lock()
            && !seen.insert((entry.dev, entry.ino))
        {
            continue;
        }
        let ext = Path::new(&entry.name)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        self_bytes = self_bytes.saturating_add(entry.size);
        self_allocated = self_allocated.saturating_add(entry.allocated);
        file_count += 1;
        categories.add(&ext, entry.size);
        let name_str = entry.name.to_string_lossy().into_owned();
        let path_str = platform::path_string(&entry.path);
        direct_files.push(FolderSize {
            name: name_str.clone(),
            path: path_str.clone(),
            bytes: entry.size,
            allocated_bytes: entry.allocated,
            files: 1,
            is_dir: false,
        });
        push_top_file(
            &mut top_files,
            FileEntry {
                name: name_str,
                path: path_str,
                is_dir: false,
                is_symlink: false,
                size: entry.size,
                modified: entry.modified,
                extension: ext,
                hidden: false,
                executable: false,
            },
        );
    }

    files_counter.fetch_add(file_count, Ordering::Relaxed);
    bytes_counter.fetch_add(self_bytes, Ordering::Relaxed);

    let child_builds = if subdirs.len() > 1 {
        subdirs
            .into_par_iter()
            .map(|(cname, cpath)| {
                scan_subtree_recursive(
                    &cpath,
                    cname,
                    cancel,
                    files_counter,
                    folders_counter,
                    bytes_counter,
                    #[cfg(unix)]
                    seen_inodes,
                )
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        subdirs
            .into_iter()
            .map(|(cname, cpath)| {
                scan_subtree_recursive(
                    &cpath,
                    cname,
                    cancel,
                    files_counter,
                    folders_counter,
                    bytes_counter,
                    #[cfg(unix)]
                    seen_inodes,
                )
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    let mut total_bytes = self_bytes;
    let mut total_allocated = self_allocated;
    let mut total_files = file_count;
    let mut total_folders = child_builds.len() as u64;
    let mut all_nodes = Vec::new();
    let mut child_indices = Vec::with_capacity(child_builds.len());

    for mut cb in child_builds {
        let child_root = &cb.nodes[cb.root_idx];
        total_bytes = total_bytes.saturating_add(child_root.total_bytes);
        total_allocated = total_allocated.saturating_add(child_root.total_allocated);
        total_files = total_files.saturating_add(child_root.file_count);
        total_folders = total_folders.saturating_add(child_root.folder_count);
        categories.merge(&child_root.categories);
        merge_top_files(&mut top_files, &child_root.top_files);

        let offset = all_nodes.len() as u32;
        for n in &mut cb.nodes {
            for cid in &mut n.children {
                *cid += offset;
            }
        }
        let child_root_global = offset + cb.root_idx as u32;
        child_indices.push(child_root_global);
        all_nodes.extend(cb.nodes);
    }

    top_files.sort_by_key(|e| std::cmp::Reverse(e.size));
    direct_files.sort_by_key(|f| std::cmp::Reverse(f.bytes));
    direct_files.truncate(50);

    let root_idx = all_nodes.len();
    all_nodes.push(DirNode {
        name,
        path: path.to_path_buf(),
        total_bytes,
        total_allocated,
        file_count: total_files,
        folder_count: total_folders,
        children: child_indices,
        direct_files,
        top_files,
        categories,
    });
    Ok(SubtreeBuild {
        nodes: all_nodes,
        root_idx,
    })
}

pub fn analyze_cancellable(
    root: &Path,
    scan_id: &str,
    cancel: &AtomicBool,
    snapshots: Option<&AnalysisSnapshots>,
    cache_db: Option<&AnalysisCacheDb>,
    mut progress: impl FnMut(AnalysisProgress) + Send,
) -> Result<Analysis, String> {
    if !root.is_dir() {
        return Err("Choose an available folder".into());
    }
    let workers = drive_worker_count(root);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .start_handler(|_| platform::lower_thread_priority())
        .build()
        .map_err(|e| e.to_string())?;

    let files_counter = AtomicU64::new(0);
    let folders_counter = AtomicU64::new(0);
    let bytes_counter = AtomicU64::new(0);
    #[cfg(unix)]
    let seen_inodes = Mutex::new(HashSet::<(u64, u64)>::new());

    let done = AtomicBool::new(false);
    let root_name = root
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| OsString::from(platform::path_string(root)));

    let build_res = std::thread::scope(|s| {
        let sid = scan_id.to_string();
        let fc = &files_counter;
        let dc = &folders_counter;
        let bc = &bytes_counter;
        let done_flag = &done;
        let cancel_flag = cancel;
        s.spawn(move || {
            while !done_flag.load(Ordering::Acquire) && !cancel_flag.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(250));
                if done_flag.load(Ordering::Acquire) || cancel_flag.load(Ordering::Acquire) {
                    break;
                }
                progress(AnalysisProgress {
                    scan_id: sid.clone(),
                    files_scanned: fc.load(Ordering::Relaxed),
                    folders_scanned: dc.load(Ordering::Relaxed),
                    bytes_scanned: bc.load(Ordering::Relaxed),
                    top_folders: Vec::new(),
                });
            }
        });

        let res = pool.install(|| {
            scan_subtree_recursive(
                root,
                root_name,
                cancel,
                &files_counter,
                &folders_counter,
                &bytes_counter,
                #[cfg(unix)]
                &seen_inodes,
            )
        });
        done.store(true, Ordering::Release);
        res
    })?;

    // Reorder so root is at index 0 for clean lookup.
    let mut nodes = build_res.nodes;
    let last_idx = build_res.root_idx;
    if last_idx != 0 {
        nodes.swap(0, last_idx);
        for node in &mut nodes {
            for cid in &mut node.children {
                if *cid == 0 {
                    *cid = last_idx as u32;
                } else if *cid == last_idx as u32 {
                    *cid = 0;
                }
            }
        }
    }

    let mut path_to_id = HashMap::with_capacity(nodes.len());
    for (idx, node) in nodes.iter().enumerate() {
        path_to_id.insert(platform::normalized_key(&node.path), idx as u32);
    }

    let scanned_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let tree = CachedTree {
        nodes,
        path_to_id,
        scanned_at,
        created: Instant::now(),
        last_access: Instant::now(),
    };
    let analysis = tree_to_analysis(&tree, 0);
    if let Some(snaps) = snapshots {
        snaps.insert(tree);
    }
    if let Some(db) = cache_db {
        db.save_summary(&analysis.path, &analysis);
    }
    Ok(analysis)
}

#[allow(dead_code)]
pub fn analyze(root: &Path, mut progress: impl FnMut(u64, u64) + Send) -> Result<Analysis, String> {
    let cancel = AtomicBool::new(false);
    analyze_cancellable(root, "sync", &cancel, None, None, |p| {
        progress(p.files_scanned, p.folders_scanned);
    })
}

#[derive(Serialize)]
pub struct DuplicateGroup {
    pub hash: String,
    pub size: u64,
    pub files: Vec<FileEntry>,
}

const HEAD_QUICK: usize = 4 * 1024;
const SAMPLE: usize = 16 * 1024;

/// Tier 1: 4 KiB head-only xxh3_64 to eliminate ~95%+ of non-duplicates in one sequential read.
fn head_quick_hash(path: &Path) -> Option<u64> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = [0u8; HEAD_QUICK];
    let read = file.read(&mut buf).ok()?;
    Some(xxh3_64(&buf[..read]))
}

/// Tier 2: 16 KiB head + 16 KiB tail xxh3_64.
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

/// Tier 3: SIMD blake3 full hash with SQLite cache for large files (> 10 MB).
fn full_hash_blake3(path: &Path, size: u64, cache: Option<&AnalysisCacheDb>) -> Option<String> {
    let before = fs::metadata(path).ok()?;
    let mtime = platform::modified_secs(&before);
    let path_str = platform::path_string(path);
    if size >= LARGE_FILE_CACHE_THRESHOLD
        && let Some(db) = cache
        && let Some(cached) = db.get_hash(&path_str, size, mtime)
    {
        return Some(cached);
    }

    let mut file = fs::File::open(path).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut chunk = vec![0u8; 256 * 1024];
    loop {
        let read = file.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    let after = fs::metadata(path).ok()?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return None;
    }
    let digest = hasher.finalize().to_hex().to_string();
    if size >= LARGE_FILE_CACHE_THRESHOLD
        && let Some(db) = cache
    {
        db.put_hash(&path_str, size, mtime, &digest);
    }
    Some(digest)
}

pub fn duplicates_with_cache(
    root: &Path,
    cache: Option<&AnalysisCacheDb>,
) -> Result<Vec<DuplicateGroup>, String> {
    if !root.is_dir() {
        return Err("Choose an available folder".into());
    }
    let workers = drive_worker_count(root);
    let mut by_size = HashMap::<u64, Vec<PathBuf>>::new();
    for item in ParallelWalkDir::new(root)
        .follow_links(false)
        .skip_hidden(false)
        .parallelism(Parallelism::RayonNewPool(workers))
    {
        let Ok(item) = item else { continue };
        if !item.file_type().is_file() {
            continue;
        }
        if let Ok(metadata) = item.metadata()
            && metadata.len() > 0
        {
            by_size.entry(metadata.len()).or_default().push(item.path());
        }
    }
    let candidates = by_size
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .collect::<Vec<_>>();
    let mut groups = candidates
        .into_par_iter()
        .flat_map_iter(|(size, files)| {
            // Tier 1: 4 KiB head-only xxh3
            let mut tier1 = HashMap::<u64, Vec<PathBuf>>::new();
            for (h, f) in files
                .into_par_iter()
                .filter_map(|f| head_quick_hash(&f).map(|h| (h, f)))
                .collect::<Vec<_>>()
            {
                tier1.entry(h).or_default().push(f);
            }

            let mut out = Vec::new();
            for t1_files in tier1.into_values().filter(|f| f.len() > 1) {
                // Tier 2: 16+16 KiB head+tail xxh3 (skip if <= 4 KiB, already fully read!)
                let tier2_groups: Vec<Vec<PathBuf>> = if size <= HEAD_QUICK as u64 {
                    vec![t1_files]
                } else {
                    let mut samples = HashMap::<u64, Vec<PathBuf>>::new();
                    for (hash, file) in t1_files
                        .into_par_iter()
                        .filter_map(|f| sample_hash(&f, size).map(|h| (h, f)))
                        .collect::<Vec<_>>()
                    {
                        samples.entry(hash).or_default().push(file);
                    }
                    samples.into_values().filter(|f| f.len() > 1).collect()
                };

                for files in tier2_groups {
                    // Tier 3: SIMD blake3
                    let mut full = HashMap::<String, Vec<PathBuf>>::new();
                    for (hash, file) in files
                        .into_par_iter()
                        .filter_map(|f| full_hash_blake3(&f, size, cache).map(|h| (h, f)))
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
            }
            out
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|g| std::cmp::Reverse(g.size.saturating_mul(g.files.len() as u64 - 1)));
    Ok(groups)
}

#[allow(dead_code)]
pub fn duplicates(root: &Path) -> Result<Vec<DuplicateGroup>, String> {
    duplicates_with_cache(root, None)
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

    let items_to_scan: Vec<(String, PathBuf, String)> = roots
        .into_iter()
        .filter(|(_, r)| r.is_dir())
        .flat_map(|(category, root)| {
            fs::read_dir(&root)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(move |item| {
                    (
                        category.clone(),
                        item.path(),
                        item.file_name().to_string_lossy().into_owned(),
                    )
                })
        })
        .collect();

    let mut candidates: Vec<CleanupCandidate> = items_to_scan
        .into_par_iter()
        .filter_map(|(category, path, name)| {
            // macOS keeps live system state here.
            if cfg!(target_os = "macos")
                && (name.starts_with("com.apple") || name == "TemporaryItems")
            {
                return None;
            }
            let metadata = fs::symlink_metadata(&path).ok()?;
            if metadata.file_type().is_symlink() || !owned_by_current_user(&metadata, home) {
                return None;
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
                return None;
            }
            if count == 0 || newest > cutoff {
                return None;
            }
            Some(CleanupCandidate {
                name,
                path: platform::path_string(&path),
                category,
                bytes,
                item_count: count,
                modified: newest
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            })
        })
        .collect();

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
    fn duplicates_3tier_distinguishes_same_head_different_middle() {
        let dir = std::env::temp_dir().join(format!("tidybeam-dupes-mid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut a = vec![0xAAu8; 40_000];
        let mut b = vec![0xAAu8; 40_000];
        b[20_000] = 0xBB;
        fs::write(dir.join("a.bin"), &a).unwrap();
        fs::write(dir.join("b.bin"), &b).unwrap();
        a[0] = 0xAA;
        let groups = duplicates(&dir).unwrap();
        assert!(groups.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn analysis_counts_files_and_caches_subtrees() {
        let dir = std::env::temp_dir().join(format!("tidybeam-analyze-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("a.bin"), vec![0u8; 100]).unwrap();
        fs::write(dir.join("b.bin"), vec![0u8; 50]).unwrap();
        let snapshots = AnalysisSnapshots::default();
        let cancel = AtomicBool::new(false);
        let analysis =
            analyze_cancellable(&dir, "t1", &cancel, Some(&snapshots), None, |_| {}).unwrap();
        assert_eq!(analysis.file_count, 2);
        assert_eq!(analysis.total_bytes, 150);
        assert_eq!(analysis.folders[0].name, "sub");
        // Subfolder drill-down served from arena cache without re-walking disk
        let sub_cached = snapshots.get(&dir.join("sub")).unwrap();
        assert_eq!(sub_cached.file_count, 1);
        assert_eq!(sub_cached.total_bytes, 100);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn analysis_respects_cancellation() {
        let dir = std::env::temp_dir().join(format!("tidybeam-cancel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.bin"), b"hello").unwrap();
        let cancel = AtomicBool::new(true);
        let res = analyze_cancellable(&dir, "c1", &cancel, None, None, |_| {});
        assert!(res.is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
