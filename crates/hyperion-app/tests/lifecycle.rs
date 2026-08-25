//! An app's whole life against the real Plugin Registry: built, signed, installed, listed,
//! described, and really removed.
//!
//! ## What this file does and does not prove
//!
//! Everything here is real: a real `PluginRegistry`, a real Ed25519-signed manifest, a real
//! `hyperion_sdk::publish`, real capability tokens really minted and really revoked. What it does
//! *not* do is run the app, because a sandboxed run needs Landlock and this workspace's existing
//! `hyperion-plugin-framework::tests::native_binary_execution` already fails on any kernel without
//! it -- including the one these tests were written on. The final assertion below is the closest
//! honest substitute: that the installed implementation really names the script as a file the
//! sandbox will grant read access to, which is the exact link that decides whether the run can
//! work at all.

use std::path::PathBuf;
use std::sync::Arc;

use hyperion_app::{
    AppDefinition, AppError, AppPaths, AppRegistry, AppTier, InputField, InputKind,
};
use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{
    sign, CapabilityGrantRequest, Contribution, ExecutionEngineContribution,
    NativeBinaryDescriptor, Operation, PluginManifest, PluginRegistry, TrustDepth,
};
use serde_json::json;

const ENGINE: &str = "test-script-engine";

struct Fixture {
    plugins: Arc<PluginRegistry>,
    monitor: CapabilityMonitor,
    admin: CapabilityToken,
    keystore: Keystore,
    apps: AppRegistry,
    _home: tempfile::TempDir,
}

/// A real, already-existing, already-executable file to stand in for an interpreter -- the same
/// honest stand-in `hyperion-plugin-framework`'s own `execution_engine` tests use for a launcher
/// they register but never spawn.
fn real_launcher() -> NativeBinaryDescriptor {
    NativeBinaryDescriptor {
        program: std::env::current_exe().unwrap(),
        args: vec!["--engine-mode".to_string()],
        script: None,
    }
}

