use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use mail_parser::{Message, MessageParser, MessagePart, PartKind};

use crate::addr::{addresses, format_mailbox};
use crate::config::Config;
use crate::frontmatter::{self, Field};
use crate::util;

/// An attachment: the display name, the MIME content type, the bytes as
/// written to disk, and the `Content-ID` when it is an inline `cid:` part.
struct Attachment {
    name: String,
    content_type: String,
    bytes: Vec<u8>,
    content_id: Option<String>,
    disk: String,
}

/// Decompile a raw message to a markdown snapshot note. The input is `-`
/// (stdin), an existing file path, or a Message-ID resolved by scanning the
/// maildir tree under `[paths] mail`. Never sends.
pub fn decompile(
    input: &str,
    stdout: bool,
    list_attachments: bool,
    link_format: Option<&str>,
    cfg: &Config,
) -> Result<(), String> {
    let wikilinks = link_format
        .or_else(|| cfg.default_link_format())
        .unwrap_or("markdown")
        == "wikilink";
    let raw = resolve_input(input, cfg)?;
    let message = parse_message(raw)?;
    let message_id = message
        .message_id()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("message has no Message-ID header")?
        .to_string();

    let (mut inline, mut discrete) = split_attachments(&message);
    assign_disk_names(&mut inline, &message_id, true);
    assign_disk_names(&mut discrete, &message_id, false);

    let cid_map: Vec<(String, String)> = inline
        .iter()
        .filter_map(|a| {
            let cid = a.content_id.clone()?;
            Some((cid, a.disk.clone()))
        })
        .collect();

    let (body, html_rendered) = render_body(&message, &cid_map, wikilinks)?;

    // Inline images embedded in the body are not re-listed; they appear in the
    // `## Attachments` section only when the text body was rendered instead.
    let listed: Vec<&Attachment> = if html_rendered {
        discrete.iter().collect()
    } else {
        inline.iter().chain(discrete.iter()).collect()
    };

    let all_attachments: Vec<(String, String)> = inline
        .iter()
        .chain(discrete.iter())
        .map(|a| (a.disk.clone(), a.content_type.clone()))
        .collect();

    let fields = build_fields(&message, Some(&message_id), &all_attachments);
    let mut body_text = body;
    if list_attachments && !listed.is_empty() {
        body_text.push_str("\n## Attachments\n\n");
        for att in &listed {
            body_text.push_str(&format!("[{}]({})\n", att.name, att.disk));
        }
    }
    let note = frontmatter::render_note(&fields, body_text.trim_end());

    if stdout {
        print!("{note}");
        return Ok(());
    }

    let snapshots_dir = cfg.paths_snapshots()?;
    std::fs::create_dir_all(&snapshots_dir)
        .map_err(|e| format!("cannot create {}: {e}", snapshots_dir.display()))?;
    let path = snapshots_dir.join(format!("{}.md", util::hash_message_id(&message_id)));
    if path.exists() {
        println!("skipped: {} (already exists)", path.display());
        return Ok(());
    }

    let snapshot_assets = cfg.paths_snapshot_assets()?;
    std::fs::create_dir_all(&snapshot_assets)
        .map_err(|e| format!("cannot create {}: {e}", snapshot_assets.display()))?;
    for att in inline.iter().chain(discrete.iter()) {
        std::fs::write(snapshot_assets.join(&att.disk), &att.bytes)
            .map_err(|e| format!("cannot write attachment: {e}"))?;
    }

    std::fs::write(&path, &note).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

pub(crate) fn resolve_input(input: &str, cfg: &Config) -> Result<Vec<u8>, String> {
    if input == "-" {
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| format!("cannot read stdin: {e}"))?;
        return Ok(buf);
    }
    if Path::new(input).is_file() {
        return std::fs::read(input).map_err(|e| format!("cannot read {input}: {e}"));
    }
    let id = input.trim().trim_start_matches('<').trim_end_matches('>');
    let root = cfg.paths_mail()?;
    let path = crate::notmuch::resolve(id, Some(&root))
        .ok_or_else(|| format!("message-id '{id}' not found under {}", root.display()))?;
    std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

/// Parse raw bytes as an RFC 5322 message, rejecting input that is not one. A
/// valid message always carries a `From` header (RFC 5322 §3.6), so its absence
/// distinguishes non-message input (e.g. a markdown draft) from real mail.
pub(crate) fn parse_message(raw: Vec<u8>) -> Result<Message<'static>, String> {
    let message = MessageParser::new()
        .parse_owned(raw)
        .ok_or("cannot parse message: not a valid RFC 5322 message")?;
    if message.from().is_none() {
        return Err("not a valid RFC 5322 message (missing From header)".to_string());
    }
    Ok(message)
}

