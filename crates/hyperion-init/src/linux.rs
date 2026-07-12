//! Real PID 1 behavior: mount the essential pseudo-filesystems, print the boot banner, then bring
//! up the real supervision tree (M5) -- every Phase 2-10 subsystem this image ships, each spawned
//! as its own real, capability-scoped Trust Boundary process (M2), plus the real interactive
//! console (M7) folded into the same supervision tree, falling back to a plain debug shell if
//! that binary isn't present in this image.
//!
//! What this deliberately does *not* do yet (real gaps closed as their own milestone, not
//! silently ignored): handle `SIGINT`/`SIGTERM` for a clean reboot/halt path (the supervision
//! tree runs forever, matching a real init; a real halt path is a real, separate feature no
//! milestone has asked for yet), or reap arbitrary orphaned grandchildren reparented to PID 1
//! that aren't one of this supervisor's own tracked children (a real init eventually needs a
//! background reaper for exactly those; `hyperion_supervisor::Supervisor` only reaps its own).

mod display_probe;
mod network_probe;
mod storage_probe;
mod update_probe;

use std::ffi::CString;
use std::path::{Path, PathBuf};

use hyperion_capability::RightsMask;
use hyperion_supervisor::{ServiceScheduling, ServiceSpec, Supervisor};
use hyperion_trust_boundary::TrustDepth;

const SHELL_PATH: &str = "/bin/sh";
const CONSOLE_PATH: &str = "/usr/bin/hyperion-console";
const IPC_RENDEZVOUS_DIR: &str = "/run/hyperion/ipc";
/// Beneath which every supervised service's own `fs_scope` and cgroup live -- real, dedicated
/// directories a real root PID 1 owns outright, not shared with anything BusyBox or Buildroot's
/// own userspace uses.
const SERVICE_STATE_ROOT: &str = "/run/hyperion/services";

pub fn run() {
    print_banner();
    mount_essential_filesystems();
    run_supervision_tree();
}

fn print_banner() {
    println!();
    println!("================================================================");
    println!(" Hyperion");
    println!();
    println!(" Humans express goals.");
    println!(" Hyperion determines how those goals become reality.");
    println!("================================================================");
    println!();
    println!("[hyperion-init] pid 1 -- real supervision tree (M5)");
    println!();
}

struct MountSpec {
    source: &'static str,
    target: &'static str,
    fstype: &'static str,
    flags: libc::c_ulong,
}

fn mount_essential_filesystems() {
    for dir in ["/dev/pts", "/dev/shm", "/run"] {
        create_dir_if_missing(dir);
    }

    // Mirrors BusyBox's default inittab sysinit sequence (board/*/linux config in this repo
    // relies on the same convention): the kernel brings root up read-only unless told
    // otherwise on the cmdline, so the very first mount is always the rw remount.
    let specs = [
        MountSpec {
            source: "none",
            target: "/",
            fstype: "none",
            flags: libc::MS_REMOUNT,
        },
        MountSpec {
            source: "proc",
            target: "/proc",
            fstype: "proc",
            flags: 0,
        },
        MountSpec {
            source: "sysfs",
            target: "/sys",
            fstype: "sysfs",
            flags: 0,
        },
        MountSpec {
            source: "devpts",
            target: "/dev/pts",
            fstype: "devpts",
            flags: 0,
        },
        MountSpec {
            source: "tmpfs",
            target: "/dev/shm",
            fstype: "tmpfs",
            flags: 0,
        },
        MountSpec {
            source: "tmpfs",
            target: "/run",
            fstype: "tmpfs",
            flags: 0,
        },
    ];

    for spec in specs {
        let target = spec.target;
        let fstype = spec.fstype;
        if let Err(e) = do_mount(spec) {
            eprintln!("[hyperion-init] warning: mount {fstype} on {target} failed: {e}");
        }
    }

    // Must come *after* the loop above, not folded into `specs`: `/sys/fs/cgroup` only exists
    // once `/sys` itself is actually mounted -- creating it any earlier would create a plain
    // directory inside the root filesystem that the sysfs mount then masks, not the real
    // mountpoint `prepare_cgroup_parent` (and every real cgroup this image ever creates) needs.
    create_dir_if_missing("/sys/fs/cgroup");
    if let Err(e) = do_mount(MountSpec {
        source: "cgroup2",
        target: "/sys/fs/cgroup",
        fstype: "cgroup2",
        flags: 0,
    }) {
        eprintln!("[hyperion-init] warning: mount cgroup2 on /sys/fs/cgroup failed: {e}");
    }
}

