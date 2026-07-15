use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_crypto::VerifyingKey;

use crate::review::validate_manifest;
use crate::types::{
    AgentContribution, CapabilityId, CapabilityManifest, Contribution, HardwareSupportContribution,
    ImplementationDescriptor, ImplementationKind, InstallState, PluginError, PluginHandle,
    PluginId, PluginManifest, QuarantineReason, RegistryEntry, TrustDepth,
};

fn rights_for(op: crate::types::Operation) -> RightsMask {
    use crate::types::Operation;
    match op {
        Operation::Read => RightsMask::READ,
        Operation::Write | Operation::NetworkEgress => RightsMask::WRITE,
        Operation::Execute => RightsMask::EXEC,
    }
}

/// Real Linux sandboxing only, and only depths this workspace's real enforcement can actually
/// provide -- `hyperion_trust_boundary::TrustDepth` has no VM-equivalent depth 3 (see that
/// crate's own doc comment on why), so this policy label's own D2/D3 both map to the strongest
/// real depth that exists, `Container` (namespaces + Landlock + seccomp), rather than pretending
/// a stronger isolation this workspace doesn't implement.
#[cfg(target_os = "linux")]
fn real_trust_depth(policy: TrustDepth) -> hyperion_trust_boundary::TrustDepth {
    match policy {
        TrustDepth::D0 | TrustDepth::D1 => hyperion_trust_boundary::TrustDepth::Process,
        TrustDepth::D2 | TrustDepth::D3 => hyperion_trust_boundary::TrustDepth::Container,
    }
}

/// A real sandboxed `NativeBinary` invocation's own bounded patience -- mirrors
/// `hyperion-ai-runtime::openai_compat_backend::GENERATE_TIMEOUT`'s same 120s reasoning: a real,
/// potentially slow tool shouldn't be cut off after a network-call-sized timeout, but a genuinely
/// hung one must not block a capability dispatch forever either.
#[cfg(target_os = "linux")]
const NATIVE_BINARY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
#[cfg(target_os = "linux")]
const NATIVE_BINARY_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

/// docs/24 — Plugin Framework. See this crate's doc comment for the full
/// real/deferred split.
pub struct PluginRegistry {
    plugins: Mutex<HashMap<PluginId, PluginManifest>>,
    boundaries: Mutex<HashMap<PluginId, TrustBoundaryId>>,
    tokens: Mutex<HashMap<PluginId, Vec<CapabilityToken>>>,
    /// A real, purpose-built `READ | WRITE` token minted once at install time (when `monitor` is
    /// really `&mut`, per [`Self::install`]'s own signature) for exactly one job: scoping a
    /// [`hyperion_trust_boundary::SpawnGrant`]'s real fs access to a `NativeBinary` invocation's
    /// own real temp I/O directory. Looked up read-only at invocation time
    /// ([`Self::invoke_native_binary`]) rather than derived fresh there, since that call site only
    /// ever has a shared `&CapabilityMonitor` available (matching
    /// `hyperion-agent-runtime::AgentRuntime::invoke`'s own concurrent-dispatch design, which
    /// deliberately never takes `&mut CapabilityMonitor`).
    sandbox_tokens: Mutex<HashMap<PluginId, CapabilityToken>>,
    registry: Mutex<HashMap<CapabilityId, RegistryEntry>>,
    /// Real registration point for `Contribution::Agent` -- docs/998-roadmap.md's Resourceful
    /// pillar named `hyperion-coordination::catalog::default_manifests`'s hardcoded, static
    /// built-in roster as having no live registry a plugin's own agent specialization could
    /// register into. Keyed by `plugin_id` (not folded into `registry`, which is keyed by
    /// `CapabilityId` and has no analogous concept for an agent specialization) so
    /// [`Self::uninstall`]/[`Self::quarantine`] can remove/hide exactly one plugin's own
    /// contributions without touching any other plugin's.
    agent_contributions: Mutex<HashMap<PluginId, Vec<AgentContribution>>>,
    /// Real registration point for `Contribution::HardwareSupport` -- the "device driver
    /// registry" `hyperion-device` has no equivalent of. Keyed by `plugin_id`, same shape and
    /// same reasoning as [`Self::agent_contributions`].
    hardware_support: Mutex<HashMap<PluginId, Vec<HardwareSupportContribution>>>,
    /// Plugin-level quarantine, tracked separately from `registry`'s own per-`CapabilityId`
    /// `InstallState` -- an `Agent`-only or `HardwareSupport`-only plugin owns no `RegistryEntry`
    /// for that mechanism to touch, so [`Self::quarantine`] needs a real place to hide its
    /// contributions too.
    quarantined_plugins: Mutex<HashSet<PluginId>>,
    next_plugin_id: AtomicU64,
    next_boundary_ordinal: AtomicU64,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    pub fn new() -> Self {
        PluginRegistry {
            plugins: Mutex::new(HashMap::new()),
            boundaries: Mutex::new(HashMap::new()),
            tokens: Mutex::new(HashMap::new()),
            sandbox_tokens: Mutex::new(HashMap::new()),
            registry: Mutex::new(HashMap::new()),
            agent_contributions: Mutex::new(HashMap::new()),
            hardware_support: Mutex::new(HashMap::new()),
            quarantined_plugins: Mutex::new(HashSet::new()),
            next_plugin_id: AtomicU64::new(1),
            next_boundary_ordinal: AtomicU64::new(1),
        }
    }