/// Render the message body to markdown: a `text/plain` part verbatim, else a
/// `text/html` part converted with pandoc. Returns the rendered body and
/// whether the HTML path was taken (so inline `cid:` images are known to be
/// embedded).
pub(crate) fn render_body(
    message: &Message,
    cid_map: &[(String, String)],
    wikilinks: bool,
) -> Result<(String, bool), String> {
    let has_text = message
        .text_body()
        .any(|p| matches!(p.kind(), PartKind::Text));
    if has_text {
        return Ok((
            message
                .body_text(0)
                .map(|c| c.into_owned())
                .unwrap_or_default(),
            false,
        ));
    }
    let has_html = message
        .html_body()
        .any(|p| matches!(p.kind(), PartKind::Html));
    if has_html {
        let html = message
            .body_html(0)
            .map(|c| c.into_owned())
            .unwrap_or_default();
        return Ok((html_to_gfm(&html, cid_map, wikilinks)?, true));
    }
    Ok((
        message
            .body_text(0)
            .map(|c| c.into_owned())
            .unwrap_or_default(),
        false,
    ))
}

/// Build the complete Lua filter source: the `cidmap` table followed by the
/// AST walker. With `wikilinks`, a `cid:` image is emitted as an Obsidian
/// `![[…]]` embed; otherwise its `src` is set to the filename, which the gfm
/// writer renders as `![…](…)`.
fn build_cid_filter(map: &[(String, String)], wikilinks: bool) -> String {
    let mut src = String::from("local cidmap = {\n");
    for (cid, disk) in map {
        if cid.is_empty() {
            continue;
        }
        src.push_str(&format!("  [{}] = {},\n", lua_quote(cid), lua_quote(disk)));
    }
    src.push_str("}\n");
    src.push_str("function Image(img)\n");
    src.push_str("  local cid = img.src:match('^cid:(.*)$')\n");
    src.push_str("  if cid and cidmap[cid] then\n");
    if wikilinks {
        src.push_str("    return pandoc.RawInline('gfm', '![[' .. cidmap[cid] .. ']]')\n");
    } else {
        src.push_str("    img.src = cidmap[cid]\n");
    }
    src.push_str("  end\n");
    src.push_str("  return img\n");
    src.push_str("end\n");
    src
}

