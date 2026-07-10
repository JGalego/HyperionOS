//! docs/41-implementation-phases.md's Phase 1 exit criterion: "cold boot is
//! measured (not yet optimized) against 36's budget." See
//! hyperion_sim::boot's module docs for exactly which slice of 36's full
//! budget this measures and which phases are still out of scope.

#[test]
fn cold_boot_is_measured_and_currently_within_the_privileged_core_init_budget() {
    let (report, monitor, root) = hyperion_sim::cold_boot();

    assert_eq!(report.budget, hyperion_sim::PRIVILEGED_CORE_INIT_BUDGET);
    assert!(
        report.within_budget(),
        "L0/L1 boot slice took {:?}, over the {:?} budget — this is a real regression signal \
         even though the rest of 36's cold-boot budget isn't implemented yet",
        report.elapsed,
        report.budget,
    );

    // The boot sequence must leave behind a usable, live root capability —
    // booting successfully but handing back a dead token would be a
    // contradiction in terms.
    assert!(monitor.is_live(&root));
}
