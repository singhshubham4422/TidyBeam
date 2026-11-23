//! Platform-specific helpers: path normalization, hidden/executable detection,
//! name validation, trash, and drive filtering.

use serde::Serialize;
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Serialize, Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: u64,
    pub extension: String,
    pub hidden: bool,
    pub executable: bool,
}

/// Canonicalizes a path without producing Windows `\\?\` verbatim prefixes when avoidable.
pub fn canonical(path: &Path) -> io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// Converts a path to the string shown in the UI and stored in settings or the index.
pub fn path_string(path: &Path) -> String {
    dunce::simplified(path).to_string_lossy().into_owned()
}

/// Removes trailing separators and `.` components so equal paths compare equal.
pub fn tidy(path: &Path) -> PathBuf {
    let simplified = dunce::simplified(path);
    let mut out = PathBuf::new();
    for component in simplified.components() {
        match component {
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        simplified.to_path_buf()
    } else {
        out
    }
}

pub fn normalized_key(path: &Path) -> String {
    let value = path_string(path);
    if cfg!(any(windows, target_os = "macos")) {
        value.to_lowercase()
    } else {
        value
    }
}

pub fn same_or_descendant(path: &Path, parent: &Path) -> bool {
    let path = normalized_key(&tidy(path));
    let mut parent = normalized_key(&tidy(parent));
    while parent.len() > 1 && parent.ends_with(['\\', '/']) {
        parent.pop();
    }
    if path == parent {
        return true;
    }
    let sep = std::path::MAIN_SEPARATOR;
    if parent.ends_with(sep) {
        path.starts_with(&parent)
    } else {
        path.starts_with(&format!("{parent}{sep}"))
    }
}

pub fn same_existing_path(left: &Path, right: &Path) -> bool {
    match (canonical(left), canonical(right)) {
        (Ok(left), Ok(right)) => {
            left == right
                || normalized_key(&left) == normalized_key(&right)
                    && same_file_identity(&left, &right)
        }
        _ => false,
    }
}

/// On case-insensitive file systems two different spellings can name the same file.
/// Comparing metadata identity avoids treating two distinct files on a case-sensitive
/// volume as the same item.
fn same_file_identity(left: &Path, right: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (fs::metadata(left), fs::metadata(right)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        // NTFS and FAT are case-insensitive by default, so the canonical path match suffices.
        let _ = (left, right);
        true
    }
}

pub fn modified_secs(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn is_hidden(name: &str, metadata: &fs::Metadata) -> bool {
    if name.starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        if metadata.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0 {
            return true;
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        const UF_HIDDEN: u32 = 0x8000;
        if metadata.st_flags() & UF_HIDDEN != 0 {
            return true;
        }
    }
    let _ = metadata;
    false
}

/// Extensions that launch programs or scripts when opened with the default handler.
const LAUNCH_EXTENSIONS: &[&str] = &[
    "exe",
    "com",
    "bat",
    "cmd",
    "msi",
    "msix",
    "appx",
    "ps1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "scr",
    "pif",
    "lnk",
    "url",
    "hta",
    "cpl",
    "reg",
    "jar",
    "sh",
    "bash",
    "zsh",
    "fish",
    "command",
    "tool",
    "run",
    "appimage",
    "desktop",
    "app",
    "pkg",
    "dmg",
    "deb",
    "rpm",
    "py",
    "pyw",
    "pl",
    "rb",
    "msp",
    "mst",
    "msc",
    "chm",
    "inf",
    "scf",
    "gadget",
    "application",
    "appref-ms",
    "settingcontent-ms",
    "ws",
    "wsc",
    "xll",
    "xbap",
    "diagcab",
    "ocx",
    "terminal",
    "scpt",
    "applescript",
    "workflow",
    "action",
    "prefpane",
    "kext",
    "csh",
    "ksh",
];

/// Filesystem roots and the home folder itself must never be trashed or deleted.
pub fn is_protected(path: &Path) -> bool {
    let tidy_path = tidy(path);
    if tidy_path
        .parent()
        .is_none_or(|parent| parent.as_os_str().is_empty())
    {
        return true;
    }
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    match (home, canonical(&tidy_path)) {
        (Some(home), Ok(resolved)) => {
            canonical(&home).is_ok_and(|home| normalized_key(&home) == normalized_key(&resolved))
        }
        _ => false,
    }
}

/// Whether opening this item could run code: checks the item and, for links, its target.
pub fn needs_launch_confirmation(path: &Path) -> Result<bool, String> {
    let entry = entry_from_path(path)?;
    if entry.executable {
        return Ok(true);
    }
    if entry.is_symlink
        && let Ok(target) = canonical(path)
    {
        return Ok(entry_from_path(&target)?.executable);
    }
    Ok(false)
}

pub fn launches_program(name: &str, extension: &str, metadata: &fs::Metadata) -> bool {
    if metadata.is_dir() {
        return cfg!(target_os = "macos") && extension == "app";
    }
    if LAUNCH_EXTENSIONS.contains(&extension) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Files without an extension that carry an execute bit are programs. Removable
        // drives often mark every file executable, so data files with extensions are excluded.
        if extension.is_empty() && metadata.permissions().mode() & 0o111 != 0 {
            return true;
        }
    }
    let _ = name;
    false
}

pub fn entry_from_metadata(path: &Path, metadata: &fs::Metadata) -> FileEntry {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_string(path));
    let is_symlink = metadata.file_type().is_symlink();
    // A link to a folder should behave like a folder for navigation.
    let target = if is_symlink {
        fs::metadata(path).ok()
    } else {
        None
    };
    let effective = target.as_ref().unwrap_or(metadata);
    let is_dir = effective.is_dir();
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    FileEntry {
        hidden: is_hidden(&name, metadata),
        executable: launches_program(&name, &extension, effective),
        is_dir,
        is_symlink,
        size: if is_dir { 0 } else { effective.len() },
        modified: modified_secs(effective),
        path: path_string(path),
        extension,
        name,
    }
}

