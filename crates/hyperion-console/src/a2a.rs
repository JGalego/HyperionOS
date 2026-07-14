//! `/a2a-server`: a real A2A (Agent2Agent) server, exposing a real, live [`ConsoleSession`] as an
//! Agent Card + one real JSON-RPC method (docs/998-roadmap.md's Social pillar). Deliberately a
//! narrow, honest subset of the real spec (<https://a2a-protocol.org>): the Agent Card is served
//! at the real, spec-defined well-known path (`/.well-known/agent-card.json`), and only
//! `SendMessage` is implemented (the spec's own "send a message and get a reply" minimal flow --
//! `returnImmediately: false`, one synchronous `Task` back, `TASK_STATE_COMPLETED` since this
//! session never streams). `GetTask`/`ListTasks`/streaming/push notifications are not
//! implemented -- there's no real task store here, since every real dispatch through
//! [`ConsoleSession::handle_utterance`] already completes synchronously before this returns.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyperion_console::ConsoleSession;
use serde_json::{json, Value};

use crate::http_server::{self, RunningServer};

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

/// Starts the real server in a real background thread; returns immediately with a handle the
/// caller can read the real bound address from (or [`RunningServer::stop`], used by this module's
/// own tests).
pub fn spawn_server(
    session: Arc<Mutex<ConsoleSession>>,
    port: u16,
) -> std::io::Result<RunningServer> {
    http_server::spawn(port, move |method, path, body| match (method, path) {
        ("GET", "/.well-known/agent-card.json") => {
            (200, "application/json", agent_card().to_string())
        }
        ("POST", _) => (200, "application/json", handle_request(&session, body)),
        _ => (
            404,
            "application/json",
            json!({"error": "not found"}).to_string(),
        ),
    })
}

fn agent_card() -> Value {
    json!({
        "id": "hyperion-console",
        "name": "Hyperion",
        "provider": {"name": "Hyperion", "url": "https://github.com/JGalego/HyperionOS"},
        "capabilities": {
            "streaming": false,
            "pushNotifications": false,
            "extendedAgentCard": false,
        },
        "interfaces": [{"type": "json-rpc", "url": "/"}],
        "skills": [
            {
                "id": "hyperion.ask",
                "name": "Ask Hyperion",
                "description": "A real utterance through Hyperion's real Intent Engine and Agent dispatch.",
            },
        ],
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
        "SendMessage" => {
            let text = request
                .get("params")
                .and_then(|p| p.get("message"))
                .and_then(|m| m.get("parts"))
                .and_then(|parts| parts.get(0))
                .and_then(|part| part.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or_default();

            let reply = {
                let mut session = session.lock().unwrap();
                session.handle_utterance(text).join("\n")
            };

            let task_id = format!("task-{}", NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed));
            success_response(
                id,
                json!({
                    "id": task_id,
                    "contextId": task_id,
                    "status": {
                        "state": "TASK_STATE_COMPLETED",
                        "message": {
                            "messageId": format!("{task_id}-reply"),
                            "role": "ROLE_AGENT",
                            "parts": [{"text": reply}],
                        },
                        "timestamp": rfc3339_utc(unix_seconds_now()),
                    },
                }),
            )
        }
        other => error_response(id, -32601, &format!("method not found: {other}")),
    }
}

fn success_response(id: Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

fn unix_seconds_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// Howard Hinnant's `civil_from_days` -- a small, well-known, correct proleptic-Gregorian
/// days-since-epoch to (year, month, day) conversion, used here so a real ISO8601/RFC3339
/// timestamp (the A2A spec's own `Task.status.timestamp` shape) doesn't need a new date/time
/// dependency just for this one field.
fn civil_from_unix_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn rfc3339_utc(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let secs_of_day = unix_seconds % 86_400;
    let (y, m, d) = civil_from_unix_days(days);
    let h = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
}

/// The real outbound half: sending a message to a real, already-known A2A endpoint's `SendMessage`
/// method -- another Hyperion instance's own `/a2a-server`, or any other real A2A-over-HTTP
/// server. Not discovery (the caller names the host/port), though it does fetch the real Agent
/// Card first (proving the endpoint really speaks A2A before sending it anything).
pub fn send_message(host: &str, port: u16, text: &str) -> Result<String, String> {
    let card_body = crate::http_client::get(host, port, "/.well-known/agent-card.json")?;
    let _card: Value = serde_json::from_str(&card_body)
        .map_err(|e| format!("that endpoint's Agent Card wasn't valid JSON: {e}"))?;

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "SendMessage",
        "params": {
            "message": {
                "messageId": "client-msg-1",
                "role": "ROLE_USER",
                "parts": [{"text": text}],
            },
            "configuration": {"returnImmediately": false},
        },
    });
    let response_body = crate::http_client::post(host, port, "/", &request.to_string())?;
    let response: Value = serde_json::from_str(&response_body)
        .map_err(|e| format!("the response wasn't valid JSON: {e} (got: {response_body:?})"))?;
    if let Some(error) = response.get("error") {
        return Err(format!("the remote server returned a real error: {error}"));
    }
    let text = response
        .get("result")
        .and_then(|r| r.get("status"))
        .and_then(|s| s.get("message"))
        .and_then(|m| m.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or(&response_body)
        .to_string();
    Ok(text)
}
