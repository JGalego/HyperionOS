//! docs/24 §5's review gate: an over-requested permission is rejected
//! pre-consent, a tampered signature is caught, and a `capability_id`
//! collision is either a real competing implementation or a hard
//! rejection — never a silent shadow.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_plugin_framework::{
    signature, validate_manifest, CapabilityGrantRequest, CapabilityManifest, Contribution,
    ImplementationKind, Operation, PluginError, PluginManifest, PluginRegistry, SemanticContract,
    SideEffect, TrustDepth,
};

fn base_manifest() -> PluginManifest {
    PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: 0,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "document.summarize".to_string(),
            contract: SemanticContract {
                inputs: vec!["text".to_string()],
                outputs: vec!["summary".to_string()],
                side_effects: vec![SideEffect::None],
            },
            implementation_kind: ImplementationKind::LocalSmallModel,
            quality_score: 0.5,
            version: 1,
        })],
        requested_permissions: vec![],
        min_trust_depth: TrustDepth::D0,
    }
}

#[test]
fn requesting_network_egress_with_no_declared_side_effect_is_rejected() {
    let mut manifest = base_manifest();
    manifest.requested_permissions = vec![CapabilityGrantRequest {
        operation: Operation::NetworkEgress,
        scope: "any".to_string(),
        justification: "phone home".to_string(),
    }];
    manifest.signature = signature(&manifest);

    let result = validate_manifest(&manifest);
    assert!(matches!(
        result,
        Err(PluginError::PermissionOverreach(Operation::NetworkEgress))
    ));
}

#[test]
fn requesting_network_egress_with_a_declared_side_effect_is_accepted() {
    let mut manifest = base_manifest();
    let Contribution::Capability(cm) = &mut manifest.contributions[0];
    cm.contract.side_effects = vec![SideEffect::NetworkEgress];
    manifest.requested_permissions = vec![CapabilityGrantRequest {
        operation: Operation::NetworkEgress,
        scope: "web.search".to_string(),
        justification: "fetch results".to_string(),
    }];
    manifest.signature = signature(&manifest);

    assert!(validate_manifest(&manifest).is_ok());
}

#[test]
fn a_read_permission_never_needs_a_declared_side_effect() {
    let mut manifest = base_manifest();
    manifest.requested_permissions = vec![CapabilityGrantRequest {
        operation: Operation::Read,
        scope: "notes".to_string(),
        justification: "read notes".to_string(),
    }];
    manifest.signature = signature(&manifest);

    assert!(validate_manifest(&manifest).is_ok());
}

#[test]
fn a_tampered_manifest_fails_signature_verification() {
    let mut manifest = base_manifest();
    manifest.signature = signature(&manifest);
    manifest.publisher = "not-acme-plugins".to_string(); // tampered after signing

    let result = validate_manifest(&manifest);
    assert!(matches!(result, Err(PluginError::SignatureInvalid)));
}

#[test]
fn a_structurally_compatible_collision_becomes_a_second_competing_implementation() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();

    let mut first = base_manifest();
    first.plugin_id = 1;
    first.signature = signature(&first);
    registry
        .install(&mut monitor, &root, first, TrustDepth::D0, true, 1_000)
        .unwrap();

    let mut second = base_manifest();
    second.plugin_id = 2;
    second.signature = signature(&second);
    registry
        .install(&mut monitor, &root, second, TrustDepth::D0, true, 1_001)
        .unwrap();

    let entry = registry.query("document.summarize").unwrap();
    assert_eq!(
        entry.implementations.len(),
        2,
        "two structurally-identical contracts must compete as candidates, not overwrite each other"
    );
}

#[test]
fn a_structurally_incompatible_collision_is_rejected() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();

    let mut first = base_manifest();
    first.plugin_id = 1;
    first.signature = signature(&first);
    registry
        .install(&mut monitor, &root, first, TrustDepth::D0, true, 1_000)
        .unwrap();

    let mut incompatible = base_manifest();
    incompatible.plugin_id = 2;
    let Contribution::Capability(cm) = &mut incompatible.contributions[0];
    cm.contract.outputs = vec!["summary".to_string(), "extra_field".to_string()];
    incompatible.signature = signature(&incompatible);

    let result = registry.install(
        &mut monitor,
        &root,
        incompatible,
        TrustDepth::D0,
        true,
        1_001,
    );
    assert!(matches!(
        result,
        Err(PluginError::CapabilityCollisionIncompatible)
    ));
}
