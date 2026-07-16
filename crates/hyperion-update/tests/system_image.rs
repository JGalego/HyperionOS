//! docs/32 §5's A/B slot machine: staging never touches the active slot,
//! boot-attempt exhaustion is the revert signal, and a real bootloader
//! restore_to path never runs for this track.

use hyperion_update::{SystemImageController, SystemImageSlotName, UpdateError};

#[test]
fn staging_does_not_disturb_the_active_slot() {
    let controller = SystemImageController::new(1);
    let staged = controller.stage_to_inactive_slot(2).unwrap();
    assert_eq!(staged, SystemImageSlotName::B);

    let active = controller.active_slot();
    assert_eq!(active.slot, SystemImageSlotName::A);
    assert_eq!(active.version, 1);
}

#[test]
fn a_successful_boot_commit_flips_the_active_slot() {
    let controller = SystemImageController::new(1);
    let staged = controller.stage_to_inactive_slot(2).unwrap();
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
    let staged = controller.stage_to_inactive_slot(2).unwrap();

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
    let staged = controller.stage_to_inactive_slot(2).unwrap();
    controller.attempt_boot(staged).unwrap();
    controller.attempt_boot(staged).unwrap();
    controller.attempt_boot(staged).unwrap();
    assert!(controller.attempt_boot(staged).is_err());

    // Re-stage a fresh version into the same (still-inactive) slot.
    let restaged = controller.stage_to_inactive_slot(3).unwrap();
    assert_eq!(restaged, staged);
    assert!(
        controller.attempt_boot(restaged).is_ok(),
        "a fresh staging cycle must reset the exhausted boot-attempt counter"
    );
}

#[test]
fn the_normal_path_refuses_to_stage_a_version_at_or_below_the_high_water_mark() {
    let controller = SystemImageController::new(5);
    assert_eq!(controller.highest_version_ever(), 5);

    let same_version = controller.stage_to_inactive_slot(5);
    assert!(matches!(
        same_version,
        Err(UpdateError::AntiRollbackViolation {
            attempted: 5,
            highest_ever: 5
        })
    ));

    let older_version = controller.stage_to_inactive_slot(3);
    assert!(matches!(
        older_version,
        Err(UpdateError::AntiRollbackViolation {
            attempted: 3,
            highest_ever: 5
        })
    ));
}

#[test]
fn the_normal_path_really_advances_the_high_water_mark_on_success() {
    let controller = SystemImageController::new(1);
    controller.stage_to_inactive_slot(2).unwrap();
    assert_eq!(controller.highest_version_ever(), 2);

    controller.stage_to_inactive_slot(7).unwrap();
    assert_eq!(controller.highest_version_ever(), 7);
}

#[test]
fn the_rollback_path_can_stage_an_older_version_without_lowering_the_high_water_mark() {
    let controller = SystemImageController::new(1);
    controller.stage_to_inactive_slot(10).unwrap();
    assert_eq!(controller.highest_version_ever(), 10);

    // The explicit, audited rollback path -- allowed to go backward. Stages into whichever slot
    // is currently inactive (B, since A is the initial active slot).
    let rolled_back = controller.stage_rollback_to_inactive_slot(5);
    assert_eq!(rolled_back, SystemImageSlotName::B);
    assert_eq!(
        controller.highest_version_ever(),
        10,
        "a real rollback must never lower the real anti-rollback high-water-mark"
    );
}

#[test]
fn replaying_an_old_vulnerable_image_through_the_normal_path_is_refused_even_right_after_a_real_rollback(
) {
    let controller = SystemImageController::new(1);
    controller.stage_to_inactive_slot(10).unwrap();
    controller.stage_rollback_to_inactive_slot(5);

    // An attacker re-flashing that same old, vulnerable, still-validly-signed image through the
    // *normal* path -- not the audited rollback one -- must still be refused, per docs/32's own
    // "downgrade is only permitted through the explicit, audited update_rollback path, never
    // through re-flashing an old signed image directly."
    let replay_attempt = controller.stage_to_inactive_slot(5);
    assert!(matches!(
        replay_attempt,
        Err(UpdateError::AntiRollbackViolation {
            attempted: 5,
            highest_ever: 10
        })
    ));
}