fn create_dir_if_missing(path: &str) {
    match std::fs::create_dir(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => eprintln!("[hyperion-init] warning: mkdir {path} failed: {e}"),
    }
}

fn do_mount(spec: MountSpec) -> std::io::Result<()> {
    let source = CString::new(spec.source).expect("mount source has no interior NUL");
    let target = CString::new(spec.target).expect("mount target has no interior NUL");
    let fstype = CString::new(spec.fstype).expect("mount fstype has no interior NUL");

    // SAFETY: `source`, `target`, and `fstype` are valid, NUL-terminated C strings kept alive
    // for the duration of this call; `data` is null, which every fstype used here accepts.
    let rc = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            fstype.as_ptr(),
            spec.flags,
            std::ptr::null(),
        )
    };

    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Real root (which this process runs as at real boot) can create and configure cgroups anywhere
/// under `/sys/fs/cgroup` directly -- unlike `hyperion-cgroups`'s own dev-sandbox test, which
/// needs an already-delegated subtree because it runs unprivileged. Bootstraps a dedicated
/// `hyperion` subtree the same way systemd bootstraps its own at boot: enable the controllers
/// this system cares about at the true root, then again one level down for this subtree's own
/// children (cgroup v2 requires each level to opt its own children in explicitly -- the same
/// fact `hyperion-cgroups`'s own test discovered one level further down).
///
/// Best-effort: any failure here (no real root, cgroup v2 not mounted, an already-hardened image
/// that doesn't expose these knobs) returns `None` rather than aborting boot -- see
/// `hyperion_cgroups`'s own docs on why real scheduling weight degrades rather than blocks
/// startup. This specific real-root path cannot be exercised in an unprivileged dev sandbox (the
/// same kind of gap as M1's real-USB-boot criterion); the mechanism it calls
/// (`hyperion_cgroups::enforce_admission`) is the exact, already-live-tested mechanism
/// `hyperion-cgroups`'s own test proves one level further down a delegated tree.
fn prepare_cgroup_parent() -> Option<PathBuf> {
    let root = PathBuf::from("/sys/fs/cgroup");
    if !root.exists() {
        return None;
    }
    let hyperion_root = root.join("hyperion");
    let enable = |path: &Path| -> std::io::Result<()> {
        std::fs::write(path.join("cgroup.subtree_control"), "+cpu +memory +pids")
    };

    enable(&root).ok()?;
    std::fs::create_dir_all(&hyperion_root).ok()?;
    enable(&hyperion_root).ok()?;
    Some(hyperion_root)
}

/// Every Phase 2-10 subsystem this image ships as a real supervised service. Two real,
/// representative entries exist end to end today (see `hyperion-supervisor`'s own docs on why
/// two, not all ~30, prove the mechanism); each `program` path is where its Buildroot rootfs
/// package would place it once wired into `boot/scripts/build-image.sh`'s overlay step -- a real,
/// separate, purely mechanical follow-on this milestone doesn't attempt (see this crate's own M5
/// completion note). A path that doesn't exist in *this* built image is skipped with a clear
/// warning, not a boot failure: this list is expected to grow long before every entry in it ships
/// in every build.
fn phase_2_10_service_specs() -> Vec<ServiceSpec> {
    vec![
        service_spec(
            "observability",
            "/usr/lib/hyperion/services/hyperion-observability-service",
        ),
        service_spec(
            "explainability",
            "/usr/lib/hyperion/services/hyperion-explainability-service",
        ),
    ]
}

