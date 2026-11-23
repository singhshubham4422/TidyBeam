//! ZIP listing, extraction (with path, size and zip-bomb checks) and creation (Zip64-aware).

use crate::platform;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

const MAX_ENTRIES: usize = 500_000;
/// Entries that expand more than this ratio and are larger than `RATIO_MIN_SIZE` are refused.
const MAX_RATIO: u64 = 500;
const RATIO_MIN_SIZE: u64 = 100 * 1024 * 1024;

#[derive(Serialize)]
pub struct ArchiveEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub compressed_size: u64,
}

fn open(path: &Path) -> Result<zip::ZipArchive<fs::File>, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    zip::ZipArchive::new(file).map_err(|e| format!("Can't open ZIP archive: {e}"))
}

pub fn list(path: &Path) -> Result<Vec<ArchiveEntry>, String> {
    let mut archive = open(path)?;
    if archive.len() > MAX_ENTRIES {
        return Err("This archive has too many entries to open".into());
    }
    (0..archive.len())
        .map(|index| {
            let entry = archive.by_index(index).map_err(|e| e.to_string())?;
            let safe = entry.enclosed_name().ok_or_else(|| {
                format!(
                    "Unsafe path inside archive: {}",
                    String::from_utf8_lossy(entry.name_raw())
                )
            })?;
            Ok(ArchiveEntry {
                name: safe
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path: safe.to_string_lossy().into_owned(),
                is_dir: entry.is_dir(),
                size: entry.size(),
                compressed_size: entry.compressed_size(),
            })
        })
        .collect()
}

fn safe_output(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut cursor = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err("Archive contains an invalid path".into());
        };
        cursor.push(name);
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err("Extraction path crosses a symbolic link".into());
                }
                let canonical = platform::canonical(&cursor).map_err(|e| e.to_string())?;
                if !platform::same_or_descendant(&canonical, root) {
                    return Err("Extraction path resolves outside the destination".into());
                }
                if index + 1 < components.len() && !metadata.is_dir() {
                    return Err("A file is in the way of a folder in this archive".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(cursor)
}

fn create_dirs(root: &Path, directory: &Path, created: &mut Vec<PathBuf>) -> Result<(), String> {
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| "Extraction path resolves outside the destination")?;
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("Archive contains an invalid path".into());
        };
        cursor.push(name);
        match fs::symlink_metadata(&cursor) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => return Err("A file is in the way of a folder in this archive".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&cursor).map_err(|e| e.to_string())?;
                created.push(cursor.clone());
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

pub fn available_space(path: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|disk| platform::same_or_descendant(path, disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| disk.available_space())
}

pub fn extract(path: &Path, destination: &Path) -> Result<usize, String> {
    if !destination.is_dir() {
        return Err("Destination folder is unavailable".into());
    }
    let root = platform::canonical(destination).map_err(|e| e.to_string())?;
    let mut archive = open(path)?;
    if archive.len() > MAX_ENTRIES {
        return Err("This archive has too many entries to extract".into());
    }
    let mut plans = Vec::with_capacity(archive.len());
    let mut seen = HashMap::<String, bool>::new();
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let relative = entry.enclosed_name().ok_or_else(|| {
            format!(
                "Unsafe path inside archive: {}",
                String::from_utf8_lossy(entry.name_raw())
            )
        })?;
        let is_dir = entry.is_dir();
        let size = entry.size();
        let compressed = entry.compressed_size().max(1);
        if size > RATIO_MIN_SIZE && size / compressed > MAX_RATIO {
            return Err(format!(
                "“{}” expands {}× and looks like a ZIP bomb, so nothing was extracted",
                String::from_utf8_lossy(entry.name_raw()),
                size / compressed
            ));
        }
        total = total.saturating_add(size);
        let key = platform::normalized_key(&relative);
        if let Some(previous_is_dir) = seen.insert(key, is_dir)
            && (!previous_is_dir || !is_dir)
        {
            return Err("Archive contains duplicate file paths".into());
        }
        plans.push((relative, is_dir, size));
    }
    if let Some(free) = available_space(&root)
        && total > free
    {
        return Err(format!(
            "This archive needs {} MB but only {} MB is free",
            total / 1_048_576,
            free / 1_048_576
        ));
    }
    for (relative, is_dir, _) in &plans {
        for parent in relative.ancestors().skip(1) {
            if !parent.as_os_str().is_empty()
                && seen.get(&platform::normalized_key(parent)) == Some(&false)
            {
                return Err("Archive contains a file where a folder is required".into());
            }
        }
        let output = safe_output(&root, relative)?;
        match fs::symlink_metadata(&output) {
            Ok(meta) if *is_dir && meta.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "The destination already contains “{}”",
                    output.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }

    let mut created_files = Vec::<PathBuf>::new();
    let mut created_dirs = Vec::<PathBuf>::new();
    let result = (|| -> Result<usize, String> {
        for (index, (relative, is_dir, declared)) in plans.iter().enumerate() {
            let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
            let output = safe_output(&root, relative)?;
            if *is_dir {
                create_dirs(&root, &output, &mut created_dirs)?;
                continue;
            }
            if let Some(parent) = output.parent() {
                create_dirs(&root, parent, &mut created_dirs)?;
            }
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .map_err(|e| e.to_string())?;
            created_files.push(output.clone());
            // Never write more than the entry declares, whatever the stream produces.
            let written =
                std::io::copy(&mut (&mut entry).take(declared.saturating_add(1)), &mut out)
                    .map_err(|e| e.to_string())?;
            if written > *declared {
                return Err(format!(
                    "“{}” is larger than the archive says",
                    relative.display()
                ));
            }
            out.flush().map_err(|e| e.to_string())?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&output, fs::Permissions::from_mode(mode & 0o777));
            }
        }
        Ok(plans.len())
    })();
    if result.is_err() {
        for file in created_files.iter().rev() {
            let _ = fs::remove_file(file);
        }
        for directory in created_dirs.iter().rev() {
            let _ = fs::remove_dir(directory);
        }
    }
    result
}

