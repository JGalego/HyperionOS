//! Real Noise Protocol Framework handshakes and session-key binding -- `hyperion-security`'s own
//! previously-named "Real Noise-protocol IPC handshakes / channel binding" gap ("Stubbed
//! entirely; this workspace's IPC (`hyperion-ipc`) has no session-key concept to bind against
//! yet"). Uses the real, canonical `snow` crate (the actual Noise Protocol Framework
//! implementation the gap names by name) rather than a bespoke reimplementation of a specified
//! protocol.
//!
//! `Noise_NN_25519_ChaChaPoly_BLAKE2s`: anonymous (no long-term static keys on either side),
//! ephemeral X25519 Diffie-Hellman, ChaCha20-Poly1305 AEAD, BLAKE2s transcript hashing.
//! Deliberately `NN`, not `IK`/`XX`: caller identity and authorization are already this crate's
//! own, separate, capability-token layer (every [`crate::Frame`] already carries a
//! [`hyperion_capability::WireToken`] claim, re-validated live by the receiver against its own
//! revocation graph) -- duplicating that as a second, Noise-static-key identity system would be
//! exactly the "second bookkeeping layer... with no exit criterion that needs it" `hyperion-
//! security`'s own crate doc already declines to build for `CapabilityGrant`/`AttenuationRecord`.
//! What Noise adds here is the piece that genuinely didn't exist: real transport confidentiality
//! and integrity over the (today, plaintext-JSON) socket, and a fresh, unforgeable per-handshake
//! *session* a capability claim now travels inside rather than alongside.
//!
//! [`NoiseSession::binding`] is the handshake's own real BLAKE2s transcript hash
//! (`HandshakeState::get_handshake_hash`) -- Noise's own real channel-binding value, the same
//! "derive a binding token from the handshake transcript, not from anything either party could
//! choose independently" property TLS channel binding (`tls-exporter`) gives. Two sessions -- even
//! two negotiated between the same pair of processes moments apart -- never share a binding value,
//! since each handshake's own fresh ephemeral keys feed the transcript; [`NoiseSession::encrypt`]/
//! [`NoiseSession::decrypt`] can only ever interoperate with the exact live session that produced
//! them, so a frame sealed under one session is real, cryptographic garbage to every other
//! session -- including a session negotiated later between the very same two endpoints.

use std::io;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::types::IpcFault;

/// Anonymous (no static keys) -- see this module's own doc comment for why identity is this
/// crate's separate capability-token layer's job, not Noise's.
const NOISE_PATTERN: &str = "Noise_NN_25519_ChaChaPoly_BLAKE2s";
/// Generous for an `NN` handshake message (one ephemeral public key plus a short MAC over an
/// empty payload) -- real messages here are on the order of tens of bytes, never close to this.
const MAX_HANDSHAKE_MSG: usize = 256;
/// Generous for one Noise transport message wrapping a [`crate::Frame`]'s own serialized bytes --
/// mirrors [`crate::transport`]'s own identical `MAX_DATAGRAM_BYTES`, since a sealed frame is a
/// plaintext frame plus a fixed, small AEAD tag overhead.
const MAX_TRANSPORT_MSG: usize = 64 * 1024;

fn params() -> snow::params::NoiseParams {
    NOISE_PATTERN
        .parse()
        .expect("this module's own fixed, valid Noise pattern string always parses")
}

fn noise_err(_: snow::Error) -> IpcFault {
    IpcFault::HandshakeFailed
}

/// A real, live Noise session, past its handshake and ready to seal/open transport messages --
/// this module's own real "session-key binding" primitive. See this module's own doc comment for
/// [`Self::binding`]'s real channel-binding meaning.
pub struct NoiseSession {
    transport: snow::TransportState,
    binding: Vec<u8>,
}

impl NoiseSession {
    /// This session's own real, unique Noise handshake transcript hash -- proof that both sides
    /// completed the *same* real handshake, not just any two independently-negotiated sessions
    /// that happen to be able to talk to each other.
    pub fn binding(&self) -> &[u8] {
        &self.binding
    }

