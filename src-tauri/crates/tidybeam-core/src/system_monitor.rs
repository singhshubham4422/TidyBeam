//! Cross-OS live CPU, RAM, Network, Temperature, and Process monitoring via sysinfo.

use serde::Serialize;
use std::sync::Mutex;
use sysinfo::{Components, Disks, Networks, Pid, ProcessesToUpdate, System};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CpuCoreInfo {
    pub name: String,
    pub usage: f32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NetIfaceInfo {
    pub name: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TempSensorInfo {
    pub label: String,
    pub celsius: f32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SystemSnapshot {
    pub os_name: String,
    pub kernel_version: String,
    pub host_name: String,
    pub cpu_brand: String,
    pub cpu_usage: f32,
    pub core_count: usize,
    pub cores: Vec<CpuCoreInfo>,
    pub mem_total: u64,
    pub mem_used: u64,
    pub mem_available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub uptime_secs: u64,
    pub networks: Vec<NetIfaceInfo>,
    pub temperatures: Vec<TempSensorInfo>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub status: String,
    pub exe_path: String,
    pub protected: bool,
}

pub struct MonitorState {
    sys: Mutex<System>,
    networks: Mutex<Networks>,
    components: Mutex<Components>,
}

impl Default for MonitorState {
    fn default() -> Self {
        let mut sys = System::new_all();
        sys.refresh_cpu_usage();
        Self {
            sys: Mutex::new(sys),
            networks: Mutex::new(Networks::new_with_refreshed_list()),
            components: Mutex::new(Components::new_with_refreshed_list()),
        }
    }
}

const PROTECTED_PROCS: &[&str] = &[
    "system",
    "system idle process",
    "registry",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "services.exe",
    "lsass.exe",
    "svchost.exe",
    "winlogon.exe",
    "dwm.exe",
    "launchd",
    "kernel_task",
    "windowserver",
    "loginwindow",
    "systemd",
    "init",
    "kthreadd",
    "dbus-daemon",
];

pub fn is_protected_process(pid: u32, name: &str) -> bool {
    if pid <= 4 {
        return true;
    }
    let lower = name.to_lowercase();
    PROTECTED_PROCS.contains(&lower.as_str())
}

impl MonitorState {
    pub fn snapshot(&self) -> SystemSnapshot {
        let mut sys = self.sys.lock().expect("monitor lock");
        sys.refresh_cpu_all();
        sys.refresh_memory();

        let cores: Vec<CpuCoreInfo> = sys
            .cpus()
            .iter()
            .map(|c| CpuCoreInfo {
                name: c.name().to_string(),
                usage: c.cpu_usage(),
            })
            .collect();
        let cpu_brand = sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .unwrap_or_default();

        let mut nets = self.networks.lock().expect("networks lock");
        nets.refresh(true);
        let networks = nets
            .iter()
            .map(|(name, data)| NetIfaceInfo {
                name: name.clone(),
                received_bytes: data.received(),
                transmitted_bytes: data.transmitted(),
            })
            .collect();

        let mut comps = self.components.lock().expect("components lock");
        comps.refresh(true);
        let temperatures = comps
            .iter()
            .filter_map(|c| {
                c.temperature().map(|t| TempSensorInfo {
                    label: c.label().to_string(),
                    celsius: t,
                })
            })
            .collect();

        SystemSnapshot {
            os_name: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into()),
            kernel_version: System::kernel_version().unwrap_or_default(),
            host_name: System::host_name().unwrap_or_default(),
            cpu_brand,
            cpu_usage: sys.global_cpu_usage(),
            core_count: cores.len(),
            cores,
            mem_total: sys.total_memory(),
            mem_used: sys.used_memory(),
            mem_available: sys.available_memory(),
            swap_total: sys.total_swap(),
            swap_used: sys.used_swap(),
            uptime_secs: System::uptime(),
            networks,
            temperatures,
        }
    }

    pub fn processes(&self) -> Vec<ProcessEntry> {
        let mut sys = self.sys.lock().expect("monitor lock");
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let mut list: Vec<ProcessEntry> = sys
            .processes()
            .iter()
            .map(|(pid, p)| {
                let name = p.name().to_string_lossy().to_string();
                let pid_u32 = pid.as_u32();
                ProcessEntry {
                    pid: pid_u32,
                    parent_pid: p.parent().map(|pp| pp.as_u32()),
                    protected: is_protected_process(pid_u32, &name),
                    name,
                    cpu_usage: p.cpu_usage(),
                    memory_bytes: p.memory(),
                    status: format!("{:?}", p.status()),
                    exe_path: p
                        .exe()
                        .map(|e| e.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                }
            })
            .collect();
        list.sort_by(|a, b| {
            b.memory_bytes
                .cmp(&a.memory_bytes)
                .then_with(|| b.cpu_usage.total_cmp(&a.cpu_usage))
        });
        list
    }

    pub fn terminate_process(&self, pid: u32) -> Result<(), String> {
        let mut sys = self.sys.lock().expect("monitor lock");
        sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), false);
        let Some(proc_) = sys.process(Pid::from_u32(pid)) else {
            return Err("Process is no longer running".into());
        };
        let name = proc_.name().to_string_lossy().to_string();
        if is_protected_process(pid, &name) {
            return Err(format!("“{name}” is a protected system process"));
        }
        if proc_.kill() {
            Ok(())
        } else {
            Err(format!("Couldn't terminate “{name}” (permission denied)"))
        }
    }
}

pub fn high_usage_disks() -> Vec<(String, u8)> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter_map(|d| {
            let total = d.total_space();
            if total == 0 {
                return None;
            }
            let used = total.saturating_sub(d.available_space());
            let pct = ((used as f64 / total as f64) * 100.0).round() as u8;
            if pct >= 85 {
                Some((d.mount_point().to_string_lossy().into_owned(), pct))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_process_guard_blocks_system_pids() {
        assert!(is_protected_process(0, "System Idle Process"));
        assert!(is_protected_process(4, "System"));
        assert!(is_protected_process(120, "lsass.exe"));
        assert!(is_protected_process(1, "systemd"));
        assert!(is_protected_process(1, "launchd"));
        assert!(!is_protected_process(4242, "notepad.exe"));
    }

    #[test]
    fn monitor_snapshot_reads_memory_and_cpus() {
        let state = MonitorState::default();
        let snap = state.snapshot();
        assert!(snap.mem_total > 0);
        assert!(snap.core_count > 0);
    }
}
