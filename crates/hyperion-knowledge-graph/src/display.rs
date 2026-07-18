//! This crate's own previously-named "no human-readable node rendering" gap: a real
//! `describe(node: &NodeRecord) -> String` heuristic existed, but only as a private helper inside
//! `hyperion-console::graph_explorer` -- every other consumer of a real `NodeRecord`
//! (`hyperion-explainability`'s own render, `hyperion-shell`'s visual rendering, any future
//! dump/export tool) had nowhere to get the same human-readable rendering without either
//! reinventing it or depending on `hyperion-console` (a real, backwards dependency direction: a
//! leaf UI crate, not a foundational one). [`NodeRecord::display_label`] is that same real logic,
//! promoted into this crate as the canonical "how do you describe this Semantic Object to a
//! person" primitive, per CLAUDE.md's "never expose technical errors [or internals] directly" --
//! deliberately no "node"/"edge"/"predicate" jargon anywhere in its output.
//!
//! This still leans on the same "metadata is JSON, interpreted loosely by `object_type`
//! convention" philosophy this crate's own doc comment already establishes -- `"intent"`
//! (every utterance's own record, read loosely rather than depending on `hyperion-intent`'s own
//! struct) and `"task_result"` (a real capability dispatch's own real output, read loosely rather
//! than depending on `hyperion-agent-runtime`'s own types) are both recognized by
//! well-known metadata key conventions, never by importing either crate's Rust types -- this
//! crate still depends on neither.

use serde_json::Value;

use crate::types::NodeRecord;

impl NodeRecord {
    /// One human-readable line describing this node -- deliberately no "node"/"edge"/"predicate"
    /// jargon, matching CLAUDE.md's "never expose technical errors [or internals] directly."
    /// `"intent"` (every utterance's own record) is rendered specially since its shape is known
    /// exactly; a `"task_result"` node (a real capability dispatch's own real output) uses
    /// [`render_capability_result`]; everything else falls back to a handful of common
    /// human-facing keys, then a truncated raw value as a last, honest resort. Never truncated
    /// itself -- a caller wanting a shorter, list-context teaser truncates this string's own
    /// output (see e.g. `hyperion_console::graph_explorer::preview`).
    pub fn display_label(&self) -> String {
        if self.object_type == "task_result" {
            if let Some(text) = render_capability_result(&self.metadata) {
                return format!("a result: {text}");
            }
        }

        if let Some(text) = self.utterance_text() {
            return match self.metadata.get("confidence").and_then(|v| v.as_f64()) {
                Some(confidence) => {
                    format!(
                        "you asked: \"{text}\" ({:.0}% confident)",
                        confidence * 100.0
                    )
                }
                None => format!("you asked: \"{text}\""),
            };
        }

        if self.object_type == "intent" {
            if let Some(predicate) = self
                .metadata
                .get("predicate")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                return format!("a planned task: {predicate}");
            }
        }

        if self.object_type.starts_with("memory_") {
            if let Some(content) = self.metadata.get("content") {
                return format!("a memory: {}", summarize_value(content));
            }
        }

        for key in ["title", "name", "text", "summary", "label", "path"] {
            if let Some(s) = self.metadata.get(key).and_then(|v| v.as_str()) {
                return format!("a {}: {s}", self.object_type);
            }
        }

        format!(
            "a {} ({})",
            self.object_type,
            summarize_value(&self.metadata)
        )
    }

    /// The real utterance text behind a root Intent node, if this is one -- `None` for a
    /// decomposed goal's own child tasks (`hyperion_intent::engine::decompose` leaves their
    /// `raw_utterance` empty, since nobody actually said "market_research") as well as for every
    /// non-`"intent"` object type. Shared by [`NodeRecord::display_label`] and
    /// `hyperion_console::graph_explorer::friendly_type` so both agree on which nodes were really
    /// said by a person -- a real bug found via a live manual check had them disagree before both
    /// were unified onto this one definition.
    pub fn utterance_text(&self) -> Option<&str> {
        if self.object_type != "intent" {
            return None;
        }
        self.metadata
            .get("raw_utterance")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
    }
}

