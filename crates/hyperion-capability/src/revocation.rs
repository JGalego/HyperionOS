use std::collections::HashMap;

use crate::types::TokenId;

/// One node in the revocation graph: every derived token is a child of the
/// token it was attenuated from, so revoking a parent revokes the whole
/// delegated subtree — docs/03-kernel-architecture.md §Data Structures.
///
/// The spec models this as an owned tree (`children: Vec<RevocationNode>`).
/// This crate instead stores nodes in a flat arena keyed by [`TokenId`],
/// which is the idiomatic Rust shape for a graph whose nodes need to be
/// found by ID (`cap_revoke` is handed a token, not a path from the root) —
/// same edges, same O(k)-in-outstanding-delegations cascading walk, no
/// change in the structure the spec describes.
#[derive(Debug)]
pub(crate) struct RevocationNode {
    pub(crate) parent: Option<TokenId>,
    pub(crate) children: Vec<TokenId>,
    /// Bumped by `cap_revoke`; a token's own cached `generation` field must
    /// equal this value or every check treats it as stale. Tracked per node
    /// (i.e. per delegation, not per `ObjectId`) — see the design-gap note
    /// on [`crate::types::TokenId`].
    pub(crate) live_generation: u64,
}

#[derive(Debug, Default)]
pub(crate) struct RevocationGraph {
    nodes: HashMap<TokenId, RevocationNode>,
}

/// Returned by `cap_revoke` so a caller can audit how much delegated
/// authority a single revocation just tore down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevocationReceipt {
    pub revoked: TokenId,
    /// Count of descendant nodes invalidated alongside `revoked` itself
    /// (not including `revoked`).
    pub descendants_invalidated: usize,
}

impl RevocationGraph {
    pub(crate) fn insert_root(&mut self, id: TokenId) {
        self.nodes.insert(
            id,
            RevocationNode {
                parent: None,
                children: Vec::new(),
                live_generation: 0,
            },
        );
    }

    pub(crate) fn insert_child(&mut self, parent: TokenId, child: TokenId) {
        self.nodes
            .entry(parent)
            .and_modify(|n| n.children.push(child));
        self.nodes.insert(
            child,
            RevocationNode {
                parent: Some(parent),
                children: Vec::new(),
                live_generation: 0,
            },
        );
    }

    pub(crate) fn live_generation(&self, id: TokenId) -> Option<u64> {
        self.nodes.get(&id).map(|n| n.live_generation)
    }

    /// Which token `id` was derived from, if any — the delegation lineage an
    /// explainability query ("why does this process hold this authority?")
    /// walks, per docs/18-explainability-and-trust.md.
    pub(crate) fn parent_of(&self, id: TokenId) -> Option<TokenId> {
        self.nodes.get(&id).and_then(|n| n.parent)
    }

    /// Bumps `id`'s generation and every descendant's, in one graph walk —
    /// `O(k)` in the number of outstanding delegations beneath `id`, not in
    /// the total number of tokens the monitor has ever minted.
    pub(crate) fn revoke(&mut self, id: TokenId) -> RevocationReceipt {
        let mut stack = vec![id];
        let mut descendants = 0usize;
        let mut first = true;
        while let Some(node_id) = stack.pop() {
            let Some(node) = self.nodes.get_mut(&node_id) else {
                continue;
            };
            node.live_generation = node.live_generation.wrapping_add(1);
            stack.extend(node.children.iter().copied());
            if first {
                first = false;
            } else {
                descendants += 1;
            }
        }
        RevocationReceipt {
            revoked: id,
            descendants_invalidated: descendants,
        }
    }
}