pub fn entry_from_path(path: &Path) -> Result<FileEntry, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(entry_from_metadata(path, &metadata))
}

const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
    "com8", "com9", "lpt0", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    "com¹", "com²", "com³", "lpt¹", "lpt²", "lpt³",
];

/// Validates a single file or folder name for the current platform.
pub fn validate_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() || name == "." || name == ".." {
        return Err("Enter a name".into());
    }
    if name.len() > 255 {
        return Err("Names can be at most 255 bytes long".into());
    }
    if name.contains(['/', '\0']) {
        return Err("Names can't contain “/”".into());
    }
    if cfg!(windows) {
        if let Some(bad) = name.chars().find(|ch| {
            matches!(ch, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || (*ch as u32) < 32
        }) {
            return Err(format!("Names can't contain “{bad}” on Windows"));
        }
        if name.ends_with('.') || name.ends_with(' ') {
            return Err("Names can't end with a dot or a space on Windows".into());
        }
        let stem = name
            .split('.')
            .next()
            .unwrap_or(name)
            .trim_end()
            .to_lowercase();
        if WINDOWS_RESERVED.contains(&stem.as_str()) {
            return Err(format!("“{name}” is a reserved name on Windows"));
        }
    }
    Ok(name.to_string())
}

pub fn trash_name() -> &'static str {
    if cfg!(windows) {
        "Recycle Bin"
    } else {
        "Trash"
    }
}

fn trash_context() -> trash::TrashContext {
    #[allow(unused_mut)]
    let mut context = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        // NSFileManager avoids the Finder automation permission prompt and sound.
        context.set_delete_method(DeleteMethod::NsFileManager);
    }
    context
}

pub fn trash_one(path: &Path) -> Result<(), String> {
    if is_protected(path) {
        return Err(format!(
            "{} can't be moved to the {}",
            path.display(),
            trash_name()
        ));
    }
    trash_context()
        .delete(path)
        .map_err(|error| error.to_string())
}

