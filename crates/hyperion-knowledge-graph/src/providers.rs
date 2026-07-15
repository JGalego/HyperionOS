//! docs/998-roadmap.md's Resourceful pillar: the real (topic -> capability_id) lookup this
//! crate previously had no equivalent of. A plugin's
//! `hyperion_plugin_framework::Contribution::KnowledgeProvider` declares which topic it can
//! supply facts about and which already-installed Capability answers a query for it — this
//! module is the real lookup a caller (e.g. a semantic search or context-assembly path with no
//! local knowledge of a topic) consults to decide which capability to invoke.
//!
//! **Never bypasses the Capability Registry's own dispatch/consent path**: this only tells a
//! caller *which* `capability_id` to invoke for a topic — actually invoking it still goes
//! through the exact same dispatch (and whatever consent/permission gate that capability
//! itself declares) every other Capability invocation already does.

use hyperion_plugin_framework::PluginRegistry;

/// Every currently-installed, non-quarantined `capability_id` that declared it can answer
/// queries about `topic`, in no particular priority order beyond installation order. Empty if
/// no plugin has ever contributed a `KnowledgeProvider` for this topic.
pub fn capabilities_for_topic(plugins: &PluginRegistry, topic: &str) -> Vec<String> {
    plugins
        .knowledge_provider_contributions()
        .into_iter()
        .filter(|kp| kp.topic == topic)
        .map(|kp| kp.capability_id)
        .collect()
}

/// The first currently-installed, non-quarantined `capability_id` that declared it can answer
/// queries about `topic` — a convenience over [`capabilities_for_topic`] for a caller that just
/// wants any one match. `None` if no installed plugin knows this topic.
pub fn capability_for_topic(plugins: &PluginRegistry, topic: &str) -> Option<String> {
    capabilities_for_topic(plugins, topic).into_iter().next()
}
