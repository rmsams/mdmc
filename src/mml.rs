use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use regex::Regex;
use uuid::Uuid;

use mml::compiler::message::MmlCompilerBuilder;

use crate::config::Config;
use crate::frontmatter::Draft;

const IMAGE_EXTS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "tiff"];

const EMBEDDED_TEMPLATE: &str = include_str!("../assets/pandoc/email.html");
const EMBEDDED_FILTER: &str = include_str!("../assets/pandoc/email.lua");
const EMBEDDED_CSS: &str = include_str!("../assets/pandoc/email.css");

/// Write a temporary file containing `content`, returning its path.
fn write_temp(content: &str, ext: &str) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!("mdmc-{}.{}", Uuid::new_v4(), ext));
    std::fs::write(&path, content).map_err(|e| format!("cannot write temp file: {e}"))?;
    Ok(path)
}

/// Resolve a filter name/path to a pandoc argument: a path is used literally,
/// a name resolves under `pandoc_data/filters/<name>.lua`.
fn resolve_filter(f: &str, pandoc_data: &Path) -> String {
    if f.contains('/') {
        f.to_string()
    } else {
        pandoc_data
            .join("filters")
            .join(format!("{f}.lua"))
            .to_string_lossy()
            .into_owned()
    }
}

/// Resolve a template name/path: a path is used literally, a name resolves
/// under `pandoc_data/templates/<name>` (no extension appended).
fn resolve_template(t: &str, pandoc_data: &Path) -> String {
    if t.contains('/') {
        t.to_string()
    } else {
        pandoc_data
            .join("templates")
            .join(t)
            .to_string_lossy()
            .into_owned()
    }
}

fn embed_obsidian() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"!\[\[([^\]|]+)(?:\|[^\]]*)?\]\]").expect("static regex"))
}

fn embed_markdown() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"!\[[^\]]*\]\(([^)\s]+)\)").expect("static regex"))
}

/// Escape any literal MML directive so the compiler does not mis-parse it.
/// `<#!` is mml's escape prefix, so an already-escaped `<#!` is left intact.
pub fn escape_mml(text: &str) -> String {
    let b = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' && i + 1 < b.len() && b[i + 1] == b'#' {
            if i + 2 < b.len() && b[i + 2] == b'!' {
                out.extend_from_slice(&b[i..i + 3]);
                i += 3;
            } else {
                out.extend_from_slice(b"<#!");
                i += 2;
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).expect("valid utf-8")
}

