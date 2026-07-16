# Roadmap

Hyperion's roadmap in three parts: the production boot milestones that take it from a hosted
simulator to a real, booting image; the Resourceful/Social/Self-Sustaining autonomy pillars; and
the backlog of product-level work that's real and named but not yet scheduled.

## Production Boot Roadmap

**Purpose of this document:** a self-contained prompt for driving Hyperion from its current
state — 31 Rust crates implementing Phases 2-10's *logic* as an in-process, `std`, hosted
simulator with no real kernel, hardware, or boot path — to a real image that boots from a USB
drive on real hardware and runs that logic for real. Paste this whole document as the opening
message of a session (or a `/loop`) to resume this work; the **Status** section below is the
living checklist — update it as milestones close, the same way `MEMORY.md`'s project notes track
completed phases.

### 0a. Execution Mode — read this too

**Do not stop.** Once a milestone's approach is clear from this document, execute it —
implementation choices, library selections, and sub-step ordering within a milestone's stated
scope are yours to make without pausing to check in. Work through the Status table top to bottom,
milestone after milestone, across as many sessions/`/loop` iterations as it takes, updating Status
as each one closes, exactly like the deferred-scope closure sprint that preceded this roadmap ran
to completion unattended. Don't ask "should I continue to the next milestone" — the answer is yes,
continue until the Status table is all `done` or you hit one of the two things below.

Only stop for:
1. **A decision this document explicitly flags as needing confirmation** — currently just §2's
   reference-hardware assumption, and any future case where a milestone's own text says so.
2. **A genuinely irreversible, physical-world destructive action** — writing a raw disk image with
   `dd` to a real device is the standing example (a wrong `of=` target destroys that device's data
   with no undo). Flag the exact command and target before running it against real hardware; this
   is the one place "keep going" does not override the standard caution around irreversible
   actions. Everything upstream of that command (building the image, testing it in QEMU, iterating
   on it) needs no such pause.

Everything else — which crate to touch first, how to structure a commit, whether to spend an extra
pass hardening something before moving on — is a normal engineering judgment call. Make it and keep
moving.

### 0. Decision Record — read this before anything else

**Decision (made 2026-07-11, by the project owner):** target a **Linux-hosted MVP**, not the
from-scratch hybrid microkernel [docs/03-kernel-architecture.md](03-kernel-architecture.md)
specifies. A real Linux kernel does the address-space management, thread scheduling, driver I/O,
and filesystem work; Hyperion's capability/scheduler/IPC model is implemented as a **real,
enforced userspace layer** on top of it (Linux namespaces, seccomp-bpf, Landlock, cgroups v2,
a real init and process supervisor) rather than as a privileged microkernel core written from
scratch.

**Why this is a real divergence, not a detail:** docs/03 §"Why a pure monolith is disqualified"
explicitly disqualifies exactly this shape — "a monolithic kernel... cannot host the Trust
Boundary invariant... inside a monolith, every driver already has everything." That argument is
correct for the *kernel itself*. This roadmap accepts a Linux kernel as ambient-authority code
the way docs/03 says a hybrid microkernel must not be, in exchange for a genuinely bootable
artifact in a tractable timeframe instead of a multi-year, expert low-level systems project
(seL4/Redox/Fuchsia-class). Every doc bullet, comment, or test that asserts "the kernel enforces
X" must be read as "the Linux-hosted enforcement layer built in Milestone 2 enforces X, using
kernel-level mechanisms (seccomp/Landlock/namespaces/cgroups) that are real and unforgeable from
userspace, just not implemented inside a from-scratch privileged core." Where this roadmap can't
honestly claim parity with docs/03's guarantee (see §7 Non-Goals), it says so — never silently.

If a future session wants to revisit this decision (e.g. to actually build the from-scratch
microkernel once the Linux-hosted MVP has validated Phases 2-10 on real hardware), that is a
new decision to make explicitly, not an assumption to slide back into.

### 1. Current State (verified 2026-07-11)

- 31 crates under `crates/*`, all plain `std`, all workspace members of one Cargo workspace, zero
  `no_std`, zero bootloader, zero hardware I/O. Every "Real:" claim in every crate's `lib.rs` doc
  comment is real *as in-process Rust logic*, tested by `cargo test` — none of it has ever run
  under a kernel it doesn't share a process with, on a device it was never told about, or across
  a reboot.
- Per this project's own memory record: "Phase 1 is being built as a hosted simulator first...
  Bare-metal porting is an explicit *later* milestone." That milestone starts now.
- `hyperion-capability`/`hyperion-ipc`/`hyperion-scheduler` are pure algorithm implementations
  (token derivation/revocation graph, channel framing, DRF/EDF dispatch) with no OS-level
  enforcement underneath — the algorithms are sound and worth reusing conceptually, but nothing
  they do currently prevents a real process from doing anything.
- `hyperion-sim::boot` measures a docs/36 budget *in-process*; it has never booted anything.
- Every crate's own "Deliberately deferred" bullets are the master inventory of what's missing —
  this roadmap organizes closing them, it does not re-enumerate every one inline.

### 2. Assumption to confirm or correct

**Reference hardware for the first bootable milestone: x86_64, UEFI firmware.** This is the
pragmatic choice for "boots from a USB drive" specifically — UEFI+x86_64 has the most mature
USB-boot tooling, the best QEMU/OVMF emulation story for fast iteration, and the broadest
real-hardware compatibility of any target. docs/41 Phase 1's entry criterion asks for two
reference platforms (an SBC and a workstation-class box); this roadmap treats a second platform
(e.g. Raspberry Pi 4/5, aarch64) as Milestone 11, after the x86_64 MVP is solid — do not attempt
both platforms simultaneously. If the actual target hardware is different (a specific SBC, a
specific enterprise server), say so before starting Milestone 1; the bootloader and driver choices
below assume UEFI.

### 3. Status

| Milestone | State |
|---|---|
| M0 — Toolchain, decision record, QEMU harness | done |
| M1 — Bootable "Hello Hyperion" image (the literal ask) | done (QEMU); real-hardware USB boot needs the user |
| M2 — Real capability/Trust-Boundary enforcement | done |
| M3 — Real IPC transport | done |
| M4 — Real scheduler enforcement (cgroups v2) | done |
| M5 — Real init & supervision tree | done |
| M6 — Real persistent storage | done |
| M7 — Real console UI, then real display | done (stage 1: text console; stage 2: a real, minimal DRM/KMS mode-set proof landed 2026-07-12, scoped deliberately to real pixels on a real screen, not a full compositor -- see stage 2 addendum to the completion note below) |
| M8 — Real local AI runtime | done (real Candle backend + real Intent→Agent→inference wiring on the actually-booted console path; production-scale model size, a plugin-framework bridge gap, and boot-time model pre-baking remain named, not solved — see completion note) |
| M9 — Real cryptography | done (real Ed25519 signing + BLAKE3 hashing via a new `hyperion-crypto` crate and a real software keystore, replacing all 5 named non-cryptographic stand-ins; TPM-backed sealing confirmed unavailable on this sandbox, named as a real-hardware stretch goal — see completion note) |
| M10 — Real networking | done (real HTTP/TLS/DNS fetch + real HTML extraction + real Knowledge Graph merge, reachable from the actual compiled console binary; guest network interface bring-up at real boot time named as a deferred, separate systems-provisioning gap — see completion note) |
| M11 — Second reference platform (aarch64) | done (real aarch64 kernel + rootfs boots to the real M7-stage-1 console loop under `qemu-system-aarch64 -M virt`; a real macOS-breaking seccomp portability bug was found and fixed along the way; real Raspberry Pi hardware itself named as a deferred, real-hardware-only handoff — see completion note) |
| M12 — Boot benchmarking against docs/36 | done (real, measured end-to-end cold-boot time on both reference platforms via a new `boot-benchmark.sh`/`.py`; both over docs/36's ~4.5s budget, named cause is this sandbox's lack of KVM/TCG-only emulation — a real, fixable GRUB-menu-wait bug was found and fixed along the way; real hardware timing itself named as the deferred, real-hardware-only measurement — see completion note) |
| M13 — Release engineering for a bootable artifact | done, sandbox-achievable portion (real `hyperion-release-gate` hardware-criteria extension; a real staged update applied to and rolled back from a real running booted system with no data loss, docs/41 Phase 10's literal exit criterion; a real signed, versioned, `dd`-able image built and its signature verified; a real, honestly-diagnosed image-non-reproducibility gap found, named, and later fixed for real on both platforms; a real CI pipeline now builds, signs, and publishes both platforms as downloadable GitHub Release assets on every version tag, with a real release-signing key and README download/verify/flash instructions — see completion note) — the literal exit criterion itself (a real USB drive, written via Balena Etcher or `dd`, booting on real reference-platform hardware) needs the user's own action, per this document's own explicit standing pause condition; not attempted here — see completion note |

**M0 completion note (2026-07-11):** built via `boot/` (Buildroot 2026.05, board `hyperion-x86_64`
modeled on Buildroot's own real-hardware `board/pc` EFI target, kernel 6.12.47 LTS). Verified for
real, not assumed: `qemu-system-x86_64 -bios OVMF... -drive file=disk.img,format=raw` (via
`boot/scripts/boot-test.sh`) boots GRUB2 → Linux → login prompt, and the built kernel's own
`.config` has `CONFIG_NAMESPACES`/`USER_NS`/`PID_NS`/`NET_NS`, the cgroups v2 controllers
(`CGROUPS`/`MEMCG`/`BLK_CGROUP`/`CGROUP_SCHED`/...), `CONFIG_SECCOMP_FILTER`, and
`CONFIG_SECURITY_LANDLOCK` all `=y` — and the boot log shows `landlock: Up and running.` at
runtime, not just compiled in. Caveat this dev sandbox has, which real hardware/CI won't: no
`/dev/kvm` access here, so this was QEMU-verified under TCG software emulation, not
KVM-accelerated — an iteration-speed limitation, not a correctness gap in what's being tested.

**M2 completion note (2026-07-11):** new crate `crates/hyperion-trust-boundary` rehosts
`hyperion-capability`'s token/revocation algorithm (reused verbatim, unmodified) with real Linux
enforcement: `spawn()` forks, applies a real Landlock ruleset (scoped to exactly the
`RightsMask`-derived rights on exactly the granted path, plus a separate always-present
read+execute grant on the target program's own path -- a real, non-obvious bug caught mid-build:
handling `ReadFile` as a category denies it *everywhere* a rule doesn't cover, including the
kernel's own read of the boundary's ELF during `execve()`) and a real default-deny seccomp-bpf
filter, then execs the target; `SpawnedBoundary::revoke()` sends real `SIGKILL` and reaps the
process, then revokes the token in `CapabilityMonitor` so the algorithm and the OS agree.

Verified for real by two integration tests exercising two real, separate Linux processes, not
mocked: `sandbox_enforces_scoped_filesystem_and_denies_unlisted_syscalls` mints a READ|WRITE
token, spawns a real process under it, and confirms (via a results file the sandboxed process
itself writes) that it can read/write inside its granted directory, gets a real permission error
reading outside it, and gets a real syscall denial calling `socket()` (not in the seccomp
allowlist) -- exactly "a syscall it can no longer make, a file it can no longer open" from M2's
exit criteria. `revoking_a_token_kills_the_real_process` spawns a long-lived process, confirms
it's alive, revokes its token, and confirms a raw `kill(pid, 0)` now returns `ESRCH`: the process
doesn't just look inaccessible, it no longer exists.

Getting the real test green surfaced three genuine, non-obvious bugs along the way (not
foreseeable from reading the Landlock/seccomp docs alone; all found via strace, not guessed, and
now documented at their exact call site so the next person doesn't rediscover them the hard way):
`AccessFs::from_read()` bundles in `Execute` (fixed by building the rights mapping field-by-field
instead of via that helper); handling `ReadFile` denies it everywhere except explicit grants,
including the boundary's own executable path (fixed by always granting the program its own
path); and a denied `poll()` (used by Rust's runtime at startup) cascades into a denied
self-`SIGABRT` via `tkill`, crashing the sandboxed process with an unrelated-looking `SIGSEGV`
unless both are allowlisted.

Two deliberate adaptations from the roadmap's exact wording, both explained in the crate's own
docs, not silent: (1) "privileged root-owned Capability Monitor daemon" is implemented as an
*unprivileged* process using its own fresh user namespace instead -- namespaces, seccomp, and
Landlock are all unprivileged-capable mechanisms, this sandbox has no root to test the
root-owned variant with anyway, and a smaller trust base is arguably the better design regardless.
(2) `TrustDepth::Container` unshares mount/net/uts/ipc namespaces but deliberately not PID:
`unshare(CLONE_NEWPID)` only takes effect for children forked *after* the call, so making the
spawned program actually land inside a fresh PID namespace needs a second fork with something
acting as that namespace's PID 1 -- exactly the supervisor M5 builds for real, so a one-off reaper
here would duplicate work ahead of it existing.

**M1 completion note (2026-07-11):** `crates/hyperion-init` (a real Rust binary, cross-compiled
static `x86_64-unknown-linux-musl`) now boots as PID 1 via `init=/hyperion-init` on the kernel
cmdline, dropped into the rootfs through a Buildroot overlay populated fresh each build. Verified
via `boot/scripts/boot-test.sh`: the kernel logs `Run /hyperion-init as init process`, the real
Hyperion banner prints, the essential filesystems mount (including the rw remount), and a fully
interactive `/bin/sh` comes up with working job control (`setsid()` + `TIOCSCTTY`, the same
sequence a getty performs, since exec-ing the shell directly from init skips getty entirely) — no
`can't access tty` warning. First test run was a false pass (the expected string briefly used,
plain `hyperion-init`, matched the kernel's own cmdline echo `init=/hyperion-init` seconds into
boot, before the real binary had even run) — corrected to a string that can only come from the
program's actual output. This is the literal ask: **exit criterion (a)**, "boots in QEMU+OVMF to
the banner/shell," is met and CI-checkable exactly as designed in §5. **Exit criterion (b)**,
"boots on at least one real x86_64 UEFI machine from a USB drive," is not something this sandbox
can perform or verify — no physical hardware or USB drive is reachable from here. The artifact
(`boot/.tools/buildroot-2026.05/output/images/disk.img`) is ready; the exact `dd` command and its
safety warning are in `boot/README.md`. This is a genuine handoff to the user, not a gap glossed
over: someone needs to run that command against real hardware and confirm the same banner/shell
appears before M1 can be marked fully, unconditionally done.

**M3 completion note (2026-07-11):** `hyperion-ipc` gains a real transport (`Endpoint`, in a new
`transport` module) alongside the existing in-process `IpcBus` simulator, reusing `Frame`,
`Channel`, `Request`/`Response`/`Notification`, and the call/notify semantics as-is per the reuse
map — only what actually carries a frame between two real, separate Linux processes (real Unix
domain datagram sockets) is new. `FrameBody::Region` has no wire form yet (a shared-memory region
needs real shared memory, not JSON bytes on a socket — an explicitly out-of-scope follow-on, not
silently broken: sending one over the real transport fails clearly with `IpcFault::SchemaMismatch`).

Getting this far surfaced a real security question, not just a plumbing one: `CapabilityToken`'s
fields are `pub(crate)`-only by design, so naively deriving `Serialize` on it (the obvious way to
put it on a wire) would have made it constructible from arbitrary bytes anyone could send over a
socket — any process could forge a token claiming any rights over any object it liked. Worse, this
would have been silently exploitable: the *existing* revocation graph only ever tracked a
token's generation, never its rights/object, because in-process the only thing standing between
"a token" and "an arbitrary forged struct" was Rust's own module privacy — sufficient until a
token's fields could arrive as data from outside the process at all. Fixed at the root, in
`hyperion-capability` (not routed around in `hyperion-ipc`): the revocation graph now records the
`object_id`/`rights` each token was actually minted or derived with, and every liveness check
verifies a presented token's claims against that record, not just its generation. A new
`WireToken` type (plain, serializable, carries only a *claim*) and
`CapabilityMonitor::authenticate_wire_token` (the only path from wire bytes to a real, usable
`CapabilityToken`, and only after independently validating the claim) are what `hyperion-ipc`
actually puts on the socket — `CapabilityToken` itself still has no `Serialize` impl at all, on
purpose. Three new unit tests in `hyperion-capability` (forged rights, forged object, forged
derived-child rights) prove escalation attempts are rejected; this is a real, unconditional
security property now, not contingent on M9's future cryptography — what crypto adds later is
confidentiality/replay-resistance for a token *observed* in transit, a different, narrower gap,
documented explicitly on `WireToken` rather than conflated with the forgery question this closes.

Proven end to end by a real two-process test (`hyperion-ipc/tests/real_transport.rs`): a
genuinely separate, `exec`'d client process (`ipc_client_probe`, holding only a `WireToken` claim
passed via an environment variable — no local monitor of its own, the realistic shape for a real
IPC client) makes a real call over a real socket to a server running in the test process (which
owns the authoritative `CapabilityMonitor`); the call round-trips for real
(`CALL_OK:pong`), then the token is revoked and an identical second call from a fresh client
process is rejected at `authenticate()` — the transport boundary — with the rejection
attributable to revocation specifically, not a generic failure. Deliberately verified this wasn't
a vacuous pass (same discipline as M1's and M2's caught false positives): temporarily removed the
`cap_revoke` call and confirmed the test fails without it before restoring it. Note on scope: the
client here is *not* additionally run under M2's Landlock/seccomp sandbox (that would need
allowlisting `AF_UNIX` socket syscalls and Landlock `MakeSock` rights, a real but separable
extension) — the security property the exit criteria actually cares about ("not just a
library-level check the process could route around if it weren't actually sandboxed") holds
regardless, because rejection is enforced by the server's independent re-validation against the
real monitor, not by anything the client itself does or refrains from doing.

**M4 completion note (2026-07-11):** new crate `crates/hyperion-cgroups` maps
`hyperion-scheduler`'s existing `ResourceVector`/DRF-weight math onto real Linux cgroups v2
(`cpu.weight`, `memory.max`, `pids.max`) and real `SCHED_DEADLINE` — per the reuse map, no second
fairness computation lives here, this crate only decides what real OS configuration *expresses* a
decision `hyperion-scheduler` already made. `cpu_weight_for` maps a DRF `priority_weight` linearly
around cgroup v2's own default (100), so `priority_weight = 1.0` (docs/04's baseline) lands exactly
on equal real CFS shares, matching DRF's equal-weight baseline with the same notion of "equal."
`SCHED_DEADLINE` is applied via the raw `sched_setattr(2)` syscall (no libc wrapper exists for it)
rather than `SCHED_RR`, deliberately: docs/04's `RealTimeUI` class is already EDF-dispatched in
`hyperion-scheduler`'s own algorithm, and `SCHED_DEADLINE` is a real in-kernel EDF implementation,
so applying it doesn't approximate the algorithm, it *is* the algorithm, kernel-enforced instead of
in-memory-sorted.

A real, non-obvious mapping bug surfaced by the crate's own unit tests (not foreseen from reading
the cgroup v2 docs alone): `f32::max` resolves a `NaN` argument by returning the *other* value
(`f32::NAN.max(0.0)` is `0.0`, not `NaN`), so an `is_finite()` check placed *after* the weight
multiplication let `NaN` silently collapse to the minimum cgroup weight while `+Infinity` correctly
mapped to the default — two equally-invalid inputs treated inconsistently. Caught by
`weight_clamps_to_cgroup_v2s_valid_range` failing (`left: 1, right: 100`) the first time both were
asserted together; fixed by checking `is_finite()` before any arithmetic, so both map to the same
default.

Proven for real by `tests/real_fairness.rs`: real `fork()`-ed CPU-burner processes (more than this
host's core count, so CFS is actually forced to arbitrate rather than both classes running
unopposed on separate cores) join two real, `hyperion-cgroups`-configured cgroups
(`INTERACTIVE_WEIGHT = 2.0`, `BACKGROUND_WEIGHT = 1.0` — the same weights and claim as
`hyperion-scheduler/tests/synthetic_workload.rs`), and the winning share is measured from real
kernel `cpu.stat` `usage_usec` accounting, not in-memory ledger state — the literal M4 exit
criterion. Deliberately verified non-vacuous the same way as M1-M3's caught false positives:
temporarily set both weights equal and confirmed the ratio assertion actually fails (got a real
`0.99x`, not the required `>1.2x`) before restoring the real `2.0`/`1.0` split and confirming a
clean pass.

Getting this test to run at all surfaced a genuine environment requirement, documented rather than
routed around: a process joining a cgroup must itself already be inside a delegated subtree,
because moving a process *out* of its current cgroup needs write access to that cgroup's own
`cgroup.procs`, not just the destination's — and a plain `cargo test` starts in the root cgroup
(`0::/`), which this uid can't write to at all. Diagnosed by hand (manual bash reproduction, then a
minimal standalone Rust repro) before finding the fix: run the compiled test binary already inside
a delegated scope, e.g. `systemd-run --user --scope --quiet -- "$TESTBIN"`. Rather than let
`cargo test --workspace` either fail on this precondition or silently need every caller to know a
launcher flag this crate can't enforce, the test checks the precondition itself
(`running_within_delegated_scope()`, via `/proc/self/cgroup`) and skips with a clear explanatory
message when unmet, keeping the ordinary workspace-wide gate meaningfully green while still running
the real, meaningful assertion whenever the precondition holds — under `systemd-run --user --scope`
here, or inside any real supervision tree (M5's real `hyperion-init`, running as real root,
delegating a subtree to its children directly). This is a genuine sandbox/test-launch requirement,
not a code bug, and won't apply to the real booted system.

Two things implemented but deliberately not wired to a live effect yet, both explained in the
crate's own docs: (1) `io.max`'s line format (`mapping::io_max_line_for`) is unit-tested but not
written anywhere — it needs a real block device's major:minor, which only exists once M6 gives this
system real storage, and this sandbox's own cgroup delegation doesn't expose the `io` controller at
all regardless (verified: absent from `cgroup.controllers` here). (2) `SCHED_DEADLINE` is verified
to reach real kernel admission control — rejected with `EPERM`, proving the syscall is well-formed
and really evaluated, not that it's unreachable — but this sandbox has no `CAP_SYS_NICE` and no
`rtprio` rlimit budget to actually be granted the policy, a kernel privilege boundary this crate
correctly respects rather than works around.

**M5 completion note (2026-07-11):** new crate `crates/hyperion-supervisor` implements the real
Erlang/OTP-style supervision tree: `Supervisor` owns one `CapabilityMonitor` and a table of every
real child process it spawned or adopted, keyed by current pid. `spawn_sandboxed` mints a token
(M2), spawns a real Trust Boundary process for it, and (best-effort) places it in a real cgroup
(M4). Every child this process forks -- sandboxed services and one plain, unsandboxed carryover
process a caller can fold in via `adopt_plain` -- is reaped through exactly one blocking
`waitpid(-1, ...)` call in `reap_and_restart_one`, deliberately: a second, independent waiter
anywhere else in the same process would race the kernel's single wait-queue for the same exited
children with no ordering guarantee over which caller reaps a given one -- a real correctness
hazard avoided by construction, not by convention, the same way every real init system (`runit`,
`s6`, `systemd`) funnels all child-reaping through one place. On a crash, the dead token is revoked,
a **fresh** token is minted under the same `origin` lineage, and the service is re-spawned --
`hyperion-recovery`'s already-real microreboot semantics, reused as the model per the roadmap's own
reuse-map entry, rehosted onto a real forked process instead of an in-process `AgentInstance`.
`hyperion-init` replaces M1's placeholder shell-supervision loop with a real `Supervisor`: it mounts
a real `cgroup2` filesystem at `/sys/fs/cgroup` (a gap M1's own mount list never had a reason to
close before M4/M5 existed), bootstraps a dedicated delegated subtree for real root the same way
systemd bootstraps its own at boot, spawns every Phase 2-10 `ServiceSpec` this image ships, and
folds the M1 debug shell into the *same* wait loop via `adopt_plain` rather than a second,
parallel one -- a shell still isn't capability-scoped the same way a Phase 2-10 service is
(deliberately: it exists to let a human debug a real boot by hand, which needs broad access; M7 is
what gives this system a real, properly capability-scoped interactive surface).

Two representative Phase 2-10 crates prove the mechanism against real, unmodified crate logic
(per the reuse map's "every other crate... runs unmodified as a real supervised service" entry) --
new `src/bin/hyperion-observability-service.rs` and `src/bin/hyperion-explainability-service.rs`,
each a minimal real process that receives its spawn-time capability grant as a `WireToken` claim
(via `HYPERION_WIRE_TOKEN`), mints its own separate, local capability domain over its own real
store (an `AuditLedger` append / an `ExplanationStore` begin+append_step+transition), and writes
the result -- tagged with the received grant's `token_id`/generation -- to a real state file an
outside observer (or a test) can read back. Both cross-compile to static musl binaries (confirmed
`static-pie linked`, no dynamic loader dependency), the same requirement M2's own probe binary
established, since they run under the same real Landlock+seccomp enforcement. Wrapping the
remaining ~30 Phase 2-10 crates the same way is a real, separate, purely mechanical migration this
mechanism now makes straightforward -- not attempted here, the same scoping discipline M2/M3/M4
each already applied to their own milestone.

Proven for real by `tests/real_supervision.rs`: spawns both real service processes, confirms each
did real, distinguishable, capability-tagged work (its own state file), SIGKILLs observability
specifically, and confirms -- from outside, via `Supervisor`'s own accessors and by reading the
respawned instance's own rewritten state file -- a new real pid, a genuinely fresh `token_id` (not
the stale one), and that explainability's pid/token/restart-count are completely untouched
throughout: M5's exit criterion, verified both in the supervisor's own bookkeeping and in what the
respawned *process itself* actually received and used. Verified non-vacuous the same way as
M1-M4's own caught false positives: temporarily skipped updating the tracked token on restart and
confirmed the "fresh grant" assertion genuinely fails (`left: 1, right: 1`, the stale id reused)
before restoring the fix.

Three real, non-obvious bugs surfaced and fixed while getting this real, not guessed or foreseeable
from documentation alone:
- **A real `getpid()` gap in M2's own seccomp allowlist.** A sandboxed service calling
  `std::process::id()` got back `4294967295` (`u32::MAX`) instead of its real pid: `getpid` wasn't
  in the baseline syscall allowlist, so it was denied (`EPERM`, i.e. `-1`) and the failure was never
  checked before the `-1` was cast to `u32`. Found via direct evidence (the state file's own
  content), not `strace` this time. Fixed in `hyperion-trust-boundary`'s baseline allowlist, since
  every Trust Boundary process legitimately needs its own pid eventually -- this belongs in the
  baseline, not gated behind a `RightsMask` bit the way the still-deferred socket syscalls are (a
  process's own pid isn't a resource access a capability governs at all).
- **A real cross-target bug in M4's `apply_sched_rr`, caught only once a musl-targeting caller of
  `hyperion-cgroups` existed** (this milestone's service binaries and `hyperion-init` are exactly
  that first caller): musl's `libc::sched_param` has more fields than glibc's (the `SCHED_SPORADIC`
  extension fields), so struct-literal syntax naming only `sched_priority` compiled on glibc but not
  musl. Fixed by zero-initializing the whole struct first, then setting the one field that matters
  -- portable across both libcs by construction. Also closed a real, pre-existing test-coverage gap
  found along the way: `apply_sched_rr` had no test at all (only `apply_sched_deadline` did); added
  one mirroring the existing EPERM-admission-control check.
- **A real, latent security question in `hyperion-supervisor` itself, caught before it ever shipped
  a test:** an earlier version of `tests/real_supervision.rs` panicked on a bad assertion partway
  through and left two real, still-running sandboxed child processes orphaned forever -- alive
  indefinitely, holding the test harness's own stdout pipe open so the run never appeared to
  finish, since `SpawnedBoundary`'s own `Drop` deliberately never kills a still-running process (the
  right behavior for a bare handle, wrong for the tree's own root owner). Fixed with
  `impl Drop for Supervisor`, which kills and reaps every remaining tracked child unconditionally:
  correct not just for tests, since nothing in this crate ever wants a dropped `Supervisor`'s real
  children to keep running unsupervised. Verified the fix actually works during a real panic
  unwind, not just a normal return, before trusting it.

What's real here vs. deferred, and why: the IPC rendezvous directory (`/run/hyperion/ipc`, M3's own
named "service-discovery directory" gap) is really created and every service is really told its
own well-known bind path, but nothing binds there for real yet -- M2's seccomp filter has no
`socket`/`bind`/`connect` syscalls allowlisted and its Landlock ruleset never handles `MakeSock`,
exactly the extension M3's own completion note already flagged as deferred and separable, still
open. Closing it needs `SpawnGrant`/`apply_seccomp`/`apply_landlock` to accept a distinct IPC-rights
dimension (the rendezvous directory is never the same path as a service's own `fs_scope`, so it
needs its own Landlock rule) -- a real, separate extension, deliberately not folded into this
already-large milestone. A respawn attempt that itself keeps failing (distinct from the original
process merely exiting) is logged and the service drops out of supervision rather than retried
indefinitely -- a real give-up/alerting policy for that case is a further refinement this MVP
doesn't attempt.

**M6 completion note (2026-07-11):** `hyperion-storage`'s WAL/`StorageEngine` needed zero code
changes -- exactly the reuse-map's promise ("WAL format, replay/recovery logic" reused as-is, only
"host tempfile → real block device" changes). What changed is *where* the WAL lives: a new,
dedicated second virtio-blk drive (distinct from the boot disk), pre-formatted ext4 at build/test
time by `boot/scripts/create-data-disk.sh` (this minimal Buildroot rootfs has no `mkfs.ext4` to
format one at runtime, and shouldn't need one -- a data volume is provisioned once, not reformatted
every boot). Deliberately a real *filesystem* on the dedicated device, not raw block I/O: the WAL's
own `append_and_fsync` relies on ordinary `O_APPEND` regular-file semantics, which only mean "grow
the file" on a real filesystem -- `O_APPEND` on a raw block special file seeks to the *device's
total capacity*, not "the end of what's been written," so pointing the existing WAL at a raw device
node directly would have been silently wrong, not just unconventional. `hyperion-init` (M5's real
supervision tree) gained a new `linux::storage_probe` module: mounts the dedicated partition at
`/var/lib/hyperion/data` if a second block device is present (inert -- returns `None` -- on any
boot without one, the same best-effort shape as M5's cgroup bootstrap), then runs a real
crash-consistency probe using `StorageEngine` at the same abstraction level
`tests/wal_recovery.rs`'s existing tests already use.

The real power-loss simulation the exit criteria asks for is a new script,
`boot/scripts/storage-crash-test.sh`: boots with the dedicated data disk attached
(`cache=none`/O_DIRECT -- load-bearing, not a performance tweak, since QEMU's default
`cache=writeback` would let a guest's fsync "complete" against the *host's* page cache, which a
SIGKILL to the qemu process itself could still lose, silently testing QEMU's own write-back
buffering instead of hyperion-storage's discipline), waits for the guest's own probe to report its
real WAL write loop has actually started, lets it run for a few more real seconds, then SIGKILLs
qemu outright -- a real abrupt kill, not a graceful shutdown. Reboots with the *same* data-disk
image and confirms the guest recovers a real, specific, in-range value via a completely fresh
process, kernel, and `StorageEngine` instance. Run three times for real: recovered `seq=358`,
`seq=316`, and `seq=300` respectively -- each a plausible value between the last printed progress
marker and the 200,000-write target, confirming the kill genuinely landed mid-loop each time, never
a corrupt or missing value.

One honest, important nuance surfaced while verifying this wasn't a vacuous pass (the same
discipline as every prior milestone): temporarily broke `Wal::replay`'s torn-trailing-record
tolerance (made it fail hard instead of truncating) and confirmed the *existing* host-side test
(`recovery_tolerates_a_torn_trailing_record_without_losing_prior_writes`) catches it immediately --
but re-running the *live* two-phase QEMU test with that same breakage in place **still passed**,
because that specific kill happened to land cleanly between two already-fsynced writes rather than
mid-write, never actually exercising the torn-record code path. This is real and worth being
explicit about, not glossed over: individual WAL appends here are small and fast enough that a
SIGKILL rarely lands mid-write specifically, so the live QEMU test's real, reliable contribution is
proving the write-and-recover mechanism produces a correct, real, in-range committed value after a
genuine abrupt kill against a real device -- not that it exercises the torn-trailing-bytes edge case
every run. That specific, narrower case remains deterministically covered by the existing
host-side test (re-confirmed still catches the same injected breakage), which needed no changes to
keep proving it. Wired into CI as a new step in the existing `boot-image` job, alongside
`boot-test.sh`.

**M7 completion note (2026-07-11, stage 1 only):** new crate `crates/hyperion-console` is the
literal exit criterion, end to end: a typed utterance drives `hyperion-intent::IntentEngine`'s real
(fully deterministic, not mocked) HTN matching to a real Intent Graph, then either
`hyperion-coordination`'s real multi-task allocator (driving real
`hyperion-agent-runtime::AgentRuntime::invoke` calls) for the one real HTN-decomposable utterance
shape, or -- since `hyperion-coordination::create_session` builds its task list from an intent's
*children* alone, and an unmatched utterance decomposes into none -- a direct
`AgentRuntime::spawn`/`invoke` call against the root goal itself using the real `web.search` stub
capability as a reasonable default action, discovered and closed as a real gap while wiring this
rather than silently only working for the one demo phrase. Both paths' real outcome is then
compiled through `hyperion-workspace::WorkspaceCompiler` into a real `WorkspaceGraph` and rendered
via the real `Modality::ScreenReader` accessibility-tree projection -- the literal "drive
hyperion-workspace's compiled UI/accessibility trees through a real TTY renderer" deliverable,
using docs/14's own text/voice-first accessibility framing rather than inventing a separate
rendering path. `hyperion-init` replaces its M1/M5 debug-shell adoption with this real console as
the primary interactive process (falling back to a plain shell if the console binary isn't present
in a given image), reusing the exact same setsid/TIOCSCTTY mechanism, and points its Knowledge
Graph at M6's real dedicated data partition when one is mounted.

