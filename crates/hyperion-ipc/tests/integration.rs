//! End-to-end tests proving docs/30-ipc-framework.md's central claim:
//! capability-scoped message passing between two simulated Trust
//! Boundaries, where the tokens from `hyperion-capability` gate both
//! *opening* a channel and *every subsequent call* over it.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, Fault, RightsMask, TrustBoundaryId};
use hyperion_ipc::{
    channel_open, ChannelClass, FrameBody, IpcBus, IpcFault, Notification, Operation, Request,
    SchemaId,
};

const SERVER: TrustBoundaryId = TrustBoundaryId(1);
const CLIENT_A: TrustBoundaryId = TrustBoundaryId(2);
const CLIENT_B: TrustBoundaryId = TrustBoundaryId(3);
const ECHO: Operation = Operation(1);

/// Runs a simulated server: authenticates every incoming frame against the
/// live monitor state, echoes CALL payloads back with a trailing marker
/// byte, and exits once its endpoint is closed.
fn spawn_echo_server(
    bus: Arc<IpcBus>,
    monitor: Arc<Mutex<CapabilityMonitor>>,
    rx: std::sync::mpsc::Receiver<hyperion_ipc::Frame>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || loop {
        // recv_raw blocks without holding the monitor lock, so a client
        // thread minting/deriving/revoking tokens concurrently is never
        // starved by a server idling on an empty endpoint.
        let outcome = bus.recv_raw(&rx).and_then(|frame| {
            let guard = monitor.lock().unwrap();
            bus.authenticate(frame, &guard, RightsMask::WRITE)
        });
        match outcome {
            Ok(call) if call.is_call => {
                let mut payload = match call.body {
                    FrameBody::Payload(p) => p,
                    _ => Vec::new(),
                };
                payload.push(0xFF);
                let _ = bus.reply(call.request_id, payload);
            }
            Ok(_) => {} // a NOTIFY; nothing to reply to
            Err(IpcFault::PeerUnreachable) => break,
            Err(_) => {} // rejected caller; keep serving other callers
        }
    })
}

#[test]
fn capability_gated_call_round_trip_between_two_processes() {
    let mut monitor = CapabilityMonitor::new();
    let server_root = monitor.mint_root(RightsMask::all(), SERVER, None);
    let client_token = monitor
        .cap_derive(&server_root, RightsMask::WRITE, None, CLIENT_A)
        .unwrap();

    let bus = Arc::new(IpcBus::new());
    let rx = bus.create_endpoint(server_root.object_id());
    let monitor = Arc::new(Mutex::new(monitor));
    let server = spawn_echo_server(Arc::clone(&bus), Arc::clone(&monitor), rx);

    let chan = {
        let guard = monitor.lock().unwrap();
        channel_open(&guard, &client_token, SchemaId(1), ChannelClass::Call).unwrap()
    };
    let response = bus
        .ipc_call(
            &chan,
            Request {
                op: ECHO,
                payload: vec![1, 2, 3],
            },
            Duration::from_secs(1),
        )
        .expect("a live, sufficiently-privileged token must be able to call the server");
    assert_eq!(response.payload, vec![1, 2, 3, 0xFF]);

    bus.close_endpoint(server_root.object_id());
    server.join().unwrap();
}

#[test]
fn revoking_a_token_blocks_the_very_next_call_over_an_already_open_channel() {
    let mut monitor = CapabilityMonitor::new();
    let server_root = monitor.mint_root(RightsMask::all(), SERVER, None);
    let client_token = monitor
        .cap_derive(&server_root, RightsMask::WRITE, None, CLIENT_A)
        .unwrap();

    let bus = Arc::new(IpcBus::new());
    let rx = bus.create_endpoint(server_root.object_id());
    let monitor = Arc::new(Mutex::new(monitor));
    let server = spawn_echo_server(Arc::clone(&bus), Arc::clone(&monitor), rx);

    let chan = {
        let guard = monitor.lock().unwrap();
        channel_open(&guard, &client_token, SchemaId(1), ChannelClass::Call).unwrap()
    };

    // First call succeeds: the channel and the token behind it are both live.
    bus.ipc_call(&chan, Request { op: ECHO, payload: vec![9] }, Duration::from_secs(1))
        .expect("first call over a freshly opened channel must succeed");

    // Revoke the client's token — the channel itself is still "open" from
    // the client's point of view (channel_open already happened), but the
    // server must re-validate on every call, not trust the snapshot it
    // checked at open time.
    monitor.lock().unwrap().cap_revoke(&client_token);

    let result = bus.ipc_call(&chan, Request { op: ECHO, payload: vec![9] }, Duration::from_secs(1));
    assert_eq!(result.unwrap_err(), IpcFault::Kernel(Fault::Revoked));

    bus.close_endpoint(server_root.object_id());
    server.join().unwrap();
}

#[test]
fn channel_open_collapses_insufficient_rights_and_revocation_into_one_opaque_fault() {
    // docs/30-ipc-framework.md §Architecture: a caller must not be able to
    // distinguish "wrong/insufficient token" from "revoked token" from
    // "nothing is there" — every failure mode of channel_open collapses to
    // the same IpcFault::NoSuchCapability.
    let mut monitor = CapabilityMonitor::new();
    let server_root = monitor.mint_root(RightsMask::all(), SERVER, None);

    // Client B only ever receives a READ-only token — insufficient to open
    // a channel that requires WRITE.
    let read_only = monitor
        .cap_derive(&server_root, RightsMask::READ, None, CLIENT_B)
        .unwrap();
    let insufficient = channel_open(&monitor, &read_only, SchemaId(1), ChannelClass::Call);
    assert_eq!(insufficient.unwrap_err(), IpcFault::NoSuchCapability);

    // Client A receives a WRITE token, sufficient at first, then revoked.
    let write_token = monitor
        .cap_derive(&server_root, RightsMask::WRITE, None, CLIENT_A)
        .unwrap();
    monitor.cap_revoke(&write_token);
    let revoked = channel_open(&monitor, &write_token, SchemaId(1), ChannelClass::Call);

    // Same fault either way — a caller cannot tell these two failures apart.
    assert_eq!(revoked.unwrap_err(), IpcFault::NoSuchCapability);
}

#[test]
fn notify_is_fire_and_forget_and_still_capability_gated() {
    let mut monitor = CapabilityMonitor::new();
    let server_root = monitor.mint_root(RightsMask::all(), SERVER, None);
    let client_token = monitor
        .cap_derive(&server_root, RightsMask::WRITE, None, CLIENT_A)
        .unwrap();

    let bus = Arc::new(IpcBus::new());
    let rx = bus.create_endpoint(server_root.object_id());
    let monitor = Arc::new(Mutex::new(monitor));
    let server = spawn_echo_server(Arc::clone(&bus), Arc::clone(&monitor), rx);

    let chan = {
        let guard = monitor.lock().unwrap();
        channel_open(&guard, &client_token, SchemaId(1), ChannelClass::Notify).unwrap()
    };
    bus.ipc_notify(&chan, Notification { op: ECHO, payload: vec![7] })
        .expect("a live, sufficiently-privileged token must be able to notify the server");

    bus.close_endpoint(server_root.object_id());
    server.join().unwrap();
}
