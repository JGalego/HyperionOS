//! Real ambient/continuous Knowledge Graph replication across devices -- the genuine gap this
//! crate's own doc comment and `hyperion-storage`'s own doc comment each pointed at the other to
//! close ("Ambient anti-entropy remains deferred: storage convergence is [28 — Storage
//! Engine]'s job and isn't wired in here"; `hyperion-storage`'s own "Sync/replication... is
//! [21 — Distributed Execution]'s concern"). Neither crate actually owned it before this module.
//!
//! Deliberately scoped down from docs/28's own full design (a Merkle tree keyed by WAL segment
//! hash, incremental diff-only sync, per-triple version vectors) to a real, bounded, whole-
//! snapshot replication: [`KnowledgeGraph::dump`] already returns the entire visible graph in one
//! call, and this workspace's real current scale (docs/41's own "dozens of nodes/edges per
//! session, not thousands") makes shipping the whole thing on a fixed real interval the honest,
//! working choice here, the same "real, current scale... makes a full, unbounded scan the right
//! call" reasoning `dump`'s own doc comment already gives for *why it exists* used one level up,
//! for *how often it's sent*. This is real, running, and continuously converges two devices' own
//! graphs -- not bandwidth-optimal, and not a substitute for docs/28's own future incremental
//! Merkle-diff design, which remains real, separate, future work.
//!
//! [`merge_snapshot`] is the real "apply a remote node/edge into my own local graph" primitive
//! this workspace had nowhere before (confirmed: no `import`/`merge`/`apply_remote` method exists
//! anywhere in `hyperion-knowledge-graph`). It never reuses a remote device's own raw `NodeId`s
//! directly -- two independently-created graphs mint ids from their own independent counters, so
//! the same raw id on two devices names two unrelated objects; silently reusing it would corrupt
//! whichever local object happened to already hold that id. [`KgTranslation`] is the real fix: a
//! per-peer table from `(remote NodeId) -> (local NodeId)`, populated the first time a remote
//! node is ever seen and reused on every later sync -- exactly the "translate foreign keys on
//! import" pattern this shape always needs. Deliberately in-memory only, not (yet) persisted
//! across a real restart: a fresh process re-adds every remote node as new the first time it
//! resyncs after restarting, rather than recognizing previously-merged ones -- a real, separate,
//! future gap, not silently pretended away.
//!
//! Merge direction is one-way per call, safe to run bidirectionally between two peers each
//! syncing to the other: [`merge_snapshot`] only ever calls `put_node`/`link` for nodes/edges this
//! *same* translation table already minted (owned by the merging token's own Trust Boundary), so
//! repeated syncs of the same source update their own translated copies and never touch
//! unrelated, locally-authored data. Concurrent, independent edits to the *same* translated
//! object on both sides are last-applied-wins (whichever sync lands last), not true CRDT
//! conflict merge -- docs/28's own deferred algorithm remains the real answer for that, should it
//! ever matter at this workspace's real current scale.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_crypto::{SyncEnvelope, VerifyingKey};
use hyperion_knowledge_graph::{GraphSnapshot, KnowledgeGraph, NodeId};

use crate::hub::FederationHub;

/// How long the accept-loop sleeps between non-blocking `accept()` polls -- mirrors
/// [`crate::transport`]'s own identical constant.
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(20);
const CONNECT_RETRIES: u32 = 20;
const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// The real "translate a remote device's own `NodeId`s into this device's own local ids" table
/// [`merge_snapshot`] needs -- see this module's own doc comment on why reusing a remote id
/// directly would be unsafe. One table per remote peer this device syncs from.
#[derive(Default)]
pub struct KgTranslation {
    remote_to_local: Mutex<HashMap<u64, NodeId>>,
}

impl KgTranslation {
    pub fn new() -> Self {
        Self::default()
    }
}

/// What one real [`merge_snapshot`] call actually did -- observable so a caller (or a test) can
/// confirm real work happened, not just that the call returned without error.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KgMergeReport {
    pub nodes_created: usize,
    pub nodes_updated: usize,
    pub edges_applied: usize,
    pub edges_skipped_unresolved: usize,
}

