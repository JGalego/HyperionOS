use hyperion_agent_runtime::{AgentManifest, TrustTier};

/// A stand-in for deriving `required_capabilities` from a sub-intent's real
/// semantic contract (docs/12 §5.1) — this crate has no Capability
/// Registry ([24 — Plugin Framework](../24-plugin-framework.md), Phase 9)
/// to consult, so it maps `hyperion-intent`'s HTN predicate strings
/// directly onto the two stub Capabilities `hyperion-agent-runtime`
/// actually implements.
pub fn required_capabilities_for(predicate: &str) -> Vec<String> {
    match predicate {
        "market_research" => vec!["web.search".to_string()],
        "business_model" | "branding" | "legal_formation" => vec!["document.draft".to_string()],
        other => vec![format!("unknown.{other}")],
    }
}

/// The small, first-party specialization roster this phase needs — docs/11
/// §4's built-in table, narrowed to the two specializations whose baseline
/// Capabilities actually match [`required_capabilities_for`]'s output.
pub fn default_manifests() -> Vec<AgentManifest> {
    vec![
        AgentManifest {
            specialization: "research".to_string(),
            baseline_capabilities: vec!["web.search".to_string()],
            requestable_capabilities: Vec::new(),
            trust_tier: TrustTier::System,
        },
        AgentManifest {
            specialization: "writer".to_string(),
            baseline_capabilities: vec!["document.draft".to_string()],
            requestable_capabilities: Vec::new(),
            trust_tier: TrustTier::System,
        },
    ]
}

/// The best-fit built-in specialization for a required-capability set —
/// docs/12 §5.1's `registry.best_fit_specialization`, narrowed to a linear
/// scan of [`default_manifests`] rather than a real registry query.
pub fn best_fit_manifest(required_capabilities: &[String]) -> Option<AgentManifest> {
    default_manifests().into_iter().find(|m| {
        required_capabilities
            .iter()
            .all(|c| m.baseline_capabilities.contains(c) || m.requestable_capabilities.contains(c))
    })
}
