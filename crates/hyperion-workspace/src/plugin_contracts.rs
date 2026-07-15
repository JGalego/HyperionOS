//! docs/998-roadmap.md's Resourceful pillar: the real UI-component registry this crate
//! previously had no equivalent of. Every existing caller of [`crate::CapabilityUiContract`]
//! hand-authors one for each `capability_ref` (see `contract_for` in `hyperion-console`/
//! `hyperion-shell`, or the `*::workspace_bridge` modules) — this module lets a plugin supply
//! one instead, via a real `hyperion_plugin_framework::Contribution::UiComponent`.

use std::collections::HashMap;

use hyperion_plugin_framework::{PluginRegistry, UiRegionAffinity};

use crate::contracts::{CapabilityUiContract, RegionAffinity};

fn region_affinity_from(region_affinity: UiRegionAffinity) -> RegionAffinity {
    match region_affinity {
        UiRegionAffinity::Left => RegionAffinity::Left,
        UiRegionAffinity::Center => RegionAffinity::Center,
        UiRegionAffinity::Right => RegionAffinity::Right,
        UiRegionAffinity::TopBar => RegionAffinity::TopBar,
        UiRegionAffinity::BottomBar => RegionAffinity::BottomBar,
    }
}

/// The real lookup a caller consults before falling back to hand-authoring its own
/// `CapabilityUiContract`: every currently-installed, non-quarantined plugin's own
/// `Contribution::UiComponent` entries are searched for an exact `capability_ref` match, and
/// the first hit is converted into this crate's own real [`CapabilityUiContract`] shape (with
/// an empty per-`ComplexityTier` `variants` map — see that contribution's own doc comment on
/// why per-tier variants aren't part of a plugin contribution yet). Returns `None` if no
/// installed plugin contributed a template for this capability.
pub fn known_contract_for(
    plugins: &PluginRegistry,
    capability_ref: &str,
) -> Option<CapabilityUiContract> {
    plugins
        .ui_component_contributions()
        .into_iter()
        .find(|ui| ui.capability_ref == capability_ref)
        .map(|ui| CapabilityUiContract {
            capability_ref: ui.capability_ref,
            panel_template: ui.panel_template,
            region_affinity: region_affinity_from(ui.region_affinity),
            min_size: ui.min_size,
            priority: ui.priority,
            binds_category: ui.binds_category,
            variants: HashMap::new(),
            accessible_role: ui.accessible_role,
            label_template: ui.label_template,
            keyboard_operations: ui.keyboard_operations,
            alt_text_hook: ui.alt_text_hook,
            contrast_ratio: ui.contrast_ratio,
            has_motion: ui.has_motion,
            reduced_motion_alternative: ui.reduced_motion_alternative,
            language_tag: ui.language_tag,
            emits_audio: ui.emits_audio,
            has_visual_alert_equivalent: ui.has_visual_alert_equivalent,
        })
}
