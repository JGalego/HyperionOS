//! docs/32 §5's A/B slot machine: staging never touches the active slot,
//! boot-attempt exhaustion is the revert signal, and a real bootloader
//! restore_to path never runs for this track.

use hyperion_update::{SystemImageController, SystemImageSlotName, UpdateError};

#[test]
fn staging_does_not_disturb_the_active_slot() {
    let controller = SystemImageController::new(1);
    let staged = controller.stage_to_inactive_slot(2);
    assert_eq!(staged, SystemImageSlotName::B);

    let active = controller.active_slot();
    assert_eq!(active.slot, SystemImageSlotName::A);
    assert_eq!(active.version, 1);
}

#[test]
fn a_successful_boot_commit_flips_the_active_slot() {
    let controller = SystemImageController::new(1);
    let staged = controller.stage_to_inactive_slot(2);
    controller.attempt_boot(staged).unwrap();
    controller.commit(staged);

    let active = controller.active_slot();
    assert_eq!(active.slot, SystemImageSlotName::B);
    assert_eq!(active.version, 2);
    assert!(active.committed);
}

#[test]
fn exhausting_boot_attempts_never_commits_leaving_the_prior_slot_active() {
    let controller = SystemImageController::new(1);
    let staged = controller.stage_to_inactive_slot(2);

    for _ in 0..3 {
        controller.attempt_boot(staged).unwrap();
    }
    let result = controller.attempt_boot(staged);
    assert!(matches!(result, Err(UpdateError::BootAttemptsExhausted)));

    // No commit() was ever called — the original slot is still active.
    let active = controller.active_slot();
    assert_eq!(active.slot, SystemImageSlotName::A);
    assert_eq!(active.version, 1);
}

#[test]
fn a_second_staging_cycle_resets_the_boot_attempt_counter() {
    let controller = SystemImageController::new(1);
    let staged = controller.stage_to_inactive_slot(2);
    controller.attempt_boot(staged).unwrap();
    controller.attempt_boot(staged).unwrap();
    controller.attempt_boot(staged).unwrap();
    assert!(controller.attempt_boot(staged).is_err());

    // Re-stage a fresh version into the same (still-inactive) slot.
    let restaged = controller.stage_to_inactive_slot(3);
    assert_eq!(restaged, staged);
    assert!(
        controller.attempt_boot(restaged).is_ok(),
        "a fresh staging cycle must reset the exhausted boot-attempt counter"
    );
}
