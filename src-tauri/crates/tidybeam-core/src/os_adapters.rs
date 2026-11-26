//! Cross-OS Startup Manager adapters for Windows, macOS, and Linux.

use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    pub id: String,
    pub name: String,
    pub command: String,
    pub scope: String,
    pub enabled: bool,
    pub path: String,
}

/// Lists user-scoped startup entries across Windows, macOS, and Linux.
pub fn list_startup_items(home: &Path) -> Vec<StartupItem> {
    let mut items = Vec::new();
    #[cfg(windows)]
    {
        list_windows_startup(home, &mut items);
    }
    #[cfg(target_os = "macos")]
    {
        list_macos_launch_agents(&home.join("Library/LaunchAgents"), &mut items);
    }
    #[cfg(target_os = "linux")]
    {
        list_linux_autostart(&home.join(".config/autostart"), &mut items);
    }
    let _ = home;
    items.sort_by_key(|a| a.name.to_lowercase());
    items
}

#[cfg(windows)]
fn list_windows_startup(home: &Path, out: &mut Vec<StartupItem>) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // 1. User Startup folder
    let startup_dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData").join("Roaming"))
        .join("Microsoft\\Windows\\Start Menu\\Programs\\Startup");
    if let Ok(entries) = fs::read_dir(&startup_dir) {
        for entry in entries.filter_map(Result::ok) {
            let p = entry.path();
            if p.is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let enabled = !name.ends_with(".disabled");
                out.push(StartupItem {
                    id: format!("folder:{}", p.display()),
                    name: name.trim_end_matches(".disabled").to_string(),
                    command: p.to_string_lossy().into_owned(),
                    scope: "Startup Folder".into(),
                    enabled,
                    path: p.to_string_lossy().into_owned(),
                });
            }
        }
    }

    // 2. HKCU\Software\Microsoft\Windows\CurrentVersion\Run
    if let Ok(output) = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("HKEY_") {
                continue;
            }
            if let Some((name, rest)) = trimmed.split_once("REG_") {
                let cmd = rest
                    .split_once("    ")
                    .map(|(_, c)| c.trim())
                    .unwrap_or("")
                    .to_string();
                let key_name = name.trim().to_string();
                if !key_name.is_empty() {
                    out.push(StartupItem {
                        id: format!("hkcu:{key_name}"),
                        name: key_name,
                        command: cmd,
                        scope: "User Registry (Run)".into(),
                        enabled: true,
                        path: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run".into(),
                    });
                }
            }
        }
    }
}

pub fn list_macos_launch_agents(dir: &Path, out: &mut Vec<StartupItem>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".plist") && !name.ends_with(".plist.disabled") {
            continue;
        }
        let enabled = !name.ends_with(".disabled");
        out.push(StartupItem {
            id: format!("launchd:{}", p.display()),
            name: name
                .trim_end_matches(".disabled")
                .trim_end_matches(".plist")
                .to_string(),
            command: p.to_string_lossy().into_owned(),
            scope: "LaunchAgent".into(),
            enabled,
            path: p.to_string_lossy().into_owned(),
        });
    }
}

pub fn list_linux_autostart(dir: &Path, out: &mut Vec<StartupItem>) {
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
        let mut name = entry.file_name().to_string_lossy().into_owned();
        let mut exec = String::new();
        let mut enabled = true;
        for line in content.lines() {
            if let Some(v) = line.strip_prefix("Name=") {
                name = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("Exec=") {
                exec = v.trim().to_string();
            } else if line.trim().eq_ignore_ascii_case("Hidden=true")
                || line
                    .trim()
                    .eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false")
            {
                enabled = false;
            }
        }
        out.push(StartupItem {
            id: format!("xdg:{}", p.display()),
            name,
            command: exec,
            scope: "XDG Autostart".into(),
            enabled,
            path: p.to_string_lossy().into_owned(),
        });
    }
}

pub fn toggle_startup_item(id: &str, enable: bool) -> Result<(), String> {
    if let Some(path_str) = id.strip_prefix("folder:") {
        let p = PathBuf::from(path_str);
        let target = if enable {
            PathBuf::from(path_str.trim_end_matches(".disabled"))
        } else if !path_str.ends_with(".disabled") {
            PathBuf::from(format!("{path_str}.disabled"))
        } else {
            p.clone()
        };
        if p != target {
            fs::rename(&p, &target).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if let Some(path_str) = id.strip_prefix("launchd:") {
        let p = PathBuf::from(path_str);
        let target = if enable {
            PathBuf::from(path_str.trim_end_matches(".disabled"))
        } else if !path_str.ends_with(".disabled") {
            PathBuf::from(format!("{path_str}.disabled"))
        } else {
            p.clone()
        };
        if p != target {
            fs::rename(&p, &target).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if let Some(path_str) = id.strip_prefix("xdg:") {
        let p = PathBuf::from(path_str);
        let content = fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let mut lines: Vec<String> = content
            .lines()
            .filter(|l| {
                !l.trim().starts_with("Hidden=")
                    && !l.trim().starts_with("X-GNOME-Autostart-enabled=")
            })
            .map(String::from)
            .collect();
        if !enable {
            lines.push("Hidden=true".into());
            lines.push("X-GNOME-Autostart-enabled=false".into());
        }
        fs::write(&p, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
        return Ok(());
    }
    Err("This startup entry can only be managed from OS Settings".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_autostart_lists_and_toggles() {
        let dir = std::env::temp_dir().join(format!("tidybeam-startup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let desktop = dir.join("demo.desktop");
        fs::write(
            &desktop,
            "[Desktop Entry]\nType=Application\nName=Demo App\nExec=/usr/bin/demo\n",
        )
        .unwrap();
        let mut items = Vec::new();
        list_linux_autostart(&dir, &mut items);
        assert_eq!(items.len(), 1);
        assert!(items[0].enabled);

        toggle_startup_item(&items[0].id, false).unwrap();
        items.clear();
        list_linux_autostart(&dir, &mut items);
        assert!(!items[0].enabled);

        toggle_startup_item(&items[0].id, true).unwrap();
        items.clear();
        list_linux_autostart(&dir, &mut items);
        assert!(items[0].enabled);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn macos_launch_agent_lists_and_toggles() {
        let dir = std::env::temp_dir().join(format!("tidybeam-launchd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let plist = dir.join("com.example.agent.plist");
        fs::write(&plist, "<plist></plist>").unwrap();
        let mut items = Vec::new();
        list_macos_launch_agents(&dir, &mut items);
        assert_eq!(items.len(), 1);
        assert!(items[0].enabled);
        toggle_startup_item(&items[0].id, false).unwrap();
        items.clear();
        list_macos_launch_agents(&dir, &mut items);
        assert!(!items[0].enabled);
        let _ = fs::remove_dir_all(&dir);
    }
}