fn resolve_embed(target: &str, attachments_dir: &Path) -> Option<PathBuf> {
    let t = target.trim();
    if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("data:") {
        return None;
    }
    let p = if t.starts_with('~') {
        crate::config::expand_home(t)
    } else {
        PathBuf::from(t)
    };
    if !p.is_absolute() && !t.contains('/') && !t.contains('\\') {
        return Some(attachments_dir.join(t));
    }
    Some(p)
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn attachment_part(path: &Path) -> String {
    format!(
        "<#part filename={} disposition=attachment>\n<#/part>",
        path.display()
    )
}

fn inline_part(path: &Path) -> String {
    format!(
        "<#part filename={} disposition=inline>\n<#/part>",
        path.display()
    )
}

/// Run pandoc `markdown → fmt`, applying lua filters, template, and (html only)
/// the CSS theme. When the draft and config specify none, the embedded
/// `email.*` defaults apply; `pandoc-raw` disables all three.
pub fn pandoc(markdown: &str, fmt: &str, cfg: &Config, draft: &Draft) -> Result<String, String> {
    let pandoc_data = cfg.paths_pandoc_data();
    let raw = draft.pandoc_raw.unwrap_or(false);
    let mut args: Vec<String> = vec!["-f".into(), "markdown".into(), "-t".into(), fmt.into()];
    let mut tmp_files: Vec<PathBuf> = Vec::new();

    if !raw {
        // Lua filters: draft `lua-filter` (single) overrides the config
        // `lua-filters` (list); when neither is set, the embedded filter runs.
        let fm_filter = draft
            .lua_filter
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(f) = fm_filter {
            args.push("--lua-filter".into());
            args.push(resolve_filter(f, &pandoc_data));
        } else if !cfg.default_lua_filters().is_empty() {
            for f in cfg.default_lua_filters() {
                let f = f.trim();
                if f.is_empty() {
                    continue;
                }
                args.push("--lua-filter".into());
                args.push(resolve_filter(f, &pandoc_data));
            }
        } else {
            let p = write_temp(EMBEDDED_FILTER, "lua")?;
            tmp_files.push(p.clone());
            args.push("--lua-filter".into());
            args.push(p.to_string_lossy().into_owned());
        }

        // Template: draft overrides config; when neither is set, the embedded
        // template runs.
        let template = draft
            .template
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                cfg.default_template()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            });
        match template {
            Some(t) => {
                args.push("--template".into());
                args.push(resolve_template(t, &pandoc_data));
            }
            None => {
                let p = write_temp(EMBEDDED_TEMPLATE, "html")?;
                tmp_files.push(p.clone());
                args.push("--template".into());
                args.push(p.to_string_lossy().into_owned());
            }
        }

        // CSS (html only): draft overrides config; when neither is set, the
        // embedded theme is inlined.
        if fmt == "html" {
            let css = draft
                .css
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .or_else(|| cfg.default_css().map(str::trim).filter(|s| !s.is_empty()));
            let style = match css {
                Some(c) if c.starts_with("http://") || c.starts_with("https://") => String::new(),
                Some(c) if c.contains('/') => std::fs::read_to_string(c).unwrap_or_default(),
                Some(c) => {
                    let path = pandoc_data
                        .join("css")
                        .join("themes")
                        .join(format!("{c}.css"));
                    std::fs::read_to_string(path).unwrap_or_default()
                }
                None => EMBEDDED_CSS.to_string(),
            };
            let p = write_temp(&format!("<style>{style}</style>"), "html")?;
            tmp_files.push(p.clone());
            args.push("--standalone".into());
            args.push(format!("--include-in-header={}", p.display()));
        }
    }

    let mut child = Command::new("pandoc")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("pandoc failed to start: {e}"))?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(markdown.as_bytes())
        .map_err(|e| format!("pandoc stdin write failed: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("pandoc failed: {e}"))?;

    for tmp in tmp_files {
        let _ = std::fs::remove_file(tmp);
    }

    if !out.status.success() {
        return Err(format!(
            "pandoc failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Build the MML body for a note. Returns `(mml_body, inline_cids)`.
///
/// Image embeds become inline `cid:` parts in html format and attachments
/// otherwise; non-image embeds degrade to attachments.
pub fn build_mml_body(
    draft: &Draft,
    body: &str,
    cfg: &Config,
    attachments: &[PathBuf],
    attachments_dir: &Path,
) -> Result<(String, Vec<String>), String> {
    let fmt = draft.format();
    let body = body.trim_end_matches('\n');

    let mut matches: Vec<(usize, usize, String)> = Vec::new();
    for m in embed_obsidian().captures_iter(body) {
        let full = m.get(0).unwrap();
        let target = m.get(1).unwrap().as_str().trim().to_string();
        matches.push((full.start(), full.end(), target));
    }
    for m in embed_markdown().captures_iter(body) {
        let full = m.get(0).unwrap();
        let target = m.get(1).unwrap().as_str().trim().to_string();
        matches.push((full.start(), full.end(), target));
    }
    matches.sort_by_key(|(start, _, _)| *start);

    let mut inline: Vec<(String, PathBuf)> = Vec::new();
    let mut attach: Vec<PathBuf> = attachments.to_vec();

    let mut pieces: Vec<String> = Vec::new();
    let mut last = 0;
    let mut cid_n = 0;
    for (start, end, target) in &matches {
        pieces.push(body[last..*start].to_string());
        last = *end;
        match resolve_embed(target, attachments_dir) {
            Some(path) if path.is_file() && is_image(&path) => {
                if fmt == "html" {
                    cid_n += 1;
                    let cid = format!("img-{cid_n}");
                    inline.push((cid.clone(), path));
                    pieces.push(format!("![](cid:{cid})"));
                } else {
                    attach.push(path);
                    pieces.push(String::new());
                }
            }
            Some(path) if path.is_file() => {
                attach.push(path);
                pieces.push(String::new());
            }
            _ => pieces.push(body[*start..*end].to_string()),
        }
    }
    pieces.push(body[last..].to_string());
    let transformed = pieces.concat();

    let inner = if fmt == "html" {
        let html = pandoc(&transformed, "html", cfg, draft)?;
        let plain = escape_mml(body);
        format!(
            "<#multipart type=alternative>\n{plain}\n<#part type=text/html>\n{html}\n<#/part>\n<#/multipart>"
        )
    } else if fmt == "plain" {
        escape_mml(&pandoc(&transformed, "plain", cfg, draft)?)
    } else {
        if transformed.is_empty() {
            "\n".to_string()
        } else {
            escape_mml(&transformed)
        }
    };

    let inner = if !inline.is_empty() {
        let parts: Vec<String> = inline.iter().map(|(_, p)| inline_part(p)).collect();
        format!(
            "<#multipart type=related>\n{inner}\n{}\n<#/multipart>",
            parts.join("\n")
        )
    } else {
        inner
    };

    let inner = if !attach.is_empty() {
        let parts: Vec<String> = attach.iter().map(|p| attachment_part(p)).collect();
        format!(
            "<#multipart type=mixed>\n{inner}\n{}\n<#/multipart>",
            parts.join("\n")
        )
    } else {
        inner
    };

    Ok((inner, inline.into_iter().map(|(cid, _)| cid).collect()))
}

/// Compile a full MML message (headers + blank line + MML body) to MIME bytes.
pub fn compile_mml(mml_source: &str) -> Result<Vec<u8>, String> {
    let compiler = MmlCompilerBuilder::new()
        .build(mml_source)
        .map_err(|e| format!("mml compile failed: {e}"))?;
    compiler
        .compile()
        .map_err(|e| format!("mml compile failed: {e}"))?
        .into_vec()
        .map_err(|e| format!("mml compile failed: {e}"))
}
