# Hyperion boot image

Builds the Linux-hosted MVP image described in
[PRODUCTION_BOOT_PROMPT.md](../PRODUCTION_BOOT_PROMPT.md) — see that document for the full
roadmap and the decision record (§0) for why this is a Linux-hosted MVP rather than the
from-scratch hybrid microkernel [docs/03](../docs/03-kernel-architecture.md) specifies.

## Layout

- `board/hyperion-x86_64/` — kernel config, GRUB config, and genimage/post-build scripts for the
  Hyperion board, modeled on Buildroot's own real-hardware `board/pc` EFI target (not the separate
  QEMU-only demo board) so the same image boots both in QEMU and on real x86_64 UEFI hardware.
- `configs/hyperion_x86_64_efi_defconfig` — the Buildroot defconfig for that board.
- `scripts/` — the build pipeline (see below). Every script is idempotent and safe to re-run.
- `.tools/` (gitignored) — Buildroot itself, downloads, and build output. Multi-GB; reproducibly
  re-derived by `scripts/fetch-buildroot.sh`, never committed.

## Building and testing

```sh
boot/scripts/build-image.sh   # fetches Buildroot if needed, builds output/images/disk.img
boot/scripts/boot-test.sh     # headless QEMU boot, asserts a login prompt appears (CI gate)
boot/scripts/run-qemu.sh      # interactive QEMU boot for a real dev loop
```

`build-image.sh` and `boot-test.sh` need no root and no `/dev/kvm` access: if a system
`qemu-system-x86_64` + OVMF aren't already installed (as they would be in CI via
`apt-get install qemu-system-x86 ovmf`), `run-qemu.sh`/`boot-test.sh` fall back to a rootless
extraction produced by `scripts/setup-qemu-toolchain.sh` (`apt-get download` + `dpkg -x`, no
`apt-get install` required). Without `/dev/kvm`, QEMU runs under TCG software emulation instead of
KVM acceleration — slower per boot, not a correctness gap in anything being tested.

## Writing to a real USB drive

**This is destructive and irreversible: it erases everything on the target device.** Confirm
`/dev/sdX` is actually the USB drive and not a disk holding data you care about (check
`lsblk`/`dmesg` after plugging it in) before running this:

```sh
dd if=boot/.tools/buildroot-2026.05/output/images/disk.img of=/dev/sdX bs=4M status=progress conv=fsync
sync
```

Never run this against a path you haven't just personally verified is the USB drive.
