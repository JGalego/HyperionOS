//! `/mcp-server`: a real MCP (Model Context Protocol) server, exposing a real, live
//! [`ConsoleSession`] as a small set of real tools over MCP's JSON-RPC 2.0 wire format
//! (AUTONOMY_ROADMAP.md's Social pillar -- "being callable" over a real, known protocol, before
//! "calling others"). Deliberately a narrow, honest subset: three real methods (`initialize`,
//! `tools/list`, `tools/call`) over HTTP (MCP's "Streamable HTTP" transport, request/response
//! only -- no SSE streaming upgrade), not the full MCP surface (no resources, prompts,
//! notifications, or stdio transport). Every tool call is a real turn through the exact same
//! [`ConsoleSession::handle_utterance`] path everything else in this crate already uses -- no new
//! bypass of the capability/consent model; an MCP client genuinely drives the real Intent Engine,
//! real Agent dispatch, real Knowledge Graph writes.

use std::sync::{Arc, Mutex};

use hyperion_console::ConsoleSession;
use serde_json::{json, Value};

use crate::http_server::{self, RunningServer};

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Starts the real server in a real background thread; returns immediately with a handle the
/// caller can read the real bound address from (or [`RunningServer::stop`], used by this module's
/// own tests) -- the console's own stdio stays free for the rest of the session.
pub fn spawn_server(
    session: Arc<Mutex<ConsoleSession>>,
    port: u16,
) -> std::io::Result<RunningServer> {
    http_server::spawn(port, move |method, _path, body| {
        if method != "POST" {
            return (
                404,
                "application/json",
                json!({"error": "POST a real JSON-RPC 2.0 request to this endpoint"}).to_string(),
            );
        }
        (200, "application/json", handle_request(&session, body))
    })
}

fn handle_request(session: &Arc<Mutex<ConsoleSession>>, body: &str) -> String {
    let request: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return error_response(Value::Null, -32700, &format!("parse error: {e}")),
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    match method {
        "initialize" => success_response(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "hyperion-console", "version": env!("CARGO_PKG_VERSION")},
            }),
        ),
        "tools/list" => success_response(id, json!({"tools": tool_definitions()})),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or(Value::Null);
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match dispatch_tool(session, name, &arguments) {
                Ok(text) => success_response(
                    id,
                    json!({"content": [{"type": "text", "text": text}], "isError": false}),
                ),
                Err(message) => success_response(
                    id,
                    json!({"content": [{"type": "text", "text": message}], "isError": true}),
                ),
            }
        }
        other => error_response(id, -32601, &format!("method not found: {other}")),
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "hyperion.ask",
            "description": "Ask Hyperion anything -- a real utterance through its real Intent Engine and Agent dispatch, not a canned reply.",
            "inputSchema": {
                "type": "object",
                "properties": {"prompt": {"type": "string"}},
                "required": ["prompt"],
            },
        },
        {
            "name": "hyperion.recall",
            "description": "Look through what this Hyperion session has recorded so far (bare, for everything recent).",
            "inputSchema": {
                "type": "object",
                "properties": {"query": {"type": "string"}},
            },
        },
        {
            "name": "hyperion.graph",
            "description": "Dump this Hyperion session's whole recorded knowledge graph -- real nodes and edges, sorted by id so two dumps of an unchanged graph are identical.",
            "inputSchema": {
                "type": "object",
                "properties": {"format": {"type": "string", "enum": ["text", "dot"]}},
            },
        },
    ])
}

fn dispatch_tool(
    session: &Arc<Mutex<ConsoleSession>>,
    name: &str,
    arguments: &Value,
) -> Result<String, String> {
    let utterance = match name {
        "hyperion.ask" => {
            let prompt = arguments
                .get("prompt")
                .and_then(|v| v.as_str())
                .ok_or("hyperion.ask needs a 'prompt' argument")?;
            prompt.to_string()
        }
        "hyperion.recall" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!("/recall {query}")
        }
        "hyperion.graph" => {
            if arguments.get("format").and_then(|v| v.as_str()) == Some("dot") {
                "/graph dot".to_string()
            } else {
                "/graph".to_string()
            }
        }
        other => return Err(format!("unknown tool: {other:?}")),
    };

    let mut session = session.lock().unwrap();
    Ok(session.handle_utterance(&utterance).join("\n"))
}

fn success_response(id: Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

/// The real outbound half: calling a real, already-known MCP endpoint's `tools/call` -- another
/// Hyperion instance's own `/mcp-server`, or any other real MCP-over-HTTP server. Not discovery
/// (the caller names the host/port), and not the full MCP client handshake (no `initialize`
/// round trip first) -- a deliberately narrow, honest subset, matching the server side's own
/// scope. See `crate::http_client` for the real HTTP transport this rides on.
pub fn call_tool(
    host: &str,
    port: u16,
    tool_name: &str,
    arguments: Value,
) -> Result<String, String> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": tool_name, "arguments": arguments},
    });
    let response_body = crate::http_client::post(host, port, "/", &request.to_string())?;
    let response: Value = serde_json::from_str(&response_body)
        .map_err(|e| format!("the response wasn't valid JSON: {e} (got: {response_body:?})"))?;
    if let Some(error) = response.get("error") {
        return Err(format!("the remote server returned a real error: {error}"));
    }
    let text = response
        .get("result")
        .and_then(|r| r.get("content"))
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or(&response_body)
        .to_string();
    Ok(text)
}
