//! Proves M5's exit criterion for real: "killing any one supervised Phase 2-10 service process
//! results in it being restarted with a fresh capability grant within a bounded time, without a
//! full reboot and without crashing sibling services." Two real, independent Phase 2-10
//! subsystems (`hyperion-observability`, `hyperion-explainability`) run as real, capability-scoped,
//! Landlock/seccomp-sandboxed processes (M2); killing one is proven, from outside, to produce a
//! new real pid *and* a new real capability `token_id` for exactly that service, while the other's
//! pid/token/restart-count are untouched throughout.
//!
//! Linux-only (`Supervisor` itself only exists there -- see hyperion-supervisor's own lib.rs),
//! same as this crate's real Landlock/seccomp-sandboxed subject matter.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hyperion_capability::RightsMask;
use hyperion_supervisor::{ServiceSpec, Supervisor};
use hyperion_trust_boundary::TrustDepth;

const MUSL_TARGET: &str = "x86_64-unknown-linux-musl";

/// Builds `bin_name` (owned by `crate_name`) for the musl target and returns its path. A
/// dynamically linked binary needs to *read* the host's dynamic linker/libc from outside any
/// Landlock `fs_scope` at `execve()` time, which fails under M2's real enforcement -- a statically
/// linked musl build needs nothing outside its own path to start. Same reasoning, and the same
/// pattern, as `hyperion-trust-boundary/tests/enforcement.rs`'s own `probe_bin()` helper.
fn service_bin(crate_name: &str, bin_name: &str) -> PathBuf {
    let status = Command::new("cargo")
        .args([
            "build",
            "--target",
            MUSL_TARGET,
            "--bin",
            bin_name,
            "-p",
            crate_name,
        ])
        .status()
        .expect("run cargo build for the musl service binary");
    assert!(status.success(), "building {bin_name} for musl failed");

    workspace_root()
        .join("target")
        .join(MUSL_TARGET)
        .join("debug")
        .join(bin_name)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/hyperion-supervisor has a workspace root two levels up")
        .to_path_buf()
}

