//! A service that keeps crash-looping is not respawned forever: the supervisor gives up after a
//! bounded number of consecutive fast failures rather than spinning indefinitely. Uses
//! `adopt_plain` with a real, minimal, always-immediately-exiting process (`/bin/false`) rather
//! than a full sandboxed service build -- this is a process-lifecycle policy test, orthogonal to
//! M2's own real Trust Boundary sandboxing, which `tests/real_supervision.rs` already covers.
#![cfg(target_os = "linux")]

use std::process::Command;
use std::sync::Mutex;

use hyperion_supervisor::{GiveUpReason, Supervisor, SupervisorError};

fn spawn_immediately_exiting_process() -> std::io::Result<libc::pid_t> {
    Command::new("false")
        .spawn()
        .map(|child| child.id() as libc::pid_t)
}

/// `Supervisor::reap_and_restart_one`'s blocking `waitpid(-1, ...)` reaps *any* child of this
/// process, not just its own -- exactly the single-wait-queue hazard this crate's own module doc
/// already names for a *production* supervisor, recurring here across two `#[test]` functions
/// that `cargo test` otherwise runs concurrently as separate threads of the same process. Without
/// this, one test's own `reap_and_restart_one` can reap the other's still-live child, and the
/// second test's own later `waitpid` call then fails with a real `ECHILD` ("No child processes").
/// `unwrap_or_else` recovers from lock poisoning so one test panicking while holding it doesn't
/// spuriously fail the other.
static REAPER_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn a_service_that_keeps_crash_looping_is_given_up_on_rather_than_restarted_forever() {
    let _guard = REAPER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().expect("create a real tempdir for the IPC rendezvous dir");
    let mut supervisor =
        Supervisor::new(root.path().join("ipc"), None).expect("create the real supervisor");

    let initial_pid =
        spawn_immediately_exiting_process().expect("spawn the real, always-exiting process");
    supervisor.adopt_plain("flaky", initial_pid, spawn_immediately_exiting_process);

    // Keep reaping and restarting; every exit here is fast (the process exits essentially
    // immediately), so the supervisor should give up well before this bound -- generous so the
    // test fails loudly (not by hanging) if the give-up policy regresses.
    let mut outcome = None;
    for _ in 0..20 {
        match supervisor.reap_and_restart_one() {
            Ok(_) => continue,
            Err(e) => {
                outcome = Some(e);
                break;
            }
        }
    }

    let Some(SupervisorError::GaveUp {
        name,
        restart_count,
        reason,
    }) = outcome
    else {
        panic!("expected the supervisor to give up with SupervisorError::GaveUp, got {outcome:?}");
    };
    assert_eq!(name, "flaky");
    assert_eq!(reason, GiveUpReason::CrashLoop);

    let given_up = supervisor
        .given_up("flaky")
        .expect("a given-up service must be queryable by name");
    assert_eq!(given_up.name, "flaky");
    assert_eq!(given_up.reason, GiveUpReason::CrashLoop);
    assert_eq!(given_up.restart_count, restart_count);

    assert!(
        supervisor.pid_of("flaky").is_none(),
        "a given-up service must no longer be tracked as running"
    );
    assert_eq!(
        supervisor.given_up_services().len(),
        1,
        "exactly the one service given up should appear in the aggregate query"
    );
}

#[test]
fn a_service_that_recovers_before_the_give_up_threshold_keeps_being_restarted() {
    let _guard = REAPER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().expect("create a real tempdir for the IPC rendezvous dir");
    let mut supervisor =
        Supervisor::new(root.path().join("ipc"), None).expect("create the real supervisor");

    let initial_pid =
        spawn_immediately_exiting_process().expect("spawn the real, always-exiting process");
    supervisor.adopt_plain(
        "occasionally-flaky",
        initial_pid,
        spawn_immediately_exiting_process,
    );

    // Two fast failures -- well under the give-up threshold -- must still be ordinary restarts,
    // not a give-up.
    for _ in 0..2 {
        let name = supervisor
            .reap_and_restart_one()
            .expect("a service that hasn't crossed the give-up threshold keeps being restarted");
        assert_eq!(name, "occasionally-flaky");
    }
    assert!(supervisor.given_up("occasionally-flaky").is_none());
    assert!(supervisor.pid_of("occasionally-flaky").is_some());
}
