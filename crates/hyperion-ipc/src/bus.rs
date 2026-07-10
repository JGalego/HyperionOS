use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Mutex;
use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, Fault, ObjectId, Operation, RightsMask};

use crate::channel::Channel;
use crate::frame::{Frame, FrameBody, HYIP_MAGIC, WIRE_VERSION};
use crate::types::{FrameFlags, IpcFault};

/// A `CALL`-class request, ready to hand to [`IpcBus::ipc_call`].
#[derive(Debug, Clone)]
pub struct Request {
    pub op: Operation,
    pub payload: Vec<u8>,
}

/// The successful result of an `ipc_call`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub payload: Vec<u8>,
}

/// A `NOTIFY`-class one-way message, ready to hand to [`IpcBus::ipc_notify`].
#[derive(Debug, Clone)]
pub struct Notification {
    pub op: Operation,
    pub payload: Vec<u8>,
}

/// A frame that has already passed the receiving server's capability check —
/// see [`IpcBus::recv_authenticated`]. `request_id` is only meaningful (and
/// only needs a reply) when `is_call` is true.
pub struct AuthenticatedCall {
    pub request_id: u64,
    pub op: Operation,
    pub body: FrameBody,
    pub is_call: bool,
}

/// The simulator's stand-in for the kernel's `endpoint_send` / `endpoint_recv`
/// primitives (docs/03-kernel-architecture.md §Interfaces / APIs) plus the
/// request/reply correlation docs/30-ipc-framework.md's `ipc_call` builds on
/// top of them. In the real system this state lives in the privileged core;
/// here, since every simulated Trust Boundary is a thread in one process
/// rather than its own address space, it's an ordinary shared value threads
/// hold an `Arc` to.
#[derive(Default)]
pub struct IpcBus {
    endpoints: Mutex<HashMap<ObjectId, mpsc::Sender<Frame>>>,
    pending_replies: Mutex<HashMap<u64, SyncSender<Frame>>>,
    next_request_id: AtomicU64,
}

