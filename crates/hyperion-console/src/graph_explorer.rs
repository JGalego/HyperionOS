//! Turns `hyperion-knowledge-graph`'s raw node/edge primitives into the console's
//! `/recall`/`/why`/`/related` meta-commands -- three ways to explore whatever Hyperion has
//! actually recorded (every utterance's own "intent" node at minimum; memory and file nodes once
//! those engines are wired into a live session) without ever showing a user a raw `NodeId`.
//!
//! Every result is shown as a small, session-local reference number (`[1]`, `[2]`...) instead --
//! `/why <n>`/`/related <n>` resolve that number back to the real id themselves, and `/related`
//! re-numbers its own output so results chain (`/recall`, then `/related 1`, then `/related 2` on
//! what that just showed). This is a deliberate, `/`-only surface with no plain-English alias
//! (unlike `/backend`/`use backend`): phrases like "what do you know about X" are exactly what a
//! future real memory-recall Intent should own, and hard-coding them here as a meta-command would
//! squat on that surface.
//!
//! Deliberately honest about what this console can't do yet: there is no real embedding pipeline
//! wired in here, so `/recall`'s search is a plain, case-insensitive text match over recorded
//! metadata, not semantic search -- see [`GraphExplorer::recall`].

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_knowledge_graph::{
    ExplainRef, GraphQuery, KnowledgeGraph, NodeId, NodeRecord, ProvenanceChain,
};

/// The most results a single `/recall`/`/related` list shows -- generous enough to browse, small
/// enough that a numbered list is still something a person can actually scan.
const MAX_RESULTS: usize = 20;

pub struct GraphExplorer {
    graph: Arc<KnowledgeGraph>,
    /// The node ids behind the most recently rendered numbered list -- `refs[0]` is "[1]", etc.
    /// `/why`/`/related` resolve their own `<n>` argument against this rather than a raw `NodeId`
    /// ever crossing the console's own text boundary.
    refs: Vec<NodeId>,
}

impl GraphExplorer {
    pub fn new(graph: Arc<KnowledgeGraph>) -> Self {
        GraphExplorer {
            graph,
            refs: Vec::new(),
        }
    }

    /// `/recall [text]` -- every recorded node (bare), or those whose recorded metadata mentions
    /// `text` (a plain substring match), newest first.
    pub fn recall(
        &mut self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        text: &str,
    ) -> Vec<String> {
        let hits = match self.graph.query(monitor, token, &GraphQuery::default()) {
            Ok(hits) => hits,
            Err(e) => return vec![format!("I couldn't look that up: {e}")],
        };

        let needle = text.trim().to_ascii_lowercase();
        let mut matches: Vec<(NodeId, NodeRecord)> = hits
            .into_iter()
            .map(|hit| (hit.node_id, hit.node))
            .filter(|(_, node)| {
                needle.is_empty() || describe(node).to_ascii_lowercase().contains(&needle)
            })
            .collect();
        matches.sort_by_key(|(_, node)| std::cmp::Reverse(node.updated_at));
        matches.truncate(MAX_RESULTS);

        if matches.is_empty() {
            self.refs.clear();
            return vec![if text.trim().is_empty() {
                "I don't have anything recorded yet.".to_string()
            } else {
                format!("I don't have anything recorded about \"{}\".", text.trim())
            }];
        }

        self.render_list(matches)
    }

    /// `/related <n>` -- what's directly connected to result `n` (one hop, any relationship),
    /// re-numbering the list so results chain.
    pub fn related(
        &mut self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        n: usize,
    ) -> Vec<String> {
        let Some(&start) = self.resolve(n) else {
            return vec![Self::unknown_ref(n)];
        };

        let subgraph = match self.graph.traverse(monitor, token, start, None, 1) {
            Ok(subgraph) => subgraph,
            Err(e) => return vec![format!("I couldn't look that up: {e}")],
        };
        let mut neighbors: Vec<(NodeId, NodeRecord)> = subgraph
            .nodes
            .into_iter()
            .filter(|(id, _, hop)| *id != start && *hop > 0)
            .map(|(id, node, _)| (id, node))
            .collect();
        neighbors.sort_by_key(|(_, node)| std::cmp::Reverse(node.updated_at));
        neighbors.truncate(MAX_RESULTS);

        if neighbors.is_empty() {
            self.refs.clear();
            return vec!["Nothing else is connected to that.".to_string()];
        }
        self.render_list(neighbors)
    }

    /// `/why <n>` -- result `n`'s own provenance: when it was recorded and how much it's
    /// connected to. This is CLAUDE.md's own Explainability questions ("Why? How? What
    /// evidence?") applied to one recorded thing.
    pub fn why(
        &mut self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        n: usize,
    ) -> Vec<String> {
        let Some(&id) = self.resolve(n) else {
            return vec![Self::unknown_ref(n)];
        };

        // `ProvenanceChain::Node` carries `object_type` alone, which can't by itself distinguish
        // a real root Intent (something a person actually asked) from one of its own child tasks
        // (same "intent" object type, no utterance of its own) -- a real bug this exact call
        // caught: an earlier version of `friendly_type` looked at `object_type` only and called
        // every task "something you asked." A second, real `get()` call reads the same metadata
        // `describe()` already inspects, so both agree.
        let node = match self.graph.get(monitor, token, id) {
            Ok(node) => node,
            Err(e) => return vec![format!("I couldn't look that up: {e}")],
        };

        match self.graph.explain(monitor, token, ExplainRef::Node(id)) {
            Ok(ProvenanceChain::Node {
                created_at,
                updated_at,
                incident_edges,
                ..
            }) => {
                let mut lines = vec![format!(
                    "[{n}] is {}, recorded {}.",
                    friendly_type(&node),
                    relative_time(created_at),
                )];
                if updated_at != created_at {
                    lines.push(format!(
                        "It was last updated {}.",
                        relative_time(updated_at)
                    ));
                }
                lines.push(match incident_edges.len() {
                    0 => "It isn't connected to anything else yet.".to_string(),
                    1 => "It's connected to 1 other thing -- try \"/related\".".to_string(),
                    count => format!("It's connected to {count} other things -- try \"/related\"."),
                });
                lines
            }
            Ok(ProvenanceChain::Edge { .. }) => vec![
                "That's a connection between two things, not a thing on its own -- try \
                 \"/related\" on one of its ends instead."
                    .to_string(),
            ],
            Err(e) => vec![format!("I couldn't look that up: {e}")],
        }
    }

