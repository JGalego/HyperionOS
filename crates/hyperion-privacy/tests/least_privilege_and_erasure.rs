//! docs/16 §5's least-privilege context assembly and erasure.

use std::collections::HashSet;
use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_privacy::{
    erase, filter_for_recipient, ErasureMode, PrivacyTier, ResidencyTag, SensitivityClass,
};

#[test]
fn an_object_the_recipient_did_not_declare_needing_is_withheld() {
    let a = hyperion_storage::ObjectId(1);
    let b = hyperion_storage::ObjectId(2);
    let tags = vec![
        ResidencyTag::new(
            a,
            SensitivityClass::Public,
            [
                PrivacyTier::FullyLocal,
                PrivacyTier::LocalPreferredWithConsent,
                PrivacyTier::CloudAssisted,
            ]
            .into_iter()
            .collect(),
        ),
        ResidencyTag::new(
            b,
            SensitivityClass::Public,
            [
                PrivacyTier::FullyLocal,
                PrivacyTier::LocalPreferredWithConsent,
                PrivacyTier::CloudAssisted,
            ]
            .into_iter()
            .collect(),
        ),
    ];
    let declared_need: HashSet<_> = [a].into_iter().collect();

    let (included, withheld) =
        filter_for_recipient(&tags, &declared_need, PrivacyTier::CloudAssisted);
    assert_eq!(included, vec![a]);
    assert_eq!(withheld.len(), 1);
    assert_eq!(withheld[0].0, b);
}

#[test]
fn an_object_whose_residency_forbids_the_recipients_tier_is_withheld_with_a_reason() {
    let restricted = hyperion_storage::ObjectId(1);
    let tags = vec![ResidencyTag::new(
        restricted,
        SensitivityClass::Restricted,
        [
            PrivacyTier::FullyLocal,
            PrivacyTier::LocalPreferredWithConsent,
            PrivacyTier::CloudAssisted,
        ]
        .into_iter()
        .collect(),
    )];
    let declared_need: HashSet<_> = [restricted].into_iter().collect();

    let (included, withheld) =
        filter_for_recipient(&tags, &declared_need, PrivacyTier::CloudAssisted);
    assert!(included.is_empty());
    assert_eq!(withheld[0].0, restricted);
    assert!(withheld[0].1.contains("residency forbids"));
}

#[test]
fn an_object_that_is_both_needed_and_permitted_is_included() {
    let a = hyperion_storage::ObjectId(1);
    let tags = vec![ResidencyTag::new(
        a,
        SensitivityClass::Personal,
        [
            PrivacyTier::FullyLocal,
            PrivacyTier::LocalPreferredWithConsent,
        ]
        .into_iter()
        .collect(),
    )];
    let declared_need: HashSet<_> = [a].into_iter().collect();

    let (included, withheld) = filter_for_recipient(
        &tags,
        &declared_need,
        PrivacyTier::LocalPreferredWithConsent,
    );
    assert_eq!(included, vec![a]);
    assert!(withheld.is_empty());
}

#[test]
fn erasing_an_object_overwrites_its_metadata_and_returns_a_receipt() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let node = graph
        .put_node(
            &monitor,
            &root,
            None,
            "Note",
            None,
            serde_json::json!({"text": "sensitive"}),
        )
        .unwrap();

    let receipt = erase(
        &monitor,
        &root,
        &graph,
        &[node],
        ErasureMode::CryptoShred,
        1_000,
    )
    .unwrap();
    assert_eq!(receipt.object_ids, vec![node]);
    assert_eq!(receipt.completed_at, Some(1_000));

    let after = graph.get(&monitor, &root, node).unwrap();
    assert_eq!(after.metadata["erased"], serde_json::json!(true));
    assert!(
        after.metadata.get("text").is_none(),
        "erasure must overwrite the prior sensitive fields, not merely tag them"
    );
}
