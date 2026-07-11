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
| M1 — Bootable "Hello Hyperion" image (the literal ask) | pending |
| M2 — Real capability/Trust-Boundary enforcement | pending |
| M3 — Real IPC transport | pending |
| M4 — Real scheduler enforcement (cgroups v2) | pending |
| M5 — Real init & supervision tree | pending |
| M6 — Real persistent storage | pending |
| M7 — Real console UI, then real display | pending |
| M8 — Real local AI runtime | pending |
| M9 — Real cryptography | pending |
| M10 — Real networking | pending |
| M11 — Second reference platform (aarch64) | pending |
| M12 — Boot benchmarking against docs/36 | pending |
| M13 — Release engineering for a bootable artifact | pending |

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
