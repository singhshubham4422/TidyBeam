//! Folder listing with backend sorting, filtering, paging, and bounded snapshots.

use crate::platform::{self, FileEntry};
use serde::Serialize;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

const SNAPSHOT_MAX_ENTRIES: usize = 250_000;
const SNAPSHOT_MAX_TOTAL: usize = 400_000;
const SNAPSHOT_MAX_FOLDERS: usize = 8;
const SNAPSHOT_TTL: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SortKey {
    Name,
    Size,
    Modified,
    Kind,
}

impl SortKey {
    pub fn parse(value: &str) -> Self {
        match value {
            "size" => Self::Size,
            "modified" => Self::Modified,
            "kind" => Self::Kind,
            _ => Self::Name,
        }
    }
}

struct Snapshot {
    entries: Vec<FileEntry>,
    lower_names: Vec<String>,
    orders: HashMap<(SortKey, bool), Arc<Vec<u32>>>,
    modified: Option<SystemTime>,
    created_at: Instant,
    last_access: Instant,
}

#[derive(Default, Clone)]
pub struct DirectorySnapshots(Arc<Mutex<HashMap<String, Snapshot>>>);

#[derive(Serialize)]
pub struct ListResult {
    pub entries: Vec<FileEntry>,
    pub total: usize,
}

pub struct ListRequest {
    pub offset: usize,
    pub limit: usize,
    pub show_hidden: bool,
    pub sort: SortKey,
    pub descending: bool,
    pub filter: String,
    pub force: bool,
}

/// Natural, case-insensitive comparison so "file2" sorts before "file10".
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut left = String::new();
                while let Some(c) = a.peek().copied().filter(char::is_ascii_digit) {
                    left.push(c);
                    a.next();
                }
                let mut right = String::new();
                while let Some(c) = b.peek().copied().filter(char::is_ascii_digit) {
                    right.push(c);
                    b.next();
                }
                let l = left.trim_start_matches('0');
                let r = right.trim_start_matches('0');
                let order = l.len().cmp(&r.len()).then_with(|| l.cmp(r));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                a.next();
                b.next();
            }
        }
    }
}

fn build_order(snapshot: &Snapshot, sort: SortKey, descending: bool) -> Vec<u32> {
    let mut order = (0..snapshot.entries.len() as u32).collect::<Vec<_>>();
    let entries = &snapshot.entries;
    let names = &snapshot.lower_names;
    order.sort_by(|&a, &b| {
        let (ea, eb) = (&entries[a as usize], &entries[b as usize]);
        let by_name = || natural_cmp(&names[a as usize], &names[b as usize]);
        let primary = match sort {
            SortKey::Name => by_name(),
            SortKey::Size => ea.size.cmp(&eb.size).then_with(by_name),
            SortKey::Modified => ea.modified.cmp(&eb.modified).then_with(by_name),
            SortKey::Kind => ea.extension.cmp(&eb.extension).then_with(by_name),
        };
        // Folders always stay first; direction applies within each group.
        eb.is_dir.cmp(&ea.is_dir).then(if descending {
            primary.reverse()
        } else {
            primary
        })
    });
    order
}

fn read_folder(directory: &Path, show_hidden: bool) -> Result<Vec<FileEntry>, String> {
    let reader =
        fs::read_dir(directory).map_err(|e| format!("Can't open {}: {e}", directory.display()))?;
    Ok(reader
        .filter_map(Result::ok)
        .filter_map(|item| {
            // DirEntry::metadata reuses data from the directory read on Windows,
            // avoiding a separate system call per item.
            let metadata = item.metadata().ok()?;
            Some(platform::entry_from_metadata(&item.path(), &metadata))
        })
        .filter(|entry| show_hidden || !entry.hidden)
        .collect())
}

