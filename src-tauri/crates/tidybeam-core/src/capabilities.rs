//! Cross-OS capability discovery for Windows, macOS, and Linux,
//! respecting standard-user MSIX Store boundaries.

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub registry_clean: bool,
    pub package_manager_clean: bool,
    pub smart_disk_health: bool,
    pub temp_sensors: bool,
    pub native_scheduler: bool,
    pub empty_working_set: bool,
    pub free_space_wipe: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilities {
    pub os: &'static str,
    pub arch: &'static str,
    pub is_elevated: bool,
    pub is_packaged_store: bool,
    pub capabilities: Capabilities,
}

pub fn detect() -> RuntimeCapabilities {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let is_elevated = detect_elevated();
    let is_packaged_store = detect_packaged_store();
    RuntimeCapabilities {
        os,
        arch: std::env::consts::ARCH,
        is_elevated,
        is_packaged_store,
        capabilities: Capabilities {
            // Registry history cleaning is Windows non-Store opt-in only.
            registry_clean: cfg!(windows) && !is_packaged_store,
            package_manager_clean: cfg!(any(target_os = "linux", target_os = "macos")),
            smart_disk_health: true,
            temp_sensors: cfg!(any(target_os = "linux", target_os = "macos", windows)),
            native_scheduler: !is_packaged_store,
            empty_working_set: cfg!(windows),
            free_space_wipe: true,
        },
    }
}

fn detect_elevated() -> bool {
    #[cfg(unix)]
    unsafe {
        libc::geteuid() == 0
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn detect_packaged_store() -> bool {
    #[cfg(windows)]
    {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_lowercase().contains("windowsapps")))
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_consistent() {
        let caps = detect();
        assert!(!caps.os.is_empty());
        assert!(!caps.arch.is_empty());
        if caps.is_packaged_store {
            assert!(!caps.capabilities.registry_clean);
        }
    }
}
