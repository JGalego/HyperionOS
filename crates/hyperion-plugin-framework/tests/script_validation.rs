//! What `validate_native_binary` really checks about a capability's `script`, and why the
//! symlink case is the one that matters.
//!
//! `program` and `script` are not symmetric. `program` must be executable, so it can never name an
//! ordinary sensitive file. `script` has no such requirement -- it is handed to an interpreter to
//! read -- which makes it the weaker of the two checks and the one worth pinning.

use std::path::PathBuf;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, CapabilityManifest, Contribution, ImplementationKind,
    NativeBinaryDescriptor, Operation, PluginError, PluginManifest, PluginRegistry, PrivacyTier,
    SemanticContract, SideEffect, TrustDepth,
};

fn install_with_script(script: Option<PathBuf>, program: PathBuf) -> Result<(), PluginError> {
    let registry = PluginRegistry::new();
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let dir = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&dir.path().join("device.key")).unwrap();

    let mut manifest = PluginManifest {
        plugin_id: 1,
        publisher: "test".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::Capability(CapabilityManifest {
            capability_id: "test.scripted".to_string(),
            contract: SemanticContract {
                inputs: vec![],
                outputs: vec![],
                side_effects: vec![SideEffect::None],
            },
            implementation_kind: ImplementationKind::NativeBinary,
            quality_score: 1.0,
            version: 1,
            native_binary: Some(NativeBinaryDescriptor {
                program,
                args: vec![],
                script,
            }),
            privacy_tier: PrivacyTier::Local,
            resource_profile: None,
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: "test.scripted".to_string(),
            justification: "test".to_string(),
        }],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));
    registry
        .install(
            &mut monitor,
            &root,
            manifest,
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .map(|_| ())
}

#[test]
fn a_real_script_file_really_installs() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("script");
    std::fs::write(&script, "print('hi')\n").unwrap();
    assert!(install_with_script(Some(script), std::env::current_exe().unwrap()).is_ok());
}

#[test]
fn a_script_that_does_not_exist_is_refused() {
    let result = install_with_script(
        Some(PathBuf::from("/definitely/not/here")),
        std::env::current_exe().unwrap(),
    );
    assert!(matches!(result, Err(PluginError::InvalidNativeBinary(_))));
}

#[test]
fn a_script_that_is_a_directory_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let result = install_with_script(
        Some(dir.path().to_path_buf()),
        std::env::current_exe().unwrap(),
    );
    assert!(matches!(result, Err(PluginError::InvalidNativeBinary(_))));
}

#[cfg(unix)]
#[test]
fn a_script_that_is_a_symlink_is_refused_even_when_its_target_is_a_real_file() {
    // The case the check is really for. `std::fs::metadata` follows symlinks, so a link to a file
    // the sandbox has no business reading would have passed `is_file()` -- and Landlock would then
    // have granted `ReadFile` on whatever it really points at, not on the link. Unlike `program`,
    // a `script` needs no execute bit, so this is the one way a manifest could name an ordinary
    // sensitive file. `symlink_metadata` is what closes it.
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::write(&real, "secret\n").unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    // The target really is a regular file -- so this is refused for being a link, not for the
    // target being unsuitable.
    assert!(std::fs::metadata(&link).unwrap().is_file());
    let result = install_with_script(Some(link), std::env::current_exe().unwrap());
    assert!(
        matches!(result, Err(PluginError::InvalidNativeBinary(_))),
        "a symlinked script must be refused"
    );
}

#[test]
fn a_capability_with_no_script_at_all_is_unaffected() {
    // Every hand-installed native binary: self-contained, no separate file to read.
    assert!(install_with_script(None, std::env::current_exe().unwrap()).is_ok());
}