fn fixture() -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let keystore = Keystore::open_or_create(&home.path().join("device.key")).unwrap();
    let plugins = Arc::new(PluginRegistry::new());
    let mut monitor = CapabilityMonitor::new();
    let admin = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);

    let mut manifest = PluginManifest {
        plugin_id: 7,
        publisher: "test-engines".to_string(),
        signature: None,
        sdk_version: 1,
        contributions: vec![Contribution::ExecutionEngine(ExecutionEngineContribution {
            engine_id: ENGINE.to_string(),
            launcher: real_launcher(),
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
        _home: home,
    }
}

fn tally_definition() -> AppDefinition {
    AppDefinition {
        owner: "alice".to_string(),
        keeps_data: false,
        name: "invoice-tally".to_string(),
        goal: "Add up this month's invoices".to_string(),
        engine_id: ENGINE.to_string(),
        script: "read the invoices, print the total\n".to_string(),
        inputs: vec![InputField {
            name: "month".to_string(),
            kind: InputKind::Text,
            description: "which month to add up".to_string(),
            required: true,
        }],
    }
}

impl Fixture {
    fn build(
        &mut self,
        definition: &AppDefinition,
    ) -> Result<hyperion_app::InstalledApp, AppError> {
        self.apps.build(
            &mut self.monitor,
            &self.admin,
            &self.keystore,
            definition,
            true,
            2_000,
        )
    }
}

#[test]
fn a_built_app_is_really_installed_and_reads_back_from_the_signed_registry() {
    let mut fixture = fixture();
    let built = fixture.build(&tally_definition()).expect("must build");

    assert_eq!(built.name, "invoice-tally");
    assert_eq!(built.goal, "Add up this month's invoices");
    assert_eq!(built.tier, AppTier::Tool);
    assert_eq!(built.capability_id, "app.invoice-tally");

    // Not read back from anything this crate remembered -- read back out of the real registry
    // entry that `hyperion_sdk::publish` really installed.
    let entry = fixture
        .plugins
        .query("app.invoice-tally")
        .expect("the capability must really be installed");
    let decoded = hyperion_app::contract::decode(&entry.contract.inputs)
        .expect("the signed manifest must carry the typed contract");
    assert_eq!(decoded.goal, "Add up this month's invoices");
    assert_eq!(decoded.fields.len(), 1);
    assert_eq!(decoded.fields[0].name, "month");

    assert_eq!(fixture.apps.list(), vec![built.clone()]);
    assert_eq!(fixture.apps.describe("invoice-tally"), Some(built));
}

#[test]
fn an_app_that_declares_no_inputs_is_an_answer_rather_than_a_tool() {
    let mut fixture = fixture();
    let mut definition = tally_definition();
    definition.name = "todays-total".to_string();
    definition.inputs = vec![];

    let built = fixture.build(&definition).expect("must build");
    assert_eq!(built.tier, AppTier::Answer);
}

#[test]
fn the_installed_implementation_really_names_the_script_the_sandbox_must_read() {
    let mut fixture = fixture();
    fixture.build(&tally_definition()).expect("must build");

    let entry = fixture.plugins.query("app.invoice-tally").unwrap();
    let native = entry.implementations[0]
        .native_binary
        .as_ref()
        .expect("an app is always backed by a real native-binary descriptor");

    let expected_script: PathBuf = fixture._home.path().join("apps/invoice-tally/script");
    // The launcher really runs, and the script is really the thing it is told to run...
    assert_eq!(native.program, real_launcher().program);
    assert!(native
        .args
        .contains(&expected_script.to_string_lossy().into_owned()));
    // ...and, crucially, it is really declared as a file the sandbox will grant read access to.
    // Without this the launcher would be handed a path it is structurally unable to open, since
    // Landlock grants only the program's own path and the per-invocation temp directory.
    assert_eq!(native.script.as_ref(), Some(&expected_script));
    assert!(
        expected_script.exists(),
        "the script must really be on disk"
    );
}

#[test]
fn arguments_are_checked_against_the_signed_contract_before_anything_runs() {
    let mut fixture = fixture();
    fixture.build(&tally_definition()).expect("must build");

    let prepared = fixture
        .apps
        .prepare_args("invoice-tally", &json!({"month": "may"}))
        .expect("a declared argument must be accepted");
    assert_eq!(prepared, json!({"month": "may"}));

    assert!(matches!(
        fixture.apps.prepare_args("invoice-tally", &json!({})),
        Err(AppError::Args(_))
    ));
    assert!(matches!(
        fixture.apps.prepare_args("no-such-app", &json!({})),
        Err(AppError::NoSuchApp(_))
    ));
}

#[test]
fn an_app_naming_an_engine_that_was_never_installed_leaves_nothing_behind() {
    let mut fixture = fixture();
    let mut definition = tally_definition();
    definition.engine_id = "no-such-engine".to_string();

    assert!(matches!(
        fixture.build(&definition),
        Err(AppError::UnknownEngine(_))
    ));
    // Refused before the script was written: a file claiming an app exists, when nothing could
    // ever dispatch it, is worse than no file.
    assert!(!fixture._home.path().join("apps/invoice-tally").exists());
    assert!(fixture.apps.list().is_empty());
}

#[test]
fn building_over_an_existing_app_is_refused_rather_than_silently_replacing_it() {
    let mut fixture = fixture();
    fixture.build(&tally_definition()).expect("must build");

    let mut second = tally_definition();
    second.goal = "Something completely different".to_string();
    assert!(matches!(
        fixture.build(&second),
        Err(AppError::AlreadyExists(_))
    ));

    // The original really survived, unchanged.
    assert_eq!(
        fixture.apps.describe("invoice-tally").unwrap().goal,
        "Add up this month's invoices"
    );
}

#[test]
fn removing_an_app_really_revokes_its_tokens_and_really_deletes_its_script() {
    let mut fixture = fixture();
    fixture.build(&tally_definition()).expect("must build");

    let entry = fixture.plugins.query("app.invoice-tally").unwrap();
    let plugin_id = entry.owning_plugins[0];
    let tokens = fixture
        .plugins
        .tokens_of(plugin_id)
        .expect("an installed app really holds minted tokens");
    assert!(!tokens.is_empty());
    for token in &tokens {
        assert!(fixture
            .monitor
            .check_rights_ok_result(token, RightsMask::EXEC)
            .is_ok());
    }

    fixture
        .apps
        .remove(
            &mut fixture.monitor,
            &fixture.admin,
            "invoice-tally",
            "alice",
        )
        .expect("must remove");

    // Gone from the registry, gone from the listing, gone from disk...
    assert!(fixture.plugins.query("app.invoice-tally").is_none());
    assert!(fixture.apps.list().is_empty());
    assert!(fixture.apps.describe("invoice-tally").is_none());
    assert!(!fixture._home.path().join("apps/invoice-tally").exists());

    // ...and the capability tokens it held are really revoked, not merely forgotten.
    for token in &tokens {
        assert!(
            fixture
                .monitor
                .check_rights_ok_result(token, RightsMask::EXEC)
                .is_err(),
            "a removed app's token must no longer authorize anything"
        );
    }
}

#[test]
fn removing_something_that_is_not_an_app_is_refused() {
    let mut fixture = fixture();
    assert!(matches!(
        fixture.apps.remove(
            &mut fixture.monitor,
            &fixture.admin,
            "never-existed",
            "alice"
        ),
        Err(AppError::NoSuchApp(_))
    ));
}

#[test]
fn a_capability_that_is_not_an_app_never_appears_in_the_listing() {
    let mut fixture = fixture();
    fixture.build(&tally_definition()).expect("must build");

    // The execution engine installed by the fixture is a real, installed contribution that is not
    // an app. `/apps` must show apps, not everything the registry happens to hold.
    let names: Vec<String> = fixture.apps.list().into_iter().map(|a| a.name).collect();
    assert_eq!(names, vec!["invoice-tally".to_string()]);
}

#[test]
fn an_app_belongs_to_whoever_built_it_and_says_so_from_the_signed_record() {
    let mut fixture = fixture();
    let built = fixture.build(&tally_definition()).expect("must build");
    assert_eq!(built.owner, "alice");

    // Read back by decoding the signed manifest, not from anything this crate remembered -- an
    // ownership record editable without invalidating a signature would not be one.
    let entry = fixture.plugins.query("app.invoice-tally").unwrap();
    let decoded = hyperion_app::contract::decode(&entry.contract.inputs).unwrap();
    assert_eq!(decoded.owner, "alice");
}

#[test]
fn someone_elses_app_is_not_yours_to_remove() {
    let mut fixture = fixture();
    fixture.build(&tally_definition()).expect("must build");

    let refused = fixture
        .apps
        .remove(&mut fixture.monitor, &fixture.admin, "invoice-tally", "bob");
    assert!(
        matches!(refused, Err(AppError::NotYours { ref owner, .. }) if owner == "alice"),
        "got: {refused:?}"
    );

    // Really refused: still installed, still runnable by everyone.
    assert!(fixture.apps.describe("invoice-tally").is_some());
    assert!(fixture.plugins.query("app.invoice-tally").is_some());

    // And its owner can still remove it.
    assert!(fixture
        .apps
        .remove(
            &mut fixture.monitor,
            &fixture.admin,
            "invoice-tally",
            "alice"
        )
        .is_ok());
}

#[test]
fn an_app_with_no_owner_is_refused_before_anything_is_written() {
    let mut fixture = fixture();
    let mut definition = tally_definition();
    definition.owner = "  ".to_string();

    assert!(matches!(
        fixture.build(&definition),
        Err(AppError::Contract(_))
    ));
    assert!(fixture.apps.list().is_empty());
}

#[test]
fn a_rebuild_replaces_what_an_app_does_and_keeps_who_it_is() {
    let mut fixture = fixture();
    let first = fixture.build(&tally_definition()).expect("must build");
    assert_eq!(first.version, 1);

    let mut second = tally_definition();
    second.goal = "Add up this quarter's invoices instead".to_string();
    second.script = "a different script\n".to_string();
    second.inputs = vec![InputField {
        name: "quarter".to_string(),
        kind: InputKind::Text,
        description: "which quarter to add up".to_string(),
        required: true,
    }];

    let rebuilt = fixture
        .apps
        .rebuild(
            &mut fixture.monitor,
            &fixture.admin,
            &fixture.keystore,
            &second,
            "alice",
            true,
        )
        .expect("its owner may rebuild it");

    // What it does really changed...
    assert_eq!(rebuilt.goal, "Add up this quarter's invoices instead");
    assert_eq!(rebuilt.inputs[0].name, "quarter");
    assert_eq!(rebuilt.version, 2);
    // ...and who it is did not. Its capability id is its identity, and its audit history is keyed
    // by that, so a rebuild that minted a new one would silently orphan everything it had done.
    assert_eq!(rebuilt.capability_id, first.capability_id);
    assert_eq!(rebuilt.owner, "alice");
    assert_eq!(
        fixture.apps.list().len(),
        1,
        "a rebuild is not a second app"
    );
}

#[test]
fn a_rebuild_never_changes_who_an_app_belongs_to() {
    let mut fixture = fixture();
    fixture.build(&tally_definition()).expect("must build");

    // Even asked to, by a definition claiming someone else: a rebuild changes what an app does,
    // never who owns it.
    let mut hijack = tally_definition();
    hijack.owner = "bob".to_string();
    let rebuilt = fixture
        .apps
        .rebuild(
            &mut fixture.monitor,
            &fixture.admin,
            &fixture.keystore,
            &hijack,
            "alice",
            true,
        )
        .expect("alice may rebuild her own app");
    assert_eq!(rebuilt.owner, "alice");
}

#[test]
fn someone_elses_app_is_not_yours_to_rebuild_either() {
    let mut fixture = fixture();
    fixture.build(&tally_definition()).expect("must build");

    let mut theirs = tally_definition();
    theirs.goal = "Something else entirely".to_string();
    let refused = fixture.apps.rebuild(
        &mut fixture.monitor,
        &fixture.admin,
        &fixture.keystore,
        &theirs,
        "bob",
        true,
    );
    assert!(
        matches!(refused, Err(AppError::NotYours { ref owner, .. }) if owner == "alice"),
        "got: {refused:?}"
    );
    // Untouched.
    assert_eq!(
        fixture.apps.describe("invoice-tally").unwrap().goal,
        "Add up this month's invoices"
    );
}

#[test]
fn rebuilding_something_that_was_never_built_is_refused() {
    let mut fixture = fixture();
    let mut orphan = tally_definition();
    orphan.name = "never-existed".to_string();
    assert!(matches!(
        fixture.apps.rebuild(
            &mut fixture.monitor,
            &fixture.admin,
            &fixture.keystore,
            &orphan,
            "alice",
            true,
        ),
        Err(AppError::NoSuchApp(_))
    ));
}