/// Each service's state file is written asynchronously after it starts (and rewritten after every
/// respawn); poll briefly rather than asserting on a fixed sleep, matching M2's own test's
/// approach to the same kind of real, async, cross-process signal.
fn wait_for_state_containing(path: &Path, needle: &str, timeout: Duration) -> String {
    let start = Instant::now();
    let mut last_seen = String::new();
    while start.elapsed() < timeout {
        if let Ok(contents) = std::fs::read_to_string(path) {
            if contents.contains(needle) {
                return contents;
            }
            last_seen = contents;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "timed out waiting for {path:?} to contain {needle:?}; last seen contents: \
         {last_seen:?}"
    );
}

fn is_alive(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 sends nothing; it only reports whether the pid is signalable.
    unsafe { libc::kill(pid, 0) == 0 }
}

struct ServiceFixture {
    spec: ServiceSpec,
    state_path: PathBuf,
}

fn build_fixture(
    root: &Path,
    crate_name: &str,
    bin_name: &str,
    service_name: &str,
) -> ServiceFixture {
    let bin = service_bin(crate_name, bin_name);
    let fs_scope = root.join(service_name);
    std::fs::create_dir_all(&fs_scope).expect("create this service's own fs_scope directory");
    let state_path = fs_scope.join("state.txt");

    let spec = ServiceSpec {
        name: service_name.to_string(),
        program: bin,
        args: Vec::new(),
        rights: RightsMask::READ | RightsMask::WRITE,
        depth: TrustDepth::Process,
        fs_scope: fs_scope.clone(),
        scheduling: None,
        extra_env: vec![(
            "HYPERION_STATE_FILE".to_string(),
            state_path.display().to_string(),
        )],
    };

    ServiceFixture { spec, state_path }
}

#[test]
fn killing_one_supervised_service_gets_a_fresh_grant_without_touching_its_sibling() {
    let root = tempfile::tempdir().expect("create a real tempdir for this test's fs_scopes");
    let ipc_dir = root.path().join("ipc");

    let observability = build_fixture(
        root.path(),
        "hyperion-observability",
        "hyperion-observability-service",
        "observability",
    );
    let explainability = build_fixture(
        root.path(),
        "hyperion-explainability",
        "hyperion-explainability-service",
        "explainability",
    );

    let mut supervisor =
        Supervisor::new(ipc_dir, None).expect("create the real supervisor + IPC rendezvous dir");
    supervisor
        .spawn_sandboxed(observability.spec.clone())
        .expect("spawn the real observability service");
    supervisor
        .spawn_sandboxed(explainability.spec.clone())
        .expect("spawn the real explainability service");

    let obs_pid_1 = supervisor
        .pid_of("observability")
        .expect("observability is tracked right after spawning it");
    let exp_pid_1 = supervisor
        .pid_of("explainability")
        .expect("explainability is tracked right after spawning it");
    assert_ne!(
        obs_pid_1, exp_pid_1,
        "sanity: the two services are really two separate processes"
    );

    let obs_token_1 = supervisor
        .token_id_of("observability")
        .expect("a sandboxed service always has a live token id");
    let exp_token_1 = supervisor
        .token_id_of("explainability")
        .expect("a sandboxed service always has a live token id");
    assert_ne!(
        obs_token_1, exp_token_1,
        "sanity: the two services really got two distinct minted tokens"
    );

    // Both services do one real, capability-checked unit of their own crate's actual work before
    // idling; wait for that real, async, cross-process side effect, then confirm it really
    // reflects the grant this process was actually told about via HYPERION_WIRE_TOKEN.
    let obs_state_1 = wait_for_state_containing(
        &observability.state_path,
        &format!("spawn_token_id={obs_token_1}"),
        Duration::from_secs(10),
    );
    assert!(
        obs_state_1.contains(&format!("pid={obs_pid_1}")),
        "observability's own state file should record its own real pid: {obs_state_1:?}"
    );
    let exp_state_1 = wait_for_state_containing(
        &explainability.state_path,
        &format!("spawn_token_id={exp_token_1}"),
        Duration::from_secs(10),
    );
    assert!(
        exp_state_1.contains(&format!("pid={exp_pid_1}")),
        "explainability's own state file should record its own real pid: {exp_state_1:?}"
    );

    // Kill observability specifically -- a real SIGKILL to a real process, not a simulated fault.
    // SAFETY: obs_pid_1 is a real, currently-alive process this supervisor spawned.
    let killed = unsafe { libc::kill(obs_pid_1, libc::SIGKILL) };
    assert_eq!(killed, 0, "the real SIGKILL to observability must succeed");

    let restarted_name = supervisor
        .reap_and_restart_one()
        .expect("reap the real exit and respawn exactly that one service");
    assert_eq!(
        restarted_name, "observability",
        "the service that actually exited is the one that should be restarted"
    );

    // The dead pid must really be gone, not just reassigned in this process's bookkeeping.
    assert!(
        !is_alive(obs_pid_1),
        "the killed process's original pid must no longer exist as a real process"
    );

    let obs_pid_2 = supervisor
        .pid_of("observability")
        .expect("observability is tracked again immediately after its restart");
    let obs_token_2 = supervisor
        .token_id_of("observability")
        .expect("the respawned instance has its own live token id");
    assert_ne!(
        obs_pid_2, obs_pid_1,
        "a real respawn is a real fork -- a new pid, never the dead one"
    );
    assert_ne!(
        obs_token_2, obs_token_1,
        "M5's exit criterion: restarted with a *fresh* capability grant, never the stale token_id"
    );
    assert_eq!(supervisor.restart_count_of("observability"), Some(1));

    // Sibling isolation: explainability's pid, token, and restart count must be completely
    // untouched by observability's crash and respawn.
    assert_eq!(
        supervisor.pid_of("explainability"),
        Some(exp_pid_1),
        "without crashing sibling services -- explainability's real pid must be unchanged"
    );
    assert_eq!(
        supervisor.token_id_of("explainability"),
        Some(exp_token_1),
        "explainability's capability grant must be unaffected by observability's restart"
    );
    assert_eq!(supervisor.restart_count_of("explainability"), Some(0));

    // The respawned *process itself* -- not just the supervisor's own bookkeeping -- must have
    // really received and used the fresh grant: its rewritten state file should show the new
    // token id, at the new pid, and the stale token id must be gone from it.
    let obs_state_2 = wait_for_state_containing(
        &observability.state_path,
        &format!("spawn_token_id={obs_token_2}"),
        Duration::from_secs(10),
    );
    assert!(
        obs_state_2.contains(&format!("pid={obs_pid_2}")),
        "the respawned instance's state file should show its own new real pid: {obs_state_2:?}"
    );
    assert!(
        !obs_state_2.contains(&format!("spawn_token_id={obs_token_1}\n")),
        "the respawned instance's state file must not still show the stale token id: \
         {obs_state_2:?}"
    );

    // No explicit cleanup call needed: `Supervisor`'s own `Drop` kills and reaps every remaining
    // real process when `supervisor` goes out of scope here, including on a panic unwind from any
    // assertion above it.
}

/// A third, independent real Phase 2-10 subsystem (`hyperion-privacy`) run the same real,
/// capability-scoped, sandboxed way -- proving M5's supervision mechanism generalizes rather than
/// being coincidentally correct for the first two crates it was proven against. Doesn't repeat
/// the full kill+respawn choreography above (already proven generically, crate-agnostically,
/// there); this test's own real, distinct value is that `hyperion-privacy`'s own real routing
/// algorithm (`route_capability_call`) genuinely reflects a real, just-requested consent grant
/// from *inside* a real sandboxed process, not merely that some process started.
#[test]
fn a_third_independent_subsystem_runs_under_real_supervision_and_does_its_own_real_work() {
    let root = tempfile::tempdir().expect("create a real tempdir for this test's fs_scopes");
    let ipc_dir = root.path().join("ipc");

    let privacy = build_fixture(
        root.path(),
        "hyperion-privacy",
        "hyperion-privacy-service",
        "privacy",
    );

    let mut supervisor =
        Supervisor::new(ipc_dir, None).expect("create the real supervisor + IPC rendezvous dir");
    supervisor
        .spawn_sandboxed(privacy.spec.clone())
        .expect("spawn the real privacy service");

    let token_id = supervisor
        .token_id_of("privacy")
        .expect("a sandboxed service always has a live token id");
    let pid = supervisor
        .pid_of("privacy")
        .expect("privacy is tracked right after spawning it");

    let state = wait_for_state_containing(
        &privacy.state_path,
        &format!("spawn_token_id={token_id}"),
        Duration::from_secs(10),
    );
    assert!(
        state.contains(&format!("pid={pid}")),
        "privacy's own state file should record its own real pid: {state:?}"
    );
    assert!(
        state.contains("routing_decision=DispatchCloud"),
        "a real consent grant this instance itself requested must really flip \
         route_capability_call's own decision from Degraded to DispatchCloud: {state:?}"
    );
}