fn service_spec(name: &str, program: &str) -> ServiceSpec {
    let fs_scope = PathBuf::from(SERVICE_STATE_ROOT).join(name);
    let state_path = fs_scope.join("state.txt");
    ServiceSpec {
        name: name.to_string(),
        program: PathBuf::from(program),
        args: Vec::new(),
        rights: RightsMask::READ | RightsMask::WRITE,
        depth: TrustDepth::Process,
        fs_scope,
        scheduling: Some(ServiceScheduling {
            priority_weight: 1.0,
            request: hyperion_scheduler::ResourceVector {
                ram_mb: 64,
                ..Default::default()
            },
        }),
        extra_env: vec![(
            "HYPERION_STATE_FILE".to_string(),
            state_path.display().to_string(),
        )],
    }
}

fn run_supervision_tree() -> ! {
    // Real guest network bring-up (deferred gap named in M10, closed here): the kernel's own
    // real DHCP handshake (CONFIG_IP_PNP_DHCP + `ip=dhcp`) already completed before this process
    // ever started -- this just writes /etc/resolv.conf from its real result, so anything this
    // supervision tree spawns (M10's own web.research capability, most concretely) has real DNS
    // resolution from the moment it starts. Runs first: everything below spawns real processes
    // that may want real network access immediately.
    network_probe::write_resolv_conf_from_kernel_dhcp();

    // M7 stage 2: real DRM/KMS mode-set on a real display device, if one is attached (inert
    // otherwise -- see that module's own doc comment).
    display_probe::run_display_probe();

    // M6: mounts a real, dedicated persistent-storage partition if a second block device is
    // attached (inert -- returns None -- on every boot that doesn't have one, e.g. real hardware
    // without a data drive yet, or an ordinary single-disk QEMU dev-loop boot). The crash-
    // consistency probe only ever does anything when a real mounted partition exists to run it
    // against; the same directory (or a tmpfs fallback -- see `console_data_dir`) is where M7's
    // real console keeps its own Knowledge Graph.
    let data_partition = storage_probe::mount_data_partition();
    if let Some(data_dir) = &data_partition {
        // M13's probe runs first: it's opt-in and self-contained within one boot (see its own
        // doc comment), while M6's crash-consistency probe's own write loop is deliberately slow
        // (200k iterations, meant to still be mid-flight whenever storage-crash-test.sh's hard
        // kill lands) and would otherwise block this sequential boot path from ever reaching the
        // M13 probe on a fresh data disk. Neither probe's own state (a separate dedicated file
        // each) depends on the other's, so the order between them is otherwise arbitrary.
        update_probe::run_update_rollback_probe(data_dir);
        storage_probe::run_crash_consistency_probe(data_dir);
    }

    let cgroup_parent = prepare_cgroup_parent();
    if cgroup_parent.is_none() {
        eprintln!(
            "[hyperion-init] warning: no real delegated cgroup v2 root available -- supervised \
             services will run unweighted, not refusing to start"
        );
    }

    let mut supervisor = Supervisor::new(IPC_RENDEZVOUS_DIR, cgroup_parent).unwrap_or_else(|e| {
        // The IPC rendezvous directory living under /run (a tmpfs this same function just
        // mounted) failing to be created is not a resource-control nicety like the cgroup case
        // above -- it means this process cannot supervise anything at all. Fail loudly rather
        // than silently run with no supervision tree, matching M2's own "never silently downgrade
        // a caller that needs to know" precedent.
        panic!("cannot start the real supervision tree: {e}")
    });

    for spec in phase_2_10_service_specs() {
        if !spec.program.exists() {
            println!(
                "[hyperion-init] skipping {:?}: {:?} is not present in this image (not yet \
                 wired into the Buildroot rootfs overlay)",
                spec.name, spec.program
            );
            continue;
        }
        if let Err(e) = std::fs::create_dir_all(&spec.fs_scope) {
            eprintln!(
                "[hyperion-init] warning: failed to create fs_scope for {:?}: {e}",
                spec.name
            );
            continue;
        }
        let name = spec.name.clone();
        match supervisor.spawn_sandboxed(spec) {
            Ok(()) => println!("[hyperion-init] started {name}"),
            Err(e) => eprintln!("[hyperion-init] warning: failed to start {name}: {e}"),
        }
    }

    adopt_interactive_process(&mut supervisor, data_partition.as_deref());

    println!("[hyperion-init] supervision tree running");
    supervisor.run_forever();
}

