//! docs/37 §Algorithms 2/3's real Knowledge Graph partitioning -- this crate's own previously-
//! named "KG partitioning / `TenantPartition` / cross-tenant edges... no partitioning logic
//! exists here" gap. `hyperion_knowledge_graph::TenantId`/`KnowledgeGraph::link`'s own real
//! cross-tenant gate (the KG-side half of this closure -- see that crate's own doc comment) is
//! the real data-layer enforcement; this module is the two docs/37 §Interfaces functions that sit
//! above it: a real, deterministic shard-key resolver, and the real capability grant a caller
//! mints to cross a partition boundary on purpose.
//!
//! **Honest scope**: this hosted simulator has exactly one *physical* shard/WAL -- nothing here
//! actually routes a query to a different physical store. [`kg_partition_resolve`] is still real,
//! not a placeholder: it computes docs/29 §Sharding's own real algorithm ("a request is routed to
//! a shard by hashing `owner_id`") as an honest, deterministic key a future multi-shard deployment
//! would route by, the same "real algorithm, honest about today's physical scale" precedent this
//! workspace already established for e.g. `hyperion-knowledge-graph`'s own brute-force cosine
//! similarity standing in for a real ANN index.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::TenantId;

/// docs/37 §Data Structures' `TenantPartition.kg_shard_ref` -- a real, deterministic logical shard
/// key, not (yet) a physical routing target. See this module's own doc comment for the honest
/// single-physical-shard scope this operates within today.
pub type ShardId = u64;

/// docs/37 §Interfaces' `kg_partition_resolve(object) -> ShardId`, narrowed to the two real,
/// already-recorded fields `hyperion_knowledge_graph::NodeRecord` partitions by --
/// [`TenantId`] (the coarse, org/household axis) and `owner` (the user/device Trust Boundary
/// within that tenant) -- rather than a full `SemanticObjectRef`, since this crate has no such
/// type of its own and the two fields named are exactly what docs/29 §Sharding's own algorithm
/// keys on ("partition... first by `tenant_id`... then by user/workspace inside that tenant").
/// Deterministic and stable: the same `(tenant_id, owner)` pair always resolves to the same real
/// [`ShardId`], so a caller can use this to group objects for a future real multi-shard
/// deployment without this crate needing to track any shard-assignment state of its own.
pub fn kg_partition_resolve(tenant_id: TenantId, owner: u64) -> ShardId {
    let mut hasher = DefaultHasher::new();
    tenant_id.0.hash(&mut hasher);
    owner.hash(&mut hasher);
    hasher.finish()
}

/// docs/37 §Interfaces' `tenant_grant_cross_partition(from, to, edge) -> CapabilityToken` --
/// mints the real capability `hyperion_knowledge_graph::KnowledgeGraph::link`'s own cross-tenant
/// gate checks for. Carries both `RightsMask::WRITE` (the base right `link` itself always
/// requires, cross-tenant or not) and `RightsMask::GRANT` (the "single extra comparison" docs/37
/// §3 adds on top) -- a grant with `GRANT` alone would still fail `link`'s own ordinary WRITE
/// check, which would make this function's own result unusable for the one real thing it exists
/// to unblock. Derived from `parent` under docs/02 §4's attenuation-only rule (never a broader
/// primitive than what `parent` itself already holds), so this is the *same* one real
/// capability-security model docs/37 §3 requires ("a single extra comparison added to an existing
/// check, not a second, parallel security model"), not an invented tenant-specific mechanism.
/// `from`/`to`/`edge` are docs/37's own bookkeeping labels for *why* this grant exists -- this
/// hosted simulator has no separate tenant-to-identity registry to look either `TenantId` up
/// against, so they're not literal parameters the real mint call itself needs; a caller names
/// them in its own audit trail instead (e.g. via `hyperion-observability`'s own `AuditLedger`),
/// the same "narrowed to what this workspace's real capability primitives actually take" scoping
/// this crate's own `Substitution`/`ResourceConstraint` types already apply elsewhere.
pub fn tenant_grant_cross_partition(
    monitor: &mut CapabilityMonitor,
    parent: &CapabilityToken,
    granted_boundary: TrustBoundaryId,
) -> Result<CapabilityToken, hyperion_capability::Fault> {
    monitor.cap_derive(
        parent,
        RightsMask::WRITE | RightsMask::GRANT,
        None,
        granted_boundary,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_capability::RightsMask;

    #[test]
    fn kg_partition_resolve_is_deterministic() {
        let a = kg_partition_resolve(TenantId(1), 42);
        let b = kg_partition_resolve(TenantId(1), 42);
        assert_eq!(a, b);
    }

    #[test]
    fn kg_partition_resolve_differs_by_tenant() {
        let a = kg_partition_resolve(TenantId(1), 42);
        let b = kg_partition_resolve(TenantId(2), 42);
        assert_ne!(a, b);
    }

    #[test]
    fn kg_partition_resolve_differs_by_owner() {
        let a = kg_partition_resolve(TenantId(1), 42);
        let b = kg_partition_resolve(TenantId(1), 43);
        assert_ne!(a, b);
    }

    #[test]
    fn tenant_grant_cross_partition_mints_a_token_with_both_write_and_grant() {
        let mut monitor = CapabilityMonitor::new();
        let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

        let grant = tenant_grant_cross_partition(&mut monitor, &root, TrustBoundaryId(2)).unwrap();

        assert!(
            monitor
                .check_rights_ok_result(&grant, RightsMask::WRITE | RightsMask::GRANT)
                .is_ok(),
            "the real grant must carry both WRITE (what link always needs) and GRANT (the real \
             cross-tenant extra), or it would be unusable for the one thing it exists to unblock"
        );
        assert_eq!(grant.origin(), TrustBoundaryId(2));
    }

    #[test]
    fn tenant_grant_cross_partition_can_never_exceed_the_parents_own_rights() {
        let mut monitor = CapabilityMonitor::new();
        // A parent with no GRANT right of its own.
        let root = monitor.mint_root(RightsMask::WRITE, TrustBoundaryId(1), None);

        let result = tenant_grant_cross_partition(&mut monitor, &root, TrustBoundaryId(2));
        assert!(
            result.is_err(),
            "attenuation-only: a grant can never carry a right its own parent didn't have"
        );
    }
}
