use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_crypto::VerifyingKey;

use crate::review::{validate_manifest, validate_manifest_against_registry};
use crate::types::{
    AgentContribution, AutomationWorkflowContribution, CapabilityGrantRequest, CapabilityId,
    CapabilityManifest, Contribution, ExecutionEngineContribution, HardwareSupportContribution,
    ImplementationDescriptor, ImplementationKind, InstallState, KnowledgeProviderContribution,
    MemoryProviderContribution, PluginError, PluginHandle, PluginId, PluginManifest,
    QuarantineReason, RegistryEntry, TrustDepth, UiComponentContribution,
};

fn rights_for(op: crate::types::Operation) -> RightsMask {
    use crate::types::Operation;
    match op {
        Operation::Read => RightsMask::READ,
        Operation::Write | Operation::NetworkEgress => RightsMask::WRITE,
        Operation::Execute => RightsMask::EXEC,
    }
}

/// `true` if `a` and `b` are "the same grant" for [`PluginRegistry::update`]'s own diffing
/// purposes -- compared by `(operation, scope)` only, deliberately ignoring `justification`: a
/// publisher rewording why it needs a permission it already has doesn't make that permission
/// "new," and re-prompting a user's consent over wording alone would be exactly the kind of
/// needless re-confirmation docs/24 §5's diff-only update UX exists to avoid.
fn grants_equal(a: &CapabilityGrantRequest, b: &CapabilityGrantRequest) -> bool {
    a.operation == b.operation && a.scope == b.scope
}

/// `true` if `manifest` contains a `NativeBinary`-kind `Capability` contribution -- the one case
/// [`PluginRegistry::install`]/[`PluginRegistry::update`] mint a real sandbox token for. Shared so
/// the two call sites can never drift on what "needs a sandbox token" means.
fn needs_sandbox_token(manifest: &PluginManifest) -> bool {
    manifest.contributions.iter().any(|c| {
        matches!(
            c,
            Contribution::Capability(cm) if cm.implementation_kind == ImplementationKind::NativeBinary
        )
    })
}