/// Where M7's real console keeps its own Knowledge Graph: M6's real, dedicated data partition
/// when one is mounted, or a real (if ephemeral) tmpfs directory otherwise -- the console still
/// needs *somewhere* real to open a `KnowledgeGraph` against even on a boot with no second disk
/// attached (e.g. `boot/scripts/run-qemu.sh`'s default single-disk invocation, or real hardware
/// not yet given a data drive), the same best-effort shape as this crate's own cgroup bootstrap.
const CONSOLE_FALLBACK_DATA_DIR: &str = "/run/hyperion/console";

fn console_data_dir(data_partition: Option<&Path>) -> PathBuf {
    if let Some(dir) = data_partition {
        return dir.to_path_buf();
    }
    if let Err(e) = std::fs::create_dir_all(CONSOLE_FALLBACK_DATA_DIR) {
        eprintln!(
            "[hyperion-init] warning: couldn't create the console's fallback data dir \
             {CONSOLE_FALLBACK_DATA_DIR}: {e}"
        );
    }
    PathBuf::from(CONSOLE_FALLBACK_DATA_DIR)
}

/// Folds the real interactive surface into the same supervision tree as every capability-scoped
/// service (see `hyperion_supervisor::Supervisor::adopt_plain`'s own docs on why a second,
/// independent wait loop for it would be unsafe). M7 replaces M1's placeholder debug shell with
/// `hyperion-console`, the real Intent -> Agent -> Workspace text loop -- "the first milestone
/// where booting Hyperion does something a person can actually use" is exactly what this crate's
/// PID 1 now spawns onto the interactive terminal, not a generic shell. Neither the console nor
/// the plain shell it falls back to (if the console binary isn't present in this image, e.g. an
/// older build) is capability-scoped the way a Phase 2-10 service is: both fundamentally need
/// broad, real-time interactive I/O access, the same reasoning M1/M5 already applied to the debug
/// shell -- a real, properly capability-scoped interactive surface is a further, separate
/// refinement M7 stage 2 (a real compositor) would need, not this stage.
fn adopt_interactive_process(supervisor: &mut Supervisor, data_partition: Option<&Path>) {
    let data_dir = console_data_dir(data_partition);
    match spawn_console(&data_dir) {
        Ok(pid) => {
            supervisor.adopt_plain("console", pid, move || spawn_console(&data_dir));
            return;
        }
        Err(e) => eprintln!(
            "[hyperion-init] warning: failed to start the real console ({e}) -- falling back to \
             a plain debug shell"
        ),
    }

    match spawn_shell() {
        Ok(pid) => supervisor.adopt_plain("debug-shell", pid, spawn_shell_retry),
        Err(e) => eprintln!("[hyperion-init] warning: failed to start the debug shell: {e}"),
    }
}

/// A `candle`-enabled console's real `hf-hub` on-disk cache, pre-baked into a real image at build
/// time (not present at all in an ordinary, mock-backend image -- inert, same as
/// `display_probe`/`storage_probe`'s own "only wire up what's actually here" convention). Real
/// Hugging Face Hub downloads (`CandleBackend::load`) resolve against this path if it's present,
/// letting a pinned-commit model already baked in here (see
/// `hyperion_ai_runtime::candle_backend`'s own `TINYLLAMAS_REVISION` doc comment) load with zero
/// network access.
const HF_CACHE_DIR: &str = "/usr/share/hyperion/hf-cache/hub";
/// A real Mozilla CA root bundle, pre-baked alongside [`HF_CACHE_DIR`] for the same reason: this
/// rootfs ships no `ca-certificates` package, but `hf-hub`'s own HTTP client
/// (`rustls-platform-verifier`) builds a real TLS trust store *unconditionally at client
/// construction time* -- before any cache lookup ever runs, so a pre-baked model cache alone
/// isn't enough; an empty trust store makes client construction itself fail outright. Pointed at
/// via `SSL_CERT_FILE`, which `rustls-native-certs` (the crate `rustls-platform-verifier` uses on
/// Linux) reads before falling back to any hardcoded distro-specific path.
const CA_BUNDLE_PATH: &str = "/usr/share/hyperion/tls/ca-certificates.crt";

