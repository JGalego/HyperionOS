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
//! [`FederationHub::start_lease_heartbeat`] (same day) closes the "heartbeat timing" half of this
//! crate's own next-named gap: a real background thread renews an `AnchorLease` on a fixed real
//! wall-clock interval (`SystemTime::now`, unlike every other method here, which takes a
//! caller-supplied logical `now`) — ambient, automatic upkeep instead of a caller explicitly
//! calling `renew_lease` itself. The returned `LeaseHeartbeat` handle joins the real thread on
//! drop/`stop()`, so a caller can be sure renewal has genuinely halted before acting on that
//! (e.g. releasing the lease). Ambient anti-entropy (storage convergence) is closed below, by
//! [`kg_sync`] — a heartbeat keeps a *lease* alive; [`KgAntiEntropyHeartbeat`] keeps Knowledge
//! Graph state in sync.
//!
//! [`SchedulerOffloadBridge`] (2026-07-18) closes `hyperion-scheduler`'s own named "distributed
//! offload" gap from this side: that crate's `schedule_epoch` had no live trigger ever reaching
//! its own "offer it for offload" branch, even though [`FederationHub::dispatch_offload`] was
//! already real. This bridge implements `hyperion_scheduler::OffloadTrigger` over
//! `dispatch_offload`, so a caller that owns both a real `Scheduler` and a real `FederationHub`
//! wires it in via `Scheduler::with_offload_trigger`, and a `SchedClass::BatchDistributable` task
//! that fails local admission genuinely reaches a real peer device instead of only ever aging.
//!
//! Deliberately deferred, and why:
//!
//! - ~~One workspace-wide, shared Explanation Record store~~ — now real for a caller that wants
//!   it: [`FederationHub::new_with_shared_explanations`] takes a real, caller-supplied
//!   `Arc<ExplanationStore>` instead of building its own private one, the same store a
//!   `hyperion_coordination::CoordinationSession` (or a `hyperion-api-gateway::ApiGateway`, which
//!   already took one) can share too. Every real `action_id` this hub mints now comes from the
//!   store's own `ExplanationStore::next_action_id`, not this hub's former private counter —
//!   `hyperion-coordination` made the identical change the same pass, closing the exact collision
//!   risk sharing a store without also sharing that counter would otherwise create. `new`/
//!   `new_with_keystore` are unchanged (still build a private store; every existing call site
//!   keeps compiling). Proven end to end, cross-crate: see `hyperion-coordination`'s own
//!   `tests/shared_explanation_store.rs`.
//! - ~~Real network transport~~ — now real: [`transport::serve_ledger_publications`] runs a
//!   real background thread accepting real `TcpListener` connections, and
//!   [`transport::publish_ledger_over_socket`] is the real client half — a
//!   [`transport::LedgerPublication`] genuinely travels, `seal_for_peer`-encrypted and signed,
//!   over a real `TcpStream` between two independent [`FederationHub`] instances, and is only
//!   applied via the receiving hub's own already-real [`FederationHub::publish_ledger`] once
//!   authentication and decryption both genuinely succeed.
//! - ~~**Ambient anti-entropy (Knowledge Graph replication across devices).**~~ (2026-07-18) —
//!   this bullet used to say "storage convergence is [28 — Storage Engine](../28-storage-engine.md)'s
//!   job and isn't wired in here"; `hyperion-storage`'s own crate doc said the reverse ("multi-device
//!   sync is [21 — Distributed Execution](21-distributed-execution.md)'s concern") — neither crate
//!   actually owned it. [`kg_sync`] closes it here, deliberately scoped down from docs/28's own
//!   full Merkle-diff/CRDT design to a real, bounded, whole-snapshot replication: [`merge_snapshot`]
//!   is the real "apply a remote node/edge into my own local graph" primitive this workspace had
//!   nowhere (translating remote `NodeId`s to local ones via [`KgTranslation`], since two
//!   independent graphs mint ids from independent counters); [`serve_kg_snapshots`]/
//!   [`publish_snapshot_over_socket`] really move a `hyperion_knowledge_graph::GraphSnapshot`
//!   between two devices over the same real `seal_for_peer`/`open_from_peer`-encrypted `TcpStream`
//!   pattern `transport` already established; and [`KgAntiEntropyHeartbeat`] is the real ambient
//!   half — a background thread that keeps re-publishing on a fixed real interval with no caller
//!   ever triggering a sync by hand, the same real-thread-with-join-on-drop shape
//!   [`LeaseHeartbeat`] already established. See [`kg_sync`]'s own doc comment for the real,
//!   honestly-named boundaries this doesn't yet cross (translation table not persisted across a
//!   restart; last-applied-wins, not true CRDT conflict merge).
//! - ~~**The Fleet aggregate-submission network endpoint** (`Fleet.submitAggregate`).~~
//!   (2026-07-18) — `hyperion-observability`'s own crate doc named this as its remaining gap
//!   ("[`aggregate::build_aggregate`] produces the gated report; nothing here sends it anywhere --
//!   no real network transport exists in this hosted simulator"), but that crate can't own the
//!   transport itself: this crate already depends on it, so the reverse direction would be a hard
//!   Cargo cycle -- the same reasoning that placed [`kg_sync`] here rather than in
//!   `hyperion-knowledge-graph`. [`fleet::serve_fleet_submissions`]/
//!   [`fleet::submit_aggregate_over_socket`] replicate [`transport`]'s own real socket shape
//!   exactly (a real `TcpListener` background thread, real `seal_for_peer`/`open_from_peer`
//!   authentication+encryption, length-prefixed frames), carrying a real
//!   `hyperion_observability::AggregateReport` as JSON; [`fleet::FleetAggregateStore`] is the
//!   real, honest, in-memory "Fleet" receiver this workspace had nowhere before.
//! - **Cold-cache pre-staging** (docs/21 §Recovery's priority-sync batch
//!   for a migration target with no local replica) — there is no Context
//!   Bundle replica model across devices yet.
//! - **A separate lease-dispute detector for genuine network partitions**
//!   — [`FederationHub::acquire_lease`]'s deterministic tie-break (more-
//!   trusted tier wins, then lower `device_id`) is real and tested, but
//!   this crate has no way to *simulate* a partition beyond calling
//!   `acquire_lease` from "both sides" in a test, which is what the test
//!   suite does.

mod fleet;
mod hub;
mod kg_sync;
mod offload_bridge;
mod transport;
mod types;

pub use fleet::{
    serve_fleet_submissions, submit_aggregate_over_socket, FleetAggregateStore,
    FleetSubmissionServer, ReceivedAggregate,
};
pub use hub::{FederationError, FederationHub, LeaseHeartbeat};
pub use kg_sync::{
    merge_snapshot, publish_snapshot_over_socket, serve_kg_snapshots, KgAntiEntropyHeartbeat,
    KgMergeReport, KgSnapshotServer, KgTranslation,
};
pub use offload_bridge::SchedulerOffloadBridge;
pub use transport::{
    publish_ledger_over_socket, serve_ledger_publications, LedgerPublication,
    LedgerPublicationServer,
};
pub use types::{
    AnchorLease, FederationTrustTier, MigrationOutcome, MigrationReceipt, OffloadDescriptor,
    PrivacyTier, VirtualResourceLedger,
};