    /// Really seals `plaintext` under this session's live transport key -- ChaCha20-Poly1305 AEAD,
    /// with Noise's own internal nonce that increments on every real call, so the exact same
    /// plaintext seals to different bytes each time.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, IpcFault> {
        let mut out = vec![0u8; plaintext.len() + 32];
        let len = self
            .transport
            .write_message(plaintext, &mut out)
            .map_err(noise_err)?;
        out.truncate(len);
        Ok(out)
    }

    /// The real inverse of [`Self::encrypt`] -- a tampered, replayed (this session's own nonce
    /// already moved past it), or wrongly-keyed ciphertext is a real, honest
    /// [`IpcFault::HandshakeFailed`], never a silent or partial decrypt.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, IpcFault> {
        let mut out = vec![0u8; ciphertext.len()];
        let len = self
            .transport
            .read_message(ciphertext, &mut out)
            .map_err(noise_err)?;
        out.truncate(len);
        Ok(out)
    }
}

fn recv_with_deadline(
    socket: &UnixDatagram,
    buf: &mut [u8],
    deadline: Instant,
) -> Result<(usize, SocketAddr), IpcFault> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(IpcFault::Timeout);
    }
    socket
        .set_read_timeout(Some(remaining))
        .map_err(|_| IpcFault::PeerUnreachable)?;
    socket.recv_from(buf).map_err(|e| {
        if matches!(
            e.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ) {
            IpcFault::Timeout
        } else {
            IpcFault::PeerUnreachable
        }
    })
}

/// The real initiator half of a two-message `Noise_NN` handshake, carried over `socket` as two
/// plain datagrams to/from `peer_path` -- the same real `UnixDatagram` [`crate::transport::Endpoint`]
/// already binds, so no second socket or listener is needed. Blocks until the responder's own
/// message arrives or `timeout` elapses.
pub fn handshake_as_initiator(
    socket: &UnixDatagram,
    peer_path: impl AsRef<Path>,
    timeout: Duration,
) -> Result<NoiseSession, IpcFault> {
    let mut hs = snow::Builder::new(params())
        .build_initiator()
        .map_err(noise_err)?;

    let mut msg1 = vec![0u8; MAX_HANDSHAKE_MSG];
    let len1 = hs.write_message(&[], &mut msg1).map_err(noise_err)?;
    socket
        .send_to(&msg1[..len1], peer_path.as_ref())
        .map_err(|_| IpcFault::PeerUnreachable)?;

    let deadline = Instant::now() + timeout;
    let mut buf = vec![0u8; MAX_HANDSHAKE_MSG];
    let (n, _from) = recv_with_deadline(socket, &mut buf, deadline)?;
    let mut discard = vec![0u8; MAX_HANDSHAKE_MSG];
    hs.read_message(&buf[..n], &mut discard)
        .map_err(noise_err)?;

    let binding = hs.get_handshake_hash().to_vec();
    let transport = hs.into_transport_mode().map_err(noise_err)?;
    Ok(NoiseSession { transport, binding })
}

/// The real responder half of a two-message `Noise_NN` handshake: blocks for *any* peer's first
/// handshake message on `socket`, replies with the second message, and returns both the live
/// session and the peer's address (needed to know who to reply to, mirroring
/// [`crate::transport::IncomingFrame`]'s own identical "the socket already tells us who sent
/// this" shape).
pub fn handshake_as_responder(
    socket: &UnixDatagram,
    timeout: Duration,
) -> Result<(NoiseSession, SocketAddr), IpcFault> {
    let mut hs = snow::Builder::new(params())
        .build_responder()
        .map_err(noise_err)?;

    let deadline = Instant::now() + timeout;
    let mut buf = vec![0u8; MAX_HANDSHAKE_MSG];
    let (n, from) = recv_with_deadline(socket, &mut buf, deadline)?;
    let mut discard = vec![0u8; MAX_HANDSHAKE_MSG];
    hs.read_message(&buf[..n], &mut discard)
        .map_err(noise_err)?;

    let mut msg2 = vec![0u8; MAX_HANDSHAKE_MSG];
    let len2 = hs.write_message(&[], &mut msg2).map_err(noise_err)?;
    socket
        .send_to_addr(&msg2[..len2], &from)
        .map_err(|_| IpcFault::PeerUnreachable)?;

    let binding = hs.get_handshake_hash().to_vec();
    let transport = hs.into_transport_mode().map_err(noise_err)?;
    Ok((NoiseSession { transport, binding }, from))
}

