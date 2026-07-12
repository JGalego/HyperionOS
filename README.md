# Hyperion

> Humans express goals. Hyperion determines how those goals become reality.

Hyperion is an intent-native operating system: instead of applications, files, and windows, you
describe what you want and Hyperion figures out how to make it happen. See
[CLAUDE.md](CLAUDE.md) for the full project philosophy and
[PRODUCTION_BOOT_PROMPT.md](PRODUCTION_BOOT_PROMPT.md) for the from-source build/boot roadmap and
its current status.

## Get Hyperion

Every tagged release publishes ready-to-flash disk images for both reference platforms on the
[Releases page](https://github.com/JGalego/HyperionOS/releases):

| Platform | Download | Boots via |
|---|---|---|
| x86_64 | `hyperion-x86_64-<version>.img` | UEFI (GPT disk image, EFI System Partition + GRUB2) |
| aarch64 | `hyperion-aarch64-<version>-Image` + `hyperion-aarch64-<version>-rootfs.ext2` | direct kernel boot |

For most people **x86_64 is the one to grab** — it's a single, complete disk image.

### Flashing the x86_64 image to a USB drive

1. Download `hyperion-x86_64-<version>.img` from the latest release.
2. Install [balenaEtcher](https://etcher.balena.io/) (Windows/macOS/Linux).
3. Open Etcher, select the downloaded `.img` file, select your USB drive, and flash. Etcher writes
   the raw image directly and verifies the write afterward — no need to unpack or convert anything.
4. Boot the target machine from the USB drive (usually a one-time boot-menu key like F12/F10/Esc
   at power-on) and select it.

Double-check the drive you select in Etcher — flashing overwrites everything on it.

### Verifying a downloaded image

Every image ships alongside a `.release.json` manifest: a BLAKE3 hash of the image's own bytes and
an Ed25519 signature over that hash, signed with Hyperion's release-signing key. Check a download
against this project's real, published verifying key before trusting it:

```
b5c19b1e890fed3e164342f0285f6a1a1635d724f2284a2ebe00589a122ac90a
```

To verify (needs a Rust toolchain and this repo checked out):

```sh
cargo run --release -p hyperion-release-gate --bin verify-release -- \
  hyperion-x86_64-<version>.img hyperion-x86_64-<version>.img.release.json
```

This recomputes the image's hash directly from its bytes (never trusts the manifest's own
recorded hash) and confirms the signature verifies against the manifest's recorded verifying key —
compare that key against the one published above.

## Building from source

See [PRODUCTION_BOOT_PROMPT.md](PRODUCTION_BOOT_PROMPT.md) and the scripts under
[boot/scripts/](boot/scripts/) (`build-image.sh` for x86_64, `build-image-aarch64.sh` for
aarch64) to build an image yourself instead of downloading one.
