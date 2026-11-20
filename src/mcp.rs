use std::io::{BufRead, Write};

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::Config;

const PROTOCOL_VERSION: &str = "2024-11-05";

const EMBEDDED_INSTRUCTIONS: &str = "\
mdmc composes and reads email. Every tool is save-only: it writes a markdown \
draft to the configured drafts directory or a snapshot to the snapshots \
directory, and never sends. Review drafts in neomutt and send from there. \
Account names are the [accounts] keys in the mdmc config. The reply and \
forward tools need a message-id, which the search tool returns.";

// ---------------------------------------------------------------------------
// Tool argument structs (serde for parsing, schemars for the derived schema)
// ---------------------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
struct ComposeArgs {
    account: Option<String>,
    to: Option<String>,
    cc: Option<String>,
    bcc: Option<String>,
    subject: Option<String>,
    body: Option<String>,
    attach: Option<Vec<String>>,
    html: Option<bool>,
    css: Option<String>,
    template: Option<String>,
    lua_filter: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ReplyArgs {
    account: Option<String>,
    message_id: String,
    reply_all: Option<bool>,
    extra_body: Option<String>,
    attach: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
struct ForwardArgs {
    account: Option<String>,
    message_id: String,
    to: Option<String>,
    extra_body: Option<String>,
    forward_attach: Option<bool>,
    attach: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
struct WriteArgs {
    draft: String,
    stdout: Option<bool>,
    folder: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct DecompileArgs {
    message_id: String,
    stdout: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct ViewArgs {
    message_id: String,
    body_only: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchArgs {
    query: String,
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct AddressArgs {
    query: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ListAccountsArgs {}

// ---------------------------------------------------------------------------
// Tool execution
// ---------------------------------------------------------------------------

fn run_compose(args: ComposeArgs, cfg: &Config) -> Result<String, String> {
    let format = if args.html == Some(true) {
        Some("html".to_string())
    } else {
        None
    };
    crate::new::new(
        cfg,
        args.account,
        args.to,
        args.cc,
        args.bcc,
        args.subject,
        None,
        None,
        None,
        None,
        args.attach.unwrap_or_default(),
        args.body,
        format,
        args.css,
        args.template,
        args.lua_filter,
        None,
        false,
        false,
        false,
        false,
    )
}

fn run_reply(args: ReplyArgs, cfg: &Config) -> Result<String, String> {
    let (reply, reply_all) = if args.reply_all.unwrap_or(false) {
        (None, Some(args.message_id))
    } else {
        (Some(args.message_id), None)
    };
    crate::new::new(
        cfg,
        args.account,
        None,
        None,
        None,
        None,
        reply,
        reply_all,
        None,
        None,
        args.attach.unwrap_or_default(),
        args.extra_body,
        None,
        None,
        None,
        None,
        None,
        false,
        false,
        false,
        false,
    )
}

fn run_forward(args: ForwardArgs, cfg: &Config) -> Result<String, String> {
    let (forward, forward_attach) = if args.forward_attach.unwrap_or(false) {
        (None, Some(args.message_id))
    } else {
        (Some(args.message_id), None)
    };
    crate::new::new(
        cfg,
        args.account,
        args.to,
        None,
        None,
        None,
        None,
        None,
        forward,
        forward_attach,
        args.attach.unwrap_or_default(),
        args.extra_body,
        None,
        None,
        None,
        None,
        None,
        false,
        false,
        false,
        false,
    )
}

fn list_accounts(cfg: &Config) -> Result<String, String> {
    let accounts: Vec<Value> = cfg
        .accounts
        .iter()
        .map(|(name, acct)| {
            json!({
                "name": name,
                "from": acct.from.as_deref(),
                "realname": acct.realname.as_deref(),
                "drafts": acct.drafts.as_deref(),
                "sent": acct.sent.as_deref(),
            })
        })
        .collect();
    Ok(format!(
        "{}\n",
        serde_json::to_string(&accounts).map_err(|e| format!("json: {e}"))?
    ))
}

// ---------------------------------------------------------------------------
// Tool registry
// ---------------------------------------------------------------------------

fn tool_def(name: &str, description: &str, input_schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": input_schema })
}

fn schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("schema serializes")
}

fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "compose",
            "Compose a new email and save it as a markdown draft. Save-only, never sends.",
            schema::<ComposeArgs>(),
        ),
        tool_def(
            "reply",
            "Compose a reply to an existing message and save it as a markdown draft (mdmc resolves the original and fills the envelope). Save-only, never sends.",
            schema::<ReplyArgs>(),
        ),
        tool_def(
            "forward",
            "Compose a forward of an existing message and save it as a markdown draft. Save-only, never sends.",
            schema::<ForwardArgs>(),
        ),
        tool_def(
            "write",
            "Compile a draft to an RFC 5322 message: to the account's drafts maildir, a folder, or stdout. Save-only, never sends.",
            schema::<WriteArgs>(),
        ),
        tool_def(
            "decompile",
            "Snapshot a message into the vault as a markdown note (the read side). Save-only, never sends.",
            schema::<DecompileArgs>(),
        ),
        tool_def(
            "view",
            "Print a message as markdown (terminal-reader mode; no attachment extraction).",
            schema::<ViewArgs>(),
        ),
        tool_def(
            "search",
            "Search messages via notmuch; returns matching message-ids with date, subject, and from.",
            schema::<SearchArgs>(),
        ),
        tool_def(
            "address",
            "Look up addresses from messages you have sent (notmuch address).",
            schema::<AddressArgs>(),
        ),
        tool_def(
            "list_accounts",
            "List the configured mdmc accounts.",
            schema::<ListAccountsArgs>(),
        ),
    ]
}

fn deserialize<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T, String> {
    serde_json::from_value(args.clone()).map_err(|e| format!("invalid arguments: {e}"))
}

fn call_tool(name: &str, args: &Value, cfg: &Config) -> Value {
    let result: Result<String, String> = match name {
        "compose" => deserialize::<ComposeArgs>(args).and_then(|a| run_compose(a, cfg)),
        "reply" => deserialize::<ReplyArgs>(args).and_then(|a| run_reply(a, cfg)),
        "forward" => deserialize::<ForwardArgs>(args).and_then(|a| run_forward(a, cfg)),
        "write" => deserialize::<WriteArgs>(args).and_then(|a| {
            crate::compile::compile(a.stdout.unwrap_or(false), a.folder, false, &a.draft, cfg)
        }),
        "decompile" => deserialize::<DecompileArgs>(args).and_then(|a| {
            crate::decompile::decompile(&a.message_id, a.stdout.unwrap_or(false), false, None, cfg)
        }),
        "view" => deserialize::<ViewArgs>(args)
            .and_then(|a| crate::view::view(&a.message_id, a.body_only.unwrap_or(false), cfg)),
        "search" => deserialize::<SearchArgs>(args)
            .and_then(|a| crate::search::search(&a.query, a.limit, true)),
        "address" => deserialize::<AddressArgs>(args)
            .and_then(|a| crate::search::address(a.query.as_deref(), true, cfg)),
        "list_accounts" => list_accounts(cfg),
        other => return tool_error(format!("unknown tool '{other}'")),
    };
    match result {
        Ok(text) => tool_result(text),
        Err(msg) => tool_error(msg),
    }
}

fn tool_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": false })
}

fn tool_error(msg: String) -> Value {
    json!({ "content": [{ "type": "text", "text": msg }], "isError": true })
}

// ---------------------------------------------------------------------------
// Server loop
// ---------------------------------------------------------------------------

fn instructions(cfg: &Config) -> String {
    let mut out = String::from(EMBEDDED_INSTRUCTIONS);
    if let Some(raw) = cfg.mcp_instructions() {
        let extra = resolve_instructions(raw);
        if !extra.is_empty() {
            out.push_str("\n\n");
            out.push_str(&extra);
        }
    }
    out
}

fn resolve_instructions(raw: &str) -> String {
    let path = crate::config::expand_home(raw);
    if path.is_file() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        raw.to_string()
    }
}

fn write_response(out: &mut impl Write, id: Option<Value>, result: Value) -> Result<(), String> {
    writeln!(
        out,
        "{}",
        json!({ "jsonrpc": "2.0", "id": id, "result": result })
    )
    .map_err(|e| format!("stdout write failed: {e}"))?;
    out.flush().map_err(|e| format!("stdout flush failed: {e}"))
}

fn write_error(
    out: &mut impl Write,
    id: Option<Value>,
    code: i64,
    message: &str,
) -> Result<(), String> {
    writeln!(
        out,
        "{}",
        json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
    )
    .map_err(|e| format!("stdout write failed: {e}"))?;
    out.flush().map_err(|e| format!("stdout flush failed: {e}"))
}

/// Serve the MCP stdio protocol: read newline-delimited JSON-RPC requests from
/// stdin and write responses to stdout until EOF.
pub fn serve(cfg: &Config) -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("stdin read failed: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_error(
                    &mut stdout,
                    Some(Value::Null),
                    -32700,
                    &format!("parse error: {e}"),
                )?;
                continue;
            }
        };
        let id = req.get("id").cloned();
        let Some(method) = req.get("method").and_then(Value::as_str) else {
            write_error(&mut stdout, id, -32600, "invalid request")?;
            continue;
        };

        match method {
            "initialize" => {
                let result = json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "mdmc", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": instructions(cfg),
                });
                write_response(&mut stdout, id, result)?;
            }
            "notifications/initialized" => {}
            "tools/list" => {
                write_response(&mut stdout, id, json!({ "tools": tool_definitions() }))?;
            }
            "tools/call" => {
                let name = req
                    .get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let args = req
                    .get("params")
                    .and_then(|p| p.get("arguments"))
                    .cloned()
                    .unwrap_or(Value::Object(serde_json::Map::new()));
                write_response(&mut stdout, id, call_tool(name, &args, cfg))?;
            }
            "ping" => {
                write_response(&mut stdout, id, json!({}))?;
            }
            other => {
                write_error(
                    &mut stdout,
                    id,
                    -32601,
                    &format!("method not found: {other}"),
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_expose_all_tools() {
        let tools = tool_definitions();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "compose",
                "reply",
                "forward",
                "write",
                "decompile",
                "view",
                "search",
                "address",
                "list_accounts"
            ]
        );
    }

    #[test]
    fn list_accounts_returns_configured_accounts() {
        let cfg = Config {
            accounts: std::collections::BTreeMap::from([(
                "a".to_string(),
                crate::config::Account {
                    from: Some("a@x".to_string()),
                    realname: Some("A B".to_string()),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        let parsed: Value = serde_json::from_str(&list_accounts(&cfg).unwrap()).unwrap();
        assert_eq!(parsed[0]["name"], "a");
        assert_eq!(parsed[0]["from"], "a@x");
        assert_eq!(parsed[0]["realname"], "A B");
    }

    #[test]
    fn call_tool_unknown_tool_is_error() {
        let result = call_tool("nope", &json!({}), &Config::default());
        assert_eq!(result["isError"], true);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unknown tool"));
    }
}
