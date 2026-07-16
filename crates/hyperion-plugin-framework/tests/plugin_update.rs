//! docs/24 §5's `plugin_update`, closing this crate's own previously-named gap ("this crate has
//! no `plugin_update` distinct from `uninstall` + `install`; a caller wanting the diff-only UX
//! composes those two calls itself"): an update presents only the *new* grants a caller hasn't
//! already consented to, reuses already-minted tokens for anything unchanged, and really revokes
//! a token for a permission the new manifest drops.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, CapabilityManifest, Contribution, ImplementationKind, Operation,
    PluginError, PluginManifest, PluginRegistry, SemanticContract, SideEffect, TrustDepth,
};

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn manifest(
    keystore: &Keystore,
    capability_id: &str,
    permissions: Vec<CapabilityGrantRequest>,
) -> PluginManifest {
    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: capability_id.to_string(),
            contract: SemanticContract {
                inputs: vec!["query".to_string()],
                outputs: vec!["results".to_string()],
                side_effects: vec![SideEffect::NetworkEgress],
            },
            implementation_kind: ImplementationKind::CloudApi,
            quality_score: 0.5,
            version: 1,
            native_binary: None,
        })],
        requested_permissions: permissions,
        min_trust_depth: TrustDepth::D1,
    };
    manifest.signature = Some(sign(&manifest, keystore));
    manifest
}

fn network_grant(scope: &str) -> CapabilityGrantRequest {
    CapabilityGrantRequest {
        operation: Operation::NetworkEgress,
        scope: scope.to_string(),
        justification: "fetch results".to_string(),
    }
}

fn read_grant(scope: &str) -> CapabilityGrantRequest {
    CapabilityGrantRequest {
        operation: Operation::Read,
        scope: scope.to_string(),
        justification: "read local state".to_string(),
    }
}

#[test]
fn an_update_with_no_new_permissions_needs_no_consent() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest(&keystore, "web.search", vec![network_grant("web.search")]),
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    // Same manifest, same one permission -- a real update with no new grants at all.
    let new_grants = registry
        .update(
            &mut monitor,
            &root,
            handle.plugin_id,
            manifest(&keystore, "web.search", vec![network_grant("web.search")]),
            TrustDepth::D2,
            false, // consent withheld -- must not matter, since nothing new was asked for
            &keystore.verifying_key(),
        )
        .unwrap();

    assert!(
        new_grants.is_empty(),
        "no new permission was requested, so the real diff must be empty"
    );
}

#[test]
fn an_update_adding_a_permission_is_rejected_without_consent_and_changes_nothing() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest(&keystore, "web.search", vec![network_grant("web.search")]),
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let result = registry.update(
        &mut monitor,
        &root,
        handle.plugin_id,
        manifest(
            &keystore,
            "web.search",
            vec![network_grant("web.search"), read_grant("local.cache")],
        ),
        TrustDepth::D2,
        false,
        &keystore.verifying_key(),
    );

    assert!(matches!(result, Err(PluginError::ConsentDeclined)));
    assert_eq!(
        registry.query("web.search").unwrap().implementations[0].quality_score,
        0.5,
        "a declined update must leave the plugin's previous install untouched"
    );
}

#[test]
fn an_update_adding_a_permission_returns_exactly_the_new_grant_and_reuses_the_old_token() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest(&keystore, "web.search", vec![network_grant("web.search")]),
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
    let old_token = registry.tokens_of(handle.plugin_id).unwrap()[0].clone();

    let new_grants = registry
        .update(
            &mut monitor,
            &root,
            handle.plugin_id,
            manifest(
                &keystore,
                "web.search",
                vec![network_grant("web.search"), read_grant("local.cache")],
            ),
            TrustDepth::D2,
            true,
            &keystore.verifying_key(),
        )
        .unwrap();

    assert_eq!(new_grants.len(), 1);
    assert_eq!(new_grants[0].scope, "local.cache");

    let updated_tokens = registry.tokens_of(handle.plugin_id).unwrap();
    assert_eq!(updated_tokens.len(), 2);
    assert_eq!(
        updated_tokens[0], old_token,
        "the unchanged web.search grant must reuse its exact original token, not a freshly \
         re-derived one with merely equivalent rights"
    );
    assert!(monitor.is_live(&updated_tokens[1]));
}

#[test]
fn an_update_dropping_a_permission_really_revokes_its_token() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest(
                &keystore,
                "web.search",
                vec![network_grant("web.search"), read_grant("local.cache")],
            ),
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();
    let tokens = registry.tokens_of(handle.plugin_id).unwrap();
    let (kept_token, dropped_token) = (tokens[0].clone(), tokens[1].clone());
    assert!(monitor.is_live(&dropped_token));

    // The updated manifest drops the local.cache read grant entirely -- no new grants, so no
    // consent is required, but the dropped permission's own token must really stop working.
    registry
        .update(
            &mut monitor,
            &root,
            handle.plugin_id,
            manifest(&keystore, "web.search", vec![network_grant("web.search")]),
            TrustDepth::D2,
            false,
            &keystore.verifying_key(),
        )
        .unwrap();

    assert!(
        monitor.is_live(&kept_token),
        "a permission still requested after the update must remain live"
    );
    assert!(
        !monitor.is_live(&dropped_token),
        "a permission dropped by the update must really be revoked, not left grantable forever"
    );
}

#[test]
fn an_update_replaces_the_registered_contribution() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest(&keystore, "web.search", vec![network_grant("web.search")]),
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let mut new_manifest = manifest(&keystore, "web.search", vec![network_grant("web.search")]);
    match &mut new_manifest.contributions[0] {
        Contribution::Capability(cm) => cm.quality_score = 0.9,
        _ => unreachable!(),
    }
    new_manifest.signature = Some(sign(&new_manifest, &keystore));

    registry
        .update(
            &mut monitor,
            &root,
            handle.plugin_id,
            new_manifest,
            TrustDepth::D2,
            false,
            &keystore.verifying_key(),
        )
        .unwrap();

    let entry = registry.query("web.search").unwrap();
    assert_eq!(
        entry.implementations.len(),
        1,
        "the old contribution must be replaced, not stacked"
    );
    assert_eq!(entry.implementations[0].quality_score, 0.9);
}

#[test]
fn updating_an_unknown_plugin_fails() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let result = registry.update(
        &mut monitor,
        &root,
        999,
        manifest(&keystore, "web.search", vec![network_grant("web.search")]),
        TrustDepth::D2,
        true,
        &keystore.verifying_key(),
    );
    assert!(matches!(result, Err(PluginError::NoSuchPlugin)));
}

#[test]
fn updating_requires_grant_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();

    let handle = registry
        .install(
            &mut monitor,
            &root,
            manifest(&keystore, "web.search", vec![network_grant("web.search")]),
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let result = registry.update(
        &mut monitor,
        &read_only,
        handle.plugin_id,
        manifest(&keystore, "web.search", vec![network_grant("web.search")]),
        TrustDepth::D2,
        true,
        &keystore.verifying_key(),
    );
    assert!(matches!(result, Err(PluginError::Unauthorized)));
}