Proven twice, for real, via a genuinely new test mechanism this milestone needed and M0-M6 didn't:
`boot/scripts/console-test.sh` boots the real image with ttyS0 backed by a real Unix domain socket
(a small `console-drive.py` helper connects to it) instead of `boot-test.sh`'s output-only
`-serial file:...` capture, since M7 is the first milestone whose exit criterion requires *sending*
real input, not just observing real output. Typing `"I need to launch my startup"` at the real
booted console produced the real 4-task decomposition, all four real tasks really completing
(`status: market_research: Done`, etc.); typing an unrelated utterance
(`"what is the weather like today"`) produced the real fallback path's real stub result, embedding
the exact utterance text verbatim
(`status: generic_goal: done -- {"results":["stub finding for query 'what is the weather like \
today'"]}`) -- proving both real code paths for real, not just in the crate's own host-side
integration tests (which also pass, and were themselves verified non-vacuous the same way as every
prior milestone: temporarily broke the fallback path's query-forwarding, confirmed the "echoes the
real utterance" assertion actually fails, then restored it).

Stage 2 (a real compositor driving real pixels from a compiled `WorkspaceGraph`, plus real text
layout/font rendering) is deliberately not attempted here, exactly as this roadmap's own M7 text
asks: "treat this as its own large sub-project; do not block M7's console stage on it." No model
inference (real or mock) is called anywhere in this pipeline either -- `hyperion-intent`'s HTN
matching is permanently, deliberately deterministic (not something M8 replaces), and wiring a real
model call into a future Agent capability is separate, later work M8 is what motivates for real.

**M7 stage 2 addendum (2026-07-12):** a real, deliberately minimal proof landed -- real DRM/KMS
mode-setting on a real display device, drawing real, controlled pixel data, scoped down from "a
full compositor" to exactly the bounded claim this roadmap's own text allows ("real pixels on a
real screen," not window management, GPU-accelerated rendering, or `WorkspaceGraph` rasterization,
all of which remain the same large, separate sub-project stage 1's own note already deferred).
New `crates/hyperion-init/src/linux/display_probe.rs`: opens `/dev/dri/card0` if present (inert
otherwise -- every other boot script in this repo attaches no display device, so this never
triggers there), finds a connected connector + mode + CRTC via the real `drm` crate (mirroring
that crate's own `legacy_modeset` example), creates a real kernel "dumb buffer" (the generic
CPU-writable framebuffer any KMS driver supports, no GPU-specific rendering pipeline needed),
writes a deliberate three-band color pattern into it (not a solid fill, so a real screenshot
matching it exactly is strong evidence of real, controlled pixel output rather than stale/garbage
VRAM content), and issues the real `SETCRTC` ioctl that actually displays it.

Proven two ways, not just "the guest-side ioctls returned success": (1) the guest's own real,
reported mode (`1280x800`, `vrefresh: 75`, `PREFERRED | DRIVER`) confirms a real KMS negotiation
happened, not a hardcoded resolution. (2) New `boot/scripts/display-test.sh` boots with a real
`virtio-gpu-pci` device attached (via the kernel's own already-enabled `CONFIG_DRM_VIRTIO_GPU`)
and, from *outside* the guest entirely, issues a real QEMU HMP `screendump` command
(`boot/scripts/screendump.py`) to capture the emulated display's actual current pixel content to
a real PPM file -- `boot/scripts/verify-screendump.py` then confirms the captured screenshot's
real pixel values match the expected three bands exactly (`RGB(107,44,74)`/`(255,255,255)`/
`(74,44,107)`, sampled and confirmed pixel-for-pixel). This is real, independent verification: the
screenshot-capture path has no dependency on anything the guest itself claims.

Scoped to x86_64 only for this pass -- proving the identical mechanism on aarch64 is real,
straightforward, deferred work (the same kernel `CONFIG_DRM_VIRTIO_GPU` is already enabled there
too; it would need aarch64's own virtio-mmio device name for the GPU rather than x86_64's PCI one,
mirroring this session's own already-learned virtio-mmio-vs-PCI device-naming lesson), not
attempted here given this milestone's own "prove the mechanism once against a representative
platform, don't redo the same thing without new engineering insight" precedent (M5/M9/M11 all
already made the same call). Everything beyond this bounded proof -- a real compositor, real
`WorkspaceGraph` rasterization, real font/text rendering, real input routing -- remains exactly as
deferred as stage 1's own note already said, and is real, separate, large future work.

**M7 stage 1 bug fix (2026-07-12): stale response text across multiple console turns.** Found by
actually driving a real multi-turn interactive session against the real booted console (both in
QEMU and reproduced natively on the host) -- never by reading code, and not caught by any existing
test, since every one of them (`console-test.sh`, `console-drive.py`) sends exactly one utterance
per boot. Typing two different utterances that both fall through to the undecomposed-goal
("generic_goal") path in the same session made the *second* one's response silently redisplay the
*first* one's text. Root cause traced with targeted `eprintln!` instrumentation at every layer of
the call chain (console input loop → `run_undecomposed_goal` → `assistant.respond` dispatch →
`MockBackend::generate`) confirmed the *prompt* was correct at every one of those layers; the bug
was one layer further, in `hyperion-workspace::WorkspaceCompiler`'s template cache. Its cache key
(`intent_predicate` + capability-set + `complexity_tier`) is deliberately coarse -- a real,
intentional optimization so two same-shaped turns reuse the same real layout decisions (panel
count/size/position, lint result) instead of redoing that work -- but `build_template` also bakes
each contract's own content (`label_template`, surfaced as `AccessibilityNode.accessible_name`)
into that same cached `Panel`, and neither `compile()` nor `get_template()` (the one
`hyperion-console::render_workspace` actually calls to get the tree it projects) ever refreshed
that content on a cache hit. Fixed in `crates/hyperion-workspace/src/compiler.rs`: both now
refresh each panel's accessibility node from the current turn's contracts, hit or miss, while
still reusing the cached structural layout. A new regression test
(`two_different_undecomposed_goals_each_get_their_own_response_text`) sends two different
undecomposed goals back to back and asserts the second doesn't carry the first's text -- confirmed
non-vacuous by reverting the fix (via `git stash`) and watching the new test fail first, then
restoring it and watching the whole suite pass clean.

**M8 completion note (2026-07-11):** `hyperion-ai-runtime` gains a new, feature-gated (`candle`,
off by default) `CandleBackend` (`src/candle_backend.rs`) implementing the crate's own
pre-existing `InferenceBackend` trait for real: real weights, real tokenizer, a real
autoregressive forward-pass-plus-sampling loop via `candle-core`/`candle-nn`/
`candle-transformers`. It loads Andrej Karpathy's real `stories15M.bin` (a genuine 15M-parameter
Llama-architecture checkpoint, ~61 MB, from `karpathy/tinyllamas` on the Hugging Face Hub, via
`hf-hub` 1.0's blocking client) rather than docs/36's 1-3B-parameter production "small resident"
tier -- a deliberate, named gap: this sandbox has no GPU/NPU, and real CPU-only inference at that
scale would be minutes-per-token, not the seconds this milestone's own proof needs. The mechanism
(a real forward pass through real weights) is identical regardless of parameter count; reaching
docs/36's actual latency/throughput target is a real-reference-hardware question this sandbox
cannot answer, the same shape of hand-off M1's USB-boot criterion and M4's real-time-scheduling
criterion already left explicit rather than silently claiming met. Verified non-vacuous the
standard way: temporarily made `generate` echo the prompt verbatim, confirmed the integration
test's `assert_ne!(result.text, request.prompt)` catches it, restored, reconfirmed a clean pass
(26-31s including first download; cached by `hf-hub` after).

Two real wiring gaps had to be found and closed for that backend to matter anywhere, not just in
its own test. First: `hyperion-api-gateway::gateway.rs`'s `invoke_capability` dispatch loop
unconditionally called `hyperion_agent_runtime::dispatch_stub_capability`, completely ignoring
`hyperion-model-router`'s own `ImplKind`/`ModelClass` decision -- fixed via a new `dispatch_one`
method that branches to a real `ai_runtime.infer(...)` call when the routed impl is
`LocalSmallModel`/`LocalLargeModel` with a registered `ModelClass`, falling through to the
existing stub otherwise (proven via a new `#[cfg(test)] mod tests` inside `gateway.rs` itself,
needed because a separate, pre-existing gap -- `hyperion-plugin-framework::ImplementationDescriptor`
has no `ModelClass`-equivalent field at all, so `router_bridge.rs::to_router_descriptor` can never
produce `model_class: Some(...)`, and `invoke_capability` re-derives and overwrites every
candidate's descriptor from the plugin registry on every call -- means that branch is structurally
unreachable through the real plugin-registration path today; documented in place, not silently
worked around, since fixing the plugin manifest shape is a separate, larger change).

Second, and more consequential: `hyperion-api-gateway`/`hyperion-model-router` turned out to be
wired into *no* real caller at all -- `hyperion-console` (M7's actual booted entry point) calls
`hyperion-agent-runtime::AgentRuntime::invoke` directly, never through the gateway. Fixing
`dispatch_one` alone would have satisfied the exit criteria's letter while leaving the actually-
booted console exactly as mock as before. So `hyperion-agent-runtime` gained a third, real
Capability alongside its two stubs (`web.search`, `document.draft`): `assistant.respond`,
dispatched via a new `dispatch_assistant_respond` method to a real, caller-supplied
`Arc<LocalAiRuntime>` (`AgentRuntime::new`'s signature change -- mechanically propagated across
all 13 real call sites in 6 crates: `hyperion-agent-runtime`'s own tests, `hyperion-recovery`,
`hyperion-federation::hub.rs`'s real per-device construction, `hyperion-threat-model`,
`hyperion-coordination`'s four test files, and `hyperion-console` itself), gated behind the exact
same Broker/quota/circuit-breaker checks every other Capability call already goes through --only
the dispatch step branches, mirroring `dispatch_one`'s own shape. `hyperion-coordination`'s
`default_manifests()` gained a new `"assistant"` specialization (baseline capability
`assistant.respond` only -- no HTN leaf predicate ever maps to it, since it exists for the
*undecomposed*-goal fallback, not a template's leaves) and `hyperion-console::session.rs`'s
`run_undecomposed_goal` now spawns that specialization and calls `assistant.respond` with the raw
utterance as `prompt`, instead of `web.search`'s canned stub string. `ConsoleSession` gained a new
`build_ai_runtime` constructing a real `LocalAiRuntime` -- `MockBackend` by default, a real
`CandleBackend` behind a new `candle` feature on `hyperion-console` itself (falling back to
`MockBackend`, not panicking, if a real load fails -- docs/02 §4 invariant 5's "degrade, never
fail closed" applied to a missing model exactly like everywhere else in this system).

Proven for real, not just asserted: built `hyperion-console` with `--features candle` and piped a
real, unmatched utterance ("what is the weather like today") into the real compiled binary.
Real output: `status: generic_goal: done -- . A big red bus comes and drives to help people. The
bus drives very fast. People get to stay in the bus and stay safe. One day, the weather is very
harsh...` -- genuinely new, coherent TinyStories-style text, not an echo, not a stub, produced by
a real forward pass through the real downloaded model, reached via the exact real
Intent→Agent→inference path the booted console exercises. Verified non-vacuous the same way as
every prior milestone: temporarily disabled the `assistant.respond` branch in
`AgentRuntime::invoke`, confirmed `hyperion-console`'s own existing integration test
(`an_unmatched_utterance_still_produces_a_real_agent_invocation_as_text`) fails against the stub's
different JSON shape, restored, reconfirmed a clean pass; that test's own comment was also
corrected in place (it had described the now-replaced `web.search`-stub behavior).

`hyperion-intent`'s own HTN decomposition and its "generative decomposition" fallback (docs/05 §2
-- an utterance matching no template, calling a real planning model to produce a real multi-leaf
plan) were deliberately **not** touched. HTN template matching remains permanently deterministic,
exactly as this document's own M7 note already said M8 would not replace. Generative decomposition
specifically was considered and rejected for this milestone: `stories15M.bin` is a real but
non-instruction-tuned children's-story completion model with no ability to reliably follow a
"decompose this goal into steps" instruction, and fabricating leaf tasks from output it cannot
actually produce reliably would be exactly the "pretend" `hyperion-intent`'s own doc comment
already rules out for this fallback ("degrade, never fail closed, but also never pretend"). What's
real instead is narrower and honest: the one *action* taken about an undecomposed goal is now
really model-driven; the goal itself still has no children, same as before M8.

Named gaps left open, deliberately, not silently: (1) the plugin-framework `ModelClass` bridge
above -- a real, separate manifest-shape change; (2) `runtime.rs::infer`'s pre-existing
`tokens_generated: 0` stub, backend-independent, unrelated to this milestone's scope; (3) a real
release image's first `CandleBackend::load()` call genuinely hits the network (Hugging Face Hub)
unless `hf-hub`'s cache is pre-populated -- fine for this dev loop, not for a real boot with no
network yet up; a real image needs the model file baked into its rootfs at Buildroot build time,
separate work from proving the inference mechanism itself. Full gate confirmed green throughout:
`cargo fmt/clippy/build/test --workspace` (default features, network-free, all pass) and
`cargo clippy --features candle` for both `hyperion-ai-runtime` and `hyperion-console` (also
clean) -- the `candle` feature stays fully opt-in on both crates, exactly as designed.

**M8 follow-up (2026-07-12): gap (3) above closed for real, on a real boot, with zero network
access.** Driving a real interactively-booted x86_64 image (`--features candle`) surfaced that
closing (3) needed two independent fixes, not one, each found by actually attempting it rather
than assumed:

1. **`hf-hub`'s own cache fast path never triggers for the default `"main"` ref.**
   `download_file_to_cache` only skips the network when `revision` is already a 40-hex-char commit
   hash -- a mutable ref like `"main"` always needs a live resolve first, even with an
   already-fully-populated local cache. Fixed by pinning the exact commit each file was verified
   against as new constants (`TINYLLAMAS_REVISION`, `LLAMA_TOKENIZER_REVISION` in
   `candle_backend.rs`), only applied when `model_id` is the one well-known repo this crate itself
   verified that commit against -- a caller-supplied `model_id` elsewhere still resolves `"main"`
   live, unchanged.
2. **Even with a pinned revision, `HFClientSync::new()` still failed with a real "builder error"
   -- because it never got that far.** `hf-hub`'s HTTP client (`rustls-platform-verifier`) builds a
   real TLS trust store *unconditionally at client construction time*, before any cache lookup
   ever runs -- an empty trust store (this rootfs ships no `ca-certificates` package) makes
   construction itself fail. No amount of cache pre-population reaches that fast path if the
   client can't even be built. Traced with targeted `eprintln!` instrumentation confirming the
   pinned revision and the on-disk snapshot were both genuinely correct at every layer up to this
   point -- the failure was one level further out than either. Fixed by pointing
   `rustls-native-certs` (which `rustls-platform-verifier` uses on Linux) at a real, baked-in CA
   bundle via `SSL_CERT_FILE`, which it checks before any hardcoded distro-specific path.

New `boot/scripts/bake-candle-cache.sh`: bakes both the pinned model/tokenizer files (reusing an
already-downloaded host `hf-hub` cache if present, else fetching them for real) and a real host CA
bundle into the x86_64 rootfs overlay. `hyperion-init` wires up `HF_HUB_CACHE`/`SSL_CERT_FILE` for
the console only when those baked paths actually exist -- inert on an ordinary mock-backend image,
same "only wire up what's actually here" convention `display_probe`/`storage_probe` already use.
Verified end to end on a real boot with `-net none` (no network device attached at all): real
generated text ("A dragon has big wings and a long tail...", genuinely different from the mock
echo and matching the same native-run output verified earlier), proving the whole chain -- pinned
revision, baked snapshot, baked CA bundle, zero network -- works together for real. Cross-compiling
`--features candle` for musl also needed one more independent fix:
`hyperion-ai-runtime/Cargo.toml`'s `tokenizers` dependency pulled in that crate's full default
features (including `esaxx_fast`, a Unigram-tokenizer speed optimization needing a real C++
toolchain this sandbox's musl cross-toolchain doesn't have); `candle-core` itself already uses
`default-features = false, features = ["onig"]` for the same dependency, a pure-Rust-buildable
choice matched here too. Not automated into `build-image.sh` itself: doing so needs a real,
checked-in musl C(++) cross-toolchain provisioning story (this session's own working fix was a
rootless-extraction-plus-hand-corrected-specs-file hack, sandbox-specific, not something to bake
into shared infra unmodified) -- named as the next real piece of work if a fully automated
candle-image build pipeline is wanted, not attempted here.

**M8 follow-up (2026-07-12, undocumented until now): a runtime backend switch.** `LocalAiRuntime`'s
single `backend` field became a `Mutex<Box<dyn InferenceBackend>>` with a `set_backend` swap, and
`hyperion-console` gained a `/backend <name>` / `use backend <name>` (deliberately the full three
words, never the bare `use <name>` -- "candle"/"mock" are ordinary enough words that a shorter
phrase could collide with a real goal utterance) meta-command, checked ahead of the intent engine
since it's a runtime control, not a goal. Lets a running console move between `CandleBackend` and
`MockBackend` with no restart. `/help` added alongside it.

**M8 follow-up (2026-07-13): "Phase 1: local-engine backends" -- Ollama, vLLM, and LiteLLM.** The
user asked Hyperion to reach "all major model providers/engines/proxies"; scoped deliberately to
local engines first (no API keys, no real money, no consent-gate friction), with cloud providers
(OpenAI/Anthropic/Gemini, needing real secret storage and a real consent gate) an explicit, later,
separate phase -- see this session's own design-review findings on why: today's `hyperion-crypto::
Keystore` is a single hard-coded 32-byte Ed25519 seed with no encryption-at-rest and no generic
secret-slot API (unsuitable for provider keys as-is), while the consent mechanism a cloud phase
would reuse already exists and works (`hyperion-agent-runtime`'s `PendingConsent`/`resolve_consent`
round trip) -- and `hyperion-model-router`'s real `cloud_consent` gate is fed a hardcoded `true` at
its one real call site (`router_bridge.rs`) today, a seam for that later phase to close, not this
one. Also confirmed and deliberately not touched here: the live console bypasses
`hyperion-model-router`/`hyperion-api-gateway` entirely (`dispatch_assistant_respond` calls
`LocalAiRuntime::infer` directly with a hardcoded `ModelClass`) -- a large, pre-existing,
separate gap.

Ollama, vLLM, and a self-hosted LiteLLM proxy all speak (or can speak) the same OpenAI-compatible
`/v1/chat/completions`+`/v1/models` REST shape, so one new backend --
`hyperion_ai_runtime::openai_compat_backend::OpenAiCompatBackend`, feature-gated behind a new
`openai-compat` Cargo feature (off by default, same convention as `candle`) -- covers all three via
`base_url`/`model` alone, rather than three bespoke clients. Its `connect()` does a real, eager
`GET {base_url}/models` at construction time (mirroring `CandleBackend::load()`'s own real-work-
eagerly precedent): `generate()` can't return a `Result` (the trait's own contract embeds every
failure as `"[backend error: ...]"` text instead), so deferring the check would let the console's
own backend-switch meta-command falsely report success and only surface a real failure garbled
into the next answer. Uses `rustls-tls-webpki-roots` (a bundled root store, like
`hyperion-netstack`'s own `real-http` feature) rather than repeating `hf-hub`'s CA-bundle-baking
problem from the M8 follow-up above.

`hyperion-console`'s `/backend`/`use backend` grammar extends (doesn't replace) the existing
`candle`/`mock` zero-arg form: `/backend <ollama|vllm|litellm> <model> [base_url]` (each preset's
own well-known default port used when `base_url` is omitted) and `/backend custom <base_url>
<model>` (no preset, both required) for any other OpenAI-compatible server. An optional per-engine
bearer key (Ollama/vLLM typically need none; a self-hosted LiteLLM proxy often does) is read from
a namespaced env var (`HYPERION_OLLAMA_API_KEY` etc.) so one provider's key can never leak onto
another's connection.

Tested two ways, both real: (1) a hand-rolled, minimal HTTP/1.1 fixture server on an ephemeral
local port (`std::net::TcpListener`, no new dependency) proving a genuine request/response round
trip without needing a real engine installed in CI/sandbox -- the same move `real_web_fetch.rs`'s
own doc comment records this workspace already made once, replacing a flaky remote test host with
a fully local, deterministic one; and (2) live, against a real Ollama instance this sandbox
happened to already have running (`gemma3:270m`, real weights, real generation): `/backend ollama
gemma3:270m`, a real prompt ("say hello in exactly five words" -> "Hello!"; "what is 2 plus 2" ->
"2 + 2 = 4"), `/backend mock`, back again, all in one running process. `cargo build/test/fmt/clippy`
all pass across every feature combination (none / `candle` / `openai-compat` / both).

**M8 follow-up (2026-07-13): "Phase 2: cloud providers" -- OpenAI, Anthropic, and Gemini, behind
real encrypted secrets and a real consent gate.** Scoped deliberately narrower than it could have
been, and why: no real Anthropic/Gemini/OpenAI API key exists in this sandbox, so the two
genuinely new backends (Anthropic's Messages API, Gemini's `generateContent` API -- OpenAI itself
needed no new backend at all, since its real API already speaks the `openai-compat` shape Phase 1
built) are only proven here against hand-rolled local fixture servers, the same rigor as Phase 1's
fixture tests but without a live-account-equivalent proof; real end-to-end verification against a
real account is the user's own follow-up step, named rather than silently assumed. Fixing
`hyperion-model-router`/`hyperion-api-gateway` being bypassed by the live console remains a
separate, untouched, pre-existing gap.

New `hyperion_crypto::secret_store::SecretStore`: `ring`'s ChaCha20-Poly1305 AEAD (already
resolved in this workspace's `Cargo.lock` transitively via the `rustls` stack, so a direct
dependency here adds zero new transitive crates) encrypting a small `HashMap<String, String>`
(provider -> API key) with a symmetric key derived, via a new `Keystore::derive_key` method, from
the same per-device Ed25519 identity M9 already established -- no new passphrase/key-management UX,
and the raw signing key itself never leaves `Keystore`. Fixes the one real gap `Keystore::
persist_new_key` itself still has (direct `fs::write` + separate `chmod`, no atomicity) for its
own new file: temp-file + rename + `chmod 0600`. A wrong device key fails closed with a real
authentication-tag mismatch, proven directly, not assumed.

New `hyperion_ai_runtime::anthropic_backend::AnthropicBackend` and `::gemini_backend::
GeminiBackend` (feature-gated `anthropic`/`gemini`), each following `CandleBackend`'s and
`OpenAiCompatBackend`'s own conventions exactly (eager real connectivity proof at `connect()` time,
every failure embedded as `"[backend error: ...]"` text since `generate()` can't return a
`Result`). Both expose a `connect_at(base_url, ...)` alongside the real `connect(...)` that only
ever targets the real API -- a real feature in its own right (Anthropic/Gemini don't have
alternate endpoints today, but this matches `OpenAiCompatBackend`'s own already-real
`HYPERION_OPENAI_BASE_URL`-style override story) as much as it's what lets this crate's own tests
prove real HTTP/JSON wiring against a local fixture server rather than a real account.

`hyperion-agent-runtime` gains three new requestable Capabilities (`cloud.openai`/
`cloud.anthropic`/`cloud.gemini`, declared on the "assistant" manifest in `hyperion-coordination`'s
`catalog.rs`, never baseline) routing through the exact same `dispatch_assistant_respond` the
baseline `assistant.respond` already uses -- dispatch itself is backend-agnostic, only the *gate*
differs, so local/mock/self-hosted-engine use stays completely ungated. A new `AgentRuntime::
grant_capability` grants a capability directly with no live `PendingConsent` required first
(`resolve_consent` itself still hard-requires one, confirmed by reading its real body, not
assumed) -- used by the console's own "connect my `<provider>`" flow so typing a real key doesn't
also demand an immediate, redundant re-confirmation.

A real, previously-latent gap found (not introduced) during this work: `hyperion-console`'s
`run_undecomposed_goal` spawned a fresh `AgentInstance` on *every utterance*, so no capability
grant -- cloud consent included -- could ever survive past a single turn, even within one running
session. Fixed by giving `ConsoleSession` one persistent `assistant_instance_id`, spawned once at
`open()` and reused by every turn instead of respawned; the one named trade-off is that this
instance's own `bound_intent` (scheduler bookkeeping only) is `None` forever now, since there's no
single root `NodeId` at session-open time to bind it to -- cosmetic, not correctness-affecting for
the real admission gate.

**M8 follow-up (2026-07-13): "explore the Knowledge Graph" -- `/recall`, `/why`, `/related`.** The
user wanted user-friendly slash commands to browse whatever the console's own, real
`hyperion-knowledge-graph` has recorded -- confirmed the console already opens a real
`KnowledgeGraph` at `open()` (`console_knowledge_graph.jsonl`) and it's genuinely populated in
normal use (`hyperion-intent::engine` writes a real `"intent"` node on every utterance, decomposed
or not), but nothing before this exposed it -- it only ever fed `ContextEngine`/`NetstackHub`
internally. New `crates/hyperion-console/src/graph_explorer.rs::GraphExplorer` wraps the three
read-only calls this needed (`KnowledgeGraph::query`/`traverse`/`explain`/`get`, all already
`RightsMask::READ`-gated and satisfied by this session's own root token) behind session-local
numbered references (`[1]`, `[2]`...) -- no raw `NodeId` is ever shown, matching CLAUDE.md's
"never expose... internals" and "progressive complexity": `/recall [text]` searches (bare, lists
everything recent), `/why <n>` explains a result's own provenance (CLAUDE.md's Explainability
questions -- why, how, how connected -- applied to one recorded thing), `/related <n>` shows what's
connected to it and re-numbers so results chain.

Deliberately `/`-only, with **no** plain-English alias unlike `/backend`/`use backend` -- named as
the one real judgment call: phrases like "what do you know about X" or "remember X" are exactly
what a future real memory-recall Intent should own, and hard-coding them here as a meta-command
would squat on that surface before that Intent exists. Also deliberately honest about scope:
`/recall`'s search is a plain, case-insensitive text match over rendered node descriptions, not
semantic search -- no embedding pipeline is wired into this console (`hyperion-ai-runtime` exposes
no `embed()` call at all today), so pretending otherwise would overclaim a capability that isn't
real.

Rendering required actually reading what `hyperion-intent` writes, not assuming a shape: a
decomposed goal's own child tasks (e.g. "market_research") share the same `"intent"` object type
as the real root utterance but carry an empty `raw_utterance` (`hyperion_intent::engine::decompose`
never sets one, since nobody actually said "market_research") -- both `describe()` (the numbered-
list line) and `friendly_type()` (`/why`'s own opening line) branch on that, falling back to the
task's own `predicate` field. A real bug in an early version of this work had only `describe()`
make that distinction; `friendly_type()` still called every task "something you asked" -- found by
actually driving the built-in "launch my startup" HTN template through a live process
(`/recall market_research` -> `/why 1`), not by reading the code, and fixed by extracting a shared
`utterance_text()` helper both functions now call. Proven for real end-to-end: the built-in HTN
template's own real `depends_on` edges (`business_model`/`branding` both really depend on
`market_research` -- `hyperion-intent/src/templates.rs`) let `/related` show genuine graph
traversal results, not fixture data, and a live run confirmed unknown references (`/why 99`) and
non-numeric arguments (`/related abc`) degrade to a plain-language message rather than a panic or
a silent no-op. `cargo build/test/fmt/clippy` all pass across every feature combination.

The console's own consent policy, arrived at by first writing the more generous version and then
catching the real problem with it: an earlier draft seeded every already-connected provider's
grant automatically at every `open()`, which would have made the real `PendingConsent` machinery
permanently unreachable through any real console sequence at all (every path to a `Cloud` backend
requires a stored secret, and every stored secret would have already carried a grant) -- exactly
the "real, tested, but never actually exercised" gap this workspace's own discipline rules out
elsewhere. Fixed before shipping, not after: a stored secret now only proves a real account
*exists*; "connect my `<provider>`" grants this *one running session* the right to use it
immediately (no redundant re-confirmation right after typing the key), but a fresh boot's first
real cloud dispatch genuinely re-asks, once per boot, through the real `PendingConsent` round trip
-- proven with a real, three-session sequence (connect + immediate use; a fresh reopen against the
same data_dir hitting a real consent prompt on first use; a fresh reopen declining consent and the
session continuing normally afterward), not just `hyperion-agent-runtime`'s own isolated tests.

Also new: a real no-echo secret-entry path (`hyperion-console/src/secret_input.rs`), bare
`libc::termios` `tcgetattr`/`tcsetattr` clearing `ECHO` on stdin for exactly the follow-up API-key
line, restored via `Drop` (so a panic mid-read still restores it) -- matching this workspace's one
existing real libc/ioctl precedent (`hyperion-init::linux::spawn_interactive`'s `TIOCSCTTY` call)
rather than a new dependency, and degrading to a harmless no-op (never a crash) if stdin isn't a
real TTY.

`cargo build/test/fmt/clippy` all pass across every new feature combination
(`anthropic`/`gemini`/`openai-compat`, individually and together) for `hyperion-crypto`,
`hyperion-ai-runtime`, `hyperion-agent-runtime`, `hyperion-coordination`, and `hyperion-console`.
Not run combined with `candle` in the same invocation: this sandbox's own cached Candle model
makes `--features candle` builds real-load successfully, which changes several pre-existing
(Phase-1-and-earlier) tests' own assumed default backend -- a known, pre-existing interaction
unrelated to this phase, confirmed by reproducing the same failures with `--features candle`
alone.

**M9 completion note (2026-07-11):** new crate `hyperion-crypto` is the real primitive every one
of the five named non-cryptographic checksum/hash stand-ins now depends on: real Ed25519
signing/verification (`ed25519-dalek`), real BLAKE3 content hashing (this workspace's own
already-stated preference, per docs/28's content-defined chunking spec), and a real, minimal,
file-backed software `Keystore` -- generates a real key via the OS CSPRNG on first use, persists
the raw seed with owner-only (`0o600`) permissions, loads the same key on every subsequent open.
Confirmed directly, not assumed: this sandbox has no TPM (`/dev/tpm*` does not exist, `/sys/class/tpm`
is empty), so hardware-backed sealing is named as a real, hardware-dependent stretch goal, exactly
the same shape of hand-off as M1's real-hardware USB boot. Every domain verifies against **one**
real device identity rather than a multi-publisher trust store -- docs/24's "verify against
publisher's registered key" implies a registry of many trusted keys that does not exist anywhere
in this workspace; building one is a separate, real PKI feature, not a "checksum → real crypto"
swap, and a single device key already satisfies the milestone's actual exit criterion
(unforgeable without the private key) without inventing an undocumented design.

All five named stand-ins replaced for real: `hyperion-ai-runtime::ModelDescriptor.signature`,
`hyperion-plugin-framework::PluginManifest.signature`, and `hyperion-update::UpdateManifest.signature`
are now real `Option<Signature>` fields, verified by each crate's own registration/install/apply
path (`register_model`/`install`/`apply_update` each gained a `&VerifyingKey` parameter -- not a
constructor change, to keep the blast radius to exactly the functions that verify, not every
caller that merely holds the surrounding struct); `hyperion-security`'s model-integrity gate calls
`hyperion-ai-runtime`'s own now-real `verify` directly (it never had its own algorithm). Worth
noting: `hyperion-update`'s old stand-in used `std::collections::hash_map::DefaultHasher`
(SipHash) -- not even the same FNV1a-style pattern the other stand-ins shared, and explicitly
documented upstream as unsuitable for anything beyond in-process `HashMap` bucketing, unstable
release to release. `hyperion-observability`'s audit-ledger hash chain is now real BLAKE3 (via
`hyperion_crypto::hash`) instead of the same non-cryptographic `DefaultHasher` -- sufficient on
its own to satisfy this milestone's exit criterion, which accepts "a real signature *or*
hash-chain check"; docs/34's fuller design (a periodic Ed25519-signed Merkle anchor over segments
of the chain, `device_key.sign(merkle_root(segment))` every `ANCHOR_INTERVAL` entries) is real,
separate, additive work this milestone does not build, named in `hyperion-observability`'s own
doc comment rather than silently implied done.

The mechanical refactor this required was, by design, smaller than M8's: putting the verifying
key on the *verifying function* rather than threading a keystore through every constructor kept
each crate's blast radius to its own `register_model`/`install`/`apply_update` call sites (6, 10,
and 3 files respectively) instead of every caller of the containing struct. `hyperion-console`'s
real device key is a real `Keystore` persisted under the same `data_dir` M6's dedicated partition
already gives the Knowledge Graph, so it survives reboots rather than regenerating every restart.

Verified non-vacuous throughout, and more strongly than the old stand-ins ever could be: every
existing "tampered X is rejected" test was preserved (content tampered post-signing still fails,
exactly as before), and a new test was added at all four signing sites proving a specifically
*forged* artifact -- signed by a real, different keypair, not just checksummed wrong -- is also
rejected; a non-cryptographic checksum could never have caught that case, since a forger can
always recompute a checksum, but never produce a valid signature without the real private key.
`hyperion-observability`'s `VerificationReport::Corrupt` path had no test anywhere in the
workspace before this milestone (a real gap the research pass surfaced); added two, inside a new
`#[cfg(test)] mod tests` within `ledger.rs` itself (the public API has no way to tamper an
already-appended entry from outside the crate -- `append` is deliberately the only write path).
The first attempt at the second test asserted the wrong `at_seq`: tracing through `verify_chain`'s
actual order of checks showed that directly corrupting an entry's own `entry_hash` fails that
entry's *own* self-consistency check before ever reaching the link check on the entry after it --
corrected to a scenario that isolates the link-check path specifically (a spliced entry whose own
hash still recomputes fine, but whose `prev_hash` points at a different chain). Full workspace
gate green throughout, including `cargo clippy --features candle` for the two `candle`-gated
crates -- 465 tests passing, up from 461 before this milestone.

Named gaps left open, deliberately: (1) a multi-publisher trust store/PKI, everywhere a single
device identity now stands in for it (see above); (2) `hyperion-update`'s anti-rollback monotonic
version counter -- docs/32 asks for one, none exists in any form, a pre-existing gap this
milestone's signature fix does not touch (closed later, 2026-07-16: see this document's Resourceful
pillar section below for `SystemImageController::highest_version_ever`); (3) `hyperion-observability`'s periodic signed
Merkle-anchor over the hash chain (see above). Also identified but explicitly left untouched, since
none is named in this milestone's own exit criteria (a tampered plugin manifest, update package,
or audit-ledger entry): `hyperion-context`'s own envelope-integrity checksum (the same FNV1a-style
pattern, explicitly cited by `hyperion-ai-runtime`'s own former doc comment as sharing it);
`hyperion-capability::wire::WireToken`, which has no signature/MAC at all today and whose own doc
comment explicitly forward-references this exact milestone by name; `hyperion-sdk`'s
`PublishSubmission::package_hash` (hardcoded `0`); `hyperion-device`'s manifests, "trusted as
given" with no verification attempted. Each is a real, separate extension of this same new
`hyperion-crypto` primitive for a later pass, not silently assumed closed by this one.