    /// docs/24 §5's `plugin_install` pseudocode: validate → check the
    /// installing environment can satisfy the manifest's declared minimum
    /// trust depth → consent → mint exactly the requested tokens (never a
    /// superset) under a fresh Trust Boundary → register every
    /// Capability contribution. Any failure before the mint step leaves
    /// no trace — a rejected manifest never partially installs.
    #[allow(clippy::too_many_arguments)]
    pub fn install(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        manifest: PluginManifest,
        available_depth: TrustDepth,
        consented: bool,
        _now: u64,
        verifying_key: &VerifyingKey,
    ) -> Result<PluginHandle, PluginError> {
        monitor
            .check_rights_ok_result(admin_token, RightsMask::GRANT)
            .map_err(|_| PluginError::Unauthorized)?;
        validate_manifest(&manifest, verifying_key)?;

        if manifest.min_trust_depth > available_depth {
            return Err(PluginError::InsufficientTrustDepth);
        }
        if !consented {
            return Err(PluginError::ConsentDeclined);
        }
        // An honest check now, not a trusted claim -- see `NativeBinaryDescriptor`'s own doc
        // comment. Checked before any minting/registration below, so a manifest that fails this
        // never partially installs, matching this function's own existing invariant.
        let needs_sandbox_token = manifest.contributions.iter().any(|c| match c {
            Contribution::Capability(cm) => {
                cm.implementation_kind == ImplementationKind::NativeBinary
            }
            Contribution::Agent(_) | Contribution::HardwareSupport(_) => false,
        });
        for contribution in &manifest.contributions {
            if let Contribution::Capability(cm) = contribution {
                if cm.implementation_kind == ImplementationKind::NativeBinary {
                    validate_native_binary(cm.native_binary.as_ref())?;
                }
            }
        }

        let plugin_id = self.next_plugin_id.fetch_add(1, Ordering::Relaxed);
        let boundary =
            TrustBoundaryId(1_000_000 + self.next_boundary_ordinal.fetch_add(1, Ordering::Relaxed));

        let mut minted = Vec::with_capacity(manifest.requested_permissions.len());
        for request in &manifest.requested_permissions {
            let token =
                monitor.cap_derive(admin_token, rights_for(request.operation), None, boundary)?;
            minted.push(token);
        }
        // Minted once here, while `monitor` is really `&mut` -- see `Self::sandbox_tokens`'s own
        // doc comment for why this can't instead be derived lazily at invocation time.
        if needs_sandbox_token {
            let sandbox_token = monitor.cap_derive(
                admin_token,
                RightsMask::READ | RightsMask::WRITE,
                None,
                boundary,
            )?;
            self.sandbox_tokens
                .lock()
                .unwrap()
                .insert(plugin_id, sandbox_token);
        }

        for contribution in &manifest.contributions {
            match contribution {
                Contribution::Capability(cm) => {
                    self.register_implementation(plugin_id, cm)?;
                }
                Contribution::Agent(ac) => {
                    self.agent_contributions
                        .lock()
                        .unwrap()
                        .entry(plugin_id)
                        .or_default()
                        .push(ac.clone());
                }
                Contribution::HardwareSupport(hs) => {
                    self.hardware_support
                        .lock()
                        .unwrap()
                        .entry(plugin_id)
                        .or_default()
                        .push(hs.clone());
                }
            }
        }

        self.plugins.lock().unwrap().insert(plugin_id, manifest);
        self.boundaries.lock().unwrap().insert(plugin_id, boundary);
        self.tokens.lock().unwrap().insert(plugin_id, minted);

        Ok(PluginHandle {
            plugin_id,
            boundary,
        })
    }

