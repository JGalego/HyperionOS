//! Building, listing, describing and removing apps -- all of it over the *existing* Plugin
//! Registry, and none of it a second mechanism.
//!
//! An app is not a new kind of thing. It is a `Contribution::Capability` with
//! `ImplementationKind::NativeBinary`, published through `hyperion_sdk::publish`, installed
//! through `PluginRegistry::install`, and run through `PluginRegistry::invoke_native_binary`
//! inside a real `hyperion_trust_boundary::spawn` sandbox -- the same four steps a hand-installed
//! native binary already takes. What this crate adds is the typed contract that rides inside that
//! manifest ([`crate::contract`]) and the shell around it that turns "a goal and a script" into
//! those steps.
//!
//! ## Why there is no `run` here
//!
//! Deliberately absent. Dispatching an app means `AgentRuntime::invoke`, which is where real
//! consent gating and automatic Explanation Records live -- a `run` in this crate could only
//! reach `invoke_native_binary` directly, and every caller that took it would silently lose both.
//! [`AppRegistry::prepare_args`] is the half that belongs here: validate against the signed
//! contract, hand back the exact `input.json` the app will receive, and let the caller dispatch it
//! through the one path that explains itself.

use std::path::PathBuf;
use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_crypto::Keystore;
use hyperion_plugin_framework::{Operation, PluginRegistry, SideEffect, TrustDepth};
use hyperion_sdk::{
    Contract, Implementation, LatencyClass, PermissionRequest, Runtime, TrustLevel,
};

use crate::contract::{self, AppContract, ArgError, ContractError, APP_CAPABILITY_PREFIX};
use crate::types::{AppDefinition, AppPaths, InstalledApp};

/// The publisher name every locally built app is signed under. Real, and honest about what it is:
/// these are signed by *this device's own* keystore, not by a third-party publisher, and
/// `hyperion-crypto` is explicit that it holds one device identity rather than a trust store of
/// many.
pub const APP_PUBLISHER: &str = "hyperion-app";

/// The SDK manifest version these submissions are built against.
const SDK_VERSION: u32 = 1;

/// What a locally built app declares as its quality score.
///
/// The Model Router scores *competing* implementations of one capability id against each other.
/// An app is the only implementation of its own id, so this never actually ranks anything -- which
/// is exactly why it is a flat constant rather than a number invented to look measured. A real
/// benchmark harness would be the thing that earns a varying score here.
const APP_QUALITY_SCORE: f32 = 1.0;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Contract(#[from] ContractError),
    #[error("{0}")]
    Args(#[from] ArgError),
    #[error(
        "there's no way to run a \"{0}\" script here yet -- nothing has taught Hyperion that \
         kind of script"
    )]
    UnknownEngine(String),
    #[error("there's already an app called \"{0}\"")]
    AlreadyExists(String),
    #[error("there's no app called \"{0}\"")]
    NoSuchApp(String),
    #[error("I couldn't save the app's script: {0}")]
    Io(String),
    #[error("I couldn't install the app: {0}")]
    Install(String),
    #[error("I couldn't remove the app: {0}")]
    Remove(String),
}

/// The apps this device has built, backed entirely by the real Plugin Registry.
pub struct AppRegistry {
    plugins: Arc<PluginRegistry>,
    paths: AppPaths,
}

impl AppRegistry {
    pub fn new(plugins: Arc<PluginRegistry>, paths: AppPaths) -> Self {
        AppRegistry { plugins, paths }
    }

    /// The capability id an app of this name installs as.
    pub fn capability_id_for(name: &str) -> String {
        format!("{APP_CAPABILITY_PREFIX}{name}")
    }

