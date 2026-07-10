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
//! Explanation Record wiring didn't reach.
//!
//! Deliberately deferred, and why:
//!
//! - **A real originating Intent id.** Neither dispatch method has a real
//!   Intent concept to attribute its Explanation Record to yet, so both
//!   record under the sentinel `triggering_intent_id = 0` — see
//!   [`FederationHub::trace_intent`].
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
//!   converge).
//! - **`SyncEnvelope`-wrapped, per-device-encrypted migration payloads**
//!   ([16 — Privacy Architecture](../16-privacy-architecture.md), Phase 8)
//!   — a checkpoint's contents transfer as plain in-process Rust values
//!   here, standing in for what a real envelope would carry.
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
