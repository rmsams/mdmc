use std::fmt;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::frontmatter;

/// A note entry: a draft or snapshot, with a display label and its path.
struct Entry {
    path: PathBuf,
    name: String,
    status: Option<String>,
    date: Option<String>,
}

impl Entry {
    fn label(&self) -> String {
        match (&self.status, &self.date) {
            (Some(status), _) => format!("{status:<7}  {}", self.name),
            (None, Some(date)) => format!("{date}  {}", self.name),
            (None, None) => self.name.clone(),
        }
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// Collect notes from `dir`: drafts (`status  slug`) or snapshots (`date
/// title`).
fn collect_entries(dir: &Path, snapshots: bool) -> Vec<Entry> {
    let mut entries = Vec::new();
    let Ok(items) = std::fs::read_dir(dir) else {
        return entries;
    };
    for item in items.flatten() {
        let path = item.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let slug = path.file_stem().unwrap().to_string_lossy().into_owned();
        let fm = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| frontmatter::load(&text).0);
        if snapshots {
            let name = fm.as_ref().and_then(|f| f.title.clone()).unwrap_or(slug);
            let date = fm.and_then(|f| f.date);
            entries.push(Entry {
                path,
                name,
                status: None,
                date,
            });
        } else {
            let status = fm
                .and_then(|f| f.status)
                .unwrap_or_else(|| "(none)".to_string());
            entries.push(Entry {
                path,
                name: slug,
                status: Some(status),
                date: None,
            });
        }
    }
    if snapshots {
        entries.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.name.cmp(&b.name)));
    } else {
        entries.sort_by_key(|a| a.label());
    }
    entries
}

fn emit_paths(entries: Vec<Entry>, lines: bool) -> Result<(), String> {
    crate::lookup::emit_values(
        entries
            .into_iter()
            .map(|e| e.path.display().to_string())
            .collect(),
        lines,
    )
}

/// `pick drafts`: pick a draft and print its path.
pub fn pick_drafts(multi: bool, lines: bool, cfg: &Config) -> Result<(), String> {
    let dir = cfg.paths_source()?;
    let entries = collect_entries(&dir, false);
    let chosen = if multi {
        crate::lookup::pick_many("draft", entries)?
    } else {
        crate::lookup::pick_one("draft", entries)?
    };
    emit_paths(chosen, lines)
}

/// `pick snapshots`: pick a snapshot and print its path.
pub fn pick_snapshots(multi: bool, lines: bool, cfg: &Config) -> Result<(), String> {
    let dir = cfg.paths_snapshots()?;
    let entries = collect_entries(&dir, true);
    let chosen = if multi {
        crate::lookup::pick_many("snapshot", entries)?
    } else {
        crate::lookup::pick_one("snapshot", entries)?
    };
    emit_paths(chosen, lines)
}

/// `list drafts`: print draft paths, or a JSON array with `--json`.
pub fn list_drafts(json: bool, cfg: &Config) -> Result<(), String> {
    let dir = cfg.paths_source()?;
    let entries = collect_entries(&dir, false);
    if json {
        let out: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "slug": e.name,
                    "status": e.status,
                    "path": e.path.display().to_string(),
                })
            })
            .collect();
        return crate::lookup::println_json(&out);
    }
    crate::lookup::print_values(entries.iter().map(|e| e.path.display().to_string()))
}

/// `list snapshots`: print snapshot paths, or a JSON array with `--json`.
pub fn list_snapshots(json: bool, cfg: &Config) -> Result<(), String> {
    let dir = cfg.paths_snapshots()?;
    let entries = collect_entries(&dir, true);
    if json {
        let out: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "title": e.name,
                    "date": e.date,
                    "path": e.path.display().to_string(),
                })
            })
            .collect();
        return crate::lookup::println_json(&out);
    }
    crate::lookup::print_values(entries.iter().map(|e| e.path.display().to_string()))
}
