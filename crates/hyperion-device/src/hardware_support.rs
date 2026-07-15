//! docs/998-roadmap.md's Resourceful pillar: the real "device driver registry" this crate's own
//! doc comment named as missing. Before this, every caller of [`crate::DeviceRegistry::register`]
//! had to hand-author a full [`CapabilityManifestEntry`] list with nothing to consult, even for
//! a well-known `(manufacturer, model)` — this module is the real lookup a plugin's
//! `Contribution::HardwareSupport` populates.
//!
//! **Never bypasses `DeviceRegistry::register`'s own real signature check** (docs/20 §8's
//! device-impersonation defense): [`known_capability_manifest`] only supplies what a real
//! pairing flow can *propose* as the expected manifest — the device (or its driver, standing in
//! for it) must still really sign over whatever manifest registration ultimately uses.

use hyperion_plugin_framework::{
    HardwareDeviceType, HardwareDirection, HardwareSafetyClass, PluginRegistry,
};

use crate::types::{CapabilityManifestEntry, DeviceType, Direction, SafetyClass};

fn device_type_matches(a: DeviceType, b: HardwareDeviceType) -> bool {
    matches!(
        (a, b),
        (DeviceType::Display, HardwareDeviceType::Display)
            | (DeviceType::Mobile, HardwareDeviceType::Mobile)
            | (DeviceType::Vehicle, HardwareDeviceType::Vehicle)
            | (DeviceType::Robot, HardwareDeviceType::Robot)
            | (DeviceType::Wearable, HardwareDeviceType::Wearable)
            | (DeviceType::HomeAppliance, HardwareDeviceType::HomeAppliance)
            | (DeviceType::Peripheral, HardwareDeviceType::Peripheral)
            | (DeviceType::Sensor, HardwareDeviceType::Sensor)
    )
}

fn direction_from(direction: HardwareDirection) -> Direction {
    match direction {
        HardwareDirection::Render => Direction::Render,
        HardwareDirection::Sense => Direction::Sense,
        HardwareDirection::Actuate => Direction::Actuate,
    }
}

fn safety_class_from(safety_class: HardwareSafetyClass) -> SafetyClass {
    match safety_class {
        HardwareSafetyClass::Cosmetic => SafetyClass::Cosmetic,
        HardwareSafetyClass::Standard => SafetyClass::Standard,
        HardwareSafetyClass::High => SafetyClass::High,
    }
}

/// The real lookup a pairing flow consults instead of requiring every integrator to hand-write
/// a capability manifest for a device type it has no reference for: every currently-installed,
/// non-quarantined plugin's own `Contribution::HardwareSupport` entries are searched for an
/// exact `(device_type, manufacturer, model)` match, and the first hit's manifest is converted
/// into this crate's own real [`CapabilityManifestEntry`] shape. Returns `None` if no installed
/// plugin knows this device — the caller still has to supply a manifest itself in that case,
/// exactly as before this module existed.
pub fn known_capability_manifest(
    plugins: &PluginRegistry,
    device_type: DeviceType,
    manufacturer: &str,
    model: &str,
) -> Option<Vec<CapabilityManifestEntry>> {
    plugins
        .hardware_support_contributions()
        .into_iter()
        .find(|hs| {
            device_type_matches(device_type, hs.device_type)
                && hs.manufacturer == manufacturer
                && hs.model == model
        })
        .map(|hs| {
            hs.capability_manifest
                .into_iter()
                .map(|entry| CapabilityManifestEntry {
                    capability_name: entry.capability_name,
                    direction: direction_from(entry.direction),
                    safety_class: safety_class_from(entry.safety_class),
                })
                .collect()
        })
}
