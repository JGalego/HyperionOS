use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceType {
    Display,
    Mobile,
    Vehicle,
    Robot,
    Wearable,
    HomeAppliance,
    Peripheral,
    Sensor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Render,
    Sense,
    Actuate,
}

/// docs/20 §4's `safety_class` — gates actuators into a higher trust tier
/// than a passive sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SafetyClass {
    Cosmetic,
    Standard,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityManifestEntry {
    pub capability_name: String,
    pub direction: Direction,
    pub safety_class: SafetyClass,
}

/// docs/20 §5.3's tiered trust — `View < Sense < Actuate` in required
/// friction, not in numeric ordering; `Actuate` requires the caller to pass
/// an explicit confirmation, checked at [`crate::DeviceRegistry::pair`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TrustTier {
    View,
    Sense,
    Actuate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresenceState {
    Connected,
    Degraded,
    Disconnected,
}

/// docs/20 §4's `DeviceObject`, narrowed per this crate's doc comment (no
/// `location_context`/`power_profile` fields yet — nothing here consumes
/// them without a real Device Framework -> Workspace integration).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceObject {
    pub device_id: u64,
    pub device_type: DeviceType,
    pub manufacturer: String,
    pub model: String,
    pub capability_manifest: Vec<CapabilityManifestEntry>,
    pub owner: u64,
    pub presence: PresenceState,
    pub last_heartbeat: u64,
}

/// docs/20 §4's `PairingRecord`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRecord {
    pub device_id: u64,
    pub trust_tier: TrustTier,
    pub granted_capabilities: Vec<String>,
    pub expiry: Option<u64>,
}

impl PairingRecord {
    pub fn grants(&self, capability_name: &str) -> bool {
        self.granted_capabilities
            .iter()
            .any(|c| c == capability_name)
    }
}
