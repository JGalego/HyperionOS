//! Turns `hyperion-knowledge-graph`'s raw node/edge primitives into the console's
//! `/recall`/`/why`/`/related`/`/result` meta-commands -- ways to explore whatever Hyperion has
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
    render_capability_result, EdgeId, EdgeRecord, ExplainRef, GraphQuery, KnowledgeGraph, NodeId,
    NodeRecord, ProvenanceChain,
};

/// The most results a single `/recall`/`/related` list shows -- generous enough to browse, small
/// enough that a numbered list is still something a person can actually scan.
const MAX_RESULTS: usize = 20;

/// How much of a real, potentially long/multi-paragraph capability result [`preview`] shows in a
/// list context -- a real cloud model's own real answer to "draft a business model" can run to
/// several paragraphs, which would flood the terminal if shown in full inside a numbered list
/// alongside everything else recalled. `/why` shows the untruncated text instead -- this is
/// deliberately just a teaser pointing there, not the only place to ever see the real content.
const PREVIEW_CHARS: usize = 100;

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
                needle.is_empty() || node.display_label().to_ascii_lowercase().contains(&needle)
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
                // Unlike `describe()`'s list-context preview, `/why` is "tell me everything
                // about this one thing" -- the real, untruncated text belongs here.
                if node.object_type == "task_result" {
                    if let Some(text) = render_capability_result(&node.metadata) {
                        lines.push(String::new());
                        lines.push(text);
                    }
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

    /// `/result <task>` -- the real, complete text a named task's own capability dispatch
    /// produced, found directly via its real `"produced"` edge rather than a numbered
    /// `/recall`/`/related` detour. Deliberately matches the task's own real `predicate` field
    /// exactly (case-insensitive), not a substring search over rendered text: a real model's own
    /// generated prose often naturalizes a task's snake_case predicate into plain words (a real,
    /// observed case: `legal_formation`'s own real result talks about "legal formation," never
    /// the literal string `"legal_formation"`), so `/recall <task>` alone can miss the very
    /// result a user is looking for.
    pub fn result(
        &mut self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        task_name: &str,
    ) -> Vec<String> {
        let task_name = task_name.trim();
        let hits = match self.graph.query(monitor, token, &GraphQuery::default()) {
            Ok(hits) => hits,
            Err(e) => return vec![format!("I couldn't look that up: {e}")],
        };

        let mut tasks: Vec<(NodeId, NodeRecord)> = hits
            .into_iter()
            .map(|hit| (hit.node_id, hit.node))
            .filter(|(_, node)| {
                node.object_type == "intent"
                    && node
                        .metadata
                        .get("predicate")
                        .and_then(|v| v.as_str())
                        .is_some_and(|p| p.eq_ignore_ascii_case(task_name))
            })
            .collect();
        // `(updated_at, id)`, not `updated_at` alone: `now()`'s second granularity means two
        // nodes written within the same real second tie on `updated_at`, and a plain reverse
        // sort would then fall back to `hits`' own (arbitrary, not creation-order) iteration
        // order -- a real, observed bug for `results` below, fixed the same defensive way here.
        // `NodeId` is `hyperion_storage`'s own monotonically-assigned `next_object_id`, so it's a
        // real, race-free "which one is newer" signal regardless of wall-clock resolution.
        tasks.sort_by_key(|(id, node)| std::cmp::Reverse((node.updated_at, *id)));

        let Some((task_id, _)) = tasks.into_iter().next() else {
            return vec![format!("I don't have a task called \"{task_name}\".")];
        };

        let subgraph = match self.graph.traverse(monitor, token, task_id, None, 1) {
            Ok(subgraph) => subgraph,
            Err(e) => return vec![format!("I couldn't look that up: {e}")],
        };
        let mut results: Vec<(NodeId, NodeRecord)> = subgraph
            .nodes
            .into_iter()
            .filter(|(id, node, hop)| {
                *id != task_id && *hop > 0 && node.object_type == "task_result"
            })
            .map(|(id, node, _)| (id, node))
            .collect();
        // A real, previously-shipped bug this fixed: a `/redo` that completes within the same
        // real wall-clock second as the task's first dispatch ties on `updated_at` alone (`now()`
        // is second-granularity) against the now-stale original `task_result` node, and a plain
        // reverse sort then keeps whichever the traversal happened to return first -- silently
        // showing the *old* result right after a real redo. `NodeId` order breaks the tie for
        // real: `task_result` nodes are only ever created fresh, never updated in place, so a
        // higher id always means a newer real result.
        results.sort_by_key(|(id, node)| std::cmp::Reverse((node.updated_at, *id)));

        let Some((result_id, result_node)) = results.into_iter().next() else {
            return vec![format!(
                "\"{task_name}\" doesn't have a real result yet -- it may still be in progress."
            )];
        };

        // A single-item numbered list -- so a follow-up "/related 1" can still explore onward
        // from the real result itself, exactly as if it had been found via `/recall`.
        self.refs = vec![result_id];
        match render_capability_result(&result_node.metadata) {
            Some(text) => vec![
                format!(
                    "\"{task_name}\"'s real result, recorded {}:",
                    relative_time(result_node.created_at)
                ),
                String::new(),
                text,
            ],
            None => vec![format!(
                "\"{task_name}\" has a recorded result, but I couldn't render it."
            )],
        }
    }

    /// `/graph` (plain text) / `/graph dot` (Graphviz) -- the *whole* recorded graph at once,
    /// unlike `/recall`/`/related`/`/result` (each a targeted question about one thing). Built for
    /// checking how a session's knowledge graph actually changed -- run once before and once after
    /// a scenario (see docs/999-usage-scenarios.md) and diff the two outputs. That comparison only works
    /// because [`KnowledgeGraph::dump`] sorts both nodes and edges by id: two dumps of an unchanged
    /// graph are byte-for-byte identical, so every line a diff shows is a real change, never
    /// ordering noise. Deliberately shows raw ids and absolute (epoch-second) timestamps rather
    /// than `/why`'s human "recorded 3 minutes ago" phrasing -- a relative phrasing would make an
    /// unchanged dump look different depending on *when* you happened to run it, defeating the
    /// point. This is a debugging/inspection surface, not a conversational answer, so that
    /// trade-off runs the other way from the rest of this module.
    pub fn dump_graph(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        as_dot: bool,
    ) -> Vec<String> {
        let snapshot = match self.graph.dump(monitor, token) {
            Ok(snapshot) => snapshot,
            Err(e) => return vec![format!("I couldn't look at the graph: {e}")],
        };

        if as_dot {
            return render_dot(&snapshot.nodes, &snapshot.edges);
        }

        if snapshot.nodes.is_empty() {
            return vec!["The knowledge graph is empty -- nothing recorded yet.".to_string()];
        }

        let mut lines = vec![format!(
            "{} node{}:",
            snapshot.nodes.len(),
            if snapshot.nodes.len() == 1 { "" } else { "s" }
        )];
        for (id, node) in &snapshot.nodes {
            lines.push(format!(
                "  [{}] {} -- {} (created {}, updated {})",
                id.0,
                node.object_type,
                node.display_label(),
                node.created_at,
                node.updated_at
            ));
        }

        lines.push(String::new());
        lines.push(format!(
            "{} edge{}:",
            snapshot.edges.len(),
            if snapshot.edges.len() == 1 { "" } else { "s" }
        ));
        for (id, edge) in &snapshot.edges {
            lines.push(format!(
                "  [{}] {} --{}--> {} (weight {:.2}{})",
                id.0,
                edge.subject.0,
                edge.predicate,
                edge.target.0,
                edge.weight,
                match edge.confidence {
                    Some(c) => format!(", confidence {c:.2}"),
                    None => String::new(),
                },
            ));
        }
        lines
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
            .map(|(i, (_, node))| format!("[{}] {}", i + 1, preview(&node.display_label())))
            .collect()
    }
}

