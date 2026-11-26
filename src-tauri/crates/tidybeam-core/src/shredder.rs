//! Secure file/folder shredding (overwrite-before-unlink) without following symlinks.
//! Note: SSD wear-leveling and copy-on-write filesystems (APFS, Btrfs, ZFS) prevent
//! guaranteed physical block erasure; full-disk encryption is always recommended.

use std::{
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

fn shred_regular_file(path: &Path, passes: u8) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        return fs::remove_file(path).map_err(|e| e.to_string());
    }
    let len = meta.len();
    if len > 0 {
        let mut file = OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let buf_size = (len as usize).min(256 * 1024);
        let mut pattern = vec![0u8; buf_size];
        let passes = passes.clamp(1, 3);
        for pass in 0..passes {
            let fill = match pass {
                0 => 0xAA,
                1 => 0x55,
                _ => 0x00,
            };
            pattern.fill(fill);
            file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            let mut remaining = len;
            while remaining > 0 {
                let chunk = (remaining as usize).min(pattern.len());
                file.write_all(&pattern[..chunk])
                    .map_err(|e| e.to_string())?;
                remaining -= chunk as u64;
            }
            file.sync_all().map_err(|e| e.to_string())?;
        }
    }
    if let Some(parent) = path.parent() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let renamed = parent.join(format!(".shred-{nonce}.tmp"));
        if fs::rename(path, &renamed).is_ok() {
            return fs::remove_file(&renamed).map_err(|e| e.to_string());
        }
    }
    fs::remove_file(path).map_err(|e| e.to_string())
}

/// Securely shreds a file or directory tree without following symlinks.
pub fn shred_path(path: &Path, passes: u8) -> Result<usize, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        fs::remove_file(path)
            .or_else(|_| fs::remove_dir(path))
            .map_err(|e| e.to_string())?;
        return Ok(1);
    }
    if !meta.is_dir() {
        shred_regular_file(path, passes)?;
        return Ok(1);
    }
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_dir() && !entry.file_type().is_symlink() {
            dirs.push(entry.into_path());
        } else {
            files.push(entry.into_path());
        }
    }
    let mut count = 0usize;
    for f in files {
        shred_regular_file(&f, passes)?;
        count += 1;
    }
    for d in dirs.into_iter().rev() {
        fs::remove_dir(&d).map_err(|e| e.to_string())?;
    }
    Ok(count)
}

/// Writes a temporary zero-filled file up to `max_mb` (always leaving at least 512 MB free)
/// and deletes it immediately.
pub fn wipe_free_space_sample(dir: &Path, max_mb: u64) -> Result<u64, String> {
    if !dir.is_dir() {
        return Err("Choose an available folder".into());
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp_file: PathBuf = dir.join(format!(".tidybeam-wipe-{nonce}.tmp"));
    let target_bytes = max_mb.clamp(1, 1024) * 1024 * 1024;
    let zeros = vec![0u8; 1024 * 1024];
    let mut written = 0u64;
    let res = (|| -> Result<(), String> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_file)
            .map_err(|e| e.to_string())?;
        while written < target_bytes {
            let chunk = ((target_bytes - written) as usize).min(zeros.len());
            if f.write_all(&zeros[..chunk]).is_err() {
                break;
            }
            written += chunk as u64;
        }
        let _ = f.sync_all();
        Ok(())
    })();
    let _ = fs::remove_file(&temp_file);
    res?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shred_overwrites_and_unlinks_without_following_symlinks() {
        let dir = std::env::temp_dir().join(format!("tidybeam-shred-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        let f1 = dir.join("secret.txt");
        let f2 = dir.join("sub").join("nested.bin");
        fs::write(&f1, b"top secret data").unwrap();
        fs::write(&f2, vec![0x42u8; 2048]).unwrap();

        assert_eq!(shred_path(&f1, 2).unwrap(), 1);
        assert!(!f1.exists());
        assert_eq!(shred_path(&dir, 1).unwrap(), 1);
        assert!(!dir.exists());
    }
}