**M10 completion note (2026-07-11):** `hyperion-netstack` gains real `MockFetchBackend`/
`MockExtractionBackend` replacements behind a new `real-http` Cargo feature (off by default, same
reason `candle` is): `ReqwestFetchBackend` is a real `FetchBackend` -- real `reqwest` blocking
client (the same sync-call-signature precedent M8's `CandleBackend` already established via
`hf-hub`'s blocking client), real rustls TLS with a *bundled* Mozilla root store
(`rustls-tls-webpki-roots`, not the OS's native trust store -- confirmed via `cargo tree -i
rustls-native-certs` that this crate's own `reqwest` resolution depends on neither
`rustls-native-certs` nor `rustls-platform-verifier` at all, so this real client needs no
`ca-certificates` package on the target rootfs to validate a real handshake), and real DNS. Real
transport-failure classification (`FetchError::Dns`/`Tls`/`Timeout`, plus a new
`ConnectionFailed` variant for a real connect-class failure that's neither) is based on this
backend's own empirically-probed error shapes -- a real nonexistent domain, a real
expired-certificate host, and a real closed local port were queried directly before writing the
classifier, not guessed from documentation; notably, a closed port in this sandbox hangs to a
real client-side timeout rather than an instant refusal, which is why `FetchError::Timeout`
(not a new variant) is what a real "connection refused" ends up classifying as here.
`HtmlHeuristicExtractionBackend` is a real, non-model `ExtractionBackend` -- real `<title>`/
`<meta name="description">`/`<p>` tag parsing via `scraper`, a new, honestly-named
`ExtractionMethod::HtmlHeuristic` (real HTML tags, no model in the loop, distinct from
`ModelBased`). `NetstackHub::web_research`'s own real fetch → quarantine-scan → extract →
resolve → merge → cache-write pipeline needed zero changes -- it was already real, only ever
gated on real input, exactly the same "the merge logic already works, only the mock input needed
replacing" shape M8 found in `hyperion-ai-runtime`.

Closing the real mechanism required finding and closing the exact same shape of gap M8 found, one
milestone later and worse: `hyperion-netstack` had zero real (non-test) callers anywhere in this
workspace, and `hyperion-compat` (the one crate whose own code *did* call it for real) was itself
never constructed outside its own tests -- a real, two-link dead chain, not one. Neither
`hyperion-console` nor `hyperion-init` (the actual booted system) depended on either crate at all.
Fixed the same way M8 fixed `assistant.respond`: `hyperion-agent-runtime` gained a new real
`web.research` Capability, dispatching to a real, caller-supplied `Arc<NetstackHub>` -- but
`Option`, not a required constructor parameter like `ai_runtime`, since only the one real
interactive console instance needs real network access wired up, and a `NetstackHub` itself needs
a real `Arc<KnowledgeGraph>` most of `AgentRuntime`'s other 13 real call sites (most of
`hyperion-federation`'s per-device instances, most of this crate's own tests) have no use for.
Zero of those 13 call sites needed touching -- a smaller mechanical footprint than M9's
verifying-key-parameter changes, let alone M8's. `hyperion-coordination`'s existing `"research"`
specialization gained `web.research` as a second baseline capability alongside its own
pre-existing `web.search` (purely additive; the real, already-proven M7 `market_research` HTN
demo still only ever needs `web.search`). `hyperion-console`'s own undecomposed-goal fallback now
recognizes a URL-shaped utterance (a minimal, deterministic substring check, the same convention
`hyperion-intent`'s own keyword lists already use) and routes to `web.research` instead of
`assistant.respond`, with a real, permissive (`"*"`) domain-egress grant registered once at
session construction -- a real interactive assistant can't pre-enumerate every domain a user
might ask about, and `hyperion-netstack::hub::domain_matches` gained real support for that
wildcard pattern (SSRF containment and the grant's own rate limit still apply independently of
which domain pattern matched; a new test proves both still fire under a `"*"` grant).

Proven for real, the same standard of proof M8 established: built `hyperion-console` with
`--features real-http` and piped a real URL-containing utterance into the real compiled binary.
It really fetched `https://example.com/` over the real network, really extracted the real page's
real `<title>` ("Example Domain") and a real summary from its real `<p>` tag content, and really
persisted it as a real `WebPage` node -- with full real provenance -- in the session's real,
on-disk Knowledge Graph file, confirmed by reading that file directly afterward. Verified
non-vacuous throughout: broke the extraction backend's own title selector and confirmed the
integration test fails, restored it; and caught a real bug in this milestone's own first attempt
at a "chaos test" for a real timeout -- an earlier version used `httpbin.org/delay/10` as the
slow endpoint, which failed for real, non-vacuously, the very first time this gate happened to
run while `httpbin.org` itself returned a real `503` instead of really delaying, exactly the
external-service-flakiness risk a test suite should avoid; replaced with the same real closed
local port (`127.0.0.1:1`) already empirically verified earlier to reliably hang to a real
timeout in this sandbox, with no dependency on any remote service's uptime. Full workspace gate
green throughout, including `cargo clippy --features real-http` and `--features candle,real-http`
together -- 467 tests passing.

**Update (2026-07-12): guest network bring-up at real boot fixed and reverified for real, on both
platforms.** Originally named as an open gap (below, preserved for the record); closed in a
follow-up pass exactly the way it was scoped: `CONFIG_IP_PNP`/`CONFIG_IP_PNP_DHCP` added to both
platforms' kernel configs, `ip=dhcp` added to every real kernel cmdline (x86_64's `grub.cfg.in`;
aarch64's own `-append` strings in `boot-test-aarch64.sh`/`boot-benchmark.sh`/
`update-rollback-test.sh`, since that platform has no single shared cmdline file to begin with).
New `crates/hyperion-init/src/linux/network_probe.rs` reads `/proc/net/pnp` (populated by the
kernel's own real DHCP client before hyperion-init ever runs) and writes a real `/etc/resolv.conf`
from it -- confirmed by direct inspection that the kernel's own format is already
`resolv.conf`-compatible line for line (`nameserver <ip>`), not assumed. Verified live on both
platforms: a real boot shows `IP-Config: Complete: ... nameserver0=10.0.2.3` from the kernel
itself, followed by this crate's own `NETWORK: wrote real /etc/resolv.conf` with the matching
content. Proven end to end, not just at the kernel-log level: built `hyperion-console` with
`--features real-http` and drove a real `https://example.com` utterance through the actual
booted console (needing a rootless `musl-tools` extraction + a hand-corrected `musl-gcc` specs
file to cross-compile `ring`'s C shim, since its own Debian package hardcodes absolute
`/usr/lib/...` paths this sandbox can't write to) -- it reported real success
(`done -- merged into the knowledge graph`), which requires real DNS resolution to have actually
worked; before this fix, no `/etc/resolv.conf` existed at all, so any hostname-based fetch would
have failed at the DNS step specifically.

Original finding, preserved for the record: the kernel config already had full real networking
(`CONFIG_NET`, `CONFIG_VIRTIO_NET`, etc.) and QEMU's own boot scripts already attached a real
virtual NIC with real outbound SLIRP networking, but nothing configured the interface itself (no
IP, no route, no resolver) once the guest actually booted. The fix named at the time (kernel IP
autoconfiguration plus a real `/etc/resolv.conf`) is exactly the fix applied above; at the time it
was named as real and scoped but not attempted in the same pass, given the core real-networking
mechanism was already fully provable through the actual compiled binary without it.

Named gaps still open, deliberately: (1) Real `schema.org`/
JSON-LD/OpenGraph structured-data parsing -- `FetchedPage::structured` is always `None` from the
real fetch backend, exactly as from the mock one; this crate's own doc comment already named this
gap before M10 and it remains exactly as deferred. (3) `web.fetch.raw` itself (the no-KG-merge
lane) has no new real dispatch path from the booted console -- only `web.research` (the
KG-merging capability the exit criteria's own "merge a real extracted entity" clause specifically
asks for) does.

**M11 completion note (2026-07-12):** new board `boot/board/hyperion-aarch64`, defconfig
`boot/configs/hyperion_aarch64_virt_defconfig`, and scripts `build-image-aarch64.sh`/
`boot-test-aarch64.sh`/`setup-aarch64-toolchain.sh` -- mirroring the existing x86_64 pipeline, but
genuinely simpler in one real respect: aarch64-virt boots via direct kernel load
(`qemu-system-aarch64 -M virt -kernel Image -append ...`), so there's no UEFI/GRUB2/GPT-partition
stage to replicate at all, just a kernel `Image` and a bare `rootfs.ext2`. `linux.config` merges
Buildroot's own proven `qemu_aarch64_virt` reference config with the identical, verbatim "Hyperion
M0" primitives block (namespaces, cgroups v2, seccomp-bpf, Landlock) the x86_64 board already
carries -- chosen over Buildroot's real Raspberry Pi board support specifically because it's
already proven to boot on plain CPU emulation, the sandbox-achievable half of this milestone;
real Pi hardware's own GPU firmware/vendor kernel fork is a separate concern (see below). Kernel
pinned to 6.18.7 (the aarch64-virt reference's own proven version, not x86_64's 6.12.47 -- cross-
architecture version skew is normal and safer than forcing an untested version onto a new machine
model). `hyperion-init`/`hyperion-console` cross-compiled for `aarch64-unknown-linux-musl`.

Two real, load-bearing gaps found and fixed, not just Buildroot config work:

1. **`hyperion-trust-boundary`'s seccomp filter was silently x86_64-only.** It hardcoded
   `TargetArch::x86_64` in `SeccompFilter::new`, and its allowlist unconditionally included
   `SYS_arch_prctl`/`SYS_open`/`SYS_access`/`SYS_poll` -- x86_64-only syscalls with no aarch64
   equivalents at all (`arch_prctl`'s whole job, setting up the userspace thread pointer, is done
   by the kernel writing `TPIDR_EL0` directly on aarch64; the legacy dual syscall forms aarch64's
   newer "asm-generic" table never had are already covered by the shared `openat`/`faccessat`/
   `ppoll` entries already on the list). Undetected until now because nothing had ever actually
   tried to compile this crate for a non-x86_64 target. Fixed with `#[cfg(target_arch = "x86_64")]`
   on the x86_64-only syscalls and a small `host_target_arch()` helper selecting the right
   `TargetArch` per `target_arch` -- verified by cross-compiling for
   `aarch64-unknown-linux-musl` cleanly, syscall list unchanged for the already-proven x86_64 path.
2. **The same discovery, one layer down, breaks macOS too -- found while verifying this milestone's
   own fix didn't regress other platforms.** `landlock`/`seccompiler` (hyperion-trust-boundary) and
   raw `SCHED_DEADLINE`/`sched_setattr`/`sched_setscheduler` calls (hyperion-cgroups's `realtime`
   module) are Linux-only APIs that were unconditional dependencies/calls -- meaning
   `cargo build --workspace --all-targets` had never actually succeeded on macOS since M2/M4
   landed, a real, live CI failure once actually looked at (see below), not a hypothetical. Fixed
   the same way `hyperion-init`'s own `main.rs` already handles this: real implementation behind
   `#[cfg(target_os = "linux")]`, nothing pretending to work elsewhere. Verified clean against both
   `x86_64-apple-darwin` and `aarch64-apple-darwin` (the actual `macos-latest` runner architecture)
   via `cargo check --target`, not just reasoned about.

Separately, this same pass found and fixed two more real, live CI gaps unrelated to aarch64 itself
(caught because they were sitting in the same `cargo test --workspace` run being watched closely):
`hyperion-cgroups`'s real-fairness test read `cpu.stat` only once, right after cgroup creation,
then panicked on a *second*, later read after its burner processes exited -- some CI runners
(confirmed live on GitHub Actions ubuntu-latest) prune a delegated cgroup once it briefly has no
live member process, a real timing window this test's own real fork/join/burn/exit sequence can
land in; fixed by reading each class's stat immediately after its own `wait_all` and treating
failure there as the same graceful skip its existing precondition checks already use, not a hard
panic. And `hyperion-trust-boundary`'s and `hyperion-supervisor`'s own Landlock/seccomp integration
tests each build a real musl probe/service binary at test time -- needing `x86_64-unknown-linux-musl`
installed, which the CI `test` job's toolchain step never requested (only the separate `boot-image`
job did), so both tests had likely been silently failing on every prior ubuntu CI run, masked by
whichever of the other bugs surfaced first. All three fixed and confirmed against a real, green
GitHub Actions run before this milestone's own work continued.

Proven the same standard of proof every real milestone here is held to: built the real image via
`build-image-aarch64.sh` (its own dedicated Buildroot `O=output-aarch64` output directory --
switching defconfigs in a *shared* output directory the first time around left a stale, wrong-
architecture host toolchain behind, a real Buildroot gotcha diagnosed and fixed along the way, not
guessed at) and booted it for real via `boot-test-aarch64.sh`. The real kernel boots, mounts the
real rootfs, execs `/hyperion-init` as real PID 1, the real M5 supervision tree starts, and the
real M7 console prompt ("Hyperion -- tell me what you'd like to do.") comes up -- the identical
`"Humans express goals"` banner the x86_64 platform's own boot-test asserts on, now proven on real
aarch64 CPU semantics under emulation, not just reasoned about.

Named gap, deliberately not attempted here, mirroring M1's own precedent for "boots from a real
USB drive": **this milestone's own literal exit criterion is booting on real Raspberry Pi
hardware**, which this sandbox cannot perform or verify. A real Pi 4/5 deployment needs its own,
substantially different board support Buildroot already has (`raspberrypi4_64_defconfig` /
`board/raspberrypi4-64/`) -- a Raspberry-Pi-Foundation kernel fork, proprietary VideoCore GPU
firmware blobs, and an SD-card/firmware-first boot flow with no UEFI or direct-kernel-load
equivalent at all, none of which the generic `aarch64-virt` board this milestone builds against
attempts to replicate. What real QEMU CPU emulation genuinely proves -- the kernel config's real
M2/M4 enforcement primitives and Hyperion's own Rust logic really work on real aarch64 instruction
semantics, not just x86_64 -- is exactly the same category of proof M1's own QEMU boot-test
provides for the x86_64 UEFI-USB claim: real, load-bearing, and explicitly not a substitute for
the literal hardware claim. Real Raspberry Pi 4/5 hardware boot remains a real, separate,
user-performed verification step this document flags but does not block on, per its own Execution
Mode section.

**M12 completion note (2026-07-12):** new `boot/scripts/boot-benchmark.sh` + `boot-benchmark.py`,
reusing M7's own console-drive.py protocol (a real Unix-socket-backed serial console that can both
observe output and send a real typed utterance) but adding real wall-clock timestamps anchored to
`t0` -- a timestamp the shell script records *before it even launches qemu* -- so the reported
numbers cover real qemu startup + real firmware/bootloader + real kernel boot + real console
readiness + a real Intent round-trip end to end, exactly the "firmware -> login/shell -> first
real Intent handled" span this milestone's own text asks for, not just a connect-to-response
window. Measures two real milestones per boot: console-ready (the M7 prompt appears) and
first-real-intent (a real utterance's real response fully printed), compared against docs/36's own
~4.5s total cold-boot budget as one honest end-to-end number -- that budget's own per-phase
boundaries are a from-scratch microkernel's L0-L6 layers, which don't correspond 1:1 to this
roadmap's real Linux-hosted MVP boot sequence (see this document's own §0 Decision Record), so
forcing a false per-phase attribution would be less honest than one real total against the same
overall target.

Real, measured results, both platforms:

| Platform | Console ready | First real Intent | vs. ~4.5s budget |
|---|---|---|---|
| x86_64 | 18.8s | 19.5s | over, ~4.3x |
| aarch64 | 3.2s | 3.9s | **under budget** |

Named cause for both being over-or-barely-under what real hardware would show: **this sandbox has
no `/dev/kvm` access** (already named in memory as a standing constraint since M0), so every real
boot measured here runs under QEMU's TCG software emulation, not real silicon or even KVM
hardware-accelerated virtualization -- substantially slower at every phase (kernel decompression,
init, the real Intent→Agent round-trip), and not a Hyperion regression. Real hardware timing (with
real firmware handoff speed and zero emulation tax) is M12's own literal exit criterion and remains
a real, separate, user-performed measurement this sandbox cannot take -- named as a deferred
handoff, mirroring M1's and M11's own precedent for their respective real-hardware claims.

One real, fixable, non-TCG cause was found and fixed along the way, not just named: the initial
x86_64 measurement (32.8s) included GRUB waiting a real 5 seconds at its boot menu before
auto-selecting the only entry (`set timeout="5"` in `grub.cfg.in`) -- 20x docs/36's own ~250ms
budget for the *entire* firmware/bootloader handoff phase, and pure waste for a single-entry,
headless appliance boot (`init=/hyperion-init`) with no human ever present to pick a different
entry. Reduced to `set timeout="0"`, rebuilt, and re-measured for real -- 32.8s -> 19.5s, a real
~13s reduction confirmed by re-running the same benchmark, not assumed from the config change
alone. The dominant remaining gap versus both the budget and the aarch64 platform's own
much-faster number is real TCG/no-KVM overhead: aarch64's own boot has no firmware/bootloader
stage at all to begin with (direct kernel load, per M11's own completion note), so it pays the
same real TCG tax on a strictly shorter real critical path, which is the concrete, measured reason
it lands under budget in this same sandbox while x86_64 does not.

**M13 completion note (2026-07-12):** extended `hyperion-release-gate` with a new
`HardwareReleaseCriteria` type (`image_build_reproducible`, `boot_tested_platforms`,
`staged_update_rollback_verified`) and widened `evaluate_release` to gate on it alongside the
existing suite/benchmark checks -- the same "caller supplies an already-computed real fact, this
crate never re-derives it" shape every other criterion here already uses. A platform simply
*absent* from `boot_tested_platforms` is treated the same as an explicit failure (an untested
platform is not a passing one); 6 new tests cover the extension, all passing alongside the
existing suite.

Real staged update + rollback, docs/41 Phase 10's literal exit criterion, finally proven against a
real system: new `crates/hyperion-init/src/linux/update_probe.rs`, opt-in via a
`hyperion.run_update_test=1` kernel cmdline parameter (so it's inert on every other boot -- M7/
M11/M12's own boot tests never pass it) and `boot/scripts/update-rollback-test.sh`. Inside the
real booted aarch64 guest (chosen over x86_64 here specifically because aarch64 boots via direct
kernel load, so the trigger parameter goes straight on QEMU's own `-append`; x86_64's cmdline is
baked into a GRUB config embedded in the disk image itself, which this pass didn't need to touch
for this proof), it writes a real node to a dedicated real `KnowledgeGraph` on the M6 persistent
data partition, applies a real, Ed25519-signed `UpdateOrchestrator::apply_update` against it (a
real health-gated staged rollout, a real `hyperion_recovery::RecoveryService` pre-update
snapshot), writes new data representing the update's own real payload effect (the orchestrator
itself has no migration DSL -- applying the actual change is the caller's job, same as any real
update needs to do), then calls a real `update_rollback`, which really restores the pre-update
snapshot via `RecoveryService::restore_to`. Confirmed live: `applied real update to v1... real
data now = "POST-UPDATE-MODIFIED"`, then `PASS -- real rollback restored real data to
"pre-update-original" (no data loss), real active version back to v0`. Verified non-vacuous by
temporarily skipping the real rollback call and confirming the probe correctly reports `FAIL` with
the stale (unrestored) data and version, then restoring it and reconfirming a clean `PASS`.

Two real, non-obvious bugs surfaced and fixed along the way, neither guessed at:
1. **Two virtio-mmio block devices enumerate in a different order than their `-device` flags on
   the command line** -- adding the real M6 data disk alongside aarch64's rootfs disk swapped
   which one became `/dev/vda` vs `/dev/vdb` relative to what a single-disk boot (and
   `storage_probe.rs`'s own hardcoded `/dev/vdb` data-device assumption) expects, discovered via a
   real boot that kernel-panicked trying to exec `/hyperion-init` from the wrong (64MB, data-only)
   disk. Fixed by reordering this test's own `-drive`/`-device` pairs to match the enumeration
   `storage_probe.rs` already assumes, rather than touching that shared production code for a
   test-script-specific device-count difference.
2. **A fresh `UpdateOrchestrator`'s `active_version` starts at 0** (its own `unwrap_or(0)`
   default, never set by any constructor) -- this probe's first attempt used `from_version: 1`,
   which `compatibility_check`'s real schema-compatibility check correctly (not a bug) rejected as
   stale. Traced through the actual check before assuming a plausible-looking version number was
   right; fixed to `from_version: 0, to_version: 1`.
3. (Separately, not a bug in new code but a real scheduling conflict) the existing M6
   crash-consistency probe's own write loop is deliberately slow (200k iterations, meant to still
   be mid-flight whenever `storage-crash-test.sh`'s own hard kill lands) and, sharing the same
   fresh data disk, blocked this sequential boot path from ever reaching the M13 probe. Fixed by
   running the M13 probe first in `run_supervision_tree()` -- both probes are independently gated
   on their own dedicated files, so the order between them was otherwise arbitrary.

Signed, versioned, `dd`-able release artifact: two new `hyperion-release-gate` binaries,
`sign-release` (real BLAKE3 hash of an image's own bytes, real Ed25519 signature over that hash
via M9's real device `Keystore`, written as a `.release.json` manifest) and `verify-release` (recomputes
the hash directly from the image's own bytes -- never trusts a manifest's recorded hash blindly --
and checks the real signature against the manifest's own recorded verifying key), plus
`boot/scripts/package-release.sh` tying both real platform images together into one versioned
release directory. Verified non-vacuous twice: a byte-tampered copy of the real signed image
correctly fails the hash check; the real image against a manifest with a fabricated verifying key
correctly fails the signature check specifically (proving the signature check, not just the hash
check, does real work).

**Update (2026-07-12): image build reproducibility fixed and reverified for real, on both
platforms.** Originally found and named as an open gap (below, preserved for the record); closed
in a follow-up pass by actually chasing down every remaining cause via repeated rebuild-and-diff
cycles, not just the first one found. Three independent, real, non-reproducibility causes existed
simultaneously, each confirmed by a real rebuild-and-hash-compare before and after its own fix:

1. **The rootfs's own `mkfs.ext4`-assigned filesystem UUID** (random by default) -- leaks into
   both GRUB's `root=PARTUUID=...` cmdline and the GPT partition table on x86_64. Fixed via
   `BR2_TARGET_ROOTFS_EXT2_MKFS_OPTIONS`'s own `-U <fixed-uuid>` (a real Buildroot-supported knob,
   found by reading `fs/ext2/Config.in` directly rather than reaching for a post-build patch
   script).
2. **The rootfs's own ext4 directory hash seed** -- generated randomly by `mke2fs`
   *independent* of `-U`, deliberately, as a real anti-hash-flooding security measure for htree
   directory lookups. Fixing only the UUID was not sufficient (confirmed by a real rebuild that
   still produced a different hash); found via a direct `dumpe2fs -h` diff between two builds'
   superblocks, not guessed at. Fixed via the same `MKFS_OPTIONS` string's `-E
   hash_seed=<fixed-uuid>`.
3. **(x86_64 only) The EFI partition's own `mkdosfs`-assigned FAT volume ID** -- also
   time-derived by default. Found the same way: after the rootfs.ext2 file itself became
   byte-identical build to build, the full `disk.img` still differed; bisected component by
   component (`bzImage` was already identical; `efi-part.vfat` was not) down to this specific
   field. Fixed via genimage's own `vfat` image type's `extraargs = "-i <fixed-hex-id>"`.
   genimage's own `disk-uuid` (GPT header) and the boot partition's own `partition-uuid` (both
   genimage's own docs say "defaults to a random value" when unset) were pinned in the same pass,
   found by reading genimage's actual README rather than assumed to already be covered by the
   rootfs-level fixes.

Verified reproducible for real on both platforms: two consecutive rebuilds of identical source
now produce byte-identical `disk.img` (x86_64) and byte-identical `Image`+`rootfs.ext2`
(aarch64), confirmed via direct SHA-256 comparison each time, not assumed after the first fix
"looked right." A real boot-test was re-run against the fixed x86_64 image afterward to confirm
none of these changes broke anything real.

Original finding, preserved for the record: building the identical x86_64 source twice, back to
back, produced two different SHA-256 image hashes (confirmed directly, not inferred) --
`post-image.sh` reads the rootfs's own randomly-assigned filesystem UUID and embeds it as
`PARTUUID` into both GRUB's cmdline and the GPT partition table, meaning the disk image's own
bytes differed on every build even when the compiled binaries and rootfs contents were
byte-identical. At the time, the fix was named as real and scoped but not attempted in the same
pass; the follow-up above is that attempt, and it found two *more* independent causes beyond the
one originally diagnosed.

Named handoff, per this document's own explicit standing pause condition (§0a): **the literal M13
exit criterion -- a real USB drive, written from this tagged release image, booting on both
real reference-platform hardware and passing a real smoke test -- needs the user's own action.**
Everything upstream (building both images, signing them, verifying the signature, boot-testing in
QEMU) is done and real; the actual write to a real device, and the real hardware boot it enables,
is real-world, physical-device, irreversible-if-wrong territory this session does not perform
unprompted, exactly as this document's own Execution Mode section requires.

Decided (2026-07-12, by the project owner): the x86_64 image stays a plain `.img` (no ISO
conversion) -- it is already a real, complete GPT disk image (a real EFI System Partition with
GRUB2 plus a real root partition), exactly the shape meant to be written directly to a device,
matching the Buildroot reference board (`board/pc`) this board was modeled on. Wrapping it in an
ISO9660/El-Torito hybrid image first would need new build tooling this pipeline doesn't have
(`grub-mkrescue`/`xorriso`) to solve a problem the `.img` doesn't have. For the actual write step,
**Balena Etcher** is the recommended tool over raw `dd` -- it writes a raw `.img` natively (no
conversion needed), and has real safety guardrails (hides system drives from its target list,
verifies the write afterward) that a bare `dd` invocation doesn't. UNetBootin was considered and
rejected: it's unmaintained and built around extracting a live-CD ISO's own filesystem and
reconstructing a syslinux/GRUB config, not around writing a custom GPT+GRUB2-EFI raw image
byte-for-byte -- a worse fit than either `.img`+Etcher or a plain `dd`. This decision covers the
x86_64 platform only: aarch64 has no combined bootable image for real hardware at all yet (M11
used QEMU's generic `virt` reference rather than real Raspberry Pi board support -- see that
milestone's own completion note), so there is nothing to write to a real Pi's SD card yet either
way.

**Update (2026-07-12): automated, downloadable `.img` releases -- real CI now builds, signs, and
publishes both platforms on every version tag.** Closes the gap between "a `package-release.sh`
script exists" and "a user can actually download a release." Four real pieces landed together:

1. **A real release-signing identity.** `hyperion-release-gate`'s new `gen-signing-key` bin
   generates a real Ed25519 keystore via M9's own `Keystore::open_or_create` and prints its
   verifying key in hex. Run once, for real, against this repo:
   `b5c19b1e890fed3e164342f0285f6a1a1635d724f2284a2ebe00589a122ac90a` -- now published in
  [README.md](../README.md) as the independent, out-of-band value a downloader checks a manifest's
   own recorded verifying key against (never trust a key recorded only inside the thing it's
   meant to authenticate). The private seed itself was base64-encoded and stored as this repo's
   real `HYPERION_RELEASE_SIGNING_KEY` GitHub Actions secret (confirmed via `gh secret list`
   after setting it) -- never committed to the repo. Verified end-to-end before trusting it for
   anything: signed a real probe file, verified it (`PASS`), and independently confirmed the
   printed hex fingerprint matches the manifest's own serialized `verifying_key` bytes.
2. **`.github/workflows/release.yml`** (new): triggers on `v*` tag pushes (or manual
   `workflow_dispatch` with an explicit version). Two parallel jobs build and boot-test each
   platform exactly the way `ci.yml` already does (same scripts, same QEMU boot-test gate -- an
   image that fails its own boot-test is never signed or published); a third `publish` job
   downloads both, decodes the real signing key from the secret, signs all three artifacts
   (x86_64 image, aarch64 kernel, aarch64 rootfs) with the *existing* `sign-release` bin,
   independently re-verifies every signature with `verify-release` before publishing anything,
   then creates a real GitHub Release via `gh release create` with the signed images and their
   `.release.json` manifests as downloadable assets.
3. **aarch64 image building added to regular CI**, not just at release time: a new
   `boot-image-aarch64` job in `ci.yml`, mirroring the existing x86_64 `boot-image` job
   (`build-image-aarch64.sh` + `boot-test-aarch64.sh`, its own Buildroot output-directory cache
   key) -- an aarch64-only regression now fails CI on every push, rather than surfacing only when
   someone eventually cuts a release.
4. **`README.md`** (new -- this repo had none before): points users at the Releases page, gives
   step-by-step Balena Etcher flashing instructions for the x86_64 `.img`, and documents the
   verifying-key check above.

Not attempted here, unchanged from directly above: the literal `dd`/Etcher-to-a-real-device step
and the real hardware boot it enables. This work makes that step a `git tag && git push --tags`
plus a browser download away, but still stops short of performing it, per this document's own
standing pause condition.

**M8 follow-up (2026-07-13): "does 'launch my startup' actually produce anything real?" -- no,
and now it does.** Asked directly, by the user, after this session's own README addition described
the flow in plain language ("everything learned gets written into the knowledge graph"). Traced
end to end rather than assumed: `hyperion-coordination::catalog::required_capabilities_for` maps
`business_model`/`branding`/`legal_formation` to `document.draft` and `market_research` to
`web.search`; both dispatched to `hyperion-agent-runtime::stubs::dispatch`'s two hand-written
canned strings (`"Stub draft document about '{topic}'."` / `"stub finding for query '{query}'"`);
`hyperion-coordination::engine::allocate`'s own args never even sent the `topic`/`query` keys that
stub read (`{"task": ..., "force_fail": ...}`); and worse, `allocate`'s own dispatch match was
`InvokeOutcome::Result(_)` -- the underlined-in-retrospect `_` discarding a capability's real
output outright, so not even that broken placeholder text ever reached anywhere a person or this
console's own rendering could see it. A real "launch my startup" run produced four lines that
each said `Done` and nothing else.

Fixed at the root, not patched at the rendering layer: `hyperion-agent-runtime` gains two new real
dispatch functions, `dispatch_document_draft`/`dispatch_market_research`, following the exact
`dispatch_assistant_respond`/`dispatch_web_research` pattern M8/M10 already established -- a real
`LocalAiRuntime::infer` call, refactored behind one shared `run_inference` helper. `web.search`'s
own result carries an explicit `"note"` field ("AI-generated research notes, not a live web
search"): this workspace still has no real search-provider integration (no API key, no consent
gate, nothing) -- inventing one here would trade one dishonest gap for another; a real search
provider (à la the OpenAI/Anthropic/Gemini phase) is its own, later, separate feature, named
rather than faked. `stubs::dispatch` itself is untouched -- `hyperion-federation`/
`hyperion-api-gateway` both call it *directly*, bypassing `AgentRuntime::invoke` entirely, as a
deterministic fixture for their own, unrelated tests.

`hyperion-coordination::engine::allocate` no longer discards the result: `TaskNode` gains a real
`result: Option<serde_json::Value>` field, and a completed task's real output is also written into
the Knowledge Graph as a new `"task_result"` node, linked back to the task's own Intent node via a
real `"produced"` edge -- so `hyperion-console`'s own `/recall`/`/why`/`/related` (this session's
immediately-preceding work) can actually surface it, not just the task's bare predicate name.
`SharedPlan` gains `root_utterance` (captured once at `create_session`, from the root Intent's own
`raw_utterance`), so each task's real capability dispatch gets genuine context -- what the user
actually asked for -- via a new `"goal"` arg, not just its own predicate name. The console's own
rendering (`ConsoleSession::render_task_detail`) now shows `"Done -- <real generated text>"`
instead of a bare status word.

Fixing the actual gap surfaced a second, real, previously-dormant one: `hyperion-federation`'s
`FederationHub::join_device` constructs each simulated device's own `LocalAiRuntime` with no
model ever registered (harmless while `web.search`/`document.draft` were canned stubs; a real,
previously-unexercised requirement the instant they dispatch through real inference, exactly like
`assistant.respond` always has). Fixed the same way `hyperion-console`'s own `build_ai_runtime`
does: a small, real, signed `ModelDescriptor`, registered via a throwaway `tempfile::tempdir`-backed
`Keystore` generated fresh per `join_device` call (this hub has no real, lasting per-device
identity to reuse, and doesn't need one just to prove a simulated device's own local inference is
genuinely callable) -- `tempfile`, not a fixed path, specifically so concurrent calls (parallel
tests in the same process) can never collide over the same key file. One pre-existing test
(`hyperion-federation`'s own `picks_the_lowest_latency_feasible_candidate`) asserted on the old
canned stub's exact string; updated to assert on the new real, still-fully-deterministic
`MockBackend`-echoed text instead.

Proven for real, not just claimed: a live run of "I need to launch my startup" now shows real,
task-specific generated text for all four sub-tasks (not a canned placeholder), `/recall`/
`/related`/`/why` surface the real `"task_result"` node and its new `"produced"` edge (market_
research's own connection count rose from 2 to 3, precisely because of this new edge), and the
full workspace test suite (every crate, default features) passes with zero failures. The one
thing this does *not* claim: `market_research`'s own output is real, model-generated reasoning,
honestly labeled as such -- not a verified live web search, which remains explicitly future,
separate work.

**M8 follow-up (2026-07-13): "it takes a while to get any kind of feedback" -- concurrent
dispatch, live progress, and truncated previews.** Reported directly, by the user, after switching
the console to a real cloud backend (`/backend openai ...`) and driving "launch my startup": every
one of the four real capability dispatches the previous follow-up made real now also runs through
a real network round trip when the active backend is a real cloud provider, and the console
rendered nothing at all until the *entire* plan (all four tasks, across three ticks) converged.

Traced to two real, previously-shipped bottlenecks, not assumed: `hyperion_agent_runtime::
AgentRuntime::invoke` held `self.instances`' single global lock across the *entire* call,
including the real capability dispatch -- so two independent tasks in the same tick (`business_
model`/`branding`, both landing on the one reused writer instance -- see the "one research + one
writer instance, reused across tasks" test) would serialize behind that lock no matter how many
real OS threads a caller spawned to dispatch them. Fixed by splitting `invoke` into a locked
"prepare" phase (lifecycle/Broker/Scheduler checks -- fast, in-memory) and an unlocked "dispatch"
phase (the real capability call). One layer deeper, `hyperion_ai_runtime::LocalAiRuntime::infer`
had the exact same shape of bug: it held the `backend` mutex across the entire real `generate`
call. Fixed by storing `Arc<dyn InferenceBackend>` behind that mutex instead of a bare `Box`, so
`infer` clones the `Arc` (a cheap refcount bump) and drops the lock before calling `generate`.
Both fixes proven directly, not just inferred from reading the code: a real `SlowBackend` test
fixture (`std::thread::sleep`, standing in for a real slow cloud round trip) drives two concurrent
calls in each crate and asserts the real elapsed wall-clock time is close to *one* delay, not two
-- these tests genuinely failed (~400ms) before the fix and pass (~200ms) after it.

With both bottlenecks closed, `hyperion_coordination::CoordinationSession::allocate` now
dispatches every ready task in a tick concurrently: a real three-phase split (prepare -- under
`self.plans`' lock, sequential, since each task's own least-loaded-instance assignment must see
the *previous* assignment in the same tick already reflected; dispatch -- no lock at all, via
`std::thread::scope`; apply -- back under the lock, recording each real result). Its own public
signature (and every existing call site) is unchanged. A new test proves a tick with two ready
tasks (`business_model`+`branding`) completes in ~200ms against a real 200ms-per-call `SlowBackend`,
not ~400ms.

Separately, `hyperion-console` now gives real, live feedback instead of staying silent for the
whole plan: `ConsoleSession::handle_utterance_with_progress` (the plain `handle_utterance` every
existing test still uses is a thin wrapper with a no-op callback) calls a caller-supplied
`on_progress` once per tick, naming each task that just completed -- `main.rs` wires this straight
to a real `println!`, so a real user watching a real multi-tick plan sees each tick's own progress
the moment it lands, not after the whole plan converges.

Also fixed, per the same report: a real cloud model's own real answer (several paragraphs, for
something like "draft a business model") used to print in full, inline, for every task -- shown in
the screenshot that prompted this. `hyperion_console::graph_explorer::preview` (a new, shared
helper) caps this to one line (first line only, truncated past 100 characters) everywhere a
capability's result appears in a *list* context (`/recall`, `/related`, and the console's own
per-task summary line, which now reads `"Done -- <preview> (see \"/recall <task>\" for the full
text)"`) -- deliberately not the only place to ever see the real content: `/why` on a
`"task_result"` node (found via that same `/recall`/`/related` pointer) still shows the complete,
untruncated text, since that command's whole point is "tell me everything about this one thing."

Verified: new real, timing-based concurrency tests in all three crates (`hyperion-ai-runtime`,
`hyperion-agent-runtime`, `hyperion-coordination`); a new console test proving the progress
callback fires per tick with the right task names, and that it never changes the final rendered
result; a rewritten "no real content dumped inline" test proving the preview/pointer shape while
still proving the full honesty-caveat text survives via `/recall` -> `/related` -> `/why`. Full
workspace test suite (every crate, default features) passes with zero failures; `cargo clippy`/
`cargo fmt --check` clean workspace-wide.

**M8 follow-up (2026-07-13): a real spinner while each tick is still in flight.** Requested
directly, by the user, as a follow-on to the progress-callback work directly above: `Done` alone,
printed only once a tick's blocking dispatch had already returned, still gave no feedback *while*
a real, slow capability call was actually running.

Raised one real, deliberate design question before writing any code, since it's a genuine
accessibility trade-off this project treats as architecture, not an afterthought: a `\r`-redrawing
spinner is a well-documented bad pattern for screen readers even on a real interactive terminal
(unlike a static banner, each redraw risks being re-announced as new content). Presented the
options -- animated by default, animated with an explicit off-switch, or a static non-animated
"working on X" line -- and the user chose animated-by-default, accepting that trade-off knowingly
rather than having it decided silently.

Required a real, previously-missing event, not just a UI change: `hyperion-console`'s own
`TaskProgress` callback enum (`Starting(Vec<String>)` / `Done(String)`) replaces the plain `&str`
progress callback from the follow-up directly above -- `Starting` fires with every task about to
run in a tick, via a new `hyperion_coordination::CoordinationSession::ready_task_descriptions`
read-only peek, called *before* that tick's own real, blocking `allocate` call, not only `Done`
after it returns. `ready_task_descriptions` shares its own real "is this task ready" predicate with
`allocate`'s existing internal one (extracted into one `is_ready` helper) rather than duplicating
the filter logic somewhere it could quietly drift out of sync.

`main.rs`'s own new `Spinner` (a real background thread, braille frames, `\r`-redrawn every 80ms,
gated behind the same `IsTerminal` check the startup banner already established -- a piped or
redirected caller never constructs one at all) starts on `Starting` and stops -- clearing its own
line with plain spaces, not an ANSI escape sequence, matching this crate's existing no-ANSI
convention -- on the matching `Done`; a plan that errors out of its own tick loop before a real
`Done` ever fires is still caught (stopped once the whole utterance-handling call returns) so a
spinner can never animate forever.