    /// docs/24 §5's structural-compatibility check on `capability_id`
    /// collision: identical contract shape merges into the existing
    /// `RegistryEntry` as one more competing implementation; an
    /// incompatible one is rejected outright rather than silently
    /// shadowing the existing contract (docs/24's `version_variant()`
    /// minting a distinct id is deferred — see this crate's doc comment).
    fn register_implementation(
        &self,
        plugin_id: PluginId,
        cm: &CapabilityManifest,
    ) -> Result<ImplementationDescriptor, PluginError> {
        let descriptor = ImplementationDescriptor {
            plugin_id,
            implementation_kind: cm.implementation_kind,
            quality_score: cm.quality_score,
            version: cm.version,
            native_binary: cm.native_binary.clone(),
        };

        let mut registry = self.registry.lock().unwrap();
        match registry.get_mut(&cm.capability_id) {
            Some(entry) => {
                if entry.contract != cm.contract {
                    return Err(PluginError::CapabilityCollisionIncompatible);
                }
                entry.implementations.push(descriptor.clone());
                entry.owning_plugins.push(plugin_id);
            }
            None => {
                registry.insert(
                    cm.capability_id.clone(),
                    RegistryEntry {
                        capability_id: cm.capability_id.clone(),
                        contract: cm.contract.clone(),
                        implementations: vec![descriptor.clone()],
                        owning_plugins: vec![plugin_id],
                        install_state: InstallState::Active,
                    },
                );
            }
        }
        Ok(descriptor)
    }

    /// docs/24 §5's uninstall: "one graph walk invalidates everything" —
    /// revoking every token this plugin was ever minted, then removing
    /// its contributions from the registry.
    pub fn uninstall(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        plugin_id: PluginId,
    ) -> Result<(), PluginError> {
        monitor
            .check_rights_ok_result(admin_token, RightsMask::REVOKE)
            .map_err(|_| PluginError::Unauthorized)?;

        let tokens = self
            .tokens
            .lock()
            .unwrap()
            .remove(&plugin_id)
            .ok_or(PluginError::NoSuchPlugin)?;
        for token in &tokens {
            monitor.cap_revoke(token);
        }

        let mut registry = self.registry.lock().unwrap();
        for entry in registry.values_mut() {
            entry.implementations.retain(|d| d.plugin_id != plugin_id);
            entry.owning_plugins.retain(|&id| id != plugin_id);
        }
        registry.retain(|_, entry| !entry.implementations.is_empty());
        drop(registry);

        self.plugins.lock().unwrap().remove(&plugin_id);
        self.boundaries.lock().unwrap().remove(&plugin_id);
        self.agent_contributions.lock().unwrap().remove(&plugin_id);
        self.hardware_support.lock().unwrap().remove(&plugin_id);
        self.quarantined_plugins.lock().unwrap().remove(&plugin_id);
        Ok(())
    }

    /// docs/24 §6's `registry_quarantine` — disables the plugin's
    /// registry entries without a full uninstall (its tokens remain
    /// live; a quarantined entry is simply never returned as an eligible
    /// candidate by [`Self::query`]/[`Self::agent_contributions`]). Keyed off
    /// `self.plugins` rather than "did this plugin own a `Capability` registry entry" so an
    /// `Agent`-only plugin (no `RegistryEntry` to touch at all) can be quarantined too.
    pub fn quarantine(
        &self,
        plugin_id: PluginId,
        _reason: QuarantineReason,
    ) -> Result<(), PluginError> {
        if !self.plugins.lock().unwrap().contains_key(&plugin_id) {
            return Err(PluginError::NoSuchPlugin);
        }
        let mut registry = self.registry.lock().unwrap();
        for entry in registry.values_mut() {
            if entry.owning_plugins.contains(&plugin_id) {
                entry.install_state = InstallState::Quarantined;
            }
        }
        drop(registry);
        self.quarantined_plugins.lock().unwrap().insert(plugin_id);
        Ok(())
    }

