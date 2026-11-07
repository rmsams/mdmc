use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Account {
    pub from: String,
    pub realname: String,
}

/// Parse `set key='value'` lines from a neomutt account file.
pub fn parse_neomutt(path: &Path) -> BTreeMap<String, String> {
    let mut cfg = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return cfg;
    };
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("set ") {
            continue;
        }
        let rest = line[4..].trim();
        let Some((key, val)) = rest.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        let mut val = val.trim();
        if val.len() >= 2 {
            let first = val.chars().next().unwrap();
            let last = val.chars().last().unwrap();
            if first == last && (first == '\'' || first == '"') {
                val = &val[1..val.len() - 1];
            }
        }
        cfg.insert(key, val.to_string());
    }
    cfg
}

pub fn list_account_names(neomutt_dir: &Path) -> Vec<String> {
    let skip = ["neomuttrc", "startup-push.rc"];
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(neomutt_dir) else {
        return names;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.starts_with("context-") || skip.contains(&name.as_str()) {
            continue;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let cfg = parse_neomutt(&path);
        if cfg.contains_key("from") && cfg.contains_key("folder") {
            names.push(name);
        }
    }
    names.sort();
    names
}

pub fn resolve_account(name: &str, neomutt_dir: &Path) -> Result<Account, String> {
    let path = neomutt_dir.join(name);
    if !path.exists() {
        let available = list_account_names(neomutt_dir).join(", ");
        return Err(format!(
            "account '{name}' not found in {}. Available: {}",
            neomutt_dir.display(),
            if available.is_empty() {
                "(none)"
            } else {
                &available
            }
        ));
    }
    let cfg = parse_neomutt(&path);
    let from = cfg.get("from").cloned();
    let folder = cfg.get("folder").cloned();
    let (Some(from), Some(_folder)) = (from, folder) else {
        return Err(format!("account '{name}' missing 'from'/'folder' settings"));
    };

    Ok(Account {
        from,
        realname: cfg.get("realname").cloned().unwrap_or_default(),
    })
}