Proven live, for real, not just by reading the code: a hand-rolled local HTTP fixture server (a
real, deliberate 1.5s response delay, not `hyperion-ai-runtime`'s own deterministic `MockBackend`)
driven through the console's own real `openai-compat` backend switch, captured over a genuine
pseudo-terminal (`pty.openpty`, since this session's own tooling only ever sees piped, non-tty
output) -- the raw byte capture shows real `\r`-prefixed spinner frames redrawing during the real
1.5s wait, a clean clear (spaces + `\r`, no leftover characters) the instant the real, delayed HTTP
response lands, and that same response's own real text flowing through to the final rendered line.
New tests: `hyperion-coordination::ready_task_descriptions_previews_exactly_what_the_next_allocate_
call_will_dispatch` (the peek matches each tick's real dispatch set exactly, across all three
ticks); `hyperion-console`'s own progress test rewritten to assert `Starting`/`Done` fire in the
right order with the right task names. Full workspace test suite (every crate, default features)
passes with zero failures; `cargo clippy`/`cargo fmt --check` clean workspace-wide.

**M8 follow-up (2026-07-13): `/result <task>` -- a direct, graph-edge-based path to a task's full
result, plus "how do I access each task's result / edit or steer one with more information?"**
Asked directly by the user after the progress/spinner work above: given `/recall market_research`
etc. only ever found "a planned task: market_research" (never its real output), reaching a
completed task's full text took a `/recall` -> `/why` -> `/related` -> `/why` detour, demonstrated
live with `legal_formation` -- whose own real generated prose said "legal formation," never the
literal snake_case string, so a plain `/recall <task>` text search couldn't even find it as a
*result* (only as the task node itself, via its `predicate` metadata field). The user also asked
where results are stored (already-real: `TaskNode.result`, plus a linked `"task_result"` Knowledge
Graph node written by `hyperion-coordination::allocate`) and for a way to redo a task with more
information.

New `hyperion_console::graph_explorer::GraphExplorer::result(task_name)` (the `/result <task>`
meta-command): finds the task node by **exact, case-insensitive `predicate` match** (not a text
search), then traverses one real hop to its linked `"task_result"` node via the actual `"produced"`
edge `allocate` already records, and shows the complete, untruncated text directly -- no numbered
detour. `render_task_detail`'s own inline hint now points at `/result <task>` instead of
`/recall <task>`, for the same reason.

**The redo/steer half:** `hyperion_coordination::TaskNode` gained `extra_context: Option<String>`,
threaded into `prepare_dispatches`'s own dispatch args and appended to the real prompt
`hyperion_agent_runtime::AgentRuntime::dispatch_document_draft`/`dispatch_market_research` build
(a new shared `append_extra_context` helper). New `CoordinationSession::amend_task(session_id,
task_name, extra_context)`: resets the named task to `Unassigned` with `attempts` reset to `0` --
deliberately not the same, separate, bounded `RETRY_LIMIT` an automatic failure retry consumes, so
a user-initiated redo is never rate-limited by that budget -- clears its now-stale `result`, and
un-blocks any dependent still marked `Blocked` because of it (`propagate_blocking` only ever adds
that mark, never removes it, so nothing else would ever re-evaluate it otherwise). Returns the
descriptions of any already-`Done` dependents, so a caller can warn they used the now-superseded
result -- deliberately does **not** cascade an automatic redo to them: silently invalidating and
re-running an entire downstream chain the user didn't ask about would be a surprising side effect,
not real user control.

`hyperion-console`'s `/redo <task> <extra instructions>` (wired into
`handle_utterance_with_progress`, ahead of `handle_meta_command`, so it can still thread the real
`TaskProgress` callback through -- unlike every other meta-command, a redo re-runs a real,
potentially slow capability dispatch and deserves the same spinner/progress support a plan's first
run gets): calls `amend_task`, then re-drives the plan's own ticks via a new
`drive_ticks_to_completion` helper extracted from `run_decomposed_plan` (both call sites need the
exact same tick-and-report loop, just starting from different plan state) -- the redone task has no
unmet dependency (it just ran), so the very next tick picks it straight back up. `ConsoleSession`
remembers the most recent plan's own session id (`last_plan_session_id`) so `/redo` knows which
plan a task name belongs to; gives an honest "nothing to redo yet" reply, not a panic, before any
plan has run.

A real, previously-shipped bug found while writing this feature's own tests, not by reading code:
`hyperion_ai_runtime::MockBackend::generate`'s deterministic echo truncated its prompt to 200
characters -- a redo's own base prompt plus its `extra_context` suffix could exceed that, so the
steering text (or the whole "Additional instructions from the user:" prefix) was silently cut off
before ever reaching the echoed result, even though the real prompt built inside
`dispatch_market_research` was proven correct at every step via targeted debug output. Fixed by
raising the cap to 500 characters -- confirmed no existing test relied on the exact 200-character
boundary.

A second, more consequential real bug surfaced by the same test suite: `GraphExplorer::result`
(and its sibling task-lookup) disambiguated between multiple same-named graph nodes using
`updated_at` alone, which `hyperion_console::session::now()` stamps at one-second granularity -- a
redo that completes within the same real wall-clock second as the task's original dispatch (every
test, and any fast-enough real run) ties on `updated_at`, and a plain reverse sort then silently
fell back to whichever node the graph traversal happened to return first, which was the **old,
stale** result, not the new one -- confirmed live end-to-end with a real running console (`/redo`
then `/result` immediately after) before landing the fix. Fixed by breaking ties on `NodeId`
instead: `hyperion_storage::Engine::put_object` assigns `NodeId`s from a real, monotonically
increasing counter for every freshly created node, so it's a race-free "which one is newer" signal
regardless of wall-clock resolution, since a `"task_result"` node is only ever created fresh, never
updated in place.

Verified: `hyperion-coordination`'s own `amend_task_*` tests (reset + extra-context propagation,
dependents warning, unknown-task rejection); `hyperion-console`'s new `redo_and_steer` test module
(seven tests: no-prior-plan, bare argument, unknown task, real extra-context propagation through a
full redo + `/result` round trip, case-insensitivity, dependents warning, and that `/redo` fires
the same real `Starting`/`Done` progress events a plan's first run does) plus four new `/result`
tests. Full workspace test suite (every crate, default features) passes with zero failures --
524 tests, up from 515 before this follow-up; `cargo clippy`/`cargo fmt --check` clean on every
touched crate. Live-verified end to end in a real running console binary: `/redo market_research
focus on the European market only` followed by `/result market_research` shows the real,
regenerated text with the steering text folded in, not the stale original.

### 4. Milestones

Each milestone below states what it delivers, what from the existing 31 crates is genuinely
reusable (the algorithms, not the process model), what's net-new, and its exit criteria — mirroring
docs/41's own phase-definition shape so this roadmap reads as a continuation of that document, not
a break from its conventions.

#### M0 — Toolchain, Decision Record, QEMU Harness

**Delivers:** a working build pipeline that can produce a bootable disk image and iterate on it in
seconds, before any real Hyperion logic is involved.
- Record this document's decision in the repo itself (e.g. a short addendum note at the top of
  `docs/03-kernel-architecture.md` pointing here, so a reader of the spec isn't misled into
  thinking the shipped system matches it exactly).
- Pick **Buildroot** as the image-building tool: it produces a minimal, reproducible root
  filesystem plus kernel plus bootloader as one buildable, `dd`-able image, supports a custom init
  (needed for M5), and has first-class QEMU output for fast iteration — this avoids reinventing a
  distro pipeline by hand (debootstrap + manual initramfs + manual GRUB wiring) for no benefit at
  MVP stage.
- Kernel: a current stable LTS Linux kernel, custom-configured (menuconfig) for a minimal image
  with cgroups v2, user namespaces, seccomp-bpf, and Landlock enabled — these four are Milestone
  2/4's real enforcement primitives and must be confirmed present in the kernel config before
  anything is built on top of them.
- Bootloader: GRUB2 (or `systemd-boot` if the simpler EFI-stub path is preferred) via Buildroot's
  standard UEFI boot flow.
- QEMU + OVMF (UEFI firmware for QEMU) as the primary dev loop: boot the image in QEMU before ever
  writing to a physical USB drive.
**Exit criteria:** `qemu-system-x86_64 -bios OVMF.fd -drive file=<image>,format=raw` boots to a
login prompt from an image Buildroot produced, with no Hyperion code involved yet — this proves the
pipeline, not the OS.

#### M1 — Bootable "Hello Hyperion" Image

