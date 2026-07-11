//! docs/17 T3: cross-Agent privilege escalation — a receiving Agent must
//! never launder authority by trusting a sender's claimed risk level.

use std::sync::Arc;

use hyperion_agent_runtime::{AgentManifest, AgentRuntime, TrustTier};
use hyperion_ai_runtime::{LocalAiRuntime, MockBackend};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_security::{
    assess, cross_agent_delegation_verify, InterventionLevel, PendingAction, SensitivityHint,
};

#[test]
fn t3_a_delegating_agent_cannot_launder_a_high_risk_action_through_a_low_risk_claim() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ai_runtime = Arc::new(LocalAiRuntime::new(Box::new(MockBackend), 8_000));
    let runtime = AgentRuntime::new(ai_runtime);
    let manifest = AgentManifest {
        specialization: "delegator".to_string(),
        baseline_capabilities: vec!["coordination.delegate".to_string()],
        requestable_capabilities: vec![],
        trust_tier: TrustTier::System,
    };
    let agent_a = runtime.spawn(&monitor, &root, manifest, None).unwrap();
    assert!(
        runtime.describe(agent_a).is_some(),
        "the delegating Agent must be a real, spawned instance, not a hypothetical"
    );

    let sender_claim = assess(&PendingAction {
        action_id: 1,
        object_refs: vec![],
        scope_size: 1,
        reversible: true,
        sensitivity: SensitivityHint::Public,
        intent_confidence: 1.0,
        corroboration: 1.0,
        provenance: None,
    });
    assert_eq!(
        sender_claim.intervention_level,
        InterventionLevel::SilentProceed
    );

    let receiver_action = PendingAction {
        action_id: 2,
        object_refs: vec![],
        scope_size: 200,
        reversible: false,
        sensitivity: SensitivityHint::Restricted,
        intent_confidence: 1.0,
        corroboration: 1.0,
        provenance: None,
    };
    let receiver_assessment = cross_agent_delegation_verify(&sender_claim, &receiver_action);

    assert_eq!(receiver_assessment.intervention_level, InterventionLevel::RequireBackupFirst, "the receiving Agent's own independent assessment must win, regardless of what the delegating Agent claimed");
}