    /// A stable plugin id derived from the app's own name, via the same real BLAKE3 hash
    /// `hyperion-sdk` already fingerprints submissions with.
    ///
    /// Deliberately deterministic rather than clock-derived: two apps built in the same
    /// millisecond can never collide on it, and an app rebuilt after removal reclaims its own id
    /// instead of leaving the previous one stranded. Truncated to this field's `u64` width the
    /// same way (and for the same reason) `hyperion_sdk::package_hash` truncates its own -- an
    /// identifier, never treated as a cryptographic commitment.
    fn plugin_id_for(name: &str) -> u64 {
        let hash = hyperion_crypto::hash(format!("hyperion-app/{name}").as_bytes());
        u64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap())
    }

    /// Builds, installs and signs an app, and returns it as the registry now really holds it.
    ///
    /// Every check that can fail happens before anything is written or installed, so a rejected
    /// definition leaves no directory, no manifest, and no half-installed capability behind.
    pub fn build(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        keystore: &Keystore,
        definition: &AppDefinition,
        now: u64,
    ) -> Result<InstalledApp, AppError> {
        let app_contract = AppContract {
            name: definition.name.clone(),
            goal: definition.goal.clone(),
            fields: definition.inputs.clone(),
        };
        contract::validate_contract(&app_contract)?;

        // Checked before the script is written, not after: an engine that was never installed
        // means this app could never run, and leaving its script on disk would be a file that
        // claims an app exists when nothing can dispatch it.
        if self
            .plugins
            .execution_engine(&definition.engine_id)
            .is_none()
        {
            return Err(AppError::UnknownEngine(definition.engine_id.clone()));
        }

        let capability_id = Self::capability_id_for(&definition.name);
        if self.plugins.query(&capability_id).is_some() {
            return Err(AppError::AlreadyExists(definition.name.clone()));
        }

        let script_path = self.write_script(&definition.name, &definition.script)?;

        let native_binary = hyperion_sdk::resolve_via_engine(
            &self.plugins,
            &definition.engine_id,
            script_path,
            Vec::new(),
        )
        .map_err(|e| AppError::Install(e.to_string()))?;

        let sdk_contract = Contract {
            id: capability_id.clone(),
            version: 1,
            summary: definition.goal.clone(),
            inputs: contract::encode(&app_contract),
            outputs: vec!["result".to_string()],
            // An M1 app really has no side effects to declare: the sandbox grants it one
            // throwaway directory and a seccomp filter with no network syscalls in it, so it
            // cannot create a durable object or reach the network even if it tried. When T2
            // (durable state) and brokered egress land, this is the field that stops being
            // `None` -- and the review gate will start requiring a real justification for the
            // permissions that come with them.
            side_effects: vec![SideEffect::None],
            permissions_requested: vec![PermissionRequest {
                operation: Operation::Execute,
                scope: capability_id.clone(),
                justification: definition.goal.clone(),
            }],
            // The strongest sandbox this workspace really implements: `TrustLevel::Elevated`
            // maps to `TrustDepth::D2`, which `hyperion-plugin-framework::real_trust_depth` maps
            // to `hyperion_trust_boundary::TrustDepth::Container` -- user namespaces, Landlock,
            // and seccomp. The naming reads backwards at first glance: a *deeper* trust depth is
            // a *more* confined process, not a more privileged one.
            trust_level: TrustLevel::Elevated,
        };

        let implementation = Implementation {
            contract_id: capability_id.clone(),
            name: definition.name.clone(),
            runtime: Runtime::NativeBinary,
            latency_class: LatencyClass::Interactive,
            requires_consent: false,
            native_binary: Some(native_binary),
            resource_profile: None,
        };

        // `Execute` really is what this implementation does, and it is exactly what the contract
        // declares -- so the SDK's own static over-request check passes honestly rather than by
        // declaring nothing.
        let submission = hyperion_sdk::prepare_submission(
            sdk_contract,
            implementation,
            APP_QUALITY_SCORE,
            vec![Operation::Execute],
        )
        .map_err(|e| AppError::Install(e.to_string()))?;

        hyperion_sdk::publish(
            monitor,
            admin_token,
            &self.plugins,
            submission,
            Self::plugin_id_for(&definition.name),
            APP_PUBLISHER,
            SDK_VERSION,
            // No human approval is asked for, and none is bypassed: a submission requesting only
            // `Execute` is `AutoApproved` by the SDK's own review gate, because `Write` and
            // `NetworkEgress` are the operations it treats as sensitive. An app that requests
            // either will reach `PendingHumanReview` here and really need a decision.
            false,
            TrustDepth::D2,
            now,
            keystore,
        )
        .map_err(|e| AppError::Install(e.to_string()))?;

        self.describe(&definition.name)
            .ok_or_else(|| AppError::NoSuchApp(definition.name.clone()))
    }

    /// Every app currently installed, in a stable order.
    ///
    /// Read entirely from the signed registry: an app appears here because its manifest is really
    /// installed and its contract really decodes, never because a side file remembered it. A
    /// capability whose inputs are not an app contract is simply skipped.
    pub fn list(&self) -> Vec<InstalledApp> {
        self.plugins
            .capability_entries()
            .into_iter()
            .filter_map(|entry| {
                let decoded = contract::decode(&entry.contract.inputs)?;
                let version = entry
                    .implementations
                    .iter()
                    .map(|i| i.version)
                    .max()
                    .unwrap_or(1);
                Some(InstalledApp {
                    tier: decoded.tier(),
                    name: decoded.name,
                    goal: decoded.goal,
                    inputs: decoded.fields,
                    capability_id: entry.capability_id,
                    version,
                })
            })
            .collect()
    }

    /// One app, by the name a person would use for it.
    pub fn describe(&self, name: &str) -> Option<InstalledApp> {
        let entry = self.plugins.query(&Self::capability_id_for(name))?;
        let decoded = contract::decode(&entry.contract.inputs)?;
        let version = entry
            .implementations
            .iter()
            .map(|i| i.version)
            .max()
            .unwrap_or(1);
        Some(InstalledApp {
            tier: decoded.tier(),
            name: decoded.name,
            goal: decoded.goal,
            inputs: decoded.fields,
            capability_id: entry.capability_id,
            version,
        })
    }

    /// Validates `args` against the app's own signed contract and returns the exact `input.json`
    /// it will receive.
    ///
    /// The caller dispatches the result through `AgentRuntime::invoke` -- see this module's own
    /// doc comment for why the dispatch deliberately does not happen here.
    pub fn prepare_args(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        let app = self
            .describe(name)
            .ok_or_else(|| AppError::NoSuchApp(name.to_string()))?;
        Ok(contract::validate_args(name, &app.inputs, args)?)
    }

    /// Removes an app: really revokes its capability tokens (a cascade `cap_revoke`, via
    /// `PluginRegistry::uninstall`) and really deletes its script.
    ///
    /// Both halves matter. Uninstalling without deleting the script leaves a file that outlives
    /// the app it belonged to; deleting the script without uninstalling leaves an installed
    /// capability pointing at nothing. "Removed" should mean removed.
    pub fn remove(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        name: &str,
    ) -> Result<(), AppError> {
        contract::validate_app_name(name)?;
        let entry = self
            .plugins
            .query(&Self::capability_id_for(name))
            .ok_or_else(|| AppError::NoSuchApp(name.to_string()))?;
        if contract::decode(&entry.contract.inputs).is_none() {
            // A capability under `app.<name>` that is not an app contract was installed by
            // something else. Removing it here would be this crate reaching outside what it owns.
            return Err(AppError::NoSuchApp(name.to_string()));
        }

        for plugin_id in entry.owning_plugins {
            self.plugins
                .uninstall(monitor, admin_token, plugin_id)
                .map_err(|e| AppError::Remove(e.to_string()))?;
        }

        let dir = self.paths.dir_for(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| AppError::Remove(e.to_string()))?;
        }
        Ok(())
    }

    fn write_script(&self, name: &str, script: &str) -> Result<PathBuf, AppError> {
        let dir = self.paths.dir_for(name);
        std::fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
        let script_path = self.paths.script_for(name);
        std::fs::write(&script_path, script).map_err(|e| AppError::Io(e.to_string()))?;
        Ok(script_path)
    }
}
