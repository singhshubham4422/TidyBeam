//! Cross-OS deep cleaner engine: declarative System, Browser, Application,
//! Privacy, and Scattered Junk rules with parallel discovery, sysinfo process
//! lock checks, and lazy paged IPC item storage.

use rayon::prelude::*;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use sysinfo::System;
use walkdir::WalkDir;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum SafetyBadge {
    Safe,
    SessionReset,
    AppOpen,
    Permanent,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CleanerFileItem {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub modified: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CleanerRuleSummary {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub badge: SafetyBadge,
    pub app_running: bool,
    pub running_process: Option<String>,
    pub total_bytes: u64,
    pub item_count: usize,
    pub sqlite_targets: Vec<String>,
    pub cookie_dbs: Vec<String>,
}

#[derive(Default)]
pub struct CleanerSession {
    items_by_rule: Mutex<HashMap<String, Vec<CleanerFileItem>>>,
    summaries: Mutex<Vec<CleanerRuleSummary>>,
}

impl CleanerSession {
    pub fn store(
        &self,
        summaries: Vec<CleanerRuleSummary>,
        items: HashMap<String, Vec<CleanerFileItem>>,
    ) {
        if let Ok(mut s) = self.summaries.lock() {
            *s = summaries;
        }
        if let Ok(mut m) = self.items_by_rule.lock() {
            *m = items;
        }
    }

    pub fn cached_summaries(&self) -> Option<Vec<CleanerRuleSummary>> {
        let s = self.summaries.lock().ok()?;
        if s.is_empty() { None } else { Some(s.clone()) }
    }

    pub fn get_items(&self, rule_id: &str, offset: usize, limit: usize) -> Vec<CleanerFileItem> {
        let Ok(map) = self.items_by_rule.lock() else {
            return Vec::new();
        };
        let Some(list) = map.get(rule_id) else {
            return Vec::new();
        };
        list.iter()
            .skip(offset)
            .take(limit.clamp(1, 500))
            .cloned()
            .collect()
    }

    pub fn paths_for_rules(&self, rule_ids: &[String]) -> Vec<String> {
        let Ok(map) = self.items_by_rule.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for id in rule_ids {
            if let Some(items) = map.get(id) {
                out.extend(items.iter().map(|i| i.path.clone()));
            }
        }
        out
    }
}

struct RuleSpec {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    description: &'static str,
    badge: SafetyBadge,
    process_names: &'static [&'static str],
    min_age_days: u64,
    paths: Vec<PathBuf>,
    sqlite_globs: Vec<PathBuf>,
    cookie_globs: Vec<PathBuf>,
}

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

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn running_processes() -> HashMap<String, String> {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, false);
    let mut map = HashMap::new();
    for proc_ in sys.processes().values() {
        let name = proc_.name().to_string_lossy().to_string();
        map.insert(name.to_lowercase(), name);
    }
    map
}