pub fn delete_permanently(path: &Path) -> Result<(), String> {
    if is_protected(path) {
        return Err(format!("{} can't be deleted", path.display()));
    }
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let result = if metadata.file_type().is_symlink() {
        // Remove only the link, never what it points to.
        fs::remove_file(path).or_else(|_| fs::remove_dir(path))
    } else if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|e| format!("{}: {e}", path.display()))
}

pub fn is_cross_device(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::CrossesDevices {
        return true;
    }
    #[cfg(windows)]
    const CROSS_DEVICE: i32 = 17; // ERROR_NOT_SAME_DEVICE
    #[cfg(not(windows))]
    const CROSS_DEVICE: i32 = 18; // EXDEV
    error.raw_os_error() == Some(CROSS_DEVICE)
}

/// Filters pseudo, loop and system mounts so the drive list shows only real user storage.
pub fn is_user_visible_mount(mount: &str, file_system: &str) -> bool {
    let fs_lower = file_system.to_lowercase();
    if cfg!(windows) {
        return true;
    }
    const PSEUDO_FS: &[&str] = &[
        "tmpfs",
        "devtmpfs",
        "devfs",
        "proc",
        "sysfs",
        "cgroup",
        "cgroup2",
        "squashfs",
        "overlay",
        "efivarfs",
        "ramfs",
        "autofs",
        "securityfs",
        "debugfs",
        "tracefs",
        "fusectl",
        "configfs",
        "pstore",
        "bpf",
        "mqueue",
        "hugetlbfs",
        "nsfs",
        "binfmt_misc",
        "devpts",
        "fuse.portal",
        "fuse.gvfsd-fuse",
        "nullfs",
    ];
    if PSEUDO_FS.contains(&fs_lower.as_str()) {
        return false;
    }
    if cfg!(target_os = "macos") {
        return mount == "/"
            || (mount.starts_with("/Volumes/") && !mount.starts_with("/Volumes/Recovery"));
    }
    if mount == "/" {
        return true;
    }
    const HIDDEN_PREFIXES: &[&str] = &[
        "/boot",
        "/snap",
        "/sys",
        "/proc",
        "/dev",
        "/var/lib",
        "/var/snap",
        "/efi",
        "/recovery",
    ];
    if HIDDEN_PREFIXES
        .iter()
        .any(|prefix| mount == *prefix || mount.starts_with(&format!("{prefix}/")))
    {
        return false;
    }
    if mount.starts_with("/run") {
        return mount.starts_with("/run/media/");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names() {
        assert!(validate_name("report.pdf").is_ok());
        assert!(validate_name("  ").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("a/b").is_err());
        if cfg!(windows) {
            assert!(validate_name("CON").is_err());
            assert!(validate_name("nul.txt").is_err());
            assert!(validate_name("what?").is_err());
            assert!(validate_name("trailing.").is_err());
            assert!(validate_name("console.txt").is_ok());
        }
    }

    #[test]
    fn containment_respects_boundaries() {
        let base = std::env::temp_dir().join("tidybeam-containment");
        assert!(same_or_descendant(&base.join("child"), &base));
        assert!(same_or_descendant(&base, &base));
        assert!(!same_or_descendant(
            &std::env::temp_dir().join("tidybeam-containment-copy"),
            &base
        ));
    }

    #[test]
    fn filters_pseudo_mounts() {
        if cfg!(target_os = "linux") {
            assert!(is_user_visible_mount("/", "ext4"));
            assert!(!is_user_visible_mount("/snap/core/1", "squashfs"));
            assert!(!is_user_visible_mount("/boot/efi", "vfat"));
            assert!(is_user_visible_mount("/run/media/me/USB", "vfat"));
            assert!(is_user_visible_mount("/media/me/USB", "exfat"));
        }
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;

    #[test]
    fn roots_and_home_are_protected() {
        let root = if cfg!(windows) {
            Path::new("C:\\")
        } else {
            Path::new("/")
        };
        assert!(is_protected(root));
        if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
            assert!(is_protected(Path::new(&home)));
        }
        assert!(!is_protected(
            &std::env::temp_dir().join("tidybeam-not-protected")
        ));
        assert!(trash_one(root).is_err());
        assert!(delete_permanently(root).is_err());
    }
}
