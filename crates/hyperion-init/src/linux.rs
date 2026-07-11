//! Real PID 1 behavior: mount the essential pseudo-filesystems, print the boot banner, then
//! supervise a single shell process forever.
//!
//! What this deliberately does *not* do yet (all real gaps closed in M5, not silently ignored):
//! reap arbitrary orphaned grandchildren reparented to PID 1 (only the one shell child is
//! waited on), handle `SIGINT`/`SIGTERM` for a clean reboot/halt path, or apply any restart-budget
//! policy beyond a flat capped backoff. M5's real supervision tree replaces this file, not just
//! extends it.

use std::ffi::CString;
use std::time::{Duration, Instant};

const SHELL_PATH: &str = "/bin/sh";
/// A respawn faster than this counts as a "fast failure" for backoff purposes.
const FAST_FAILURE_THRESHOLD: Duration = Duration::from_secs(2);

pub fn run() {
    print_banner();
    mount_essential_filesystems();
    supervise_shell();
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
    println!("[hyperion-init] pid 1 -- M1 placeholder (mount, banner, supervised shell)");
    println!("[hyperion-init] real supervision tree lands in M5");
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

fn supervise_shell() -> ! {
    let mut consecutive_fast_failures: u32 = 0;

    loop {
        let started_at = Instant::now();
        match spawn_and_wait(SHELL_PATH) {
            Ok(status) => println!("[hyperion-init] {SHELL_PATH} exited ({status}); respawning"),
            Err(e) => eprintln!("[hyperion-init] failed to run {SHELL_PATH}: {e}"),
        }

        consecutive_fast_failures = if started_at.elapsed() < FAST_FAILURE_THRESHOLD {
            consecutive_fast_failures + 1
        } else {
            0
        };
        std::thread::sleep(backoff_duration(consecutive_fast_failures));
    }
}

/// Capped exponential backoff so a shell that fails instantly doesn't spin PID 1 at 100% CPU.
/// Not a restart-budget policy -- M5's real supervision tree owns that; this just keeps the M1
/// placeholder from being pathological in the meantime.
fn backoff_duration(consecutive_fast_failures: u32) -> Duration {
    let capped_exponent = consecutive_fast_failures.min(5);
    Duration::from_millis(200 * 2u64.pow(capped_exponent))
}

/// Forks, execs `path` with no arguments in the child, and waits for it to exit. Returns the
/// raw `wait(2)` status on success.
fn spawn_and_wait(path: &str) -> std::io::Result<i32> {
    let c_path = CString::new(path).expect("shell path has no interior NUL");
    let argv: [*const libc::c_char; 2] = [c_path.as_ptr(), std::ptr::null()];

    // SAFETY: fork() duplicates the process; the child branch only ever calls async-signal-safe
    // functions (execv, _exit) before either replacing itself or exiting, so it never returns
    // into Rust-level shared state it forked away from.
    let pid = unsafe { libc::fork() };
    match pid.cmp(&0) {
        std::cmp::Ordering::Less => Err(std::io::Error::last_os_error()),
        std::cmp::Ordering::Equal => {
            // SAFETY: c_path and argv are valid for this call, which does not return on success.
            unsafe {
                libc::execv(c_path.as_ptr(), argv.as_ptr());
            }
            // execv only returns on failure.
            std::process::exit(127);
        }
        std::cmp::Ordering::Greater => {
            let mut status: libc::c_int = 0;
            // SAFETY: `pid` is the child just forked above; `status` is a valid out-pointer.
            let rc = unsafe { libc::waitpid(pid, &mut status, 0) };
            if rc < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(status)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        let d0 = backoff_duration(0);
        let d1 = backoff_duration(1);
        let d5 = backoff_duration(5);
        let d50 = backoff_duration(50);

        assert!(
            d0 < d1,
            "backoff should grow with consecutive fast failures"
        );
        assert_eq!(d5, d50, "backoff should cap rather than grow unbounded");
        assert!(
            d50 < Duration::from_secs(30),
            "capped backoff should stay bounded"
        );
    }
}
