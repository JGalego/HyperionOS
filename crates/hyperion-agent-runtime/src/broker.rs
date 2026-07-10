use crate::types::{AgentInstance, CapabilityGrant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GrantDecision {
    Granted,
    PendingConsent,
    Denied,
}

/// docs/11 §6.1's Capability Broker: resolve a grant in order — baseline
/// and already-scoped is immediate; requestable needs a consent round
/// trip (already-granted-this-session short-circuits it); anything not
/// declared in the manifest at all is denied unconditionally, no prompt,
/// no exception — "the enforcement point for [02's] 'no silent
/// authority' invariant."
pub(crate) fn resolve_grant(instance: &AgentInstance, capability_ref: &str) -> GrantDecision {
    if instance
        .manifest
        .baseline_capabilities
        .iter()
        .any(|c| c == capability_ref)
    {
        return GrantDecision::Granted;
    }
    if instance
        .manifest
        .requestable_capabilities
        .iter()
        .any(|c| c == capability_ref)
    {
        let already_granted = instance
            .grants
            .iter()
            .any(|g: &CapabilityGrant| g.capability_ref == capability_ref);
        return if already_granted {
            GrantDecision::Granted
        } else {
            GrantDecision::PendingConsent
        };
    }
    GrantDecision::Denied
}