/// Seals `bytes` under `session` and sends the result to `peer_path` as one real datagram -- the
/// real "channel binding" send half [`crate::transport::Endpoint`]'s own secure call/notify paths
/// use instead of sending a [`crate::Frame`]'s serialized bytes in the clear.
pub fn send_secure(
    socket: &UnixDatagram,
    peer_path: impl AsRef<Path>,
    session: &mut NoiseSession,
    bytes: &[u8],
) -> Result<(), IpcFault> {
    let sealed = session.encrypt(bytes)?;
    socket
        .send_to(&sealed, peer_path.as_ref())
        .map_err(|_| IpcFault::PeerUnreachable)?;
    Ok(())
}

/// As [`send_secure`], but replying directly to an already-known `SocketAddr` (e.g. from a prior
/// [`crate::transport::IncomingFrame`]) rather than resolving a filesystem path again -- the real
/// secure counterpart of [`crate::transport::Endpoint`]'s own `reply`/`reply_fault_to`.
pub fn send_secure_to_addr(
    socket: &UnixDatagram,
    to: &SocketAddr,
    session: &mut NoiseSession,
    bytes: &[u8],
) -> Result<(), IpcFault> {
    let sealed = session.encrypt(bytes)?;
    socket
        .send_to_addr(&sealed, to)
        .map_err(|_| IpcFault::PeerUnreachable)?;
    Ok(())
}

