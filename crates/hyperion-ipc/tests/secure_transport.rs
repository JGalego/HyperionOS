//! `hyperion-security`'s own previously-named "Real Noise-protocol IPC handshakes / channel
//! binding" gap, proven end to end over real Unix domain sockets: a real `Noise_NN` handshake
//! establishes a live session between two real [`Endpoint`]s, [`Endpoint::ipc_call_with_claim_secure`]
//! carries a real capability claim *inside* that session rather than as plaintext, and revocation
//! is still enforced exactly as this crate's own plaintext `real_transport.rs` test proves for the
//! unsealed path -- the secure path is a real, additional layer, not a replacement for capability
//! checking.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId, WireToken};
use hyperion_ipc::{Endpoint, IpcFault, Operation, Request, Response, SchemaId};

const TIMEOUT: Duration = Duration::from_secs(2);

#[test]
fn a_real_secure_call_round_trips_and_revocation_is_still_enforced() {
    let dir = tempfile::tempdir().expect("create tempdir for real sockets");
    let server_sock = dir.path().join("server.sock");
    let client_sock = dir.path().join("client.sock");

    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::WRITE, TrustBoundaryId(1), None);
    let claim = WireToken::from(&token);

    let server = Endpoint::bind(&server_sock).expect("bind real server socket");
    let client = Endpoint::bind(&client_sock).expect("bind real client socket");

    // The real handshake: server plays responder (blocks for the client's first message), client
    // plays initiator.
    thread::scope(|scope| {
        let responder_handle =
            scope.spawn(|| server.noise_handshake_as_responder(TIMEOUT).unwrap());
        thread::sleep(Duration::from_millis(20));
        let client_session = client
            .noise_handshake_as_initiator(&server_sock, TIMEOUT)
            .expect("real client-side Noise handshake must succeed");
        let (mut server_session, _from) = responder_handle.join().unwrap();

        assert_eq!(
            client_session.binding(),
            server_session.binding(),
            "both real sides of the same handshake must derive the identical transcript hash"
        );

        // Both real client-side calls run in one spawned thread, owning `client_session` for its
        // whole life -- `thread::scope` requires anything a spawned closure borrows to live for
        // the entire scope block, so reborrowing `&mut client_session` across two *sequential*
        // `scope.spawn` calls (as two separate closures) doesn't type-check even though the
        // borrows never actually overlap at runtime. A real `mpsc` handoff lets the main thread
        // (running the server side) tell this one exactly when the token has been revoked, so
        // both real phases still happen in the right order.
        let (revoked_tx, revoked_rx) = mpsc::channel::<()>();
        let call_thread = scope.spawn(move || {
            let mut client_session = client_session;
            let first = client.ipc_call_with_claim_secure(
                &server_sock,
                &claim,
                SchemaId(1),
                Request {
                    op: Operation(1),
                    payload: b"ping".to_vec(),
                },
                &mut client_session,
                TIMEOUT,
            );
            revoked_rx
                .recv()
                .expect("main thread signals after revoking");
            let second = client.ipc_call_with_claim_secure(
                &server_sock,
                &claim,
                SchemaId(1),
                Request {
                    op: Operation(1),
                    payload: b"ping-again".to_vec(),
                },
                &mut client_session,
                TIMEOUT,
            );
            (first, second)
        });

        // Phase 1 server side: the token is live. A real secure call over the real,
        // just-negotiated session gets a real, sealed reply.
        let incoming = server
            .recv_raw_secure(&mut server_session, TIMEOUT)
            .expect("receive the real sealed CALL frame");
        let call = server
            .authenticate_secure(&incoming, &monitor, RightsMask::WRITE, &mut server_session)
            .expect("a live capability must authenticate over the real secure channel");
        assert!(call.is_call);
        server
            .reply_secure(&incoming, b"pong".to_vec(), &mut server_session)
            .expect("reply sealed under the real live session");

        // Phase 2 server side: revoke the token -- the secure channel changes nothing about
        // capability enforcement, it only changes what travels in the clear.
        monitor.cap_revoke(&token);
        revoked_tx
            .send(())
            .expect("client thread still waiting on this signal");

        let incoming = server
            .recv_raw_secure(&mut server_session, TIMEOUT)
            .expect("receive the real sealed second CALL frame");
        let auth_result =
            server.authenticate_secure(&incoming, &monitor, RightsMask::WRITE, &mut server_session);
        assert!(
            auth_result.is_err(),
            "a revoked capability must be rejected even over an already-established secure session"
        );

        let (first, second): (Result<Response, IpcFault>, Result<Response, IpcFault>) =
            call_thread.join().unwrap();
        assert_eq!(
            first
                .expect("the real secure round trip must succeed")
                .payload,
            b"pong"
        );
        assert!(
            matches!(second, Err(IpcFault::Kernel(_))),
            "the client must see the real revocation fault, sealed back over the same live \
             session: {second:?}"
        );
    });
}

#[test]
fn a_secure_message_is_never_valid_plaintext_json_on_the_wire() {
    let dir = tempfile::tempdir().expect("create tempdir for real sockets");
    let server_sock = dir.path().join("server.sock");
    let client_sock = dir.path().join("client.sock");

    let server = Endpoint::bind(&server_sock).expect("bind real server socket");
    let client = Endpoint::bind(&client_sock).expect("bind real client socket");

    thread::scope(|scope| {
        let responder_handle =
            scope.spawn(|| server.noise_handshake_as_responder(TIMEOUT).unwrap());
        thread::sleep(Duration::from_millis(20));
        let mut client_session = client
            .noise_handshake_as_initiator(&server_sock, TIMEOUT)
            .unwrap();
        let _ = responder_handle.join().unwrap();

        let sealed = client_session.encrypt(b"a real capability claim").unwrap();
        assert!(
            serde_json::from_slice::<serde_json::Value>(&sealed).is_err(),
            "a real Noise-sealed message must never be valid, readable JSON -- if it parses, \
             nothing was actually encrypted"
        );
        assert!(
            !sealed
                .windows(b"capability".len())
                .any(|w| w == b"capability"),
            "the plaintext claim's own bytes must never appear verbatim in the sealed output"
        );
    });
}
