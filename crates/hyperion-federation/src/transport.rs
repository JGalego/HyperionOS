//! A real socket transport for cross-process ledger publication -- this crate's own
//! previously-named "actual sockets carrying these envelopes between processes" gap. Everything
//! a transport needs was already real ([`FederationHub::seal_for_peer`]/
//! [`FederationHub::open_from_peer`], real X25519 key agreement via
//! [`FederationHub::establish_shared_secret`]); this module is the wire itself: a real
//! [`std::net::TcpListener`] background thread ([`serve_ledger_publications`]) that receives a
//! real, encrypted+signed [`hyperion_crypto::SyncEnvelope`] over a real
//! [`std::net::TcpStream`], authenticates+decrypts it, and only then applies it via the
//! receiving hub's own already-real [`FederationHub::publish_ledger`]; [`publish_ledger_over_socket`]
//! is the real client half.
//!
//! `sender_verifying_key`/`shared_secret` are fixed for a server's whole lifetime -- a real
//! production deployment would resolve these per-connection from a real peer directory, which
//! this crate has no scope to build yet ([`crate`]'s own doc comment); a caller here wires
//! exactly one already-key-exchanged peer per server, matching
//! [`FederationHub::seal_for_peer`]'s own single-peer-at-a-time shape.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hyperion_crypto::{SyncEnvelope, VerifyingKey};
use hyperion_scheduler::ResourceVector;

use crate::hub::FederationHub;

/// How long the accept-loop sleeps between non-blocking `accept()` polls -- real, but short
/// enough that [`LedgerPublicationServer::stop`] returns promptly.
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(20);

const RESOURCE_VECTOR_FIELD_COUNT: usize = 9;
const RESOURCE_VECTOR_LEN: usize = RESOURCE_VECTOR_FIELD_COUNT * 4;
/// [`LedgerPublication::to_bytes`]'s fixed wire size: `device_id` (8) + a [`ResourceVector`]'s
/// nine `u32` fields (36) + `network_latency_ms` (4) + `ttl_secs` (8).
const PUBLICATION_LEN: usize = 8 + RESOURCE_VECTOR_LEN + 4 + 8;

/// The real wire payload one [`publish_ledger_over_socket`] call carries -- everything
/// [`FederationHub::publish_ledger`] needs except `trust_tier` (the receiving hub looks that up
/// itself from its own already-`join_device`d registration of the sender -- a wire sender has no
/// business asserting its own trust tier) and `now` (the receiver's own real wall clock, not a
/// value a remote, possibly clock-skewed sender could lie about).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LedgerPublication {
    pub device_id: u64,
    pub available: ResourceVector,
    pub network_latency_ms: u32,
    pub ttl_secs: u64,
}

impl LedgerPublication {
    fn to_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(PUBLICATION_LEN);
        bytes.extend_from_slice(&self.device_id.to_le_bytes());
        for field in [
            self.available.cpu_shares,
            self.available.ram_mb,
            self.available.gpu_shares,
            self.available.vram_mb,
            self.available.storage_iops,
            self.available.network_bw_kbps,
            self.available.inference_tokens_per_sec,
            self.available.context_window_slots,
            self.available.battery_budget_mw,
        ] {
            bytes.extend_from_slice(&field.to_le_bytes());
        }
        bytes.extend_from_slice(&self.network_latency_ms.to_le_bytes());
        bytes.extend_from_slice(&self.ttl_secs.to_le_bytes());
        bytes
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != PUBLICATION_LEN {
            return None;
        }
        let device_id = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
        let mut fields = [0u32; RESOURCE_VECTOR_FIELD_COUNT];
        for (i, field) in fields.iter_mut().enumerate() {
            let start = 8 + i * 4;
            *field = u32::from_le_bytes(bytes[start..start + 4].try_into().ok()?);
        }
        let latency_start = 8 + RESOURCE_VECTOR_LEN;
        let network_latency_ms =
            u32::from_le_bytes(bytes[latency_start..latency_start + 4].try_into().ok()?);
        let ttl_start = latency_start + 4;
        let ttl_secs = u64::from_le_bytes(bytes[ttl_start..ttl_start + 8].try_into().ok()?);
        Some(LedgerPublication {
            device_id,
            available: ResourceVector {
                cpu_shares: fields[0],
                ram_mb: fields[1],
                gpu_shares: fields[2],
                vram_mb: fields[3],
                storage_iops: fields[4],
                network_bw_kbps: fields[5],
                inference_tokens_per_sec: fields[6],
                context_window_slots: fields[7],
                battery_budget_mw: fields[8],
            },
            network_latency_ms,
            ttl_secs,
        })
    }
}

