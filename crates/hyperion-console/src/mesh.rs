//! Real, many-instance Hyperion-to-Hyperion capability delegation (docs/998-roadmap.md's Social
//! pillar): a node that lacks a capability locally finds a real peer on the real LAN that
//! advertises it (via mDNS discovery + that peer's own real Agent Card, not a new discovery
//! protocol) and delegates to it over the real `/a2a-server`/`send_message` transport this crate
//! already has. This module is the shared plumbing both `/mesh-request` (a node asking for help)
//! and `a2a::handle_request`'s `SendMessage` arm (a node being asked) record events into, and what
//! `/mesh-dashboard`'s background refresh loop polls to build a live view of the whole mesh.
//!
//! **Events are live telemetry, not product data**: a capped, in-memory ring buffer, the same
//! shape `a2a::TaskStore` already uses -- nothing here needs to survive a restart the way
//! `peer_trust.json`/the knowledge-graph WAL do.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use crate::{discovery, http_client};

/// How many of the most recent mesh events a single node's own `MeshEventLog` keeps -- old enough
/// to still narrate "what just happened here" to a polling dashboard, bounded so a long-running
/// demo node doesn't grow this without limit.
const MAX_EVENTS: usize = 200;

/// A real, in-process, insertion-ordered record of this node's own mesh activity -- both sides:
/// what it discovered/delegated (from `/mesh-request`) and what it was asked to do (from
/// `a2a::handle_request`'s `SendMessage` arm). See this module's own doc comment on why this is
/// in-memory rather than file-backed.
#[derive(Default)]
pub struct MeshEventLog {
    events: Mutex<VecDeque<Value>>,
}

impl MeshEventLog {
    /// Records one real event: `kind` is one of `"Discovered"`, `"DelegationCompleted"`,
    /// `"DelegationFailed"`, or `"DelegationReceived"`; `peer` is the other side's `host:port`
    /// (or `"unknown"` when this node is the one receiving a request over a protocol that never
    /// authenticates its caller); `detail` is a short, human-readable summary.
    pub fn record(&self, kind: &str, peer: &str, detail: &str) {
        let mut events = self.events.lock().unwrap();
        events.push_back(json!({
            "ts": crate::a2a::rfc3339_utc(crate::a2a::unix_seconds_now()),
            "kind": kind,
            "peer": peer,
            "detail": detail,
        }));
        while events.len() > MAX_EVENTS {
            events.pop_front();
        }
    }

    /// The most recent `n` events, oldest first -- the order a scrolling log panel wants to
    /// append in.
    pub fn recent(&self, n: usize) -> Vec<Value> {
        let events = self.events.lock().unwrap();
        let mut newest_first: Vec<Value> = events.iter().rev().take(n).cloned().collect();
        newest_first.reverse();
        newest_first
    }
}

/// `GET /mesh/status`'s real response body: this node's own configured capabilities, which real
/// backend it's actually dispatching through (see [`hyperion_console::ConsoleSession::backend_label`]
/// -- an honest "mock vs real, and which real" signal the dashboard uses to color/label nodes),
/// its real, persisted trust-store contents (see [`hyperion_console::peer_trust::PeerTrustStore`]),
/// and its own recent mesh events -- everything a `/mesh-dashboard` needs to describe this one node.
pub fn mesh_status(
    data_dir: &str,
    capabilities: &[String],
    backend_label: &str,
    log: &MeshEventLog,
) -> Value {
    let trusted_peers = hyperion_console::peer_trust::PeerTrustStore::open_or_create(
        crate::peer_trust_path(data_dir),
    )
    .map(|store| store.trusted_peers())
    .unwrap_or_default();
    json!({
        "capabilities": capabilities,
        "backend": backend_label,
        "trusted_peers": trusted_peers,
        "recent_events": log.recent(50),
    })
}

