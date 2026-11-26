//! Local-only CleanerML XML and winapp2.ini rule parser + custom cleaner store.

use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportedCleanerRule {
    pub name: String,
    pub section: String,
    pub source_format: String, // "winapp2" | "cleanerml" | "custom"
    pub os_compat: Vec<String>,
    pub targets: Vec<String>,
}

/// Parses a local `winapp2.ini` file into cross-OS-tagged `ImportedCleanerRule` entries.
pub fn parse_winapp2_ini(content: &str) -> Vec<ImportedCleanerRule> {
    let mut rules = Vec::new();
    let mut current_name = String::new();
    let mut current_section = String::from("Custom");
    let mut current_targets = Vec::new();

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if !current_name.is_empty() && !current_targets.is_empty() {
                rules.push(ImportedCleanerRule {
                    name: current_name.clone(),
                    section: current_section.clone(),
                    source_format: "winapp2".into(),
                    os_compat: vec!["windows".into()],
                    targets: std::mem::take(&mut current_targets),
                });
            }
            current_name = line[1..line.len() - 1].trim().to_string();
            current_section = "Custom".into();
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            let val = v.trim();
            if key.eq_ignore_ascii_case("Section") {
                current_section = val.to_string();
            } else if key.to_ascii_lowercase().starts_with("filekey") {
                if let Some((path_part, _)) = val.split_once('|') {
                    current_targets.push(path_part.trim().to_string());
                } else {
                    current_targets.push(val.to_string());
                }
            }
        }
    }
    if !current_name.is_empty() && !current_targets.is_empty() {
        rules.push(ImportedCleanerRule {
            name: current_name,
            section: current_section,
            source_format: "winapp2".into(),
            os_compat: vec!["windows".into()],
            targets: current_targets,
        });
    }
    rules
}

/// Parses a local CleanerML XML definition into `ImportedCleanerRule` entries.
pub fn parse_cleanerml(content: &str) -> Vec<ImportedCleanerRule> {
    let mut rules = Vec::new();
    let mut cleaner_name = String::from("CleanerML Rule");
    if let Some(start) = content.find("<label>")
        && let Some(end) = content[start + 7..].find("</label>")
    {
        cleaner_name = content[start + 7..start + 7 + end].trim().to_string();
    }
    let mut targets = Vec::new();
    let mut rest = content;
    while let Some(idx) = rest.find("path=\"") {
        let after = &rest[idx + 6..];
        if let Some(end) = after.find('"') {
            targets.push(after[..end].to_string());
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    if !targets.is_empty() {
        rules.push(ImportedCleanerRule {
            name: cleaner_name,
            section: "CleanerML".into(),
            source_format: "cleanerml".into(),
            os_compat: vec!["windows".into(), "macos".into(), "linux".into()],
            targets,
        });
    }
    rules
}

pub fn import_local_rules_file(path: &Path) -> Result<Vec<ImportedCleanerRule>, String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if ext == "xml" || content.trim_start().starts_with('<') {
        Ok(parse_cleanerml(&content))
    } else {
        Ok(parse_winapp2_ini(&content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_winapp2_and_cleanerml() {
        let ini =
            "[Demo Cache *]\nSection=Applications\nFileKey1=%LocalAppData%\\Demo\\Cache|*.*\n";
        let w = parse_winapp2_ini(ini);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].name, "Demo Cache *");
        assert_eq!(w[0].targets[0], "%LocalAppData%\\Demo\\Cache");

        let xml = r#"<cleaner><label>My App</label><option><action command="delete" search="glob" path="~/.cache/myapp/*"/></option></cleaner>"#;
        let c = parse_cleanerml(xml);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "My App");
        assert_eq!(c[0].targets[0], "~/.cache/myapp/*");
    }
}
