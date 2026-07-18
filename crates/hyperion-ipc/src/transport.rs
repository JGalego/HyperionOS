//! A real transport for the frame/channel model [`crate::bus`] already defines: Unix domain
//! datagram sockets instead of an in-process `mpsc` bus, per
//! [docs/998-roadmap.md](../../../docs/998-roadmap.md) M3. Reuses [`crate::Frame`],
//! [`crate::Channel`], [`crate::Request`]/[`crate::Response`]/[`crate::Notification`], and the
//! call/notify semantics as-is; only what actually carries a frame between two real, separate
//! Linux processes is new.
//!
//! `FrameBody::Region` has no wire representation here: a shared-memory region needs a real
//! shared-memory mechanism (mmap/shm_open), not bytes on a socket -- docs/03's zero-copy fast
//! path, a real, separate follow-on this milestone doesn't attempt. Sending one over this
//! transport fails clearly (`IpcFault::SchemaMismatch`), not silently.
//!
//! Addressing is a real limitation worth being upfront about: two real processes share no
//! memory, so "send to this `ObjectId`" can't resolve via a shared in-process table the way
//! [`crate::bus::IpcBus`] does. This transport resolves peers by an explicit filesystem path
//! instead (a bound `UnixDatagram`'s address) -- whoever spawns a Trust Boundary process tells it
//! where its peers live. A real service-discovery directory (e.g. a well-known
//! `object_id -> socket path` registry) is a real, separate piece of infrastructure this
//! milestone doesn't build.

use std::io;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use hyperion_capability::{CapabilityMonitor, Fault, RightsMask, WireToken};

use crate::bus::{AuthenticatedCall, Notification, Request, Response};
use crate::channel::Channel;
use crate::frame::{Frame, FrameBody, HYIP_MAGIC, WIRE_VERSION};
use crate::noise_session::{self, NoiseSession};
use crate::types::{FrameFlags, IpcFault};

/// Generous enough for this milestone's control-plane-sized frames; bulk transfer is the
/// shared-memory region path's job (not yet real over this transport — see the module docs),
/// not something a frame's `Payload` bytes need to accommodate.
const MAX_DATAGRAM_BYTES: usize = 64 * 1024;

/// The wire-serializable mirror of [`Frame`]: identical shape, except `cap_token` carries a
/// [`WireToken`] (an unauthenticated *claim*) instead of a `CapabilityToken`, which cannot cross
/// a real process boundary at all — see `WireToken`'s own docs on why that's a deliberate
/// safety property, not an oversight.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WireFrame {
    magic: u32,
    version: u16,
    schema_id: crate::types::SchemaId,
    flags: FrameFlags,
    request_id: u64,
    cap_token: Option<WireToken>,
    op: hyperion_capability::Operation,
    body: WireFrameBody,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum WireFrameBody {
    Payload(Vec<u8>),
    Fault(Fault),
}

impl WireFrame {
    fn from_frame(frame: &Frame) -> Result<Self, IpcFault> {
        let body = match &frame.body {
            FrameBody::Payload(p) => WireFrameBody::Payload(p.clone()),
            FrameBody::Fault(f) => WireFrameBody::Fault(*f),
            FrameBody::Region(_) => return Err(IpcFault::SchemaMismatch),
        };
        Ok(WireFrame {
            magic: frame.magic,
            version: frame.version,
            schema_id: frame.schema_id,
            flags: frame.flags,
            request_id: frame.request_id,
            cap_token: frame.cap_token.as_ref().map(WireToken::from),
            op: frame.op,
            body,
        })
    }
}

/// A raw frame as it arrived off the wire, plus enough to reply to whoever sent it — the
/// real-transport equivalent of the in-process bus's `pending_replies` map: since a bound
/// `UnixDatagram`'s `recv_from` already hands back the sender's address, there is no separate
/// table to maintain, just this address to hold onto until [`Endpoint::reply`]/`reply_fault`.
pub struct IncomingFrame {
    from: SocketAddr,
    wire: WireFrame,
}

/// A real endpoint: a `UnixDatagram` bound to a real filesystem path, receiving whatever frames
/// arrive for the object this Trust Boundary owns. The real-transport counterpart of
/// [`crate::bus::IpcBus::create_endpoint`]'s in-process queue.
pub struct Endpoint {
    socket: UnixDatagram,
    next_request_id: AtomicU64,
}

