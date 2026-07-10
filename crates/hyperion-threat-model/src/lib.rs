//! Hyperion Threat Model regression suite — Phase 8, sixth and final
//! slice.
//!
//! Implements docs/17-threat-model.md's `ThreatRegistry` as a literal,
//! queryable catalog ([`catalog`]) and, in `tests/`, a passing regression
//! test per attacker-goal/mitigation pair — the Phase 8 exit criterion:
//! "every attacker-goal/mitigation pair in docs/17 has a passing
//! regression test." Most mitigations are proven by composing this
//! workspace's already-real crates (`hyperion-capability`'s cascade
//! revocation, `hyperion-netstack`'s SSRF/quarantine containment,
//! `hyperion-security`'s risk-assessment floors, `hyperion-privacy`'s
//! least-privilege context assembly, `hyperion-federation`'s lease
//! split-brain tie-break) rather than reimplementing logic that already
//! has its own test suite — this crate's tests are the cross-cutting,
//! often multi-crate scenarios docs/17 describes, not a duplicate of any
//! one crate's unit tests.
//!
//! This crate is deliberately almost all tests, no runtime logic: the
//! [`catalog`] function is the one piece of real library code, existing
//! so the mapping from attacker goal to mitigation owner is a checked
//! Rust value, not only prose in this doc comment.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreatStatus {
    /// A regression test in this crate's `tests/` directory exercises the
    /// mitigation end to end.
    Mitigated,
    /// The mitigation exists in an owning crate but this suite only
    /// covers part of the attack surface docs/17 describes.
    PartiallyMitigated,
}

/// docs/17 §8's `ThreatRecord`, narrowed to `'static` string fields since
/// this crate's catalog is a fixed, compiled-in table, not a runtime
/// database.
#[derive(Debug, Clone, Copy)]
pub struct ThreatRecord {
    pub id: &'static str,
    pub surface: &'static str,
    pub attacker_goal: &'static str,
    pub mitigation: &'static str,
    pub mitigation_owner_crate: &'static str,
    pub severity: Severity,
    pub status: ThreatStatus,
}

/// docs/17 §8's `ThreatRegistry.report/query` — the fixed catalog every
/// `tests/t*.rs` regression test corresponds to exactly one row of.
pub fn catalog() -> Vec<ThreatRecord> {
    vec![
        ThreatRecord {
            id: "T1",
            surface: "Prompt injection into Intents/Context",
            attacker_goal: "Cause an Agent to invoke a Capability the user never asked for, laundered through the Agent's own legitimate authority",
            mitigation: "Channel separation (data vs. instruction) plus intent-provenance taint propagation, flooring intervention at require-explicit-confirm for any action tracing to unconfirmed ingested-external content",
            mitigation_owner_crate: "hyperion-security (taint floor) + hyperion-netstack (quarantine scanner)",
            severity: Severity::High,
            status: ThreatStatus::Mitigated,
        },
        ThreatRecord {
            id: "T2",
            surface: "Malicious/compromised Plugin or Capability supply chain",
            attacker_goal: "Ship or update a Plugin/Capability to exceed its declared Capability contract",
            mitigation: "Kernel-authoritative rights ceiling (manifest is advisory-only); generation-based cascade revocation the instant a deviation is detected",
            mitigation_owner_crate: "hyperion-capability (cap_derive/cap_revoke cascade)",
            severity: Severity::High,
            status: ThreatStatus::Mitigated,
        },
        ThreatRecord {
            id: "T3",
            surface: "Cross-Agent privilege escalation",
            attacker_goal: "Get Agent B to act on Agent A's behalf, laundering authority A lacks",
            mitigation: "No delegated risk assessment: a receiving Agent always re-assesses independently against its own inputs, never honoring a sender's claimed risk level",
            mitigation_owner_crate: "hyperion-security (cross_agent_delegation_verify)",
            severity: Severity::High,
            status: ThreatStatus::Mitigated,
        },
        ThreatRecord {
            id: "T4",
            surface: "Knowledge Graph / Semantic Filesystem poisoning",
            attacker_goal: "Plant a malicious Semantic Object that later surfaces as trusted context",
            mitigation: "Every node/edge write is capability-checked and records its authoring Trust Boundary (owner); ambiguous entity merges are never silently accepted",
            mitigation_owner_crate: "hyperion-knowledge-graph (owner/capability check) + hyperion-netstack (resolve::MatchDecision::Ambiguous)",
            severity: Severity::Medium,
            status: ThreatStatus::PartiallyMitigated,
        },
        ThreatRecord {
            id: "T5",
            surface: "Memory poisoning",
            attacker_goal: "Inject a false \"remembered\" fact to bias a future risk assessment's corroboration signal toward a lower intervention level",
            mitigation: "Corroboration is weighted with a negative coefficient but two unconditional floors (tainted provenance; irreversible+wide-blast-radius) override the weighted score and cannot be bought down by any corroboration value",
            mitigation_owner_crate: "hyperion-security (assess's unconditional floors)",
            severity: Severity::High,
            status: ThreatStatus::Mitigated,
        },
        ThreatRecord {
            id: "T6",
            surface: "Context Propagation leakage across a Trust Boundary",
            attacker_goal: "An over-inclusive Context Bundle exposes objects to a recipient across a Trust Boundary that never needed them",
            mitigation: "Build-for-recipient-contract construction: an object is included only if the recipient's declared sub-intent needs it AND its residency tag permits the recipient's privacy tier; every exclusion is reported with a reason, never silent",
            mitigation_owner_crate: "hyperion-privacy (filter_for_recipient)",
            severity: Severity::Medium,
            status: ThreatStatus::Mitigated,
        },
        ThreatRecord {
            id: "T7",
            surface: "Device-federation impersonation / split-brain",
            attacker_goal: "A compromised, cloned, or stale device claims to be a trusted anchor for an Agent session it no longer legitimately controls",
            mitigation: "Deterministic anchor-lease tie-break (higher trust tier wins, ties by lower device_id); removing a device tears down its Trust Boundary and capability grants instantly",
            mitigation_owner_crate: "hyperion-federation (acquire_lease tie-break) + hyperion-device (revoke)",
            severity: Severity::High,
            status: ThreatStatus::Mitigated,
        },
        ThreatRecord {
            id: "T8",
            surface: "Model supply-chain compromise",
            attacker_goal: "Poisoned model weights silently bias Intent/Agent reasoning",
            mitigation: "Content-hash verification before promotion (blocks regardless of score) plus a canary differential test blocking promotion on score drift",
            mitigation_owner_crate: "hyperion-security (canary_gate_model_promotion)",
            severity: Severity::Critical,
            status: ThreatStatus::Mitigated,
        },
    ]
}

pub fn find(id: &str) -> Option<ThreatRecord> {
    catalog().into_iter().find(|t| t.id == id)
}
