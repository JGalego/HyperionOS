//! docs/07-context-propagation.md §Interfaces: "Context Propagation owns
//! only the envelope contract... not the bytes-on-the-wire transport."
//! `export`/`import` on their own only prove the contract's *shape* --
//! constructed and immediately consumed in the same call, on the same
//! Trust Boundary. This proves the envelope genuinely survives a real
//! `hyperion-ipc` hop between two separate Trust Boundaries: serialized
//! to bytes, sent as a real `NOTIFY` frame over a real `IpcBus` channel,
//! received on a different thread, deserialized, and imported cleanly.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{
    Budget, ContextEngine, ContextEnvelope, ContextPropagation, RedactionAction, RedactionPolicy,
    Representation, Scope, TrustLevel,
};
use hyperion_ipc::{
    channel_open, ChannelClass, FrameBody, IpcBus, Notification, Operation, SchemaId,
};
use hyperion_knowledge_graph::KnowledgeGraph;

const SENDER: TrustBoundaryId = TrustBoundaryId(1);
const RECEIVER: TrustBoundaryId = TrustBoundaryId(2);
const CONTEXT_ENVELOPE_OP: Operation = Operation(1);

#[test]
fn an_exported_envelope_survives_a_real_ipc_hop_and_imports_cleanly() {
    let mut monitor = CapabilityMonitor::new();
    // The Receiver owns the endpoint object the Sender's channel targets --
    // the same "server mints the root, client holds an attenuated token to
    // the same object" shape `hyperion-sim` already establishes.
    let receiver_root = monitor.mint_root(RightsMask::all(), RECEIVER, None);
    let sender_token = monitor
        .cap_derive(&receiver_root, RightsMask::WRITE, None, SENDER)
        .expect("attenuating WRITE out of an all-rights root must succeed");

    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let node_id = graph
        .put_node(
            &monitor,
            &receiver_root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "hello from the sender"}),
        )
        .unwrap();

    let context_engine = ContextEngine::new(graph.clone());
    let bundle = context_engine
        .assemble(
            &monitor,
            &receiver_root,
            &Scope {
                intent_id: "i1".to_string(),
                session_id: "s1".to_string(),
                mentions: Vec::new(),
                anchors: vec![node_id],
            },
            Budget::default(),
        )
        .unwrap();

    let propagation = ContextPropagation::new(graph.clone());
    let mut field_rules = HashMap::new();
    field_rules.insert("Note".to_string(), RedactionAction::Pass);
    let policy = RedactionPolicy::new(TrustLevel::TrustedAgent, field_rules);
    let envelope = propagation
        .export(
            &monitor,
            &receiver_root,
            &bundle,
            TrustLevel::TrustedAgent,
            &policy,
            3600,
        )
        .unwrap();
    // A real byte payload, not a Rust reference handed across a function
    // call -- this is the actual "bytes-on-the-wire" shape docs/07 says
    // this crate deliberately doesn't own.
    let payload = serde_json::to_vec(&envelope).unwrap();

    let bus = Arc::new(IpcBus::new());
    let rx = bus.create_endpoint(receiver_root.object_id());
    let monitor = Arc::new(Mutex::new(monitor));

    let receiver_thread = {
        let bus = Arc::clone(&bus);
        let monitor = Arc::clone(&monitor);
        thread::spawn(move || {
            let frame = bus.recv_raw(&rx).expect("sender is still alive");
            let call = {
                let guard = monitor.lock().unwrap();
                bus.authenticate(frame, &guard, RightsMask::WRITE)
                    .expect("the sender's token authorizes this endpoint")
            };
            match call.body {
                FrameBody::Payload(bytes) => bytes,
                _ => panic!("expected a payload frame"),
            }
        })
    };

    let chan = {
        let guard = monitor.lock().unwrap();
        channel_open(&guard, &sender_token, SchemaId(1), ChannelClass::Notify)
            .expect("a fresh WRITE token must be able to open a channel to the receiver")
    };
    bus.ipc_notify(
        &chan,
        Notification {
            op: CONTEXT_ENVELOPE_OP,
            payload,
        },
    )
    .expect("the receiver's endpoint is still registered");

    let received_bytes = receiver_thread.join().unwrap();
    let received_envelope: ContextEnvelope = serde_json::from_slice(&received_bytes)
        .expect("an envelope that crossed the wire must still deserialize cleanly");

    let monitor = Arc::try_unwrap(monitor).unwrap().into_inner().unwrap();
    let (entries, _freshness) = propagation
        .import(&monitor, &receiver_root, received_envelope)
        .expect("a freshly exported, unmodified, unreplayed envelope must import cleanly");

    assert_eq!(entries.len(), 1);
    match &entries[0].representation {
        Representation::ByValue { content, .. } => {
            assert_eq!(content["text"], "hello from the sender");
        }
        other => panic!("expected real content to have survived the real IPC hop, got {other:?}"),
    }
}