fn build_catalog(home: &Path) -> Vec<RuleSpec> {
    let mut rules = Vec::new();
    let local_app = env_path("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"));
    let roaming_app = env_path("APPDATA").unwrap_or_else(|| home.join("AppData").join("Roaming"));
    let xdg_cache = env_path("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"));
    let xdg_config = env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
    let xdg_data = env_path("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share"));

    // 1. System Temp & Caches
    let mut sys_temp_paths = vec![std::env::temp_dir()];
    if cfg!(windows) {
        sys_temp_paths.push(local_app.join("Microsoft").join("Windows").join("WER"));
        sys_temp_paths.push(local_app.join("CrashDumps"));
    } else if cfg!(target_os = "macos") {
        sys_temp_paths.push(home.join("Library").join("Logs"));
        sys_temp_paths.push(home.join("Library").join("Caches"));
    } else {
        sys_temp_paths.push(xdg_cache.join("thumbnails"));
    }
    rules.push(RuleSpec {
        id: "system-temp",
        name: "System Temporary Files & Logs",
        category: "System",
        description: "Temporary files, crash dumps, and user logs older than 7 days.",
        badge: SafetyBadge::Safe,
        process_names: &[],
        min_age_days: 7,
        paths: sys_temp_paths,
        sqlite_globs: Vec::new(),
        cookie_globs: Vec::new(),
    });

    // 2. Privacy Traces (Recent documents)
    let mut recent_paths = Vec::new();
    if cfg!(windows) {
        recent_paths.push(roaming_app.join("Microsoft").join("Windows").join("Recent"));
    } else if cfg!(target_os = "linux") {
        recent_paths.push(xdg_data.join("recently-used.xbel"));
    }
    if !recent_paths.is_empty() {
        rules.push(RuleSpec {
            id: "privacy-recents",
            name: "Recent Documents History",
            category: "Privacy Traces",
            description: "OS recent file shortcuts and recently-used lists.",
            badge: SafetyBadge::Safe,
            process_names: &[],
            min_age_days: 0,
            paths: recent_paths,
            sqlite_globs: Vec::new(),
            cookie_globs: Vec::new(),
        });
    }

    // 3. Browsers (Chromium + Gecko + Safari)
    let mut chrome_caches = Vec::new();
    let mut chrome_cookies = Vec::new();
    let mut chrome_dbs = Vec::new();
    if cfg!(windows) {
        let base = local_app.join("Google").join("Chrome").join("User Data");
        for profile in ["Default", "Profile 1", "Profile 2"] {
            let p = base.join(profile);
            chrome_caches.push(p.join("Cache"));
            chrome_caches.push(p.join("Code Cache"));
            chrome_caches.push(p.join("GPUCache"));
            chrome_dbs.push(p.join("History"));
            chrome_cookies.push(p.join("Network").join("Cookies"));
        }
    } else if cfg!(target_os = "macos") {
        let base = home
            .join("Library")
            .join("Caches")
            .join("Google")
            .join("Chrome");
        chrome_caches.push(base);
        let app_sup = home
            .join("Library")
            .join("Application Support")
            .join("Google")
            .join("Chrome");
        for profile in ["Default", "Profile 1"] {
            chrome_dbs.push(app_sup.join(profile).join("History"));
            chrome_cookies.push(app_sup.join(profile).join("Cookies"));
        }
    } else {
        for base in [
            xdg_cache.join("google-chrome"),
            home.join(".var/app/com.google.Chrome/cache/google-chrome"),
        ] {
            chrome_caches.push(base);
        }
        let cfg_base = xdg_config.join("google-chrome");
        for profile in ["Default", "Profile 1"] {
            chrome_dbs.push(cfg_base.join(profile).join("History"));
            chrome_cookies.push(cfg_base.join(profile).join("Cookies"));
        }
    }
    rules.push(RuleSpec {
        id: "browser-chrome-cache",
        name: "Google Chrome Cache",
        category: "Browsers",
        description: "Cached web assets, shader cache, and code cache.",
        badge: SafetyBadge::AppOpen,
        process_names: &["chrome.exe", "chrome", "google chrome"],
        min_age_days: 0,
        paths: chrome_caches,
        sqlite_globs: chrome_dbs,
        cookie_globs: chrome_cookies,
    });

    // Edge
    let mut edge_caches = Vec::new();
    if cfg!(windows) {
        let base = local_app.join("Microsoft").join("Edge").join("User Data");
        for profile in ["Default", "Profile 1"] {
            let p = base.join(profile);
            edge_caches.push(p.join("Cache"));
            edge_caches.push(p.join("Code Cache"));
        }
    } else if cfg!(target_os = "macos") {
        edge_caches.push(home.join("Library/Caches/Microsoft Edge"));
    } else {
        edge_caches.push(xdg_cache.join("microsoft-edge"));
    }
    rules.push(RuleSpec {
        id: "browser-edge-cache",
        name: "Microsoft Edge Cache",
        category: "Browsers",
        description: "Cached web assets and GPU shader cache.",
        badge: SafetyBadge::AppOpen,
        process_names: &["msedge.exe", "msedge", "microsoft edge"],
        min_age_days: 0,
        paths: edge_caches,
        sqlite_globs: Vec::new(),
        cookie_globs: Vec::new(),
    });

    // Brave
    let mut brave_caches = Vec::new();
    if cfg!(windows) {
        let base = local_app
            .join("BraveSoftware")
            .join("Brave-Browser")
            .join("User Data");
        brave_caches.push(base.join("Default").join("Cache"));
    } else if cfg!(target_os = "macos") {
        brave_caches.push(home.join("Library/Caches/BraveSoftware/Brave-Browser"));
    } else {
        brave_caches.push(xdg_cache.join("BraveSoftware/Brave-Browser"));
    }
    rules.push(RuleSpec {
        id: "browser-brave-cache",
        name: "Brave Browser Cache",
        category: "Browsers",
        description: "Cached web assets and code cache.",
        badge: SafetyBadge::AppOpen,
        process_names: &["brave.exe", "brave", "brave browser"],
        min_age_days: 0,
        paths: brave_caches,
        sqlite_globs: Vec::new(),
        cookie_globs: Vec::new(),
    });

    // Firefox
    let mut ff_paths = Vec::new();
    if cfg!(windows) {
        ff_paths.push(local_app.join("Mozilla").join("Firefox").join("Profiles"));
    } else if cfg!(target_os = "macos") {
        ff_paths.push(home.join("Library/Caches/Firefox/Profiles"));
    } else {
        ff_paths.push(xdg_cache.join("mozilla/firefox"));
        ff_paths.push(home.join(".var/app/org.mozilla.firefox/cache/mozilla/firefox"));
        ff_paths.push(home.join("snap/firefox/common/.cache/mozilla/firefox"));
    }
    rules.push(RuleSpec {
        id: "browser-firefox-cache",
        name: "Mozilla Firefox Cache",
        category: "Browsers",
        description: "Firefox disk cache and thumbnail cache.",
        badge: SafetyBadge::AppOpen,
        process_names: &["firefox.exe", "firefox"],
        min_age_days: 0,
        paths: ff_paths,
        sqlite_globs: Vec::new(),
        cookie_globs: Vec::new(),
    });

    // 4. Applications (VS Code, Discord, Spotify)
    let mut vscode_paths = Vec::new();
    if cfg!(windows) {
        vscode_paths.push(roaming_app.join("Code").join("Cache"));
        vscode_paths.push(roaming_app.join("Code").join("CachedData"));
        vscode_paths.push(roaming_app.join("Code").join("logs"));
    } else if cfg!(target_os = "macos") {
        vscode_paths.push(home.join("Library/Application Support/Code/Cache"));
        vscode_paths.push(home.join("Library/Application Support/Code/CachedData"));
    } else {
        vscode_paths.push(xdg_config.join("Code/Cache"));
        vscode_paths.push(xdg_config.join("Code/CachedData"));
    }
    rules.push(RuleSpec {
        id: "app-vscode",
        name: "VS Code Cache & Logs",
        category: "Applications",
        description: "Editor cached data and old extension host logs.",
        badge: SafetyBadge::AppOpen,
        process_names: &["code.exe", "code"],
        min_age_days: 0,
        paths: vscode_paths,
        sqlite_globs: Vec::new(),
        cookie_globs: Vec::new(),
    });

    let mut discord_paths = Vec::new();
    if cfg!(windows) {
        discord_paths.push(roaming_app.join("discord").join("Cache"));
        discord_paths.push(roaming_app.join("discord").join("Code Cache"));
    } else if cfg!(target_os = "macos") {
        discord_paths.push(home.join("Library/Application Support/discord/Cache"));
    } else {
        discord_paths.push(xdg_config.join("discord/Cache"));
    }
    rules.push(RuleSpec {
        id: "app-discord",
        name: "Discord Cache",
        category: "Applications",
        description: "Cached images, stickers, and media shards.",
        badge: SafetyBadge::AppOpen,
        process_names: &["discord.exe", "discord"],
        min_age_days: 0,
        paths: discord_paths,
        sqlite_globs: Vec::new(),
        cookie_globs: Vec::new(),
    });

    rules
}

