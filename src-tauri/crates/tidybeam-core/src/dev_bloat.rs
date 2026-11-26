//! Developer & Creator Bloat Cleaner + Old Installer Sweeper.

use rayon::prelude::*;
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DevBloatItem {
    pub name: String,
    pub path: String,
    pub kind: String, // "node_modules" | "rust_target" | "python_venv" | "old_installer" | "empty_dir"
    pub bytes: u64,
    pub modified: u64,
}

fn dir_size_and_newest(path: &Path) -> (u64, SystemTime) {
    let mut bytes = 0u64;
    let mut newest = UNIX_EPOCH;
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file()
            && let Ok(meta) = entry.metadata()
        {
            bytes = bytes.saturating_add(meta.len());
            if let Ok(m) = meta.modified() {
                newest = newest.max(m);
            }
        }
    }
    (bytes, newest)
}

pub fn scan_dev_bloat(root: &Path, inactive_days: u64) -> Vec<DevBloatItem> {
    if !root.is_dir() {
        return Vec::new();
    }
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(inactive_days * 24 * 60 * 60))
        .unwrap_or(UNIX_EPOCH);

    let mut candidate_dirs = Vec::new();
    let mut it = WalkDir::new(root)
        .follow_links(false)
        .max_depth(6)
        .into_iter();
    while let Some(Ok(entry)) = it.next() {
        if !entry.file_type().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        let kind = match name.as_ref() {
            "node_modules" => Some("node_modules"),
            "target"
                if entry
                    .path()
                    .parent()
                    .is_some_and(|p| p.join("Cargo.toml").is_file()) =>
            {
                Some("rust_target")
            }
            ".venv" | "venv" | "__pycache__" => Some("python_venv"),
            _ => None,
        };
        if let Some(k) = kind {
            candidate_dirs.push((k.to_string(), entry.into_path()));
            it.skip_current_dir();
        }
    }

    let mut items: Vec<DevBloatItem> = candidate_dirs
        .into_par_iter()
        .filter_map(|(kind, path)| {
            let (bytes, newest) = dir_size_and_newest(&path);
            if bytes == 0 || (inactive_days > 0 && newest > cutoff) {
                return None;
            }
            Some(DevBloatItem {
                name: path
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| {
                        format!(
                            "{}/{}",
                            n.to_string_lossy(),
                            path.file_name().unwrap_or_default().to_string_lossy()
                        )
                    })
                    .unwrap_or_else(|| path.to_string_lossy().into_owned()),
                path: path.to_string_lossy().into_owned(),
                kind,
                bytes,
                modified: newest
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            })
        })
        .collect();

    // Also scan Downloads for old installers
    let downloads = root.join("Downloads");
    if downloads.is_dir()
        && let Ok(entries) = fs::read_dir(&downloads)
    {
        for entry in entries.filter_map(Result::ok) {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default();
            if !matches!(
                ext.as_str(),
                "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "iso"
            ) {
                continue;
            }
            if let Ok(meta) = fs::symlink_metadata(&p) {
                let modified = meta.modified().unwrap_or(UNIX_EPOCH);
                if inactive_days == 0 || modified <= cutoff {
                    items.push(DevBloatItem {
                        name: entry.file_name().to_string_lossy().into_owned(),
                        path: p.to_string_lossy().into_owned(),
                        kind: "old_installer".into(),
                        bytes: meta.len(),
                        modified: modified
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0),
                    });
                }
            }
        }
    }

    items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
    items
}

/// Finds empty directories under `root` (depth <= 5).
pub fn find_empty_dirs(root: &Path) -> Vec<PathBuf> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut empty = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .min_depth(1)
        .max_depth(5)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_dir()
            && let Ok(mut rd) = fs::read_dir(entry.path())
            && rd.next().is_none()
        {
            empty.push(entry.into_path());
        }
    }
    empty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rust_target_and_empty_dirs() {
        let dir = std::env::temp_dir().join(format!("tidybeam-devbloat-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("proj").join("target")).unwrap();
        fs::write(
            dir.join("proj").join("Cargo.toml"),
            b"[package]\nname=\"p\"",
        )
        .unwrap();
        fs::write(dir.join("proj").join("target").join("bin.o"), vec![1u8; 64]).unwrap();
        fs::create_dir_all(dir.join("empty_sub")).unwrap();

        let bloat = scan_dev_bloat(&dir, 0);
        assert!(bloat.iter().any(|b| b.kind == "rust_target"));
        let empty = find_empty_dirs(&dir);
        assert!(empty.iter().any(|p| p.ends_with("empty_sub")));
        let _ = fs::remove_dir_all(&dir);
    }
}
