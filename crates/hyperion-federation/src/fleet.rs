//! This crate's own previously-named "Fleet aggregate-submission network endpoint" gap
//! (`hyperion-observability`'s own doc comment: "`Fleet.submitAggregate`. `build_aggregate`
//! produces the gated report; nothing here sends it anywhere -- no real network transport exists
//! in this hosted simulator"). `hyperion-observability` cannot own this itself: it has no
//! transport primitive of its own and this crate already depends on it (confirmed via its
//! `Cargo.toml`), so the reverse dependency direction would be a hard Cargo cycle -- the same
//! "already depends the other way" reasoning that placed [`crate::kg_sync`] here rather than in
//! `hyperion-knowledge-graph`.
//!
//! Replicates [`crate::transport`]'s real socket shape exactly (a real `TcpListener` background
//! thread, real `seal_for_peer`/`open_from_peer` authentication+encryption, length-prefixed
//! frames, `Drop`-joins-thread shutdown), but carries a real
//! `hyperion_observability::AggregateReport` as JSON (via that type's own real
//! `Serialize`/`Deserialize`) rather than [`crate::transport::LedgerPublication`]'s fixed-width
//! binary encoding -- `AggregateReport::summaries` is a variable-length `Vec`, the same reason
//! [`crate::kg_sync`] chose JSON over a fixed layout for its own variable-length
//! `GraphSnapshot` payload.
//!
//! There is no "Fleet service" storage concept anywhere in this workspace to receive into, so
//! [`FleetAggregateStore`] is the real, honest, in-memory one this gap actually needed: every
//! genuinely authenticated, decrypted, well-formed submission is appended and observable via
//! [`FleetAggregateStore::received`] -- not a fabricated success with nowhere for the data to
//! land.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use hyperion_crypto::{SyncEnvelope, VerifyingKey};
use hyperion_observability::AggregateReport;

use crate::hub::FederationHub;

/// Mirrors [`crate::kg_sync`]'s own identical constant.
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(20);
const CONNECT_RETRIES: u32 = 20;
const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// One real, genuinely-authenticated [`AggregateReport`] submission this Fleet endpoint has
/// received -- `sender_device_id` names who sent it (the receiver's own known identity for the
/// wired peer, never asserted by the wire payload itself), mirroring
/// [`crate::transport::LedgerPublication`]'s own "the receiver looks up who it's really talking
/// to, not a value the sender could lie about" convention.
#[derive(Debug, Clone)]
pub struct ReceivedAggregate {
    pub sender_device_id: u64,
    pub report: AggregateReport,
}

/// The real, in-memory "Fleet" a device's own [`FleetSubmissionServer`] receives into -- this
/// workspace's honest current scale (docs/41's own "dozens, not thousands") makes an unbounded
/// `Vec` behind a `Mutex` the real, working choice, the same reasoning [`crate::kg_sync`]'s own
/// doc comment already gives for an equivalent shape of gap.
#[derive(Default)]
pub struct FleetAggregateStore {
    received: Mutex<Vec<ReceivedAggregate>>,
}

impl FleetAggregateStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every real submission received so far, oldest first.
    pub fn received(&self) -> Vec<ReceivedAggregate> {
        self.received.lock().unwrap().clone()
    }

    fn record(&self, sender_device_id: u64, report: AggregateReport) {
        self.received.lock().unwrap().push(ReceivedAggregate {
            sender_device_id,
            report,
        });
    }
}

fn report_to_wire(report: &AggregateReport) -> Vec<u8> {
    serde_json::to_vec(report).expect("a real AggregateReport always serializes")
}

fn report_from_wire(bytes: &[u8]) -> Option<AggregateReport> {
    serde_json::from_slice(bytes).ok()
}

/// A real, running background thread accepting [`serve_fleet_submissions`] connections. Stopped
/// by dropping this handle (or calling [`Self::stop`] explicitly) -- the real thread is joined,
/// not merely detached, mirroring [`crate::kg_sync::KgSnapshotServer`]'s own identical shutdown
/// contract.
pub struct FleetSubmissionServer {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    local_addr: SocketAddr,
}

