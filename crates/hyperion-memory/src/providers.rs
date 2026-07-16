//! docs/998-roadmap.md's Resourceful pillar: the real `(tier, entity_key) -> capability_id`
//! lookup this crate previously had no equivalent of. A plugin's
//! `hyperion_plugin_framework::Contribution::MemoryProvider` declares which tier + entity key it
//! can supply facts about and which already-installed Capability answers a query for it — this
//! module is the real lookup a caller (e.g. a recall path that found no local
//! [`crate::types::MemoryRecord`] for an entity) consults to decide which capability to invoke.
//!
//! **Never bypasses the Capability Registry's own dispatch/consent path**: this only tells a
//! caller *which* `capability_id` to invoke for a `(tier, entity_key)` pair — actually invoking
//! it still goes through the exact same dispatch (and whatever consent/permission gate that
//! capability itself declares) every other Capability invocation already does. This also never
//! writes the result back as a [`crate::types::MemoryRecord`] itself — that remains the caller's
//! own, separately-justified decision.

use hyperion_plugin_framework::{MemoryTierKind, PluginRegistry};

use crate::types::MemoryTier;

fn as_tier_kind(tier: MemoryTier) -> MemoryTierKind {
    match tier {
        MemoryTier::Episodic => MemoryTierKind::Episodic,
        MemoryTier::Semantic => MemoryTierKind::Semantic,
        MemoryTier::Procedural => MemoryTierKind::Procedural,
        MemoryTier::LongTerm => MemoryTierKind::LongTerm,
    }
}

/// Every currently-installed, non-quarantined `capability_id` that declared it can supply facts
/// about `entity_key` in `tier`, in no particular priority order beyond installation order.
/// Empty if no plugin has ever contributed a `MemoryProvider` for this `(tier, entity_key)` pair.
pub fn capabilities_for(
    plugins: &PluginRegistry,
    tier: MemoryTier,
    entity_key: &str,
) -> Vec<String> {
    let kind = as_tier_kind(tier);
    plugins
        .memory_provider_contributions()
        .into_iter()
        .filter(|mp| mp.tier == kind && mp.entity_key == entity_key)
        .map(|mp| mp.capability_id)
        .collect()
}

/// The first currently-installed, non-quarantined `capability_id` that declared it can supply
/// facts about `entity_key` in `tier` — a convenience over [`capabilities_for`] for a caller that
/// just wants any one match. `None` if no installed plugin knows this `(tier, entity_key)` pair.
pub fn capability_for(
    plugins: &PluginRegistry,
    tier: MemoryTier,
    entity_key: &str,
) -> Option<String> {
    capabilities_for(plugins, tier, entity_key)
        .into_iter()
        .next()
}
