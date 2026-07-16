//! `/a2a-server`: a real A2A (Agent2Agent) server, exposing a real, live [`ConsoleSession`] as an
//! Agent Card + one real JSON-RPC method (docs/998-roadmap.md's Social pillar). Deliberately a
//! narrow, honest subset of the real spec (<https://a2a-protocol.org>): the Agent Card is served
//! at the real, spec-defined well-known path (`/.well-known/agent-card.json`), and only
//! `SendMessage` is implemented (the spec's own "send a message and get a reply" minimal flow --
//! `returnImmediately: false`, one synchronous `Task` back, `TASK_STATE_COMPLETED` since this
//! session never streams). `GetTask`/`ListTasks`/streaming/push notifications are not
//! implemented -- there's no real task store here, since every real dispatch through
//! [`ConsoleSession::handle_utterance`] already completes synchronously before this returns.
//!
//! **Real identity (docs/998-roadmap.md's Social pillar, "identity" half):** the Agent Card now
//! carries this session's own real, hex-encoded Ed25519 [`ConsoleSession::verifying_key`], and
//! every `SendMessage` reply is really signed over with [`ConsoleSession::sign`]. A real caller
//! ([`send_message`]) verifies that signature against the presented key (proving whoever replied
//! genuinely holds the matching private key) and checks the key against a real, persisted
//! [`hyperion_console::peer_trust::PeerTrustStore`] (proving it's the *same* key this caller has
//! always seen for this peer) before ever showing the reply text.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyperion_console::peer_trust::{decode_hex, encode_hex, PeerTrustStore, TrustOutcome};
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
    let verifying_key_hex = encode_hex(&session.lock().unwrap().verifying_key().to_bytes());
    http_server::spawn(port, move |method, path, body| match (method, path) {
        ("GET", "/.well-known/agent-card.json") => (
            200,
            "application/json",
            agent_card(&verifying_key_hex).to_string(),
        ),
        ("POST", _) => (200, "application/json", handle_request(&session, body)),
        _ => (
            404,
            "application/json",
            json!({"error": "not found"}).to_string(),
        ),
    })
}

fn agent_card(verifying_key_hex: &str) -> Value {
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
        // Not part of the real A2A spec -- an additive, real proof of identity this Agent Card
        // also happens to be a convenient place to publish (see this module's own doc comment).
        "publicKey": verifying_key_hex,
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

            let (reply, signature_hex) = {
                let mut session = session.lock().unwrap();
                let reply = session.handle_utterance(text).join("\n");
                let signature = encode_hex(&session.sign(reply.as_bytes()).to_bytes());
                (reply, signature)
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
                    // Not part of the real A2A spec -- see this module's own doc comment.
                    "signature": signature_hex,
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
///
/// **Real identity check**, only when the peer's own Agent Card advertises one (a real,
/// non-Hyperion A2A server that doesn't carry this proprietary extension is neither penalized
/// nor silently trusted -- it's simply outside what this check can verify, exactly like SSH
/// falls back to "unknown host" rather than refusing to ever talk to a server with no host key
/// at all): the reply's own real signature must verify against the card's claimed public key
/// (proof the responder genuinely holds the matching private key), and that key must match
/// `trust_store`'s own record for `host:port` (proof it's the *same* peer as last time -- see
/// [`hyperion_console::peer_trust`]'s own doc comment). A key that verifies but doesn't match the
/// trust store's record is a hard failure: the reply is never returned, only the warning.
pub fn send_message(
    host: &str,
    port: u16,
    text: &str,
    trust_store: &mut PeerTrustStore,
) -> Result<String, String> {
    let card_body = crate::http_client::get(host, port, "/.well-known/agent-card.json")?;
    let card: Value = serde_json::from_str(&card_body)
        .map_err(|e| format!("that endpoint's Agent Card wasn't valid JSON: {e}"))?;
    let claimed_key_hex = card.get("publicKey").and_then(|k| k.as_str());

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
    let reply = response
        .get("result")
        .and_then(|r| r.get("status"))
        .and_then(|s| s.get("message"))
        .and_then(|m| m.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or(&response_body)
        .to_string();

    let Some(claimed_key_hex) = claimed_key_hex else {
        // No identity claim to check at all -- a real, non-Hyperion A2A server. Same trust model
        // as before this slice existed.
        return Ok(reply);
    };
    let signature_hex = response
        .get("result")
        .and_then(|r| r.get("signature"))
        .and_then(|s| s.as_str());
    let Some(signature_hex) = signature_hex else {
        return Err(format!(
            "{host}:{port} claims identity {claimed_key_hex} in its Agent Card but its reply \
             carried no signature to prove it -- refusing to trust an unproven claim"
        ));
    };

    let verifying_key_bytes = decode_hex(claimed_key_hex)
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

    if !hyperion_crypto::verify(reply.as_bytes(), &signature, &verifying_key) {
        return Err(format!(
            "{host}:{port}'s reply signature does NOT verify against its own claimed public \
             key -- this reply may not really be from the identity it claims. Refusing to \
             show it."
        ));
    }

    let peer_id = format!("{host}:{port}");
    match trust_store
        .verify_or_trust(&peer_id, claimed_key_hex)
        .map_err(|e| format!("couldn't check {peer_id}'s trust record: {e}"))?
    {
        TrustOutcome::FirstTrust => Ok(format!(
            "{reply}\n\n(Trusting {peer_id}'s identity for the first time: {claimed_key_hex}.)"
        )),
        TrustOutcome::Trusted => Ok(reply),
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
