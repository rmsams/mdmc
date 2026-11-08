use std::io::{IsTerminal, Write};

use crate::config::Config;
use crate::frontmatter;

/// List drafts as `status  slug` rows, colour-coding the status column
/// when stdout is a terminal.
pub fn list(cfg: &Config) -> Result<(), String> {
    let source_dir = cfg.paths_source();
    let Ok(entries) = std::fs::read_dir(&source_dir) else {
        return Ok(());
    };

    let mut rows: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let slug = path.file_stem().unwrap().to_string_lossy().into_owned();
        let status = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| frontmatter::load(&text).0)
            .and_then(|fm| fm.status)
            .unwrap_or_else(|| "(none)".to_string());
        rows.push((status, slug));
    }
    rows.sort();

    let color = std::io::stdout().is_terminal();
    let mut out = std::io::stdout().lock();
    for (status, slug) in rows {
        let (label, code) = match status.as_str() {
            "sent" => ("sent", "32"),
            "pending" => ("pending", "33"),
            "draft" => ("draft", "36"),
            other => (other, "2;37"),
        };
        let padded = format!("{label:<7}");
        let cell = if color {
            format!("\x1b[{code}m{padded}\x1b[0m")
        } else {
            padded
        };
        writeln!(out, "{cell}  {slug}").map_err(|e| format!("write failed: {e}"))?;
    }
    Ok(())
}