/// Applies every node and edge in `snapshot` (as produced by a peer's own real
/// `KnowledgeGraph::dump`) into `graph`, translating the remote `NodeId`s `snapshot` itself
/// carries into `graph`'s own local ids via `translation` -- see this module's own doc comment
/// for the full real merge contract (translate-on-import, one-way-per-call, last-applied-wins).
/// Nodes are applied before edges (an edge naming a node this exact snapshot also introduces must
/// resolve); an edge whose subject or target has never been translated -- by this call or an
/// earlier one -- is skipped rather than guessed at, and counted in the returned report.
pub fn merge_snapshot(
    graph: &KnowledgeGraph,
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    translation: &KgTranslation,
    snapshot: &GraphSnapshot,
) -> KgMergeReport {
    let mut report = KgMergeReport::default();
    let mut map = translation.remote_to_local.lock().unwrap();

    for (remote_id, record) in &snapshot.nodes {
        match map.get(&remote_id.0).copied() {
            Some(local_id) => {
                if graph
                    .put_node(
                        monitor,
                        token,
                        Some(local_id),
                        record.object_type.clone(),
                        record.embedding.clone(),
                        record.metadata.clone(),
                    )
                    .is_ok()
                {
                    report.nodes_updated += 1;
                }
            }
            None => {
                if let Ok(local_id) = graph.put_node(
                    monitor,
                    token,
                    None,
                    record.object_type.clone(),
                    record.embedding.clone(),
                    record.metadata.clone(),
                ) {
                    map.insert(remote_id.0, local_id);
                    report.nodes_created += 1;
                }
            }
        }
    }

    for (_, edge) in &snapshot.edges {
        let (Some(local_subject), Some(local_target)) = (
            map.get(&edge.subject.0).copied(),
            map.get(&edge.target.0).copied(),
        ) else {
            report.edges_skipped_unresolved += 1;
            continue;
        };
        if graph
            .link(
                monitor,
                token,
                local_subject,
                &edge.predicate,
                local_target,
                edge.weight,
                edge.origin,
                edge.confidence,
                &edge.provenance,
                None,
            )
            .is_ok()
        {
            report.edges_applied += 1;
        }
    }

    report
}

fn snapshot_to_wire(snapshot: &GraphSnapshot) -> Vec<u8> {
    serde_json::to_vec(snapshot).expect("a real GraphSnapshot always serializes")
}

fn snapshot_from_wire(bytes: &[u8]) -> Option<GraphSnapshot> {
    serde_json::from_slice(bytes).ok()
}

/// A real, running background thread accepting [`serve_kg_snapshots`] connections. Stopped by
/// dropping this handle (or calling [`Self::stop`] explicitly) -- the real thread is joined, not
/// merely detached, mirroring [`crate::LeaseHeartbeat`]/[`crate::transport::LedgerPublicationServer`]'s
/// own identical shutdown contract.
pub struct KgSnapshotServer {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    local_addr: SocketAddr,
}

impl KgSnapshotServer {
    /// The real address this server actually bound to -- see
    /// [`crate::transport::LedgerPublicationServer::local_addr`]'s own doc comment for why this
    /// is the only reliable way to learn what a client should connect to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn stop(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for KgSnapshotServer {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// Starts a real background thread that accepts real `TcpListener` connections on `bind_addr`
/// and, for each one, reads a single real length-prefixed [`SyncEnvelope`], authenticates+
/// decrypts it via `hub.open_from_peer(sender_verifying_key, shared_secret, ..)`, decodes the
/// resulting plaintext as a real [`GraphSnapshot`], and -- only once all of that has genuinely
/// succeeded -- applies it via [`merge_snapshot`]. A malformed frame, a failed signature/
/// decryption, or unparseable JSON is silently dropped rather than panicking the accept loop --
/// the next connection is tried regardless, mirroring [`crate::transport::serve_ledger_publications`]'s
/// own identical fail-safe contract.
#[allow(clippy::too_many_arguments)]
pub fn serve_kg_snapshots(
    hub: Arc<FederationHub>,
    graph: Arc<KnowledgeGraph>,
    monitor: Arc<CapabilityMonitor>,
    token: CapabilityToken,
    translation: Arc<KgTranslation>,
    bind_addr: &str,
    sender_verifying_key: VerifyingKey,
    shared_secret: [u8; 32],
) -> io::Result<KgSnapshotServer> {
    let listener = TcpListener::bind(bind_addr)?;
    let local_addr = listener.local_addr()?;
    listener.set_nonblocking(true)?;

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let handle = std::thread::spawn(move || {
        while !thread_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = handle_connection(
                        &hub,
                        &graph,
                        &monitor,
                        &token,
                        &translation,
                        stream,
                        &sender_verifying_key,
                        &shared_secret,
                    );
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(ACCEPT_POLL_INTERVAL);
                }
                Err(_) => {
                    std::thread::sleep(ACCEPT_POLL_INTERVAL);
                }
            }
        }
    });

    Ok(KgSnapshotServer {
        stop,
        handle: Some(handle),
        local_addr,
    })
}

