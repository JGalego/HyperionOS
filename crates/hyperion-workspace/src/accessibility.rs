use crate::contracts::CapabilityUiContract;
use crate::types::{AccessibilityNode, AccessibilityTree};

pub const CONTRAST_MIN: f32 = 4.5;
pub const TARGET_SIZE_MIN_DP: u32 = 44;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Violation {
    pub rule_id: &'static str,
    pub node_id: u64,
    pub severity: Severity,
}

#[derive(Debug, Clone)]
pub struct AccessibilityLintResult {
    pub passed: bool,
    pub violations: Vec<Violation>,
}

/// docs/14 §5.1/§10: derives a valid `AccessibilityNode` from a
/// Capability's declared contract, falling back to a deterministic,
/// generic-but-valid name/role when the contract omits them — "nothing is
/// ever silently inaccessible, only less precisely labeled."
pub(crate) fn derive_node(
    node_id: u64,
    panel_ref: u64,
    contract: &CapabilityUiContract,
    binding_present: bool,
) -> AccessibilityNode {
    let role = contract
        .accessible_role
        .clone()
        .unwrap_or_else(|| "generic".to_string());
    let accessible_name = contract
        .label_template
        .clone()
        .unwrap_or_else(|| humanize_capability_ref(&contract.capability_ref));

    AccessibilityNode {
        node_id,
        panel_ref,
        role,
        accessible_name,
        description: contract.capability_ref.clone(),
        language_tag: if contract.language_tag.is_empty() {
            "en".to_string()
        } else {
            contract.language_tag.clone()
        },
        target_size: contract.min_size,
        is_interactive: binding_present || !contract.keyboard_operations.is_empty(),
        has_motion: contract.has_motion,
        reduced_motion_alternative: contract.reduced_motion_alternative,
        contrast_ratio: contract.contrast_ratio,
        actions: contract.keyboard_operations.clone(),
        emits_audio: contract.emits_audio,
        has_visual_alert_equivalent: contract.has_visual_alert_equivalent,
    }
}

fn humanize_capability_ref(capability_ref: &str) -> String {
    capability_ref.replace(['.', '_'], " ")
}

/// docs/14 §7's `lint_template` pseudocode, evaluated over a structured
/// tree rather than a rendered surface — see this crate's doc comment on
/// why `contrast_ratio` is contract-declared instead of measured.
pub fn lint_template(
    tree: &AccessibilityTree,
    target_size_multiplier: f32,
) -> AccessibilityLintResult {
    let mut violations = Vec::new();

    for node in &tree.nodes {
        if node.accessible_name.trim().is_empty() {
            violations.push(Violation {
                rule_id: "accessible-name-present",
                node_id: node.node_id,
                severity: Severity::Error,
            });
        }
        if node.contrast_ratio < CONTRAST_MIN {
            violations.push(Violation {
                rule_id: "contrast-ratio",
                node_id: node.node_id,
                severity: Severity::Error,
            });
        }
        let min_size = TARGET_SIZE_MIN_DP as f32 * target_size_multiplier;
        if node.is_interactive
            && ((node.target_size.0 as f32) < min_size || (node.target_size.1 as f32) < min_size)
        {
            violations.push(Violation {
                rule_id: "target-size",
                node_id: node.node_id,
                severity: Severity::Error,
            });
        }
        if node.has_motion && !node.reduced_motion_alternative {
            violations.push(Violation {
                rule_id: "motion-alternative",
                node_id: node.node_id,
                severity: Severity::Warning,
            });
        }
        if node.language_tag.trim().is_empty() {
            violations.push(Violation {
                rule_id: "language-tag-present",
                node_id: node.node_id,
                severity: Severity::Error,
            });
        }
        // docs/14 §5.7: an audio-only alert (no visual/haptic equivalent)
        // is rejected the same way a nameless node is.
        if node.emits_audio && !node.has_visual_alert_equivalent {
            violations.push(Violation {
                rule_id: "audio-alert-has-visual-equivalent",
                node_id: node.node_id,
                severity: Severity::Error,
            });
        }
    }

    let reachable: std::collections::HashSet<u64> = tree.focus_order.iter().copied().collect();
    for node in tree.nodes.iter().filter(|n| n.is_interactive) {
        if !reachable.contains(&node.node_id) {
            violations.push(Violation {
                rule_id: "focus-order-valid",
                node_id: node.node_id,
                severity: Severity::Error,
            });
        }
    }

    let passed = !violations.iter().any(|v| v.severity == Severity::Error);
    AccessibilityLintResult { passed, violations }
}