impl FleetSubmissionServer {
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

impl Drop for FleetSubmissionServer {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// Starts a real background thread that accepts real `TcpListener` connections on `bind_addr`
/// and, for each one, reads a single real length-prefixed [`SyncEnvelope`], authenticates+
/// decrypts it via `hub.open_from_peer(sender_verifying_key, shared_secret, ..)`, decodes the
/// resulting plaintext as a real [`AggregateReport`], and -- only once all of that has genuinely
/// succeeded -- records it into `store`. A malformed frame, a failed signature/decryption, or
/// unparseable JSON is silently dropped rather than panicking the accept loop -- the next
/// connection is tried regardless, mirroring [`crate::kg_sync::serve_kg_snapshots`]'s own
/// identical fail-safe contract.
pub fn serve_fleet_submissions(
    hub: Arc<FederationHub>,
    store: Arc<FleetAggregateStore>,
    sender_device_id: u64,
    bind_addr: &str,
    sender_verifying_key: VerifyingKey,
    shared_secret: [u8; 32],
) -> io::Result<FleetSubmissionServer> {
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
                        &store,
                        sender_device_id,
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

    Ok(FleetSubmissionServer {
        stop,
        handle: Some(handle),
        local_addr,
    })
}

fn handle_connection(
    hub: &Arc<FederationHub>,
    store: &Arc<FleetAggregateStore>,
    sender_device_id: u64,
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
    let Some(report) = report_from_wire(&plaintext) else {
        return Ok(());
    };
    store.record(sender_device_id, report);
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

fn send_report(addr: &str, wire: &[u8]) -> io::Result<()> {
    let mut stream = TcpStream::connect(addr)?;
    stream.write_all(&(wire.len() as u32).to_le_bytes())?;
    stream.write_all(wire)?;
    Ok(())
}

/// The real client half of [`serve_fleet_submissions`]: `docs/34-observability-telemetry.md`'s
/// own literal `Fleet.submitAggregate`. Seals `report`'s real wire bytes via
/// `hub.seal_for_peer(shared_secret, sender_device_id, ..)`, then sends the resulting
/// [`SyncEnvelope`] length-prefixed over a real `TcpStream` to `addr`. Resending on a transient
/// connect/write failure is safe: the receiving [`FleetAggregateStore`] simply records the same
/// report twice rather than corrupting anything, the same "safe to resend" reasoning
/// [`crate::kg_sync::publish_snapshot_over_socket`]'s own doc comment gives.
pub fn submit_aggregate_over_socket(
    hub: &FederationHub,
    addr: &str,
    shared_secret: &[u8; 32],
    sender_device_id: u64,
    report: &AggregateReport,
) -> io::Result<()> {
    let envelope = hub.seal_for_peer(shared_secret, sender_device_id, &report_to_wire(report));
    let wire = envelope.to_wire_bytes();

    let mut last_err = None;
    for attempt in 0..=CONNECT_RETRIES {
        match send_report(addr, &wire) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_crypto::Keystore;

    fn hub_pair() -> (Arc<FederationHub>, Arc<FederationHub>, [u8; 32]) {
        let sender_hub = Arc::new(FederationHub::new_with_keystore(Keystore::ephemeral()));
        let receiver_hub = Arc::new(FederationHub::new_with_keystore(Keystore::ephemeral()));
        let shared_secret = sender_hub.establish_shared_secret(&receiver_hub.x25519_public());
        (sender_hub, receiver_hub, shared_secret)
    }

    #[test]
    fn a_real_submission_arrives_authenticated_and_decrypted() {
        let (sender_hub, receiver_hub, shared_secret) = hub_pair();
        let store = Arc::new(FleetAggregateStore::new());
        let server = serve_fleet_submissions(
            Arc::clone(&receiver_hub),
            Arc::clone(&store),
            /* sender_device_id */ 7,
            "127.0.0.1:0",
            sender_hub.verifying_key(),
            shared_secret,
        )
        .expect("real bind must succeed");

        let report = AggregateReport {
            cohort_size: 12,
            summaries: vec![("latency_ms".to_string(), 42.0)],
            suppressed: false,
        };
        submit_aggregate_over_socket(
            &sender_hub,
            &server.local_addr().to_string(),
            &shared_secret,
            7,
            &report,
        )
        .expect("real send must succeed");

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut received = store.received();
        while received.is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            received = store.received();
        }

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].sender_device_id, 7);
        assert_eq!(received[0].report.cohort_size, 12);
        assert_eq!(
            received[0].report.summaries,
            vec![("latency_ms".to_string(), 42.0)]
        );
        assert!(!received[0].report.suppressed);

        server.stop();
    }

    #[test]
    fn a_suppressed_report_still_arrives_with_empty_summaries() {
        let (sender_hub, receiver_hub, shared_secret) = hub_pair();
        let store = Arc::new(FleetAggregateStore::new());
        let server = serve_fleet_submissions(
            Arc::clone(&receiver_hub),
            Arc::clone(&store),
            3,
            "127.0.0.1:0",
            sender_hub.verifying_key(),
            shared_secret,
        )
        .expect("real bind must succeed");

        let report = AggregateReport {
            cohort_size: 2,
            summaries: Vec::new(),
            suppressed: true,
        };
        submit_aggregate_over_socket(
            &sender_hub,
            &server.local_addr().to_string(),
            &shared_secret,
            3,
            &report,
        )
        .expect("real send must succeed");

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut received = store.received();
        while received.is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            received = store.received();
        }

        assert_eq!(received.len(), 1);
        assert!(received[0].report.suppressed);
        assert!(received[0].report.summaries.is_empty());

        server.stop();
    }

    #[test]
    fn a_bad_signature_is_silently_dropped_not_recorded() {
        let (sender_hub, receiver_hub, shared_secret) = hub_pair();
        let impostor_hub = Arc::new(FederationHub::new_with_keystore(Keystore::ephemeral()));
        let store = Arc::new(FleetAggregateStore::new());
        let server = serve_fleet_submissions(
            Arc::clone(&receiver_hub),
            Arc::clone(&store),
            9,
            "127.0.0.1:0",
            sender_hub.verifying_key(),
            shared_secret,
        )
        .expect("real bind must succeed");

        let report = AggregateReport {
            cohort_size: 5,
            summaries: vec![("x".to_string(), 1.0)],
            suppressed: false,
        };
        submit_aggregate_over_socket(
            &impostor_hub,
            &server.local_addr().to_string(),
            &shared_secret,
            9,
            &report,
        )
        .expect("the send itself still succeeds -- only the receiver's verification rejects it");

        std::thread::sleep(Duration::from_millis(200));
        assert!(store.received().is_empty());

        server.stop();
    }
}
