//! App Builder T2: an app that keeps something between runs, and the permission that takes.
//!
//! What is proven here is everything up to the sandbox: that statefulness is declared, signed,
//! consented to, and erased. The one thing that cannot be proven in this container is the run
//! itself -- it has no Landlock, so no sandboxed process starts at all (the same reason
//! `hyperion-plugin-framework`'s own `native_binary_execution` test fails here).

use std::sync::Arc;

use hyperion_app::{AppDefinition, AppError, AppPaths, AppRegistry, InputField, InputKind};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, Contribution, ExecutionEngineContribution,
    NativeBinaryDescriptor, Operation, PluginManifest, PluginRegistry, SideEffect, TrustDepth,
};

const ENGINE: &str = "test-script-engine";

struct Fixture {
    plugins: Arc<PluginRegistry>,
    monitor: CapabilityMonitor,
    admin: CapabilityToken,
    keystore: Keystore,
    apps: AppRegistry,
    home: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&home.path().join("device.key")).unwrap();
    let plugins = Arc::new(PluginRegistry::new_with_app_data(
        home.path().join("apps").join("data"),
    ));
    let mut monitor = CapabilityMonitor::new();
    let admin = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let mut manifest = PluginManifest {
        plugin_id: 7,
        publisher: "test-engines".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::ExecutionEngine(ExecutionEngineContribution {
            engine_id: ENGINE.to_string(),
            launcher: NativeBinaryDescriptor {
                program: std::env::current_exe().unwrap(),
                args: vec![],
                script: None,
            },
        })],
        requested_permissions: vec![CapabilityGrantRequest {
            operation: Operation::Execute,
            scope: ENGINE.to_string(),
            justification: "the engine's own launcher must be dispatchable".to_string(),
        }],
        min_trust_depth: TrustDepth::D0,
    };
    manifest.signature = Some(sign(&manifest, &keystore));
    plugins
        .install(
            &mut monitor,
            &admin,
            manifest,
            TrustDepth::D0,
            true,
            1_000,
            &keystore.verifying_key(),
        )
        .unwrap();

    let apps = AppRegistry::new(
        Arc::clone(&plugins),
        AppPaths::new(home.path().join("apps")),
    );
    Fixture {
        plugins,
        monitor,
        admin,
        keystore,
        apps,
        home,
    }
}

fn definition(keeps_data: bool) -> AppDefinition {
    AppDefinition {
        name: "shopping-list".to_string(),
        goal: "Keep my shopping list".to_string(),
        owner: "alice".to_string(),
        keeps_data,
        engine_id: ENGINE.to_string(),
        script: "remember the list\n".to_string(),
        inputs: vec![InputField {
            name: "item".to_string(),
            kind: InputKind::Text,
            description: "what to add".to_string(),
            required: true,
        }],
    }
}

impl Fixture {
    fn build(
        &mut self,
        definition: &AppDefinition,
        consented: bool,
    ) -> Result<hyperion_app::InstalledApp, AppError> {
        self.apps.build(
            &mut self.monitor,
            &self.admin,
            &self.keystore,
            definition,
            consented,
            2_000,
        )
    }
}

#[test]
fn keeping_data_is_a_permission_a_person_has_to_grant() {
    let mut fixture = fixture();
    let refused = fixture.build(&definition(true), false);
    assert!(
        matches!(refused, Err(AppError::NeedsStorageConsent { .. })),
        "got: {refused:?}"
    );
    // Refused before anything was written, so an unapproved app leaves nothing behind.
    assert!(fixture.apps.list().is_empty());
    assert!(!fixture.home.path().join("apps/shopping-list").exists());

    assert!(fixture.build(&definition(true), true).is_ok());
}

#[test]
fn a_stateless_app_is_never_asked_about_storage() {
    // The question has to be about a real difference: an app that keeps nothing must install
    // without anyone being asked to approve anything.
    let mut fixture = fixture();
    assert!(fixture.build(&definition(false), false).is_ok());
    assert!(!fixture.apps.describe("shopping-list").unwrap().keeps_data);
}

#[test]
fn what_an_app_may_keep_is_recorded_in_the_signed_contract() {
    let mut fixture = fixture();
    fixture.build(&definition(true), true).unwrap();

    let entry = fixture.plugins.query("app.shopping-list").unwrap();
    let decoded = hyperion_app::contract::decode(&entry.contract.inputs).unwrap();
    assert!(
        decoded.keeps_data,
        "statefulness must survive the round trip"
    );

    // And it is the declaration the sandbox actually reads: `CreatesSemanticObject` is what
    // `PluginRegistry` checks before granting durable storage, and what the review gate requires
    // before allowing the `Write` permission.
    assert!(entry
        .contract
        .side_effects
        .contains(&SideEffect::CreatesSemanticObject));
}

