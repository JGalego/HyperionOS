use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};
use hyperion_crypto::{Signature, VerifyingKey};
use hyperion_knowledge_graph::{KnowledgeGraph, NodeId};

use crate::manifest;
use crate::types::{
    CapabilityManifestEntry, DeviceObject, DeviceType, Direction, PairingRecord, PresenceState,
    TrustTier,
};

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("no such device")]
    NotFound,
    #[error("device is not paired")]
    NotPaired,
    #[error("the requested trust tier is insufficient for this capability")]
    InsufficientTier,
    #[error("capability not declared in this device's manifest")]
    CapabilityNotDeclared,
    #[error("actuation-tier pairing requires explicit confirmation")]
    ActuationRequiresConfirmation,
    #[error("device is not reachable")]
    Unreachable,
    #[error("device manifest signature does not verify against the trusted device identity")]
    SignatureInvalid,
    #[error("knowledge graph error: {0}")]
    Graph(#[from] hyperion_knowledge_graph::GraphError),
}

/// The one real function every write to a device's Knowledge Graph node goes through --
/// [`DeviceRegistry::register`]'s first write and every real [`DeviceRegistry::resync_kg_node`]
/// re-sync alike -- so the node's own metadata shape can never drift between them.
/// `DeviceObject`'s own fields are still flattened at the top level (unchanged from before this
/// re-sync gap closed, so an existing query by e.g. `metadata["manufacturer"]` keeps working
/// unchanged); `pairing` is a real, new sibling field carrying the device's current
/// `PairingRecord`, or `null` when never paired (or revoked) -- the "grants" half of this crate's
/// own previously-named "presence, `last_heartbeat`, grants" re-sync gap.
fn device_metadata(device: &DeviceObject, pairing: Option<&PairingRecord>) -> serde_json::Value {
    let mut metadata = serde_json::to_value(device).expect("DeviceObject always serializes");
    if let serde_json::Value::Object(ref mut map) = metadata {
        map.insert(
            "pairing".to_string(),
            serde_json::to_value(pairing).expect("Option<&PairingRecord> always serializes"),
        );
    }
    metadata
}

/// docs/20 — Device Framework. See this crate's doc comment for what's
/// deferred.
pub struct DeviceRegistry {
    devices: Mutex<HashMap<u64, DeviceObject>>,
    pairings: Mutex<HashMap<u64, PairingRecord>>,
    next_id: AtomicU64,
    /// docs/20 §4: "a Semantic Object subtype" — every registered
    /// `DeviceObject` is mirrored here as a real Knowledge Graph node,
    /// keyed by `device_id`. Populated at [`Self::register`] time only;
    /// see this crate's doc comment on why later mutations
    /// (`heartbeat`/`tick`/`pair`) don't yet re-sync it.
    graph: Arc<KnowledgeGraph>,
    kg_nodes: Mutex<HashMap<u64, NodeId>>,
}

