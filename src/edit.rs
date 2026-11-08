use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::frontmatter;

/// Open a draft in `$EDITOR`. With an input, resolve a slug, path, or
/// Message-ID to a draft; with no input, present an interactive picker over the
/// source directory (sent drafts excluded unless `--all`). Never sends.
pub fn edit(input: Option<&str>, all: bool, cfg: &Config) -> Result<(), String> {
    let source_dir = cfg.paths_source()?;
    let path = match input {
        Some(arg) => resolve_draft(arg, &source_dir)?,
        None => pick_draft(&source_dir, all)?,
    };
    crate::new::run_editor(&path);
    Ok(())
}

/// Resolve an input to a draft path: an existing file (possibly `~`-expanded,
/// relative to the source directory, or with `.md` appended) wins; otherwise
/// the input is treated as a Message-ID and matched against drafts' frontmatter.
fn resolve_draft(arg: &str, source_dir: &Path) -> Result<PathBuf, String> {
    let mut path = crate::config::expand_home(arg);
    if !path.is_absolute() {
        path = source_dir.join(arg);
    }
    if path.extension().and_then(|e| e.to_str()) != Some("md") {
        path.set_extension("md");
    }
    if path.is_file() {
        return Ok(path);
    }
    if let Some(found) = find_by_message_id(source_dir, arg) {
        return Ok(found);
    }
    Err(format!("no draft found for '{arg}'"))
}

fn find_by_message_id(source_dir: &Path, message_id: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(source_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        if let (Some(fm), _, _) = frontmatter::load(&text) {
            if fm.message_id.as_deref() == Some(message_id) {
                return Some(path);
            }
        }
    }
    None
}

/// Present an interactive picker over the source directory, returning the
/// selected draft path.
fn pick_draft(source_dir: &Path, all: bool) -> Result<PathBuf, String> {
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    if let Ok(items) = std::fs::read_dir(source_dir) {
        for item in items.flatten() {
            let path = item.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let slug = path.file_stem().unwrap().to_string_lossy().into_owned();
            let status = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| frontmatter::load(&text).0)
                .and_then(|fm| fm.status)
                .unwrap_or_else(|| "(none)".to_string());
            if !all && status == "sent" {
                continue;
            }
            entries.push((format!("{status:<7}  {slug}"), path));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if entries.is_empty() {
        return Err("no drafts to edit".to_string());
    }

    let options: Vec<String> = entries.iter().map(|(label, _)| label.clone()).collect();
    let choice = inquire::Select::new("edit draft", options)
        .prompt()
        .map_err(|e| format!("picker failed: {e}"))?;

    entries
        .into_iter()
        .find(|(label, _)| label == &choice)
        .map(|(_, path)| path)
        .ok_or_else(|| "picker returned an unknown draft".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(path: &Path, slug: &str, message_id: Option<&str>, status: &str) {
        let mid = message_id
            .map(|m| format!("message-id: {m}\n"))
            .unwrap_or_default();
        std::fs::write(
            path.join(format!("{slug}.md")),
            format!("---\ntitle: {slug}\ntype: mail-draft\nstatus: {status}\n{mid}---\n\nbody\n"),
        )
        .unwrap();
    }

    #[test]
    fn resolve_draft_matches_slug_and_message_id() {
        let dir = std::env::temp_dir().join(format!("mdmc-edit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        draft(&dir, "hello", Some("abc@x"), "draft");

        assert_eq!(resolve_draft("hello", &dir).unwrap(), dir.join("hello.md"));
        assert_eq!(resolve_draft("abc@x", &dir).unwrap(), dir.join("hello.md"));
        assert!(resolve_draft("nope", &dir).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_by_message_id_skips_missing() {
        let dir = std::env::temp_dir().join(format!("mdmc-edit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        draft(&dir, "a", Some("aaa@x"), "draft");

        assert!(find_by_message_id(&dir, "aaa@x").is_some());
        assert!(find_by_message_id(&dir, "bbb@x").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