/// A single-line, length-capped teaser of `text` -- the first line, capped at
/// [`PREVIEW_CHARS`], with a trailing `"..."` whenever anything was actually cut off (either the
/// first line itself was too long, or there was real content after it). Shared by
/// [`NodeRecord::display_label`] callers here (list context) and `hyperion_console::session`'s own
/// task-outcome rendering -- `/why` is where the same node's full, untruncated text lives instead.
pub(crate) fn preview(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or(text);
    let truncated: String = first_line.chars().take(PREVIEW_CHARS).collect();
    if first_line.chars().count() > PREVIEW_CHARS || text.lines().count() > 1 {
        format!("{truncated}...")
    } else {
        truncated
    }
}

/// `/graph dot`'s real output: valid Graphviz DOT (`digraph { ... }`), so it can actually be
/// *drawn* -- `dot -Tsvg` or any online Graphviz renderer -- rather than only ever read as text.
/// A deliberate second format, not the default: plain text (see [`GraphExplorer::dump_graph`])
/// stays screen-reader-friendly and diffable with plain `diff`, matching CLAUDE.md's
/// accessibility-first stance; DOT is an opt-in for whoever specifically wants a picture.
fn render_dot(nodes: &[(NodeId, NodeRecord)], edges: &[(EdgeId, EdgeRecord)]) -> Vec<String> {
    let mut lines = vec!["digraph knowledge_graph {".to_string()];
    for (id, node) in nodes {
        let label = format!(
            "[{}] {}: {}",
            id.0,
            node.object_type,
            preview(&node.display_label())
        );
        lines.push(format!(
            "  \"{}\" [label=\"{}\"];",
            id.0,
            dot_escape(&label)
        ));
    }
    for (_, edge) in edges {
        lines.push(format!(
            "  \"{}\" -> \"{}\" [label=\"{}\"];",
            edge.subject.0,
            edge.target.0,
            dot_escape(&edge.predicate)
        ));
    }
    lines.push("}".to_string());
    lines
}

/// Graphviz DOT quoted-string escaping -- just `"` and `\`, the only two characters that would
/// otherwise break out of a `label="..."` attribute; every real label here is plain, short,
/// generated text (a node's own [`describe`] output or an edge's own predicate), never arbitrary
/// untrusted input, so this narrow escaping is enough.
fn dot_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn friendly_type(node: &NodeRecord) -> String {
    if node.utterance_text().is_some() {
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