fn scan_rule(
    spec: &RuleSpec,
    home: &Path,
    procs: &HashMap<String, String>,
) -> (CleanerRuleSummary, Vec<CleanerFileItem>) {
    let cutoff = if spec.min_age_days > 0 {
        SystemTime::now()
            .checked_sub(Duration::from_secs(spec.min_age_days * 24 * 60 * 60))
            .unwrap_or(UNIX_EPOCH)
    } else {
        SystemTime::now()
    };

    let running_process = spec
        .process_names
        .iter()
        .find_map(|p| procs.get(&p.to_lowercase()).cloned());
    let app_running = running_process.is_some();

    let mut items = Vec::new();
    let mut total_bytes = 0u64;

    for root in &spec.paths {
        if !root.exists() {
            continue;
        }
        if root.is_file() {
            if let Ok(meta) = fs::symlink_metadata(root)
                && !meta.file_type().is_symlink()
                && owned_by_current_user(&meta, home)
            {
                let modified = meta.modified().unwrap_or(UNIX_EPOCH);
                if spec.min_age_days == 0 || modified <= cutoff {
                    let bytes = meta.len();
                    total_bytes = total_bytes.saturating_add(bytes);
                    items.push(CleanerFileItem {
                        name: root
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        path: root.to_string_lossy().into_owned(),
                        bytes,
                        modified: modified
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0),
                    });
                }
            }
            continue;
        }

        let Ok(dir) = fs::read_dir(root) else {
            continue;
        };
        for entry in dir.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if cfg!(target_os = "macos")
                && (name.starts_with("com.apple") || name == "TemporaryItems")
            {
                continue;
            }
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() || !owned_by_current_user(&meta, home) {
                continue;
            }
            let mut bytes = 0u64;
            let mut newest = meta.modified().unwrap_or(UNIX_EPOCH);
            if meta.is_file() {
                bytes = meta.len();
            } else if meta.is_dir() {
                let mut count = 0u64;
                for child in WalkDir::new(&path)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(Result::ok)
                {
                    if child.file_type().is_file()
                        && let Ok(cm) = child.metadata()
                    {
                        bytes = bytes.saturating_add(cm.len());
                        count += 1;
                        if let Ok(t) = cm.modified() {
                            newest = newest.max(t);
                        }
                    }
                }
                if count == 0 {
                    continue;
                }
            } else {
                continue;
            }
            if spec.min_age_days > 0 && newest > cutoff {
                continue;
            }
            total_bytes = total_bytes.saturating_add(bytes);
            items.push(CleanerFileItem {
                name,
                path: path.to_string_lossy().into_owned(),
                bytes,
                modified: newest
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            });
        }
    }

    items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
    let sqlite_targets = spec
        .sqlite_globs
        .iter()
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let cookie_dbs = spec
        .cookie_globs
        .iter()
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    let summary = CleanerRuleSummary {
        id: spec.id.to_string(),
        name: spec.name.to_string(),
        category: spec.category.to_string(),
        description: spec.description.to_string(),
        badge: if app_running {
            SafetyBadge::AppOpen
        } else {
            spec.badge
        },
        app_running,
        running_process,
        total_bytes,
        item_count: items.len(),
        sqlite_targets,
        cookie_dbs,
    };
    (summary, items)
}