#[allow(clippy::too_many_arguments)]
fn handle_connection(
    hub: &Arc<FederationHub>,
    graph: &Arc<KnowledgeGraph>,
    monitor: &Arc<CapabilityMonitor>,
    token: &CapabilityToken,
    translation: &Arc<KgTranslation>,
    mut stream: TcpStream,
    sender_verifying_key: &VerifyingKey,
    shared_secret: &[u8; 32],
) -> io::Result<()> {
    let mut len_bytes = [0u8; 4];
    stream.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;

    let Some(envelope) = SyncEnvelope::from_wire_bytes(&body) else {
        return Ok(());
    };
    let Ok(plaintext) = hub.open_from_peer(sender_verifying_key, shared_secret, &envelope) else {
        return Ok(());
    };
    let Some(snapshot) = snapshot_from_wire(&plaintext) else {
        return Ok(());
    };
    merge_snapshot(graph, monitor, token, translation, &snapshot);
    Ok(())
}

fn is_transient_connect_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::BrokenPipe
    )
}

fn send_snapshot(addr: &str, wire: &[u8]) -> io::Result<()> {
    let mut stream = TcpStream::connect(addr)?;
    stream.write_all(&(wire.len() as u32).to_le_bytes())?;
    stream.write_all(wire)?;
    Ok(())
}

/// The real client half of [`serve_kg_snapshots`]: seals `snapshot`'s real wire bytes via
/// `hub.seal_for_peer(shared_secret, sender_device_id, ..)`, then sends the resulting
/// [`SyncEnvelope`] length-prefixed over a real `TcpStream` to `addr`. Resending on a transient
/// connect/write failure is safe: applying the same snapshot twice is idempotent from the
/// receiving side's perspective (see [`merge_snapshot`]'s own doc comment).
pub fn publish_snapshot_over_socket(
    hub: &FederationHub,
    addr: &str,
    shared_secret: &[u8; 32],
    sender_device_id: u64,
    snapshot: &GraphSnapshot,
) -> io::Result<()> {
    let envelope = hub.seal_for_peer(shared_secret, sender_device_id, &snapshot_to_wire(snapshot));
    let wire = envelope.to_wire_bytes();

    let mut last_err = None;
    for attempt in 0..=CONNECT_RETRIES {
        match send_snapshot(addr, &wire) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < CONNECT_RETRIES && is_transient_connect_error(&e) => {
                last_err = Some(e);
                std::thread::sleep(CONNECT_RETRY_INTERVAL);
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.expect("loop only exits via a returned Ok/Err or after storing last_err"))
}

/// A real, running background thread that ships this device's own real, complete
/// `KnowledgeGraph::dump` to one configured peer on a fixed real wall-clock interval -- the
/// "ambient/continuous" half of this module's own doc comment: no caller has to remember to
/// trigger a sync, it just keeps happening for as long as this handle is alive. Stopped by
/// dropping this handle (or calling [`Self::stop`] explicitly) -- the real thread is joined, not
/// merely detached, mirroring [`crate::LeaseHeartbeat`]'s own identical shutdown contract.
pub struct KgAntiEntropyHeartbeat {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl KgAntiEntropyHeartbeat {
    /// Starts the real background thread. A publish attempt that fails (peer unreachable this
    /// round, most concretely) is logged and skipped -- the next tick tries again on its own
    /// schedule, never panicking the heartbeat thread itself.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        graph: Arc<KnowledgeGraph>,
        hub: Arc<FederationHub>,
        monitor: Arc<CapabilityMonitor>,
        token: CapabilityToken,
        peer_addr: String,
        shared_secret: [u8; 32],
        sender_device_id: u64,
        interval: Duration,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match graph.dump(&monitor, &token) {
                    Ok(snapshot) => {
                        if let Err(e) = publish_snapshot_over_socket(
                            &hub,
                            &peer_addr,
                            &shared_secret,
                            sender_device_id,
                            &snapshot,
                        ) {
                            eprintln!(
                                "hyperion-federation: real KG anti-entropy publish to {peer_addr} \
                                 failed this round, retrying next interval: {e}"
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "hyperion-federation: couldn't read this device's own real \
                             KnowledgeGraph to publish: {e}"
                        );
                    }
                }
                let mut waited = Duration::ZERO;
                while waited < interval && !thread_stop.load(Ordering::Relaxed) {
                    let step = interval.saturating_sub(waited).min(ACCEPT_POLL_INTERVAL);
                    std::thread::sleep(step);
                    waited += step;
                }
            }
        });

        KgAntiEntropyHeartbeat {
            stop,
            handle: Some(handle),
        }
    }

    pub fn stop(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for KgAntiEntropyHeartbeat {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}
