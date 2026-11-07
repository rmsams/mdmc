use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Expand a leading `~` or `~/` to the user's home directory.
pub fn expand_home(raw: &str) -> PathBuf {
    if raw == "~" {
        return home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(raw)
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn de_string_or_list<'de, D>(d: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(Some(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    }))
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub paths: Paths,
    #[serde(default)]
    pub folders: BTreeMap<String, String>,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub accounts: BTreeMap<String, Account>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Paths {
    pub drafts: Option<String>,
    pub attachments: Option<String>,
    pub pandoc_data: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Defaults {
    pub account: Option<String>,
    pub css: Option<String>,
    pub template: Option<String>,
    #[serde(
        rename = "lua-filters",
        default,
        deserialize_with = "de_string_or_list"
    )]
    pub lua_filters: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Account {
    pub from: Option<String>,
    pub realname: Option<String>,
    pub drafts: Option<String>,
    pub signing_key: Option<String>,
}

impl Account {
    /// The `From:` header address: `realname <from>` when a realname is set,
    /// else the bare `from` address.
    pub fn sender(&self) -> String {
        let from = self.from.as_deref().unwrap_or("");
        match self
            .realname
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(name) => format!("{name} <{from}>"),
            None => from.to_string(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Config {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn paths_drafts(&self) -> PathBuf {
        expand_home(
            self.paths
                .drafts
                .as_deref()
                .unwrap_or("~/kb/ops/mail/drafts"),
        )
    }

    pub fn paths_attachments(&self) -> PathBuf {
        expand_home(
            self.paths
                .attachments
                .as_deref()
                .unwrap_or("~/kb/attachments"),
        )
    }

    pub fn paths_pandoc_data(&self) -> PathBuf {
        expand_home(
            self.paths
                .pandoc_data
                .as_deref()
                .unwrap_or("~/.local/share/pandoc"),
        )
    }

    pub fn default_account(&self) -> Option<&str> {
        self.defaults.account.as_deref()
    }

    pub fn default_css(&self) -> Option<&str> {
        self.defaults.css.as_deref()
    }

    pub fn default_template(&self) -> Option<&str> {
        self.defaults.template.as_deref()
    }

    pub fn default_lua_filters(&self) -> &[String] {
        self.defaults.lua_filters.as_deref().unwrap_or_default()
    }

    /// Resolve a `--folder` target: a `[folders]` registry name or a literal path.
    pub fn resolve_target_folder(&self, name: &str) -> PathBuf {
        let raw = self.folders.get(name).map(String::as_str).unwrap_or(name);
        expand_home(raw)
    }

    /// Look up an account by name in `[accounts]`.
    pub fn resolve_account(&self, name: &str) -> Result<&Account, String> {
        self.accounts.get(name).ok_or_else(|| {
            let available = self.accounts.keys().cloned().collect::<Vec<_>>().join(", ");
            format!(
                "account '{name}' not found in config. Available: {}",
                if available.is_empty() {
                    "(none)"
                } else {
                    &available
                }
            )
        })
    }
}