/// Scans the real LAN for a real peer whose own Agent Card lists `capability` among its
/// `skills[].id`, excluding `own_port` (this node itself, so a node never delegates to itself).
/// Retries up to 5 times (2s each, ~10s worst case) rather than assuming one short scan finds
/// every peer -- several `hyperion-console` processes starting at roughly the same time (this
/// demo's whole point) won't all have finished mDNS-converging by the time the first one asks,
/// and real multicast convergence across several concurrently-advertising processes can take
/// longer than a single short scan, especially over a virtualized/NAT'd network adapter.
pub fn find_capability_peer(
    capability: &str,
    own_port: u16,
) -> Result<(String, SocketAddr), String> {
    for _attempt in 0..5 {
        let peers = discovery::discover(discovery::A2A_SERVICE_TYPE, Duration::from_secs(2))
            .map_err(|e| format!("couldn't scan the LAN for A2A peers: {e}"))?;
        for peer in &peers {
            if peer.addr.port() == own_port {
                continue;
            }
            let Ok(card_body) = http_client::get(
                &peer.addr.ip().to_string(),
                peer.addr.port(),
                "/.well-known/agent-card.json",
            ) else {
                continue;
            };
            let Ok(card) = serde_json::from_str::<Value>(&card_body) else {
                continue;
            };
            let has_capability =
                card.get("skills")
                    .and_then(|s| s.as_array())
                    .is_some_and(|skills| {
                        skills.iter().any(|skill| {
                            skill.get("id").and_then(|id| id.as_str()) == Some(capability)
                        })
                    });
            if has_capability {
                return Ok((peer.instance_name.clone(), peer.addr));
            }
        }
    }
    Err(format!(
        "no peer advertising capability {capability:?} answered after 5 real LAN scans"
    ))
}

/// A real, one-shot snapshot of the whole mesh from whichever process calls this: discovers every
/// live A2A peer, fetches each one's real Agent Card + `/mesh/status`, and assembles a graph
/// `/mesh-dashboard`'s own served page renders directly. Best-effort per peer -- a peer that
/// doesn't answer in time is simply left out of this snapshot, not treated as a hard error (the
/// next refresh, ~2s later, tries again).
pub fn build_mesh_graph() -> Value {
    let peers = discovery::discover(discovery::A2A_SERVICE_TYPE, Duration::from_secs(2))
        .unwrap_or_default();

    // A real peer resolves to one real address per real network interface it's reachable on
    // (loopback, a host's other real adapters, ...) -- only the reachable one is worth a node;
    // deduping by instance name (keeping the first resolution whose Agent Card actually answers)
    // is what keeps this from showing the same real peer as several empty "ghost" nodes.
    let mut by_name: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    for peer in &peers {
        if by_name.contains_key(&peer.instance_name) {
            continue;
        }
        let host = peer.addr.ip().to_string();
        let port = peer.addr.port();
        let Some(card) = http_client::get(&host, port, "/.well-known/agent-card.json")
            .ok()
            .and_then(|body| serde_json::from_str::<Value>(&body).ok())
        else {
            continue;
        };
        let status = http_client::get(&host, port, "/mesh/status")
            .ok()
            .and_then(|body| serde_json::from_str::<Value>(&body).ok());
        let capabilities = card
            .get("skills")
            .and_then(|s| s.as_array())
            .map(|skills| {
                skills
                    .iter()
                    .filter_map(|s| s.get("id").and_then(|id| id.as_str()).map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        by_name.insert(
            peer.instance_name.clone(),
            json!({
                "id": format!("{host}:{port}"),
                "name": peer.instance_name,
                "capabilities": capabilities,
                "backend": status.as_ref().and_then(|s| s.get("backend")).cloned().unwrap_or(json!("mock")),
                "trusted_peers": status.as_ref().and_then(|s| s.get("trusted_peers")).cloned().unwrap_or(json!([])),
                "recent_events": status.as_ref().and_then(|s| s.get("recent_events")).cloned().unwrap_or(json!([])),
            }),
        );
    }

    // A fresh `HashMap` every call (`discover`'s own return order isn't stable between real LAN
    // scans either) means an unsorted `Vec` would reshuffle every ~2s refresh -- a real node's
    // position in the dashboard's own circle layout must stay put, so this sorts by the one
    // thing that's actually stable across refreshes: the node's own id.
    let mut nodes: Vec<Value> = by_name.into_values().collect();
    nodes.sort_by(|a, b| {
        a.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("id").and_then(|v| v.as_str()).unwrap_or(""))
    });

    json!({
        "generated_at": crate::a2a::rfc3339_utc(crate::a2a::unix_seconds_now()),
        "nodes": nodes,
    })
}
