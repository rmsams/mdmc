use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::frontmatter;

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

/// List drafts (`status  slug`) or snapshots (`title`). `--paths` prints file
/// paths only; `--pick` opens a multi-select picker and prints the chosen paths.
pub fn list(snapshots: bool, paths: bool, pick: bool, cfg: &Config) -> Result<(), String> {
    let dir = if snapshots {
        cfg.paths_snapshots()?
    } else {
        cfg.paths_source()?
    };
    let entries = collect_entries(&dir, snapshots);

    if pick {
        return pick_entries(&entries);
    }
    if paths {
        return print_paths(&entries);
    }
    print_rows(&entries);
    Ok(())
}

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

fn pick_entries(entries: &[Entry]) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    let options: Vec<String> = entries.iter().map(|e| e.label()).collect();
    let chosen = inquire::MultiSelect::new("select", options)
        .prompt()
        .map_err(|e| format!("picker failed: {e}"))?;
    let mut out = std::io::stdout().lock();
    for e in entries {
        if chosen.contains(&e.label()) {
            writeln!(out, "{}", e.path.display()).map_err(|e| format!("write failed: {e}"))?;
        }
    }
    Ok(())
}

fn print_paths(entries: &[Entry]) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    for e in entries {
        writeln!(out, "{}", e.path.display()).map_err(|e| format!("write failed: {e}"))?;
    }
    Ok(())
}

fn print_rows(entries: &[Entry]) {
    let color = std::io::stdout().is_terminal();
    let mut out = std::io::stdout().lock();
    for e in entries {
        let line = match (&e.status, color) {
            (Some(status), true) => {
                let code = match status.as_str() {
                    "sent" => "32",
                    "pending" => "33",
                    "draft" => "36",
                    _ => "2;37",
                };
                format!("\x1b[{code}m{status:<7}\x1b[0m  {}", e.name)
            }
            _ => e.label(),
        };
        let _ = writeln!(out, "{line}");
    }
}
