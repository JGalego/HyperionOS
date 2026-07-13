# Hyperion Production Boot Roadmap

**Purpose of this document:** a self-contained prompt for driving Hyperion from its current
state — 31 Rust crates implementing Phases 2-10's *logic* as an in-process, `std`, hosted
simulator with no real kernel, hardware, or boot path — to a real image that boots from a USB
drive on real hardware and runs that logic for real. Paste this whole document as the opening
message of a session (or a `/loop`) to resume this work; the **Status** section below is the
living checklist — update it as milestones close, the same way `MEMORY.md`'s project notes track
completed phases.

## 0a. Execution Mode — read this too

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

## 0. Decision Record — read this before anything else

**Decision (made 2026-07-11, by the project owner):** target a **Linux-hosted MVP**, not the
from-scratch hybrid microkernel [docs/03-kernel-architecture.md](docs/03-kernel-architecture.md)
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

## 1. Current State (verified 2026-07-11)

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

## 2. Assumption to confirm or correct

**Reference hardware for the first bootable milestone: x86_64, UEFI firmware.** This is the
pragmatic choice for "boots from a USB drive" specifically — UEFI+x86_64 has the most mature
USB-boot tooling, the best QEMU/OVMF emulation story for fast iteration, and the broadest
real-hardware compatibility of any target. docs/41 Phase 1's entry criterion asks for two
reference platforms (an SBC and a workstation-class box); this roadmap treats a second platform
(e.g. Raspberry Pi 4/5, aarch64) as Milestone 11, after the x86_64 MVP is solid — do not attempt
both platforms simultaneously. If the actual target hardware is different (a specific SBC, a
specific enterprise server), say so before starting Milestone 1; the bootloader and driver choices
below assume UEFI.

## 3. Status

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
milestone's signature fix does not touch; (3) `hyperion-observability`'s periodic signed
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
   [README.md](README.md) as the independent, out-of-band value a downloader checks a manifest's
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

## 4. Milestones

Each milestone below states what it delivers, what from the existing 31 crates is genuinely
reusable (the algorithms, not the process model), what's net-new, and its exit criteria — mirroring
docs/41's own phase-definition shape so this roadmap reads as a continuation of that document, not
a break from its conventions.

### M0 — Toolchain, Decision Record, QEMU Harness

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

### M1 — Bootable "Hello Hyperion" Image

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

### M2 — Real Capability / Trust Boundary Enforcement

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

### M3 — Real IPC Transport