/// Quote a string as a Lua string literal.
fn lua_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn html_to_gfm(
    html: &str,
    cid_map: &[(String, String)],
    wikilinks: bool,
) -> Result<String, String> {
    let mut args: Vec<String> = vec!["-f".into(), "html".into(), "-t".into(), "gfm".into()];
    let mut filter_tmp = None;
    if !cid_map.is_empty() {
        let path = write_temp_filter(&build_cid_filter(cid_map, wikilinks))?;
        args.push("--lua-filter".into());
        args.push(path.to_string_lossy().into_owned());
        filter_tmp = Some(path);
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
        .write_all(html.as_bytes())
        .map_err(|e| format!("pandoc stdin write failed: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("pandoc failed: {e}"))?;

    if let Some(p) = filter_tmp {
        let _ = std::fs::remove_file(p);
    }

    if !out.status.success() {
        return Err(format!(
            "pandoc failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Write a temporary `.lua` filter file containing `content`, returning its path.
fn write_temp_filter(content: &str) -> Result<std::path::PathBuf, String> {
    let path = std::env::temp_dir().join(format!("mdmc-cidmap-{}.lua", uuid::Uuid::new_v4()));
    std::fs::write(&path, content).map_err(|e| format!("cannot write temp filter: {e}"))?;
    Ok(path)
}

fn split_attachments(message: &Message) -> (Vec<Attachment>, Vec<Attachment>) {
    let mut inline = Vec::new();
    let mut discrete = Vec::new();
    for part in message.attachments() {
        let Some(name) = attachment_name(&part) else {
            continue;
        };
        let att = Attachment {
            name,
            content_type: part_content_type(&part),
            bytes: part.decoded().into_owned(),
            content_id: part.content_id().map(str::to_string),
            disk: String::new(),
        };
        if att.content_id.is_some() && matches!(part.kind(), PartKind::InlineBinary) {
            inline.push(att);
        } else {
            discrete.push(att);
        }
    }
    (inline, discrete)
}

/// The part's `type/subtype`, defaulting to `application/octet-stream` when it
/// declares no `Content-Type`.
fn part_content_type(part: &MessagePart) -> String {
    part.content_type()
        .map(|ct| match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_string(),
        })
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

/// A display name for a part: its attachment filename, else a name derived from
/// the `Content-ID` plus the content-type subtype.
fn attachment_name(part: &MessagePart) -> Option<String> {
    if let Some(name) = part
        .attachment_name()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        return Some(name.to_string());
    }
    let cid = part.content_id()?;
    let ext = part
        .content_type()
        .and_then(|ct| ct.subtype().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "img".to_string());
    Some(format!("{cid}.{ext}"))
}

/// The on-disk filename of an extracted file: `<hash>-<name>` for a discrete
/// attachment, `cid-<hash>-<name>` for an inline `cid:` asset.
fn disk_name(message_id: &str, name: &str, inline: bool) -> String {
    let hash = util::hash_message_id(message_id);
    let base = sanitize_name(name);
    if inline {
        format!("cid-{hash}-{base}")
    } else {
        format!("{hash}-{base}")
    }
}

/// Assign each attachment its on-disk filename, disambiguating a repeated
/// sanitised name within a message with a `-2`, `-3`, … suffix.
fn assign_disk_names(atts: &mut [Attachment], message_id: &str, inline: bool) {
    let mut seen = HashSet::new();
    for att in atts.iter_mut() {
        let base = disk_name(message_id, &att.name, inline);
        let mut name = base.clone();
        let mut n = 2;
        while !seen.insert(name.clone()) {
            name = format!("{base}-{n}");
            n += 1;
        }
        att.disk = name;
    }
}

pub(crate) fn build_fields(
    message: &Message,
    message_id: Option<&str>,
    attachments: &[(String, String)],
) -> Vec<(String, Field)> {
    let mut fields: Vec<(String, Field)> = Vec::new();
    let subject = message
        .subject()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(no subject)");
    fields.push(("title".to_string(), Field::Str(subject.to_string())));
    fields.push(("type".to_string(), Field::Str("mail-snapshot".to_string())));
    if let Some(message_id) = message_id {
        fields.push(("message-id".to_string(), Field::Str(message_id.to_string())));
    }
    if let Some(from) = message.from().and_then(|list| list.first()) {
        let s = format_mailbox(&from);
        if !s.is_empty() {
            fields.push(("from".to_string(), Field::Str(s)));
        }
    }
    if let Some(date) = message.date() {
        fields.push(("date".to_string(), Field::Str(date.to_rfc3339())));
    }
    if let Some(to) = message.to().filter(|l| !l.is_empty()) {
        fields.push(("to".to_string(), Field::List(addresses(&to))));
    }
    if let Some(cc) = message.cc().filter(|l| !l.is_empty()) {
        fields.push(("cc".to_string(), Field::List(addresses(&cc))));
    }
    if let Some(irt) = message.in_reply_to().and_then(|list| list.first()) {
        fields.push(("in-reply-to".to_string(), Field::Str(irt.to_string())));
    }
    if let Some(references) = message.references().filter(|l| !l.is_empty()) {
        fields.push((
            "references".to_string(),
            Field::List(references.iter().map(str::to_string).collect()),
        ));
    }
    if !attachments.is_empty() {
        let objects: Vec<Vec<(String, String)>> = attachments
            .iter()
            .map(|(file, content_type)| {
                vec![
                    ("file".to_string(), file.clone()),
                    ("content-type".to_string(), content_type.clone()),
                ]
            })
            .collect();
        fields.push(("attachments".to_string(), Field::ObjectList(objects)));
    }
    fields
}

/// Replace path separators, whitespace, and other filesystem-hostile
/// characters so an attachment name is a safe single path component.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '\0' => '_',
            c if c.is_control() || c.is_whitespace() => '_',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_name_replaces_separators() {
        assert_eq!(sanitize_name("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_name("plain.txt"), "plain.txt");
    }

    #[test]
    fn addresses_preserves_display_names() {
        let raw = b"From: Ann <ann@example.com>\r\n\
            To: Bob <bob@example.com>, team: cy@example.com, dee@example.com;\r\n\r\nBody";
        let message = MessageParser::new().parse(raw).unwrap();
        let to = message.to().unwrap();
        assert_eq!(
            addresses(&to),
            vec!["Bob <bob@example.com>", "cy@example.com", "dee@example.com"]
        );
    }

    #[test]
    fn format_mailbox_renders_name_and_address() {
        let raw = b"From: Ann <ann@example.com>\r\n\r\nBody";
        let message = MessageParser::new().parse(raw).unwrap();
        let from = message.from().unwrap().first().unwrap();
        assert_eq!(format_mailbox(&from), "Ann <ann@example.com>");
    }

    #[test]
    fn lua_quote_escapes_specials() {
        assert_eq!(lua_quote("plain"), "\"plain\"");
        assert_eq!(lua_quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(lua_quote("line\nbreak"), "\"line\\nbreak\"");
    }

    #[test]
    fn build_cid_filter_embeds_map_and_walker() {
        let map = vec![
            ("logo123".to_string(), "id--a.png".to_string()),
            ("pic 2".to_string(), "id--b.png".to_string()),
        ];
        let markdown = build_cid_filter(&map, false);
        assert!(markdown.starts_with("local cidmap = {\n"));
        assert!(markdown.contains(r#"["logo123"] = "id--a.png","#));
        assert!(markdown.contains(r#"["pic 2"] = "id--b.png","#));
        assert!(markdown.contains("function Image(img)"));
        assert!(markdown.contains("img.src = cidmap[cid]"));

        let wikilink = build_cid_filter(&map, true);
        assert!(wikilink.contains("pandoc.RawInline('gfm', '![[' .. cidmap[cid] .. ']]')"));
    }

    #[test]
    fn split_attachments_separates_inline_from_discrete() {
        let raw = concat!(
            "From: Ann <ann@example.com>\r\n",
            "Message-ID: <m1@x>\r\n",
            "Content-Type: multipart/related; boundary=\"b\"\r\n",
            "\r\n",
            "--b\r\n",
            "Content-Type: text/html\r\n",
            "\r\n",
            "<img src=\"cid:img1\">\r\n",
            "--b\r\n",
            "Content-Type: image/png\r\n",
            "Content-ID: <img1>\r\n",
            "Content-Disposition: inline; filename=\"pic.png\"\r\n",
            "Content-Transfer-Encoding: base64\r\n",
            "\r\n",
            "iVBORw0KGgo=\r\n",
            "--b--\r\n",
        );
        let message = MessageParser::new().parse(raw).unwrap();
        let (inline, discrete) = split_attachments(&message);
        assert_eq!(inline.len(), 1);
        assert!(discrete.is_empty());
        assert_eq!(inline[0].content_id.as_deref(), Some("img1"));
        assert_eq!(inline[0].name, "pic.png");
    }

    #[test]
    fn disk_name_prefixes_and_sanitizes() {
        let hash = crate::util::hash_message_id("m1@x");
        assert_eq!(
            disk_name("m1@x", "a/b c.png", false),
            format!("{hash}-a_b_c.png")
        );
        assert_eq!(
            disk_name("m1@x", "a/b c.png", true),
            format!("cid-{hash}-a_b_c.png")
        );
    }

    const SAMPLE: &str = concat!(
        "From: Ann <ann@example.com>\r\n",
        "To: Bob <bob@example.com>\r\n",
        "Subject: Report\r\n",
        "Message-ID: <m1@example.com>\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: multipart/mixed; boundary=\"b\"\r\n",
        "\r\n",
        "--b\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Hello body.\r\n",
        "--b\r\n",
        "Content-Type: application/pdf; name=\"report.pdf\"\r\n",
        "Content-Transfer-Encoding: base64\r\n",
        "Content-Disposition: attachment; filename=\"report.pdf\"\r\n",
        "\r\n",
        "JVBERi0xLjQK\r\n",
        "--b--\r\n",
    );

    fn setup() -> (
        std::path::PathBuf,
        Config,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let dir = std::env::temp_dir().join(format!("mdmc-decompile-{}", uuid::Uuid::new_v4()));
        let mail = dir.join("mail");
        let snapshots = dir.join("snapshots");
        let attachments = dir.join("attachments");
        std::fs::create_dir_all(mail.join("rmsams-gmail.com/cur")).unwrap();
        std::fs::create_dir_all(&snapshots).unwrap();
        std::fs::create_dir_all(&attachments).unwrap();
        let cfg = Config {
            paths: crate::config::Paths {
                mail: Some(mail.to_string_lossy().into_owned()),
                snapshots: Some(snapshots.to_string_lossy().into_owned()),
                snapshot_assets: Some(attachments.to_string_lossy().into_owned()),
                ..Default::default()
            },
            ..Default::default()
        };
        (dir, cfg, snapshots, attachments)
    }

    fn write_sample(cfg: &Config) {
        let cur = cfg
            .paths_mail()
            .unwrap()
            .join("rmsams-gmail.com")
            .join("cur");
        std::fs::write(cur.join("msg1"), SAMPLE).unwrap();
    }

    #[test]
    fn decompile_writes_note_and_attachments() {
        let (dir, cfg, snapshots, attachments) = setup();
        write_sample(&cfg);
        let hash = crate::util::hash_message_id("m1@example.com");

        decompile("m1@example.com", false, true, None, &cfg).unwrap();

        let note = std::fs::read_to_string(snapshots.join(format!("{hash}.md"))).unwrap();
        assert!(note.contains("title: Report\n"));
        assert!(note.contains("type: mail-snapshot\n"));
        assert!(note.contains("message-id: m1@example.com\n"));
        assert!(note.contains("from: Ann <ann@example.com>\n"));
        assert!(note.contains("to:\n  - Bob <bob@example.com>\n"));
        let expected = format!(
            "attachments:\n  - file: {hash}-report.pdf\n    content-type: application/pdf\n"
        );
        assert!(note.contains(expected.as_str()));
        assert!(note.contains("Hello body."));
        assert!(note.contains("## Attachments"));
        assert!(note.contains(format!("[report.pdf]({hash}-report.pdf)").as_str()));

        assert_eq!(
            std::fs::read(attachments.join(format!("{hash}-report.pdf"))).unwrap(),
            b"%PDF-1.4\n"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn decompile_omits_attachments_section_by_default() {
        let (dir, cfg, snapshots, _) = setup();
        write_sample(&cfg);
        let hash = crate::util::hash_message_id("m1@example.com");

        decompile("m1@example.com", false, false, None, &cfg).unwrap();

        let note = std::fs::read_to_string(snapshots.join(format!("{hash}.md"))).unwrap();
        assert!(!note.contains("## Attachments"));
        let expected = format!(
            "attachments:\n  - file: {hash}-report.pdf\n    content-type: application/pdf\n"
        );
        assert!(note.contains(expected.as_str()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn decompile_is_idempotent() {
        let (dir, cfg, snapshots, _) = setup();
        write_sample(&cfg);
        let hash = crate::util::hash_message_id("m1@example.com");

        decompile("m1@example.com", false, false, None, &cfg).unwrap();
        let note_path = snapshots.join(format!("{hash}.md"));
        std::fs::write(&note_path, "sentinel").unwrap();

        decompile("m1@example.com", false, false, None, &cfg).unwrap();
        assert_eq!(std::fs::read_to_string(&note_path).unwrap(), "sentinel");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn decompile_stdout_writes_no_files() {
        let (dir, cfg, snapshots, _) = setup();
        write_sample(&cfg);
        let hash = crate::util::hash_message_id("m1@example.com");

        decompile("m1@example.com", true, false, None, &cfg).unwrap();
        assert!(!snapshots.join(format!("{hash}.md")).exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn decompile_emits_wikilinks() {
        let dir = std::env::temp_dir().join(format!("mdmc-decompile-wl-{}", uuid::Uuid::new_v4()));
        let mail = dir.join("mail");
        let snapshots = dir.join("snapshots");
        let assets = dir.join("assets");
        std::fs::create_dir_all(mail.join("rmsams-gmail.com/cur")).unwrap();
        std::fs::create_dir_all(&snapshots).unwrap();
        std::fs::create_dir_all(&assets).unwrap();
        let cfg = Config {
            paths: crate::config::Paths {
                mail: Some(mail.to_string_lossy().into_owned()),
                snapshots: Some(snapshots.to_string_lossy().into_owned()),
                snapshot_assets: Some(assets.to_string_lossy().into_owned()),
                ..Default::default()
            },
            ..Default::default()
        };

        let raw = concat!(
            "From: Ann <ann@example.com>\r\n",
            "To: Bob <bob@example.com>\r\n",
            "Subject: Pic\r\n",
            "Message-ID: <m1@x>\r\n",
            "Content-Type: multipart/related; boundary=\"b\"\r\n",
            "\r\n",
            "--b\r\n",
            "Content-Type: text/html\r\n",
            "\r\n",
            "<p>see <img src=\"cid:img1\"></p>\r\n",
            "--b\r\n",
            "Content-Type: image/png\r\n",
            "Content-ID: <img1>\r\n",
            "Content-Disposition: inline; filename=\"pic.png\"\r\n",
            "Content-Transfer-Encoding: base64\r\n",
            "\r\n",
            "iVBORw0KGgo=\r\n",
            "--b--\r\n",
        );
        std::fs::write(mail.join("rmsams-gmail.com/cur/msg1"), raw).unwrap();

        decompile("m1@x", false, false, Some("wikilink"), &cfg).unwrap();

        let hash = crate::util::hash_message_id("m1@x");
        let note = std::fs::read_to_string(snapshots.join(format!("{hash}.md"))).unwrap();
        assert!(note.contains(&format!("![[cid-{hash}-pic.png]]")));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn decompile_rejects_non_message_input() {
        let dir = std::env::temp_dir().join(format!("mdmc-reject-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let draft = dir.join("draft.md");
        std::fs::write(&draft, "---\ntitle: x\ntype: mail-draft\n---\n\nbody\n").unwrap();

        let cfg = Config::default();
        let err = decompile(draft.to_str().unwrap(), true, false, None, &cfg).unwrap_err();
        assert!(err.contains("not a valid RFC 5322 message"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
