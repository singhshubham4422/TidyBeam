//! One-Click Health Check & Safe Boost combining junk, startup, memory, and disk checks.

use crate::{cleaner, os_adapters, system_monitor};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HealthPillar {
    pub id: String,
    pub title: String,
    pub status: String, // "good" | "review" | "attention"
    pub summary: String,
    pub reclaimable_bytes: u64,
    pub fixable: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub score: u8,
    pub total_reclaimable: u64,
    pub pillars: Vec<HealthPillar>,
}

pub fn run_health_check(
    home: &Path,
    monitor: &system_monitor::MonitorState,
    cleaner_session: &cleaner::CleanerSession,
) -> HealthReport {
    let summaries = cleaner_session
        .cached_summaries()
        .unwrap_or_else(|| cleaner::scan_deep_cleanup(home, Some(cleaner_session), |_| {}));
    let safe_junk_bytes: u64 = summaries
        .iter()
        .filter(|r| !r.app_running && r.badge == cleaner::SafetyBadge::Safe)
        .map(|r| r.total_bytes)
        .sum();
    let total_junk_bytes: u64 = summaries.iter().map(|r| r.total_bytes).sum();

    let startup = os_adapters::list_startup_items(home);
    let enabled_startup = startup.iter().filter(|s| s.enabled).count();

    let snap = monitor.snapshot();
    let mem_pct = if snap.mem_total > 0 {
        ((snap.mem_used as f64 / snap.mem_total as f64) * 100.0).round() as u8
    } else {
        0
    };

    let full_disks = system_monitor::high_usage_disks();

    let mut score = 100i32;
    if total_junk_bytes > 500 * 1024 * 1024 {
        score -= 15;
    } else if total_junk_bytes > 100 * 1024 * 1024 {
        score -= 8;
    }
    if enabled_startup > 8 {
        score -= 10;
    }
    if mem_pct >= 85 {
        score -= 15;
    }
    if !full_disks.is_empty() {
        score -= 15;
    }

    let pillars = vec![
        HealthPillar {
            id: "junk".into(),
            title: "Junk & Browser Caches".into(),
            status: if total_junk_bytes > 500 * 1024 * 1024 {
                "attention"
            } else if total_junk_bytes > 50 * 1024 * 1024 {
                "review"
            } else {
                "good"
            }
            .into(),
            summary: format!(
                "{} MB of temporary files & caches can be safely reviewed",
                total_junk_bytes / (1024 * 1024)
            ),
            reclaimable_bytes: safe_junk_bytes,
            fixable: safe_junk_bytes > 0,
        },
        HealthPillar {
            id: "startup".into(),
            title: "Startup Impact".into(),
            status: if enabled_startup > 8 {
                "review"
            } else {
                "good"
            }
            .into(),
            summary: format!("{enabled_startup} enabled user startup item(s)"),
            reclaimable_bytes: 0,
            fixable: false,
        },
        HealthPillar {
            id: "memory".into(),
            title: "Memory & System Load".into(),
            status: if mem_pct >= 85 {
                "attention"
            } else if mem_pct >= 70 {
                "review"
            } else {
                "good"
            }
            .into(),
            summary: format!("RAM {mem_pct}% used · CPU {:.0}%", snap.cpu_usage),
            reclaimable_bytes: 0,
            fixable: false,
        },
        HealthPillar {
            id: "disk".into(),
            title: "Storage & Disk Space".into(),
            status: if full_disks.is_empty() {
                "good"
            } else {
                "attention"
            }
            .into(),
            summary: if full_disks.is_empty() {
                "All mounted drives have healthy free space".into()
            } else {
                format!(
                    "{} drive(s) above 85% capacity ({})",
                    full_disks.len(),
                    full_disks[0].0
                )
            },
            reclaimable_bytes: 0,
            fixable: false,
        },
        HealthPillar {
            id: "security".into(),
            title: "Privacy & OS Posture".into(),
            status: "good".into(),
            summary: "100% local-first · zero telemetry · Trash-safe defaults".into(),
            reclaimable_bytes: 0,
            fixable: false,
        },
    ];

    HealthReport {
        score: score.clamp(10, 100) as u8,
        total_reclaimable: safe_junk_bytes,
        pillars,
    }
}