    fn resolve(&self, n: usize) -> Option<&NodeId> {
        n.checked_sub(1).and_then(|i| self.refs.get(i))
    }

    fn unknown_ref(n: usize) -> String {
        format!(
            "I don't have a \"[{n}]\" from anything I've shown you recently -- try \"/recall\" \
             first."
        )
    }

    fn render_list(&mut self, items: Vec<(NodeId, NodeRecord)>) -> Vec<String> {
        self.refs = items.iter().map(|(id, _)| *id).collect();
        items
            .iter()
            .enumerate()
            .map(|(i, (_, node))| format!("[{}] {}", i + 1, describe(node)))
            .collect()
    }
}

/// The real utterance text behind a root Intent node, if this is one -- `None` for a decomposed
/// goal's own child tasks (`hyperion_intent::engine::decompose` leaves their `raw_utterance`
/// empty, since nobody actually said "market_research") as well as for every non-"intent" object
/// type. Shared by [`describe`] and [`friendly_type`] so both agree on which nodes were really
/// said by a person -- a real bug found via a live manual check had them disagree.
fn utterance_text(node: &NodeRecord) -> Option<&str> {
    if node.object_type != "intent" {
        return None;
    }
    node.metadata
        .get("raw_utterance")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

/// One human-readable line for a recorded node -- deliberately no "node"/"edge"/"predicate"
/// jargon, matching CLAUDE.md's "never expose technical errors [or internals] directly."
/// `"intent"` (every utterance's own record) is rendered specially since its shape is known
/// exactly ([`hyperion_intent::types::Intent`]'s own JSON, read loosely rather than depending on
/// that crate's exact struct -- this crate's metadata is JSON by design, see
/// `hyperion-knowledge-graph`'s own crate doc); a `"task_result"` node (a real capability
/// dispatch's own real output -- see `hyperion-coordination::CoordinationSession::allocate`) uses
/// [`render_capability_result`]; everything else falls back to a handful of common human-facing
/// keys, then a truncated raw value as a last, honest resort.
fn describe(node: &NodeRecord) -> String {
    if node.object_type == "task_result" {
        if let Some(text) = render_capability_result(&node.metadata) {
            return format!("a result: {text}");
        }
    }

    if let Some(text) = utterance_text(node) {
        return match node.metadata.get("confidence").and_then(|v| v.as_f64()) {
            Some(confidence) => {
                format!(
                    "you asked: \"{text}\" ({:.0}% confident)",
                    confidence * 100.0
                )
            }
            None => format!("you asked: \"{text}\""),
        };
    }

    if node.object_type == "intent" {
        if let Some(predicate) = node
            .metadata
            .get("predicate")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            return format!("a planned task: {predicate}");
        }
    }

    if node.object_type.starts_with("memory_") {
        if let Some(content) = node.metadata.get("content") {
            return format!("a memory: {}", summarize_value(content));
        }
    }

    for key in ["title", "name", "text", "summary", "label", "path"] {
        if let Some(s) = node.metadata.get(key).and_then(|v| v.as_str()) {
            return format!("a {}: {s}", node.object_type);
        }
    }

    format!(
        "a {} ({})",
        node.object_type,
        summarize_value(&node.metadata)
    )
}

fn summarize_value(value: &serde_json::Value) -> String {
    let text = match value {
        serde_json::Value::String(s) => s.clone(),
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
/// Shared by [`describe`] (a `"task_result"` node found via `/recall`/`/related`) and
/// `hyperion_console::session`'s own task-outcome rendering, so a person sees the same real
/// content the same way in both places -- one source of truth for "how do we turn a capability's
/// raw JSON into words," not two definitions that could quietly drift apart.
pub(crate) fn render_capability_result(value: &serde_json::Value) -> Option<String> {
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

fn friendly_type(node: &NodeRecord) -> String {
    if utterance_text(node).is_some() {
        return "something you asked".to_string();
    }
    match node.object_type.as_str() {
        "intent" => "a planned task".to_string(),
        "task_result" => "a result".to_string(),
        t if t.starts_with("memory_") => "a memory".to_string(),
        t => format!("a {t}"),
    }
}

/// A short, human-facing "N ago" -- no new date/time dependency for a console that already has
/// nothing more precise to say than "recently" vs. "days ago."
fn relative_time(created_at: u64) -> String {
    let elapsed = crate::session::now().saturating_sub(created_at);
    match elapsed {
        0..=9 => "just now".to_string(),
        10..=59 => format!("{elapsed} seconds ago"),
        60..=3599 => plural(elapsed / 60, "minute"),
        3600..=86_399 => plural(elapsed / 3600, "hour"),
        _ => plural(elapsed / 86_400, "day"),
    }
}

fn plural(count: u64, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{count} {unit}s ago")
    }
}
