//! docs/37 §3's `apply_and_explain`: the audit notice is written as a
//! real, tamper-evident `hyperion-observability` entry, and an
//! `AlternateImplementation` substitution is confirmed against a real
//! `hyperion-plugin-framework` registry before that notice is written.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_observability::{AuditAction, AuditLedger, AuditPayload, PrincipalRef};
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, CapabilityManifest, Contribution, ImplementationKind, Operation,
    PluginManifest, PluginRegistry, PrivacyTier, SemanticContract, TrustDepth,
};
use hyperion_scalability::{
    apply_and_explain, CapacityDescriptor, DegradationOutcome, DegradationPlan, ScalabilityError,
    Substitution,
};

fn small_footprint() -> CapacityDescriptor {
    CapacityDescriptor {
        ram_mb: 512,
        vram_mb: 0,
        compute_tops: 1,
    }
}

fn keystore() -> (tempfile::TempDir, Keystore) {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();
    (dir, keystore)
}

fn manifest_with(capability_id: &str, keystore: &Keystore) -> PluginManifest {
    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "acme-plugins".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: capability_id.to_string(),
            contract: SemanticContract {
                inputs: vec!["input".to_string()],
                outputs: vec!["output".to_string()],
                side_effects: vec![],
            },
            implementation_kind: ImplementationKind::LocalSmallModel,
            quality_score: 0.5,
            version: 1,
            native_binary: None,
            privacy_tier: PrivacyTier::Local,
            resource_profile: None,
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Read,
            scope: capability_id.to_string(),
            justification: "provide a real alternate implementation".to_string(),
        }],
        min_trust_depth: TrustDepth::D1,
    };
    manifest.signature = Some(sign(&manifest, keystore));
    manifest
}

#[test]
fn a_degradation_plan_is_recorded_verbatim_in_the_audit_ledger() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let audit = AuditLedger::new();
    let plan = DegradationPlan {
        capability_ref: "vision.generate".to_string(),
        outcome: DegradationOutcome::Substituted {
            substitution: Substitution::Disable,
        },
        notice: "vision.generate disabled on this device".to_string(),
    };

    let installed = apply_and_explain(
        &monitor,
        &root,
        &audit,
        PrincipalRef::System,
        &plan,
        None,
        1_000,
    )
    .unwrap();
    assert!(installed.is_none());

    let entries = audit
        .query(&monitor, &root, |e| e.action == AuditAction::AdminOverride)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].target, Some("vision.generate".to_string()));
    match &entries[0].payload {
        AuditPayload::Note(note) => assert_eq!(note, &plan.notice),
        other => panic!("expected a Note payload, got {other:?}"),
    }
}

#[test]
fn apply_and_explain_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let audit = AuditLedger::new();
    let plan = DegradationPlan {
        capability_ref: "x".to_string(),
        outcome: DegradationOutcome::FullFidelity,
        notice: "n".to_string(),
    };

    let result = apply_and_explain(
        &monitor,
        &read_only,
        &audit,
        PrincipalRef::System,
        &plan,
        None,
        1_000,
    );
    assert!(matches!(result, Err(ScalabilityError::Unauthorized)));
}

#[test]
fn an_alternate_implementation_substitution_naming_a_real_registered_capability_is_recorded() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let audit = AuditLedger::new();
    let registry = PluginRegistry::new();
    let (_dir, keystore) = keystore();
    registry
        .install(
            &mut monitor,
            &root,
            manifest_with("vision.generate.small", &keystore),
            TrustDepth::D2,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let plan = DegradationPlan {
        capability_ref: "vision.generate".to_string(),
        outcome: DegradationOutcome::Substituted {
            substitution: Substitution::AlternateImplementation(
                "vision.generate.small".to_string(),
                small_footprint(),
            ),
        },
        notice: "vision.generate substituted with a smaller local model".to_string(),
    };

    let installed = apply_and_explain(
        &monitor,
        &root,
        &audit,
        PrincipalRef::System,
        &plan,
        Some(&registry),
        1_000,
    )
    .unwrap();

    let entry = installed.expect("an AlternateImplementation substitution must return the real, confirmed RegistryEntry it was checked against");
    assert_eq!(entry.capability_id, "vision.generate.small");
    assert_eq!(entry.implementations.len(), 1);
    assert_eq!(entry.implementations[0].quality_score, 0.5);

    let entries = audit
        .query(&monitor, &root, |e| e.action == AuditAction::AdminOverride)
        .unwrap();
    assert_eq!(entries.len(), 1);
}

#[test]
fn an_alternate_implementation_substitution_naming_an_unregistered_capability_is_refused() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let audit = AuditLedger::new();
    let registry = PluginRegistry::new();

    let plan = DegradationPlan {
        capability_ref: "vision.generate".to_string(),
        outcome: DegradationOutcome::Substituted {
            substitution: Substitution::AlternateImplementation(
                "vision.generate.nonexistent".to_string(),
                small_footprint(),
            ),
        },
        notice: "vision.generate substituted with a smaller local model".to_string(),
    };

    let result = apply_and_explain(
        &monitor,
        &root,
        &audit,
        PrincipalRef::System,
        &plan,
        Some(&registry),
        1_000,
    );
    assert!(matches!(
        result,
        Err(ScalabilityError::AlternateImplementationNotRegistered(ref c)) if c == "vision.generate.nonexistent"
    ));

    // No audit notice must have been written claiming a fallback that never happened.
    let entries = audit
        .query(&monitor, &root, |e| e.action == AuditAction::AdminOverride)
        .unwrap();
    assert!(entries.is_empty());
}
