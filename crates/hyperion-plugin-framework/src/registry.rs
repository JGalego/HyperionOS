use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_crypto::VerifyingKey;

use crate::review::validate_manifest;
use crate::types::{
    CapabilityId, CapabilityManifest, Contribution, ImplementationDescriptor, InstallState,
    PluginError, PluginHandle, PluginId, PluginManifest, QuarantineReason, RegistryEntry,
    TrustDepth,
};

fn rights_for(op: crate::types::Operation) -> RightsMask {
    use crate::types::Operation;
    match op {
        Operation::Read => RightsMask::READ,
        Operation::Write | Operation::NetworkEgress => RightsMask::WRITE,
        Operation::Execute => RightsMask::EXEC,
    }
}

/// docs/24 — Plugin Framework. See this crate's doc comment for the full
/// real/deferred split.
pub struct PluginRegistry {
    plugins: Mutex<HashMap<PluginId, PluginManifest>>,
    boundaries: Mutex<HashMap<PluginId, TrustBoundaryId>>,
    tokens: Mutex<HashMap<PluginId, Vec<CapabilityToken>>>,
    registry: Mutex<HashMap<CapabilityId, RegistryEntry>>,
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
            registry: Mutex::new(HashMap::new()),
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

        let plugin_id = self.next_plugin_id.fetch_add(1, Ordering::Relaxed);
        let boundary =
            TrustBoundaryId(1_000_000 + self.next_boundary_ordinal.fetch_add(1, Ordering::Relaxed));

        let mut minted = Vec::with_capacity(manifest.requested_permissions.len());
        for request in &manifest.requested_permissions {
            let token =
                monitor.cap_derive(admin_token, rights_for(request.operation), None, boundary)?;
            minted.push(token);
        }

        for contribution in &manifest.contributions {
            let Contribution::Capability(cm) = contribution;
            self.register_implementation(plugin_id, cm)?;
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
        Ok(())
    }

    /// docs/24 §6's `registry_quarantine` — disables the plugin's
    /// registry entries without a full uninstall (its tokens remain
    /// live; a quarantined entry is simply never returned as an eligible
    /// candidate by [`Self::query`]).
    pub fn quarantine(
        &self,
        plugin_id: PluginId,
        _reason: QuarantineReason,
    ) -> Result<(), PluginError> {
        let mut registry = self.registry.lock().unwrap();
        let mut touched = false;
        for entry in registry.values_mut() {
            if entry.owning_plugins.contains(&plugin_id) {
                entry.install_state = InstallState::Quarantined;
                touched = true;
            }
        }
        if touched {
            Ok(())
        } else {
            Err(PluginError::NoSuchPlugin)
        }
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
}
