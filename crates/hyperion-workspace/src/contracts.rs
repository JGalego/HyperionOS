use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionAffinity {
    Left,
    Center,
    Right,
    TopBar,
    BottomBar,
}

/// docs/13 §5.3's Adaptive Complexity density tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComplexityTier {
    Beginner,
    Pro,
    Dev,
}

/// A tier-specific override of a contract's base fields — docs/13 §5.3's
/// worked example: "`spreadsheet.formula_bar.basic` vs.
/// `spreadsheet.formula_bar.advanced`." Only the fields that actually vary
/// by tier are optional; everything else falls back to the base contract.
#[derive(Debug, Clone, Default)]
pub struct PanelVariant {
    pub panel_template: Option<String>,
    pub min_size: Option<(u32, u32)>,
}

/// docs/13 §6's Capability UI Contract, extended with docs/14 §6's
/// mandatory accessibility fields — the two documents describe one
/// contract, not two, which is the same reason this crate is one crate.
#[derive(Debug, Clone)]
pub struct CapabilityUiContract {
    pub capability_ref: String,
    pub panel_template: String,
    pub region_affinity: RegionAffinity,
    pub min_size: (u32, u32),
    pub priority: f32,
    /// docs/13 §5.1: which Context Bundle category this panel binds to —
    /// a narrowed stand-in for `context_bundle.relevant_objects_for
    /// (panel.capability_ref)`; see `compiler.rs`.
    pub binds_category: Option<String>,
    pub variants: HashMap<ComplexityTier, PanelVariant>,

    // docs/14 §6's mandatory accessibility fields. `None` is a legal value
    // for the optional ones — the compiler derives a generic-but-valid
    // fallback rather than ever emitting a nameless node (§10).
    pub accessible_role: Option<String>,
    pub label_template: Option<String>,
    pub keyboard_operations: Vec<String>,
    pub alt_text_hook: Option<String>,
    /// docs/14 §5.2's contrast-ratio rule needs a real rendered color pair
    /// to compute from; without real rendering, the contract declares its
    /// own contrast ratio directly, and the linter checks the declared
    /// value against the same 4.5:1 threshold the doc specifies.
    pub contrast_ratio: f32,
    pub has_motion: bool,
    pub reduced_motion_alternative: bool,
    pub language_tag: String,
    /// docs/14 §5.7: an audio-emitting Capability must also declare a
    /// visual alert equivalent — enforced by the linter, not merely
    /// documented.
    pub emits_audio: bool,
    pub has_visual_alert_equivalent: bool,
}

impl CapabilityUiContract {
    pub(crate) fn resolve_variant(&self, tier: ComplexityTier) -> (String, (u32, u32)) {
        let variant = self.variants.get(&tier);
        let panel_template = variant
            .and_then(|v| v.panel_template.clone())
            .unwrap_or_else(|| self.panel_template.clone());
        let min_size = variant.and_then(|v| v.min_size).unwrap_or(self.min_size);
        (panel_template, min_size)
    }
}