impl IpcBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a fresh queue for the endpoint object `object_id` (already
    /// named by a capability minted elsewhere) and returns the receiving
    /// half its owning Trust Boundary polls via [`Self::recv_authenticated`].
    pub fn create_endpoint(&self, object_id: ObjectId) -> Receiver<Frame> {
        let (tx, rx) = mpsc::channel();
        self.endpoints.lock().unwrap().insert(object_id, tx);
        rx
    }

    /// Deregisters `object_id`'s queue. Any `recv_authenticated` blocked on
    /// it observes the peer as gone (`IpcFault::PeerUnreachable`) rather
    /// than hanging — the clean-shutdown counterpart to
    /// [`Self::create_endpoint`], e.g. for tearing down a simulated server.
    pub fn close_endpoint(&self, object_id: ObjectId) {
        self.endpoints.lock().unwrap().remove(&object_id);
    }

    fn send(&self, object_id: ObjectId, frame: Frame) -> Result<(), IpcFault> {
        let senders = self.endpoints.lock().unwrap();
        let tx = senders.get(&object_id).ok_or(IpcFault::PeerUnreachable)?;
        tx.send(frame).map_err(|_| IpcFault::PeerUnreachable)
    }

    /// `ipc_call` — docs/30-ipc-framework.md §Algorithms' synchronous
    /// call/response rendezvous. Blocks until the matching `REPLY` arrives,
    /// the peer is gone, or `timeout` elapses.
    pub fn ipc_call(&self, chan: &Channel, req: Request, timeout: Duration) -> Result<Response, IpcFault> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.pending_replies.lock().unwrap().insert(request_id, reply_tx);

        let frame = Frame {
            magic: HYIP_MAGIC,
            version: WIRE_VERSION,
            schema_id: chan.schema_id,
            flags: FrameFlags::CALL,
            request_id,
            cap_token: Some(chan.endpoint.clone()),
            op: req.op,
            body: FrameBody::Payload(req.payload),
        };

        if let Err(e) = self.send(chan.endpoint.object_id(), frame) {
            self.pending_replies.lock().unwrap().remove(&request_id);
            return Err(e);
        }

        match reply_rx.recv_timeout(timeout) {
            Ok(reply) => match reply.body {
                FrameBody::Payload(payload) => Ok(Response { payload }),
                FrameBody::Fault(fault) => Err(IpcFault::Kernel(fault)),
                FrameBody::Region(_) => Err(IpcFault::SchemaMismatch),
            },
            Err(_) => {
                self.pending_replies.lock().unwrap().remove(&request_id);
                Err(IpcFault::Timeout)
            }
        }
    }

    /// `ipc_notify` — asynchronous one-way message; returns as soon as the
    /// frame is enqueued at the peer's endpoint, no reply wait.
    pub fn ipc_notify(&self, chan: &Channel, note: Notification) -> Result<(), IpcFault> {
        let frame = Frame {
            magic: HYIP_MAGIC,
            version: WIRE_VERSION,
            schema_id: chan.schema_id,
            flags: FrameFlags::NOTIFY,
            request_id: 0,
            cap_token: Some(chan.endpoint.clone()),
            op: note.op,
            body: FrameBody::Payload(note.payload),
        };
        self.send(chan.endpoint.object_id(), frame)
    }

    /// Blocks for the next frame on `endpoint_rx`. Deliberately takes no
    /// `CapabilityMonitor` reference: a real monitor is typically shared
    /// across Trust Boundaries behind a lock, and this call can block
    /// indefinitely waiting for a sender — holding that lock for the
    /// duration would let a blocked receiver starve every other holder
    /// out of ever deriving, revoking, or checking a token. Pair with
    /// [`Self::authenticate`], acquiring the monitor only for that much
    /// shorter, non-blocking step.
    pub fn recv_raw(&self, endpoint_rx: &Receiver<Frame>) -> Result<Frame, IpcFault> {
        endpoint_rx.recv().map_err(|_| IpcFault::PeerUnreachable)
    }

    /// Re-validates the *live* capability `frame`'s caller embedded in it —
    /// the per-invocation revalidation docs/03 §Security Considerations
    /// requires ("checking the live revocation graph generation on every
    /// invocation, not a cached copy"), applied at the IPC framing layer.
    /// A `CALL` frame that fails this check is auto-replied with the
    /// rejecting fault rather than left to time out.
    pub fn authenticate(
        &self,
        frame: Frame,
        monitor: &CapabilityMonitor,
        required: RightsMask,
    ) -> Result<AuthenticatedCall, IpcFault> {
        let is_call = frame.flags.contains(FrameFlags::CALL);

        // A CALL/NOTIFY frame always carries the caller's token (see
        // Frame::cap_token's docs); its absence here would itself be a
        // schema violation, treated as insufficient authority rather than
        // silently trusted.
        let Some(cap_token) = frame.cap_token.as_ref() else {
            let fault = Fault::InsufficientRights;
            if is_call {
                let _ = self.reply_fault(frame.request_id, fault);
            }
            return Err(IpcFault::Kernel(fault));
        };

        if let Err(fault) = monitor.check_rights_ok_result(cap_token, required) {
            if is_call {
                let _ = self.reply_fault(frame.request_id, fault);
            }
            return Err(IpcFault::Kernel(fault));
        }

        Ok(AuthenticatedCall {
            request_id: frame.request_id,
            op: frame.op,
            body: frame.body,
            is_call,
        })
    }

    /// Convenience composition of [`Self::recv_raw`] and [`Self::authenticate`]
    /// for callers whose `monitor` is *not* shared behind a lock a blocking
    /// receive could starve (e.g. a single-threaded test, or a monitor
    /// dedicated to one server). Prefer the split form when `monitor` is
    /// shared with other threads that must keep making progress.
    pub fn recv_authenticated(
        &self,
        endpoint_rx: &Receiver<Frame>,
        monitor: &CapabilityMonitor,
        required: RightsMask,
    ) -> Result<AuthenticatedCall, IpcFault> {
        let frame = self.recv_raw(endpoint_rx)?;
        self.authenticate(frame, monitor, required)
    }

    /// Fulfils a pending `ipc_call` with a successful reply.
    pub fn reply(&self, request_id: u64, payload: Vec<u8>) -> Result<(), IpcFault> {
        let tx = self
            .pending_replies
            .lock()
            .unwrap()
            .remove(&request_id)
            .ok_or(IpcFault::PeerUnreachable)?;
        let frame = Frame {
            magic: HYIP_MAGIC,
            version: WIRE_VERSION,
            schema_id: crate::types::SchemaId(0),
            flags: FrameFlags::REPLY,
            request_id,
            // A reply flows server-to-client and asserts no fresh authority
            // of its own — see Frame::cap_token's docs.
            cap_token: None,
            op: Operation(0),
            body: FrameBody::Payload(payload),
        };
        tx.send(frame).map_err(|_| IpcFault::PeerUnreachable)
    }

    /// Fulfils a pending `ipc_call` with a rejecting fault instead of data.
    pub fn reply_fault(&self, request_id: u64, fault: Fault) -> Result<(), IpcFault> {
        let tx = self
            .pending_replies
            .lock()
            .unwrap()
            .remove(&request_id)
            .ok_or(IpcFault::PeerUnreachable)?;
        let frame = Frame {
            magic: HYIP_MAGIC,
            version: WIRE_VERSION,
            schema_id: crate::types::SchemaId(0),
            flags: FrameFlags::REPLY | FrameFlags::ERROR,
            request_id,
            cap_token: None,
            op: Operation(0),
            body: FrameBody::Fault(fault),
        };
        tx.send(frame).map_err(|_| IpcFault::PeerUnreachable)
    }
}