fn summarize_value(value: &Value) -> String {
    let text = match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if text.chars().count() > 80 {
        let truncated: String = text.chars().take(77).collect();
        format!("{truncated}...")
    } else {
        text
    }
}

/// Turns a real capability dispatch's own returned JSON into one human sentence --
/// `document.draft`'s `"draft"`, `web.search`'s `"results"`/honesty-caveat `"note"` (see
/// `hyperion_agent_runtime::AgentRuntime::dispatch_document_draft`/`dispatch_market_research`).
/// Shared by [`NodeRecord::display_label`] (a `"task_result"` node found via a real graph query)
/// and any caller rendering a real, in-flight capability outcome that hasn't been written to the
/// graph yet (e.g. `hyperion_console::session`'s own task-outcome rendering) -- one source of
/// truth for "how do we turn a capability's raw JSON into words," not two definitions that could
/// quietly drift apart. Public (not just used internally) for exactly that second, no-`NodeRecord`
/// case.
pub fn render_capability_result(value: &Value) -> Option<String> {
    if let Some(draft) = value.get("draft").and_then(|v| v.as_str()) {
        return Some(draft.to_string());
    }
    if let Some(results) = value.get("results").and_then(|v| v.as_array()) {
        let joined = results
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        return Some(match value.get("note").and_then(|v| v.as_str()) {
            Some(note) => format!("{joined} ({note})"),
            None => joined,
        });
    }
    value
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(object_type: &str, metadata: Value) -> NodeRecord {
        NodeRecord {
            object_type: object_type.to_string(),
            embedding: None,
            metadata,
            owner: 1,
            device_origin: 0,
            origin: crate::types::NodeOrigin::default(),
            corroboration_count: 0,
            created_at: 0,
            updated_at: 0,
            tombstone: false,
        }
    }

    #[test]
    fn a_task_result_node_renders_via_render_capability_result() {
        let n = node(
            "task_result",
            serde_json::json!({"draft": "a quarterly report"}),
        );
        assert_eq!(n.display_label(), "a result: a quarterly report");
    }

    #[test]
    fn an_utterance_node_renders_with_its_confidence() {
        let n = node(
            "intent",
            serde_json::json!({"raw_utterance": "launch my startup", "confidence": 0.8}),
        );
        assert_eq!(
            n.display_label(),
            "you asked: \"launch my startup\" (80% confident)"
        );
    }

    #[test]
    fn an_intent_node_with_no_utterance_renders_its_predicate() {
        let n = node(
            "intent",
            serde_json::json!({"predicate": "market_research"}),
        );
        assert_eq!(n.display_label(), "a planned task: market_research");
    }

    #[test]
    fn a_memory_node_renders_its_content() {
        let n = node(
            "memory_episodic",
            serde_json::json!({"content": "prefers oat milk"}),
        );
        assert_eq!(n.display_label(), "a memory: prefers oat milk");
    }

    #[test]
    fn a_node_with_a_well_known_key_renders_via_that_key() {
        let n = node("document", serde_json::json!({"title": "Q3 Plan"}));
        assert_eq!(n.display_label(), "a document: Q3 Plan");
    }

    #[test]
    fn an_unrecognized_node_falls_back_to_a_truncated_raw_value() {
        let n = node("widget", serde_json::json!({"arbitrary": "data"}));
        assert_eq!(n.display_label(), "a widget ({\"arbitrary\":\"data\"})");
    }

    #[test]
    fn render_capability_result_reads_web_search_shaped_results_with_its_honesty_note() {
        let value = serde_json::json!({
            "results": ["some AI-generated notes"],
            "note": "AI-generated research notes, not a live web search",
        });
        assert_eq!(
            render_capability_result(&value).unwrap(),
            "some AI-generated notes (AI-generated research notes, not a live web search)"
        );
    }
}
