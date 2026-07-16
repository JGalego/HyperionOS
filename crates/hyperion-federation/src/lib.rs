//! Hyperion L1/L4 Distributed Execution — Phase 7, second slice.
//!
//! Implements docs/21-distributed-execution.md's two additions over the
//! already-real capability and scheduling primitives this workspace has:
//! **which device** a Capability invocation runs on
//! ([`FederationHub::dispatch_offload`]), and **how an in-flight Agent
//! session moves** from one device to another
//! ([`FederationHub::migrate`]). Per docs/41-implementation-phases.md's own
//! Phase 7 guidance, multiple "devices" are simulated as separate Trust
//! Boundaries the same way `hyperion-sim` already simulates two processes
//! as threads — each device here is a genuinely separate, real
//! [`hyperion_agent_runtime::AgentRuntime`] instance with its own
//! capability-derived [`hyperion_capability::TrustBoundaryId`], not a
//! pretend label on one shared instance.
//!
//! Real: federation membership as an ordinary `cap_derive`'d capability
//! grant, one distinct Trust Boundary per device — "remove this device" is
//! the same revocation-graph walk that stops a runaway Agent, no second
//! trust ceremony; offload placement scored against
//! `hyperion-scheduler::ResourceVector`, unmodified, with a hard privacy
//! gate (an unconsented `CloudRented` tier device is architecturally
//! invisible to placement, never merely deprioritized) and stale-ledger
//! invalidation with automatic retry against the next candidate; and
//! session migration that reuses `hyperion-agent-runtime`'s real
//! checkpoint/spawn/terminate machinery *across* two independent
//! `AgentRuntime` instances — the checkpoint's manifest and bound Intent
//! reference genuinely transfer, they are not merely relabeled.
//! [`FederationHub::dispatch_offload`] and [`FederationHub::invoke_agent`]
//! each open a real `hyperion-explainability` Explanation Record around
//! their dispatch (`begin` before, a `ReasoningStep` naming the device/
//! agent, `transition` to `Completed`/`RolledBack`/`Interrupted` on the
//! real outcome) — these were this crate's own two remaining direct
//! `AgentRuntime::invoke` call sites `hyperion-coordination`'s own
//! Explanation Record wiring didn't reach. Both now take a real,
//! caller-supplied `triggering_intent_id`, so a caller that drives a real
//! `hyperion_intent::IntentEngine::submit` first gets a genuine
//! correlation via [`FederationHub::trace_intent`], not a hardcoded
//! sentinel — this crate still doesn't depend on `hyperion-intent`
//! itself, since attributing a dispatch to an Intent needs no Intent
//! Graph structure, only its id. [`FederationHub`] also holds
//! one real `hyperion_observability::TelemetryCollector` per device
//! (minted at [`FederationHub::join_device`], resolved by
//! [`FederationHub::telemetry_for`]), and [`FederationHub::migrate`] is
//! the real production call site for
//! `TelemetryCollector::merge_remote_trace` docs/34's own crate doc named
//! as real but never invoked from anywhere: it pulls whatever a caller
//! recorded on the source device under a migrating agent's `trace_id`
//! into the target device's collector before tearing the source instance
//! down, reconstructing the whole cross-device trace on the target side.
//!
//! [`FederationHub::seal`]/[`FederationHub::open`] (2026-07-16) close
//! this crate's own previously-named "`SyncEnvelope`-wrapped, per-device-
//! encrypted migration payloads" gap for real: every hub now holds its
//! own real [`hyperion_crypto::Keystore`] (a fresh
//! [`hyperion_crypto::Keystore::ephemeral`] identity by default via
//! [`FederationHub::new`], or a real persisted one via
//! [`FederationHub::new_with_keystore`]), and `seal`/`open` really
//! encrypt (ChaCha20-Poly1305) and really sign (Ed25519) a payload
//! through it.
//!
//! [`FederationHub::x25519_public`]/[`FederationHub::establish_shared_secret`]/
//! [`FederationHub::seal_for_peer`]/[`FederationHub::open_from_peer`] (same day) close this
//! crate's own next-named gap: real X25519 Diffie-Hellman key agreement between two genuinely
//! independent, separately-keyed hubs — neither ever learns the other's private key, only its
//! real public X25519 key, and each derives the identical real shared secret independently (see
//! [`hyperion_crypto::key_exchange`]'s own doc comment and tests for the actual DH property
//! proven). `seal`/`open` (above) remain the one-shared-`Keystore` case; `seal_for_peer`/
//! `open_from_peer` are the genuinely-independent-devices case, verifying against the *peer's*
//! real public signing key rather than the opener's own.
//!
//! Deliberately deferred, and why:
//!
//! - **One workspace-wide, shared Explanation Record store.** This hub's
//!   store is private to it, not shared with `hyperion-coordination`'s or
//!   `hyperion-api-gateway`'s own separate stores — the same deliberate
//!   per-owner boundary `hyperion-coordination`'s doc comment already
//!   notes.
//! - **Real network transport, heartbeat timing, ambient anti-entropy.**
//!   Ledger publication and lease renewal are direct method calls driven
//!   by a caller-supplied clock, not a real heartbeat loop; storage
//!   convergence is [28 — Storage Engine](../28-storage-engine.md)'s job
//!   and isn't wired in here (no multi-device KG replica exists yet to
//!   converge). What now *is* real — the payload confidentiality/
//!   authenticity, and now the real key agreement, a wire transport would
//!   need — is `seal`/`open` and `seal_for_peer`/`open_from_peer`, above;
//!   the transport itself (actual sockets carrying these envelopes
//!   between processes) is still deferred.
//! - **Cold-cache pre-staging** (docs/21 §Recovery's priority-sync batch
//!   for a migration target with no local replica) — there is no Context
//!   Bundle replica model across devices yet.
//! - **A separate lease-dispute detector for genuine network partitions**
//!   — [`FederationHub::acquire_lease`]'s deterministic tie-break (more-
//!   trusted tier wins, then lower `device_id`) is real and tested, but
//!   this crate has no way to *simulate* a partition beyond calling
//!   `acquire_lease` from "both sides" in a test, which is what the test
//!   suite does.

mod hub;
mod types;

pub use hub::{FederationError, FederationHub};
pub use types::{
    AnchorLease, FederationTrustTier, MigrationOutcome, MigrationReceipt, OffloadDescriptor,
    PrivacyTier, VirtualResourceLedger,
};
