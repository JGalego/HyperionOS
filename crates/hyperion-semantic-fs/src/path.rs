use std::collections::HashMap;

use hyperion_knowledge_graph::NodeId;

#[derive(Debug, Clone)]
pub(crate) struct PathEntry {
    pub(crate) path: String,
    pub(crate) is_collection: bool,
}

/// docs/10 §Data Structures' `PathMapping cache`: `{synthesized_path <->
/// object_id}`, populated lazily, never precomputed for the whole graph.
#[derive(Debug, Default)]
pub(crate) struct PathMappingCache {
    forward: HashMap<NodeId, PathEntry>,
    reverse: HashMap<String, NodeId>,
}

impl PathMappingCache {
    pub(crate) fn entry(&self, id: NodeId) -> Option<&PathEntry> {
        self.forward.get(&id)
    }

    pub(crate) fn resolve(&self, path: &str) -> Option<NodeId> {
        self.reverse.get(path).copied()
    }

    /// docs/10 §Algorithms' "Path synthesis" + §Recovery Mechanisms'
    /// "stable suffix ordering keyed by object_id, not creation order":
    /// an object that already has a cached path always keeps it (first
    /// assignment is permanent — legacy tooling depends on that); among
    /// members that don't yet have one, collisions within *this batch*
    /// are resolved lowest-`object_id`-first rather than by whatever
    /// arbitrary order the caller's member list happens to be in, so
    /// repeated resolutions of the same (or overlapping) member set always
    /// converge on the same mapping.
    pub(crate) fn synthesize_batch(&mut self, candidates: &[(NodeId, String)]) -> Vec<String> {
        let mut order: Vec<usize> = (0..candidates.len()).collect();
        order.sort_by_key(|&i| candidates[i].0);

        let mut results = vec![String::new(); candidates.len()];
        for i in order {
            let (id, base) = &candidates[i];
            if let Some(existing) = self.forward.get(id) {
                results[i] = existing.path.clone();
                continue;
            }
            let mut suffix = 1u32;
            let mut path = base.clone();
            while self.reverse.contains_key(&path) {
                suffix += 1;
                path = format!("{base}-{suffix}");
            }
            self.forward.insert(
                *id,
                PathEntry {
                    path: path.clone(),
                    is_collection: false,
                },
            );
            self.reverse.insert(path.clone(), *id);
            results[i] = path;
        }
        results
    }

    /// Pins an explicit path (e.g. from `write_back` into a virtual
    /// folder) without going through collision disambiguation — the
    /// caller already knows the exact path.
    pub(crate) fn pin(&mut self, path: String, id: NodeId, is_collection: bool) {
        if let Some(old) = self.forward.get(&id) {
            if old.path != path {
                self.reverse.remove(&old.path.clone());
            }
        }
        self.forward.insert(
            id,
            PathEntry {
                path: path.clone(),
                is_collection,
            },
        );
        self.reverse.insert(path, id);
    }
}
