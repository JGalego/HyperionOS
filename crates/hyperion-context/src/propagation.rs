use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_knowledge_graph::{GraphError, KnowledgeGraph, NodeId};
use serde::{Deserialize, Serialize};

use crate::types::ContextBundle;
use crate::ContextEntry;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// A non-cryptographic checksum, not a real digital signature — see this
/// crate's doc comment's "Real signatures and trust-level classification"
/// deferral. Enough to prove and test fail-closed-on-tamper behavior; not a
/// security boundary until [15 — Security Architecture](../15-security-architecture.md)
/// (Phase 8) supplies real key material.
fn checksum(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// docs/07 §Algorithms 2: the recipient's classification, driving which
/// `RedactionPolicy` applies. Caller-supplied rather than derived from a
/// real Security Architecture `classify()` — see this crate's doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustLevel {
    /// Local IPC hop within the same Trust Boundary — the only level where
    /// `by_reference` is ever chosen (docs/07 §Algorithms 1).
    SameBoundary,
    TrustedAgent,
    SandboxedCapability,
    RemoteDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RedactionAction {
    Pass,
    Summarize,
    Redact,
}

/// docs/07 §Data Structures' `RedactionPolicy`. `default_action` is fixed to
/// `Redact` at construction — fail-closed by construction, per docs/07
/// §Algorithms 2: "the default action for any category not explicitly
/// listed... is `redact`."
#[derive(Debug, Clone)]
pub struct RedactionPolicy {
    pub target_trust_level: TrustLevel,
    pub field_rules: HashMap<String, RedactionAction>,
}

impl RedactionPolicy {
    pub fn new(
        target_trust_level: TrustLevel,
        field_rules: HashMap<String, RedactionAction>,
    ) -> Self {
        RedactionPolicy {
            target_trust_level,
            field_rules,
        }
    }

    pub fn rule_for(&self, category: &str) -> RedactionAction {
        self.field_rules
            .get(category)
            .copied()
            .unwrap_or(RedactionAction::Redact)
    }
}

/// docs/07 §Data Structures' `EnvelopeEntry.representation`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Representation {
    ByReference {
        node_id: NodeId,
    },
    ByValue {
        node_id: NodeId,
        content: serde_json::Value,
    },
    RedactedPlaceholder {
        category: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeEntry {
    pub category: String,
    pub representation: Representation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeProvenance {
    pub originating_boundary: u64,
    pub capability_token_id: u64,
    /// Handoff history — always empty until [11 — Agent
    /// Runtime](../11-agent-runtime.md) (Phase 4) exists to populate it.
    pub agent_chain: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeStaleness {
    pub per_entry_generation: HashMap<NodeId, u64>,
    pub freshness_horizon_secs: u64,
    pub captured_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Integrity {
    pub checksum: u64,
}

/// docs/07 §Data Structures' `ContextEnvelope` — the wire format a
/// [`ContextBundle`] becomes when it crosses a boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEnvelope {
    pub envelope_id: u64,
    pub schema_version: String,
    pub bundle_intent_id: String,
    pub bundle_session_id: String,
    pub entries: Vec<EnvelopeEntry>,
    pub provenance: EnvelopeProvenance,
    pub scope_applied: TrustLevel,
    pub staleness: EnvelopeStaleness,
    pub integrity: Integrity,
}

pub const SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessStatus {
    Fresh,
    StaleWithinHorizon,
    StaleBeyondHorizon,
}

#[derive(Debug, Clone, Default)]
pub struct FreshnessReport {
    pub per_entry: HashMap<NodeId, FreshnessStatus>,
}

#[derive(Debug, Clone)]
pub enum MergeOutcome {
    Merged(Vec<ContextEntry>),
    Conflicts(Vec<(ContextEntry, ContextEntry)>),
}

#[derive(Debug, thiserror::Error)]
pub enum PropagationError {
    #[error("envelope failed integrity verification")]
    IntegrityFailure,
    #[error("envelope {0} was already imported (replay)")]
    Replayed(u64),
    #[error("knowledge graph error: {0}")]
    Graph(#[from] GraphError),
}

/// docs/07 — Context Propagation, against a real
/// [`hyperion_knowledge_graph::KnowledgeGraph`] for staleness/generation
/// checks and metadata materialization. See this crate's doc comment for
/// what's deferred (real signing, real trust classification, transport).
pub struct ContextPropagation {
    graph: Arc<KnowledgeGraph>,
    replay_cache: Mutex<HashSet<u64>>,
    next_envelope_id: AtomicU64,
}

impl ContextPropagation {
    pub fn new(graph: Arc<KnowledgeGraph>) -> Self {
        ContextPropagation {
            graph,
            replay_cache: Mutex::new(HashSet::new()),
            next_envelope_id: AtomicU64::new(1),
        }
    }

    fn summarize(metadata: &serde_json::Value) -> serde_json::Value {
        match metadata {
            serde_json::Value::Object(map) => serde_json::Value::Object(
                map.iter()
                    .take(3)
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    fn sign(mut envelope: ContextEnvelope) -> ContextEnvelope {
        envelope.integrity.checksum = 0;
        let bytes = serde_json::to_vec(&envelope).expect("envelope always serializes");
        envelope.integrity.checksum = checksum(&bytes);
        envelope
    }

    fn verify(envelope: &ContextEnvelope) -> bool {
        let mut copy = envelope.clone();
        let claimed = copy.integrity.checksum;
        copy.integrity.checksum = 0;
        let bytes = serde_json::to_vec(&copy).expect("envelope always serializes");
        checksum(&bytes) == claimed
    }

    /// `ContextPropagation.export` — docs/07 §Algorithms 1/2: representation
    /// selection (`by_reference` only within the same Trust Boundary) and
    /// redaction (fail-closed default), then a signed, staleness-stamped
    /// envelope.
    pub fn export(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        bundle: &ContextBundle,
        target: TrustLevel,
        policy: &RedactionPolicy,
        freshness_horizon_secs: u64,
    ) -> Result<ContextEnvelope, PropagationError> {
        let same_boundary = target == TrustLevel::SameBoundary;
        let mut entries = Vec::with_capacity(bundle.entries.len());
        let mut per_entry_generation = HashMap::new();

        for entry in &bundle.entries {
            let rule = policy.rule_for(&entry.category);
            if rule == RedactionAction::Redact {
                entries.push(EnvelopeEntry {
                    category: entry.category.clone(),
                    representation: Representation::RedactedPlaceholder {
                        category: entry.category.clone(),
                        reason: format!("policy:{:?}", target),
                    },
                });
                continue;
            }

            per_entry_generation.insert(
                entry.node_id,
                self.graph.generation(monitor, token, entry.node_id)?,
            );

            let representation = if same_boundary {
                Representation::ByReference {
                    node_id: entry.node_id,
                }
            } else {
                let node = self.graph.get(monitor, token, entry.node_id)?;
                let content = if rule == RedactionAction::Summarize {
                    Self::summarize(&node.metadata)
                } else {
                    node.metadata
                };
                Representation::ByValue {
                    node_id: entry.node_id,
                    content,
                }
            };
            entries.push(EnvelopeEntry {
                category: entry.category.clone(),
                representation,
            });
        }

        let envelope = ContextEnvelope {
            envelope_id: self.next_envelope_id.fetch_add(1, Ordering::Relaxed),
            schema_version: SCHEMA_VERSION.to_string(),
            bundle_intent_id: bundle.scope.intent_id.clone(),
            bundle_session_id: bundle.scope.session_id.clone(),
            entries,
            provenance: EnvelopeProvenance {
                originating_boundary: token.origin().0,
                capability_token_id: token.token_id().0,
                agent_chain: Vec::new(),
            },
            scope_applied: target,
            staleness: EnvelopeStaleness {
                per_entry_generation,
                freshness_horizon_secs,
                captured_at: now(),
            },
            integrity: Integrity { checksum: 0 },
        };
        Ok(Self::sign(envelope))
    }

    fn freshness_report(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        envelope: &ContextEnvelope,
    ) -> Result<FreshnessReport, PropagationError> {
        let mut report = FreshnessReport::default();
        let elapsed = now().saturating_sub(envelope.staleness.captured_at);
        for (&node_id, &captured_generation) in &envelope.staleness.per_entry_generation {
            let status = match self.graph.generation(monitor, token, node_id) {
                Ok(current) if current == captured_generation => FreshnessStatus::Fresh,
                Ok(_) if elapsed <= envelope.staleness.freshness_horizon_secs => {
                    FreshnessStatus::StaleWithinHorizon
                }
                _ => FreshnessStatus::StaleBeyondHorizon,
            };
            report.per_entry.insert(node_id, status);
        }
        Ok(report)
    }

    /// `ContextPropagation.checkStaleness` — docs/07 §Interfaces, usable
    /// standalone (no import/replay-cache side effects).
    pub fn check_staleness(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        envelope: &ContextEnvelope,
    ) -> Result<FreshnessReport, PropagationError> {
        self.freshness_report(monitor, token, envelope)
    }

    /// `ContextPropagation.import` — docs/07 §Algorithms 4/5: verify
    /// signature (fail closed on the whole envelope), reject a replayed
    /// `envelope_id`, then downgrade any entry stale beyond the freshness
    /// horizon to a `redacted_placeholder`-equivalent rather than trusting
    /// it silently.
    pub fn import(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        envelope: ContextEnvelope,
    ) -> Result<(Vec<EnvelopeEntry>, FreshnessReport), PropagationError> {
        if !Self::verify(&envelope) {
            return Err(PropagationError::IntegrityFailure);
        }
        {
            let mut cache = self.replay_cache.lock().unwrap();
            if !cache.insert(envelope.envelope_id) {
                return Err(PropagationError::Replayed(envelope.envelope_id));
            }
        }

        let report = self.freshness_report(monitor, token, &envelope)?;
        let entries = envelope
            .entries
            .into_iter()
            .map(|entry| {
                let node_id = match &entry.representation {
                    Representation::ByReference { node_id }
                    | Representation::ByValue { node_id, .. } => Some(*node_id),
                    Representation::RedactedPlaceholder { .. } => None,
                };
                let stale_beyond = node_id
                    .and_then(|id| report.per_entry.get(&id))
                    .is_some_and(|s| *s == FreshnessStatus::StaleBeyondHorizon);
                if stale_beyond {
                    EnvelopeEntry {
                        category: entry.category.clone(),
                        representation: Representation::RedactedPlaceholder {
                            category: entry.category,
                            reason: "stale".to_string(),
                        },
                    }
                } else {
                    entry
                }
            })
            .collect();

        Ok((entries, report))
    }

    /// `ContextPropagation.revalidate` — docs/07 §Interfaces: the
    /// background refresh path for a `stale_within_horizon` entry.
    pub fn revalidate(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        node_id: NodeId,
    ) -> Result<EnvelopeEntry, PropagationError> {
        let node = self.graph.get(monitor, token, node_id)?;
        Ok(EnvelopeEntry {
            category: node.object_type,
            representation: Representation::ByValue {
                node_id,
                content: node.metadata,
            },
        })
    }
}

/// `ContextPropagation.merge` — docs/07 §Pseudocode `merge()`: matching
/// generations or a one-sided entry merge automatically; a genuine conflict
/// (different generations, but overlapping/divergent content — i.e. not
/// simply one being a newer superset) is surfaced, never auto-resolved.
pub fn merge(bundle_a: &[ContextEntry], bundle_b: &[ContextEntry]) -> MergeOutcome {
    let map_a: HashMap<NodeId, &ContextEntry> = bundle_a.iter().map(|e| (e.node_id, e)).collect();
    let map_b: HashMap<NodeId, &ContextEntry> = bundle_b.iter().map(|e| (e.node_id, e)).collect();
    let all_ids: HashSet<NodeId> = map_a.keys().chain(map_b.keys()).copied().collect();

    let mut merged = Vec::new();
    let mut conflicts = Vec::new();

    for id in all_ids {
        match (map_a.get(&id), map_b.get(&id)) {
            (Some(ea), None) => merged.push((*ea).clone()),
            (None, Some(eb)) => merged.push((*eb).clone()),
            (Some(ea), Some(eb)) if ea.generation == eb.generation => merged.push((*ea).clone()),
            (Some(ea), Some(eb)) => {
                let divergent = ea.category != eb.category || ea.content != eb.content;
                if divergent {
                    conflicts.push(((*ea).clone(), (*eb).clone()));
                } else {
                    let winner = if ea.generation >= eb.generation {
                        ea
                    } else {
                        eb
                    };
                    merged.push((*winner).clone());
                }
            }
            (None, None) => unreachable!("id drawn from the union of both maps' keys"),
        }
    }

    if conflicts.is_empty() {
        MergeOutcome::Merged(merged)
    } else {
        MergeOutcome::Conflicts(conflicts)
    }
}
