use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use uuid::Uuid;

use mail_builder::{
    headers::{content_type::ContentType, message_id::MessageId},
    mime::{BodyPart, MimePart},
    MessageBuilder,
};
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

/// Resolve a file reference (an embed target or an `attachments` entry) to a
/// path: `~` against HOME, an absolute path as-is, a relative path with a
/// separator against the draft's directory, and a bare filename against the
/// `source-assets` base. Returns `None` for a bare filename with no base set.
pub fn resolve_reference(raw: &str, note_dir: &Path, base: Option<&Path>) -> Option<PathBuf> {
    let t = raw.trim();
    if t.starts_with('~') {
        return Some(crate::config::expand_home(t));
    }
    let p = Path::new(t);
    if p.is_absolute() {
        return Some(PathBuf::from(t));
    }
    if t.contains('/') || t.contains('\\') {
        return Some(note_dir.join(t));
    }
    base.map(|b| b.join(t))
}

/// Whether a reference is a bare filename — no `~`, scheme, or path separator —
/// i.e. the only form that needs a `source-assets` base to resolve.
fn is_bare(t: &str) -> bool {
    let t = t.trim();
    !t.starts_with('~')
        && !t.starts_with("http://")
        && !t.starts_with("https://")
        && !t.starts_with("data:")
        && !Path::new(t).is_absolute()
        && !t.contains('/')
        && !t.contains('\\')
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Run pandoc, piping `input` to stdin and returning stdout. `args` is the
/// command-line argument list (after the `pandoc` program name).
fn run_pandoc(input: &str, args: &[String]) -> Result<String, String> {
    let mut child = Command::new("pandoc")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("pandoc failed to start: {e}"))?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .map_err(|e| format!("pandoc stdin write failed: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("pandoc failed: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "pandoc failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse markdown to the pandoc JSON AST (bare — no filters, template, or CSS).
fn pandoc_parse(markdown: &str) -> Result<Value, String> {
    let args = vec!["-f".into(), "markdown".into(), "-t".into(), "json".into()];
    let out = run_pandoc(markdown, &args)?;
    serde_json::from_str(&out).map_err(|e| format!("cannot parse pandoc JSON: {e}"))
}

/// Build the pandoc arguments for the render pass (`json → fmt`). The lua
/// filter, template, and CSS trio applies to html output only; `pandoc-raw`
/// disables it. Plain and markdown output render with no filter, template, or
/// CSS.
fn render_args(
    cfg: &Config,
    draft: &Draft,
    fmt: &str,
) -> Result<(Vec<String>, Vec<PathBuf>), String> {
    let pandoc_data = cfg.paths_pandoc_data();
    let raw = draft.pandoc_raw.unwrap_or(false);
    let mut args: Vec<String> = vec!["-f".into(), "json".into(), "-t".into(), fmt.into()];
    let mut tmp_files: Vec<PathBuf> = Vec::new();

    if fmt == "html" && !raw {
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

    Ok((args, tmp_files))
}

/// Render a pandoc JSON AST to `fmt` (html, plain, or markdown), applying the
/// lua filters, template, and CSS as configured.
fn pandoc_render(json: &Value, fmt: &str, cfg: &Config, draft: &Draft) -> Result<String, String> {
    let (args, tmp_files) = render_args(cfg, draft, fmt)?;
    let out = run_pandoc(&serde_json::to_string(json).expect("valid json"), &args);
    for tmp in tmp_files {
        let _ = std::fs::remove_file(tmp);
    }
    out
}

/// The target of an Obsidian `![[…]]` embed, i.e. the text of a `Str` node that
/// is exactly `![[target]]` (optionally with a `|alias` suffix).
fn obsidian_target(text: &str) -> Option<String> {
    let t = text.trim();
    let rest = t.strip_prefix("![[")?;
    let inner = rest.strip_suffix("]]")?;
    let target = inner.split('|').next()?.trim();
    if target.is_empty() {
        None
    } else {
        Some(target.to_string())
    }
}

/// The resolution of a single embed target.
#[derive(Debug)]
enum Embed {
    /// An inline image (html format): the resolved path. The caller assigns a
    /// `cid:`.
    Inline(PathBuf),
    /// A discrete attachment: the resolved path.
    Attach(PathBuf),
    /// A URL or data URI — leave the node untouched.
    Skip,
}

/// Resolve one embed target, classifying it as inline, attachment, or skip.
fn classify_embed(
    target: &str,
    note_dir: &Path,
    source_assets: Option<&Path>,
    html: bool,
) -> Result<Embed, String> {
    let t = target.trim();
    if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("data:") {
        return Ok(Embed::Skip);
    }
    if is_bare(t) && source_assets.is_none() {
        return Err(format!(
            "embed '{t}' is a bare filename but [paths] source-assets is not set"
        ));
    }
    let Some(path) = resolve_reference(t, note_dir, source_assets) else {
        return Ok(Embed::Skip);
    };
    if !path.is_file() {
        return Err(format!(
            "embed '{t}' resolves to {} which does not exist",
            path.display()
        ));
    }
    if is_image(&path) && html {
        Ok(Embed::Inline(path))
    } else {
        Ok(Embed::Attach(path))
    }
}

/// Inline image parts: `(cid, path)` pairs.
type InlineParts = Vec<(String, PathBuf)>;

/// Walk the pandoc JSON AST, resolving every embed (`Image` nodes and
/// `![[…]]` `Str` nodes) in place. Image embeds (html) become `cid:` srcs;
/// all other embeds are removed (they become attachments). Returns the inline
/// `(cid, path)` pairs and the attachment paths discovered from non-image
/// embeds.
fn resolve_embeds(
    json: &mut Value,
    note_dir: &Path,
    source_assets: Option<&Path>,
    html: bool,
) -> Result<(InlineParts, Vec<PathBuf>), String> {
    let mut inline = Vec::new();
    let mut attach = Vec::new();
    let mut cid_n = 0u32;
    walk(
        json,
        note_dir,
        source_assets,
        html,
        &mut inline,
        &mut attach,
        &mut cid_n,
    )?;
    Ok((inline, attach))
}

fn walk(
    v: &mut Value,
    note_dir: &Path,
    source_assets: Option<&Path>,
    html: bool,
    inline: &mut InlineParts,
    attach: &mut Vec<PathBuf>,
    cid_n: &mut u32,
) -> Result<(), String> {
    match v {
        Value::Array(items) => {
            for item in items.iter_mut() {
                walk(item, note_dir, source_assets, html, inline, attach, cid_n)?;
            }
        }
        Value::Object(map) => {
            let tag = map.get("t").and_then(Value::as_str).map(str::to_string);
            match tag.as_deref() {
                Some("Image") => {
                    if let Some(src) = map
                        .get("c")
                        .and_then(|c| c.get(2))
                        .and_then(|t| t.get(0))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                    {
                        match classify_embed(&src, note_dir, source_assets, html)? {
                            Embed::Inline(path) => {
                                *cid_n += 1;
                                let cid = format!("img-{cid_n}");
                                inline.push((cid.clone(), path));
                                let c = serde_json::json!([
                                    ["", [], []],
                                    [],
                                    [format!("cid:{cid}"), ""]
                                ]);
                                map.insert("c".to_string(), c);
                            }
                            Embed::Attach(path) => {
                                attach.push(path);
                                map.insert("t".to_string(), Value::String("Str".to_string()));
                                map.insert("c".to_string(), Value::String(String::new()));
                            }
                            Embed::Skip => {}
                        }
                    }
                    return Ok(());
                }
                Some("Str") => {
                    let text = map.get("c").and_then(Value::as_str).map(str::to_string);
                    if let Some(target) = text.as_deref().and_then(obsidian_target) {
                        match classify_embed(&target, note_dir, source_assets, html)? {
                            Embed::Inline(path) => {
                                *cid_n += 1;
                                let cid = format!("img-{cid_n}");
                                inline.push((cid.clone(), path));
                                map.insert("t".to_string(), Value::String("Image".to_string()));
                                let c = serde_json::json!([
                                    ["", [], []],
                                    [],
                                    [format!("cid:{cid}"), ""]
                                ]);
                                map.insert("c".to_string(), c);
                            }
                            Embed::Attach(path) => {
                                attach.push(path);
                                map.insert("c".to_string(), Value::String(String::new()));
                            }
                            Embed::Skip => {}
                        }
                        return Ok(());
                    }
                }
                _ => {}
            }
            for val in map.values_mut() {
                walk(val, note_dir, source_assets, html, inline, attach, cid_n)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Build the MML text body for a draft, resolving embeds through the pandoc
/// JSON AST. Returns `(mml_body, inline, embed_attachments)` where `mml_body`
/// is the alternative (html) or plain text (plain/text), `inline` is the
/// `(cid, path)` pairs for inline images, and `embed_attachments` is the paths
/// discovered from non-image embeds.
pub fn build_mml_body(
    draft: &Draft,
    body: &str,
    cfg: &Config,
    source_assets: Option<&Path>,
    note_dir: &Path,
) -> Result<(String, InlineParts, Vec<PathBuf>), String> {
    let fmt = draft.format();
    let body = body.trim_end_matches('\n');
    let html = fmt == "html";

    let mut json = pandoc_parse(body)?;
    let (inline, embed_attach) = resolve_embeds(&mut json, note_dir, source_assets, html)?;

    let inner = if fmt == "html" {
        let rendered = pandoc_render(&json, "html", cfg, draft)?;
        let plain = escape_mml(body);
        format!(
            "<#multipart type=alternative>\n{plain}\n<#part type=text/html>\n{rendered}\n<#/part>\n<#/multipart>"
        )
    } else if fmt == "plain" {
        escape_mml(&pandoc_render(&json, "plain", cfg, draft)?)
    } else {
        let rendered = pandoc_render(&json, "markdown", cfg, draft)?;
        if rendered.trim().is_empty() {
            "\n".to_string()
        } else {
            escape_mml(&rendered)
        }
    };

    Ok((inner, inline, embed_attach))
}

/// Encryption parameters: the recipient email addresses and an optional
/// self-encryption key (`--encrypt-to`).
pub struct EncryptSpec<'a> {
    pub recipients: &'a [String],
    pub encrypt_to: Option<&'a str>,
}

/// Build an inline image MIME part from a file, carrying an explicit
/// `Content-ID`. The `cid:` reference in the html body resolves against this.
fn inline_part(cid: &str, path: &Path) -> Result<MimePart<'static>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let ctype = ContentType::new(tree_magic_mini::from_u8(&bytes));
    let mut part = MimePart::new(ctype, bytes).inline();
    part.headers
        .push(("Content-ID".into(), MessageId::new(cid.to_string()).into()));
    Ok(part)
}

/// Build a discrete attachment MIME part from a file.
fn attachment_part(path: &Path) -> Result<MimePart<'static>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let ctype = ContentType::new(tree_magic_mini::from_u8(&bytes));
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("noname")
        .to_string();
    Ok(MimePart::new(ctype, bytes).attachment(name))
}

/// Compile a full MML message (headers + blank line + text body) to MIME bytes,
/// wrapping the body in `multipart/related` with the inline images and in
/// `multipart/mixed` with the attachments, then optionally signing and/or
/// encrypting.
pub fn compile_mml(
    mml_source: &str,
    inline: &[(String, PathBuf)],
    attachments: &[PathBuf],
    signing_key: Option<&str>,
    encrypt: Option<EncryptSpec<'_>>,
    forward_attach: Option<(&str, &[u8])>,
) -> Result<Vec<u8>, String> {
    let compiler = MmlCompilerBuilder::new()
        .build(mml_source)
        .map_err(|e| format!("mml compile failed: {e}"))?;
    let result = compiler
        .compile()
        .map_err(|e| format!("mml compile failed: {e}"))?;
    let mut builder = result.into_msg_builder();

    if !inline.is_empty() {
        let body = builder.body.take().ok_or("no body to wrap")?;
        let mut parts = vec![body];
        for (cid, path) in inline {
            parts.push(inline_part(cid, path)?);
        }
        builder.body = Some(MimePart::new(
            ContentType::new("multipart/related"),
            BodyPart::Multipart(parts),
        ));
    }

    if !attachments.is_empty() {
        let body = builder.body.take().ok_or("no body to wrap")?;
        let mut parts = vec![body];
        for path in attachments {
            parts.push(attachment_part(path)?);
        }
        builder.body = Some(MimePart::new(
            ContentType::new("multipart/mixed"),
            BodyPart::Multipart(parts),
        ));
    }

    if let Some((message_id, raw)) = forward_attach {
        let body = builder.body.take().ok_or("no body to forward")?;
        let forward_part = MimePart::new("message/rfc822", BodyPart::Binary(raw.to_vec().into()))
            .attachment(format!("{message_id}.eml"));
        let mixed = MimePart::new(
            ContentType::new("multipart/mixed"),
            BodyPart::Multipart(vec![body, forward_part]),
        );
        builder.body = Some(mixed);
    }
    if let Some(key) = signing_key {
        sign_body(&mut builder, key)?;
    }
    if let Some(spec) = encrypt {
        encrypt_body(&mut builder, spec.recipients, spec.encrypt_to)?;
    }

    builder
        .write_to_vec()
        .map_err(|e| format!("cannot serialize message: {e}"))
}

/// Sign the message body with OpenPGP, wrapping it in `multipart/signed`
/// (RFC 3156). Returns an error if the body is missing or gpg fails.
fn sign_body(builder: &mut MessageBuilder, signing_key: &str) -> Result<(), String> {
    let body = builder.body.take().ok_or("no body to sign")?;

    let mut body_bytes = Vec::new();
    body.clone()
        .write_part(&mut body_bytes)
        .map_err(|e| format!("cannot serialize body: {e}"))?;

    let signature = gpg_detach_sign(&body_bytes, signing_key)?;

    let sig_part = MimePart::new(
        "application/pgp-signature",
        BodyPart::Text(signature.into()),
    );
    let ctype = ContentType::new("multipart/signed")
        .attribute("protocol", "application/pgp-signature")
        .attribute("micalg", "pgp-sha512");
    let signed = MimePart::new(ctype, BodyPart::Multipart(vec![body, sig_part]));
    builder.body = Some(signed);
    Ok(())
}

fn gpg_detach_sign(body: &[u8], signing_key: &str) -> Result<String, String> {
    let mut child = Command::new("gpg")
        .args([
            "--detach-sign",
            "--armor",
            "--digest-algo",
            "sha512",
            "--local-user",
            signing_key,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("gpg failed to start: {e}"))?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(body)
        .map_err(|e| format!("gpg stdin write failed: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("gpg failed: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "gpg --detach-sign failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Encrypt the message body with OpenPGP, wrapping it in `multipart/encrypted`
/// (RFC 3156). Runs after `sign_body`, so a signed body is encrypted whole.
fn encrypt_body(
    builder: &mut MessageBuilder,
    recipients: &[String],
    encrypt_to: Option<&str>,
) -> Result<(), String> {
    let body = builder.body.take().ok_or("no body to encrypt")?;

    let mut body_bytes = Vec::new();
    body.clone()
        .write_part(&mut body_bytes)
        .map_err(|e| format!("cannot serialize body: {e}"))?;

    let ciphertext = gpg_encrypt(&body_bytes, recipients, encrypt_to)?;

    let version_part = MimePart::new(
        "application/pgp-encrypted",
        BodyPart::Text("Version: 1\n".into()),
    );
    let data_part = MimePart::new(
        "application/octet-stream",
        BodyPart::Text(ciphertext.into()),
    );
    let ctype =
        ContentType::new("multipart/encrypted").attribute("protocol", "application/pgp-encrypted");
    let encrypted = MimePart::new(ctype, BodyPart::Multipart(vec![version_part, data_part]));
    builder.body = Some(encrypted);
    Ok(())
}

fn gpg_encrypt(
    body: &[u8],
    recipients: &[String],
    encrypt_to: Option<&str>,
) -> Result<String, String> {
    let mut args: Vec<String> = vec!["--encrypt".into(), "--armor".into()];
    for r in recipients {
        args.push("--recipient".into());
        args.push(r.clone());
    }
    if let Some(key) = encrypt_to {
        args.push("--encrypt-to".into());
        args.push(key.to_string());
    }

    let mut child = Command::new("gpg")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("gpg failed to start: {e}"))?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(body)
        .map_err(|e| format!("gpg stdin write failed: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("gpg failed: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "gpg --encrypt failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_mml_escapes_directives() {
        assert_eq!(escape_mml("hello <#part foo"), "hello <#!part foo");
        assert_eq!(
            escape_mml("<#multipart type=alt>"),
            "<#!multipart type=alt>"
        );
    }

    #[test]
    fn escape_mml_leaves_escaped_and_plain_text() {
        assert_eq!(escape_mml("already <#!part"), "already <#!part");
        assert_eq!(escape_mml("no directives"), "no directives");
    }

    #[test]
    fn resolve_reference_routes_by_shape() {
        let note = Path::new("/notes");
        let attach = Path::new("/attachments");

        assert_eq!(
            resolve_reference("foo.png", note, Some(attach)),
            Some(PathBuf::from("/attachments/foo.png"))
        );
        assert_eq!(
            resolve_reference("sub/foo.png", note, Some(attach)),
            Some(PathBuf::from("/notes/sub/foo.png"))
        );
        assert_eq!(
            resolve_reference("/abs/foo.png", note, Some(attach)),
            Some(PathBuf::from("/abs/foo.png"))
        );
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            resolve_reference("~/foo.png", note, Some(attach)),
            Some(PathBuf::from(format!("{home}/foo.png")))
        );
        assert_eq!(resolve_reference("foo.png", note, None), None);
    }

    #[test]
    fn obsidian_target_extracts_target_and_alias() {
        assert_eq!(obsidian_target("![[foo.png]]"), Some("foo.png".to_string()));
        assert_eq!(
            obsidian_target("![[foo.png|alias]]"),
            Some("foo.png".to_string())
        );
        assert_eq!(
            obsidian_target("![[sub/foo.png]]"),
            Some("sub/foo.png".to_string())
        );
        assert_eq!(obsidian_target("see ![[foo.png]]"), None);
        assert_eq!(obsidian_target("plain text"), None);
    }

    #[test]
    fn classify_embed_skips_urls() {
        let note = Path::new("/notes");
        let attach = Path::new("/attachments");
        assert!(matches!(
            classify_embed("http://x/y.png", note, Some(attach), true).unwrap(),
            Embed::Skip
        ));
        assert!(matches!(
            classify_embed("https://x/y.png", note, Some(attach), true).unwrap(),
            Embed::Skip
        ));
        assert!(matches!(
            classify_embed("data:image/png;base64,abc", note, Some(attach), true).unwrap(),
            Embed::Skip
        ));
    }

    #[test]
    fn classify_embed_errors_on_bare_without_base() {
        let note = Path::new("/notes");
        let err = classify_embed("foo.png", note, None, true).unwrap_err();
        assert!(err.contains("source-assets"));
    }

    #[test]
    fn classify_embed_classifies_image_and_attachment() {
        let dir = std::env::temp_dir().join(format!("mdmc-embed-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("logo.png");
        let pdf = dir.join("doc.pdf");
        std::fs::write(&img, b"fake").unwrap();
        std::fs::write(&pdf, b"fake").unwrap();

        let Embed::Inline(p) = classify_embed("logo.png", &dir, Some(&dir), true).unwrap() else {
            panic!("expected Inline for image in html");
        };
        assert_eq!(p, img);

        let Embed::Attach(p) = classify_embed("logo.png", &dir, Some(&dir), false).unwrap() else {
            panic!("expected Attach for image in plain");
        };
        assert_eq!(p, img);

        let Embed::Attach(p) = classify_embed("doc.pdf", &dir, Some(&dir), true).unwrap() else {
            panic!("expected Attach for non-image");
        };
        assert_eq!(p, pdf);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_bare_detects_bare_filenames() {
        assert!(is_bare("foo.png"));
        assert!(is_bare("  foo.png  "));
        assert!(!is_bare("sub/foo.png"));
        assert!(!is_bare("~/foo.png"));
        assert!(!is_bare("/abs/foo.png"));
        assert!(!is_bare("http://x/y.png"));
        assert!(!is_bare("data:image/png;base64,abc"));
    }

    #[test]
    fn resolve_embeds_skips_code_blocks() {
        let mut json = serde_json::json!({
            "blocks": [
                {"t": "CodeBlock", "c": [["", [], []], "![[missing.png]]"]}
            ]
        });
        let (inline, attach) = resolve_embeds(&mut json, Path::new("/n"), None, true).unwrap();
        assert!(inline.is_empty());
        assert!(attach.is_empty());
    }

    #[test]
    fn build_mml_body_resolves_inline_image_to_cid() {
        let dir = std::env::temp_dir().join(format!("mdmc-body-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("logo.png"), b"fake").unwrap();

        let draft = Draft {
            format: Some("html".to_string()),
            ..Default::default()
        };
        let (mml, inline, attach) = build_mml_body(
            &draft,
            "![[logo.png]]",
            &Config::default(),
            Some(&dir),
            &dir,
        )
        .unwrap();

        assert_eq!(inline.len(), 1);
        assert_eq!(inline[0].0, "img-1");
        assert_eq!(inline[0].1, dir.join("logo.png"));
        assert!(attach.is_empty());
        assert!(mml.contains("cid:img-1"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
