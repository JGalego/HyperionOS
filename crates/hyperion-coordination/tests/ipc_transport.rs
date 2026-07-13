//! docs/12 §5.2's `propose_write`: "the message shape... is real, the
//! wire format is not" — this crate's own doc comment says coordination
//! calls are direct in-process method calls, not `CoordMessage`s over a
//! real transport. This proves the message shape itself genuinely
//! survives a real `hyperion-ipc` hop: serialized to bytes, sent as a
//! real `CALL` frame between two separate Trust Boundaries, applied for
//! real against a live `CoordinationSession` on the receiving thread, and
//! the real `WriteOutcome` carried back over the same real reply path.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use hyperion_agent_runtime::AgentRuntime;
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_coordination::{CoordinationSession, WriteOutcome};
use hyperion_intent::{HandleOutcome, IntentEngine};
use hyperion_ipc::{channel_open, ChannelClass, FrameBody, IpcBus, Operation, Request, SchemaId};
use hyperion_knowledge_graph::KnowledgeGraph;
use serde::{Deserialize, Serialize};

const SENDER: TrustBoundaryId = TrustBoundaryId(1);
const RECEIVER: TrustBoundaryId = TrustBoundaryId(2);
const PROPOSE_WRITE_OP: Operation = Operation(1);

/// The real `propose_write` call, reshaped as a serializable message —
/// docs/12's own "message shape is real" framing, made literal.
#[derive(Serialize, Deserialize)]
struct ProposeWriteMessage {
    session_id: u64,
    agent_instance: u64,
    key: String,
    base_version: u64,
    value: serde_json::Value,
}

#[test]
fn a_propose_write_message_survives_a_real_ipc_hop_and_is_applied_for_real() {
    let mut monitor = CapabilityMonitor::new();
    let receiver_root = monitor.mint_root(RightsMask::all(), RECEIVER, None);
    let sender_token = monitor
        .cap_derive(&receiver_root, RightsMask::WRITE, None, SENDER)
        .expect("attenuating WRITE out of an all-rights root must succeed");

    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let intent_engine = IntentEngine::new(graph.clone(), context);
    let coordination = Arc::new(CoordinationSession::new(
        Arc::new(AgentRuntime::new(Arc::new(
            hyperion_ai_runtime::LocalAiRuntime::new(
                Box::new(hyperion_ai_runtime::MockBackend),
                8_000,
            ),
        ))),
        graph,
    ));

    let root = match intent_engine
        .handle_utterance(
            &monitor,
            &receiver_root,
            "I need to launch my startup",
            "s1",
        )
        .unwrap()
    {
        HandleOutcome::Submitted(id) => id,
        other => panic!("expected Submitted, got {other:?}"),
    };
    let ticket = intent_engine
        .submit(&monitor, &receiver_root, root)
        .unwrap();
    let session_id = coordination
        .create_session(&monitor, &receiver_root, &intent_engine, &ticket)
        .unwrap();

    let message = ProposeWriteMessage {
        session_id,
        agent_instance: 1,
        key: "product_name".to_string(),
        base_version: 0,
        value: serde_json::json!("Hyperion"),
    };
    let payload = serde_json::to_vec(&message).unwrap();

    let bus = Arc::new(IpcBus::new());
    let rx = bus.create_endpoint(receiver_root.object_id());
    let monitor = Arc::new(Mutex::new(monitor));

    let receiver_thread = {
        let bus = Arc::clone(&bus);
        let monitor = Arc::clone(&monitor);
        let coordination = Arc::clone(&coordination);
        let receiver_root = receiver_root.clone();
        thread::spawn(move || {
            let frame = bus.recv_raw(&rx).expect("sender is still alive");
            let call = {
                let guard = monitor.lock().unwrap();
                bus.authenticate(frame, &guard, RightsMask::WRITE)
                    .expect("the sender's token authorizes this endpoint")
            };
            let bytes = match call.body {
                FrameBody::Payload(bytes) => bytes,
                _ => panic!("expected a payload frame"),
            };
            let received: ProposeWriteMessage = serde_json::from_slice(&bytes)
                .expect("a propose_write message that crossed the wire must still deserialize");

            let outcome = {
                let guard = monitor.lock().unwrap();
                coordination
                    .propose_write(
                        &guard,
                        &receiver_root,
                        received.session_id,
                        received.agent_instance,
                        &received.key,
                        received.base_version,
                        received.value,
                    )
                    .expect("a fresh key at base_version 0 must be accepted")
            };
            let reply_payload = serde_json::to_vec(&outcome).unwrap();
            bus.reply(call.request_id, reply_payload)
                .expect("the sender is still waiting for this reply");
        })
    };

    let chan = {
        let guard = monitor.lock().unwrap();
        channel_open(&guard, &sender_token, SchemaId(1), ChannelClass::Call)
            .expect("a fresh WRITE token must be able to open a channel to the receiver")
    };
    let response = bus
        .ipc_call(
            &chan,
            Request {
                op: PROPOSE_WRITE_OP,
                payload,
            },
            Duration::from_secs(5),
        )
        .expect("the receiver replies before the timeout");

    receiver_thread.join().unwrap();

    let outcome: WriteOutcome = serde_json::from_slice(&response.payload)
        .expect("a WriteOutcome that crossed the wire must still deserialize");
    match outcome {
        WriteOutcome::Accepted { new_version } => assert_eq!(new_version, 1),
        WriteOutcome::Conflict(c) => panic!("expected Accepted, got a conflict: {c:?}"),
    }

    // Confirm the write really landed against the live session, not just
    // that a plausible-looking reply came back.
    let monitor = Arc::try_unwrap(monitor).unwrap().into_inner().unwrap();
    let plan = coordination
        .get_plan(&monitor, &receiver_root, session_id)
        .unwrap();
    assert_eq!(
        plan.facts.get("product_name"),
        Some(&(1, serde_json::json!("Hyperion")))
    );
}