    /// The real registration point docs/998-roadmap.md's Resourceful pillar named as missing:
    /// every currently-installed, non-quarantined plugin's own `Contribution::Agent` entries,
    /// flattened into one list. `hyperion-coordination::catalog::best_fit_manifest_with_plugins`
    /// is the real caller, merging this with its own built-in roster so a plugin-contributed
    /// specialization competes for task allocation exactly like a first-party one.
    pub fn agent_contributions(&self) -> Vec<AgentContribution> {
        let quarantined = self.quarantined_plugins.lock().unwrap();
        self.agent_contributions
            .lock()
            .unwrap()
            .iter()
            .filter(|(plugin_id, _)| !quarantined.contains(*plugin_id))
            .flat_map(|(_, contributions)| contributions.iter().cloned())
            .collect()
    }

    /// The real "device driver registry" docs/998-roadmap.md's Resourceful pillar named as
    /// missing: every currently-installed, non-quarantined plugin's own
    /// `Contribution::HardwareSupport` entries, flattened into one list. `hyperion-device`'s own
    /// real caller looks one up by `(manufacturer, model)` rather than requiring every
    /// integrator to hand-write a capability manifest with no reference to consult.
    pub fn hardware_support_contributions(&self) -> Vec<HardwareSupportContribution> {
        let quarantined = self.quarantined_plugins.lock().unwrap();
        self.hardware_support
            .lock()
            .unwrap()
            .iter()
            .filter(|(plugin_id, _)| !quarantined.contains(*plugin_id))
            .flat_map(|(_, contributions)| contributions.iter().cloned())
            .collect()
    }

    /// docs/24 §6's `registry_query`, narrowed to exact `capability_id`
    /// lookup — the doc's semantic-embedding+threshold variant needs
    /// `hyperion-knowledge-graph`'s vector index, which this crate does
    /// not depend on (see this crate's doc comment).
    pub fn query(&self, capability_id: &str) -> Option<RegistryEntry> {
        self.registry
            .lock()
            .unwrap()
            .get(capability_id)
            .filter(|e| e.install_state != InstallState::Quarantined)
            .cloned()
    }

    pub fn boundary_of(&self, plugin_id: PluginId) -> Option<TrustBoundaryId> {
        self.boundaries.lock().unwrap().get(&plugin_id).copied()
    }

