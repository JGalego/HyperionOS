//! `/mcp-server`: a real MCP (Model Context Protocol) server, exposing a real, live
//! [`ConsoleSession`] as a small set of real tools over MCP's JSON-RPC 2.0 wire format
//! (docs/998-roadmap.md's Social pillar -- "being callable" over a real, known protocol, before
//! "calling others"). Deliberately a narrow, honest subset: three real methods (`initialize`,
//! `tools/list`, `tools/call`) over HTTP (MCP's "Streamable HTTP" transport, request/response
//! only -- no SSE streaming upgrade), not the full MCP surface (no resources, prompts,
//! notifications, or stdio transport). Every tool call is a real turn through the exact same
//! [`ConsoleSession::handle_utterance`] path everything else in this crate already uses -- no new
//! bypass of the capability/consent model; an MCP client genuinely drives the real Intent Engine,
//! real Agent dispatch, real Knowledge Graph writes.
//!
//! **Real identity (docs/998-roadmap.md's Social pillar), the same shape `crate::a2a` already
//! established:** `initialize`'s response now carries this session's own real, hex-encoded
//! Ed25519 [`ConsoleSession::verifying_key`], and every `tools/call` response is really signed
//! over with [`ConsoleSession::sign`]. [`call_tool`] performs a real `initialize` round trip
//! first (closing this module's own previously-named "no real client handshake" gap) purely to
//! fetch that key, verifies each call's signature against it, and checks it against a real,
//! persisted [`hyperion_console::peer_trust::PeerTrustStore`] -- the identical trust-on-first-use
//! model `crate::a2a::send_message` uses, applied here instead of duplicated as a new one.

use std::sync::{Arc, Mutex};

use hyperion_console::peer_trust::{decode_hex, encode_hex, PeerTrustStore, TrustOutcome};
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
    let verifying_key_hex = encode_hex(&session.lock().unwrap().verifying_key().to_bytes());
    http_server::spawn(port, move |method, _path, body| {
        if method != "POST" {
            return (
                404,
                "application/json",
                json!({"error": "POST a real JSON-RPC 2.0 request to this endpoint"}).to_string(),
            );
        }
        (
            200,
            "application/json",
            handle_request(&session, body, &verifying_key_hex),
        )
    })
}

fn handle_request(
    session: &Arc<Mutex<ConsoleSession>>,
    body: &str,
    verifying_key_hex: &str,
) -> String {
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
                // Not part of the real MCP spec -- see this module's own doc comment.
                "publicKey": verifying_key_hex,
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
                Ok(text) => {
                    let signature_hex =
                        encode_hex(&session.lock().unwrap().sign(text.as_bytes()).to_bytes());
                    success_response(
                        id,
                        json!({
                            "content": [{"type": "text", "text": text}],
                            "isError": false,
                            // Not part of the real MCP spec -- see this module's own doc comment.
                            "signature": signature_hex,
                        }),
                    )
                }
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
/// (the caller names the host/port). Performs a real `initialize` round trip first -- purely to
/// fetch a real identity claim, if the peer makes one (see this module's own doc comment) -- so
/// this is now a real, if still narrow, two-call client handshake, not the single bare
/// `tools/call` this function used to send alone.
///
/// **Real identity check**, only when `initialize`'s response claims one -- a real, non-Hyperion
/// MCP server is neither penalized nor silently trusted, exactly like `crate::a2a::send_message`'s
/// own identical check: the reply's signature must verify against the claimed key, and that key
/// must match `trust_store`'s own record for `host:port`. A key that verifies but doesn't match
/// the trust store's record is a hard failure -- the reply is never returned, only the warning.
pub fn call_tool(
    host: &str,
    port: u16,
    tool_name: &str,
    arguments: Value,
    trust_store: &mut PeerTrustStore,
) -> Result<String, String> {
    let init_response_body = crate::http_client::post(
        host,
        port,
        "/",
        &json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {}}).to_string(),
    )?;
    let init_response: Value = serde_json::from_str(&init_response_body).map_err(|e| {
        format!("the initialize response wasn't valid JSON: {e} (got: {init_response_body:?})")
    })?;
    let claimed_key_hex = init_response
        .get("result")
        .and_then(|r| r.get("publicKey"))
        .and_then(|k| k.as_str())
        .map(str::to_string);

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

    let Some(claimed_key_hex) = claimed_key_hex else {
        return Ok(text);
    };
    let signature_hex = response
        .get("result")
        .and_then(|r| r.get("signature"))
        .and_then(|s| s.as_str());
    let Some(signature_hex) = signature_hex else {
        return Err(format!(
            "{host}:{port} claims identity {claimed_key_hex} in its initialize response but its \
             tools/call reply carried no signature to prove it -- refusing to trust an unproven \
             claim"
        ));
    };

    let verifying_key_bytes = decode_hex(&claimed_key_hex)
        .ok_or_else(|| format!("{host}:{port}'s claimed public key isn't valid hex"))?;
    let verifying_key = hyperion_crypto::VerifyingKey::try_from(verifying_key_bytes.as_slice())
        .map_err(|e| {
            format!("{host}:{port}'s claimed public key isn't a valid Ed25519 key: {e}")
        })?;
    let signature_bytes = decode_hex(signature_hex)
        .ok_or_else(|| format!("{host}:{port}'s reply signature isn't valid hex"))?;
    let signature =
        hyperion_crypto::Signature::try_from(signature_bytes.as_slice()).map_err(|e| {
            format!("{host}:{port}'s reply signature isn't a valid Ed25519 signature: {e}")
        })?;

    if !hyperion_crypto::verify(text.as_bytes(), &signature, &verifying_key) {
        return Err(format!(
            "{host}:{port}'s reply signature does NOT verify against its own claimed public \
             key -- this reply may not really be from the identity it claims. Refusing to \
             show it."
        ));
    }

    let peer_id = format!("{host}:{port}");
    match trust_store
        .verify_or_trust(&peer_id, &claimed_key_hex)
        .map_err(|e| format!("couldn't check {peer_id}'s trust record: {e}"))?
    {
        TrustOutcome::FirstTrust => Ok(format!(
            "{text}\n\n(Trusting {peer_id}'s identity for the first time: {claimed_key_hex}.)"
        )),
        TrustOutcome::Trusted => Ok(text),
        TrustOutcome::KeyMismatch {
            previously_trusted_key_hex,
        } => Err(format!(
            "WARNING: {peer_id} just presented a DIFFERENT identity than before!\n  \
             previously trusted: {previously_trusted_key_hex}\n  just presented: \
             {claimed_key_hex}\nThis could mean the peer was reinstalled, or that something \
             else is impersonating it. Refusing to show its reply. If you're sure this is \
             expected, use \"/trust forget {peer_id}\" and try again."
        )),
    }
}
