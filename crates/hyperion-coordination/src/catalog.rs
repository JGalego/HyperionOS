use hyperion_agent_runtime::{AgentManifest, TrustTier};

/// A stand-in for deriving `required_capabilities` from a sub-intent's real
/// semantic contract (docs/12 §5.1) — this crate has no Capability
/// Registry ([24 — Plugin Framework](../24-plugin-framework.md), Phase 9)
/// to consult, so it maps `hyperion-intent`'s HTN predicate strings
/// directly onto `web.search`/`document.draft`, the two Capabilities
/// `hyperion-agent-runtime` dispatches through a real `LocalAiRuntime::infer` call (real
/// generated content now, not a hand-written canned stub — see that crate's own doc comment on
/// the "launch my startup produces zero real content" gap this fixed).
pub fn required_capabilities_for(predicate: &str) -> Vec<String> {
    match predicate {
        "market_research" => vec!["web.search".to_string()],
        "business_model" | "branding" | "legal_formation" => vec!["document.draft".to_string()],
        other => vec![format!("unknown.{other}")],
    }
}

/// The small, first-party specialization roster this phase needs — docs/11
/// §4's built-in table, narrowed to the specializations whose baseline
/// Capabilities actually match [`required_capabilities_for`]'s output, plus
/// `"assistant"` (docs/998-roadmap.md M8): the one specialization
/// whose baseline Capability (`assistant.respond`,
/// `hyperion-agent-runtime`'s own real-inference dispatch) is never a
/// target of [`required_capabilities_for`] — no HTN leaf predicate maps to
/// it — because it exists for `hyperion-console`'s *undecomposed*-goal
/// fallback (no template, no leaves, nothing for that function to be
/// asked about), not for a template's own leaves. `"research"`'s own baseline
/// capabilities gained `web.research` the same way (docs/998-roadmap.md M10) — also never
/// a target of [`required_capabilities_for`], for the same reason: it's `hyperion-console`'s
/// *other* undecomposed-goal fallback (a URL-shaped utterance), not a template leaf. Purely
/// additive: `market_research`'s own existing real 4-task-decomposition demo (M7) still only
/// ever needs this same specialization's pre-existing `web.search`.
pub fn default_manifests() -> Vec<AgentManifest> {
    vec![
        AgentManifest {
            specialization: "research".to_string(),
            baseline_capabilities: vec!["web.search".to_string(), "web.research".to_string()],
            requestable_capabilities: Vec::new(),
            trust_tier: TrustTier::System,
        },
        AgentManifest {
            specialization: "writer".to_string(),
            baseline_capabilities: vec!["document.draft".to_string()],
            requestable_capabilities: Vec::new(),
            trust_tier: TrustTier::System,
        },
        AgentManifest {
            specialization: "assistant".to_string(),
            baseline_capabilities: vec!["assistant.respond".to_string()],
            // docs/998-roadmap.md "Phase 2: cloud providers": requestable, never baseline
            // -- these four route to the exact same dispatch `assistant.respond` does
            // (`hyperion_agent_runtime::runtime::AgentRuntime::dispatch_assistant_respond`), but
            // only when the console's currently-active backend is the matching real cloud
            // provider (see `hyperion_console::session::BackendKind::capability_ref`). Being
            // requestable is what puts a real `GrantDecision::PendingConsent` between a cloud
            // dispatch and ever actually running -- local/mock/self-hosted-engine use is
            // untouched, since `assistant.respond` itself stays baseline.
            requestable_capabilities: vec![
                "cloud.openai".to_string(),
                "cloud.anthropic".to_string(),
                "cloud.gemini".to_string(),
                "cloud.groq".to_string(),
            ],
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