#[test]
fn a_stateless_app_declares_no_side_effect_and_asks_for_no_write() {
    let mut fixture = fixture();
    fixture.build(&definition(false), false).unwrap();

    let entry = fixture.plugins.query("app.shopping-list").unwrap();
    assert!(entry.contract.side_effects.contains(&SideEffect::None));
    assert!(!entry
        .contract
        .side_effects
        .contains(&SideEffect::CreatesSemanticObject));
}

#[test]
fn removing_a_stateful_app_really_deletes_what_it_kept() {
    // docs/16's erasure promise: removing something removes it. Data left behind would also be
    // inherited by a later app installed under the same name.
    let mut fixture = fixture();
    fixture.build(&definition(true), true).unwrap();

    // Stand in for what a real sandboxed run would have written, at the exact path
    // `PluginRegistry::data_scope_for` composes.
    let encoded: String = "app.shopping-list"
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("");
    let alices = fixture.home.path().join("apps/data/1000").join(&encoded);
    let bobs = fixture.home.path().join("apps/data/1001").join(&encoded);
    for dir in [&alices, &bobs] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("list.txt"), "milk\n").unwrap();
    }

    fixture
        .apps
        .remove(
            &mut fixture.monitor,
            &fixture.admin,
            "shopping-list",
            "alice",
        )
        .unwrap();

    // Everyone's, not just the remover's -- the app is gone, so nobody's data for it should remain.
    assert!(!alices.exists(), "alice's data must be gone");
    assert!(!bobs.exists(), "bob's data must be gone too");
}

#[test]
fn a_rebuild_keeps_what_the_app_has_already_stored() {
    // The difference between rebuilding and removing: a rebuild changes what an app does, so
    // throwing away what it had remembered would make every fix cost the person their data.
    let mut fixture = fixture();
    fixture.build(&definition(true), true).unwrap();

    let encoded: String = "app.shopping-list"
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("");
    let data = fixture.home.path().join("apps/data/1000").join(&encoded);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("list.txt"), "milk\n").unwrap();

    let mut changed = definition(true);
    changed.goal = "Keep my shopping list, with quantities".to_string();
    fixture
        .apps
        .rebuild(
            &mut fixture.monitor,
            &fixture.admin,
            &fixture.keystore,
            &changed,
            "alice",
            true,
        )
        .expect("its owner may rebuild it");

    assert!(
        data.join("list.txt").exists(),
        "a rebuild must not erase data"
    );
    assert_eq!(
        std::fs::read_to_string(data.join("list.txt")).unwrap(),
        "milk\n"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn the_registry_and_the_eraser_agree_on_where_data_lives() {
    // The one place these two could silently drift: `PluginRegistry::data_scope_for` composes the
    // path an app writes to, and `AppRegistry::erase_data_for` composes the path removal deletes.
    // They are deliberately separate (the registry knows nothing about apps), so this proves they
    // still name the same directory rather than trusting a comment that says they do.
    //
    // The invocation itself fails -- this container has no Landlock, so no sandboxed process ever
    // starts -- but the directory is prepared before the spawn is attempted, which is exactly the
    // path under test.
    let mut fixture = fixture();
    fixture.build(&definition(true), true).unwrap();

    let alice = TrustBoundaryId(1_000);
    let _ = fixture.plugins.invoke_native_binary(
        "app.shopping-list",
        serde_json::json!({ "item": "milk" }),
        alice,
    );

    // Whatever the registry created, removal must reach.
    let data_root = fixture.home.path().join("apps/data");
    let created: Vec<std::path::PathBuf> = std::fs::read_dir(data_root.join("1000"))
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    assert_eq!(
        created.len(),
        1,
        "the registry should have prepared exactly one directory for this app, got: {created:?}"
    );

    fixture
        .apps
        .remove(
            &mut fixture.monitor,
            &fixture.admin,
            "shopping-list",
            "alice",
        )
        .unwrap();
    assert!(
        !created[0].exists(),
        "removal must delete the very directory the registry created, {created:?}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn a_stateless_app_is_never_given_a_directory_at_all() {
    // Least privilege: an app that never asked to keep anything must not be handed somewhere it
    // could quietly start keeping things.
    let mut fixture = fixture();
    fixture.build(&definition(false), false).unwrap();

    let _ = fixture.plugins.invoke_native_binary(
        "app.shopping-list",
        serde_json::json!({ "item": "milk" }),
        TrustBoundaryId(1_000),
    );

    let data_root = fixture.home.path().join("apps/data");
    assert!(
        std::fs::read_dir(data_root.join("1000")).is_err(),
        "a stateless app must be given no durable directory"
    );
}
