use std::collections::HashSet;

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    CapabilityGrantRequest, CapabilityManifest, Contribution, ImplementationKind, PluginHandle,
    PluginManifest, PluginRegistry, SemanticContract, TrustDepth,
};

use crate::types::{Contract, Implementation, PublishSubmission, ReviewStatus, Runtime, SdkError};

fn sensitive(op: hyperion_plugin_framework::Operation) -> bool {
    use hyperion_plugin_framework::Operation;
    matches!(op, Operation::NetworkEgress | Operation::Write)
}

fn implementation_kind(runtime: Runtime) -> ImplementationKind {
    match runtime {
        Runtime::LocalModel => ImplementationKind::LocalSmallModel,
        Runtime::CloudApi => ImplementationKind::CloudApi,
        Runtime::NativeBinary | Runtime::ComposedCapability => ImplementationKind::NativeBinary,
    }
}

fn to_capability_manifest(
    contract: &Contract,
    implementation: &Implementation,
    quality_score: f32,
) -> CapabilityManifest {
    CapabilityManifest {
        capability_id: contract.id.clone(),
        contract: SemanticContract {
            inputs: contract.inputs.clone(),
            outputs: contract.outputs.clone(),
            side_effects: contract.side_effects.clone(),
        },
        implementation_kind: implementation_kind(implementation.runtime),
        quality_score,
        version: contract.version,
        // AUTONOMY_ROADMAP.md's "tool creation" slice: carried straight through from the
        // submission now, so a `Runtime::NativeBinary` `Implementation` that names a real,
        // existing, executable program installs as a genuinely *runnable* capability -- not just
        // a labeled placeholder -- the moment it's published.
        native_binary: implementation.native_binary.clone(),
    }
}

/// docs/24's `PluginManifest` a published (Contract, Implementation)
/// pair compiles down to — the seam docs/25 §3 names but doesn't itself
/// define ("25 explicitly states it does not own the registry (24)").
#[allow(clippy::too_many_arguments)]
pub fn to_plugin_manifest(
    contract: &Contract,
    implementation: &Implementation,
    quality_score: f32,
    plugin_id: u64,
    publisher: &str,
    sdk_version: u32,
    keystore: &Keystore,
) -> PluginManifest {
    let requested_permissions = contract
        .permissions_requested
        .iter()
        .map(|p| CapabilityGrantRequest {
            operation: p.operation,
            scope: p.scope.clone(),
            justification: p.justification.clone(),
        })
        .collect();

    let mut manifest = PluginManifest {
        plugin_id,
        publisher: publisher.to_string(),
        signature: None,
        sdk_version,
        contributions: vec![Contribution::Capability(to_capability_manifest(
            contract,
            implementation,
            quality_score,
        ))],
        requested_permissions,
        min_trust_depth: contract.trust_level.min_depth(),
    };
    manifest.signature = Some(hyperion_plugin_framework::sign(&manifest, keystore));
    manifest
}

/// docs/25 §4's publish workflow, up to (not including) the network
/// submission step: static permission analysis — an implementation that
/// statically observed a permission the contract never declared fails
/// the build outright, before any review — then routes to
/// [`crate::types::ReviewStatus`] by whether any declared permission is
/// sensitive (`NetworkEgress`/`Write`).
pub fn prepare_submission(
    contract: Contract,
    implementation: Implementation,
    quality_score: f32,
    statically_observed_permissions: Vec<hyperion_plugin_framework::Operation>,
) -> Result<PublishSubmission, SdkError> {
    let declared: HashSet<hyperion_plugin_framework::Operation> = contract
        .permissions_requested
        .iter()
        .map(|p| p.operation)
        .collect();
    let observed: HashSet<hyperion_plugin_framework::Operation> =
        statically_observed_permissions.iter().copied().collect();

    if !observed.is_subset(&declared) {
        return Err(SdkError::UndeclaredPermissionObserved);
    }

    let review_status = if declared.iter().any(|&op| sensitive(op)) {
        ReviewStatus::PendingHumanReview
    } else {
        ReviewStatus::AutoApproved
    };

    Ok(PublishSubmission {
        package_hash: 0,
        contract,
        implementation,
        quality_score,
        declared_permissions: declared.into_iter().collect(),
        statically_observed_permissions,
        review_status,
    })
}

/// docs/25 §4's `hyperion publish`: resolves the submission's review
/// gate (auto-approved proceeds immediately; pending-human-review needs
/// the caller-supplied `human_approved` — the same "caller supplies the
/// confirmation, no real prompt UI" pattern this workspace already uses
/// throughout), then compiles the (Contract, Implementation) pair into a
/// `PluginManifest` and installs it through the real
/// `hyperion-plugin-framework` registry — never a separate, parallel
/// installation path.
#[allow(clippy::too_many_arguments)]
pub fn publish(
    monitor: &mut CapabilityMonitor,
    admin_token: &CapabilityToken,
    registry: &PluginRegistry,
    submission: PublishSubmission,
    plugin_id: u64,
    publisher: &str,
    sdk_version: u32,
    human_approved: bool,
    available_depth: TrustDepth,
    now: u64,
    keystore: &Keystore,
) -> Result<PluginHandle, SdkError> {
    let approved = match submission.review_status {
        ReviewStatus::Rejected => return Err(SdkError::SubmissionRejected),
        ReviewStatus::AutoApproved => true,
        ReviewStatus::PendingHumanReview => human_approved,
    };
    if !approved {
        return Err(SdkError::SubmissionRejected);
    }

    let manifest = to_plugin_manifest(
        &submission.contract,
        &submission.implementation,
        submission.quality_score,
        plugin_id,
        publisher,
        sdk_version,
        keystore,
    );
    registry
        .install(
            monitor,
            admin_token,
            manifest,
            available_depth,
            true,
            now,
            &keystore.verifying_key(),
        )
        .map_err(SdkError::from)
}
