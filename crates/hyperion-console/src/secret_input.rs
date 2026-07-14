//! Real terminal-echo suppression for the one real secret this console ever reads from a live
//! user: a cloud provider API key (docs/998-roadmap.md "Phase 2: cloud providers", the
//! "connect my `<provider>` account" flow). Bare `libc` termios calls, matching the one existing
//! real ioctl/libc precedent in this workspace (`hyperion-init::linux::spawn_interactive`'s
//! `TIOCSCTTY` call) rather than a new dependency (`rpassword`/`termion`/`nix`) for what's a very
//! small, well-understood piece of POSIX terminal programming.

/// Disables terminal `ECHO` on stdin for as long as the returned guard lives, restoring the
/// original terminal mode on drop (including on an early return or a panic mid-read, since
/// `Drop` still runs during unwind). Best-effort, not a hard requirement: if stdin isn't a real
/// TTY (a pipe, a file -- common in tests and non-interactive contexts), `tcgetattr`/`tcsetattr`
/// simply fail and this degrades to a harmless no-op rather than a crash -- the secret is still
/// read correctly either way, just without the extra display protection a real terminal would
/// have given anyway. Matches this workspace's own established posture (`hyperion-init::linux::
/// spawn_interactive` ignores an equivalent ioctl failure for the same "worst case is degraded
/// display behavior, not a functional failure" reasoning).
pub struct RawEchoOff {
    original: Option<libc::termios>,
}

impl RawEchoOff {
    pub fn disable() -> Self {
        let mut original: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: `original` is a valid, zero-initialized `termios` for `tcgetattr` to fill in;
        // `STDIN_FILENO` (0) is always a valid file descriptor to query, real TTY or not.
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut original) } != 0 {
            return RawEchoOff { original: None };
        }

        let mut raw = original;
        raw.c_lflag &= !libc::ECHO;
        // SAFETY: `raw` is a real `termios` value just read from this same fd via `tcgetattr`
        // above, with only the `ECHO` flag cleared.
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) } != 0 {
            return RawEchoOff { original: None };
        }

        RawEchoOff {
            original: Some(original),
        }
    }
}

impl Drop for RawEchoOff {
    fn drop(&mut self) {
        if let Some(original) = self.original {
            // SAFETY: `original` is the real, previously-valid `termios` state this same fd had
            // before `disable` changed it.
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
            }
        }
    }
}