impl Endpoint {
    /// Binds a fresh socket at `path`, removing a stale socket file left behind by a previous
    /// run at the same path first (`bind` fails with `EADDRINUSE` on an existing path
    /// otherwise, even for a socket nothing is listening on anymore).
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let socket = UnixDatagram::bind(path)?;
        Ok(Endpoint {
            socket,
            next_request_id: AtomicU64::new(1),
        })
    }

    /// `ipc_call` over a real socket — sends a real `CALL` frame to `peer_path` and blocks for
    /// the matching `REPLY`, exactly like [`crate::bus::IpcBus::ipc_call`]'s semantics, just
    /// carried by real bytes on a real socket instead of an in-process channel send. Requires a
    /// [`Channel`] (i.e. a local, monitor-validated `channel_open`) — for a genuinely separate
    /// `exec`'d client process that only ever received a bare [`WireToken`] *claim* (e.g. via an
    /// env var from whoever spawned it) and has no local monitor to validate it against, see
    /// [`Self::ipc_call_with_claim`]: presenting a claim, unvalidated, is exactly what a real
    /// client does — validation is the *server's* job, via [`Self::authenticate`].
    pub fn ipc_call(
        &self,
        peer_path: impl AsRef<Path>,
        chan: &Channel,
        req: Request,
        timeout: Duration,
    ) -> Result<Response, IpcFault> {
        self.ipc_call_with_claim(
            peer_path,
            &WireToken::from(&chan.endpoint),
            chan.schema_id,
            req,
            timeout,
        )
    }

    /// The claim-based core of [`Self::ipc_call`]: sends a real `CALL` frame carrying `claim`
    /// as-is (no local validation attempted, since a real cross-process client typically has
    /// none to perform) and blocks for the matching `REPLY`.
    pub fn ipc_call_with_claim(
        &self,
        peer_path: impl AsRef<Path>,
        claim: &WireToken,
        schema_id: crate::types::SchemaId,
        req: Request,
        timeout: Duration,
    ) -> Result<Response, IpcFault> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let frame = Frame {
            magic: HYIP_MAGIC,
            version: WIRE_VERSION,
            schema_id,
            flags: FrameFlags::CALL,
            request_id,
            cap_token: None, // placeholder; real claim goes on the wire directly below
            op: req.op,
            body: FrameBody::Payload(req.payload),
        };
        self.send_claim_frame(peer_path, &frame, Some(claim.clone()))?;

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(IpcFault::Timeout);
            }
            self.socket
                .set_read_timeout(Some(remaining))
                .map_err(|_| IpcFault::PeerUnreachable)?;

            let wire = match self.recv_wire_frame() {
                Ok((wire, _from)) => wire,
                Err(IpcFault::Timeout) => return Err(IpcFault::Timeout),
                Err(e) => return Err(e),
            };

            if wire.flags.contains(FrameFlags::REPLY) && wire.request_id == request_id {
                return match wire.body {
                    WireFrameBody::Payload(payload) => Ok(Response { payload }),
                    WireFrameBody::Fault(fault) => Err(IpcFault::Kernel(fault)),
                };
            }
            // Not the reply we're waiting for (e.g. an unrelated NOTIFY landed on this same
            // socket first). This endpoint's `ipc_call` is single-call-at-a-time by design for
            // this milestone -- keep waiting up to the deadline rather than misrouting it.
        }
    }

    /// As [`Self::ipc_call_with_claim`], but every frame -- both directions -- travels sealed
    /// under a live [`NoiseSession`] (from [`Self::noise_handshake_as_initiator`]) instead of as
    /// plaintext JSON. `hyperion-security`'s own previously-named "Real Noise-protocol IPC
    /// handshakes / channel binding" gap, closed: a capability claim now travels *inside* a real,
    /// authenticated-encrypted channel bound to one specific negotiated session, not merely
    /// alongside a bare socket send.
    pub fn ipc_call_with_claim_secure(
        &self,
        peer_path: impl AsRef<Path>,
        claim: &WireToken,
        schema_id: crate::types::SchemaId,
        req: Request,
        session: &mut NoiseSession,
        timeout: Duration,
    ) -> Result<Response, IpcFault> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let frame = Frame {
            magic: HYIP_MAGIC,
            version: WIRE_VERSION,
            schema_id,
            flags: FrameFlags::CALL,
            request_id,
            cap_token: None,
            op: req.op,
            body: FrameBody::Payload(req.payload),
        };
        self.send_claim_frame_secure(peer_path, &frame, Some(claim.clone()), session)?;

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(IpcFault::Timeout);
            }
            let wire = match self.recv_wire_frame_secure(session, remaining) {
                Ok((wire, _from)) => wire,
                Err(IpcFault::Timeout) => return Err(IpcFault::Timeout),
                Err(e) => return Err(e),
            };

            if wire.flags.contains(FrameFlags::REPLY) && wire.request_id == request_id {
                return match wire.body {
                    WireFrameBody::Payload(payload) => Ok(Response { payload }),
                    WireFrameBody::Fault(fault) => Err(IpcFault::Kernel(fault)),
                };
            }
        }
    }

    /// `ipc_notify` over a real socket — asynchronous one-way send, no reply wait, exactly like
    /// [`crate::bus::IpcBus::ipc_notify`]. See [`Self::ipc_call`]'s docs for why this takes a
    /// [`Channel`] and [`Self::ipc_notify_with_claim`] exists alongside it.
    pub fn ipc_notify(
        &self,
        peer_path: impl AsRef<Path>,
        chan: &Channel,
        note: Notification,
    ) -> Result<(), IpcFault> {
        self.ipc_notify_with_claim(
            peer_path,
            &WireToken::from(&chan.endpoint),
            chan.schema_id,
            note,
        )
    }

    /// The claim-based core of [`Self::ipc_notify`] — see [`Self::ipc_call_with_claim`]'s docs.
    pub fn ipc_notify_with_claim(
        &self,
        peer_path: impl AsRef<Path>,
        claim: &WireToken,
        schema_id: crate::types::SchemaId,
        note: Notification,
    ) -> Result<(), IpcFault> {
        let frame = Frame {
            magic: HYIP_MAGIC,
            version: WIRE_VERSION,
            schema_id,
            flags: FrameFlags::NOTIFY,
            request_id: 0,
            cap_token: None, // placeholder; real claim goes on the wire directly below
            op: note.op,
            body: FrameBody::Payload(note.payload),
        };
        self.send_claim_frame(peer_path, &frame, Some(claim.clone()))
    }

    /// Builds the wire form of `frame` and sends it to `peer_path`, with `claim` (if any)
    /// substituted in as `cap_token` — `frame.cap_token` itself is never read here; both call
    /// sites ([`Self::ipc_call_with_claim`], [`Self::ipc_notify_with_claim`]) already leave it
    /// as a placeholder and pass the real claim separately, so there is exactly one place that
    /// decides what claim goes on the wire.
    fn wire_frame_bytes(frame: &Frame, claim: Option<WireToken>) -> Result<Vec<u8>, IpcFault> {
        let mut wire = WireFrame::from_frame(frame)?;
        wire.cap_token = claim;
        serde_json::to_vec(&wire).map_err(|_| IpcFault::SchemaMismatch)
    }

    fn send_claim_frame(
        &self,
        peer_path: impl AsRef<Path>,
        frame: &Frame,
        claim: Option<WireToken>,
    ) -> Result<(), IpcFault> {
        let bytes = Self::wire_frame_bytes(frame, claim)?;
        self.socket
            .send_to(&bytes, peer_path.as_ref())
            .map_err(|_| IpcFault::PeerUnreachable)?;
        Ok(())
    }

    /// As [`Self::send_claim_frame`], but sealed under a live [`NoiseSession`] instead of sent as
    /// plaintext JSON -- see [`crate::noise_session`]'s own doc comment for the real "channel
    /// binding" this closes: `hyperion-security`'s own previously-named "Real Noise-protocol IPC
    /// handshakes / channel binding" gap.
    fn send_claim_frame_secure(
        &self,
        peer_path: impl AsRef<Path>,
        frame: &Frame,
        claim: Option<WireToken>,
        session: &mut NoiseSession,
    ) -> Result<(), IpcFault> {
        let bytes = Self::wire_frame_bytes(frame, claim)?;
        noise_session::send_secure(&self.socket, peer_path, session, &bytes)
    }

    fn recv_wire_frame(&self) -> Result<(WireFrame, SocketAddr), IpcFault> {
        let mut buf = vec![0u8; MAX_DATAGRAM_BYTES];
        let (n, from) = self.socket.recv_from(&mut buf).map_err(|e| {
            if matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) {
                IpcFault::Timeout
            } else {
                IpcFault::PeerUnreachable
            }
        })?;
        let wire = serde_json::from_slice(&buf[..n]).map_err(|_| IpcFault::SchemaMismatch)?;
        Ok((wire, from))
    }

    /// As [`Self::recv_wire_frame`], but opening a real Noise-sealed datagram under `session`
    /// first -- the real receive half of [`Self::send_claim_frame_secure`].
    fn recv_wire_frame_secure(
        &self,
        session: &mut NoiseSession,
        timeout: Duration,
    ) -> Result<(WireFrame, SocketAddr), IpcFault> {
        let (plaintext, from) = noise_session::recv_secure(&self.socket, session, timeout)?;
        let wire = serde_json::from_slice(&plaintext).map_err(|_| IpcFault::SchemaMismatch)?;
        Ok((wire, from))
    }

    /// Performs a real, live `Noise_NN` handshake with the peer bound at `peer_path`, over this
    /// same endpoint's own socket -- no second socket or listener needed. See
    /// [`crate::noise_session::handshake_as_initiator`]'s own doc comment for the real handshake
    /// this runs.
    pub fn noise_handshake_as_initiator(
        &self,
        peer_path: impl AsRef<Path>,
        timeout: Duration,
    ) -> Result<NoiseSession, IpcFault> {
        noise_session::handshake_as_initiator(&self.socket, peer_path, timeout)
    }

    /// The real responder half of [`Self::noise_handshake_as_initiator`]: blocks for the next
    /// peer's first handshake message on this endpoint's own socket. See
    /// [`crate::noise_session::handshake_as_responder`]'s own doc comment.
    pub fn noise_handshake_as_responder(
        &self,
        timeout: Duration,
    ) -> Result<(NoiseSession, SocketAddr), IpcFault> {
        noise_session::handshake_as_responder(&self.socket, timeout)
    }

    /// Blocks for the next raw frame — like [`crate::bus::IpcBus::recv_raw`], but reading real
    /// bytes off a real socket. Deliberately returns the unauthenticated wire form, not a
    /// validated [`AuthenticatedCall`]: pair with [`Self::authenticate`], for the same reason
    /// `recv_raw`/`authenticate` are split in the in-process bus — a blocking receive shouldn't
    /// need to hold a shared `CapabilityMonitor` lock for its entire (possibly unbounded)
    /// duration.
    pub fn recv_raw(&self) -> Result<IncomingFrame, IpcFault> {
        self.socket
            .set_read_timeout(None)
            .map_err(|_| IpcFault::PeerUnreachable)?;
        let (wire, from) = self.recv_wire_frame()?;
        Ok(IncomingFrame { from, wire })
    }

    /// Re-validates the *live* capability embedded in `incoming` against `monitor` — the
    /// real-transport counterpart of [`crate::bus::IpcBus::authenticate`]. This is the exact
    /// point a forged or stale [`WireToken`] gets caught: `monitor.authenticate_wire_token`
    /// checks the claim against the monitor's own revocation-graph record, not just trusting
    /// whatever bytes arrived — see [`hyperion_capability::WireToken`]'s docs.
    ///
    /// Takes `incoming` by reference, not by value: unlike the in-process bus (whose `reply`
    /// looks a pending call up by `request_id` in its own map), this transport replies by
    /// address, so a caller that successfully authenticates a call still needs `incoming` -- to
    /// pass to [`Self::reply`] -- after this returns.
    pub fn authenticate(
        &self,
        incoming: &IncomingFrame,
        monitor: &CapabilityMonitor,
        required: RightsMask,
    ) -> Result<AuthenticatedCall, IpcFault> {
        match Self::validate(incoming, monitor, required) {
            Ok(call) => Ok(call),
            Err((is_call, fault)) => {
                if is_call {
                    let _ = self.reply_fault(&incoming.from, incoming.wire.request_id, fault);
                }
                Err(IpcFault::Kernel(fault))
            }
        }
    }

    /// As [`Self::authenticate`], but a failed check's automatic fault reply is sealed under
    /// `session` too (via [`Self::reply_fault_to_secure`]) instead of sent as plaintext — a call
    /// that arrived over a real, live secure session must never have even its *rejection*
    /// leak onto the wire in the clear.
    pub fn authenticate_secure(
        &self,
        incoming: &IncomingFrame,
        monitor: &CapabilityMonitor,
        required: RightsMask,
        session: &mut NoiseSession,
    ) -> Result<AuthenticatedCall, IpcFault> {
        match Self::validate(incoming, monitor, required) {
            Ok(call) => Ok(call),
            Err((is_call, fault)) => {
                if is_call {
                    let _ = self.reply_fault_to_secure(
                        &incoming.from,
                        incoming.wire.request_id,
                        fault,
                        session,
                    );
                }
                Err(IpcFault::Kernel(fault))
            }
        }
    }

    /// The real, transport-agnostic validation core [`Self::authenticate`]/[`Self::
    /// authenticate_secure`] share -- differs only in *how* a failure's automatic fault reply is
    /// sent, never in what counts as valid. `Err((is_call, fault))` carries everything either
    /// caller needs to send that reply itself.
    fn validate(
        incoming: &IncomingFrame,
        monitor: &CapabilityMonitor,
        required: RightsMask,
    ) -> Result<AuthenticatedCall, (bool, Fault)> {
        let wire = &incoming.wire;
        let is_call = wire.flags.contains(FrameFlags::CALL);

        let Some(wire_token) = wire.cap_token.as_ref() else {
            return Err((is_call, Fault::InsufficientRights));
        };

        let cap_token = monitor
            .authenticate_wire_token(wire_token)
            .map_err(|fault| (is_call, fault))?;

        monitor
            .check_rights_ok_result(&cap_token, required)
            .map_err(|fault| (is_call, fault))?;

        let body = match &wire.body {
            WireFrameBody::Payload(payload) => FrameBody::Payload(payload.clone()),
            WireFrameBody::Fault(fault) => FrameBody::Fault(*fault),
        };
        Ok(AuthenticatedCall {
            request_id: wire.request_id,
            op: wire.op,
            body,
            is_call,
        })
    }

    /// Convenience composition of [`Self::recv_raw`] and [`Self::authenticate`], returning both
    /// the validated call and the `IncomingFrame` needed to reply to it (see [`Self::reply`]) —
    /// for callers whose `monitor` is not shared with other threads a blocking receive could
    /// starve. Prefer the split form otherwise, exactly as `IpcBus::recv_authenticated`'s own
    /// docs already explain.
    pub fn recv_authenticated(
        &self,
        monitor: &CapabilityMonitor,
        required: RightsMask,
    ) -> Result<(AuthenticatedCall, IncomingFrame), IpcFault> {
        let incoming = self.recv_raw()?;
        let call = self.authenticate(&incoming, monitor, required)?;
        Ok((call, incoming))
    }

    /// As [`Self::recv_raw`], but opening a real Noise-sealed datagram under `session` first —
    /// the receive half of [`Self::ipc_call_with_claim_secure`]'s own server side. [`Self::
    /// authenticate`] works unchanged on the result: capability authentication is a property of
    /// the *decrypted* wire frame, independent of which transport delivered it.
    pub fn recv_raw_secure(
        &self,
        session: &mut NoiseSession,
        timeout: Duration,
    ) -> Result<IncomingFrame, IpcFault> {
        let (wire, from) = self.recv_wire_frame_secure(session, timeout)?;
        Ok(IncomingFrame { from, wire })
    }

    /// As [`Self::recv_authenticated`], but over a real, live [`NoiseSession`] — see
    /// [`Self::recv_raw_secure`].
    pub fn recv_authenticated_secure(
        &self,
        session: &mut NoiseSession,
        monitor: &CapabilityMonitor,
        required: RightsMask,
        timeout: Duration,
    ) -> Result<(AuthenticatedCall, IncomingFrame), IpcFault> {
        let incoming = self.recv_raw_secure(session, timeout)?;
        let call = self.authenticate_secure(&incoming, monitor, required, session)?;
        Ok((call, incoming))
    }

    fn reply_bytes(
        request_id: u64,
        body: WireFrameBody,
        extra_flags: FrameFlags,
    ) -> Result<Vec<u8>, IpcFault> {
        let wire = WireFrame {
            magic: HYIP_MAGIC,
            version: WIRE_VERSION,
            schema_id: crate::types::SchemaId(0),
            flags: FrameFlags::REPLY | extra_flags,
            request_id,
            cap_token: None,
            op: hyperion_capability::Operation(0),
            body,
        };
        serde_json::to_vec(&wire).map_err(|_| IpcFault::SchemaMismatch)
    }

    fn reply_to(
        &self,
        to: &SocketAddr,
        request_id: u64,
        body: WireFrameBody,
        extra_flags: FrameFlags,
    ) -> Result<(), IpcFault> {
        let bytes = Self::reply_bytes(request_id, body, extra_flags)?;
        self.socket
            .send_to_addr(&bytes, to)
            .map_err(|_| IpcFault::PeerUnreachable)?;
        Ok(())
    }

    /// As [`Self::reply_to`], but sealed under `session` -- the secure reply half
    /// [`Self::authenticate_secure`]/[`Self::reply_secure`]/[`Self::reply_fault_to_secure`] use.
    fn reply_to_secure(
        &self,
        to: &SocketAddr,
        request_id: u64,
        body: WireFrameBody,
        extra_flags: FrameFlags,
        session: &mut NoiseSession,
    ) -> Result<(), IpcFault> {
        let bytes = Self::reply_bytes(request_id, body, extra_flags)?;
        noise_session::send_secure_to_addr(&self.socket, to, session, &bytes)
    }

    /// Fulfils a pending real `ipc_call` with a successful reply, sent directly back to
    /// whichever address the original frame came from (see [`IncomingFrame`]'s docs for why no
    /// separate correlation table is needed here, unlike the in-process bus's `reply`).
    pub fn reply(&self, incoming_from: &IncomingFrame, payload: Vec<u8>) -> Result<(), IpcFault> {
        self.reply_to(
            &incoming_from.from,
            incoming_from.wire.request_id,
            WireFrameBody::Payload(payload),
            FrameFlags::empty(),
        )
    }

    /// As [`Self::reply`], but sealed under a real, live [`NoiseSession`] -- the secure reply
    /// half of [`Self::ipc_call_with_claim_secure`]'s own server side.
    pub fn reply_secure(
        &self,
        incoming_from: &IncomingFrame,
        payload: Vec<u8>,
        session: &mut NoiseSession,
    ) -> Result<(), IpcFault> {
        self.reply_to_secure(
            &incoming_from.from,
            incoming_from.wire.request_id,
            WireFrameBody::Payload(payload),
            FrameFlags::empty(),
            session,
        )
    }

    /// Fulfils a pending real `ipc_call` with a rejecting fault instead of data.
    pub fn reply_fault_to(
        &self,
        incoming_from: &IncomingFrame,
        fault: Fault,
    ) -> Result<(), IpcFault> {
        self.reply_to(
            &incoming_from.from,
            incoming_from.wire.request_id,
            WireFrameBody::Fault(fault),
            FrameFlags::ERROR,
        )
    }

    fn reply_fault(&self, to: &SocketAddr, request_id: u64, fault: Fault) -> Result<(), IpcFault> {
        self.reply_to(
            to,
            request_id,
            WireFrameBody::Fault(fault),
            FrameFlags::ERROR,
        )
    }

    fn reply_fault_to_secure(
        &self,
        to: &SocketAddr,
        request_id: u64,
        fault: Fault,
        session: &mut NoiseSession,
    ) -> Result<(), IpcFault> {
        self.reply_to_secure(
            to,
            request_id,
            WireFrameBody::Fault(fault),
            FrameFlags::ERROR,
            session,
        )
    }
}