/// docs/24 §5's `version_variant()`: a real, deterministic, collision-free `capability_id` for a
/// structurally incompatible collision -- appends `#N` for the smallest `N >= 2` not already a
/// key in `registry` (the original, un-suffixed id is implicitly "`#1`"). Deterministic rather
/// than random so the same sequence of installs always assigns the same variant ids, and always
/// terminates (`N` only ever grows) -- this can never itself fail the way the collision it
/// replaces used to.
fn version_variant(
    registry: &HashMap<CapabilityId, RegistryEntry>,
    capability_id: &str,
) -> CapabilityId {
    let mut n = 2u64;
    loop {
        let candidate = format!("{capability_id}#{n}");
        if !registry.contains_key(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// An honest check at install/update time, not a trusted claim: every `NativeBinary`-backed
/// contribution (a `Capability` of that kind, or any `ExecutionEngine`) must really name a
/// program that really exists and is really executable right now. Shared by
/// [`PluginRegistry::install`] and [`PluginRegistry::update`], and run before either one mutates
/// any state, so a manifest that fails this never partially installs/updates.
fn validate_contributions(manifest: &PluginManifest) -> Result<(), PluginError> {
    for contribution in &manifest.contributions {
        match contribution {
            Contribution::Capability(cm) => {
                if cm.implementation_kind == ImplementationKind::NativeBinary {
                    validate_native_binary(cm.native_binary.as_ref())?;
                }
            }
            // Same honest "must really exist and really be executable now" check a
            // Capability's own `NativeBinaryDescriptor` gets -- an engine that can never
            // actually launch anything must not install as if it could.
            Contribution::ExecutionEngine(ee) => {
                validate_native_binary(Some(&ee.launcher))?;
            }
            Contribution::Agent(_)
            | Contribution::HardwareSupport(_)
            | Contribution::KnowledgeProvider(_)
            | Contribution::UiComponent(_)
            | Contribution::AutomationWorkflow(_)
            | Contribution::MemoryProvider(_) => {}
        }
    }
    Ok(())
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
    /// Real registration point for `Contribution::KnowledgeProvider` -- the (topic ->
    /// capability_id) lookup `hyperion-knowledge-graph` had no equivalent of. Same shape and
    /// same reasoning as [`Self::agent_contributions`]/[`Self::hardware_support`].
    knowledge_providers: Mutex<HashMap<PluginId, Vec<KnowledgeProviderContribution>>>,
    /// Real registration point for `Contribution::UiComponent` -- the "every
    /// `CapabilityUiContract` is hand-authored, with no registry to consult" gap
    /// `hyperion-workspace` had. Same shape and same reasoning as
    /// [`Self::agent_contributions`]/[`Self::hardware_support`]/[`Self::knowledge_providers`].
    ui_components: Mutex<HashMap<PluginId, Vec<UiComponentContribution>>>,
    /// Real registration point for `Contribution::AutomationWorkflow` -- the hardcoded,
    /// crate-private `TEMPLATES` list `hyperion-intent` had no live registry equivalent of.
    /// Same shape and same reasoning as
    /// [`Self::agent_contributions`]/[`Self::hardware_support`]/[`Self::knowledge_providers`]/
    /// [`Self::ui_components`].
    automation_workflows: Mutex<HashMap<PluginId, Vec<AutomationWorkflowContribution>>>,
    /// Real registration point for `Contribution::MemoryProvider` -- the "no external memory
    /// source registry" gap `hyperion-memory` had. Same shape and same reasoning as
    /// [`Self::agent_contributions`]/[`Self::hardware_support`]/[`Self::knowledge_providers`]/
    /// [`Self::ui_components`]/[`Self::automation_workflows`].
    memory_providers: Mutex<HashMap<PluginId, Vec<MemoryProviderContribution>>>,
    /// Real registration point for `Contribution::ExecutionEngine` -- the "runtimes usable by
    /// Capability implementations" registry docs/24 names. Same shape and same reasoning as
    /// [`Self::agent_contributions`]/[`Self::hardware_support`]/[`Self::knowledge_providers`]/
    /// [`Self::ui_components`]/[`Self::automation_workflows`]/[`Self::memory_providers`].
    execution_engines: Mutex<HashMap<PluginId, Vec<ExecutionEngineContribution>>>,
    /// Plugin-level quarantine, tracked separately from `registry`'s own per-`CapabilityId`
    /// `InstallState` -- an `Agent`-only, `HardwareSupport`-only, `KnowledgeProvider`-only,
    /// `UiComponent`-only, `AutomationWorkflow`-only, `MemoryProvider`-only, or
    /// `ExecutionEngine`-only plugin owns no `RegistryEntry` for that mechanism to touch, so
    /// [`Self::quarantine`] needs a real place to hide its contributions too.
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
            knowledge_providers: Mutex::new(HashMap::new()),
            ui_components: Mutex::new(HashMap::new()),
            automation_workflows: Mutex::new(HashMap::new()),
            memory_providers: Mutex::new(HashMap::new()),
            execution_engines: Mutex::new(HashMap::new()),
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
        now: u64,
        verifying_key: &VerifyingKey,
    ) -> Result<PluginHandle, PluginError> {
        monitor
            .check_rights_ok_result(admin_token, RightsMask::GRANT)
            .map_err(|_| PluginError::Unauthorized)?;
        validate_manifest(&manifest, verifying_key)?;
        self.install_validated(
            monitor,
            admin_token,
            manifest,
            available_depth,
            consented,
            now,
        )
    }

    /// As [`Self::install`], but resolving the manifest's real trusted signing key from
    /// `manifest.publisher` via a real `hyperion_crypto::PublisherRegistry` instead of taking one
    /// caller-supplied key on faith -- docs/24's own "verify against publisher's registered key"
    /// framing, made real. This crate's own previously-named "multi-party / publisher trust
    /// stores" gap, closed here rather than by replacing [`Self::install`]'s own signature (every
    /// existing caller trusting one known device identity directly keeps working unchanged). A
    /// publisher `publishers` has no key registered for is [`PluginError::UnknownPublisher`] —
    /// never silently trusted against some other key.
    #[allow(clippy::too_many_arguments)]
    pub fn install_with_publisher_registry(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        manifest: PluginManifest,
        available_depth: TrustDepth,
        consented: bool,
        now: u64,
        publishers: &hyperion_crypto::PublisherRegistry,
    ) -> Result<PluginHandle, PluginError> {
        monitor
            .check_rights_ok_result(admin_token, RightsMask::GRANT)
            .map_err(|_| PluginError::Unauthorized)?;
        validate_manifest_against_registry(&manifest, publishers)?;
        self.install_validated(
            monitor,
            admin_token,
            manifest,
            available_depth,
            consented,
            now,
        )
    }

    /// The real, shared rest of `plugin_install`, once a manifest's signature has already
    /// verified against whichever key [`Self::install`]/[`Self::install_with_publisher_registry`]
    /// resolved it against -- kept as one function so the two entry points can never drift on
    /// what happens after signature verification.
    fn install_validated(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        manifest: PluginManifest,
        available_depth: TrustDepth,
        consented: bool,
        _now: u64,
    ) -> Result<PluginHandle, PluginError> {
        if manifest.min_trust_depth > available_depth {
            return Err(PluginError::InsufficientTrustDepth);
        }
        if !consented {
            return Err(PluginError::ConsentDeclined);
        }
        // An honest check now, not a trusted claim -- see `NativeBinaryDescriptor`'s own doc
        // comment. Checked before any minting/registration below, so a manifest that fails this
        // never partially installs, matching this function's own existing invariant.
        validate_contributions(&manifest)?;
        let needs_sandbox_token = needs_sandbox_token(&manifest);

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

        self.register_contributions(plugin_id, &manifest)?;

        self.plugins.lock().unwrap().insert(plugin_id, manifest);
        self.boundaries.lock().unwrap().insert(plugin_id, boundary);
        self.tokens.lock().unwrap().insert(plugin_id, minted);

        Ok(PluginHandle {
            plugin_id,
            boundary,
        })
    }

    /// docs/24 §5's `plugin_update`, closing this crate's own previously-named gap ("this crate
    /// has no `plugin_update` distinct from `uninstall` + `install`; a caller wanting the
    /// diff-only UX composes those two calls itself"). Presents (and returns, for a real caller's
    /// own consent UI to show) only the *new* grants `new_manifest.requested_permissions` asks
    /// for that the plugin's currently-installed permission set doesn't already cover, compared
    /// by `(operation, scope)` -- a reworded `justification` alone doesn't make a grant "new."
    /// Already-granted permissions reuse their existing token rather than being re-minted (and
    /// re-consented to) from scratch the way composing `uninstall` + `install` would; a permission
    /// present in the old manifest but dropped from the new one is really revoked, not silently
    /// left grantable forever. No consent is required at all when the diff is empty. Every
    /// contribution is re-registered from `new_manifest` (the old ones are removed first, exactly
    /// like `uninstall`'s own non-token cleanup) -- an update really replaces what a plugin
    /// contributes, it doesn't merely top up its permissions.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        plugin_id: PluginId,
        new_manifest: PluginManifest,
        available_depth: TrustDepth,
        consented_to_new_grants: bool,
        verifying_key: &VerifyingKey,
    ) -> Result<Vec<CapabilityGrantRequest>, PluginError> {
        monitor
            .check_rights_ok_result(admin_token, RightsMask::GRANT)
            .map_err(|_| PluginError::Unauthorized)?;
        let (old_manifest, boundary) = self.plugin_and_boundary(plugin_id)?;
        validate_manifest(&new_manifest, verifying_key)?;
        self.update_validated(
            monitor,
            admin_token,
            plugin_id,
            old_manifest,
            boundary,
            new_manifest,
            available_depth,
            consented_to_new_grants,
        )
    }

    /// As [`Self::update`], but resolving the new manifest's real trusted signing key from
    /// `new_manifest.publisher` via a real `hyperion_crypto::PublisherRegistry` -- the identical
    /// real multi-publisher trust closure [`Self::install_with_publisher_registry`] gives
    /// installation.
    #[allow(clippy::too_many_arguments)]
    pub fn update_with_publisher_registry(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        plugin_id: PluginId,
        new_manifest: PluginManifest,
        available_depth: TrustDepth,
        consented_to_new_grants: bool,
        publishers: &hyperion_crypto::PublisherRegistry,
    ) -> Result<Vec<CapabilityGrantRequest>, PluginError> {
        monitor
            .check_rights_ok_result(admin_token, RightsMask::GRANT)
            .map_err(|_| PluginError::Unauthorized)?;
        let (old_manifest, boundary) = self.plugin_and_boundary(plugin_id)?;
        validate_manifest_against_registry(&new_manifest, publishers)?;
        self.update_validated(
            monitor,
            admin_token,
            plugin_id,
            old_manifest,
            boundary,
            new_manifest,
            available_depth,
            consented_to_new_grants,
        )
    }

    fn plugin_and_boundary(
        &self,
        plugin_id: PluginId,
    ) -> Result<(PluginManifest, TrustBoundaryId), PluginError> {
        let old_manifest = self
            .plugins
            .lock()
            .unwrap()
            .get(&plugin_id)
            .cloned()
            .ok_or(PluginError::NoSuchPlugin)?;
        let boundary = *self
            .boundaries
            .lock()
            .unwrap()
            .get(&plugin_id)
            .ok_or(PluginError::NoSuchPlugin)?;
        Ok((old_manifest, boundary))
    }

    /// The real, shared rest of `plugin_update`, once the new manifest's signature has already
    /// verified against whichever key [`Self::update`]/[`Self::update_with_publisher_registry`]
    /// resolved it against.
    #[allow(clippy::too_many_arguments)]
    fn update_validated(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        plugin_id: PluginId,
        old_manifest: PluginManifest,
        boundary: TrustBoundaryId,
        new_manifest: PluginManifest,
        available_depth: TrustDepth,
        consented_to_new_grants: bool,
    ) -> Result<Vec<CapabilityGrantRequest>, PluginError> {
        if new_manifest.min_trust_depth > available_depth {
            return Err(PluginError::InsufficientTrustDepth);
        }

        let new_grants: Vec<CapabilityGrantRequest> = new_manifest
            .requested_permissions
            .iter()
            .filter(|grant| {
                !old_manifest
                    .requested_permissions
                    .iter()
                    .any(|existing| grants_equal(existing, grant))
            })
            .cloned()
            .collect();
        if !new_grants.is_empty() && !consented_to_new_grants {
            return Err(PluginError::ConsentDeclined);
        }
        // Same real "must exist and be executable now" check `install` performs, before any real
        // mutation below -- a failing update must leave the plugin's previous, still-working
        // install untouched.
        validate_contributions(&new_manifest)?;

        let old_tokens = self
            .tokens
            .lock()
            .unwrap()
            .get(&plugin_id)
            .cloned()
            .unwrap_or_default();
        let mut updated_tokens = Vec::with_capacity(new_manifest.requested_permissions.len());
        for grant in &new_manifest.requested_permissions {
            let reused_token = old_manifest
                .requested_permissions
                .iter()
                .position(|existing| grants_equal(existing, grant))
                .map(|i| old_tokens[i].clone());
            let token = match reused_token {
                Some(token) => token,
                None => {
                    monitor.cap_derive(admin_token, rights_for(grant.operation), None, boundary)?
                }
            };
            updated_tokens.push(token);
        }
        for (i, old_grant) in old_manifest.requested_permissions.iter().enumerate() {
            let still_requested = new_manifest
                .requested_permissions
                .iter()
                .any(|grant| grants_equal(grant, old_grant));
            if !still_requested {
                monitor.cap_revoke(&old_tokens[i]);
            }
        }

        self.remove_registry_and_contributions(plugin_id);
        self.register_contributions(plugin_id, &new_manifest)?;

        let needs_sandbox = needs_sandbox_token(&new_manifest);
        let mut sandbox_tokens = self.sandbox_tokens.lock().unwrap();
        if needs_sandbox && !sandbox_tokens.contains_key(&plugin_id) {
            let sandbox_token = monitor.cap_derive(
                admin_token,
                RightsMask::READ | RightsMask::WRITE,
                None,
                boundary,
            )?;
            sandbox_tokens.insert(plugin_id, sandbox_token);
        } else if !needs_sandbox {
            sandbox_tokens.remove(&plugin_id);
        }
        drop(sandbox_tokens);

        self.plugins.lock().unwrap().insert(plugin_id, new_manifest);
        self.tokens
            .lock()
            .unwrap()
            .insert(plugin_id, updated_tokens);

        Ok(new_grants)
    }

    /// Registers every one of `manifest`'s own contributions under `plugin_id` -- `Capability`
    /// contributions into the shared registry via [`Self::register_implementation`], every other
    /// variant into its own per-`plugin_id` map. Shared by [`Self::install`] and [`Self::update`];
    /// callers must run [`validate_contributions`] first (this method only mutates, it never
    /// validates).
    fn register_contributions(
        &self,
        plugin_id: PluginId,
        manifest: &PluginManifest,
    ) -> Result<(), PluginError> {
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
                Contribution::KnowledgeProvider(kp) => {
                    self.knowledge_providers
                        .lock()
                        .unwrap()
                        .entry(plugin_id)
                        .or_default()
                        .push(kp.clone());
                }
                Contribution::UiComponent(ui) => {
                    self.ui_components
                        .lock()
                        .unwrap()
                        .entry(plugin_id)
                        .or_default()
                        .push(ui.clone());
                }
                Contribution::AutomationWorkflow(wf) => {
                    self.automation_workflows
                        .lock()
                        .unwrap()
                        .entry(plugin_id)
                        .or_default()
                        .push(wf.clone());
                }
                Contribution::MemoryProvider(mp) => {
                    self.memory_providers
                        .lock()
                        .unwrap()
                        .entry(plugin_id)
                        .or_default()
                        .push(mp.clone());
                }
                Contribution::ExecutionEngine(ee) => {
                    self.execution_engines
                        .lock()
                        .unwrap()
                        .entry(plugin_id)
                        .or_default()
                        .push(ee.clone());
                }
            }
        }
        Ok(())
    }

    /// Removes every registered contribution and registry entry belonging to `plugin_id` -- the
    /// non-token half of [`Self::uninstall`]'s own cleanup, reused as-is by [`Self::update`]
    /// (which must remove the plugin's *old* contributions before registering its new ones, but
    /// -- unlike a real uninstall -- never revokes every token, only the ones the new manifest no
    /// longer requests; see [`Self::update`]'s own doc comment).
    fn remove_registry_and_contributions(&self, plugin_id: PluginId) {
        let mut registry = self.registry.lock().unwrap();
        for entry in registry.values_mut() {
            entry.implementations.retain(|d| d.plugin_id != plugin_id);
            entry.owning_plugins.retain(|&id| id != plugin_id);
        }
        registry.retain(|_, entry| !entry.implementations.is_empty());
        drop(registry);

        self.agent_contributions.lock().unwrap().remove(&plugin_id);
        self.hardware_support.lock().unwrap().remove(&plugin_id);
        self.knowledge_providers.lock().unwrap().remove(&plugin_id);
        self.ui_components.lock().unwrap().remove(&plugin_id);
        self.automation_workflows.lock().unwrap().remove(&plugin_id);
        self.memory_providers.lock().unwrap().remove(&plugin_id);
        self.execution_engines.lock().unwrap().remove(&plugin_id);
    }

    /// docs/24 §5's structural-compatibility check on `capability_id`
    /// collision: identical contract shape merges into the existing
    /// `RegistryEntry` as one more competing implementation; a structurally
    /// incompatible one registers instead under a real, distinct
    /// `version_variant()` id — docs/24 §5's own pseudocode ("the new
    /// implementation registers as a genuinely separate `RegistryEntry`
    /// instead of being rejected outright or silently shadowing the
    /// existing contract"), closing this crate's own previously-named
    /// "`version_variant()` minting a distinct id is deferred" gap. Never
    /// fails: [`version_variant`] always finds a free id, so a manifest
    /// with an incompatible `capability_id` still installs in full,
    /// exactly like a compatible one — it just competes under a different
    /// name.
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
            Some(entry) if entry.contract == cm.contract => {
                entry.implementations.push(descriptor.clone());
                entry.owning_plugins.push(plugin_id);
            }
            Some(_) => {
                let versioned_id = version_variant(&registry, &cm.capability_id);
                registry.insert(
                    versioned_id.clone(),
                    RegistryEntry {
                        capability_id: versioned_id,
                        contract: cm.contract.clone(),
                        implementations: vec![descriptor.clone()],
                        owning_plugins: vec![plugin_id],
                        install_state: InstallState::Active,
                    },
                );
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

        self.remove_registry_and_contributions(plugin_id);

        self.plugins.lock().unwrap().remove(&plugin_id);
        self.boundaries.lock().unwrap().remove(&plugin_id);
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

    /// The real (topic -> capability_id) lookup docs/998-roadmap.md's Resourceful pillar named
    /// as missing: every currently-installed, non-quarantined plugin's own
    /// `Contribution::KnowledgeProvider` entries, flattened into one list.
    /// `hyperion-knowledge-graph`'s own real caller filters this by topic to decide which
    /// installed capability can answer a query it has no local knowledge of.
    pub fn knowledge_provider_contributions(&self) -> Vec<KnowledgeProviderContribution> {
        let quarantined = self.quarantined_plugins.lock().unwrap();
        self.knowledge_providers
            .lock()
            .unwrap()
            .iter()
            .filter(|(plugin_id, _)| !quarantined.contains(*plugin_id))
            .flat_map(|(_, contributions)| contributions.iter().cloned())
            .collect()
    }

    /// The real registration point docs/998-roadmap.md's Resourceful pillar named as missing:
    /// every currently-installed, non-quarantined plugin's own `Contribution::UiComponent`
    /// entries, flattened into one list. `hyperion-workspace`'s own real caller looks one up by
    /// `capability_ref` instead of every integrator hand-authoring a `CapabilityUiContract` with
    /// no registry to consult.
    pub fn ui_component_contributions(&self) -> Vec<UiComponentContribution> {
        let quarantined = self.quarantined_plugins.lock().unwrap();
        self.ui_components
            .lock()
            .unwrap()
            .iter()
            .filter(|(plugin_id, _)| !quarantined.contains(*plugin_id))
            .flat_map(|(_, contributions)| contributions.iter().cloned())
            .collect()
    }

    /// The real registration point docs/998-roadmap.md's Resourceful pillar named as missing:
    /// every currently-installed, non-quarantined plugin's own `Contribution::AutomationWorkflow`
    /// entries, flattened into one list. `hyperion-intent`'s own real caller matches these
    /// alongside its hardcoded `TEMPLATES` roster, so a plugin-contributed goal template really
    /// competes for a real utterance match.
    pub fn automation_workflow_contributions(&self) -> Vec<AutomationWorkflowContribution> {
        let quarantined = self.quarantined_plugins.lock().unwrap();
        self.automation_workflows
            .lock()
            .unwrap()
            .iter()
            .filter(|(plugin_id, _)| !quarantined.contains(*plugin_id))
            .flat_map(|(_, contributions)| contributions.iter().cloned())
            .collect()
    }

    /// The real registration point docs/998-roadmap.md's Resourceful pillar named as missing:
    /// every currently-installed, non-quarantined plugin's own `Contribution::MemoryProvider`
    /// entries, flattened into one list. `hyperion-memory`'s own real caller filters this by
    /// `(tier, entity_key)` to decide which installed capability can supply facts about an
    /// entity it has no local memory record of.
    pub fn memory_provider_contributions(&self) -> Vec<MemoryProviderContribution> {
        let quarantined = self.quarantined_plugins.lock().unwrap();
        self.memory_providers
            .lock()
            .unwrap()
            .iter()
            .filter(|(plugin_id, _)| !quarantined.contains(*plugin_id))
            .flat_map(|(_, contributions)| contributions.iter().cloned())
            .collect()
    }

    /// The real "runtimes usable by Capability implementations" registration point docs/24
    /// names as an `ExecutionEngine` contribution's job. `hyperion_sdk::resolve_via_engine` is
    /// the real caller: it looks an installed, non-quarantined engine up by `engine_id` and
    /// turns a caller's own script into a concrete, runnable `NativeBinaryDescriptor` by
    /// prepending this engine's own real launcher. `None` if no installed, non-quarantined
    /// plugin ever contributed this `engine_id`.
    pub fn execution_engine(&self, engine_id: &str) -> Option<ExecutionEngineContribution> {
        let quarantined = self.quarantined_plugins.lock().unwrap();
        self.execution_engines
            .lock()
            .unwrap()
            .iter()
            .filter(|(plugin_id, _)| !quarantined.contains(*plugin_id))
            .flat_map(|(_, contributions)| contributions.iter().cloned())
            .find(|ee| ee.engine_id == engine_id)
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

    /// The real, currently-tracked tokens this plugin was minted, in the same order as its own
    /// `requested_permissions` -- exposed for the same reason [`Self::boundary_of`] is: real
    /// capability state a caller (or this crate's own tests) can inspect without a second,
    /// parallel bookkeeping system. [`Self::update`] reuses a token here across an update exactly
    /// when the corresponding grant is unchanged, rather than re-minting one with equivalent
    /// rights under a new identity -- checkable here via plain [`CapabilityToken`] equality, not
    /// just [`hyperion_capability::CapabilityMonitor::is_live`]. `None` if `plugin_id` isn't
    /// installed.
    pub fn tokens_of(&self, plugin_id: PluginId) -> Option<Vec<CapabilityToken>> {
        self.tokens.lock().unwrap().get(&plugin_id).cloned()
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
            // A one-shot NativeBinary invocation communicates via input.json/output.json in its
            // own real fs_scope -- it has no rendezvous socket to bind, so no IPC rights at all.
            ipc_rendezvous: None,
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
