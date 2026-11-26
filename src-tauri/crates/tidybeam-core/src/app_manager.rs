//! Cross-OS Installed Application inventory & Safe Leftover Scanner.

use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstalledApp {
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub location: String,
    pub source: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverFolder {
    pub name: String,
    pub path: String,
    pub bytes: u64,
}

pub fn list_installed_apps(home: &Path) -> Vec<InstalledApp> {
    let mut apps = Vec::new();
    #[cfg(windows)]
    {
        list_windows_uninstall(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall",
            "User",
            &mut apps,
        );
        list_windows_uninstall(
            r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall",
            "System",
            &mut apps,
        );
    }
    #[cfg(target_os = "macos")]
    {
        list_macos_apps(Path::new("/Applications"), "Applications", &mut apps);
        list_macos_apps(&home.join("Applications"), "User Applications", &mut apps);
    }
    #[cfg(target_os = "linux")]
    {
        list_linux_desktops(Path::new("/usr/share/applications"), "System", &mut apps);
        list_linux_desktops(&home.join(".local/share/applications"), "User", &mut apps);
    }
    let _ = home;
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    apps
}

#[cfg(windows)]
fn list_windows_uninstall(key: &str, source: &str, out: &mut Vec<InstalledApp>) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let Ok(output) = std::process::Command::new("reg")
        .args(["query", key, "/s"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut name = String::new();
    let mut version = String::new();
    let mut publisher = String::new();
    let mut location = String::new();

    let flush = |out: &mut Vec<InstalledApp>,
                 name: &mut String,
                 version: &mut String,
                 publisher: &mut String,
                 location: &mut String| {
        if !name.is_empty() {
            out.push(InstalledApp {
                name: std::mem::take(name),
                version: std::mem::take(version),
                publisher: std::mem::take(publisher),
                location: std::mem::take(location),
                source: source.into(),
            });
        } else {
            version.clear();
            publisher.clear();
            location.clear();
        }
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("HKEY_") {
            flush(out, &mut name, &mut version, &mut publisher, &mut location);
            continue;
        }
        if let Some((k, rest)) = trimmed.split_once("REG_SZ") {
            let val = rest.trim().to_string();
            match k.trim() {
                "DisplayName" => name = val,
                "DisplayVersion" => version = val,
                "Publisher" => publisher = val,
                "InstallLocation" => location = val,
                _ => {}
            }
        }
    }
    flush(out, &mut name, &mut version, &mut publisher, &mut location);
}

#[allow(dead_code)]
pub fn list_macos_apps(dir: &Path, source: &str, out: &mut Vec<InstalledApp>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("app") {
            let name = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(InstalledApp {
                name,
                version: String::new(),
                publisher: String::new(),
                location: p.to_string_lossy().into_owned(),
                source: source.into(),
            });
        }
    }
}

#[allow(dead_code)]
pub fn list_linux_desktops(dir: &Path, source: &str, out: &mut Vec<InstalledApp>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&p) else {
            continue;
        };
        let mut name = String::new();
        for line in content.lines() {
            if let Some(v) = line.strip_prefix("Name=") {
                name = v.trim().to_string();
                break;
            }
        }
        if !name.is_empty() {
            out.push(InstalledApp {
                name,
                version: String::new(),
                publisher: String::new(),
                location: p.to_string_lossy().into_owned(),
                source: source.into(),
            });
        }
    }
}

/// Scans user app-data directories for folders matching `app_name`.
pub fn find_app_leftovers(app_name: &str, home: &Path) -> Vec<LeftoverFolder> {
    let needle = app_name.trim().to_lowercase();
    if needle.len() < 3 {
        return Vec::new();
    }
    let mut search_roots = Vec::new();
    if cfg!(windows) {
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            search_roots.push(PathBuf::from(l));
        }
        if let Some(r) = std::env::var_os("APPDATA") {
            search_roots.push(PathBuf::from(r));
        }
    } else if cfg!(target_os = "macos") {
        search_roots.push(home.join("Library/Application Support"));
        search_roots.push(home.join("Library/Caches"));
    } else {
        search_roots.push(home.join(".config"));
        search_roots.push(home.join(".local/share"));
        search_roots.push(home.join(".cache"));
    }

    let mut found = Vec::new();
    for root in search_roots {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let fname = entry.file_name().to_string_lossy().into_owned();
            if fname.to_lowercase().contains(&needle) {
                let bytes = walkdir::WalkDir::new(&p)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().is_file())
                    .filter_map(|e| e.metadata().ok())
                    .map(|m| m.len())
                    .sum();
                found.push(LeftoverFolder {
                    name: fname,
                    path: p.to_string_lossy().into_owned(),
                    bytes,
                });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_desktop_inventory_parses_names() {
        let dir = std::env::temp_dir().join(format!("tidybeam-apps-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("sample.desktop"),
            "[Desktop Entry]\nName=Sample Editor\nExec=/usr/bin/sample\n",
        )
        .unwrap();
        let mut apps = Vec::new();
        list_linux_desktops(&dir, "User", &mut apps);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Sample Editor");
        let _ = fs::remove_dir_all(&dir);
    }
}