**Delivers:** `hyperion-ipc`'s frame/channel model (Request/Response/Notification, `ipc_call`/
`ipc_notify`) carried over a **real transport** — Unix domain sockets for the MVP (io_uring-based
batching is a real, valuable follow-on per docs/30, not required to prove the transport is real).
**Reuse:** the frame types and call/notify semantics as-is; only the transport underneath changes.
**Exit criteria:** two real, separate Linux processes (started under M2's enforcement) exchange a
real IPC call/notify frame across a real socket; a call from a process whose capability was
revoked is rejected at the transport boundary, not just by a library-level check the process could
route around if it weren't actually sandboxed.

### M4 — Real Scheduler Enforcement

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

### M5 — Real Init & Supervision Tree

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

### M6 — Real Persistent Storage

**Delivers:** `hyperion-storage`'s WAL-backed object store pointed at a **real, dedicated block
device or partition** (an attached NVMe/SSD for actual daily-driver persistence — writing heavily
to the boot USB itself is both slow and bad for the drive's lifespan) instead of a file on the
host's existing filesystem in a temp directory, as every current test does.
**Exit criteria:** the existing crash-consistency-by-replay guarantee is re-validated against a
real power-loss simulation (e.g. `qemu`'s ability to hard-kill the VM mid-write) on the real block
device, not just a simulated partial-write test against a host tempfile.

### M7 — Real Console UI, Then Real Display

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

### M8 — Real Local AI Runtime

**Delivers:** `hyperion-ai-runtime`'s mock model execution replaced with a real on-device inference
engine — **Candle** (Rust-native) is the natural fit for this codebase's own Rust-first convention
and avoids an FFI boundary to a C++ engine; `llama.cpp` bindings are the fallback if Candle's model
support gap is a blocker for a specific desired model. A real small resident model must run within
docs/36's latency budget on the reference hardware.
**Exit criteria:** `hyperion-intent`'s decomposition and `hyperion-model-router`'s routing produce
real output driven by a real model's real inference, on the booted image, not the deterministic
mock backend every current test uses.

### M9 — Real Cryptography

**Delivers:** every "non-cryptographic checksum stand-in" this workspace uses as a deliberate,
documented placeholder — `hyperion-ai-runtime::checksum`, `hyperion-plugin-framework::signature`,
`hyperion-security`'s model-integrity check, `hyperion-update::signature`,
`hyperion-observability`'s hash-chain — replaced with real primitives (ed25519 or RSA signing via
a real Rust crypto crate; real SHA-256/BLAKE3 hashing) and a real key-management story (a software
keystore at minimum; TPM-backed sealing as a stretch goal where the reference hardware has a TPM).
**Exit criteria:** a tampered plugin manifest/update package/audit-ledger entry is rejected by a
real signature or hash-chain check, not a checksum a forger could trivially reproduce.

### M10 — Real Networking

**Delivers:** `hyperion-netstack`'s `MockFetchBackend`/`MockExtractionBackend` replaced with a real
HTTP client (`reqwest`/`hyper`) over the booted machine's real NIC, real DNS, real TLS.
**Exit criteria:** `web.research`/`web.fetch.raw` fetch a real URL over the real network from the
booted image and merge a real extracted entity into the real Knowledge Graph.

### M11 — Second Reference Platform

**Delivers:** bring-up on a second, lower-tier reference platform (Raspberry Pi 4/5, aarch64) to
satisfy docs/41 Phase 1's literal two-platform exit criterion — re-run M0-M4 for this target
(Buildroot supports aarch64; note that Raspberry Pi's boot path is SD-card/firmware-first, not
generic UEFI-USB, so M1's "boot from USB drive" claim is inherently x86_64-UEFI-specific and this
platform validates the *rest* of the stack, not an additional USB-boot claim).
**Exit criteria:** the same Hyperion image (kernel config and Buildroot target adjusted for
aarch64) boots to the M7-stage-1 console loop on real Raspberry Pi hardware.

### M12 — Boot Benchmarking Against docs/36

**Delivers:** real cold-boot timing on real hardware (both reference platforms), measured
end-to-end (firmware → login/shell → first real Intent handled), against docs/36's full boot
budget — not `hyperion_sim::boot`'s in-process "privileged-core init" 250ms slice, which only ever
measured one sub-phase of a boot that didn't yet exist.
**Exit criteria:** real, measured cold-boot time is reported against the docs/36 budget on both
platforms; if over budget, the gap and its cause are named explicitly (kernel init time, initramfs
size, service startup ordering, model load time) rather than the milestone being closed on
optimism.

### M13 — Release Engineering for a Bootable Artifact

**Delivers:** extend the existing `hyperion-release-gate` crate's criteria to cover the new
hardware/boot surface: image build reproducibility, boot-tested on both reference platforms per
M11/M12, a staged update (`hyperion-update`) applied to a real running booted system and rolled
back without data loss (docs/41 Phase 10's literal exit criterion, finally tested against a real
system instead of an in-process orchestrator), and a signed (M9), versioned, `dd`-able USB image
published as the actual release artifact.
**Exit criteria:** a fresh USB drive, written from a tagged release image, boots on both reference
platforms and passes a smoke test exercising M7 stage 1's real Intent→Agent→output loop.

## 5. What changes about "one commit per item"

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

## 6. Reuse map (what NOT to rewrite)

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

## 7. Explicit Non-Goals for This Roadmap

Named here so no future session assumes silent scope creep:
- Formal verification of the enforcement layer (docs/03's seL4-class assurance target) — not
  attempted; the Linux-hosted enforcement in M2 is real but not formally proven.
- A from-scratch hybrid microkernel — explicitly deferred per §0's decision record, not abandoned.
- Real hardware virtualization/VM Trust-Depth-3 sandboxing for foreign-kernel guests
  ([27 — Compatibility Layer](docs/27-compatibility-layer.md)'s Windows path) — out of scope until
  well after M13.
- GPU driver work beyond basic KMS/DRM framebuffer output — a real GPU compute/NPU driver story is
  a separate, later project, not part of M7's display milestone.
- Multi-device federation over a real network ([21 — Distributed Execution](docs/21-distributed-execution.md))
  — M10 gets one device onto a real network; federating two *real, separately booted* Hyperion
  machines is a follow-on roadmap, not part of this one.
