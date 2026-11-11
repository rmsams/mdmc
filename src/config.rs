use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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

/// Resolve a config path: `~` against HOME, absolute as-is, relative against
/// the config file's directory.
fn resolve_path(raw: &str, base: &Path) -> PathBuf {
    if raw.starts_with('~') {
        expand_home(raw)
    } else if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        base.join(raw)
    }
}

fn merge_map<K: Ord, V>(local: BTreeMap<K, V>, global: BTreeMap<K, V>) -> BTreeMap<K, V> {
    let mut merged = global;
    for (k, v) in local {
        merged.insert(k, v);
    }
    merged
}

/// Discover the effective config: an explicit `--config` path wins; otherwise a
/// local `.mdmc.toml` (found by walking up from the current directory) is
/// merged over the global `~/.config/mdmc/config.toml`.
pub fn discover(flag: Option<PathBuf>) -> Config {
    if let Some(path) = flag {
        return Config::load(&path);
    }
    let global = Config::load(&expand_home("~/.config/mdmc/config.toml"));
    match find_local_config() {
        Some(local_path) => Config::merge(Config::load(&local_path), global),
        None => global,
    }
}

fn find_local_config() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(".mdmc.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
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

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub paths: Paths,
    #[serde(default)]
    pub folders: BTreeMap<String, String>,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub accounts: BTreeMap<String, Account>,
    #[serde(default)]
    pub editors: Vec<Editor>,
    #[serde(default, skip_serializing_if = "Lookup::is_empty")]
    pub lookup: Lookup,
}

/// Defaults for the `list`/`pick` commands (picker page size).
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Lookup {
    pub page_size: Option<usize>,
}

impl Lookup {
    fn is_empty(&self) -> bool {
        self.page_size.is_none()
    }
}

