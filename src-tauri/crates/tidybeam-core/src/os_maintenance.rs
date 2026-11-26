//! Cross-OS Disk Health overview and standard-user OS Settings deep-links.

use serde::Serialize;
use sysinfo::{DiskKind, Disks};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiskHealthInfo {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub kind: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_pct: u8,
    pub removable: bool,
    pub status: String,
}

pub fn list_disk_health() -> Vec<DiskHealthInfo> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| d.total_space() > 0)
        .map(|d| {
            let total = d.total_space();
            let avail = d.available_space();
            let used = total.saturating_sub(avail);
            let used_pct = ((used as f64 / total as f64) * 100.0).round() as u8;
            let kind = match d.kind() {
                DiskKind::SSD => "SSD",
                DiskKind::HDD => "HDD",
                _ => "Drive",
            }
            .to_string();
            let status = if used_pct >= 90 {
                "Low Space"
            } else if used_pct >= 80 {
                "Review"
            } else {
                "Healthy"
            }
            .to_string();
            DiskHealthInfo {
                name: d.name().to_string_lossy().into_owned(),
                mount_point: d.mount_point().to_string_lossy().into_owned(),
                file_system: d.file_system().to_string_lossy().into_owned(),
                kind,
                total_bytes: total,
                available_bytes: avail,
                used_pct,
                removable: d.is_removable(),
                status,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_health_reports_mounted_drives() {
        let disks = list_disk_health();
        assert!(!disks.is_empty());
    }
}