fn page(snapshot: &mut Snapshot, request: &ListRequest) -> ListResult {
    let key = (request.sort, request.descending);
    let order = match snapshot.orders.get(&key) {
        Some(order) => order.clone(),
        None => {
            let order = Arc::new(build_order(snapshot, request.sort, request.descending));
            snapshot.orders.insert(key, order.clone());
            order
        }
    };
    let filter = request.filter.trim().to_lowercase();
    let limit = request.limit.clamp(1, 1000);
    if filter.is_empty() {
        return ListResult {
            total: order.len(),
            entries: order
                .iter()
                .skip(request.offset)
                .take(limit)
                .map(|&index| snapshot.entries[index as usize].clone())
                .collect(),
        };
    }
    let matching = order
        .iter()
        .filter(|&&index| snapshot.lower_names[index as usize].contains(&filter))
        .collect::<Vec<_>>();
    ListResult {
        total: matching.len(),
        entries: matching
            .into_iter()
            .skip(request.offset)
            .take(limit)
            .map(|&index| snapshot.entries[index as usize].clone())
            .collect(),
    }
}

pub fn list_entries(
    snapshots: &DirectorySnapshots,
    directory: &Path,
    request: ListRequest,
) -> Result<ListResult, String> {
    let metadata = fs::metadata(directory)
        .map_err(|e| format!("Folder is unavailable: {} ({e})", directory.display()))?;
    if !metadata.is_dir() {
        return Err(format!("Not a folder: {}", directory.display()));
    }
    let modified = metadata.modified().ok();
    let canonical = platform::canonical(directory).unwrap_or_else(|_| directory.to_path_buf());
    let key = format!(
        "{}|{}",
        platform::normalized_key(&canonical),
        request.show_hidden
    );
    let now = Instant::now();
    {
        let mut cache = snapshots
            .0
            .lock()
            .map_err(|_| "Folder cache is unavailable")?;
        if let Some(snapshot) = cache.get_mut(&key) {
            if !request.force
                && snapshot.modified == modified
                && now.duration_since(snapshot.created_at) < SNAPSHOT_TTL
            {
                snapshot.last_access = now;
                return Ok(page(snapshot, &request));
            }
            cache.remove(&key);
        }
    }

    let entries = read_folder(directory, request.show_hidden)?;
    let lower_names = entries.iter().map(|e| e.name.to_lowercase()).collect();
    let mut snapshot = Snapshot {
        entries,
        lower_names,
        orders: HashMap::new(),
        modified,
        created_at: now,
        last_access: now,
    };
    let result = page(&mut snapshot, &request);
    if snapshot.entries.len() <= SNAPSHOT_MAX_ENTRIES {
        let mut cache = snapshots
            .0
            .lock()
            .map_err(|_| "Folder cache is unavailable")?;
        while cache.len() >= SNAPSHOT_MAX_FOLDERS
            || cache.values().map(|s| s.entries.len()).sum::<usize>() + snapshot.entries.len()
                > SNAPSHOT_MAX_TOTAL
        {
            let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, s)| s.last_access)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            cache.remove(&oldest);
        }
        cache.insert(key, snapshot);
    }
    Ok(result)
}

#[derive(Serialize)]
pub struct Segment {
    pub name: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct PathInfo {
    pub path: String,
    pub parent: Option<String>,
    pub segments: Vec<Segment>,
}

pub fn path_info(path: &Path) -> PathInfo {
    let path = platform::tidy(path);
    let mut segments = path
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .map(|ancestor| Segment {
            name: ancestor
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| platform::path_string(ancestor)),
            path: platform::path_string(ancestor),
        })
        .collect::<Vec<_>>();
    segments.reverse();
    PathInfo {
        parent: path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(platform::path_string),
        path: platform::path_string(&path),
        segments,
    }
}

/// Resolves user-typed locations: expands `~`, rejects relative paths.
pub fn resolve_input(raw: &str, home: &Path) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Enter a location".into());
    }
    let path = if trimmed == "~" {
        home.to_path_buf()
    } else if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        home.join(rest)
    } else {
        PathBuf::from(trimmed)
    };
    if !path.is_absolute() {
        return Err(format!("“{trimmed}” isn't a full path"));
    }
    Ok(platform::tidy(&path))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderComparison {
    pub left: String,
    pub right: String,
    pub left_only: usize,
    pub right_only: usize,
    pub different: usize,
    pub items: Vec<FolderComparisonItem>,
}

#[derive(Serialize)]
pub struct FolderComparisonItem {
    pub name: String,
    pub status: String,
    pub left: Option<FileEntry>,
    pub right: Option<FileEntry>,
}