**Delivers:** the literal ask — a `.img` that boots from a real USB drive on real x86_64 UEFI
hardware into something visibly Hyperion (even if it's just a branded console banner and a shell).
- Replace the stock init with a placeholder `hyperion-init` (can be trivial at this stage — mount
  what's needed, print a banner, drop to a shell) to prove the custom-init path works end to end
  before M5 makes it real.
- Produce the final artifact as a raw `.img` `dd`-able to a USB drive (`dd if=hyperion.img
  of=/dev/sdX bs=4M status=progress` — document the exact command, and the safety warning about
  `of=` targeting the wrong device, prominently, since this is a destructive operation).
**Exit criteria:** the same image (a) boots in QEMU+OVMF to the banner/shell, and (b) boots on at
least one real x86_64 UEFI machine from a USB drive to the same banner/shell within a bounded time.
This is the milestone that actually answers "does Hyperion boot from a USB drive" — everything
after this is making what boots do something real.

#### M2 — Real Capability / Trust Boundary Enforcement

**Delivers:** `hyperion-capability`'s token/table/monitor *algorithm* (derivation, attenuation,
revocation graph — this logic is sound and should be reused, not rewritten) rehosted so that
minting, deriving, and revoking a token has a **real, kernel-enforced effect** on a real Linux
process, not just an in-memory struct check.
- A privileged root-owned "Capability Monitor" daemon is the only thing that spawns Trust-Boundary
  processes; at spawn time it applies the real enforcement for whatever `RightsMask`/depth was
  granted: `unshare()` into new namespaces (mount/pid/user/net as appropriate to the Trust Depth
  table in docs/03), install a seccomp-bpf filter scoping which syscalls are permitted, and apply a
  Landlock ruleset scoping filesystem access — all derived mechanically from the same
  `CapabilityToken`/`RightsMask` shape already defined in `hyperion-capability`.
- Revocation must have a **real** effect: killing/freezing the process (or removing its access at
  the enforcement layer) when `cap_revoke` fires on its token, not merely marking a struct stale.
**Exit criteria (mirrors docs/41 Phase 1 exactly, made real):** a capability token can be minted,
delegated, attenuated, and revoked end-to-end across two real, separate Linux processes — revoking
the token measurably removes that process's real ability to do something it could do a moment
before (a syscall it can no longer make, a file it can no longer open), verified by a test that
attempts the now-forbidden action and observes it fail.

#### M3 — Real IPC Transport

**Delivers:** `hyperion-ipc`'s frame/channel model (Request/Response/Notification, `ipc_call`/
`ipc_notify`) carried over a **real transport** — Unix domain sockets for the MVP (io_uring-based
batching is a real, valuable follow-on per docs/30, not required to prove the transport is real).
**Reuse:** the frame types and call/notify semantics as-is; only the transport underneath changes.
**Exit criteria:** two real, separate Linux processes (started under M2's enforcement) exchange a
real IPC call/notify frame across a real socket; a call from a process whose capability was
revoked is rejected at the transport boundary, not just by a library-level check the process could
route around if it weren't actually sandboxed.

#### M4 — Real Scheduler Enforcement

**Delivers:** `hyperion-scheduler`'s `ResourceVector`/`ResourceLedger`/DRF+EDF dispatch algorithm
mapped onto real Linux cgroups v2 controllers (`cpu`, `memory`, `io`, and the GPU controller where
the kernel/driver exposes one) and real scheduling policies (`SCHED_DEADLINE` or `SCHED_RR` for the
`RealTimeUI` class; `cpu.weight` approximating DRF fair-sharing for `InteractiveAgent`/
`BackgroundAgent`/`BatchDistributable`).
**Reuse:** the admission-control and fairness math as-is; it becomes the *policy* that decides what
cgroup weights/deadlines to write, not a replacement for the kernel's own real dispatch.
**Exit criteria:** the existing synthetic multi-class workload test (RealTimeUI + Interactive +
Background sharing CPU/RAM) is re-run against real cgroups on real Linux and its fairness/admission
assertions hold when measured from `/sys/fs/cgroup` accounting, not just from in-memory ledger
state.

#### M5 — Real Init & Supervision Tree

**Delivers:** a real `hyperion-init` (PID 1) that mounts the real root filesystem, starts the
Capability Monitor (M2), the IPC bus (M3), and the scheduler enforcement daemon (M4), then starts
every Phase 2-10 subsystem as a real, capability-scoped, supervised process — the "supervisor
tree, Erlang/OTP-style, microreboot" pattern docs/03 already specifies, implementable on Linux via
a small hand-rolled supervisor (model it after `runit`/`s6`'s simplicity, not `systemd`'s scope, to
keep the trusted-init surface auditable) rather than shelling out to a general-purpose init system
that would itself need to be trusted.
**Exit criteria:** killing any one supervised Phase 2-10 service process results in it being
restarted with a fresh capability grant within a bounded time, without a full reboot and without
crashing sibling services — the real version of the "microreboot" claim already tested in-process
in `hyperion-recovery`.

#### M6 — Real Persistent Storage

**Delivers:** `hyperion-storage`'s WAL-backed object store pointed at a **real, dedicated block
device or partition** (an attached NVMe/SSD for actual daily-driver persistence — writing heavily
to the boot USB itself is both slow and bad for the drive's lifespan) instead of a file on the
host's existing filesystem in a temp directory, as every current test does.
**Exit criteria:** the existing crash-consistency-by-replay guarantee is re-validated against a
real power-loss simulation (e.g. `qemu`'s ability to hard-kill the VM mid-write) on the real block
device, not just a simulated partial-write test against a host tempfile.

#### M7 — Real Console UI, Then Real Display

**Delivers, staged:**
1. **Text console first:** drive `hyperion-workspace`'s compiled UI/accessibility trees through a
   real TTY renderer, so a real human gets a real Intent → real Agent → real text-output loop from
   the booted image. This alone is the first milestone where booting Hyperion does something a
   person can actually use.
2. **Real display, later:** a minimal real compositor (a `wlroots`-based Wayland compositor, or a
   raw DRM/KMS framebuffer renderer for something even smaller) driving real pixels from a compiled
   `WorkspaceGraph`, plus real text layout/font rendering — this is where `hyperion-workspace`'s own
   "no pixels anywhere in this crate" scope note finally gets a real backend. Treat this as its own
   large sub-project; do not block M7's console stage on it.
**Exit criteria (stage 1):** a real utterance typed at the real booted console produces a real
Intent Graph, a real Agent invocation, and real text output rendered to the real TTY.

#### M8 — Real Local AI Runtime

**Delivers:** `hyperion-ai-runtime`'s mock model execution replaced with a real on-device inference
engine — **Candle** (Rust-native) is the natural fit for this codebase's own Rust-first convention
and avoids an FFI boundary to a C++ engine; `llama.cpp` bindings are the fallback if Candle's model
support gap is a blocker for a specific desired model. A real small resident model must run within
docs/36's latency budget on the reference hardware.
**Exit criteria:** `hyperion-intent`'s decomposition and `hyperion-model-router`'s routing produce
real output driven by a real model's real inference, on the booted image, not the deterministic
mock backend every current test uses.

#### M9 — Real Cryptography

**Delivers:** every "non-cryptographic checksum stand-in" this workspace uses as a deliberate,
documented placeholder — `hyperion-ai-runtime::checksum`, `hyperion-plugin-framework::signature`,
`hyperion-security`'s model-integrity check, `hyperion-update::signature`,
`hyperion-observability`'s hash-chain — replaced with real primitives (ed25519 or RSA signing via
a real Rust crypto crate; real SHA-256/BLAKE3 hashing) and a real key-management story (a software
keystore at minimum; TPM-backed sealing as a stretch goal where the reference hardware has a TPM).
**Exit criteria:** a tampered plugin manifest/update package/audit-ledger entry is rejected by a
real signature or hash-chain check, not a checksum a forger could trivially reproduce.

#### M10 — Real Networking

**Delivers:** `hyperion-netstack`'s `MockFetchBackend`/`MockExtractionBackend` replaced with a real
HTTP client (`reqwest`/`hyper`) over the booted machine's real NIC, real DNS, real TLS.
**Exit criteria:** `web.research`/`web.fetch.raw` fetch a real URL over the real network from the
booted image and merge a real extracted entity into the real Knowledge Graph.

#### M11 — Second Reference Platform

**Delivers:** bring-up on a second, lower-tier reference platform (Raspberry Pi 4/5, aarch64) to
satisfy docs/41 Phase 1's literal two-platform exit criterion — re-run M0-M4 for this target
(Buildroot supports aarch64; note that Raspberry Pi's boot path is SD-card/firmware-first, not
generic UEFI-USB, so M1's "boot from USB drive" claim is inherently x86_64-UEFI-specific and this
platform validates the *rest* of the stack, not an additional USB-boot claim).
**Exit criteria:** the same Hyperion image (kernel config and Buildroot target adjusted for
aarch64) boots to the M7-stage-1 console loop on real Raspberry Pi hardware.

#### M12 — Boot Benchmarking Against docs/36

**Delivers:** real cold-boot timing on real hardware (both reference platforms), measured
end-to-end (firmware → login/shell → first real Intent handled), against docs/36's full boot
budget — not `hyperion_sim::boot`'s in-process "privileged-core init" 250ms slice, which only ever
measured one sub-phase of a boot that didn't yet exist.
**Exit criteria:** real, measured cold-boot time is reported against the docs/36 budget on both
platforms; if over budget, the gap and its cause are named explicitly (kernel init time, initramfs
size, service startup ordering, model load time) rather than the milestone being closed on
optimism.

#### M13 — Release Engineering for a Bootable Artifact

**Delivers:** extend the existing `hyperion-release-gate` crate's criteria to cover the new
hardware/boot surface: image build reproducibility, boot-tested on both reference platforms per
M11/M12, a staged update (`hyperion-update`) applied to a real running booted system and rolled
back without data loss (docs/41 Phase 10's literal exit criterion, finally tested against a real
system instead of an in-process orchestrator), and a signed (M9), versioned, `dd`-able USB image
published as the actual release artifact.
**Exit criteria:** a fresh USB drive, written from a tagged release image, boots on both reference
platforms and passes a smoke test exercising M7 stage 1's real Intent→Agent→output loop.

### 5. What changes about "one commit per item"

The Rust-logic sprint that preceded this roadmap gated every commit on
`cargo build/test/fmt/clippy --workspace`. Systems/boot work doesn't decompose the same way — a
bootloader that gets 80% of the way to a kernel entry point isn't independently testable the way
half of a Rust function is. Gate milestones instead on:
- **M0-M1:** "does the image boot in QEMU" as the primary CI-able gate (a QEMU boot test that
  asserts a known banner string appears on the serial console within a timeout is realistic to
  automate); real-hardware boot is a manual verification step logged in this document's Status
  table, not something CI can assert.
- **M2-M6:** each gets its own real integration test asserting the *enforced* effect (a forbidden
  syscall actually fails, a revoked socket actually can't connect, a cgroup actually caps real CPU
  usage) — these can run in CI inside a container or VM with the right kernel features enabled.
- **M7-M10:** each is large enough to warrant its own sequence of sub-commits; keep the existing
  workspace's fmt/clippy/test discipline for any pure-Rust-logic portions of each (e.g. the real
  crypto primitives' own unit tests), layered under a milestone-level manual/CI boot-and-smoke-test
  gate for the integrated result.

### 6. Reuse map (what NOT to rewrite)

| Existing crate | What's reused as-is | What's rehosted/replaced |
|---|---|---|
| `hyperion-capability` | Token/derivation/revocation-graph algorithm | In-process struct checks → real seccomp/Landlock/namespace enforcement (M2) |
| `hyperion-ipc` | Frame types, call/notify semantics | In-process channel → real Unix sockets (M3) |
| `hyperion-scheduler` | DRF/EDF admission and fairness math | In-memory ledger → real cgroups v2 (M4) |
| `hyperion-storage` | WAL format, replay/recovery logic | Host tempfile → real block device (M6) |
| `hyperion-recovery` | Microreboot semantics, recovery points | In-process restart → real supervised-process restart (M5) |
| `hyperion-workspace` | Compiled UI/accessibility tree model | No renderer → real TTY (M7.1), then real compositor (M7.2) |
| `hyperion-ai-runtime`, `hyperion-model-router` | Routing/orchestration logic | Mock inference → real Candle/llama.cpp backend (M8) |
| `hyperion-*` checksum/signature stand-ins | Call-site shape (`sign`/`verify` interfaces) | Non-cryptographic checksum → real crypto (M9) |
| `hyperion-netstack` | Canonicalization, resolution, quarantine logic | Mock fetch/extraction → real HTTP client (M10) |
| `hyperion-release-gate` | Existing release-criteria structure | Extended with hardware/boot criteria (M13) |
| Every other crate (context, intent, memory, coordination, federation, device, explainability, observability, privacy, security, threat-model, plugin-framework, sdk, api-gateway, compat, scalability, update) | Runs unmodified as a real supervised service once M5's init/supervision exists | Nothing structural — these become real once their *environment* (M2-M6) is real |

### 7. Explicit Non-Goals for This Roadmap

Named here so no future session assumes silent scope creep:
- Formal verification of the enforcement layer (docs/03's seL4-class assurance target) — not
  attempted; the Linux-hosted enforcement in M2 is real but not formally proven.
- A from-scratch hybrid microkernel — explicitly deferred per §0's decision record, not abandoned.
- Real hardware virtualization/VM Trust-Depth-3 sandboxing for foreign-kernel guests
  ([27 — Compatibility Layer](27-compatibility-layer.md)'s Windows path) — out of scope until
  well after M13.
- GPU driver work beyond basic KMS/DRM framebuffer output — a real GPU compute/NPU driver story is
  a separate, later project, not part of M7's display milestone.
- Multi-device federation over a real network ([21 — Distributed Execution](21-distributed-execution.md))
  — M10 gets one device onto a real network; federating two *real, separately booted* Hyperion
  machines is a follow-on roadmap, not part of this one.

## Autonomy Roadmap

CLAUDE.md's "Autonomy" section states the commitment in full — resourceful, social,
self-sustaining. This document is the living record of what's actually real today versus what's
deliberately deferred, in this project's own "deliberately deferred, and why" convention (see
every crate's own doc comment for precedent). Nothing below is marked real until it's built,
tested, and gated (`cargo build/test/fmt/clippy`) — the same standard this document's own
Production Boot Roadmap section and [999 — Usage Scenarios](999-usage-scenarios.md) already hold
themselves to.

### Resourceful — use existing tools, create new ones

**Real today:**

- `hyperion-plugin-framework::PluginRegistry` already does real Ed25519-signed
  install/uninstall/query of capability implementations (this document's own Production Boot
  Roadmap section, M9).
- `hyperion-trust-boundary::spawn` already does real Linux sandboxing (user namespaces, Landlock,
  seccomp-bpf) of a forked process (M2).
- **Slice 1, landed.** Those two connect for real: `ImplementationDescriptor`/`CapabilityManifest`
  carry a real `NativeBinaryDescriptor` for `ImplementationKind::NativeBinary`, validated (program
  must really exist and really be executable) at install time;
  `PluginRegistry::invoke_native_binary` runs it inside a real `hyperion_trust_boundary::spawn`
  sandbox (real temp-dir I/O, a real bounded timeout, a real non-blocking `try_wait` poll loop —
  fixed live after an earlier version hung forever, since `is_alive()` alone can't distinguish "still
  running" from "exited but unreaped"); `AgentRuntime::invoke` dispatches an unrecognized
  `capability_ref` to it when a wired `PluginRegistry` has a matching installed implementation,
  instead of falling through to `stubs::dispatch`'s echo. Proven end to end: a real, statically
  linked (musl) tool, installed and invoked through the real console/agent-runtime path, produces
  its own real output.

**Also being built this pass** (pulled forward from an earlier, more conservative draft of this
roadmap, per direct instruction — see this file's own git history for what "deferred" used to
mean here):

- **Tool *creation*, landed (in the safe, honest sense this workspace can support today).**
  `hyperion-sdk::Implementation` carries a real `native_binary: Option<NativeBinaryDescriptor>`,
  threaded through `prepare_submission` → `publish` → `PluginRegistry::install`: naming an
  existing, real, already-vetted program as a `Runtime::NativeBinary` submission now installs it
  as a genuinely *runnable* capability, invocable through Slice 1's real execution path the moment
  `publish` returns — proven end to end.
- **`hyperion-api-gateway`'s parallel gap, landed.** Its own `ApiGateway::dispatch_one` now checks
  `self.registry` for a runnable `NativeBinary` implementation before falling back to
  `hyperion_agent_runtime::dispatch_stub_capability`, the exact same real execution path Slice 1
  built — proven end to end the same way, through `invoke_capability`.
- **Tool creation from scratch, landed (2026-07-16).** `hyperion-sdk::codegen::review_and_build`
  closes what the previous paragraph named as not built: an agent's freshly generated Rust source
  is rejected outright if it contains `unsafe`, then really compiled (`cargo build --release`) and
  really linted (`cargo clippy -- -D warnings`) in a throwaway scratch package — three real gates,
  no simulated pass/fail. Only source that survives all three becomes a real, runnable
  `NativeBinaryDescriptor`, installable through the exact same `publish` → `PluginRegistry::install`
  path (and therefore the exact same sandboxed execution path) as a hand-installed `NativeBinary`.
  Proven end to end: a clean generated program really compiles, really lints, and its real compiled
  binary really runs and produces real output; a source containing `unsafe`, a source that fails to
  compile, and a source that compiles but fails clippy are each proven to really be rejected before
  ever executing. Real code review of generated code beyond compiler + clippy (e.g. LLM-based
  semantic review, sandboxed dry-run fuzzing) remains future work — this gate is real and honest
  about what it does and doesn't catch, not a claim of perfect review.
- **`hyperion-coordination`'s object-affinity plan partitioning, landed (2026-07-16)** (docs/12
  §12's own named scale optimization for tens of concurrent Agents: "the plan is therefore
  partitioned by object-affinity so unrelated branches... rarely contend on the same version
  counter"). `SharedPlan.partition_versions` replaces the single, plan-wide `version` counter every
  task-status change bumped regardless of which task changed — confirmed dead first (nothing
  anywhere in this workspace ever read it) rather than merely shadowed — with a real, distinct
  counter per connected group of tasks (`task_partition_key`, a pure BFS over real
  `TaskNode::dependencies` edges), bumped only by that specific group's own real status changes and
  readable via `CoordinationSession::partition_version`. Proven end to end: two genuinely unrelated
  synthetic branches (this workspace's one built-in HTN template is a single connected chain, so a
  live test alone can't exercise two) get different real partitions; a live `allocate` pass really
  bumps only the completed task's own partition, provably shared correctly by every task in the
  same real dependency chain.
- **`hyperion-update`'s anti-rollback monotonic counter, landed (2026-07-16)** (docs/32 §Security
  Considerations, M9's own named remaining gap: "a signed monotonic version counter prevents an
  attacker from reinstalling a deliberately-downgraded, vulnerable prior image... downgrade is only
  permitted through the explicit, audited `update_rollback` path, never through re-flashing an old
  signed image directly"). `SystemImageController::highest_version_ever` is a real, monotonic
  high-water-mark, distinct from either A/B slot's own `version` field (which *can* legitimately
  move backward — that's what a rollback is). The normal forward path,
  `stage_to_inactive_slot`, now really refuses (`UpdateError::AntiRollbackViolation`) to stage
  anything at or below it; only the new, separate `stage_rollback_to_inactive_slot` — the
  "explicit, audited" counterpart — may stage an older version, and doing so never lowers the
  high-water-mark, so replaying that same old, vulnerable, still-validly-signed image through the
  normal path immediately afterward is still refused. Proven end to end: staging at or below the
  high-water-mark is rejected; a legitimate rollback succeeds without lowering it; a same-version
  replay attempt right after that rollback is still rejected. Honest scope boundary: a real
  counter enforced in software, not yet a real cryptographically tamper-evident one persisted to a
  real state store — this crate still has no keystore/state-store concept for any of its data
  (every field is in-process `Mutex` state, gone on restart), a separate, larger gap this pass
  doesn't attempt to close.
- **A workspace-wide, shared Explanation Record store, landed for a caller that wants it
  (2026-07-16)** (independently named in `hyperion-coordination`'s, `hyperion-federation`'s, and
  `hyperion-api-gateway`'s own doc comments as "deliberately not shared... a follow-up for
  whichever future slice needs one workspace-wide trace rather than several independent ones").
  `CoordinationSession::new_with_shared_explanations`/`FederationHub::new_with_shared_explanations`
  now take a real, caller-supplied `Arc<hyperion_explainability::ExplanationStore>` instead of
  building a private one — the same store `hyperion-api-gateway::ApiGateway` already took (it
  needed no change). The one real correctness fix this required:
  `ExplanationStore::next_action_id` now mints every real `action_id` from the store's own single
  counter — each of the three owners previously minted `action_id`s from its own private counter
  (all starting at 1), so sharing one store without also sharing this would let two independent
  owners' `action_id`s collide, and `get_by_action`/`resolve_why`'s first-match lookup would
  silently resolve to the wrong owner's record. Every existing constructor (`CoordinationSession::new`,
  `FederationHub::new`/`new_with_keystore`) is unchanged — still builds its own private store, every
  existing call site across the workspace keeps compiling unmodified. Proven end to end,
  cross-crate: a real `CoordinationSession` and a real, genuinely independent `FederationHub`,
  sharing one store, each contribute a real record under the same real Intent id with no
  `action_id` collision, both findable through the one shared store's own `trace_intent`.

**Deliberately still deferred:**

- **`Contribution::Agent`, landed (2026-07-16).** `hyperion-plugin-framework::PluginRegistry`
  gained a real, live registration point:
  `agent_contributions()` returns every currently-installed, non-quarantined plugin's own
  `Contribution::Agent` entries. `hyperion-coordination::catalog::best_fit_manifest_with_plugins`
  merges those with the built-in roster (`default_manifests`), and
  `CoordinationSession::allocate` calls it through a new `AgentRuntime::plugin_registry()`
  accessor — so a plugin-contributed specialization really competes for task allocation, not
  just the hardcoded static list this gap previously named. Always installs at `TrustTier::Community`
  (the least-trusted tier — no publisher-key trust store exists yet to justify anything higher);
  can only ever justify `Read`/`Execute` permissions on its own, never `Write`/`NetworkEgress`.
  Proven end to end: installed, uninstalled, and quarantined agent contributions really
  appear/disappear from `agent_contributions()`, and a plugin-contributed manifest is really
  selected and really spawns a real `AgentInstance` when no built-in specialization fits.
- **`Contribution::HardwareSupport`, landed (2026-07-16).**
  `PluginRegistry::hardware_support_contributions()` is the real "device driver registry"
  `hyperion-device` had no equivalent of. `hyperion-device::known_capability_manifest` searches
  every currently-installed, non-quarantined plugin's own driver profiles for an exact
  `(device_type, manufacturer, model)` match and converts it into a real `CapabilityManifestEntry`
  list — so a real pairing flow can *propose* an expected manifest instead of an integrator
  hand-authoring one with nothing to consult. Never weakens
  `DeviceRegistry::register`'s own real signature check (docs/20 §8's device-impersonation
  defense) at all: the device (or its driver) still has to really sign whatever manifest
  registration ultimately uses. A bare `HardwareSupport` contribution can only ever justify
  `Read`, never `Write`/`NetworkEgress`/`Execute`. Proven end to end: a known
  `(device_type, manufacturer, model)` is found and correctly converted; an unknown or
  mismatched one isn't; uninstall/quarantine really remove it from the lookup.
- **`Contribution::KnowledgeProvider`, landed (2026-07-16).**
  `PluginRegistry::knowledge_provider_contributions()` is the real (topic -> capability_id)
  lookup `hyperion-knowledge-graph` had no equivalent of.
  `hyperion-knowledge-graph::capability_for_topic`/`capabilities_for_topic` search every
  currently-installed, non-quarantined plugin's own declared topics for a match — a real caller
  with no local knowledge of a topic uses the result to decide which installed Capability to
  invoke, never a second, parallel dispatch path (the matched capability still goes through the
  exact same invocation/consent machinery every other Capability already does). A bare
  `KnowledgeProvider` contribution can only ever justify `Read`. Proven end to end: exact-topic
  lookup, multiple providers for one topic, and uninstall/quarantine really removing it.
- **`Contribution::UiComponent`, landed (2026-07-16).**
  `PluginRegistry::ui_component_contributions()` is the real UI-component registry
  `hyperion-workspace` had no equivalent of — before this, every caller hand-authored a
  `CapabilityUiContract` (see `contract_for` in `hyperion-console`/`hyperion-shell`, or the
  `*::workspace_bridge` modules). `hyperion-workspace::known_contract_for` searches every
  currently-installed, non-quarantined plugin's own templates for an exact `capability_ref`
  match and converts it into a real `CapabilityUiContract` (per-`ComplexityTier` `variants` are
  not part of a plugin contribution yet — a real but separate Adaptive-Complexity refinement).
  A bare `UiComponent` contribution can only ever justify `Read`. Proven end to end: a known
  `capability_ref` is found and correctly converted; an unknown one isn't; uninstall/quarantine
  really remove it from the lookup.
- **`Contribution::AutomationWorkflow`, landed (2026-07-16).**
  `PluginRegistry::automation_workflow_contributions()` is the real, live goal-template registry
  `hyperion-intent`'s own hardcoded, crate-private built-in roster had no equivalent of.
  `hyperion_intent::templates::match_template_with_plugins` checks the built-ins first (the same
  "existing roster wins ties" convention `hyperion-coordination::catalog::
  best_fit_manifest_with_plugins` already established for `Contribution::Agent`), then a
  currently-installed, non-quarantined plugin's own workflow templates — reached through a new
  `IntentEngine::new_with_plugins` constructor (`new` itself is unchanged, so every existing
  caller is untouched). A bare `AutomationWorkflow` contribution can only ever justify `Read`.
  Proven end to end: a plugin-contributed template really decomposes a matching utterance into
  real, dependency-linked sub-intents; a built-in template still wins a colliding match; without
  a registry the same utterance stays ungrounded; quarantine stops a plugin's template from
  matching. Not yet wired into a live `hyperion-console`/`hyperion-shell` session (no
  `PluginRegistry` is constructed there today) — a real, separate next step, same scope boundary
  the `Agent` slice already drew.
- **`Contribution::MemoryProvider`, landed (2026-07-16).**
  `PluginRegistry::memory_provider_contributions()` is the real `(tier, entity_key) ->
  capability_id` registry docs/24's own "memory providers register storage backends into [08 —
  Memory Engine]" gap named as missing. `hyperion_memory::{capability_for, capabilities_for}`
  mirror `hyperion-knowledge-graph`'s own `KnowledgeProvider` lookup exactly — never bypassing
  the Capability Registry's own dispatch/consent path, never writing a `MemoryRecord` itself. A
  bare `MemoryProvider` contribution can only ever justify `Read`. Proven end to end: exact
  `(tier, entity_key)` lookup, multiple providers for the same pair, and uninstall/quarantine
  really removing it.
- **`Contribution::ExecutionEngine`, landed (2026-07-16) — the last of the eight variants.**
  `PluginRegistry::execution_engine()` is the real "runtimes usable by Capability
  implementations" registration point docs/24 describes: a plugin's launcher is validated the
  exact same honest way a `Capability`'s own `NativeBinaryDescriptor` already is (must really
  exist, must really be executable), then stored and served like every other contribution here.
  `hyperion_sdk::resolve_via_engine` is the real consumer: it turns a caller's own script path
  into a concrete `NativeBinaryDescriptor` by prepending the engine's launcher, so a capability
  published "via" an engine installs and runs through the exact same
  `ImplementationKind::NativeBinary` path a hand-written native binary already uses — no second,
  parallel execution mechanism. A bare `ExecutionEngine` contribution can only ever justify
  `Read`/`Execute`. Proven end to end: a real musl companion binary really receives the real
  script path threaded all the way through resolution → publish → sandboxed invocation, and
  really echoes it back in its real output; an unknown engine ID fails honestly at resolution,
  before any capability is ever published against it.

  Every `Contribution` variant docs/24 names now has a real owning subsystem (`Model` remains
  deliberately not a distinct variant — see `hyperion-plugin-framework`'s own doc comment on why
  it was never a real ninth gap).

### Social — connect with other Hyperion instances

**Real today:**

- **`/mcp-server [port]`** — a real MCP (Model Context Protocol) server, started as a real
  background thread from an ordinary console meta-command, over real HTTP (JSON-RPC 2.0:
  `initialize`, `tools/list`, `tools/call`) rather than stdio — stdio stays free for the rest of
  the session, unlike a `--mcp`-flag design that would need to own it exclusively. Exposes
  `hyperion.ask`/`hyperion.recall`/`hyperion.graph` as real tools, each a real turn through the
  exact same `ConsoleSession::handle_utterance` path everything else in this crate uses — no new
  bypass of the capability/consent model.
- **`/a2a-server [port]`** — a real A2A (Agent2Agent) server, same shape: a real Agent Card at the
  real spec-defined `/.well-known/agent-card.json`, and the real `SendMessage` JSON-RPC method
  (the spec's own minimal "send a message, get a reply" flow), backed by the same live session.
- **`/mcp-call <host> <port> <tool> <json args>`** and **`/a2a-call <host> <port> <message
  text>`** — the real outbound half: Hyperion calling *out* to a real, already-known MCP/A2A
  endpoint, including another Hyperion instance's own server. Verified live: two real
  `hyperion-console` processes, one running `/a2a-server`, the other running `/a2a-call` against
  it, genuinely exchanging a real reply pulled from the *first* process's own conversation history.
  Not discovery — the endpoint is named, not found (this pillar's own next gap when this was
  written; now landed, next bullet).
- **mDNS/DNS-SD advertise + discover, landed (2026-07-16).** `/mcp-server`/`/a2a-server` really
  publish a real `_hyperion-mcp._tcp.local.`/`_hyperion-a2a._tcp.local.` mDNS service record (via
  the pure-Rust `mdns-sd` crate) on the real port they bound; `/mcp-discover [seconds]`/
  `/a2a-discover [seconds]` really browse the real LAN for the same service types and list every
  real peer resolved. Feature-gated (`mdns`, off by default, matching `real-http`/`candle`'s own
  convention) — a binary not built with it gets an honest `DiscoveryError::NotCompiledIn` from
  both commands instead of a silently faked result. Proven end to end (including on this sandbox's
  own real network stack): a running `/mcp-server` really resolves its own real advertised
  address(es) via `/mcp-discover`, including across multiple real interfaces/address families on
  a multi-homed host; scanning with nothing advertising finds nothing; the no-feature build
  degrades honestly for both directions. Discovery only, not identity/trust on its own (see the
  identity slice below, landed the same pass) — and not yet consumed by `hyperion-device`'s own
  separate "real discovery protocols
  (mDNS/BLE/Matter/cloud-relay)" gap for *device pairing*, which is a distinct concern from this
  console-level *peer* discovery and remains its own, separate future step.
- **`/standby`** — blocks on a real read of this process's own stdin until the user provides real
  input, then exits. Exists specifically so a scenario that starts a background server doesn't
  have the whole process (server included) exit the instant the scenario file ends — the real
  mechanism for "keep this alive long enough to test the server from another terminal, on my own
  schedule."
- Both servers share one real `Arc<Mutex<ConsoleSession>>` with the console's own interactive/
  scenario-file loop — a real MCP/A2A tool call and a real typed utterance affect (and can observe)
  the very same conversation, not two divergent copies.
- **Real A2A peer identity via trust-on-first-use, landed (2026-07-16).** The *identity* half of
  "real cross-instance discovery, identity, and trust" (discovery itself landed earlier this same
  pass, above). `ConsoleSession` now retains its own real, persistent Ed25519 device identity
  (previously created transiently in `open` and dropped); `/a2a-server`'s Agent Card carries its
  real, hex-encoded public key, and every `SendMessage` reply is really signed with it. `/a2a-call`
  really verifies that signature against the claimed key (proof the responder genuinely holds the
  matching private key) and checks the key against a real, persisted
  `hyperion_console::peer_trust::PeerTrustStore` keyed by `host:port` — the same well-established
  model SSH's own `known_hosts` uses: first contact records the key; a later contact with a
  *different* key is a hard, surfaced failure (the reply is never shown), not silently trusted or
  silently overwritten. New `/trust list`/`/trust forget <peer>` commands make the trust store
  real inspectable/reversible state, not an invisible one-way ratchet. A real, non-Hyperion A2A
  server with no `publicKey` claim is neither penalized nor silently trusted — this check simply
  doesn't apply to it, the same "unknown host" fallback SSH itself uses. Proven end to end,
  including a real impersonation scenario: two real, differently-keyed `hyperion-console`
  processes answering the same `host:port` in sequence — the second one's real reply is refused
  with a real warning, and only shown after an explicit `/trust forget`. This is identity
  *continuity*, not *authorization* — nothing here decides whether a peer *should* be talked to,
  only whether it's still the same one as last time.
- **MCP identity parity, landed the same pass (2026-07-16).** The exact same trust-on-first-use
  check, for `/mcp-call` instead of duplicated as a new one: `initialize`'s response now carries a
  real public key and every `tools/call` reply is really signed, verified, and checked against
  the same shared `PeerTrustStore` `/a2a-call` uses (a peer is keyed by `host:port` regardless of
  which protocol reached it). [`crate::mcp::call_tool`] now performs a real `initialize` round
  trip first — purely to fetch the key, this also happens to close this module's own previously-
  named "no real client handshake" gap for free. Proven end to end the identical way: a real
  impersonation swap is refused, not silently shown.
- **MCP `resources/list`/`resources/read`, landed the same pass (2026-07-16).** Two real,
  read-only resources (`hyperion://graph`, `hyperion://recall`) reached through the exact same
  `ConsoleSession::handle_utterance` path `tools/call` already uses — no second, resource-
  specific bypass. `initialize`'s advertised `capabilities` now includes `"resources": {}`. New
  `/mcp-resource <host> <port> <uri>` client command performs the identical real `initialize` +
  identity-check handshake `/mcp-call` uses (both now share `fetch_claimed_key`/
  `verify_and_finalize` rather than duplicating the check a second time). Proven end to end: a
  real client reads `hyperion://graph`'s real live content over a real connection, an unknown URI
  is a real honest JSON-RPC error, and the identity check really fires the same way it does for
  `tools/call`.
- **A2A `GetTask`/`ListTasks`, landed the same pass (2026-07-16).** A real, in-process,
  insertion-ordered `TaskStore` keeps every completed `SendMessage` `Task`; `GetTask <id>` really
  re-fetches one (a real caller that lost its own copy, or wants to check on it later, doesn't
  have to have kept it), `ListTasks` really lists every one completed so far, in order. Streaming/
  push notifications remain out of scope — there's still nothing to *stream*, since every real
  dispatch still completes synchronously before `SendMessage` returns; a task store is a real
  history, not a queue of work in flight. Proven end to end: a completed task's id really
  round-trips through `GetTask`; an unknown id is a real, honest JSON-RPC error; `ListTasks`
  really returns every completed task in the order they finished.
- **MCP real stdio transport, landed the same pass (2026-07-16).** `--mcp-stdio` takes over the
  whole process as a real MCP server speaking newline-delimited JSON-RPC over its own real
  stdin/stdout — the transport most real MCP clients (e.g. Claude Desktop) actually launch a
  server with, not just the HTTP one this crate already had. Reuses the exact same
  `handle_request` dispatch `/mcp-server`'s own HTTP transport uses (made `pub(crate)` rather
  than duplicated) — one real protocol implementation, two real transports. Proven end to end: a
  real child process spawned with `--mcp-stdio`, driven with real `initialize`/`tools/call`/
  `resources/list` requests over real pipes, replies correctly on each and exits cleanly on real
  stdin EOF.
- **`SyncEnvelope`-wrapped encrypted federation payloads, landed (2026-07-16).**
  `hyperion-federation`'s own previously-named deferred gap. Every `FederationHub` now holds a
  real `hyperion_crypto::Keystore` (a fresh ephemeral identity by default via `new()`, or a real
  persisted one via `new_with_keystore`); `FederationHub::seal`/`open` really encrypt
  (ChaCha20-Poly1305) and really sign (Ed25519) a payload through it — the same real
  `derive_key`/AEAD pattern `hyperion-crypto::secret_store::SecretStore` already established, plus
  a new, reusable `hyperion_crypto::sync_envelope` module.
- **Real per-device X25519 key-exchange, landed (2026-07-16).** The scope boundary the bullet
  above originally named — sealer and opener sharing the *same* `Keystore` — is closed for real.
  `hyperion_crypto::key_exchange` adds real X25519 Diffie-Hellman: `Keystore::x25519_public`/
  `diffie_hellman` let two genuinely independent, separately-keyed devices each derive the
  identical real shared secret (proven directly: `diffie_hellman(a, b.x25519_public()) ==
  diffie_hellman(b, a.x25519_public())`), reusing the same per-device Ed25519 identity via
  `derive_key` rather than a second, separately-persisted keypair. `sync_envelope::seal_for_peer`/
  `open_from_peer` (and `FederationHub::seal_for_peer`/`open_from_peer`/`x25519_public`/
  `establish_shared_secret`) use that shared secret in place of one shared `Keystore` — the sender
  signs with its own identity, the opener verifies against the *sender's* known public key, not its
  own. Proven end to end: two independently-keyed hubs seal/open for each other; a third hub's own
  shared secret (established with a *different* peer) cannot open an envelope meant for someone
  else.
- **The rest of each real spec.** MCP: prompts, notifications, the SSE-streaming half of
  "Streamable HTTP." A2A: streaming/push notifications themselves (see above for why a task
  store alone doesn't need them yet).
- **A2A, gossip, or any custom/invented protocol.** Worth exactly when a real, concrete need
  outgrows what MCP already covers — not before.
- **Real lease-renewal heartbeat timing, landed (2026-07-16).** `FederationHub::start_lease_heartbeat`
  spawns a real background thread that renews an `AnchorLease` on a fixed real wall-clock interval
  (`SystemTime::now`, unlike every other method on this hub, which takes a caller-supplied logical
  `now`) — ambient, automatic upkeep rather than a caller explicitly calling `renew_lease` itself.
  Returns a `LeaseHeartbeat` handle: dropping it (or calling `.stop()`) signals the real thread to
  stop and joins it, so a caller can be sure renewal has genuinely halted before, e.g., releasing
  the lease. Proven end to end: a real, running heartbeat keeps a one-second-ttl lease looking
  fresh across 1.2+ real seconds; the identical lease with no heartbeat genuinely goes stale in the
  same window; a stopped heartbeat provably stops renewing (the lease goes stale again afterward).
- **Real network transport for federation, landed (2026-07-16).**
  `hyperion_federation::serve_ledger_publications` runs a real background thread accepting real
  `TcpListener` connections; `publish_ledger_over_socket` is the real client half. A
  `LedgerPublication` (a device's resource headroom) genuinely travels — `seal_for_peer`-encrypted
  and Ed25519-signed the whole way, via `hyperion-crypto`'s new `SyncEnvelope::to_wire_bytes`/
  `from_wire_bytes` — over a real `TcpStream` between two independent `FederationHub` instances,
  and is only applied via the receiving hub's own already-real `FederationHub::publish_ledger`
  once authentication and decryption both genuinely succeed (a malformed frame or the wrong
  signing identity is silently dropped, never applied). The receiver stamps `published_at` with
  its own real wall clock, never a value the remote sender could lie about. Proven end to end: a
  ledger published on one hub really arrives on a genuinely separate hub over a real socket; a
  publication signed by the wrong identity is never applied. **Ambient anti-entropy remains
  deferred** — it needs a real multi-device Knowledge Graph replica model
  ([28 — Storage Engine](../28-storage-engine.md)) to converge, which doesn't exist yet; this
  closes only the transport, not continuous background re-publication.
- **Many-instance mesh delegation + live dashboard, landed (2026-07-16).** Past scenario 12's
  two hardcoded-host/port processes: any number of `hyperion-console` instances now each advertise
  a real, configurable `HYPERION_CONSOLE_CAPABILITIES` (`agent_card`'s `skills` array is built from
  it, no longer one hardcoded entry) and a new `/mesh-request <own_port> <capability> <text>`
  command really discovers (via the existing mDNS `discover`, bounded-retried 5×2s for real
  multi-process convergence) whichever peer's own Agent Card actually lists a capability this node
  lacks, then delegates to it via the existing identity-checked `a2a::send_message` — no host/port
  named by a human anywhere in that path. A new `crate::mesh` module holds the shared, in-process
  `MeshEventLog` both the requesting side (`/mesh-request`) and the receiving side
  (`a2a::handle_request`'s `SendMessage` arm, honestly `"unknown"`-attributed since that method
  never authenticates its caller) record into, served at a new `GET /mesh/status`. A new
  `/mesh-dashboard [port]` role — not itself a delegation participant — polls every discovered
  peer's Agent Card + `/mesh/status` once a second and serves a real, self-contained live page
  (`mesh_dashboard.html`, this crate's first `include_str!` asset) rendering the whole mesh as an
  animated graph: dashed edges for real, persisted trust, a brief pulse for a delegation that just
  happened, plus a raw scrolling event log. `scripts/run-mesh-demo.sh` launches six such nodes
  with distinct/overlapping capabilities plus one dashboard end to end. See
  [999 — Usage Scenarios](999-usage-scenarios.md) scenario 15 for the full transcript, two real
  bugs this scenario's own manual verification caught and fixed (a dashboard-startup ordering
  block, and duplicate "ghost" nodes from one peer's several resolved network interfaces), and an
  honestly-named open finding about real mDNS convergence speed over some virtualized networks.

### Self-Sustaining — degrade safely, recover, come out stronger

**Real today:**

- `hyperion-agent-runtime`'s circuit breaker already suspends an instance after
  `CIRCUIT_BREAKER_THRESHOLD` consecutive failures (M8-era).
- `hyperion-recovery`'s undo/redo/crash-recovery journal already exists (real, but purely
  reactive — restores last-known-good, no learning).
- `hyperion-supervisor`'s exponential backoff + give-up policy already exists for OS *processes*.
- **Slice 3, landed.** A `Suspended` `AgentInstance` now has a real way back:
  `AgentRuntime::prepare_invoke` auto-resumes it once a real, adaptive backoff window
  (`backoff_duration`, capped exponential, re-derived from `hyperion-supervisor`'s own proven
  shape for OS processes) has elapsed, with a real, explainable audit entry
  (`auto_resumed_after_backoff`) — instead of staying stuck until something external intervenes.
  `QuotaState.times_suspended` (the instance's real *whole-life* suspension count, distinct from
  `consecutive_failures`, which already resets on any success) makes a repeat offender's next
  backoff measurably longer — verified live: a second suspension's backoff genuinely outlasts the
  first. A real streak of `SUCCESS_STREAK_TO_DECAY` (3) consecutive successes after a resume
  decays `times_suspended` back down by one, each decay its own audited event
  (`backoff_decayed`) — the actual "recovers, and comes out stronger" mechanic, not a fixed
  penalty forever or a reset to baseline on the first success.
- **Slice 3b, landed.** That same suspend/auto-resume/backoff-decay history now survives a real
  process restart. `AgentRuntime` gains an `Option<Arc<MemoryEngine>>` (same optional-real-backend
  shape as `Option<Arc<NetstackHub>>`/`Option<Arc<PluginRegistry>>`): `spawn` seeds a fresh
  instance's `times_suspended` by querying `hyperion-memory`'s Procedural tier for that
  specialization's own remembered history, and every suspend/auto-resume/decay
  (`record_resilience_event`) writes back into it — so the "this instance is a repeat offender"
  signal outlives the process, not just the in-memory `AgentInstance`. Proven end to end: opening
  the exact same on-disk Knowledge Graph path twice, with a fresh `AgentRuntime`/`MemoryEngine`
  each time and nothing else shared (the closest a single test process can get to a genuine
  restart) — a specialization's suspension count is really there the second time, and a
  *different* specialization's fresh instance genuinely starts at zero (no cross-contamination).

- **`hyperion-recovery` learning from what it rolls back, landed (2026-07-16).** No longer
  purely reactive. `RecoveryService::restore_to_with_cause` really remembers a real
  `RollbackCause` (a short reason plus whatever structured data justified it) in an optional,
  real wired `MemoryEngine`'s Procedural tier — the same `Option<Arc<...>>` shape this pillar's
  own agent-runtime slice above already established, so every existing caller of the unchanged
  `restore_to`/`new` keeps working exactly as before. `RecoveryService::rollback_causes` really
  queries that history back. `hyperion-update::UpdateOrchestrator` is the real caller: its own
  health-breach rollback path used to compute a real `CohortHealth` breach and immediately
  discard it — it's now threaded into the cause a real rollback is recorded with. A rollback's
  cause now really shapes a future decision, not just a future log line:
  `UpdateOrchestrator::apply_update` checks history before ever starting a rollout again, and
  refuses outright (`UpdateError::RepeatedRecentRollback`, before `health_for_stage` is even
  called) to retry the exact same `(subject, from_version, to_version)` that already rolled back
  once — while a genuinely different update for the same subject is untouched by that history.
  Proven end to end with both cases.

- **A model-router-style "demote, never remove" signal for agent instances, landed (2026-07-16).**
  `hyperion-coordination::CoordinationEngine::allocate`'s own existing-participant selection used
  to rank candidates by current load alone. It now generalizes `hyperion-model-router`'s own
  circuit breaker precedent (previously scoped to model selection only) to agent instances: an
  eligible instance whose real, whole-life `QuotaState.times_suspended` (already tracked by the
  Slice 3/3b work above) has reached `REPEAT_OFFENDER_THRESHOLD` (3, the same "3 strikes"
  convention `hyperion-model-router`'s own `CIRCUIT_BREAKER_THRESHOLD` and this pillar's own
  `SUCCESS_STREAK_TO_DECAY` already use) is demoted to the bottom of the ranking, never excluded
  outright — a load-balancing tie among two otherwise-equal repeat offenders is still broken by
  load, just underneath every clean instance. Needed zero new state: purely a selection-ordering
  change reading data Slice 3/3b already collects. The ranking decision itself
  (`ranking_key`) is a small, pure function, directly and thoroughly unit-tested (a full live
  race between two same-specialization participants needs a real, already-suspended instance in
  the very same plan — real but expensive setup a focused unit test on the actual decision
  doesn't need); naturally racing that scenario end-to-end remains a real, separate addition, not
  attempted here.

- **`UndoScope::Session`/`UndoScope::Goal`, landed (2026-07-16)** (docs/33 §4's own item,
  `hyperion-recovery`'s previously-named gap: "neither concept has a first-class id anywhere in
  this workspace" — false the moment `hyperion_coordination::types::SharedPlan.session_id`/
  `root_intent` existed). `RecoveryService::record_action_started_with_scope` is the real, tagged
  counterpart to `record_action_started` (which still tags neither, for every caller with no
  session/goal concept); `CoordinationSession::with_recovery` is the real, optional
  (`Option<Arc<...>>`) caller — every real task dispatch `allocate()` completes now opens a real,
  best-effort recovery point + `ActionRecord` around the real `"task_result"` node it creates,
  tagged with that session's own real `session_id`/`root_intent`. Every existing constructor and
  call site is unchanged (`with_recovery` is opt-in, chained after construction). Honest scope
  boundary: `"task_result"` is always a *fresh* KG node, so this specific action's own undo can't
  restore it (this crate's own pre-existing "un-creating a freshly created object" limitation) —
  the real value landed here is genuine crash-recovery journaling and session/goal-scoped
  bookkeeping. Proven end to end in both crates: `hyperion-recovery`'s own
  `UndoScope::Session`/`UndoScope::Goal` correctly scope to only the tagged actions (an untagged
  action never matches either); `hyperion-coordination`'s real dispatch tags a real `ActionRecord`
  with its own real session/goal ids, recorded `Committed`, not left dangling `InFlight`.

- **`hyperion-observability`'s retention/rollup compaction, landed (2026-07-16)** (docs/34 §5's
  own previously-named gap: "raw metrics are kept at full resolution for a short window (default
  24h) then compacted to percentile rollups... logs age out per level-based TTL" — samples and log
  events previously accumulated for the process lifetime). `TelemetryCollector::compact_metrics`
  really removes every raw `MetricSample` older than a caller-supplied retention window from raw
  storage and folds it into one real `MetricRollup` per metric name — real min/max/count and real
  p50/p95/p99 via the nearest-rank method, computed from the actual aged-out values, never
  fabricated; a name with nothing newly aged out produces no empty rollup.
  `TelemetryCollector::expire_logs` really drops any `LogEvent` whose own real level has passed its
  TTL in a real, distinct-per-level `LogRetentionPolicy` (a real, deliberately-chosen default:
  noisier levels expire sooner). Both are caller-driven passes, matching this crate's own existing
  "on-demand, not backgrounded" convention for `AuditLedger::verify_chain`. Proven end to end: a
  fresh sample stays raw; an aged-out one is removed from raw storage and its rollup's percentiles
  are independently verified against a known ten-sample set; two aged-out batches for the same
  metric produce two separate rollup windows; an expired log is really gone while a fresh one
  survives the same pass.

- **`hyperion-observability`'s globally-unique cross-device span identity, landed (2026-07-16)**
  (this crate's own previously-named gap: `TelemetryCollector::merge_remote_trace` was real, but a
  span id was only unique *within* the collector that minted it, so a merged cross-device trace
  could contain two spans sharing a `span_id`). `SpanId` is now a real struct pairing the minting
  collector's own `device_id` with a per-collector monotonic sequence number, and
  `TelemetryCollector::new_with_device_id` is the real constructor a caller with a real device
  identity uses instead of `TelemetryCollector::new` (which stays `device_id: 0`, unchanged for
  every existing caller). `hyperion-federation`'s `FederationHub::join_device` — the one real
  production call site `merge_remote_trace` is actually invoked from (via `FederationHub::migrate`)
  — now does exactly that. Proven end to end: two collectors built with distinct real `device_id`s
  never mint a colliding `SpanId`, even after merging their spans into one trace.

- **`hyperion-knowledge-graph`'s inferred-edge decay, landed (2026-07-16)** (docs/09 §5.2's own
  previously-named gap: "neither kind of inferred edge decays yet (weight is reset to a fixed
  value each pass, not accumulated or aged)"). `EdgeRecord` gains a real `last_confirmed_at`
  field, distinct from `created_at` (which stays fixed at an edge's original creation) — every
  real `KnowledgeGraph::link` call, fresh or reconfirming, advances it, exactly the "continued
  co-occurrence or continued similarity" event docs/09 §5.2 names as what keeps an inferred edge
  from decaying. `effective_edge_weight` is the real, on-demand decayed weight this drives: an
  `EdgeOrigin::Inferred` edge's weight shrinks with real elapsed time since its last real
  confirmation using the same recency-weighted mechanism docs/09 §5.2 points at
  (`hyperion-memory::decay::decay_score`'s own 30-day tau for its Semantic/Procedural tiers,
  reused rather than an invented constant); an `EdgeOrigin::Explicit` edge never decays at all —
  "a hypothesis is allowed to fade," an explicit fact is not. A pure, recompute-from-scratch
  function mirroring `decay_score`'s own shape, not a batch job overwriting `weight` in place.
  Proven end to end: a freshly confirmed edge is at full strength; an edge unconfirmed for twice
  its tau has decayed substantially; an explicit edge is untouched even ten tau-periods out; a
  real reconfirmation (a second real `link` call) restores full strength.

## Backlog

Product-level work that's real, named, and intentionally not yet scheduled — distinct from this
document's own Autonomy Roadmap section above (which tracks code that's actually landed vs.
deliberately deferred for the Resourceful/Social/Self-Sustaining pillars) and from
[41 — Implementation Phases](41-implementation-phases.md) (which
tracks the build-out of subsystems already designed in `docs/`). This file is for a different
kind of gap: things the *vision* is missing, discovered by holding Hyperion up against an outside
argument and checking whether the architecture actually answers it — not just whether a crate
compiles.

No target release date is committed by putting something here. "v0.2.0+" means "after the current
build-out, revisit."

### v0.2.0+ — Protect the Human (cognitive load, judgment, meaning)

Source: a working session (2026-07-14) that read an external keynote deck ("brains go brrr —
finding balance in the age of Generative AI") and asked, in good faith, whether Hyperion's own
design already answers its central claim — that AI capability is exponential while human cognitive
bandwidth is linear/finite, and that the resulting gap shows up as burnout, eroded judgment, and
lost meaning, not just wasted time. CLAUDE.md's own principles (User Control, Explainability,
Progressive Complexity) already answer *some* of this well. These items are the parts that don't
have a real architectural home yet.

- **Forced "think" checkpoint before intent decomposition — landed (2026-07-16).** Today (before
  this pass): utterance → Intent Engine → decomposition happened with zero friction, by design
  (docs/05). `IntentEngine::set_think_mode` opts a session into a real, human-owned pause *before*
  Hyperion decides what a goal means — not a confirmation dialog for a risky action (that already
  existed, see `hyperion-capability`'s consent gate), but a genuine moment for the human's own
  reasoning to run first: `handle_utterance` withholds decomposition entirely
  (`HandleOutcome::PendingThink`) until an explicit `IntentEngine::proceed_with_decomposition`
  call. `hyperion-console`'s `/think on|off`/`/think-proceed` meta-commands are the real,
  human-facing surface — proven end to end: a paused utterance produces no decomposition at all
  (zero leaves) until `/think-proceed`, at which point the exact same decomposition happens as if
  think mode had been off the whole time. Opt-in and per-session, matching this item's own explicit
  constraint — a session that never enables it behaves exactly as before.
- **Declared judgment/taste/empathy/context boundary, distinct from "risky" — landed (2026-07-16).**
  The existing consent gate triggers on irreversibility/cost/security — a different axis from
  "this decision is a matter of taste or empathy and deserves more human involvement regardless of
  how reversible it is" (e.g., branding a startup vs. filing its paperwork, dispatched identically
  before this pass). `hyperion-coordination::catalog::judgment_class_for` really classifies a task
  predicate (this item's own worked example: `"branding"` is `JudgmentClass::TasteOrEmpathy`,
  `"legal_formation"` is `JudgmentClass::Mechanical`), stamped onto each real `TaskNode` at
  `create_session` time. `allocate` appends a real, second `ReasoningStep` to a `TasteOrEmpathy`
  task's Explanation Record naming the reason — advisory only, per CLAUDE.md's User Control
  principle: it never changes dispatch, routing, or eligibility. Proven end to end: branding's own
  dispatch carries the extra step, business_model's dispatched in the very same tick does not.
  Honest scope boundary: the classification table is small and hardcoded (one real predicate today,
  matching this crate's own existing `required_capabilities_for` precedent) and nothing yet
  consumes the signal on a human-facing surface — the reasoning step is real and queryable, not
  yet surfaced through `hyperion-console`.
- **Cache-protection throttle for the human's own skill — the signal half landed (2026-07-16).**
  `hyperion-memory`'s tiered design already protected the *system's* memory ("your brain has a
  cache, don't empty it completely" — the deck's own framing); nothing protected the *user's*.
  `MemoryEngine::count_procedural_delegations` now really counts how many times a specific kind of
  task (a caller-supplied `entity_key`) has been delegated within a caller-supplied window, over
  the Procedural tier; `hyperion-api-gateway::ApiGateway::check_skill_delegation_signal` (a new
  `MemoryQuery` scope) is the real bridge this item's own suggested home named ("surfaced back
  through `hyperion-explainability` rather than only used internally") — a threshold-crossing
  count opens a real, completed Explanation Record naming the count and threshold. Deliberately
  advisory, never enforcing (CLAUDE.md's User Control principle: "Hyperion assists. It does not
  control."). Still missing, honestly: no console/workspace surface actually calls this or asks
  the user "want to do the next one yourself?" yet — this closes the *signal*, not the end-to-end
  UX.
- **"Was this meaningful" signal, distinct from "was this fast" — landed (2026-07-16).**
  `hyperion-observability` tracks system health and latency; nothing tracked whether a completed
  goal actually mattered to the user, as opposed to how quickly it was produced.
  `hyperion-console`'s new `/meaningful yes|no` meta-command is a real, optional per-goal
  reflection — persisted through a real `hyperion_memory::MemoryEngine` (a new, small dependency
  this console gained for exactly this), reflecting on `/meaningful`'s own real last-handled-goal
  text (`ConsoleSession::last_utterance`), never a task's speed or latency; bare `/meaningful` asks
  what it would record without recording anything. Proven end to end: a recorded reflection is
  really findable afterward through this same console's own `/recall`, since it's stored in the
  same shared Knowledge Graph, not a second, parallel store. Honest scope boundary: this is
  user-invoked, not automatically prompted after every goal (deliberately — CLAUDE.md's Progressive
  Complexity principle, and the item's own "optional" framing), and nothing yet aggregates
  reflections into a queryable trend.
- **Teaching mode — the explicit-invocation half landed (2026-07-16).** The model-role catalog
  (planning/coding/reasoning/vision/etc., docs/23) had no role oriented around building the
  *user's* competence rather than producing an artifact — nothing that explains the underlying
  principle instead of just the output. `hyperion-console`'s new `/teach <topic>` meta-command is
  the real, explicit invocation this item asks for: a prompt shaped to ask for the reasoning behind
  an answer, not just the answer, dispatched through the exact same
  `current_backend.capability_ref()` path (and real cloud-consent gate) `run_undecomposed_goal`
  already established. Deliberately *not* built as a new `hyperion_ai_runtime::ModelClass` variant
  — this workspace has no genuinely distinct teaching backend anywhere to register one under, and
  minting a class nothing could ever resolve against would be real-looking, never-exercised API
  surface (the same discipline this workspace already applies elsewhere: exercise the real thing,
  never fake it). A real, separate `ModelClass::Teaching` is honest future work once a genuinely
  different backend for it exists — named here, not built prematurely.

Each of the above is a backlog item, not a design — none has interfaces, contracts, or a chosen
approach yet. Per CLAUDE.md's own engineering principle ("design APIs before implementation"), the
next step on any of these is a design pass, not code.

- **`hyperion-ai-runtime`'s cancellable streaming, landed (2026-07-16)** (docs/22's own "Cancellable
  streaming (§Data Structures' `TokenStream`)" item). `LocalAiRuntime::cancel(request_id)` was a
  literal no-op stub (`pub fn cancel(&self, _request_id: u64) {}`) — this closes it for real. A new
  `CancellationToken` (wrapping an `Arc<AtomicBool>`) threads through `InferenceBackend::generate`'s
  signature as a fourth parameter; a new `LocalAiRuntime::infer_cancellable(request_id, ...)`
  registers a real token under the caller-supplied `request_id` in a real `in_flight: Mutex<
  HashMap<u64, CancellationToken>>` registry before calling `generate`, and `cancel(request_id)`
  looks it up and flips it for real. The existing `infer()` is untouched — it delegates to the same
  internal path with a `CancellationToken::never_cancelled()` that no caller can ever reach, since
  no `request_id` is ever surfaced for it. Honest split, not uniform: `hyperion_ai_runtime::
  candle_backend::CandleBackend` is the one real backend in this crate with a genuine per-token
  loop, so it's the one that actually checks `cancel.is_cancelled()` once per token and stops early,
  decoding whatever was already sampled; every HTTP-backed backend (`MockBackend`,
  `OpenAiCompatBackend`, `AnthropicBackend`, `GeminiBackend`) receives the same token but ignores it
  — a single blocking round trip has no per-chunk boundary to check it at, named in each one's own
  doc comment rather than faked. Proven end to end: a new synthetic `StepCountingBackend` test
  (`tests/infer.rs`) proves the runtime's own registry/`cancel()` plumbing reaches the token
  `generate` sees without needing the `candle` feature; a second, `candle`-feature-gated test
  (`tests/candle_inference.rs`) proves the real thing — a real `infer_cancellable` call against the
  real downloaded TinyStories model, cancelled from a concurrent real thread mid-generation,
  produces genuinely fewer real generated tokens than an uncancelled real run of the same prompt.

- **`hyperion-context`'s semantic summarization, landed (2026-07-16)** (docs/06 §2's own `summary`
  inclusion mode, previously blocked on "Phase 3's Local AI Runtime" — which now exists, hardened
  this same day with cancellable streaming above). `ContextEngine::summarize` truncated an entry's
  metadata to its first 3 fields as a stand-in for a real summary; a new `ContextEngine::
  new_with_ai_runtime(graph, ai_runtime)` constructor wires a real `hyperion_ai_runtime::
  LocalAiRuntime` in (no circular dependency — `hyperion-ai-runtime` has no reverse dependency on
  this crate), and `summarize` now sends the entry's metadata through a real `ModelClass::Slm`
  inference call, returning the model's real generated text as the entry's `summary`-mode content
  rather than a truncated copy of its own metadata. `ContextEngine::new` (no `ai_runtime` supplied)
  keeps the exact previous behavior unchanged, and `summarize` falls back to the same truncation
  stand-in — not an error — when this caller's token isn't authorized for real inference or
  nothing is resident locally for `ModelClass::Slm`, exactly like this method's own caller already
  tolerates one unreachable anchor without failing the rest of `assemble()`. `hyperion-console`'s
  real `ConsoleSession::open` is the one production caller wired end to end: it now builds its real
  `LocalAiRuntime` before its `ContextEngine` and passes it through, so every real console session's
  own Context Bundles get real summarization, not the stub. Proven end to end in a new
  `tests/summarization.rs`: a real `MockBackend`-backed run produces the backend's own real,
  distinguishable response text as a plain string (not the old truncated JSON object); a run with
  no model registered for `ModelClass::Slm` falls back to the truncated object, not a panic or an
  error; and a run built via the original `ContextEngine::new` (no `ai_runtime` at all) is
  byte-for-byte identical to this crate's pre-existing behavior.

- **`hyperion-netstack`'s `robots.txt` fetching/parsing, landed (2026-07-16)** (docs/19's own
  named "`robots.txt` fetching/parsing" gap). `FetchedPage::robots_disallowed` was always
  hardcoded `false` from `ReqwestFetchBackend`, even under the real, feature-gated HTTP client M10
  already landed. A new, dependency-free `robots::RobotsRules` parses a real `robots.txt` body:
  it selects the `User-agent` group naming this crate's own real UA (`hyperionos-netstack`, now
  also sent as the real client's own `User-Agent` header) if one exists, falling back to
  `User-agent: *`, then resolves `Allow`/`Disallow` by longest-matching-prefix-wins -- the same
  real precedence real crawlers use, not a naive first-match or an "apply every group" merge.
  `ReqwestFetchBackend::fetch` performs a real `GET {scheme}://{host}/robots.txt` *before* ever
  requesting a real page (a real crawler must not fetch a path it was told not to, not merely
  label it disallowed after fetching it anyway), cached per real host+scheme for this backend's
  own lifetime so a session fetching many pages from the same origin only ever fetches its
  `robots.txt` once. A `robots.txt` that can't be reached at all (404, connection failure) allows
  everything, the real convention for "no `robots.txt` exists." `MockFetchBackend` is unaffected --
  a fixture still declares the flag directly. Proven end to end: 7 fast, dependency-free unit
  tests in `robots.rs` itself cover group selection, wildcard fallback, longest-prefix-wins, empty
  values, and comment/blank-line handling; a new `candle`-style `real-http`-feature-gated
  `tests/real_robots.rs`, against a real local HTTP/1.1 fixture server (not a remote host, so the
  test can assert on which real requests were and weren't received), proves a disallowed path is
  genuinely never fetched, an allowed path fetches normally, a missing `robots.txt` allows
  everything, and a second page fetch against the same host reuses the cached ruleset rather than
  re-fetching it.

- **`hyperion-netstack`'s `schema.org`/JSON-LD/OpenGraph microformat parsing, landed
  (2026-07-16)** (docs/19's own named "no real `schema.org`/JSON-LD/OpenGraph microformat parser
  exists" gap). `FetchedPage::structured` was always `None` from `ReqwestFetchBackend`, so
  `extract_entity`'s own structured-data-preferred branch was dead code against every real fetch.
  A new, dependency-light `microformats::parse` (using the already-present `scraper` crate) reads
  a real `<script type="application/ld+json">` block first (schema.org's own typed vocabulary,
  mapped through a deliberately narrow, explicit `@type` → `EntityType` allowlist -- an
  unrecognized or generic type honestly falls back to `EntityType::WebPage`, the same floor
  `MockExtractionBackend`'s own doc comment already establishes for "no confident entity", not a
  guessed-at specific type with no real evidence behind it), falling back to real
  `<meta property="og:*">` OpenGraph tags (requiring at least a real, non-empty `og:title`) when
  no JSON-LD block parses. `ReqwestFetchBackend::fetch` now populates `FetchedPage::structured`
  with the real result rather than always `None`; `MockFetchBackend` is unaffected. Named scope
  boundary, not silently assumed complete: nested JSON-LD relationships (`author`/`publisher`,
  etc.) are not extracted as `StructuredSignal::relationships` -- real nested-graph traversal this
  module does not attempt, matching every real backend in this crate's own `relationships:
  Vec::new()` scope rather than half-building it. Proven end to end: 6 fast, dependency-free unit
  tests in `microformats.rs` cover JSON-LD parsing, unrecognized-type fallback, malformed-JSON-LD
  falling through to OpenGraph, OpenGraph-alone parsing, a missing-title OpenGraph block correctly
  producing no signal, and plain pages with no markup at all producing `None`; a new
  `real-http`-feature-gated `tests/real_microformats.rs`, against a real local HTTP/1.1 fixture
  server, proves `ReqwestFetchBackend` itself populates a real signal from a real JSON-LD page and
  from a real OpenGraph-only page, and leaves `structured` `None` for a real plain page -- with an
  explicit note on why this crate's own `NetstackHub`-level "structured wins over the model
  fallback" behavior (already proven via `MockFetchBackend` in `extraction_and_resolution.rs`)
  can't be re-proven against a local fixture: this crate's own real SSRF containment correctly
  refuses a loopback target at that layer, the same reason `real_web_fetch.rs`'s own hub-level
  tests use a real remote host instead of a local one.

- **`hyperion-plugin-framework`'s consent-diffing `plugin_update`, landed (2026-07-16)** (docs/24
  §5's own named gap: "this crate has no `plugin_update` distinct from `uninstall` + `install`; a
  caller wanting the diff-only UX composes those two calls itself"). A new
  `PluginRegistry::update(monitor, admin_token, plugin_id, new_manifest, available_depth,
  consented_to_new_grants, verifying_key) -> Result<Vec<CapabilityGrantRequest>, PluginError>`
  compares `new_manifest.requested_permissions` against the plugin's own currently-installed
  permission set (a new `grants_equal` helper, comparing by `(operation, scope)` — a reworded
  `justification` alone doesn't make a grant "new") and returns exactly the new grants a real
  consent UI should present; consent is required only when that diff is non-empty. A grant
  unchanged across the update reuses its exact original `CapabilityToken` (checked by identity,
  not just equivalent rights, in this landed work's own tests) rather than being re-derived from
  scratch the way composing `uninstall` + `install` would force; a grant the new manifest drops is
  really revoked via `monitor.cap_revoke`, not silently left grantable forever. Every contribution
  is re-registered from the new manifest — the old ones are removed first via a new
  `remove_registry_and_contributions` helper, factored out of `uninstall`'s own non-token cleanup
  so the two can never drift — since an update really replaces what a plugin contributes, not
  merely tops up its permissions. `install`/`uninstall` were refactored to share three small new
  helpers (`validate_contributions`, `needs_sandbox_token`, `register_contributions`,
  `remove_registry_and_contributions`) with `update` rather than duplicating their own validation/
  registration logic a third time; all of `install_and_uninstall.rs`'s pre-existing tests still
  pass unchanged, confirming the refactor preserved exact prior behavior. A new, small
  `PluginRegistry::tokens_of(plugin_id)` accessor (mirroring the existing `boundary_of`) exposes a
  plugin's currently-tracked tokens for real inspection — needed to make token reuse/revocation
  observable at all, by a real caller or this landed work's own tests, without a second, parallel
  bookkeeping system. Proven end to end in a new `tests/plugin_update.rs` (7 tests): an update with
  no new permissions needs no consent; one adding a permission is rejected without consent and
  leaves the previous install completely untouched; one adding a permission returns exactly that
  one new grant and reuses the unchanged grant's exact original token; one dropping a permission
  really revokes that permission's token while the kept one stays live; an update replaces the
  registered contribution (not stacks it); updating an unknown plugin fails with `NoSuchPlugin`;
  and updating without `GRANT` rights fails with `Unauthorized`, matching `install`/`uninstall`'s
  own existing rights-gating tests.

- **`hyperion-plugin-framework`'s `version_variant()`, landed (2026-07-16)** (docs/24 §5's own
  named gap and pseudocode: a structurally incompatible `capability_id` collision used to be
  rejected outright with `PluginError::CapabilityCollisionIncompatible`, failing the *entire*
  install rather than minting the doc's own `version_variant()` id). A new, private
  `registry::version_variant` helper is real and deterministic — `capability_id#N` for the
  smallest `N >= 2` not already a registry key — and always terminates, so this path can never
  itself fail the way the collision it replaces used to.
  `PluginRegistry::register_implementation` now has three real outcomes on a `capability_id`
  collision, not two: identical contract merges as one more competing implementation (unchanged);
  incompatible contract registers under a real, distinct variant id as its own `RegistryEntry`
  (new); no prior entry still just inserts fresh (unchanged). An incompatible manifest therefore
  now installs *in full* — matching every other manifest's behavior — rather than aborting the
  whole install. Proven end to end: a structurally incompatible collision installs successfully
  and is discoverable under `document.summarize#2`, with its own real (not copied) contract, while
  the original entry is left untouched; a second, independent incompatible collision against the
  same base id gets its own `#3`, never colliding with the first variant.

- **`hyperion-privacy`'s soft-delete grace-period expiry timer, landed (2026-07-16)** (docs/16
  §10's own named gap: "nothing in this workspace runs a background clock that turns that grace
  period into a permanent `CryptoShred` once it lapses"). `erase(SoftDelete)` already registered a
  real, undoable `hyperion-recovery::ActionRecord`; it just stayed undoable forever, with no real
  mechanism to ever seal it. `hyperion-recovery` gains a genuinely new state transition,
  `RecoveryService::expire(action_id)`, and `ActionStatus` gains a fourth variant, `Expired`
  (distinct from `Aborted` — never took effect — and `Undone` — reverted: an `Expired` action's
  real effects stand permanently, exactly like `Committed`, but it can never be undone/redone
  again). Only valid from `Committed`; a new `RecoveryError::ActionNotCommitted` rejects expiring
  anything else. `hyperion-privacy`'s new `expire_lapsed_soft_deletes(recovery, now,
  grace_period_secs)` is the real, caller-driven clock (matching this workspace's hosted-simulator
  convention of a caller-supplied `now` over a real background thread): it sweeps every soft-delete
  `ActionRecord` still `Committed` (recognized by the exact `note` string `erase` already tags
  them with, since `hyperion-recovery`'s own `ActionRecord` deliberately stays privacy-agnostic —
  many other crates journal through it too) whose age has reached `grace_period_secs`, and expires
  each one for real. Named simplification: docs/16 §4's own `ErasureRequest.grace_period` is a
  per-request `Duration`; this sweep applies one caller-supplied duration uniformly at sweep time,
  since `ActionRecord` has no per-action grace-period field of its own to vary it by. Proven end to
  end: a new `hyperion-recovery` test file (`tests/expire.rs`, 5 tests) proves `expire` seals a
  `Committed` action against further undo, rejects expiring an `InFlight`/already-`Undone`/unknown
  action, and — critically — proves an `Expired` action's own real effects still count as a
  genuine conflict blocking an *earlier* action's own undo, exactly like `Committed` does (not
  silently excluded the way `Aborted`/`Undone` are). A new `hyperion-privacy` test file
  (`tests/grace_period_expiry.rs`, 5 tests) proves a soft-delete within its grace period is
  untouched and stays undoable; one past it is expired and never undoable again; sweeping twice
  never double-expires the same action; a `CryptoShred` erasure (which journals nothing) gives the
  sweep nothing to touch; and an unrelated subsystem's own real `ActionRecord` (not tagged with
  this crate's own soft-delete note) is never swept even once old enough, and stays undoable.

- **`hyperion-explainability`'s rolling Brier-score calibration tracking, landed (2026-07-16)**
  (docs/18 §10/§13's own named gap: "per-Agent/Capability calibration drift over time is not
  tracked; each `ConfidenceScore` is a point-in-time value with no aggregation"). A new
  `ExplanationStore::calibration_score(agent_id, capability_ref) -> Option<CalibrationScore>`
  computes a real, standard Brier score (mean squared error between each record's own
  `confidence.value` and its real observed outcome — `1.0` for `ControlState::Completed`, `0.0`
  for `ControlState::RolledBack`) over every real record this store already holds for that pair —
  no new store, no background job, just real arithmetic over data already flowing through the
  explain-then-commit pipeline this crate's own M8-era work already made real. `Proposed`/
  `Executing`/`Interrupted`/`Modified` records have no real terminal outcome yet, so they're
  excluded, matching `ExplanationStore::incomplete`'s own existing convention for what counts as
  resolved. `CalibrationScore.alert` is docs/18 §13's own "feeding an alert if an Agent's stated
  confidence systematically diverges from observed outcomes," `true` once the score crosses a
  real, documented threshold (`0.25` — a coin-flip-confidence forecaster's own score, a real
  reference point, not an arbitrary tuning knob) with enough real samples (`5`) to trust the
  signal rather than a tiny sample's noise. Proven end to end: 7 fast, dependency-free unit tests
  in the new `calibration.rs` module cover no-matching-records, non-terminal exclusion,
  no-confidence exclusion, a perfectly-calibrated real score of `0.0`, a confidently-wrong real
  Agent alerting once past the sample threshold and *not* alerting below it, and independent
  scoring per distinct `(agent_id, capability_ref)` pair; a new `tests/calibration.rs` (5 tests)
  proves the same behaviors through the real `ExplanationStore` API end to end, not just the pure
  scoring function in isolation.

- **`hyperion-release-gate`'s sigma-based statistical-significance regression testing, landed
  (2026-07-16)** (docs/36 §1/§2's own named gap: "`RegressionGate` is a flat percentage threshold;
  docs/36's `{sigma: f32}` variant needs a sample-variance history this crate doesn't maintain").
  `RegressionGate.threshold_pct: f32` becomes a real `RegressionThreshold` enum —
  `Percent(f32)` (this crate's original mechanism, unchanged behavior) or `Sigma(f32)` (new, real).
  `BenchmarkRegistry` gains a real, per-`(spec_id, hardware_profile)` rolling result window
  (`RegressionGate.baseline_window_builds` trailing `p99_ms` values — docs/36 §1's own
  `baseline_window: {builds: u32}`), bounded so it never grows unboundedly across a long-running
  process. A new `evaluate_sigma_gate` computes a real z-score — `(result - mean) / stddev` over
  the window's own real, computed mean and population standard deviation — gated the same way
  `evaluate_gate`'s percent path already was; fewer than 2 real prior results has nothing real to
  compute a variance against yet, `Pass`, matching the `Percent` path's own "no baseline yet"
  precedent. A real, exactly-zero-variance history (every prior result identical) with a result
  that genuinely differs is handled as maximally significant rather than a division-by-zero crash.
  Proven end to end in a new `tests/sigma_gate.rs` (8 tests): too few samples passes; a result
  within real historical variance passes; one genuinely far outside it blocks (and separately,
  only warns under a `Warn` gate); a real zero-variance history correctly flags any real deviation
  and correctly passes an identical repeat; the rolling window is really bounded (old, evicted
  results provably stop influencing the score); and different hardware tiers never share a
  window, matching this crate's own pre-existing same-tier-only invariant for `Percent` gates.

- **`hyperion-capability`'s real `WireToken` replay resistance / signing, landed (2026-07-16)**
  (docs/03's own named gap: "confidentiality or replay resistance for a token in transit... requires
  either transport-level access control... or cryptographic signing — the latter is M9's job...
  not repeated here ahead of its own milestone"). M9 (`hyperion-crypto`, real Ed25519) has existed
  since earlier this session and is now used here exactly the way `hyperion-plugin-framework`'s
  manifest signing and `hyperion-ai-runtime`'s model-descriptor signing already established: a new
  `WireToken.signature: Option<Signature>` field, a new `WireToken::signed(token, keystore)`
  constructor producing a real signature over every other claimed field's own canonical bytes, and
  a new `CapabilityMonitor::authenticate_wire_token_signed(wire, verifying_key)` that rejects a
  missing or invalid signature with a new `Fault::SignatureInvalid` before any liveness/rights
  check even runs — a forged or replayed-from-elsewhere claim never reaches that check at all. The
  original, unsigned `WireToken::from`/`CapabilityMonitor::authenticate_wire_token` path is
  completely unchanged — this crate makes signing possible, it does not make it mandatory
  workspace-wide; wiring a real caller (`hyperion-ipc`'s `Endpoint`, `hyperion-supervisor`'s
  spawned-service handoff) to actually use it by default is real, separate follow-up work, not
  attempted here (both would need a `Keystore`/`VerifyingKey` threaded into state that doesn't hold
  one today — a real, distinct integration task from the primitive itself). No dependency cycle:
  `hyperion-crypto` has no reverse dependency on `hyperion-capability`. Proven end to end in 5 new
  tests: a genuinely signed token authenticates against its own real signer's `VerifyingKey`; an
  unsigned token is rejected by the signed entry point; a token signed by a real, different device
  is rejected under the wrong `VerifyingKey`; a field tampered with after signing invalidates the
  signature; and a genuinely signed but revoked token still fails the real liveness check — a real
  signature never bypasses the real revocation graph. All 8 of this crate's own pre-existing wire
  tests still pass unchanged, and the full workspace (every downstream crate depending on
  `hyperion-capability`) builds and tests clean.

- **`hyperion-sdk`'s real `package_hash` content fingerprinting, landed (2026-07-16)** (docs/25 §4's
  own named gap: "`PublishSubmission.package_hash` is left at `0` by `prepare_submission`"). A new
  `package_hash(contract, implementation)` computes a real BLAKE3 hash (`hyperion_crypto::hash`,
  already this crate's own dependency — no new one added) over a new `canonical_submission_bytes`
  encoding of every field describing what a submission's content actually *is* (`Contract`'s
  `id`/`version`/`summary`/`inputs`/`outputs`/`side_effects`/`permissions_requested`/`trust_level`,
  `Implementation`'s `contract_id`/`name`/`runtime`/`latency_class`/`requires_consent`/
  `native_binary`), truncated to the field's own `u64` width from BLAKE3's real 256-bit digest.
  Distinct from `to_plugin_manifest`'s own real Ed25519 signature (real since M9, not the
  non-cryptographic checksum this bullet's own prior text described — that text was already stale):
  the signature authenticates the *publisher*; `package_hash` fingerprints the *content*, unaffected
  by `quality_score` or which permissions were statically observed on a given build. Proven end to
  end in a new `tests/package_hash.rs` (7 tests): the hash is no longer the old hardcoded `0`;
  identical content really fingerprints identically and deterministically; a different contract id,
  implementation name, native-binary descriptor, or side-effects/permissions set each really
  changes the hash; and re-scoring the same content (a different `quality_score`) or a different
  statically-observed-permissions list leaves the content fingerprint unchanged. All 19 of this
  crate's own pre-existing tests still pass unchanged.

- **`hyperion-model-router`'s percentage-based canary traffic splitting, landed (2026-07-16)**
  (docs/23's own named gap: "`RolloutStage::Canary` is tracked and lightly discounted in scoring,
  but no random sampling actually splits live traffic by percentage"). `RolloutStage::Canary`
  gains the real `f32` traffic-percentage payload docs/23 §Data Structures always specified
  (`Canary(f32)`); `ModelRouter::route` now really samples it via a new, deterministic
  `canary_sampled_in(invocation_id, impl_id, pct)` — a real hash of `(invocation_id, impl_id)`
  normalized into `[0, 1)` and compared against `pct` — so only that declared fraction of live
  calls even consider the candidate this cycle; the rest are excluded with a new
  `ExclusionReason::CanaryNotSampled` and fall straight through to whatever GA (or other in-sample
  Canary) candidate already exists, docs/23's own "existing fallback chain still live as a safety
  net." `invocation_id` generation moved to the top of `route()` (previously assigned only when
  building the final `RoutingDecision`) so the same id both drives the real sampling decision and
  labels the decision it produced. Additive, not a replacement: a candidate that *is* sampled in
  still carries the exact same modest `availability_fit` discount (`0.8`) this crate always applied
  to Canary. `hyperion-api-gateway::router_bridge`'s own exhaustive match over `ExclusionReason`
  gained the new variant's real rejection-reason text as part of this same change — the one real
  downstream call site this enum change touched. Proven end to end in a new
  `tests/canary_traffic_splitting.rs` (5 tests): a `Canary(0.0)` is never sampled in and the real
  GA candidate is chosen every time; a `Canary(1.0)` is always sampled in; a `Canary(0.3)` lands
  within `[20%, 40%]` over 2,000 real calls; two independent `Canary(0.5)` candidates for the same
  capability sometimes disagree (proving independent sampling, not correlated/lockstep behavior);
  and a sampled-in candidate still carries the real `0.8` availability discount. All 8 of this
  crate's own pre-existing routing tests, and all 22 of `hyperion-api-gateway`'s, still pass
  unchanged.

- **Ensemble/verification dispatch, landed (2026-07-16)** (docs/23 §Algorithms 5's own named gap:
  `route()` computed `needs_verification` and reported it in `Rationale`, but nothing ever actually
  dispatched a second candidate or reconciled agreement/disagreement). Real, but deliberately not
  in `hyperion-model-router` itself — that crate's own architecture is "a decision, never an
  execution," so the real dispatch lives in `hyperion-api-gateway::ApiGateway::verify_with_ensemble`,
  the crate that already bridges decision to real invocation. When `needs_verification` is true,
  it picks the highest-composite candidate with a different `ImplKind` than the primary (a real,
  already-available "architecturally distinct" signal — no new taxonomy invented), dispatches it
  through the exact same real `ApiGateway::dispatch_one` every ordinary call already uses, and
  compares real outputs. Real agreement genuinely boosts confidence — a real, deterministic
  `boost_confidence` (halves the remaining distance to `1.0`) — recorded as a second, superseding
  `set_confidence` call tagged `ConfidenceMethod::Ensemble`, and returned as a new
  `InvokeResponse.ensemble: Option<EnsembleOutcome>`. Real disagreement is never silently resolved:
  this crate has no `designated_tiebreaker` concept to consult, so it surfaces as a new
  `ApiError::EnsembleDisagreement`, carrying both real outputs, rather than discarding one. Fails
  open — no ensemble dispatch at all — when there's no architecturally distinct candidate to
  verify against, or the verifying candidate itself can't dispatch, so the primary's already-
  successful result is never blocked on a verification that can't happen. Proven end to end: two
  competing, architecturally distinct, non-local-model candidates that both really dispatch
  through the identical real stub fallback genuinely agree, boosting recorded confidence past 0.5
  and re-tagging its method as `Ensemble`; two real, distinct local models (different `ModelClass`/
  `model_id`, so `MockBackend`'s own real echo genuinely differs) registered directly on the
  router genuinely disagree, returning both real outputs rather than silently picking one; and a
  candidate pool with only one `ImplKind` present skips ensemble dispatch entirely rather than
  fabricating a partner.

- **`hyperion-device`'s Knowledge Graph node re-sync, landed (2026-07-16)** (this crate's own
  named gap: `heartbeat`/`tick`/`pair` update the in-process `DeviceObject` but never call
  `put_node` again, so the KG node was a real, queryable registration-time snapshot, never a live
  mirror). A new, shared `resync_kg_node` helper (and a `device_metadata` helper both it and
  `register` now go through, so the node's metadata shape can never drift between a first write
  and a later re-sync) closes it for all four mutators. `heartbeat`/`tick` — the two that
  previously took no `CapabilityMonitor`/`CapabilityToken` at all, since a device's own physical
  heartbeat isn't itself capability-mediated and `tick` sweeps every device at once with no single
  token to authorize it — now both take one real, caller-supplied token, the same "caller supplies
  the token a background/periodic action reuses" precedent `hyperion-federation`'s own
  `start_lease_heartbeat` already established, rather than this registry minting its own internal
  one. The KG node's metadata also gained a real `pairing` sibling field (the device's current
  `PairingRecord`, or `null`) — the "grants" half of the named gap — without changing the
  already-flattened `DeviceObject` fields any existing query relied on, so `pair`/`revoke` have
  something real to re-sync too. Proven end to end in a new
  `heartbeat_tick_pair_and_revoke_all_really_resync_the_kg_node` test: a real heartbeat's updated
  `last_heartbeat`, a real tick's changed `presence`, a real pair's granted capabilities, and a
  real revoke's cleared `pairing` all land on the exact same real node, never a second, parallel
  one. All 18 of this crate's own pre-existing tests, and `hyperion-threat-model`'s own device
  tests (already calling `pair`/`revoke` with a real token), pass unchanged.

- **`hyperion-crypto`'s multi-publisher trust store, landed (2026-07-16)** (this crate's own named
  gap: docs/24 describes verifying a plugin manifest's signature "against publisher's registered
  key," implying a registry of many trusted publisher keys, but no such registry existed anywhere
  in this workspace). A new `PublisherRegistry` (a real `publisher_id -> VerifyingKey` map,
  `register`/`verifying_key_for`) is exactly that — real key rotation (re-registering an id
  replaces its key), and a real, honest `None` for an unrecognized publisher, never a silent
  fallback to some other trust. `hyperion-plugin-framework` is its real first consumer: new
  `PluginRegistry::install_with_publisher_registry`/`update_with_publisher_registry` resolve a
  manifest's real trusted key from its own declared `publisher` field instead of taking one
  caller-supplied key on faith, closing that crate's own mirrored "multi-party / publisher trust
  stores" gap. `install`/`update` are unchanged and still every existing caller's default — the
  registry-aware entry points are additive, both delegating to the identical shared
  `install_validated`/`update_validated` logic so the two verification paths can never drift on
  what happens after signature verification. A manifest claiming a publisher it wasn't really
  signed by is a real `PluginError::SignatureInvalid`; a manifest from a publisher nobody
  registered a key for is a new, real `PluginError::UnknownPublisher`, never silently accepted.
  `hyperion-update`'s own mirrored deferred bullet is left open, explicitly: `UpdateManifest` has
  no `publisher` field to key a lookup by — docs/32 describes a single-vendor OS update model, not
  a marketplace, so closing it there would mean inventing a field that doc never asks for, not
  closing a gap it names. Proven end to end in a new `tests/publisher_registry.rs` (5 tests) plus
  3 in `hyperion-crypto` itself: two publishers each verify against their own real key; a manifest
  claiming a publisher it wasn't really signed by is rejected; an unregistered publisher is
  rejected; and `update_with_publisher_registry` resolves the same real trust. All of both crates'
  pre-existing tests pass unchanged.

- **`hyperion-model-router`'s staged-rollout percentage now really comes from
  `hyperion-update`, landed (2026-07-16)** (this crate's own named gap: "*deciding* what
  percentage to declare and when to ratchet it up over a real rollout's lifetime remains
  [32 — Update System]'s own job" — real canary sampling existed, but nothing ever actually called
  it from a real rollout). `hyperion_update::UpdateOrchestrator::apply_update_with_rollout` is a
  new, additive sibling of `apply_update` (unchanged, still every existing caller's default; both
  delegate to a shared `apply_update_inner` so the two paths can never drift on anything but the
  rollout side effect) — a real, caller-supplied `ImplId` (this crate's own `UpdateManifest`
  subject has no numeric Model Router identity of its own to derive one from, since multiple
  competing implementations can share one `capability_id` string) is really promoted via
  `ModelRouter::set_rollout_stage` at every real, health-gated stage: `Canary(stage.percent / 100)`
  on each stage that passes health, `Ga` once every stage has, and demoted back to `Shadow` — never
  left live at a partial percentage — on a health breach, before the real
  `UpdateError::RolloutHealthBreach` this crate already returned. No dependency cycle
  (`hyperion-model-router` has no reverse edge onto `hyperion-update`); a new
  `UpdateError::ModelRouter(#[from] ModelRouterError)` variant folds the router's own capability
  check into this crate's existing error type. Proven end to end in a new
  `tests/staged_rollout_model_router.rs` (3 tests): a fully healthy rollout reaches `Ga`; each
  stage's own health check observes exactly the *previous* stage's real promotion (`Shadow` →
  `Canary(0.01)` → `Canary(0.10)` → `Canary(0.50)`), proving genuine ratcheting rather than a jump
  straight to `Ga`; and a health breach demotes the real candidate back to `Shadow` rather than
  leaving it stuck at whatever partial percentage it last reached. All 29 of this crate's own
  pre-existing tests pass unchanged.

- **`hyperion-memory`'s real AI-backed Working → Episodic distillation, landed (2026-07-16)**
  (docs/08 §5.1's own named gap: this crate accepted a caller-supplied summary rather than
  summarizing the turn buffer itself, deferred pending a real summarization capability — which
  now exists). `MemoryEngine::new_with_ai_runtime` wires a real `hyperion-ai-runtime::LocalAiRuntime`
  in, the same real path `hyperion-context::ContextEngine::new_with_ai_runtime` already proved; a
  new `MemoryEngine::distill_working_memory` turns a session's real `WorkingMemory` turn buffer
  (docs/08 §4's RAM-only ring buffer, "discarded at session close after distillation") into one
  real Episodic `MemoryRecord` — a real, model-generated summary when an `ai_runtime` is wired,
  falling back to a plain verbatim join of every turn when it isn't, the token lacks real-
  inference rights (`RuntimeError::Unauthorized`), or nothing is resident locally
  (`RuntimeError::InfeasibleLocally`) — a caller loses summarization fidelity, never the memory
  itself, the same graceful-degradation contract `ContextEngine::summarize` already established.
  `MemoryEngine::new` is unchanged and still every existing caller's default; the AI-runtime-aware
  constructor is additive. `distill_working_memory` is never called automatically —
  `WorkingMemory` has no lifecycle hook of its own, so a real caller distills explicitly (e.g. at
  session close). This crate's own remaining "model-estimated salience" bullet is left open,
  explicitly: it needs a real, structured numeric estimate from a model, not just free-text
  summarization, and extracting a reliable float out of free-form model output isn't attempted
  here. Proven end to end in a new `tests/distillation.rs` (4 tests): a wired, resident model
  produces a real summary distinguishable from the raw turns; an unregistered model class falls
  back to the exact verbatim join, not a failure; no `ai_runtime` wired at all keeps the same
  verbatim-join behavior; and a distilled record really persists as a real Episodic Knowledge
  Graph node with its own real `importance`/`pinned` fields. All 21 of this crate's own
  pre-existing tests pass unchanged.

- **`hyperion-trust-boundary`/`hyperion-supervisor`'s real IPC-rights dimension for the
  rendezvous socket, landed (2026-07-16)** (M2/M3/M5's own repeatedly-named gap: the seccomp
  filter allowlisted no `socket`/`bind`/`connect` syscalls and Landlock never handled `MakeSock`,
  so a supervised service could not actually bind a real `hyperion_ipc::Endpoint` at its own
  well-known `HYPERION_IPC_SOCK` rendezvous path — "would need allowlisting AF_UNIX socket
  syscalls and Landlock MakeSock rights, a real but separable extension," still open until now).
  `SpawnGrant` gains a real, distinct `ipc_rendezvous: Option<PathBuf>` field — `None` (every
  existing caller's default) grants no IPC rights at all; `Some(rendezvous_path)` is the one
  socket path this boundary may really bind. `apply_seccomp` additionally allowlists exactly four
  syscalls (`socket`/`bind`/`sendto`/`recvfrom` — confirmed via a real `strace` against a real
  minimal `UnixDatagram` bind/send/recv round trip, this crate's own established rigor, not
  guessed; `connect`/`listen`/`accept` are deliberately absent since `hyperion_ipc::transport::Endpoint`
  is connectionless) when `ipc_rendezvous` is `Some`. `apply_landlock` adds a real `MakeSock` rule
  scoped to the rendezvous path's *parent directory* — Landlock's creation-type rights can only
  ever be scoped to a containing directory, never one exact not-yet-existing filename, an honest,
  documented scope boundary rather than a hidden one. `hyperion-supervisor::Supervisor::spawn_sandboxed_inner`
  is the real production caller: every supervised service's grant now carries
  `ipc_rendezvous: Some(self.socket_path_for(&spec.name))`, the same path already exposed via
  `HYPERION_IPC_SOCK`. Proven end to end in two new `hyperion-trust-boundary` tests (extending the
  existing real, separate-process `probe` binary with a new `ipc-check` subcommand): a grant with
  `ipc_rendezvous` really binds a real `UnixDatagram` at the granted path and round-trips a real
  send/receive through it, while a bind attempt *outside* the granted rendezvous directory is
  really denied by Landlock; a grant with no `ipc_rendezvous` at all cannot bind a socket there
  either, since `socket()` itself stays denied by the baseline seccomp filter. All pre-existing
  tests in `hyperion-trust-boundary`, `hyperion-supervisor` (including the real, separate-process
  `real_supervision.rs` suite), and `hyperion-plugin-framework` (whose own `NativeBinary`
  sandboxed-execution grant now explicitly carries `ipc_rendezvous: None`, since a one-shot tool
  invocation has no rendezvous socket to bind) pass unchanged.

- **`hyperion-privacy` wrapped as a third real supervised service, landed (2026-07-16)**
  (`hyperion-supervisor`'s own reuse-map: ~30 Phase 2-10 crates need "nothing structural changed
  in their own `lib.rs`... only a real process entry point," and only two, `hyperion-observability`/
  `hyperion-explainability`, had one so far). A new `hyperion-privacy/src/bin/hyperion-privacy-service.rs`
  proves the same real M5 supervision mechanism generalizes to a third, independent subsystem: it
  mints its own local capability domain from its real spawn-time `WireToken` claim (the identical
  pattern the first two services established), requests a real `ConsentGrant` via
  `ConsentLedger::request`, and calls docs/16 §5's real `route_capability_call` for a domain
  requiring exactly that consent — its real, written state file shows `routing_decision=DispatchCloud`,
  proving the routing algorithm genuinely reflects a real, just-granted consent from *inside* a
  real sandboxed process, not a hardcoded decision. Proven end to end in a new
  `hyperion-supervisor` test, `a_third_independent_subsystem_runs_under_real_supervision_and_does_its_own_real_work`
  — deliberately not repeating the existing kill+respawn choreography already proven generically
  crate-agnostic by the first two services, per this crate's own "don't redo the same wrapping
  thirty times with no new engineering insight" scoping discipline. All pre-existing tests in
  `hyperion-privacy` and `hyperion-supervisor` pass unchanged.

- **Per-implementation privacy tier from the Plugin Framework manifest, landed (2026-07-16)**
  (`hyperion-api-gateway`'s own named gap: `router_bridge::to_router_descriptor` hardcoded every
  bridged candidate as `PrivacyTier::Local`, since `hyperion-plugin-framework::CapabilityManifest`
  carried no privacy-tier field at all). `CapabilityManifest`/`ImplementationDescriptor` both gain
  a real `privacy_tier: PrivacyTier` field — a narrowed local copy of docs/16's taxonomy, the same
  simplification `hyperion-model-router`'s own `PrivacyTier` already makes, deliberately not a
  dependency on that crate (matching the existing `ImplKind` adapter precedent). A publisher's
  real, declared tier now flows: `CapabilityManifest` → `register_implementation`'s
  `ImplementationDescriptor` → a new `router_bridge::to_router_privacy_tier` adapter →
  `hyperion-model-router`'s real `privacy_fit` scoring — instead of every candidate being
  indistinguishably `Local`. `hyperion-sdk::publish::to_plugin_manifest` is the real first
  populator: `Implementation.requires_consent` (previously folded only into `package_hash`'s
  canonical bytes, never acted on) now maps straight to `ConsentedCloud`/`Local`, giving that
  field its first real behavioral consumer. 25 pre-existing `CapabilityManifest` construction
  sites across 8 test files (plus one production site in `hyperion-sdk`) were updated to declare
  `privacy_tier` explicitly, all defaulting to `Local` — the exact behavior every existing caller
  already had. Proven end to end: a new `router_bridge` unit test confirms the adapter itself;
  a new `hyperion-api-gateway` integration test installs two otherwise-identical candidates
  differing only in declared privacy tier and confirms the `Local` one now genuinely outscores
  the `ConsentedCloud` one and wins real candidate selection — not just carries a different label.
  All pre-existing tests across `hyperion-plugin-framework`, `hyperion-api-gateway`,
  `hyperion-sdk`, `hyperion-scalability`, and `hyperion-agent-runtime` pass unchanged.

- **`hyperion-scalability`'s `Substitution` -> real resource-footprint mapping, landed
  (2026-07-16)** (this crate's own named gap: `fit::scheduler_backed_resolver` built a real
  fit-check backed by `hyperion_scheduler::Scheduler`'s live ledgers, but `types::Substitution`
  carried no `CapacityDescriptor` at all, so a caller still had to separately maintain a
  `ModelTier`/`CapabilityRef` -> footprint lookup table just to use it). `Substitution::CheaperLocalTier`/
  `AlternateImplementation` each gain a real `CapacityDescriptor` field — the footprint now
  travels with the fallback declaration itself, since whoever declares "fall back to this cheaper
  tier" already knows what it costs. A new `Substitution::footprint()` accessor
  (`ConsentedCloudUpgrade`/`Disable` real, honest `None`s: a cloud upgrade's footprint is the
  remote provider's problem, disabling costs nothing) is what `fit::scheduler_backed_resolver`
  now reads directly, dropping its caller-supplied `footprint_for` closure parameter entirely —
  `scheduler_backed_resolver(&scheduler)` instead of `scheduler_backed_resolver(&scheduler,
  footprint_for)`. `CapacityDescriptor` gained `PartialEq`/`Eq` to support this (all-`u32` fields,
  no floats). Fully self-contained: confirmed zero external crates construct or depend on
  `Substitution`. All 10 pre-existing call sites across 3 test files were updated to declare a
  real footprint explicitly, preserving exact prior test behavior. Proven end to end in two new
  unit tests on `Substitution::footprint()` itself (real footprint for the two variants that carry
  one, real `None` for the two that don't) plus all 3 existing `scheduler_fit.rs` integration
  tests continuing to pass with the closure parameter gone. All 13 of this crate's own
  pre-existing tests pass unchanged.

- **`hyperion-sdk`'s `Implementation.resourceProfile`, landed (2026-07-16)** (this crate's own
  named gap: "Not modeled — no consumer"). `types::Implementation` gains a real, optional
  `hyperion_scheduler::ResourceVector` field, mirroring the exact `privacy_tier` pattern above:
  `publish::to_capability_manifest` threads it into a new, identically-named
  `hyperion-plugin-framework::CapabilityManifest`/`ImplementationDescriptor` field, carried through
  `register_implementation` unchanged. `hyperion-agent-runtime::AgentRuntime::prepare_invoke` is
  the real consumer: it now looks the invoked `capability_ref` up in the installed
  `PluginRegistry` and submits that implementation's own declared reservation to the real
  Scheduler admission algorithm, instead of the same fixed one-token-per-second stand-in for every
  capability regardless of what it actually needs — falling back to that same fixed request when
  no installed implementation declares a profile. Every pre-existing `CapabilityManifest`/
  `Implementation` construction site across the workspace (`hyperion-sdk`'s own 3 test files, plus
  the same ~10 `CapabilityManifest` sites the `privacy_tier` bullet above touched) was updated to
  declare the new field explicitly, defaulting to `None` — the exact behavior every existing caller
  already had. Proven end to end in two new `hyperion-agent-runtime` integration tests: a
  capability declaring a reservation larger than the runtime's own ledger capacity is genuinely
  denied (`InvokeOutcome::QuotaExceeded`) by the real Scheduler — something the old hardcoded
  request could never trigger — while a capability with no declared profile still admits under the
  same fixed default as before. All pre-existing tests across `hyperion-sdk`,
  `hyperion-plugin-framework`, `hyperion-api-gateway`, `hyperion-scalability`, and
  `hyperion-agent-runtime` pass unchanged.

- **`hyperion-scheduler`'s model-tier degradation, landed (2026-07-16)** (this crate's own named
  gap: `schedule_epoch`'s non-admit branch had no way to ask "what's a cheaper `ResourceVector`
  for this capability," since `hyperion-model-router::ImplementationDescriptor` had no
  resource-cost axis and `TaskDescriptor` had no capability reference to look one up by — a schema
  change to both crates, not a wiring fix). `hyperion-model-router::types` gains a real
  `ResourceCost` struct — a narrowed local copy of this crate's own `ResourceVector` shape,
  deliberately not a shared dependency: `hyperion-scheduler` depends on `hyperion-model-router`
  (to ask for a cheaper alternative), so a reverse dependency would cycle. `ImplementationDescriptor`
  gains a real, optional `resource_cost: Option<ResourceCost>` field, and a new
  `ModelRouter::declared_costs(capability_id)` returns every real, non-`Shadow`,
  not-circuit-broken registered implementation's declared cost. `hyperion-scheduler::TaskDescriptor`
  gains a real `capability_ref: Option<String>`, and `Scheduler` gains an optional
  `Arc<ModelRouter>` field (`Scheduler::new_with_model_router`) — `schedule_epoch`'s non-admit
  branch now calls a new `try_degrade` that asks the wired router for every declared alternative,
  sorts by total footprint, and admits at the cheapest one that actually fits the real ledgers,
  falling back to the pre-existing aging-and-requeue behavior when nothing fits or no router is
  wired. `hyperion-api-gateway::router_bridge` — the seam that already bridges
  `hyperion-plugin-framework`'s registry shape to `hyperion-model-router`'s own — gained a new
  `to_router_resource_cost` adapter so a publisher's own `Implementation.resourceProfile`
  (gap landed just above) genuinely reaches `declared_costs` field-for-field, not just declared in
  isolation. `hyperion-agent-runtime::AgentRuntime` — this crate's one real production caller — is
  the real end-to-end consumer: a new `new_with_netstack_and_plugins_and_memory_and_model_router`
  constructor wires an optional `ModelRouter` straight into its own `Scheduler`, and
  `prepare_invoke` now names the invoked capability on every submitted `TaskDescriptor`. 11
  pre-existing `TaskDescriptor` construction sites and 3 `hyperion-model-router::ImplementationDescriptor`
  construction sites across the workspace were updated to declare the two new fields explicitly,
  defaulting to `None` — the exact behavior every existing caller already had. Proven end to end:
  3 new `hyperion-scheduler` unit tests (a non-fitting task admits at a cheaper registered
  implementation; a task with no `capability_ref` still ages and requeues; a task with no wired
  router still ages and requeues), a new `hyperion-api-gateway` unit test on the new adapter, and a
  new `hyperion-agent-runtime` integration test proving a capability whose own declared
  `resource_profile` doesn't fit the runtime's ledger is genuinely admitted via a separately
  registered, cheaper Model Router implementation instead of being refused outright. All
  pre-existing tests across `hyperion-scheduler`, `hyperion-model-router`, `hyperion-api-gateway`,
  `hyperion-agent-runtime`, `hyperion-sim`, `hyperion-memory`, and `hyperion-update` pass unchanged.

- **`hyperion-intent`'s generative decomposition, landed (2026-07-16)** (this crate's own named
  gap: "needs a real planning model ([22 — Local AI Runtime]'s real backend)... An utterance
  matching no template becomes a single, undecomposed root Intent... rather than a fabricated
  plan"). `IntentEngine` gains a real, optional `hyperion_ai_runtime::LocalAiRuntime` field via a
  new `new_with_plugins_and_ai_runtime` constructor, mirroring `hyperion-context::ContextEngine::
  new_with_ai_runtime`'s own precedent exactly. A new private `generate_template` runs only when
  no curated or plugin-contributed template matched: it prompts the model for a short ordered
  step list and turns each non-empty response line into one real `TemplateLeaf` depending on the
  line before it — a genuine, if modest, linear plan built from the model's own real response,
  never a fabricated dependency structure. `handle_utterance` gives a generated plan `0.9` (curated
  template) > `0.6` (generated plan) > `0.3` (no plan at all) confidence, and `IntentStatus::Planned`
  instead of `Proposed`, reusing the exact same `decompose_and_record`/think-mode pause path a
  curated template already goes through — no new dispatch branch needed. Degrades honestly at
  every real failure point `hyperion-context`'s own precedent established: no `ai_runtime` wired,
  an unauthorized token, no model resident for `ModelClass::Slm`, or a response with no usable
  lines all fall straight back to the pre-existing single-undecomposed-root behavior — proven by a
  new test using a `LocalAiRuntime` with no model registered. A second new test wires a real,
  registered `MockBackend`-backed model and confirms the root Intent lands `Planned` at confidence
  `0.6`, real leaves are recorded, and at least one leaf's predicate is genuinely derived from the
  model's own (mocked) response text — not an invented label. All 22 pre-existing
  `hyperion-intent` tests pass unchanged.

- **`hyperion-observability`'s background scheduled chain verification, landed (2026-07-16)**
  (this crate's own named gap: "`AuditLedger::verify_chain` is on-demand only, not run on a
  background schedule" — the ring-buffer write-ahead-spill half of that same bullet remains
  separately deferred). A new `AuditLedger::start_periodic_verification(interval)` spawns a real
  background thread that re-invokes `verify_chain` over the whole chain every real `interval`,
  mirroring `hyperion-federation::FederationHub::start_lease_heartbeat`'s own `Arc<Self>`/
  stop-flag/join-on-drop shape exactly (a new `VerificationSchedule` handle, structurally
  identical to `LeaseHeartbeat`). A caller reads `VerificationSchedule::last_report` for the most
  recent real `VerificationReport` instead of only ever being able to check on demand.
  `hyperion-observability-service`'s own real, long-running `main()` — the real consumer — now
  starts one of these (a real 60-second cadence) right after its pre-existing on-demand startup
  check, for as long as the process lives. Proven end to end with two new unit tests inside
  `ledger.rs`'s own internal test module (the only place that can simulate a realistic tamper,
  since `append` is deliberately the only public write path): one confirms the background
  thread's own first real tick reports `Intact`, and the other tampers an entry directly and
  confirms the background thread's own *next* tick — not an on-demand `verify_chain` call from
  the test — catches the corruption at its exact `seq`. All pre-existing tests across
  `hyperion-observability` and `hyperion-supervisor` (whose own real, sandboxed-process
  integration test spawns this exact service binary) pass unchanged.

- **`hyperion-explainability`'s `control.modify` signal plumbing, landed (2026-07-16)** (this
  crate's own named gap: "`types::ControlState` has `Interrupted`/`Modified` variants a caller can
  transition a record into, but no scheduler or Agent Runtime hook actually delivers those
  signals from this crate" — while auditing this, found `Interrupted` was already real,
  pre-existing, undocumented: `CoordinationSession::apply_dispatch_results` already transitions a
  task's record to `Interrupted` on a real `PendingConsent`/`QuotaExceeded` dispatch outcome; only
  `Modified` and `control.resume` were still open). `CoordinationSession` gains a real
  `last_explanation_by_task: Mutex<HashMap<NodeId, ExplanationId>>`, populated every dispatch tick
  in `apply_dispatch_results` alongside the existing terminal-state transition — since each real
  dispatch mints a fresh `ExplanationId` per tick, this is deliberately "most recent," not one id
  a task keeps for its whole history. `amend_task` — `hyperion-console`'s own real `/redo`
  meta-command's entry point — now looks the amended task's most recent real record up and
  transitions it to `ControlState::Modified`, honestly skipped (never an error) when the task was
  never dispatched yet. `ExplanationStore::transition` places no restriction on which state a
  record can move to, so transitioning an already-terminal (`Completed`/`RolledBack`) record to
  `Modified` is a real, well-defined operation, not a fabricated escape hatch. Proven end to end
  with 2 new `hyperion-coordination` integration tests: one drives a real dispatch to `Done`, calls
  `amend_task`, and confirms the record genuinely reads `Modified` afterward (not just that the
  enum variant compiles); the other confirms amending a never-dispatched task succeeds honestly
  with nothing to transition. `control.resume` (transitioning a record back to `Executing` once
  `Interrupted`) remains the one real signal still undelivered by any caller. All 14 pre-existing
  `hyperion-coordination` `worked_trace` tests pass unchanged.

- **`hyperion-explainability`'s `control.resume` signal plumbing, landed (2026-07-16)** — the
  last of that crate's own named "`control.interrupt`/`control.modify`/`control.resume` signal
  plumbing" gap, closing it entirely. `is_ready`'s `Unassigned`-only readiness check left a task
  `apply_dispatch_results` marked `Claimed` on a real `PendingConsent`/`QuotaExceeded` outcome
  permanently stuck — nothing ever moved it back. A new `CoordinationSession::resume_task` looks
  the named task's most recent real record up via the `last_explanation_by_task` map (added for
  `control.modify`), confirms it's genuinely `Claimed` with an `Interrupted` record (distinguishing
  a real pause from a terminally `Denied` task, which also leaves `Claimed` but with a
  `RolledBack` record — a new `CoordError::TaskNotInterrupted` refuses that case honestly, not
  silently), resets the task to `Unassigned` so the very next real `allocate()` tick genuinely
  re-attempts it, and transitions the record to `ControlState::Executing`. The actual re-dispatch
  mints its own fresh `ExplanationId`, exactly like every other real attempt, rather than
  inventing a mechanism to reuse one record across multiple attempts. No built-in HTN template in
  this workspace's own test fixtures can naturally reach a genuine `PendingConsent`/`QuotaExceeded`
  outcome (every template leaf's required Capability is always baseline, never requestable, for
  the specialization that dispatches it), so this is proven by a new internal `engine.rs` test
  module that fabricates the real, production-reachable `Claimed`+`Interrupted` state directly
  against the session's own private fields — the same technique `hyperion-observability`'s
  `ledger.rs` test module already uses to simulate a tamper the public API can't otherwise
  produce. Three new tests: a genuinely interrupted task really resumes (status `Unassigned`,
  record `Executing`); a `Claimed`-but-`RolledBack` (i.e. `Denied`) task is correctly refused; an
  unknown task name is rejected. All pre-existing `hyperion-coordination` tests (7 prior unit + 14
  `worked_trace` integration tests) pass unchanged.

- **`hyperion-netstack`'s nested JSON-LD relationship extraction, landed (2026-07-16)** (this
  crate's own named scope boundary: "`StructuredSignal::relationships` is always empty here -- a
  real schema.org JSON-LD document often nests related entities (`"author": {"@type": "Person",
  ...}`, `"publisher": {...}`), and extracting those... would need real nested-graph traversal
  this module does not attempt"). A new `microformats::extract_relationships` walks every
  top-level property of a real parsed JSON-LD object whose own value is itself an object (or array
  of objects) declaring a real `@type` of its own — the property name is the real predicate; the
  nested entity's identifier falls back through the same `@id`/`url`/`identifier` chain the
  top-level entity uses, plus `name` as an honest last resort for a stub that names nothing else.
  An untyped nested object (most schema.org properties — `address`, plain strings, numbers) never
  contributes a fabricated relationship. `parse_json_ld` now calls this instead of hardcoding
  `relationships: Vec::new()`; `extract::extract_entity` already cloned `structured.relationships`
  straight through unchanged. `hub::NetstackHub::web_research`'s own pre-existing real
  edge-writing loop — previously starved of input for the JSON-LD path, since `relationships` was
  always empty — now genuinely writes real typed edges from real fetched pages instead of only
  ever running for the (untouched) OpenGraph/heuristic paths, which never carried relationships
  anyway. `extract::HtmlHeuristicExtractionBackend`'s own `relationships: Vec::new()` remains
  exactly as scoped — a real HTML heuristic has no nested entity structure to walk at all.
  `MockFetchBackend` is unaffected — a fixture still declares `structured` directly. Proven with 3
  new tests: a nested typed `author` (with a real `@id`) and `publisher` (with only a real `name`)
  both become real relationships; a schema.org array of multiple authors each becomes its own
  relationship under the same predicate; an untyped nested object (`address`) contributes nothing.
  All 6 pre-existing `microformats` tests and this crate's full `--features real-http` suite pass
  unchanged (one pre-existing, unrelated `real_web_fetch.rs` timeout-classification test remains
  environment-flaky in this sandbox, confirmed via `git stash` to fail identically without this
  change).

- **`hyperion-intent`'s "conflict detection across active graphs" write-back prerequisite, landed
  (2026-07-16)** (this crate's own named gap: "needs multiple concurrently-*executing* Intents
  with real Agents mid-execution... for 'exclusive-resource conflict' to mean anything real" —
  auditing this found the real *prerequisite* still missing even where the precondition,
  `hyperion-coordination`, already exists: nothing ever wrote a leaf's real dispatch outcome back
  into its own Intent status past decomposition time). A new `IntentEngine::mark_status` is the
  same real read-modify-write `abandon_subtree`/`bump_version` already use, exposed for a real
  external caller. `CoordinationSession` gains an optional `Arc<IntentEngine>` field via a new
  `with_intent_engine` builder (the same `Option<Arc<...>>`/builder shape `with_recovery` already
  established) — `apply_dispatch_results`'s own real `Done` branch now calls `mark_status(...,
  IntentStatus::Completed)` best-effort, alongside its existing graph/recovery writes in that same
  code path, never failing the dispatch itself over a hiccup. `TaskNode.task_id` is already
  literally the Intent leaf's own `NodeId` (`create_session` sets it directly from the leaf), so
  no new identity mapping was needed. Real conflict detection itself — comparing genuinely
  `Executing` leaves across *multiple* active graphs — remains open; this closes only the
  write-back half that any such detection would need real data from. Proven with a new
  `hyperion-intent` unit test (`mark_status` really transitions status and bumps `updated_at`,
  leaving every other leaf untouched) and a new `hyperion-coordination` integration test (a real
  dispatch through a wired `IntentEngine` genuinely lands `IntentStatus::Completed` on the correct
  leaf, leaving an undispatched sibling `Planned`). All pre-existing tests in both crates pass
  unchanged.

- **`hyperion-knowledge-graph`'s real node deletion (tombstone), landed (2026-07-16)** (this
  crate's own gap named by its two real consumers: `hyperion-recovery`/`hyperion-privacy`'s "no
  node-delete operation (only edges tombstone)"). `NodeRecord` gains a real `tombstone: bool`
  field (`#[serde(default)]`, so every pre-existing WAL record replays as "not tombstoned" —
  unchanged behavior); a new `KnowledgeGraph::delete_node` tombstones a node exactly the way
  `unlink` already tombstones an edge, per docs/09 §10's own "deletions are tombstones...
  undoable within a retention window" precedent, now applied to nodes for the first time.
  `get`/`query`/`traverse`/`dump` all now exclude a tombstoned node — `get`/`traverse`'s own
  start-node check both return a real, honest `NotFound`, indistinguishable from a node that
  never existed. A plain `put_node` update on an already-tombstoned node id never silently
  resurrects it (the same "an insert never revives a deliberate deletion" invariant `link`
  already enforces for edges) — `delete_node` is the one real way a tombstone is ever set.
  `explain`/`get_at_version` deliberately still read a tombstoned node's history unfiltered — an
  audit/historical-read path, not a live-view one, matching this workspace's own Explainability
  principle ("Undo?"). Deliberately scoped to the KG-side primitive only: neither
  `hyperion-privacy::erasure::erase(CryptoShred)` nor `hyperion-recovery`'s "un-creating a freshly
  created object" limitation calls it yet — both crates' own doc comments now name this as real,
  separate follow-up wiring rather than a missing primitive. Proven with 9 new tests: real
  `NotFound` after delete; deleting an unknown/already-deleted node; exclusion from
  `query`/`dump`; `traverse` refusing a tombstoned start and never expanding into a tombstoned
  neighbor; the tombstone surviving a real WAL replay (`KnowledgeGraph::open` twice); and a plain
  `put_node` update never resurrecting one. All 25 pre-existing `hyperion-knowledge-graph` tests
  pass unchanged.

- **`hyperion-recovery`'s "un-creating a freshly created object," landed (2026-07-16)** (this
  crate's own named gap, closed the moment `hyperion-knowledge-graph::KnowledgeGraph::delete_node`
  landed above: "a recovery-point snapshot of an object that didn't exist yet is recorded as
  `None` and is simply not restorable"). `apply_snapshot` — the shared helper both
  `restore_objects`/`restore_to` and `redo` call — now calls the real `delete_node` for exactly a
  `None` snapshot entry, genuinely un-creating the object instead of silently leaving it behind;
  `GraphError::NotFound` is treated as a benign no-op (something else may have already deleted the
  same object by the time this snapshot applies), never a hard failure. One real, honestly-named
  asymmetry this doesn't close: `redo`'s own reverse direction re-creates via `put_node`, which —
  correctly, mirroring the CRDT tombstone-never-silently-resurrected invariant edges already have
  — can never resurrect a node this same path just tombstoned, so redoing an undone Create still
  leaves the object gone. A pre-existing test (`a_recovery_point_over_a_not_yet_created_object_
  cannot_undo_its_creation`) asserted the *old*, broken behavior directly — renamed and rewritten
  to assert the new, correct one (a real `GraphError::NotFound` after undo, not "still there").
  Proven further with a new test in `redo.rs` that documents the real asymmetry: redoing an
  undone Create reports `Targeted` but the object genuinely stays gone, not silently and
  incorrectly resurrected. All other pre-existing `hyperion-recovery` tests, plus
  `hyperion-coordination`'s own `recovery_bridge.rs` (whose real task-result nodes are always
  fresh creates — exactly this code path), pass unchanged.

- **`hyperion-privacy`'s `CryptoShred` wiring to the real `delete_node`, landed (2026-07-16)**
  (this crate's own named gap, the second and final consumer of
  `hyperion-knowledge-graph::KnowledgeGraph::delete_node`: "`erasure::erase` overwrites a node's
  current metadata with a tombstone-shaped placeholder rather than physically removing it").
  `erase`'s per-id loop now branches on `mode`: `CryptoShred` calls the real `delete_node` — a
  genuine tombstone no `get`/`query`/`traverse`/`dump` call ever surfaces again — while
  `SoftDelete` deliberately keeps the placeholder overwrite via `put_node` unchanged, since its
  own real grace-period `undo` restores through `put_node`, which could never un-tombstone a node
  `delete_node` had genuinely deleted (the same "an insert never resurrects a deliberate
  deletion" invariant edges already have). A pre-existing test asserted the *old* placeholder
  behavior for `CryptoShred` directly (`graph.get(...).unwrap()` succeeding, checking the
  placeholder's own fields) — renamed and rewritten to assert the new, correct one (a real
  `GraphError::NotFound`). The sibling `SoftDelete` test, and the crypto-shred/grace-period-sweep
  interaction test, both pass completely unchanged, confirming `SoftDelete`'s own path was never
  touched. Still not a byte-level deletion from the WAL's history, and still no real
  encryption-at-rest to shred — both remain honestly out of scope, unchanged from before. All
  pre-existing `hyperion-privacy` tests pass unchanged.

- **`hyperion-memory`'s model-estimated salience, landed (2026-07-16)** (this crate's own named
  gap: docs/08 §5.2's `I(r) = max(explicit_flag, model_estimated_salience)` — "`I(r)` here is
  still just the caller-supplied `importance` flag"). A new private `estimate_salience` prompts
  the same wired `ai_runtime` `distill` already uses for a numeric 0.0-1.0 rating and parses the
  response's own text with `str::parse::<f32>`, clamping to range on success — falling back to
  `0.0` (a neutral, no-effect value under `max`, never a fabricated number) when no `ai_runtime`
  is wired, this token isn't authorized, nothing is resident for `ModelClass::Slm`, or the
  response can't be parsed as a real number, matching `distill`'s own graceful-degradation
  contract exactly. `distill_working_memory` — the one real call site with both a real
  `ai_runtime` and a caller-supplied `importance` — now takes `importance.max(estimate_salience(...))`
  per docs/08's own literal formula before persisting, so a real model's own higher-confidence
  rating genuinely reaches `MemoryRecord.importance`/`decay_score`, not just the caller's flag.
  Proven with 3 new tests using a real `NumericRatingBackend` test double (a real
  `InferenceBackend` that answers with a fixed, real, parseable number — `MockBackend`'s own echo
  never parses as one): a real model estimate higher than the explicit flag wins; the explicit
  flag wins when it's the higher of the two; and an unparseable model response (`MockBackend`'s
  own real echo) never fabricates a value, falling back to the explicit flag alone. All 4
  pre-existing `hyperion-memory` `distillation` tests, plus the rest of the crate's suite, pass
  unchanged.

- **`hyperion-privacy`'s soft-delete grace period actually shredding on expiry, landed
  (2026-07-16)** (docs/16 §10's own "soft-deletes honor a grace period before cryptographic
  shredding" — `expire_lapsed_soft_deletes` sealed the `ActionRecord` against `undo` via
  `RecoveryService::expire`, but never called `KnowledgeGraph::delete_node`, so a lapsed
  soft-delete stayed an overwritten-but-still-readable `"Erased"` placeholder forever,
  contradicting this same function's own doc comment's claim of matching `CryptoShred`'s
  irreversibility "from the start" — true only for undo-ability, never for the object's actual
  readability). `expire_lapsed_soft_deletes` now takes `monitor`/`token`/`graph` alongside
  `recovery`, and for every action it really expires, calls the real `delete_node` on each of that
  action's `objects_touched` — the same real primitive gap 33's own `hyperion-recovery::
  apply_snapshot` undo-path already wires to, `GraphError::NotFound` treated as benign the same
  way. The existing `a_soft_delete_past_its_grace_period_is_expired_and_can_never_be_undone_again`
  test's own final assertion (previously checking the placeholder's `"erased": true` field was
  still readable) now asserts the correct, opposite outcome: `graph.get(...)` returns a real
  `GraphError::NotFound`. All other pre-existing `grace_period_expiry` tests — the
  within-grace-period case, the double-sweep case, the `CryptoShred`-has-nothing-to-sweep case,
  and the unrelated-action case — pass with only their call sites updated for the new parameters,
  no behavior change.

- **`hyperion-recovery`'s pinning enforcement via a real compaction pass, landed (2026-07-16)**
  (this crate's own named gap: "`pin`/`unpin` exist; nothing yet reads that flag to protect a
  point from eviction, since this crate has no eviction/compaction pass at all yet — recovery
  points and the action journal simply accumulate for the process lifetime"). New
  `RecoveryService::compact(now, retention_secs) -> usize`, a real caller-driven sweep (matching
  `hyperion-observability`'s own `compact_metrics`/`expire_logs` convention — this crate has no
  background thread of its own to tick on): evicts a `RecoveryPoint` and its snapshot once it
  isn't pinned, its age has reached `retention_secs`, and no still-live `ActionRecord`
  (`InFlight`, needed by `recover_from_crash`; `Committed`, needed by `undo`) references it via
  `recovery_point_before` — `Aborted`/`Undone`/`Expired` actions never read theirs again (`redo`
  restores from its own separate, `ActionId`-keyed `redo_snapshots`, not this one), and a point
  with no `ActionRecord` at all (the direct `restore_to`/`restore_to_with_cause` caller shape) is
  eligible the same way once its window lapses — pinning is exactly how such a caller protects
  one it still needs. Returns the count evicted for a caller to log or audit. Proven with 5 new
  tests in `tests/retention_compaction.rs`: within-window survives regardless of pin state; an
  unpinned, unreferenced point past its window is really removed (`recovery_point` returns
  `None`, not just a count); a pinned point past its window survives; a point backing a
  `Committed` action is never evicted and `undo` still succeeds afterward; a point backing only
  an `Undone` action is evicted past its window. Named simplification: one real, general eviction
  mechanism, not retention *classes* — every point is swept by the same caller-supplied
  `retention_secs`, the same "one policy applies uniformly" shape `hyperion-privacy`'s own
  grace-period sweep already established. All pre-existing `hyperion-recovery` tests, including
  the existing `pin_and_unpin_round_trip` test, pass unchanged.

- **`hyperion-storage`'s version-retention compaction, landed (2026-07-16)** (this crate's own
  named gap: "**Garbage collection / compaction** — nothing here is ever deleted or compacted
  yet"). New `StorageEngine::compact(monitor, token) -> Result<usize, StorageError>` collapses
  every object's version chain unconditionally to its current head, which becomes its own new
  genesis (`parent_version: None`) — a real, WAL-rewriting sweep via a new
  `Wal::compact(path, records) -> Result<Self, StorageError>` (atomic same-filesystem `rename`
  over the old WAL — a crash mid-rewrite leaves either the untouched original or the fully-written
  replacement, never a torn hybrid), not merely an in-memory prune: a real restart must not
  resurrect history compaction already dropped. The WAL rewrite happens *before* the in-memory
  prune, mirroring `put_object`'s own "durable append is the commit point" ordering — if the
  rewrite fails, in-memory state is left exactly as it was, still consistent with the (untouched)
  on-disk WAL. Named simplification: docs/28's own fuller design tiers retention across N
  versions/T days into periodic snapshots; neither `VersionRecord` nor `WalRecord` carries a
  timestamp to key a time-based tier by, so every object collapses to one head uniformly, the
  same "one real, general mechanism, not retention *classes*" shape this session's own
  `hyperion-recovery`/`hyperion-privacy` compaction/expiry sweeps already established. The
  blob-refcount GC, inferred-edge pruning, and ANN index rebuild docs/28 also names remain
  genuinely out of scope — none of those subsystems (blob store, Knowledge Graph inferred edges,
  vector index) exist in this crate. Proven with 5 new tests in
  `tests/retention_compaction.rs`: capability gating; collapsing 3 versions down to 1 head while
  the head stays readable and pruned versions return `NotFound`; **durability across a real
  `StorageEngine::open` reopen** (not just an in-memory check — the crux fact that makes this
  real, extending `wal_recovery.rs`'s own "survives a clean reopen" pattern); idempotence (a
  second compaction evicts 0); and the compare-and-swap invariant still holding correctly after
  compaction (a stale, now-pruned `expected_version` is still rejected as a conflict; the real
  current head still works). All pre-existing `hyperion-storage` tests
  (`capability_gating`/`concurrency`/`wal_recovery`) pass unchanged.

- **`hyperion-model-router`/`hyperion-observability`'s `get_rationale`-by-`invocation_id`, landed
  (2026-07-16)** (both crates' own named gap: docs/23-multi-model-orchestration.md's literal,
  previously-unbuilt `get_rationale(decision_id: InvocationId) -> Rationale`, "consumed by
  [18 — Explainability & Trust]" — `AuditPayload::ModelRouting` carried a `Rationale` with no way
  to look one up by the invocation that produced it, only by `target`/`seq`). `ModelRouting`
  is now a struct variant carrying its own real `invocation_id` alongside the `Rationale`
  (`hyperion-api-gateway`'s `invoke_capability` was computing `decision.invocation_id` and
  discarding it — it's now threaded through to the audit entry); new
  `AuditLedger::rationale_for_invocation(monitor, token, invocation_id) ->
  Result<Option<Rationale>, ObservabilityError>` is built on the ledger's own existing `query`
  rather than a second, separately-maintained index — this ledger is never rolled up or
  truncated, so a parallel `HashMap` would be state to keep consistent forever for no correctness
  a scan doesn't already give at this scale; new `ApiGateway::get_rationale` is docs/23's own
  literal API, the same bridge-method shape `audit_query`/`memory_export` already established.
  Proven with a new test extending `invoke_capability_appends_a_real_model_routing_audit_entry`
  (asserting `get_rationale` resolves the real entry's own `invocation_id` and returns `None` for
  an unrelated one) plus a new
  `get_rationale_disambiguates_two_decisions_sharing_the_same_target` test: two real
  `invoke_capability` calls against the *same* `contract_id` produce two distinct
  `invocation_id`s and two distinct `Rationale`s, both sharing `target == "web.search"` in the
  ledger — proving the lookup genuinely resolves by `invocation_id`, not `target`, which a
  `target`-keyed lookup could never disambiguate. All pre-existing
  `hyperion-model-router`/`hyperion-observability`/`hyperion-api-gateway` tests pass unchanged.

- **`hyperion-api-gateway`'s `cloud_consent` real consent check, landed (2026-07-16)** (this
  crate's own named gap: "`cloud_consent` stays a fixed `true` — deliberately, not by omission,"
  pending a dedicated commit rather than "an incidental side effect of unrelated work," per
  `hyperion-privacy`'s own crate doc). New `router_bridge::build_invocation_with_consent` checks a
  real, live `hyperion-privacy::ConsentLedger` standing grant scoped to the exact capability being
  invoked, never assuming consent — the same `ConsentedCloudUpgrade` check
  `hyperion-scalability::degrade::degrade_capability` already established as this workspace's
  convention for exactly this question. New `ApiGateway::new_with_consent_ledger` wires an
  `Option<Arc<ConsentLedger>>` (this workspace's own established optional-backend shape); plain
  `ApiGateway::new` keeps `consent_ledger: None`, so `cloud_consent` stays the unchanged,
  permissive `true` default for every existing caller. Explicitly *not* the migration
  `hyperion-privacy`'s own doc comment asks not to be done as a side effect —
  `hyperion-model-router`'s own already-shipped, already-tested two-value `PrivacyTier` gate is
  untouched; only the plain `cloud_consent: bool` value fed into it becomes real, supplied from
  this gateway's own new integration seam (already depending on both crates, with no dependency
  cycle — confirmed via every crate's own `Cargo.toml`), exactly the "new integration work should
  depend on `hyperion-privacy`'s types" path that same doc comment invites. Proven with 3 new
  tests in `tests/consent_gated_routing.rs`: without a `ConsentLedger` wired, a `ConsentedCloud`-
  only candidate is still selected (the unchanged default); with one wired and no standing grant,
  the same candidate is genuinely excluded (`invoke_capability` returns
  `ApiError::NoEligibleImplementation`, since it was the only registered candidate); after a real
  `ConsentLedger::request` grant scoped to that exact capability, the candidate becomes eligible
  and dispatch succeeds. All pre-existing `hyperion-api-gateway` tests pass unchanged.

- **`hyperion-knowledge-graph`'s inferred-edge pruning sweep, landed (2026-07-16)** (docs/28
  §"Garbage collection / compaction"'s own named gap for this crate: "inferred edges below a
  confidence threshold... are pruned... explicit edges... are never auto-pruned" — this crate's
  own `effective_edge_weight` was the real, on-demand decay *read*, but nothing ever swept a
  decayed edge into a real tombstone). New `KnowledgeGraph::prune_decayed_edges(monitor, token,
  threshold, now) -> Result<Vec<EdgeId>, GraphError>`: every non-tombstoned `Inferred` edge whose
  current `effective_edge_weight` has fallen below `threshold` is tombstoned for real via the
  existing `unlink` (the same real, WAL-backed, undoable-within-a-retention-window tombstone
  every other deletion in this crate already uses) — an `Explicit` edge is never even considered,
  regardless of `threshold` or `now`, matching docs/28's own explicit carve-out. Named
  simplification: `EdgeRecord` has no separate provenance-TTL field distinct from
  `last_confirmed_at`'s own tau-decay, so the confidence-threshold check alone is this sweep's
  one real mechanism — the same "one real, general mechanism, not two separate ones" shape this
  session's other compaction sweeps (`hyperion-recovery`/`hyperion-storage`) already established.
  Proven with 5 new tests in `tests/edge_pruning.rs`: a deeply decayed `Inferred` edge is
  tombstoned and never resurfaced by `dump`; a freshly confirmed one survives untouched; an
  `Explicit` edge is never pruned even under an extreme threshold and far-future `now`;
  idempotence (a second pass over an already-pruned edge evicts nothing new); and durability
  across a real `KnowledgeGraph::open` reopen (the prune survives replay, not just the live
  in-memory index). All pre-existing `hyperion-knowledge-graph` tests, including the existing
  `edge_decay.rs` suite, pass unchanged.

- **`hyperion-explainability`'s `ConfidenceMethod::SelfConsistency`, landed (2026-07-16)** (this
  crate's own named gap: "`ConfidenceScore.method` implementations" — `SelfConsistency`/
  `Verifier`/`Ensemble` were declared but none were ever computed). New
  `self_consistency_confidence(ai_runtime, monitor, token, prompt, samples) ->
  Option<ConfidenceScore>` is docs/18 §9's own "self-consistency across repeated sampling": calls
  a real, wired `hyperion-ai-runtime::LocalAiRuntime` with the identical `prompt` `samples` real
  times and reports the real majority-answer agreement fraction as the confidence value —
  `None` (never a fabricated score) if this token isn't authorized, nothing is resident for
  `ModelClass::Slm`, or any one of the `samples` real calls fails, matching every other
  `ai_runtime`-backed method's graceful-degradation contract in this workspace
  (`hyperion-context::ContextEngine::summarize` is the template this mirrors). A free function
  exported directly at crate level (matching `resolve_why`'s own precedent), not a stateful
  `ExplanationStore` field — the store itself gains no new `ai_runtime` dependency, since
  `set_confidence` already accepts any real `ConfidenceScore` a caller computed. `Verifier`/
  `Ensemble` remain declared but uncomputed, exactly as before: `Verifier` needs real formal
  verification this workspace doesn't have, and `Ensemble` needs
  [23 — Multi-Model Orchestration](../23-multi-model-orchestration.md)'s actual candidate
  models — neither is what this function computes. Proven with 4 new inline unit tests in
  `src/confidence.rs` (using a real, deterministically-cycling test `InferenceBackend` —
  `MockBackend`'s own echo is identical every call for a fixed prompt, which can only prove the
  always-agrees case): unanimous real answers yield full confidence; a real 3-of-4 partial
  disagreement yields exactly `0.75`, not a fabricated value; no resident model degrades to
  `None`; zero `samples` is never a real computation. A new integration test,
  `tests/self_consistency_confidence.rs`, proves the real score genuinely flows into a real
  `ExplanationStore` record via `set_confidence` and back out through `resolve_why`, not just the
  pure function in isolation. All pre-existing `hyperion-explainability` tests pass unchanged.

- **`hyperion-knowledge-graph`'s owner-based ACL on single-object accessors, landed
  (2026-07-16)** — a real, confirmed contradiction between this crate's own doc comments: its
  `traverse` doc comment already claimed `get` gives "the same 'never reveal existence of what
  you can't see' shape" `query`/`traverse`/`dump` enforce via an `owner == caller_boundary`
  check — but `get` never actually checked `owner` at all, only rights and tombstone status. The
  same missing check existed on `get_at_version`, `delete_node`, `unlink`, and `explain` (the
  last of which returns `owner`/`provenance` verbatim, directly leaking cross-boundary metadata).
  All five now real-check `owner`, returning `GraphError::NotFound` (never `Unauthorized`, which
  would leak that the object exists under a different boundary) — `delete_node`/`unlink` check
  ownership *before* the tombstone short-circuit, so a foreign-boundary caller can't even learn
  whether the object is already deleted. `prune_decayed_edges` (landed earlier this session) is
  updated to scope its own sweep to the caller's Trust Boundary too — otherwise it would now
  hard-fail partway through a sweep the moment it hit a foreign-owned decayed edge, since it
  calls `unlink` internally.

  A related, more severe bug surfaced during this fix: `put_node`'s update path unconditionally
  set `owner: token.origin().0` on every write, silently reassigning a foreign-boundary node to
  whichever caller last updated it — and, critically, making every owner check just added
  trivially bypassable (steal ownership via `put_node`, then freely `get`/`delete_node` as the
  new "owner"). `put_node` now rejects an update to a node the caller doesn't already own
  (`NotFound`) and preserves `existing.owner` verbatim on a legitimate update, mirroring `link`'s
  own edge-update path, which never had this bug (edges already preserved their owner across an
  update).

  One real regression surfaced by this fix and corrected: `hyperion-coordination`'s
  `create_session_and_allocate_require_write_rights` test derived its own `read_only` token with
  a *different* `TrustBoundaryId` than the root token that created the intent node it went on to
  read — incidental to what the test actually meant to exercise (insufficient rights, not
  boundary-crossing), and only ever worked because `get`/`get_graph` had no owner check before
  this fix. Corrected to derive `read_only` from the same boundary, matching how every other
  rights-only gating test in this workspace is authored.

  Proven with 7 new tests in `tests/single_object_owner_acl.rs`: `get`/`get_at_version` never
  return a different boundary's node; `delete_node`/`unlink` never delete a different boundary's
  object and never actually tombstone it (verified via a subsequent same-owner read/traverse
  still succeeding); `explain` never leaks a different boundary's node or edge provenance;
  `put_node` can never steal a different boundary's node (the original owner's metadata is
  verified untouched); `prune_decayed_edges` only ever prunes the caller's own edges, never
  erroring out over another boundary's decayed edge. All pre-existing
  `hyperion-knowledge-graph` tests pass unchanged, and the one genuine regression
  (`hyperion-coordination`) was fixed rather than worked around.
