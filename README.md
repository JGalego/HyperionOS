<div align="center">

<img src="assets/banner.svg" alt="Hyperion -- the first intent-native operating system." width="100%" />

[![CI](https://github.com/JGalego/HyperionOS/actions/workflows/ci.yml/badge.svg)](https://github.com/JGalego/HyperionOS/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/JGalego/HyperionOS?style=flat-square&color=d9a54a&label=release)](https://github.com/JGalego/HyperionOS/releases) [![License](https://img.shields.io/github/license/JGalego/HyperionOS?style=flat-square&color=8c6220)](LICENSE) [![Platforms](https://img.shields.io/badge/platforms-x86__64%20%7C%20aarch64-e6bb6e?style=flat-square)](PRODUCTION_BOOT_PROMPT.md)

</div>

Hyperion is an intent-native operating system: humans express goals, and the system determines how those goals become reality.

Want the thinking behind it? Read [`CLAUDE.md`](CLAUDE.md).

Want to know exactly what's built and what's next? See [`PRODUCTION_BOOT_PROMPT.md`](PRODUCTION_BOOT_PROMPT.md).

## Philosophy

> Your goals are the operating system.

Every prior operating system asked you to manage proxies for what you wanted: a process, a file, a window, an application. 

Hyperion manages what you actually have.

| Old OSes | Hyperion |
|---|---|
| Processes | Goals |
| Threads | Intentions |
| Files | Knowledge |
| Windows | Context |
| Applications | Memory, reasoning, capabilities |

## Architecture

> You speak. Everything else listens.

Hyperion is built in layers, each one building on the one below it.

Closer to the hardware, it's fast and safe.

Closer to you, it understands what you mean.

<details>
<summary><code>L0</code> <strong>Kernel</strong> — where it begins</summary><br>

The hardware layer everything else stands on. It's open source, so its safety claims are checkable. Capability-secured from the ground up: nothing crosses a trust boundary without an explicit, revocable grant.
</details>

<details>
<summary><code>L1</code> <strong>System runtime</strong> — how it stays fast and safe</summary><br>

Schedules work across CPU, GPU, memory, and battery, keeping every process in its own boundary. A unified scheduler balances compute, inference tokens, and context windows, the way earlier operating systems balanced CPU and RAM.
</details>

<details>
<summary><code>L2</code> <strong>Platform services</strong> — what it's built from</summary><br>

Reusable capabilities, storage, updates, and networking. Capabilities are Hyperion's replacement for the application: a declared contract, a trust level, and one or more interchangeable implementations.
</details>

<details>
<summary><code>L3</code> <strong>Knowledge</strong> — what it knows and remembers</summary><br>

Your information, connected by meaning. Every document, photo, message, or project is a Semantic Object with typed relationships to everything else: a knowledge graph standing in for folders and filenames.
</details>

<details>
<summary><code>L4</code> <strong>Cognition</strong> — where it thinks</summary><br>

Understands what you're asking for, recalls what's relevant, and picks the right model for the job. The Intent Engine turns language into a graph of sub-goals. The Context Engine attaches what already exists (a calendar event, a past conversation). The Model Router picks local or cloud reasoning per task.
</details>

<details>
<summary><code>L5</code> <strong>Coordination</strong> — how the work gets organized</summary><br>

When a goal needs more than one specialist, this layer decides who does what, and keeps a record of why. Multi-agent orchestration assigns sub-goals to specialized agents and resolves conflicts between them. Every decision is logged for you to inspect.
</details>

<details>
<summary><code>L6</code> <strong>Experience</strong> — what you see and say</summary><br>

Conversation, generated screens, and voice: the only parts of Hyperion you directly touch. The Dynamic UI Runtime assembles a Workspace on demand for whatever you're doing, then tears it down when you're done. Accessibility is built into that runtime.
</details>

#### Example: `"I need to launch my startup."`

- <code>L6</code> The console captures it.
- <code>L4</code> The Intent Engine splits it into sub-goals — market research, a business model, branding, legal formation.
- <code>L5</code> Coordination assigns each sub-goal to a specialized agent.
- <code>L4</code> Each agent gets its answer by invoking a capability, routed to whichever model fits.
- <code>L3</code> Everything learned along the way is written into the knowledge graph, connected to everything else you know.
- <code>L2</code>/<code>L1</code>/<code>L0</code> Every one of those steps was checked against a capability grant first, and scheduled safely underneath.
- <code>L6</code> The results land back as one workspace — not four separate app windows.

## Getting started

Every tagged release publishes ready-to-flash disk images for both reference platforms on the [Releases](https://github.com/JGalego/HyperionOS/releases) page:

| Platform | Download | Boots via |
|---|---|---|
| x86_64 | `hyperion-x86_64-<version>.img` | UEFI (GPT disk image, EFI System Partition + GRUB2) |
| aarch64 | `hyperion-aarch64-<version>-Image` + `hyperion-aarch64-<version>-rootfs.ext2` | direct kernel boot |

For most people **x86_64 is the one to grab** - it's a single, complete disk image.

### Put it on a USB drive

1. Download `hyperion-x86_64-<version>.img` from the [latest release](https://github.com/JGalego/HyperionOS/releases/latest).
2. Install [balenaEtcher](https://etcher.balena.io/) (Windows/macOS/Linux).
3. Open Etcher, select the downloaded `.img` file, select your USB drive, and flash. Etcher writes the raw image directly and verifies the write afterward - no need to unpack or convert anything.
4. Boot the target machine from the USB drive (usually a one-time boot-menu key like F12/F10/Esc at power-on) and select it.

Double-check the drive you select in Etcher - flashing overwrites everything on it.

### Check what you downloaded

Every image ships with proof that it's untampered and really came from this project: a `.release.json` manifest holding a BLAKE3 hash of the image and an Ed25519 signature over that hash, signed with Hyperion's release key. Check it against the real, published verifying key before you trust it:

```
b5c19b1e890fed3e164342f0285f6a1a1635d724f2284a2ebe00589a122ac90a
```

To verify (needs a Rust toolchain and this repo checked out):

```sh
cargo run --release -p hyperion-release-gate --bin verify-release -- \
  hyperion-x86_64-<version>.img hyperion-x86_64-<version>.img.release.json
```

This recomputes the hash directly from the image's own bytes (it never trusts the manifest's own recorded hash) and confirms the signature checks out against the manifest's recorded key - compare that key against the one published above.

## Build it yourself

See [PRODUCTION_BOOT_PROMPT.md](PRODUCTION_BOOT_PROMPT.md) and the scripts under [boot/scripts/](boot/scripts/) (`build-image.sh` for x86_64, `build-image-aarch64.sh` for aarch64) if you'd rather build an image from source than download one.

## License

MIT - See [LICENSE](LICENSE)