pub fn compare_folders(
    left: &Path,
    right: &Path,
    show_hidden: bool,
) -> Result<FolderComparison, String> {
    if !left.is_dir() || !right.is_dir() {
        return Err("Both folders must be available to compare".into());
    }
    let fold = |name: &str| {
        if cfg!(any(windows, target_os = "macos")) {
            name.to_lowercase()
        } else {
            name.to_string()
        }
    };
    let read = |folder: &Path| -> Result<HashMap<String, FileEntry>, String> {
        Ok(read_folder(folder, show_hidden)?
            .into_iter()
            .map(|entry| (fold(&entry.name), entry))
            .collect())
    };
    let left_entries = read(left)?;
    let right_entries = read(right)?;
    let mut keys = left_entries
        .keys()
        .chain(right_entries.keys())
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    keys.sort_by(|a, b| natural_cmp(a, b));
    let (mut left_only, mut right_only, mut different) = (0, 0, 0);
    let mut items = Vec::new();
    for key in keys {
        let (l, r) = (left_entries.get(&key), right_entries.get(&key));
        let status = match (l, r) {
            (Some(_), None) => {
                left_only += 1;
                "left-only"
            }
            (None, Some(_)) => {
                right_only += 1;
                "right-only"
            }
            (Some(a), Some(b))
                if a.is_dir != b.is_dir || a.size != b.size || a.modified != b.modified =>
            {
                different += 1;
                "different"
            }
            _ => continue,
        };
        if items.len() < 500 {
            items.push(FolderComparisonItem {
                name: l.or(r).map(|e| e.name.clone()).unwrap_or_default(),
                status: status.into(),
                left: l.cloned(),
                right: r.cloned(),
            });
        }
    }
    Ok(FolderComparison {
        left: platform::path_string(left),
        right: platform::path_string(right),
        left_only,
        right_only,
        different,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort_orders_numbers() {
        let mut names = vec!["file10", "file2", "File1", "file02b"];
        names.sort_by(|a, b| natural_cmp(&a.to_lowercase(), &b.to_lowercase()));
        assert_eq!(names, vec!["File1", "file2", "file02b", "file10"]);
    }

    #[test]
    fn path_info_lists_ancestors() {
        let root = if cfg!(windows) {
            PathBuf::from(r"C:\Users\me\docs")
        } else {
            PathBuf::from("/home/me/docs")
        };
        let info = path_info(&root);
        assert_eq!(info.segments.len(), 4);
        assert_eq!(info.segments.last().unwrap().name, "docs");
        assert!(info.parent.unwrap().ends_with("me"));
    }

    #[test]
    fn resolve_rejects_relative_and_expands_home() {
        let home = std::env::temp_dir();
        assert!(resolve_input("relative/path", &home).is_err());
        assert_eq!(resolve_input("~", &home).unwrap(), platform::tidy(&home));
    }

    #[test]
    fn lists_sorted_and_filtered_pages() {
        let dir = std::env::temp_dir().join(format!("tidybeam-list-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("folder")).unwrap();
        fs::write(dir.join("b.txt"), b"12345").unwrap();
        fs::write(dir.join("a10.txt"), b"1").unwrap();
        fs::write(dir.join("a2.txt"), b"123").unwrap();
        let snapshots = DirectorySnapshots::default();
        let request = |sort, descending, filter: &str| ListRequest {
            offset: 0,
            limit: 50,
            show_hidden: false,
            sort,
            descending,
            filter: filter.into(),
            force: true,
        };
        let names = |r: ListResult| r.entries.into_iter().map(|e| e.name).collect::<Vec<_>>();
        assert_eq!(
            names(list_entries(&snapshots, &dir, request(SortKey::Name, false, "")).unwrap()),
            vec!["folder", "a2.txt", "a10.txt", "b.txt"]
        );
        assert_eq!(
            names(list_entries(&snapshots, &dir, request(SortKey::Size, true, "")).unwrap()),
            vec!["folder", "b.txt", "a2.txt", "a10.txt"]
        );
        let filtered = list_entries(&snapshots, &dir, request(SortKey::Name, false, "A")).unwrap();
        assert_eq!(filtered.total, 2);
        let _ = fs::remove_dir_all(&dir);
    }
}