pub fn scan_deep_cleanup(
    home: &Path,
    session: Option<&CleanerSession>,
    mut on_rule: impl FnMut(&CleanerRuleSummary) + Send,
) -> Vec<CleanerRuleSummary> {
    let catalog = build_catalog(home);
    let procs = running_processes();
    let results: Vec<(CleanerRuleSummary, Vec<CleanerFileItem>)> = catalog
        .par_iter()
        .map(|spec| scan_rule(spec, home, &procs))
        .collect();

    let mut summaries = Vec::with_capacity(results.len());
    let mut items_map = HashMap::with_capacity(results.len());
    for (summary, items) in results {
        on_rule(&summary);
        items_map.insert(summary.id.clone(), items);
        summaries.push(summary);
    }
    summaries.sort_by_key(|s| std::cmp::Reverse(s.total_bytes));
    if let Some(sess) = session {
        sess.store(summaries.clone(), items_map);
    }
    summaries
}

/// Empties the OS Recycle Bin / Trash after explicit user confirmation.
pub fn empty_os_trash() -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::Shell::{
            SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW,
        };
        let hr = SHEmptyRecycleBinW(
            std::ptr::null_mut(),
            std::ptr::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        );
        // S_OK (0) or E_UNEXPECTED (0x8000FFFF when already empty)
        if hr == 0 || hr as u32 == 0x8000_FFFF {
            Ok(())
        } else {
            Err(format!("SHEmptyRecycleBinW failed (0x{:08x})", hr as u32))
        }
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME not set")?;
        let trash = home.join(".Trash");
        if trash.is_dir() {
            for entry in fs::read_dir(&trash).map_err(|e| e.to_string())?.flatten() {
                let p = entry.path();
                if p.is_dir() && !p.is_symlink() {
                    fs::remove_dir_all(&p).map_err(|e| e.to_string())?;
                } else {
                    fs::remove_file(&p).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let data = env_path("XDG_DATA_HOME").unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".local/share")
        });
        let trash = data.join("Trash");
        for sub in ["files", "info"] {
            let dir = trash.join(sub);
            if dir.is_dir() {
                for entry in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
                    let p = entry.path();
                    if p.is_dir() && !p.is_symlink() {
                        fs::remove_dir_all(&p).map_err(|e| e.to_string())?;
                    } else {
                        fs::remove_file(&p).map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_cleanup_scan_populates_session_safely() {
        let dir = std::env::temp_dir().join(format!("tidybeam-cleaner-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let sess = CleanerSession::default();
        let summaries = scan_deep_cleanup(&dir, Some(&sess), |_| {});
        assert!(!summaries.is_empty());
        assert!(sess.cached_summaries().is_some());
        let _ = fs::remove_dir_all(&dir);
    }
}