/// A real, running background thread accepting [`serve_ledger_publications`] connections.
/// Stopped by dropping this handle (or calling [`Self::stop`] explicitly) -- the real thread is
/// joined, not merely detached, mirroring [`crate::LeaseHeartbeat`]'s own shutdown contract.
pub struct LedgerPublicationServer {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    local_addr: SocketAddr,
}

impl LedgerPublicationServer {
    /// The real address this server actually bound to -- the caller-supplied `bind_addr` may
    /// have asked for an OS-chosen port (`"127.0.0.1:0"`), so this is the only reliable way to
    /// learn what a client should really connect to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Signals the real background thread to stop and blocks until it has genuinely exited.
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

impl Drop for LedgerPublicationServer {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// Starts a real background thread that accepts real `TcpListener` connections on `bind_addr`
/// and, for each one, reads a single real length-prefixed [`SyncEnvelope`], authenticates+
/// decrypts it via `hub.open_from_peer(sender_verifying_key, shared_secret, ..)`, decodes the
/// resulting plaintext as a real [`LedgerPublication`], and -- only once all of that has
/// genuinely succeeded -- applies it via `hub`'s own already-real
/// [`FederationHub::publish_ledger`], stamped with this device's own real wall-clock `now`
/// (never a value the remote sender supplied, which it could lie about). A malformed frame, a
/// failed signature/decryption, or an unrecognized `device_id` is silently dropped rather than
/// panicking the accept loop -- the next connection is tried regardless.
pub fn serve_ledger_publications(
    hub: Arc<FederationHub>,
    bind_addr: &str,
    sender_verifying_key: VerifyingKey,
    shared_secret: [u8; 32],
) -> io::Result<LedgerPublicationServer> {
    let listener = TcpListener::bind(bind_addr)?;
    let local_addr = listener.local_addr()?;
    listener.set_nonblocking(true)?;

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let handle = std::thread::spawn(move || {
        while !thread_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = handle_connection(&hub, stream, &sender_verifying_key, &shared_secret);
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

    Ok(LedgerPublicationServer {
        stop,
        handle: Some(handle),
        local_addr,
    })
}

fn handle_connection(
    hub: &Arc<FederationHub>,
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
    let Some(publication) = LedgerPublication::from_bytes(&plaintext) else {
        return Ok(());
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs();
    let _ = hub.publish_ledger(
        publication.device_id,
        publication.available,
        publication.network_latency_ms,
        now,
        publication.ttl_secs,
    );
    Ok(())
}

/// A same-host TCP handshake into a listener whose accept-loop thread hasn't reached its first
/// `accept()` poll yet has no guaranteed instant readiness, even on loopback -- observed as a
/// transient `ConnectionReset`/`BrokenPipe`/`ConnectionRefused` on some platforms' TCP stacks
/// under load. Retried the same real number of times [`wait_for_ledger`]-style callers already
/// tolerate on the receiving side.
const CONNECT_RETRIES: u32 = 20;
const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(50);

fn is_transient_connect_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::BrokenPipe
    )
}

fn send_publication(addr: &str, wire: &[u8]) -> io::Result<()> {
    let mut stream = TcpStream::connect(addr)?;
    stream.write_all(&(wire.len() as u32).to_le_bytes())?;
    stream.write_all(wire)?;
    Ok(())
}

/// The real client half of [`serve_ledger_publications`]: seals `publication`'s real wire bytes
/// via `hub.seal_for_peer(shared_secret, publication.device_id, ..)` (so the receiving hub can
/// verify genuine authorship via its own `open_from_peer`), then sends the resulting
/// [`SyncEnvelope`] length-prefixed over a real `TcpStream` to `addr`. Resending on a transient
/// connect/write failure is safe: a `LedgerPublication` resend is idempotent from the receiving
/// hub's perspective (it only ever overwrites `device_id`'s ledger with the latest values).
pub fn publish_ledger_over_socket(
    hub: &FederationHub,
    addr: &str,
    shared_secret: &[u8; 32],
    publication: LedgerPublication,
) -> io::Result<()> {
    let envelope = hub.seal_for_peer(
        shared_secret,
        publication.device_id,
        &publication.to_bytes(),
    );
    let wire = envelope.to_wire_bytes();

    let mut last_err = None;
    for attempt in 0..=CONNECT_RETRIES {
        match send_publication(addr, &wire) {
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
