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
/// the CSS theme, mirroring the Python `pandoc()` helper.
pub fn pandoc(markdown: &str, fmt: &str, cfg: &Config, draft: &Draft) -> Result<String, String> {
    let pandoc_data = cfg.paths_pandoc_data();
    let mut args: Vec<String> = vec!["-f".into(), "markdown".into(), "-t".into(), fmt.into()];

    let fm_filter = draft.lua_filter.as_deref().unwrap_or("").trim();
    let filters: Vec<String> = if !fm_filter.is_empty() {
        vec![fm_filter.to_string()]
    } else {
        cfg.default_lua_filters().to_vec()
    };
    for f in filters {
        let f = f.trim();
        if f.is_empty() {
            continue;
        }
        let resolved = if f.contains('/') {
            f.to_string()
        } else {
            pandoc_data
                .join("filters")
                .join(format!("{f}.lua"))
                .to_string_lossy()
                .into_owned()
        };
        args.push("--lua-filter".into());
        args.push(resolved);
    }

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
    if let Some(template) = template {
        let resolved = if template.contains('/') {
            template.to_string()
        } else {
            pandoc_data
                .join("templates")
                .join(template)
                .to_string_lossy()
                .into_owned()
        };
        args.push("--template".into());
        args.push(resolved);
    }

    let mut css_tmp: Option<PathBuf> = None;
    if fmt == "html" {
        let css = draft
            .css
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| cfg.default_css().map(str::trim).filter(|s| !s.is_empty()));
        if let Some(css) = css {
            let resolved_css =
                if css.starts_with("http://") || css.starts_with("https://") || css.contains('/') {
                    css.to_string()
                } else {
                    pandoc_data
                        .join("css")
                        .join("themes")
                        .join(format!("{css}.css"))
                        .to_string_lossy()
                        .into_owned()
                };
            let style = if css.starts_with("http://") || css.starts_with("https://") {
                String::new()
            } else {
                std::fs::read_to_string(&resolved_css).unwrap_or_default()
            };
            let tmp = std::env::temp_dir().join(format!("mdmc-css-{}.html", Uuid::new_v4()));
            std::fs::write(&tmp, format!("<style>{style}</style>"))
                .map_err(|e| format!("cannot write temp css file: {e}"))?;
            css_tmp = Some(tmp.clone());
            args.push("--standalone".into());
            args.push(format!("--include-in-header={}", tmp.display()));
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

    if let Some(tmp) = css_tmp {
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
