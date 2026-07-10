//! Phase 1 integration harness: wires `hyperion-capability`, `hyperion-ipc`,
//! and `hyperion-scheduler` together across two simulated Trust Boundaries.
//!
//! Each crate is tested in isolation already; this crate exists to prove
//! the composition docs/41-implementation-phases.md's Phase 1 exit
//! criterion actually describes — "a capability token can be minted,
//! delegated, attenuated, and revoked end-to-end across two sandboxed
//! processes" — where "processes" means real `hyperion-ipc` channels and
//! the work they trigger is real `hyperion-scheduler` admission, not a
//! standalone unit test of either piece.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_ipc::{
    channel_open, ChannelClass, FrameBody, IpcBus, IpcFault, Operation, Request, SchemaId,
};
use hyperion_scheduler::{
    IntentId, ResourceDimension, ResourceLedger, ResourceVector, SchedClass, Scheduler,
    TaskDescriptor, TaskId,
};

pub const SERVER: TrustBoundaryId = TrustBoundaryId(1);
pub const CLIENT: TrustBoundaryId = TrustBoundaryId(2);
pub const ECHO_OP: Operation = Operation(1);

/// What the demo scenario produced, for a caller (test or binary) to assert
/// on or print.
pub struct DemoOutcome {
    pub first_call_reply: Vec<u8>,
    pub post_revocation_result: Result<Vec<u8>, IpcFault>,
}

/// Runs the scenario: a Server Trust Boundary owns an IPC endpoint and a
/// CPU resource ledger; a Client holds only an attenuated, `WRITE`-only
/// capability derived from the Server's root. The Client calls the Server
/// over `hyperion-ipc`; handling that call makes the Server submit and
/// admit real `hyperion-scheduler` work, gated by the Server's own
/// capability token. Finally the Client's token is revoked and the same
/// call is retried, proving revocation reaches all the way through the IPC
/// layer, not just a bare `hyperion-capability` check.
pub fn run_demo() -> DemoOutcome {
    let mut monitor = CapabilityMonitor::new();
    let server_root = monitor.mint_root(RightsMask::all(), SERVER, None);
    let client_token = monitor
        .cap_derive(&server_root, RightsMask::WRITE, None, CLIENT)
        .expect("attenuating WRITE out of an all-rights root must succeed");

    let bus = Arc::new(IpcBus::new());
    let rx = bus.create_endpoint(server_root.object_id());

    let mut scheduler = Scheduler::new();
    scheduler.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 100, 10));

    let monitor = Arc::new(Mutex::new(monitor));
    let scheduler = Arc::new(Mutex::new(scheduler));
    let server_work_token = server_root.clone();

    let server_thread = {
        let bus = Arc::clone(&bus);
        let monitor = Arc::clone(&monitor);
        let scheduler = Arc::clone(&scheduler);
        thread::spawn(move || {
            let mut next_task_id = 0u64;
            loop {
                // No lock is held across this blocking receive — see
                // hyperion-ipc's own recv_raw/authenticate split, which
                // exists precisely to avoid starving the Client thread's
                // concurrent monitor.cap_revoke call below.
                let Ok(frame) = bus.recv_raw(&rx) else {
                    break; // endpoint closed: shut down cleanly
                };
                let call = {
                    let guard = monitor.lock().unwrap();
                    bus.authenticate(frame, &guard, RightsMask::WRITE)
                };
                let Ok(call) = call else {
                    continue; // rejected caller; keep serving everyone else
                };
                if !call.is_call {
                    continue;
                }

                // Handling the call is itself scheduled work, gated by the
                // *Server's* own token (it is the Server's CPU time being
                // spent), demonstrating the scheduler and capability crates
                // composing, not just IPC and capability.
                next_task_id += 1;
                let task = TaskDescriptor {
                    id: TaskId(next_task_id),
                    owner_intent: IntentId(1),
                    owner_agent: None,
                    class: SchedClass::InteractiveAgent,
                    deadline: None,
                    priority_weight: 1.0,
                    request: ResourceVector {
                        cpu_shares: 5,
                        ..Default::default()
                    },
                    cap_token: server_work_token.clone(),
                };
                let ticket = {
                    let guard = monitor.lock().unwrap();
                    scheduler
                        .lock()
                        .unwrap()
                        .submit_task(&guard, task)
                        .expect("the server's own root token always authorizes its own work")
                };
                {
                    let mut sched = scheduler.lock().unwrap();
                    sched.schedule_epoch();
                    let _ = sched.complete(ticket);
                }

                let mut reply_payload = match call.body {
                    FrameBody::Payload(p) => p,
                    _ => Vec::new(),
                };
                reply_payload.push(0xFF);
                let _ = bus.reply(call.request_id, reply_payload);
            }
        })
    };

    let chan = {
        let guard = monitor.lock().unwrap();
        channel_open(&guard, &client_token, SchemaId(1), ChannelClass::Call)
            .expect("a fresh WRITE token must be able to open a channel to the server")
    };

    let first = bus
        .ipc_call(
            &chan,
            Request {
                op: ECHO_OP,
                payload: vec![1, 2, 3],
            },
            Duration::from_secs(2),
        )
        .expect("a live, sufficiently-privileged client token must succeed");

    monitor.lock().unwrap().cap_revoke(&client_token);

    let post_revocation = bus
        .ipc_call(
            &chan,
            Request {
                op: ECHO_OP,
                payload: vec![9],
            },
            Duration::from_secs(2),
        )
        .map(|r| r.payload);

    bus.close_endpoint(server_root.object_id());
    server_thread.join().expect("server thread must not panic");

    DemoOutcome {
        first_call_reply: first.payload,
        post_revocation_result: post_revocation,
    }
}