/// Receives one real sealed datagram and opens it under `session` -- the real receive half of
/// [`send_secure`].
pub fn recv_secure(
    socket: &UnixDatagram,
    session: &mut NoiseSession,
    timeout: Duration,
) -> Result<(Vec<u8>, SocketAddr), IpcFault> {
    let deadline = Instant::now() + timeout;
    let mut buf = vec![0u8; MAX_TRANSPORT_MSG];
    let (n, from) = recv_with_deadline(socket, &mut buf, deadline)?;
    let plaintext = session.decrypt(&buf[..n])?;
    Ok((plaintext, from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn bound_socket(dir: &tempfile::TempDir, name: &str) -> (UnixDatagram, std::path::PathBuf) {
        let path = dir.path().join(name);
        let socket = UnixDatagram::bind(&path).unwrap();
        (socket, path)
    }

    #[test]
    fn a_real_handshake_produces_matching_bindings_on_both_sides() {
        let dir = tempfile::tempdir().unwrap();
        let (initiator_socket, initiator_path) = bound_socket(&dir, "initiator.sock");
        let (responder_socket, responder_path) = bound_socket(&dir, "responder.sock");

        let responder_thread = thread::spawn(move || {
            handshake_as_responder(&responder_socket, Duration::from_secs(2)).unwrap()
        });
        // Give the responder a moment to be blocked in recv before the initiator's first message
        // is sent -- a real UnixDatagram send to an already-bound socket never actually needs
        // this for correctness (the kernel queues it), but keeps this test's own timing honest.
        thread::sleep(Duration::from_millis(20));

        let initiator_session =
            handshake_as_initiator(&initiator_socket, &responder_path, Duration::from_secs(2))
                .unwrap();
        let (responder_session, from) = responder_thread.join().unwrap();

        assert_eq!(
            from.as_pathname(),
            Some(initiator_path.as_path()),
            "the responder must see the real initiator's own bound address"
        );
        assert_eq!(
            initiator_session.binding(),
            responder_session.binding(),
            "both real sides of the same handshake must derive the identical transcript hash"
        );
    }

    #[test]
    fn two_independent_handshakes_never_share_a_binding() {
        let dir = tempfile::tempdir().unwrap();

        let make_session_pair = |suffix: &str| {
            let (initiator_socket, _initiator_path) = bound_socket(&dir, &format!("i{suffix}"));
            let (responder_socket, responder_path) = bound_socket(&dir, &format!("r{suffix}"));
            let responder_thread = thread::spawn(move || {
                handshake_as_responder(&responder_socket, Duration::from_secs(2))
                    .unwrap()
                    .0
            });
            thread::sleep(Duration::from_millis(20));
            let initiator_session =
                handshake_as_initiator(&initiator_socket, &responder_path, Duration::from_secs(2))
                    .unwrap();
            let responder_session = responder_thread.join().unwrap();
            (initiator_session, responder_session)
        };

        let (a_initiator, _a_responder) = make_session_pair("_a");
        let (b_initiator, _b_responder) = make_session_pair("_b");

        assert_ne!(
            a_initiator.binding(),
            b_initiator.binding(),
            "two genuinely independent handshakes must never produce the same binding"
        );
    }

    #[test]
    fn a_message_encrypted_under_one_session_cannot_be_decrypted_by_another() {
        let dir = tempfile::tempdir().unwrap();
        let (initiator_socket, _initiator_path) = bound_socket(&dir, "initiator.sock");
        let (responder_socket, responder_path) = bound_socket(&dir, "responder.sock");

        let responder_thread = thread::spawn(move || {
            handshake_as_responder(&responder_socket, Duration::from_secs(2)).unwrap()
        });
        thread::sleep(Duration::from_millis(20));
        let mut initiator_session =
            handshake_as_initiator(&initiator_socket, &responder_path, Duration::from_secs(2))
                .unwrap();
        let (mut responder_session, _from) = responder_thread.join().unwrap();

        let sealed = initiator_session.encrypt(b"real secret payload").unwrap();
        let opened = responder_session.decrypt(&sealed).unwrap();
        assert_eq!(opened, b"real secret payload");

        // A completely unrelated third session must never open it.
        let dir2 = tempfile::tempdir().unwrap();
        let (foreign_initiator_socket, _foreign_initiator_path) = bound_socket(&dir2, "fi.sock");
        let (foreign_responder_socket, foreign_responder_path) = bound_socket(&dir2, "fr.sock");
        let foreign_responder_thread = thread::spawn(move || {
            handshake_as_responder(&foreign_responder_socket, Duration::from_secs(2))
                .unwrap()
                .0
        });
        thread::sleep(Duration::from_millis(20));
        let _foreign_initiator_session = handshake_as_initiator(
            &foreign_initiator_socket,
            &foreign_responder_path,
            Duration::from_secs(2),
        )
        .unwrap();
        let mut foreign_responder_session = foreign_responder_thread.join().unwrap();

        let sealed_again = initiator_session.encrypt(b"another real message").unwrap();
        assert!(
            foreign_responder_session.decrypt(&sealed_again).is_err(),
            "a frame sealed under one live session must be real cryptographic garbage to an \
             unrelated session, even one using the exact same Noise pattern"
        );
    }

    #[test]
    fn send_secure_and_recv_secure_round_trip_over_a_real_socket() {
        let dir = tempfile::tempdir().unwrap();
        let (initiator_socket, _initiator_path) = bound_socket(&dir, "initiator.sock");
        let (responder_socket, responder_path) = bound_socket(&dir, "responder.sock");

        // The responder side (handshake + the one real message it then receives) all runs in the
        // spawned thread, so `responder_socket` is only ever owned by one side -- avoiding a
        // needless `Arc` just to share a socket this test never actually touches concurrently.
        let responder_thread = thread::spawn(move || {
            let (mut session, _from) =
                handshake_as_responder(&responder_socket, Duration::from_secs(2)).unwrap();
            let (received, _from) =
                recv_secure(&responder_socket, &mut session, Duration::from_secs(2)).unwrap();
            received
        });
        thread::sleep(Duration::from_millis(20));
        let mut initiator_session =
            handshake_as_initiator(&initiator_socket, &responder_path, Duration::from_secs(2))
                .unwrap();

        send_secure(
            &initiator_socket,
            &responder_path,
            &mut initiator_session,
            b"a real, bound transport message",
        )
        .unwrap();
        let received = responder_thread.join().unwrap();
        assert_eq!(received, b"a real, bound transport message");
    }
}