    /// The real, previously-missing execution this crate's own doc comment named: given a
    /// `capability_id` this registry has an installed `NativeBinary` implementation for, runs it
    /// for real, inside a real `hyperion_trust_boundary::spawn` sandbox, and returns its real
    /// output. `args` crosses the boundary as a real JSON file (`input.json`) in a fresh real temp
    /// directory that *is* the sandbox's entire real fs scope (Landlock-enforced); the program is
    /// expected to write its own real JSON result to `output.json` in that same directory before
    /// exiting. No `monitor`/`&mut` needed here — the one token this needs was already minted, for
    /// real, at install time (see [`Self::sandbox_tokens`]'s own doc comment for why).
    #[cfg(target_os = "linux")]
    pub fn invoke_native_binary(
        &self,
        capability_id: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let entry = self
            .query(capability_id)
            .ok_or(PluginError::NoSuchCapability)?;
        let descriptor = entry
            .implementations
            .iter()
            .find(|d| d.native_binary.is_some())
            .ok_or_else(|| PluginError::NoRunnableImplementation(capability_id.to_string()))?;
        // `install` already validated this `Some` -- see `validate_native_binary`.
        let native = descriptor.native_binary.as_ref().unwrap();
        let plugin_id = descriptor.plugin_id;

        let token = self
            .sandbox_tokens
            .lock()
            .unwrap()
            .get(&plugin_id)
            .cloned()
            .ok_or(PluginError::NoSuchPlugin)?;
        let policy_depth = self
            .plugins
            .lock()
            .unwrap()
            .get(&plugin_id)
            .map(|m| m.min_trust_depth)
            .ok_or(PluginError::NoSuchPlugin)?;

        let tempdir = tempfile::tempdir().map_err(|e| {
            PluginError::ExecutionFailed(format!(
                "couldn't create a real temp dir for sandboxed I/O: {e}"
            ))
        })?;
        let input_path = tempdir.path().join("input.json");
        let output_path = tempdir.path().join("output.json");
        std::fs::write(
            &input_path,
            serde_json::to_vec(&args).map_err(|e| {
                PluginError::ExecutionFailed(format!("couldn't serialize args: {e}"))
            })?,
        )
        .map_err(|e| PluginError::ExecutionFailed(format!("couldn't write input.json: {e}")))?;

        let mut command = std::process::Command::new(&native.program);
        command
            .args(&native.args)
            .arg(&input_path)
            .arg(&output_path);

        let grant = hyperion_trust_boundary::SpawnGrant {
            token,
            depth: real_trust_depth(policy_depth),
            fs_scope: tempdir.path().to_path_buf(),
        };
        let mut boundary_handle = hyperion_trust_boundary::spawn(&grant, command).map_err(|e| {
            PluginError::ExecutionFailed(format!("couldn't spawn the sandbox: {e}"))
        })?;

        // `try_wait` (a real, non-blocking `waitpid(WNOHANG)`), not `is_alive` -- `is_alive` only
        // checks signalability, which stays true for a real zombie (exited but unreaped), so a
        // loop built on it alone would spin until `NATIVE_BINARY_TIMEOUT` even for a tool that
        // finished instantly (a real bug this exact code caught live during development).
        let deadline = std::time::Instant::now() + NATIVE_BINARY_TIMEOUT;
        let exit_code = loop {
            if let Some(exit_code) = boundary_handle.try_wait().map_err(|e| {
                PluginError::ExecutionFailed(format!("couldn't check the sandbox's status: {e}"))
            })? {
                break exit_code;
            }
            if std::time::Instant::now() >= deadline {
                boundary_handle.kill().map_err(|e| {
                    PluginError::ExecutionFailed(format!(
                        "sandboxed process timed out and couldn't even be killed: {e}"
                    ))
                })?;
                return Err(PluginError::ExecutionFailed(format!(
                    "'{capability_id}' timed out after {NATIVE_BINARY_TIMEOUT:?} and was killed"
                )));
            }
            std::thread::sleep(NATIVE_BINARY_POLL_INTERVAL);
        };
        if exit_code != 0 {
            return Err(PluginError::ExecutionFailed(format!(
                "'{capability_id}' exited with a real, non-zero status {exit_code}"
            )));
        }

        let output_bytes = std::fs::read(&output_path).map_err(|e| {
            PluginError::ExecutionFailed(format!(
                "'{capability_id}' exited 0 but left no readable output.json: {e}"
            ))
        })?;
        serde_json::from_slice(&output_bytes).map_err(|e| {
            PluginError::ExecutionFailed(format!(
                "'{capability_id}'s output.json wasn't valid JSON: {e}"
            ))
        })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn invoke_native_binary(
        &self,
        capability_id: &str,
        _args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Err(PluginError::ExecutionFailed(format!(
            "'{capability_id}' needs real sandboxed execution (hyperion-trust-boundary), which \
             is Linux-only -- this platform can't run it"
        )))
    }
}

/// An honest check at install time, not a trusted claim: a `NativeBinary` contribution must
/// really name a program, and that program must really exist and really be executable right now
/// -- a manifest that only *claims* runnability never gets to install as if it had it.
fn validate_native_binary(
    descriptor: Option<&crate::types::NativeBinaryDescriptor>,
) -> Result<(), PluginError> {
    let Some(descriptor) = descriptor else {
        return Err(PluginError::InvalidNativeBinary(
            "ImplementationKind::NativeBinary requires a native_binary descriptor".to_string(),
        ));
    };
    let metadata = std::fs::metadata(&descriptor.program).map_err(|e| {
        PluginError::InvalidNativeBinary(format!(
            "{:?} doesn't exist or isn't readable: {e}",
            descriptor.program
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(PluginError::InvalidNativeBinary(format!(
                "{:?} exists but isn't executable",
                descriptor.program
            )));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
    }
    Ok(())
}
