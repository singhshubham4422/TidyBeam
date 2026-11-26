//! Companion CLI for TidyBeam (`tidybeam-cli`) built on `tidybeam-core`.

use std::path::PathBuf;
use tidybeam_core::{capabilities, cleaner};

fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn print_usage() {
    eprintln!(
        "TidyBeam CLI\n\
         Usage:\n\
           tidybeam-cli capabilities [--json]\n\
           tidybeam-cli list [--json]\n\
           tidybeam-cli preview [--category <cat>] [--json]\n\
           tidybeam-cli clean [--category <cat>] [--dry-run] [--json]"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return;
    }
    let json = args.iter().any(|a| a == "--json");
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let category_filter = args
        .windows(2)
        .find(|w| w[0] == "--category")
        .map(|w| w[1].to_lowercase());

    match args[0].as_str() {
        "capabilities" => {
            let caps = capabilities::detect();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&caps).unwrap_or_default()
                );
            } else {
                println!(
                    "OS: {} ({}) | Elevated: {} | Packaged Store: {}",
                    caps.os, caps.arch, caps.is_elevated, caps.is_packaged_store
                );
            }
        }
        "list" | "preview" => {
            let session = cleaner::CleanerSession::default();
            let summaries = cleaner::scan_deep_cleanup(&home_dir(), Some(&session), |_| {});
            let filtered: Vec<_> = summaries
                .into_iter()
                .filter(|s| {
                    category_filter
                        .as_ref()
                        .is_none_or(|c| s.category.to_lowercase() == *c)
                })
                .collect();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&filtered).unwrap_or_default()
                );
            } else {
                for s in &filtered {
                    println!(
                        "[{}] {} ({}): {} items, {} bytes{}",
                        s.category,
                        s.name,
                        s.id,
                        s.item_count,
                        s.total_bytes,
                        if s.app_running { " [APP RUNNING]" } else { "" }
                    );
                }
            }
        }
        "clean" => {
            let session = cleaner::CleanerSession::default();
            let summaries = cleaner::scan_deep_cleanup(&home_dir(), Some(&session), |_| {});
            let rule_ids: Vec<String> = summaries
                .into_iter()
                .filter(|s| !s.app_running && s.item_count > 0)
                .filter(|s| {
                    category_filter
                        .as_ref()
                        .is_none_or(|c| s.category.to_lowercase() == *c)
                })
                .map(|s| s.id)
                .collect();
            let paths = session.paths_for_rules(&rule_ids);
            if dry_run {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&paths).unwrap_or_default()
                    );
                } else {
                    println!(
                        "Dry run: would move {} item(s) to Trash/Recycle Bin.",
                        paths.len()
                    );
                }
                return;
            }
            let mut trashed = 0usize;
            let mut failed = 0usize;
            let ctx = trash::TrashContext::default();
            for p in &paths {
                if ctx.delete(p).is_ok() {
                    trashed += 1;
                } else {
                    failed += 1;
                }
            }
            if json {
                println!(r#"{{"trashed":{trashed},"failed":{failed}}}"#);
            } else {
                println!("Moved {trashed} item(s) to Trash/Recycle Bin ({failed} failed).");
            }
        }
        _ => {
            print_usage();
            std::process::exit(1);
        }
    }
}
