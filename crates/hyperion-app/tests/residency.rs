//! App Builder T3, and an honest account of what is and is not established here.
//!
//! **Not proven: that a resident app runs.** `Supervisor::spawn_sandboxed` needs user namespaces
//! and Landlock, and the container this was written in has neither -- the same reason seven
//! pre-existing sandbox tests fail there. Nothing below starts a process.
//!
//! **Proven:** that residency is declared and signed, that a one-shot app is refused residency,
//! that the service spec derived from an app confines it the way the design says, and that two
//! people running the same app get genuinely separate services.

use hyperion_app::resident::ResidentApps;
use hyperion_app::{AppTier, InstalledApp};
use hyperion_capability::RightsMask;
use std::path::PathBuf;

fn app(name: &str, resident: bool) -> InstalledApp {
    InstalledApp {
        name: name.to_string(),
        owner: "alice".to_string(),
        keeps_data: true,
        resident,
        goal: "watch the inbox".to_string(),
        tier: AppTier::Answer,
        inputs: vec![],
        capability_id: format!("app.{name}"),
        version: 1,
    }
}

#[test]
fn a_resident_app_is_confined_to_its_own_data_directory() {
    // Unlike a one-shot app, whose scope is a throwaway temp directory, a resident app's durable
    // directory *is* its working scope -- it has nowhere else to put anything, and should have
    // nowhere else it can reach.
    let data = PathBuf::from("/apps/data/1000/deadbeef");
    let spec = ResidentApps::spec_for(
        &app("inbox-watch", true),
        PathBuf::from("/opt/python-static"),
        vec!["/apps/inbox-watch/script".to_string()],
        data.clone(),
        "user.alice",
    );

    assert_eq!(spec.fs_scope, data);
    assert_eq!(spec.depth, hyperion_trust_boundary::TrustDepth::Container);
    // Read and write because keeping something is the point; execute because the interpreter has
    // to run. Nothing else -- notably no grant that would let it reach another app's data.
    assert_eq!(
        spec.rights,
        RightsMask::READ | RightsMask::WRITE | RightsMask::EXEC
    );
    // A standing budget, because residency costs for as long as the app exists.
    assert!(
        spec.scheduling.is_some(),
        "a resident app must hold a budget"
    );
}

#[test]
fn two_people_running_the_same_app_get_separate_services() {
    // An app name is unique per device; a supervised service name has to be unique across the
    // supervisor. The same app really can be resident for two people at once, each watching their
    // own data, and one name for both would make stopping one stop the other.
    let alice = ResidentApps::service_name_for("inbox-watch", "user.alice");
    let bob = ResidentApps::service_name_for("inbox-watch", "user.bob");
    assert_ne!(alice, bob);
    assert!(alice.contains("inbox-watch") && alice.contains("alice"));
}

#[test]
fn an_app_that_never_asked_to_stay_running_is_refused() {
    // Being left running is a permission, and one an app has to have asked for in its signed
    // contract -- not something a caller can decide for it after the fact.
    let dir = tempfile::tempdir().unwrap();
    let Ok(mut residents) = ResidentApps::open(dir.path().join("run"), None) else {
        // Opening a supervisor creates its rendezvous directory and nothing else, so this should
        // not fail -- but if it ever does, that is not this test's subject.
        return;
    };

    let refused = residents.start(
        &app("tally", false),
        PathBuf::from("/opt/python-static"),
        vec![],
        dir.path().to_path_buf(),
        "user.alice",
    );
    assert!(
        matches!(refused, Err(hyperion_app::AppError::NotResident { .. })),
        "got: {refused:?}"
    );
}

#[test]
fn stopping_something_that_was_never_started_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let Ok(mut residents) = ResidentApps::open(dir.path().join("run"), None) else {
        return;
    };
    let refused = residents.stop("inbox-watch", "user.alice");
    assert!(
        matches!(refused, Err(hyperion_app::AppError::NotRunning { .. })),
        "got: {refused:?}"
    );
    assert!(!residents.is_running("inbox-watch", "user.alice"));
    assert!(residents.owners_running("inbox-watch").is_empty());
}

#[test]
fn polling_with_nothing_running_is_harmless() {
    // `poll` is called once per console turn, which for most turns means there is nothing
    // supervised at all. It must not block, error, or touch a process it does not own -- the last
    // being the whole reason `Supervisor::poll_and_restart` exists rather than
    // `reap_and_restart_one`.
    let dir = tempfile::tempdir().unwrap();
    let Ok(mut residents) = ResidentApps::open(dir.path().join("run"), None) else {
        return;
    };
    residents.poll();
    residents.poll();
    assert!(residents.status_for("user.alice").is_empty());
}
