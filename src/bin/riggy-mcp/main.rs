//! `riggy-mcp`: stdio MCP server with one tool, `speak(text, language)`, that reads a short text aloud.
//!
//! Speech comes from the Qwen3-TTS server (`riggy::speech_server`) and is played by `riggy::speaker`: if the avatar runs, it plays the audio and lip-syncs to it; otherwise `pw-play` plays it. `speak` returns at once; texts are queued, each plays to its end, with a pause before the next one. Voice and tempo come from the tray's settings (`riggy::settings`), read for every text, and nothing is spoken while the tray has speech switched off. With "Start with the MCP server" chosen in the tray, the server starts the tray (and so the avatar) if it does not run yet.

use std::io::{BufRead, Write};

use riggy::settings::{self, Settings};
use riggy::speaker::{Player, Speech};
use riggy::{speech_server, tray_process};
use serde_json::{Value, json};

const PROTOCOL_VERSION: &str = "2025-06-18";
const LANGUAGES: [&str; 3] = ["German", "English", "auto"];

fn main() {
    if Settings::load().start_with_mcp
        && !tray_process::tray_running()
        && let Err(err) = tray_process::start_tray()
    {
        eprintln!("riggy-mcp: cannot start riggy-tray: {err}");
    }
    let player = Player::start();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // Notifications carry no id and get no reply.
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let reply = match handle(&message, &player) {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": error } }),
        };
        if writeln!(stdout, "{reply}").and_then(|()| stdout.flush()).is_err() {
            break;
        }
    }
    player.stop();
}

/// The result of one request, or the error message for an unknown method or tool.
fn handle(message: &Value, player: &Player) -> Result<Value, String> {
    let method = message["method"].as_str().unwrap_or_default();
    let params = &message["params"];
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSION),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "riggy", "version": env!("CARGO_PKG_VERSION") },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": [tool()] })),
        "tools/call" => match params["name"].as_str() {
            Some("speak") => Ok(json!({ "content": [{ "type": "text", "text": speak(&params["arguments"], player) }] })),
            other => Err(format!("Unknown tool: {}", other.unwrap_or_default())),
        },
        _ => Err(format!("Unknown method: {method}")),
    }
}

fn tool() -> Value {
    json!({
        "name": "speak",
        "description": "Read a very short summary aloud on the user's speakers. Returns immediately. Pass one or two plain spoken sentences, with hardly any numbers and no Markdown, code, tables or URLs.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "Plain text to speak." },
                "language": { "type": "string", "enum": LANGUAGES, "description": "Language of the text: the language of the message it summarises." },
            },
            "required": ["text", "language"],
        },
    })
}

/// The status text for the agent; failing to speak must never look like a failed turn.
fn speak(arguments: &Value, player: &Player) -> String {
    let text = arguments["text"].as_str().unwrap_or_default().trim();
    let Some(language) = arguments["language"].as_str() else {
        return format!("No language given. Use one of: {}.", LANGUAGES.join(", "));
    };
    if text.is_empty() {
        return "Nothing to speak: text is empty.".into();
    }
    if !LANGUAGES.contains(&language) {
        return format!("Unknown language {language:?}. Use one of: {}.", LANGUAGES.join(", "));
    }
    if settings::speech_off() {
        return "TTS is switched off. Nothing was spoken.".into();
    }
    let url = speech_server::url();
    if !speech_server::up(&url) {
        return format!("TTS server not reachable at {url}. Nothing was spoken.");
    }
    player.play(Speech { text: text.into(), language: language.into() });
    "Speaking.".into()
}