impl DeviceRegistry {
    pub fn new(graph: Arc<KnowledgeGraph>) -> Self {
        DeviceRegistry {
            devices: Mutex::new(HashMap::new()),
            pairings: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            graph,
            kg_nodes: Mutex::new(HashMap::new()),
        }
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), DeviceError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| DeviceError::Unauthorized)
    }

    /// docs/20 §5.1/§5.2: normalizes an already-discovered device's
    /// advertised manifest — see this crate's doc comment on the deferred
    /// real discovery transport. docs/20 §8's device-impersonation defense,
    /// now real: `signature` must verify against `verifying_key` over
    /// exactly this manifest's own fields ([`manifest::sign`] is what a
    /// caller producing one uses), or registration is refused outright,
    /// before anything is recorded.
    #[allow(clippy::too_many_arguments)]
    pub fn register(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        device_type: DeviceType,
        manufacturer: &str,
        model: &str,
        capability_manifest: Vec<CapabilityManifestEntry>,
        owner: u64,
        now: u64,
        signature: &Signature,
        verifying_key: &VerifyingKey,
    ) -> Result<u64, DeviceError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        if !manifest::verify(
            device_type,
            manufacturer,
            model,
            &capability_manifest,
            owner,
            signature,
            verifying_key,
        ) {
            return Err(DeviceError::SignatureInvalid);
        }
        let device_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let device = DeviceObject {
            device_id,
            device_type,
            manufacturer: manufacturer.to_string(),
            model: model.to_string(),
            capability_manifest,
            owner,
            presence: PresenceState::Connected,
            last_heartbeat: now,
        };

        // Never paired yet -- see `device_metadata`'s own doc comment on why every write to this
        // node (this one and every real re-sync below) goes through the identical function.
        let metadata = device_metadata(&device, None);
        let node_id = self
            .graph
            .put_node(monitor, token, None, "device", None, metadata)?;
        self.kg_nodes.lock().unwrap().insert(device_id, node_id);

        self.devices.lock().unwrap().insert(device_id, device);
        Ok(device_id)
    }

    /// This crate's own previously-named "re-syncing the Knowledge Graph node after
    /// registration" gap, closed for real: re-serializes `device_id`'s current `DeviceObject` (and
    /// its current `PairingRecord`, if any) via [`device_metadata`] and writes it back to the
    /// exact same real node [`Self::register`] created, via `put_node`'s own real "update in
    /// place" mode (`Some(node_id)`) -- never a second, parallel node. A silent no-op if
    /// `device_id` was never registered (or was somehow removed), rather than a hard error --
    /// every caller of this is itself a real, already-successful mutation
    /// ([`Self::heartbeat`]/[`Self::tick`]/[`Self::pair`]/[`Self::revoke`]) that shouldn't be
    /// undone just because there's no real KG mirror to update.
    fn resync_kg_node(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        device_id: u64,
    ) -> Result<(), DeviceError> {
        let Some(device) = self.devices.lock().unwrap().get(&device_id).cloned() else {
            return Ok(());
        };
        let pairing = self.pairings.lock().unwrap().get(&device_id).cloned();
        let metadata = device_metadata(&device, pairing.as_ref());
        let node_id = self.kg_node_for(device_id);
        let new_node_id = self
            .graph
            .put_node(monitor, token, node_id, "device", None, metadata)?;
        self.kg_nodes.lock().unwrap().insert(device_id, new_node_id);
        Ok(())
    }

    /// The real Knowledge Graph node [`Self::register`] created for
    /// `device_id`, per docs/20 §4's "a Semantic Object subtype" — the
    /// queryable proof this registry doesn't just hold `DeviceObject`s
    /// in-process.
    pub fn kg_node_for(&self, device_id: u64) -> Option<NodeId> {
        self.kg_nodes.lock().unwrap().get(&device_id).copied()
    }

    fn required_tier_for(direction: Direction) -> TrustTier {
        match direction {
            Direction::Render => TrustTier::View,
            Direction::Sense => TrustTier::Sense,
            Direction::Actuate => TrustTier::Actuate,
        }
    }

    /// docs/20 §5.3's tiered trust negotiation. `confirmed` must be `true`
    /// to request [`TrustTier::Actuate`] — the one deliberate exception to
    /// this workspace's usual frictionless defaults, per the doc's own
    /// "Golden Rule resolves this tension in favor of physical-world
    /// safety."
    pub fn pair(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        device_id: u64,
        requested_tier: TrustTier,
        capabilities: Vec<String>,
        confirmed: bool,
    ) -> Result<PairingRecord, DeviceError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        if requested_tier == TrustTier::Actuate && !confirmed {
            return Err(DeviceError::ActuationRequiresConfirmation);
        }

        let devices = self.devices.lock().unwrap();
        let device = devices.get(&device_id).ok_or(DeviceError::NotFound)?;
        for capability in &capabilities {
            let entry = device
                .capability_manifest
                .iter()
                .find(|m| &m.capability_name == capability)
                .ok_or(DeviceError::CapabilityNotDeclared)?;
            if requested_tier < Self::required_tier_for(entry.direction) {
                return Err(DeviceError::InsufficientTier);
            }
        }
        drop(devices);

        let record = PairingRecord {
            device_id,
            trust_tier: requested_tier,
            granted_capabilities: capabilities,
            expiry: None,
        };
        self.pairings
            .lock()
            .unwrap()
            .insert(device_id, record.clone());
        self.resync_kg_node(monitor, token, device_id)?;
        Ok(record)
    }

    pub fn revoke(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        device_id: u64,
    ) -> Result<(), DeviceError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        self.pairings.lock().unwrap().remove(&device_id);
        self.resync_kg_node(monitor, token, device_id)
    }

    /// docs/20 §6's `device.capability.invoke` — validated against the
    /// manifest contract and the pairing grant before dispatch, never
    /// dispatched to an unpaired or undeclared capability. No real
    /// actuator exists, so a successful dispatch returns a deterministic
    /// echo, matching this workspace's stub-capability convention.
    pub fn invoke(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        device_id: u64,
        capability_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, DeviceError> {
        self.require(monitor, token, RightsMask::EXEC)?;

        let devices = self.devices.lock().unwrap();
        let device = devices.get(&device_id).ok_or(DeviceError::NotFound)?;
        if device.presence == PresenceState::Disconnected {
            return Err(DeviceError::Unreachable);
        }
        if !device
            .capability_manifest
            .iter()
            .any(|m| m.capability_name == capability_name)
        {
            return Err(DeviceError::CapabilityNotDeclared);
        }
        drop(devices);

        let pairings = self.pairings.lock().unwrap();
        let pairing = pairings.get(&device_id).ok_or(DeviceError::NotPaired)?;
        if !pairing.grants(capability_name) {
            return Err(DeviceError::CapabilityNotDeclared);
        }

        Ok(serde_json::json!({"device_id": device_id, "capability": capability_name, "echo": args}))
    }

    /// Now really re-syncs the real Knowledge Graph node [`Self::register`] created (see
    /// [`Self::resync_kg_node`]) -- this crate's own previously-named "re-syncing the Knowledge
    /// Graph node after registration" gap, for the one of its two hardest cases (the other being
    /// [`Self::tick`]) that "take no `CapabilityMonitor`/`CapabilityToken` at all," per this
    /// crate's own doc comment: a device's own physical heartbeat isn't itself a capability-
    /// mediated action, so the caller driving it (whatever real loop calls this on the device's
    /// behalf) now supplies the one that authorizes the resulting KG write.
    pub fn heartbeat(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        device_id: u64,
        now: u64,
    ) -> Result<(), DeviceError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        {
            let mut devices = self.devices.lock().unwrap();
            let device = devices.get_mut(&device_id).ok_or(DeviceError::NotFound)?;
            device.last_heartbeat = now;
            device.presence = PresenceState::Connected;
        }
        self.resync_kg_node(monitor, token, device_id)
    }

    /// docs/20 §5.6's transient-connectivity state machine, recomputed
    /// statelessly from elapsed time rather than incrementally — repeated
    /// calls with the same `now` are idempotent, which is the property a
    /// caller-driven simulator clock needs. Now really re-syncs every device's real Knowledge
    /// Graph node too (see [`Self::resync_kg_node`]) -- the harder of this crate's own two named
    /// "no single token that would authorize writing all of them" cases: `tick` sweeps every
    /// device at once, so the one real `token` a caller now supplies here authorizes the whole
    /// sweep's real writes, not a per-device one. Correctness over efficiency, appropriate at
    /// this scale (this crate's own established convention, see `hyperion-api-gateway`'s
    /// identical choice for re-deriving Model Router candidates on every call): every device is
    /// re-synced every tick, not just ones whose presence actually changed.
    pub fn tick(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        now: u64,
        degraded_after_secs: u64,
        disconnected_after_secs: u64,
    ) -> Result<(), DeviceError> {
        self.require(monitor, token, RightsMask::WRITE)?;
        let device_ids: Vec<u64> = self.devices.lock().unwrap().keys().copied().collect();
        for device_id in device_ids {
            {
                let mut devices = self.devices.lock().unwrap();
                let device = devices
                    .get_mut(&device_id)
                    .expect("device_id was just listed from this same map");
                let elapsed = now.saturating_sub(device.last_heartbeat);
                device.presence = if elapsed > disconnected_after_secs {
                    PresenceState::Disconnected
                } else if elapsed > degraded_after_secs {
                    PresenceState::Degraded
                } else {
                    PresenceState::Connected
                };
            }
            self.resync_kg_node(monitor, token, device_id)?;
        }
        Ok(())
    }

    pub fn get(&self, device_id: u64) -> Option<DeviceObject> {
        self.devices.lock().unwrap().get(&device_id).cloned()
    }

    pub fn pairing_of(&self, device_id: u64) -> Option<PairingRecord> {
        self.pairings.lock().unwrap().get(&device_id).cloned()
    }

    /// docs/20 §10's substitute-device handoff: another `Connected`
    /// device of the same owner declaring the same capability.
    pub fn find_substitute(&self, capability_name: &str, owner: u64, exclude: u64) -> Option<u64> {
        self.devices
            .lock()
            .unwrap()
            .values()
            .filter(|d| {
                d.device_id != exclude && d.owner == owner && d.presence == PresenceState::Connected
            })
            .find(|d| {
                d.capability_manifest
                    .iter()
                    .any(|m| m.capability_name == capability_name)
            })
            .map(|d| d.device_id)
    }

    /// docs/20 §5.5's Device Registry query that Cross-Device Workspace
    /// Assembly would consult — see this crate's doc comment on the
    /// deferred `hyperion-workspace` wiring.
    pub fn find_render_surfaces(&self, owner: u64) -> Vec<u64> {
        self.devices
            .lock()
            .unwrap()
            .values()
            .filter(|d| d.owner == owner && d.presence != PresenceState::Disconnected)
            .filter(|d| {
                d.capability_manifest
                    .iter()
                    .any(|m| m.direction == Direction::Render)
            })
            .map(|d| d.device_id)
            .collect()
    }
}