/// Forks and execs the real console, returning immediately with its pid -- reaping and
/// restart-on-exit is `hyperion_supervisor::Supervisor`'s single, unified job (see that crate's
/// own docs on why a second waiter would race it). Not attempted at all if the binary isn't
/// present in this image (not yet wired into an older Buildroot overlay build): the caller falls
/// back to a plain shell rather than treating a missing optional binary as a boot failure.
fn spawn_console(data_dir: &Path) -> std::io::Result<libc::pid_t> {
    if !Path::new(CONSOLE_PATH).exists() {
        return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
    }
    let mut extra_env = vec![("HYPERION_CONSOLE_DATA_DIR", data_dir.display().to_string())];
    if Path::new(HF_CACHE_DIR).exists() {
        extra_env.push(("HF_HUB_CACHE", HF_CACHE_DIR.to_string()));
    }
    if Path::new(CA_BUNDLE_PATH).exists() {
        extra_env.push(("SSL_CERT_FILE", CA_BUNDLE_PATH.to_string()));
    }
    spawn_interactive(CONSOLE_PATH, &extra_env)
}

fn spawn_shell_retry() -> std::io::Result<libc::pid_t> {
    spawn_shell()
}

fn spawn_shell() -> std::io::Result<libc::pid_t> {
    spawn_interactive(SHELL_PATH, &[])
}

/// Forks and execs `path` with `extra_env` set, claiming a real controlling terminal for it --
/// the shared mechanism behind both the real console and the plain debug-shell fallback. Never
/// blocks waiting for the child to exit (unlike M1's original `spawn_and_wait`): reaping and
/// restart-on-exit is `hyperion_supervisor::Supervisor`'s job now.
fn spawn_interactive(path: &str, extra_env: &[(&str, String)]) -> std::io::Result<libc::pid_t> {
    let c_path = CString::new(path).expect("interactive process path has no interior NUL");
    let argv: [*const libc::c_char; 2] = [c_path.as_ptr(), std::ptr::null()];

    // SAFETY: fork() duplicates the process; the child branch only calls async-signal-safe
    // functions (setsid, ioctl, execv, exit) before either replacing itself or exiting, so it
    // never returns into Rust-level state shared with the parent.
    let pid = unsafe { libc::fork() };
    match pid.cmp(&0) {
        std::cmp::Ordering::Less => Err(std::io::Error::last_os_error()),
        std::cmp::Ordering::Equal => {
            // The child inherits fd 0/1/2 = /dev/console from PID 1, but not a *controlling*
            // terminal -- without claiming one, it has no job control. setsid() makes the child a
            // new session leader with no controlling terminal, then TIOCSCTTY claims fd 0 as one,
            // the same sequence a getty performs.
            //
            // SAFETY: setsid/ioctl are async-signal-safe and valid to call here; errors are
            // deliberately ignored (worst case is no job control, not a boot failure).
            unsafe {
                libc::setsid();
                libc::ioctl(0, libc::TIOCSCTTY as _, 0);
            }
            // set_var before exec is inherited by the exec'd program; execv (unlike execve)
            // carries no separate envp of its own, so this is the simplest way to hand the real
            // console its one real config value without hand-building a raw environment array.
            for (key, value) in extra_env {
                std::env::set_var(key, value);
            }
            // SAFETY: c_path and argv are valid for this call, which does not return on success.
            unsafe {
                libc::execv(c_path.as_ptr(), argv.as_ptr());
            }
            // execv only returns on failure.
            std::process::exit(127);
        }
        std::cmp::Ordering::Greater => Ok(pid),
    }
}
