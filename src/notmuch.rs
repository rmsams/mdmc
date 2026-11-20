use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The notmuch binary name.
pub(crate) fn binary() -> &'static str {
    "notmuch"
}

/// Resolve a Message-ID (angle brackets optional) to a file path via notmuch's
/// `mid:` term. Returns `None` when notmuch is unavailable, fails, or the id is
/// not indexed.
pub fn resolve_message_id(id: &str) -> Option<PathBuf> {
    resolve_message_id_with(binary(), id)
}

fn resolve_message_id_with(bin: &str, id: &str) -> Option<PathBuf> {
    let out = std::process::Command::new(bin)
        .args([
            "search",
            "--output=files",
            "--limit=1",
            &format!("mid:{id}"),
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
}

/// Resolve a Message-ID, falling back to a full maildir scan when notmuch has
/// no answer.
pub fn resolve(id: &str, mail_root: Option<&Path>) -> Option<PathBuf> {
    resolve_message_id(id)
        .or_else(|| mail_root.and_then(|root| crate::maildir::find_message_in_tree(root, id)))
}

/// A message thread from `notmuch search --format=json`.
#[derive(Deserialize)]
pub struct SearchThread {
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub authors: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub query: Vec<Option<String>>,
}

/// An address from `notmuch address --format=json`.
#[derive(Deserialize)]
pub struct AddressEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub address: String,
}

/// Search messages with notmuch, returning the parsed threads.
pub fn search(query: &str, limit: Option<usize>) -> Result<Vec<SearchThread>, String> {
    search_with(binary(), query, limit)
}

fn search_with(bin: &str, query: &str, limit: Option<usize>) -> Result<Vec<SearchThread>, String> {
    let mut args = vec!["search".to_string(), "--format=json".to_string()];
    if let Some(n) = limit {
        args.push(format!("--limit={n}"));
    }
    args.push(query.to_string());
    let out = run(bin, &args)?;
    serde_json::from_str(&out).map_err(|e| format!("cannot parse notmuch search output: {e}"))
}

/// Look up addresses with notmuch, returning the parsed entries.
pub fn address(query: &str) -> Result<Vec<AddressEntry>, String> {
    address_with(binary(), query)
}

fn address_with(bin: &str, query: &str) -> Result<Vec<AddressEntry>, String> {
    let args = vec![
        "address".to_string(),
        "--format=json".to_string(),
        "--deduplicate=address".to_string(),
        "--output=recipients".to_string(),
        query.to_string(),
    ];
    let out = run(bin, &args)?;
    serde_json::from_str(&out).map_err(|e| format!("cannot parse notmuch address output: {e}"))
}

/// Run a notmuch subcommand, returning its stdout (stderr on failure).
fn run(bin: &str, args: &[String]) -> Result<String, String> {
    let out = std::process::Command::new(bin)
        .args(args)
        .output()
        .map_err(|e| format!("{bin} failed to start: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{bin} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake notmuch that answers `search --output=files --limit=1 mid:X`.
    #[test]
    fn resolve_message_id_parses_first_path() {
        let dir = std::env::temp_dir().join(format!("mdmc-notmuch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("notmuch");
        std::fs::write(
            &script,
            "#!/bin/sh\nif [ \"$1\" = search ] && [ \"$2\" = --output=files ]; then echo /mail/cur/msg1; fi\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        assert_eq!(
            resolve_message_id_with(script.to_str().unwrap(), "abc@x"),
            Some(PathBuf::from("/mail/cur/msg1"))
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    fn fake_notmuch(dir: &Path, body: &str) -> std::path::PathBuf {
        let script = dir.join("notmuch");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        script
    }

    #[test]
    fn search_parses_threads() {
        let dir = std::env::temp_dir().join(format!("mdmc-notmuch-s-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = fake_notmuch(
            &dir,
            "echo '[{\"thread\":\"t1\",\"timestamp\":0,\"authors\":\"Ann <ann@x>\",\"subject\":\"Hi\",\"query\":[\"id:m1@x\"]}]'",
        );
        let hits = search_with(script.to_str().unwrap(), "from:ann", None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].query, vec![Some("id:m1@x".to_string())]);
        assert_eq!(hits[0].authors, "Ann <ann@x>");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn address_parses_entries() {
        let dir = std::env::temp_dir().join(format!("mdmc-notmuch-a-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = fake_notmuch(&dir, "echo '[{\"name\":\"Ann\",\"address\":\"ann@x\"}]'");
        let entries = address_with(script.to_str().unwrap(), "from:me").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Ann");
        assert_eq!(entries[0].address, "ann@x");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn address_requests_recipients() {
        let dir = std::env::temp_dir().join(format!("mdmc-notmuch-r-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = fake_notmuch(
            &dir,
            "case \" $* \" in *\" --output=recipients \"*) echo '[{\"name\":\"Ann\",\"address\":\"ann@x\"}]';; *) echo 'missing --output=recipients'; exit 1;; esac",
        );
        let entries = address_with(script.to_str().unwrap(), "from:me").unwrap();
        assert_eq!(entries.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