fn add_file<W: Write + std::io::Seek>(
    writer: &mut zip::ZipWriter<W>,
    entry_name: String,
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), String> {
    writer
        .start_file(entry_name, file_options(metadata))
        .map_err(|e| e.to_string())?;
    let mut input = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::io::copy(&mut input, writer).map_err(|e| e.to_string())?;
    Ok(())
}

fn file_options(metadata: &fs::Metadata) -> zip::write::SimpleFileOptions {
    #[allow(unused_mut)]
    let mut options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(metadata.len() >= u32::MAX as u64);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        options = options.unix_permissions(metadata.permissions().mode() & 0o777);
    }
    options
}

pub fn create(paths: &[String], destination: &Path, archive_name: &str) -> Result<String, String> {
    if !destination.is_dir() {
        return Err("Destination folder is unavailable".into());
    }
    let canonical_dest = platform::canonical(destination).map_err(|e| e.to_string())?;
    let name = platform::validate_name(archive_name)?;
    let name = if name.to_lowercase().ends_with(".zip") {
        name
    } else {
        format!("{name}.zip")
    };
    let archive_path = destination.join(&name);
    if fs::symlink_metadata(&archive_path).is_ok() {
        return Err("An archive with that name already exists".into());
    }
    for source in paths {
        let source = PathBuf::from(source);
        if source.is_dir()
            && let Ok(canonical_source) = platform::canonical(&source)
            && platform::same_or_descendant(&canonical_dest, &canonical_source)
        {
            return Err("Choose a destination outside the folders being archived".into());
        }
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_path = destination.join(format!(".{name}.{stamp}.partial"));
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        let mut writer = zip::ZipWriter::new(std::io::BufWriter::with_capacity(1 << 20, file));
        let dir_options = zip::write::SimpleFileOptions::default();
        for source_text in paths {
            let source = PathBuf::from(source_text);
            let Ok(metadata) = fs::symlink_metadata(&source) else {
                continue;
            };
            let root_name = source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if metadata.is_dir() {
                for item in WalkDir::new(&source).follow_links(false) {
                    let item = item.map_err(|e| e.to_string())?;
                    let relative = item
                        .path()
                        .strip_prefix(&source)
                        .map_err(|e| e.to_string())?;
                    let mut entry_name = Path::new(&root_name)
                        .join(relative)
                        .to_string_lossy()
                        .replace('\\', "/");
                    if item.file_type().is_dir() {
                        if !entry_name.ends_with('/') {
                            entry_name.push('/');
                        }
                        writer
                            .add_directory(entry_name, dir_options)
                            .map_err(|e| e.to_string())?;
                    } else if item.file_type().is_file() {
                        let meta = item.metadata().map_err(|e| e.to_string())?;
                        add_file(&mut writer, entry_name, item.path(), &meta)?;
                    }
                }
            } else if metadata.is_file() {
                add_file(&mut writer, root_name, &source, &metadata)?;
            }
        }
        let output = writer.finish().map_err(|e| e.to_string())?;
        let file = output.into_inner().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temp_path, &archive_path).map_err(|e| e.to_string())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    Ok(platform::path_string(&archive_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tidybeam-zip-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn destination_inside_source_is_rejected() {
        let dir = temp("inside");
        let source = dir.join("source");
        let destination = source.join("archives");
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join("item.txt"), b"content").unwrap();
        assert!(create(&[source.to_string_lossy().into()], &destination, "backup").is_err());
        assert!(!destination.join("backup.zip").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trip_and_conflict_preflight() {
        let dir = temp("round");
        let source = dir.join("source");
        let archives = dir.join("archives");
        let extracted = dir.join("extracted");
        for folder in [&source, &archives, &extracted] {
            fs::create_dir_all(folder).unwrap();
        }
        fs::write(source.join("item.txt"), b"content").unwrap();
        let archive = create(&[source.to_string_lossy().into()], &archives, "backup").unwrap();
        assert_eq!(list(Path::new(&archive)).unwrap().len(), 2);
        assert_eq!(extract(Path::new(&archive), &extracted).unwrap(), 2);
        assert_eq!(
            fs::read(extracted.join("source").join("item.txt")).unwrap(),
            b"content"
        );
        // A second extraction must refuse without touching existing files.
        assert!(extract(Path::new(&archive), &extracted).is_err());
        assert_eq!(
            fs::read(extracted.join("source").join("item.txt")).unwrap(),
            b"content"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn zip_bomb_is_refused() {
        let dir = temp("bomb");
        let bomb = dir.join("bomb.zip");
        {
            let mut writer = zip::ZipWriter::new(fs::File::create(&bomb).unwrap());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .large_file(false);
            writer.start_file("zeros.bin", options).unwrap();
            let chunk = vec![0u8; 1 << 20];
            for _ in 0..200 {
                writer.write_all(&chunk).unwrap();
            }
            writer.finish().unwrap();
        }
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        assert!(extract(&bomb, &out).unwrap_err().contains("ZIP bomb"));
        assert!(!out.join("zeros.bin").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