/// A named editor command string (program + args), e.g. `emacsclient -t`.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Editor {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Paths {
    pub source: Option<String>,
    pub snapshots: Option<String>,
    #[serde(rename = "source-assets")]
    pub source_assets: Option<String>,
    #[serde(rename = "snapshot-assets")]
    pub snapshot_assets: Option<String>,
    pub pandoc_data: Option<String>,
    pub mail: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Account {
    pub from: Option<String>,
    pub realname: Option<String>,
    pub drafts: Option<String>,
    pub sent: Option<String>,
    pub signing_key: Option<String>,
    pub encrypt_to: Option<String>,
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
        let mut cfg: Config = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default();
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        cfg.resolve_paths(base);
        cfg
    }

    /// Resolve all path-valued fields against `base`, so the config holds
    /// absolute paths after loading.
    fn resolve_paths(&mut self, base: &Path) {
        if let Some(p) = &mut self.paths.source {
            *p = resolve_path(p, base).to_string_lossy().into_owned();
        }
        if let Some(p) = &mut self.paths.source_assets {
            *p = resolve_path(p, base).to_string_lossy().into_owned();
        }
        if let Some(p) = &mut self.paths.snapshot_assets {
            *p = resolve_path(p, base).to_string_lossy().into_owned();
        }
        if let Some(p) = &mut self.paths.pandoc_data {
            *p = resolve_path(p, base).to_string_lossy().into_owned();
        }
        if let Some(p) = &mut self.paths.mail {
            *p = resolve_path(p, base).to_string_lossy().into_owned();
        }
        if let Some(p) = &mut self.paths.snapshots {
            *p = resolve_path(p, base).to_string_lossy().into_owned();
        }
        for v in self.folders.values_mut() {
            *v = resolve_path(v, base).to_string_lossy().into_owned();
        }
        for acct in self.accounts.values_mut() {
            if let Some(d) = &mut acct.drafts {
                *d = resolve_path(d, base).to_string_lossy().into_owned();
            }
            if let Some(s) = &mut acct.sent {
                *s = resolve_path(s, base).to_string_lossy().into_owned();
            }
        }
    }

    /// Merge a local config over a global one: local fields take precedence,
    /// and `accounts`/`folders` maps are a union with local precedence.
    pub fn merge(local: Config, global: Config) -> Config {
        Config {
            paths: Paths {
                source: local.paths.source.or(global.paths.source),
                snapshots: local.paths.snapshots.or(global.paths.snapshots),
                source_assets: local.paths.source_assets.or(global.paths.source_assets),
                snapshot_assets: local.paths.snapshot_assets.or(global.paths.snapshot_assets),
                pandoc_data: local.paths.pandoc_data.or(global.paths.pandoc_data),
                mail: local.paths.mail.or(global.paths.mail),
            },
            folders: merge_map(local.folders, global.folders),
            defaults: Defaults {
                account: local.defaults.account.or(global.defaults.account),
                css: local.defaults.css.or(global.defaults.css),
                template: local.defaults.template.or(global.defaults.template),
                lua_filters: local.defaults.lua_filters.or(global.defaults.lua_filters),
            },
            accounts: merge_map(local.accounts, global.accounts),
            editors: if local.editors.is_empty() {
                global.editors
            } else {
                local.editors
            },
            lookup: Lookup {
                page_size: local.lookup.page_size.or(global.lookup.page_size),
            },
        }
    }

    pub fn paths_source(&self) -> Result<PathBuf, String> {
        let raw =
            self.paths.source.as_deref().ok_or_else(|| {
                "[paths] source is required (where markdown drafts live)".to_string()
            })?;
        Ok(expand_home(raw))
    }

    /// The optional bare-filename base for draft embeds and attachments.
    pub fn paths_source_assets(&self) -> Option<PathBuf> {
        self.paths.source_assets.as_deref().map(expand_home)
    }

    /// The extraction store for decompiled snapshots (inline + discrete).
    pub fn paths_snapshot_assets(&self) -> Result<PathBuf, String> {
        let raw = self
            .paths
            .snapshot_assets
            .as_deref()
            .ok_or_else(|| "[paths] snapshot-assets is required".to_string())?;
        Ok(expand_home(raw))
    }

    pub fn paths_pandoc_data(&self) -> PathBuf {
        expand_home(
            self.paths
                .pandoc_data
                .as_deref()
                .unwrap_or("~/.local/share/pandoc"),
        )
    }

    pub fn paths_mail(&self) -> Result<PathBuf, String> {
        let raw = self
            .paths
            .mail
            .as_deref()
            .ok_or_else(|| "[paths] mail is required (the maildir root)".to_string())?;
        Ok(expand_home(raw))
    }

    pub fn paths_snapshots(&self) -> Result<PathBuf, String> {
        let raw = self.paths.snapshots.as_deref().ok_or_else(|| {
            "[paths] snapshots is required (where decompiled notes live)".to_string()
        })?;
        Ok(expand_home(raw))
    }

    /// The picker page size (`[lookup] page_size`, default 7).
    pub fn page_size(&self) -> usize {
        self.lookup.page_size.unwrap_or(7)
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

    /// Resolve an editor command string. An explicit `name` is looked up in
    /// `[editors]`; otherwise the first configured editor is used, falling back
    /// to `$VISUAL`/`$EDITOR` (`vi` when neither is set). The command is a
    /// `sh -c` script with the file path available as `$1`/`$@`.
    pub fn editor(&self, name: Option<&str>) -> Result<String, String> {
        if let Some(name) = name {
            return self
                .editors
                .iter()
                .find(|e| e.name == name)
                .map(|e| e.command.clone())
                .ok_or_else(|| {
                    let available = self
                        .editors
                        .iter()
                        .map(|e| e.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "editor '{name}' not configured. Available: {}",
                        if available.is_empty() {
                            "(none)"
                        } else {
                            &available
                        }
                    )
                });
        }
        if let Some(first) = self.editors.first() {
            return Ok(first.command.clone());
        }
        Ok(std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .map(|e| format!("{e} \"$@\""))
            .unwrap_or_else(|_| "vi \"$@\"".to_string()))
    }

    /// Serialize the effective config to TOML (resolved absolute paths).
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| format!("cannot serialize config: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_home_handles_tilde() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_home("~/foo"), PathBuf::from(format!("{home}/foo")));
        assert_eq!(expand_home("~"), PathBuf::from(&home));
        assert_eq!(expand_home("/abs/path"), PathBuf::from("/abs/path"));
    }

    #[test]
    fn load_parses_accounts_and_defaults() {
        let toml = r#"
[accounts.test]
from = "a@b.c"
realname = "A B"
signing_key = "0xABC"

[defaults]
account = "test"
lua-filters = ["email"]
"#;
        let path = std::env::temp_dir().join(format!("mdmc-cfg-{}.toml", uuid::Uuid::new_v4()));
        std::fs::write(&path, toml).unwrap();
        let cfg = Config::load(&path);
        std::fs::remove_file(&path).ok();

        let acct = cfg.resolve_account("test").unwrap();
        assert_eq!(acct.from.as_deref(), Some("a@b.c"));
        assert_eq!(acct.realname.as_deref(), Some("A B"));
        assert_eq!(acct.signing_key.as_deref(), Some("0xABC"));
        assert_eq!(acct.sender(), "A B <a@b.c>");
        assert_eq!(cfg.default_account(), Some("test"));
        assert_eq!(cfg.default_lua_filters(), &["email".to_string()][..]);
    }

    #[test]
    fn resolve_account_missing_errors() {
        let cfg = Config::default();
        assert!(cfg.resolve_account("nope").is_err());
    }

    #[test]
    fn required_paths_error_when_unset() {
        let cfg = Config::default();
        assert!(cfg.paths_source().is_err());
        assert!(cfg.paths_snapshot_assets().is_err());
        assert!(cfg.paths_mail().is_err());
        assert!(cfg.paths_snapshots().is_err());
        assert!(cfg.paths_pandoc_data().ends_with(".local/share/pandoc"));
        assert!(cfg.paths_source_assets().is_none());
    }

    #[test]
    fn sender_omits_realname_when_absent() {
        let acct = Account {
            from: Some("a@b.c".to_string()),
            ..Default::default()
        };
        assert_eq!(acct.sender(), "a@b.c");
    }

    fn temp_config(toml: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("mdmc-cfg-{}.toml", uuid::Uuid::new_v4()));
        std::fs::write(&path, toml).unwrap();
        path
    }

    #[test]
    fn merge_local_overrides_global() {
        let global = Config::load(&temp_config(
            r#"
[defaults]
account = "global"
css = "globalcss"

[accounts.a]
from = "a@global"
"#,
        ));
        let local = Config::load(&temp_config(
            r#"
[defaults]
account = "local"

[accounts.a]
from = "a@local"

[accounts.b]
from = "b@local"
"#,
        ));
        let merged = Config::merge(local, global);

        assert_eq!(merged.default_account(), Some("local"));
        assert_eq!(merged.default_css(), Some("globalcss"));
        assert_eq!(
            merged.resolve_account("a").unwrap().from.as_deref(),
            Some("a@local")
        );
        assert_eq!(
            merged.resolve_account("b").unwrap().from.as_deref(),
            Some("b@local")
        );
    }

    #[test]
    fn load_resolves_relative_paths_against_config_dir() {
        let dir = std::env::temp_dir().join(format!("mdmc-cfgdir-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".mdmc.toml");
        std::fs::write(
            &path,
            "[paths]\nsource = \"source\"\nsource-assets = \"attachments\"\nsnapshot-assets = \"snapshots\"\n",
        )
        .unwrap();
        let cfg = Config::load(&path);
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(cfg.paths_source().unwrap(), dir.join("source"));
        assert_eq!(cfg.paths_source_assets().unwrap(), dir.join("attachments"));
        assert_eq!(cfg.paths_snapshot_assets().unwrap(), dir.join("snapshots"));
    }

    #[test]
    fn load_parses_editors_and_resolves_by_name() {
        let toml = r#"
[[editors]]
name = "emacs"
command = "emacsclient -t"

[[editors]]
name = "obsidian"
command = "obs open"
"#;
        let cfg = Config::load(&temp_config(toml));

        assert_eq!(cfg.editors.len(), 2);
        assert_eq!(cfg.editor(None).unwrap(), "emacsclient -t");
        assert_eq!(cfg.editor(Some("obsidian")).unwrap(), "obs open");
        assert!(cfg.editor(Some("nope")).is_err());
    }

    #[test]
    fn to_toml_serializes_resolved_config() {
        let toml = r#"
[paths]
source = "~/kb/ops/mail/drafts"

[[editors]]
name = "emacs"
command = "emacsclient -t \"$@\""
"#;
        let cfg = Config::load(&temp_config(toml));
        let out = cfg.to_toml().unwrap();
        assert!(out.contains("[paths]"));
        assert!(out.contains("source = "));
        assert!(out.contains("[[editors]]"));
        assert!(out.contains("name = \"emacs\""));
    }
}
