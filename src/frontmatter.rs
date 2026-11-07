use regex::Regex;
use serde::Deserialize;

/// A YAML field that may be a single string or a list of strings.
#[derive(Debug, Clone, Default)]
pub struct StringList(pub Vec<String>);

impl<'de> Deserialize<'de> for StringList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            One(String),
            Many(Vec<String>),
        }
        Ok(match Repr::deserialize(deserializer)? {
            Repr::One(s) => StringList(vec![s]),
            Repr::Many(v) => StringList(v),
        })
    }
}

/// The frontmatter fields `write` reads from a draft note. Unknown fields
/// (title, type, status, message-id, …) are ignored.
#[derive(Debug, Default, Deserialize)]
pub struct Draft {
    pub account: Option<String>,
    #[serde(rename = "to", default)]
    pub to: StringList,
    #[serde(rename = "cc", default)]
    pub cc: StringList,
    #[serde(rename = "bcc", default)]
    pub bcc: StringList,
    pub subject: Option<String>,
    #[serde(rename = "in-reply-to", default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub css: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(rename = "lua-filter", default)]
    pub lua_filter: Option<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
}

impl Draft {
    pub fn format(&self) -> &str {
        self.format.as_deref().unwrap_or("text")
    }
}

/// Parse `---`-delimited YAML frontmatter.
///
/// Returns `(None, text)` when the text has no valid `---` frontmatter.
/// A present-but-unparseable frontmatter yields a default (empty) `Draft`
/// with the body, matching the Python tool's behaviour of failing later on
/// the missing required fields.
pub fn load(text: &str) -> (Option<Draft>, &str) {
    if !text.starts_with("---") {
        return (None, text);
    }
    let rest = &text[3..];
    match rest.chars().next() {
        Some(c) if c != '\n' && c != '\r' => return (None, text),
        None => return (None, text),
        _ => {}
    }

    let re = Regex::new(r"(?m)^---[ \t]*$").expect("static regex");
    let Some(m) = re.find(rest) else {
        return (None, text);
    };

    let yaml_text = rest[..m.start()].trim_end_matches(['\n', '\r']);
    let body = &rest[m.end()..];
    let draft = serde_yaml::from_str::<Draft>(yaml_text).unwrap_or_default();
    (Some(draft), body)
}

/// Rewrite scalar frontmatter fields in place, preserving all other content
/// and formatting. A key already present has its value replaced; a missing key
/// is appended before the closing `---`. Values are written as plain scalars.
pub fn set_fields(text: &str, fields: &[(&str, &str)]) -> String {
    let Some(closing) = closing_line_index(text) else {
        return text.to_string();
    };
    let lines: Vec<&str> = text.split('\n').collect();

    let mut seen = vec![false; fields.len()];
    let mut body: Vec<String> = Vec::new();
    for line in &lines[1..closing] {
        let mut replaced = false;
        for (i, (key, value)) in fields.iter().enumerate() {
            if line.starts_with(&format!("{key}:")) {
                body.push(format!("{key}: {value}"));
                seen[i] = true;
                replaced = true;
                break;
            }
        }
        if !replaced {
            body.push((*line).to_string());
        }
    }
    for (i, (key, value)) in fields.iter().enumerate() {
        if !seen[i] {
            body.push(format!("{key}: {value}"));
        }
    }

    let mut out = String::from("---\n");
    out.push_str(&body.join("\n"));
    out.push('\n');
    out.push_str(&lines[closing..].join("\n"));
    out
}

fn closing_line_index(text: &str) -> Option<usize> {
    if !text.starts_with("---") {
        return None;
    }
    for (i, line) in text.split('\n').enumerate().skip(1) {
        if line.trim_end() == "---" {
            return Some(i);
        }
    }
    None
}
