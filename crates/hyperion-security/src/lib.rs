//! Hyperion L1-cross-cutting Security Architecture — Phase 8, second
//! slice.
//!
//! Implements docs/15-security-architecture.md's Risk-Assessment Engine
//! (the literal Phase 8 exit criterion: "a risky action — deleting many
//! Semantic Objects — correctly triggers backup-then-confirm... rather
//! than a blanket dialog"), plus the three docs/17-threat-model.md
//! mitigations that have no coverage in any already-built crate: the
//! provenance-taint floor (T1/T3), the cross-Agent "no delegated risk
//! assessment" rule (T3), and the model canary-gate (T8).
//!
//! Real: [`engine::assess`] is docs/15 §7's exact algorithm — a weighted
//! composite over blast radius/reversibility/sensitivity/confidence/
//! corroboration, with two *unconditional* floors (tainted provenance;
//! irreversible-and-wide-blast-radius) that override the weighted score
//! rather than folding into it, closing the gap where the corroboration
//! term's negative weight could otherwise buy a maximal-risk action just
//! under the backup-first threshold — this is docs/17 T5's exact concern,
//! and is the one thing this crate's test suite most directly regression-
//! tests. [`engine::assess_and_prepare`] is docs/15's relationship to
//! [33 — Rollback & Recovery](../33-rollback-recovery.md): it calls
//! `hyperion_recovery::RecoveryService::recovery_point_create`
//! synchronously, in the request path, before any `RequireBackupFirst`
//! action is allowed to proceed — the recovery point is a precondition of
//! execution, per docs/33's own framing ("15 calls into 33, not the
//! reverse"). [`engine::cross_agent_delegation_verify`] enforces docs/17
//! T3's rule that a receiving Agent must never honor a sender's claimed
//! risk level. [`model_integrity::canary_gate_model_promotion`]
//! implements docs/17 T8 on top of `hyperion-ai-runtime`'s existing
//! checksum and a deterministic canary-score-drift comparison.
//!
//! Deliberately deferred, and why:
//!
//! - **`CapabilityGrant`/`AttenuationRecord`/`SandboxProfile`/
//!   `IPCSession`.** docs/15 §4 defines a subject-facing provenance/
//!   delegation-chain bookkeeping layer over capability tokens. This
//!   workspace's `hyperion-capability` crate already *is* the real,
//!   authoritative enforcement point (generation-based revocation,
//!   attenuation-only `cap_derive`) docs/15 §7's `capability_check`
//!   pseudocode explicitly says to call into rather than duplicate.
//!   Building a second bookkeeping layer on top, with no Phase 8 exit
//!   criterion that needs it, would be scope without a test to justify
//!   it — deferred to whichever future phase (most likely Phase 9's SDK/
//!   Plugin Framework) first needs delegation-chain provenance a bare
//!   `CapabilityToken` doesn't carry.
//! - **Device attestation (T7) and hardware root-of-trust /secure-
//!   enclave signing (§8).** No real hardware exists in a hosted
//!   simulator; `hyperion-federation`'s lease split-brain tie-break
//!   already covers the device-identity-conflict shape T7 cares about at
//!   the level this workspace can test it.
//! - **Real Noise-protocol IPC handshakes / channel binding.** Stubbed
//!   entirely; this workspace's IPC (`hyperion-ipc`) has no session-key
//!   concept to bind against yet.
//! - **`ProvenanceRecord`/trust-scoring for Knowledge Graph poisoning
//!   (T4).** `hyperion-knowledge-graph` already records `owner`/
//!   `device_origin` per node (its own crate doc's "Per-object ACL
//!   enforcement" deferral); layering a trust score on top with no
//!   consumer yet is deferred alongside that.
//! - ~~**Blast-radius/sensitivity/reversibility *classifiers*.**~~ — narrowed, not fully closed:
//!   [`engine::verify_action`] (wired into [`engine::assess_and_prepare`], the one real
//!   production entry point) re-derives `scope_size` from the real `object_refs.len()` rather
//!   than trusting a separately claimed number, and downgrades `reversible`/escalates
//!   `sensitivity` whenever any referenced object can't actually be read back by this same token
//!   via `hyperion-knowledge-graph` — a caller can no longer claim a safe, reversible,
//!   low-sensitivity action against objects it cannot itself verify are real, live, and theirs.
//!   Full *content-based* sensitivity classification (what does this object's real content mean
//!   for privacy risk) is still future work — docs/15 §7's real classifier pipeline this crate's
//!   own scoring layer consumes — since no crate in this workspace maintains a real per-object
//!   sensitivity registry to query yet (`hyperion-privacy::ResidencyTag` is a plain struct
//!   threaded call-to-call today, not a store).

mod engine;
mod model_integrity;
mod types;

pub use engine::{assess, assess_and_prepare, cross_agent_delegation_verify, verify_action};
pub use model_integrity::canary_gate_model_promotion;
pub use types::{
    ActionId, CanaryResult, IntentProvenanceChain, InterventionLevel, ModelIntegrityRecord,
    OriginType, PendingAction, PromotionStatus, ProvenanceNode, RiskAssessment, SecurityError,
    SensitivityHint,
};
