//! Cold-boot measurement against docs/36-performance-benchmarks.md's budget
//! — docs/41-implementation-phases.md's Phase 1 exit criterion: "cold boot
//! is measured (not yet optimized) against 36's budget."
//!
//! A hosted simulator has no firmware/bootloader handoff, no real drivers,
//! no resident model to load, and no compositor — most of 36's ~4.5 s
//! budget belongs to subsystems later phases haven't built yet. What *can*
//! be measured honestly today is the L0/L1 slice this crate actually stands
//! up: the capability monitor, the scheduler's resource ledgers, and one
//! IPC endpoint. This module measures exactly that slice against 36's
//! "Privileged-core init" line (250 ms budget) and is explicit about what
//! it does not yet cover, rather than comparing against the full 4.5 s
//! figure and calling that a meaningful pass.

use std::time::{Duration, Instant};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_ipc::IpcBus;
use hyperion_scheduler::{ResourceDimension, ResourceLedger, Scheduler};

/// docs/36-performance-benchmarks.md §Performance Analysis: "Privileged-core
/// init | L0 | 250 ms | Capability monitor bootstrap, address-space init,
/// scheduling classes registered." This is the only budget line this
/// simulator's boot sequence can honestly be measured against today.
pub const PRIVILEGED_CORE_INIT_BUDGET: Duration = Duration::from_millis(250);

/// Phases of 36's full ~4.5 s cold-boot budget this simulator's `cold_boot`
/// actually exercises.
pub const COVERED_PHASES: &[&str] = &[
    "L0 privileged-core init (capability monitor bootstrap)",
    "L0/L1 scheduling classes / resource ledgers registered",
    "L1 one IPC endpoint stood up (driver bring-up stand-in)",
];

/// Phases 36 budgets for that this hosted simulator has no hardware,
/// storage engine, knowledge graph, model runtime, or compositor to measure
/// yet — listed explicitly so a passing `cold_boot` benchmark is never
/// mistaken for "Hyperion boots in under 5 seconds."
pub const NOT_YET_COVERED_PHASES: &[&str] = &[
    "pre-L0 firmware/bootloader handoff (no bare-metal target yet)",
    "L0/L1 full driver bring-up (HAL device-class enumeration; only one \
     IPC endpoint is stood up here, not real storage/display/input/network drivers)",
    "L2 platform services (Storage Engine mount, Plugin Framework, Event System — none exist yet)",
    "L3 knowledge layer attach (Knowledge Graph index, Semantic Filesystem mount)",
    "L4 cognition layer + resident model load (Context/Intent Engine, Local AI Runtime — Phase 3)",
    "L6 experience layer first frame (compositor, conversational shell — Phase 5)",
];

/// What one `cold_boot()` run produced.
#[derive(Debug, Clone, Copy)]
pub struct BootReport {
    pub elapsed: Duration,
    pub budget: Duration,
}

impl BootReport {
    pub fn within_budget(&self) -> bool {
        self.elapsed <= self.budget
    }
}

/// Stands up the L0/L1 slice of a Hyperion boot this workspace currently
/// implements: mint the root capability the rest of boot derives from,
/// register a representative Tier-2 (laptop-class, per
/// docs/37-scalability-roadmap.md's hardware table) resource ledger across
/// every `ResourceVector` dimension, and stand up one IPC endpoint. Returns
/// the wall-clock elapsed time measured against
/// [`PRIVILEGED_CORE_INIT_BUDGET`].
pub fn cold_boot() -> (BootReport, CapabilityMonitor, CapabilityToken) {
    let start = Instant::now();

    let mut monitor = CapabilityMonitor::new();
    let boot_root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(0), None);

    let mut scheduler = Scheduler::new();
    // Representative Tier-2 laptop profile (docs/37 §Performance Analysis):
    // 20-40 TOPS NPU/iGPU, 16-32 GB RAM, 512 GB-2 TB NVMe. Units are the
    // same coarse shares/mb/kbps ResourceVector already uses elsewhere in
    // this workspace, not calibrated hardware counters.
    scheduler.register_resource_provider(ResourceLedger::new(ResourceDimension::Cpu, 100, 20));
    scheduler.register_resource_provider(ResourceLedger::new(
        ResourceDimension::Ram,
        24_000,
        1_000,
    ));
    scheduler.register_resource_provider(ResourceLedger::new(ResourceDimension::Gpu, 100, 10));
    scheduler.register_resource_provider(ResourceLedger::new(ResourceDimension::Vram, 8_000, 500));
    scheduler.register_resource_provider(ResourceLedger::new(
        ResourceDimension::StorageIops,
        50_000,
        2_000,
    ));
    scheduler.register_resource_provider(ResourceLedger::new(
        ResourceDimension::NetworkBw,
        1_000_000,
        10_000,
    ));
    scheduler.register_resource_provider(ResourceLedger::new(
        ResourceDimension::InferenceTokens,
        40,
        5,
    ));
    scheduler.register_resource_provider(ResourceLedger::new(
        ResourceDimension::ContextWindowSlots,
        8_192,
        512,
    ));
    scheduler.register_resource_provider(ResourceLedger::new(
        ResourceDimension::Battery,
        60_000,
        2_000,
    ));

    let bus = IpcBus::new();
    let _endpoint_rx = bus.create_endpoint(boot_root.object_id());

    let elapsed = start.elapsed();
    (
        BootReport {
            elapsed,
            budget: PRIVILEGED_CORE_INIT_BUDGET,
        },
        monitor,
        boot_root,
    )
}
