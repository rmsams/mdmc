use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::frontmatter;

/// A note entry: a draft or snapshot, with a display label and its path.
struct Entry {
    path: PathBuf,
    name: String,
    status: Option<String>,
    date: Option<String>,
    from: Option<String>,
    message_id: Option<String>,
}

/// Maximum widths for the sender and title columns of the snapshot picker.
const SENDER_MAX: usize = 28;
const TITLE_MAX: usize = 48;

/// Truncate `s` to at most `max` characters, appending `…` when it overflows.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max - 1).collect();
    t.push('…');
    t
}

impl Entry {
    fn label(&self) -> String {
        if let Some(status) = &self.status {
            return format!("{status:<7}  {}", self.name);
        }
        let Some(date) = &self.date else {
            return self.name.clone();
        };
        let sender = truncate(self.from.as_deref().unwrap_or(""), SENDER_MAX);
        let title = truncate(&self.name, TITLE_MAX);
        format!("{date} | {sender:<SENDER_MAX$} | {title}")
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
        let (fm, json) = std::fs::read_to_string(&path)
            .ok()
            .map(|text| {
                let (draft, json, _) = frontmatter::load(&text);
                (draft, json)
            })
            .unwrap_or((None, None));
        if snapshots {
            let name = fm
                .as_ref()
                .and_then(|f| f.title.clone())
                .unwrap_or_else(|| slug.clone());
            let date = fm.as_ref().and_then(|f| f.date.clone());
            let from = json
                .as_ref()
                .and_then(|j| j.get("from"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let message_id = json
                .as_ref()
                .and_then(|j| j.get("message-id"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            entries.push(Entry {
                path,
                name,
                status: None,
                date,
                from,
                message_id,
            });
        } else {
            let status = fm
                .as_ref()
                .and_then(|f| f.status.clone())
                .unwrap_or_else(|| "(none)".to_string());
            entries.push(Entry {
                path,
                name: slug,
                status: Some(status),
                date: None,
                from: None,
                message_id: None,
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

/// Run the picker over `entries`, returning the chosen entries.
fn select(
    prompt: &str,
    entries: Vec<Entry>,
    multi: bool,
    page_size: usize,
) -> Result<Vec<Entry>, String> {
    if multi {
        pick_many(prompt, entries, page_size)
    } else {
        pick_one(prompt, entries, page_size)
    }
}

/// Pick a single draft interactively and return its path (for `mdmc edit`).
pub fn pick_draft_path(page_size: usize, cfg: &Config) -> Result<PathBuf, String> {
    let dir = cfg.paths_source()?;
    let entries = collect_entries(&dir, false);
    let chosen = pick_one("draft", entries, page_size)?;
    let entry = chosen
        .into_iter()
        .next()
        .ok_or_else(|| "no draft selected".to_string())?;
    Ok(entry.path)
}

/// `pick drafts`: pick a draft and print its path.
pub fn pick_drafts(multi: bool, page_size: usize, cfg: &Config) -> Result<(), String> {
    let dir = cfg.paths_source()?;
    let entries = collect_entries(&dir, false);
    let chosen = select("draft", entries, multi, page_size)?;
    print_values(chosen.into_iter().map(|e| e.path.display().to_string()))
}

/// `pick snapshots`: pick a snapshot and print its path.
pub fn pick_snapshots(multi: bool, page_size: usize, cfg: &Config) -> Result<(), String> {
    let dir = cfg.paths_snapshots()?;
    let entries = collect_entries(&dir, true);
    let chosen = select("snapshot", entries, multi, page_size)?;
    print_values(chosen.into_iter().map(|e| e.path.display().to_string()))
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
        return println_json(&out);
    }
    print_values(entries.iter().map(|e| e.path.display().to_string()))
}

fn pick_one<T: fmt::Display>(
    prompt: &str,
    options: Vec<T>,
    page_size: usize,
) -> Result<Vec<T>, String> {
    if options.is_empty() {
        return Err("no matches".to_string());
    }
    let selected = inquire::Select::new(prompt, options)
        .with_page_size(page_size)
        .raw_prompt()
        .map(|op| op.value)
        .map_err(|e| format!("picker failed: {e}"))?;
    Ok(vec![selected])
}

fn pick_many<T: fmt::Display>(
    prompt: &str,
    options: Vec<T>,
    page_size: usize,
) -> Result<Vec<T>, String> {
    if options.is_empty() {
        return Err("no matches".to_string());
    }
    let selected = inquire::MultiSelect::new(prompt, options)
        .with_page_size(page_size)
        .raw_prompt()
        .map(|ops| ops.into_iter().map(|op| op.value).collect())
        .map_err(|e| format!("picker failed: {e}"))?;
    Ok(selected)
}

fn print_values(values: impl Iterator<Item = String>) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    for value in values {
        writeln!(out, "{value}").map_err(|e| format!("write failed: {e}"))?;
    }
    Ok(())
}

fn println_json(value: &[serde_json::Value]) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string(value).map_err(|e| format!("json: {e}"))?
    );
    Ok(())
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
                    "message-id": e.message_id,
                    "path": e.path.display().to_string(),
                })
            })
            .collect();
        return println_json(&out);
    }
    print_values(entries.iter().map(|e| e.path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_leaves_short_strings_alone() {
        assert_eq!(truncate("hi", 10), "hi");
        assert_eq!(truncate("0123456789", 10), "0123456789");
    }

    #[test]
    fn truncate_ellipsizes_long_strings() {
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn snapshot_label_pads_and_truncates_columns() {
        let e = Entry {
            path: PathBuf::from("x.md"),
            name: "A very long title that should be cut down to fit the column width".to_string(),
            status: None,
            date: Some("2026-11-11T12:00:00+00:00".to_string()),
            from: Some("Ann <ann@example.com>".to_string()),
            message_id: Some("m1@x".to_string()),
        };
        let label = e.label();
        assert!(label.starts_with("2026-11-11T12:00:00+00:00 | "));
        assert!(label.contains("Ann <ann@example.com>"));
        assert!(label.ends_with('…'));
    }
}
