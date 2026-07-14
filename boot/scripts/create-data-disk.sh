#!/usr/bin/env bash
# docs/998-roadmap.md M6: creates a real, dedicated, pre-formatted ext4 data disk image --
# a real block device from the guest's point of view (a second virtio-blk drive), distinct from
# the boot disk, for hyperion-storage's WAL to live on instead of a host tempfile. Pre-formatted
# at build time by this script (which runs on the host, where mkfs.ext4 is available) rather than
# by the guest at boot: this minimal Buildroot rootfs has no e2fsprogs to format one at runtime,
# and shouldn't need one -- a fresh data volume is provisioned once, not reformatted on every boot.
#
# Usage: create-data-disk.sh <output-path> [size]
set -euo pipefail

OUT="${1:?usage: create-data-disk.sh <output-path> [size]}"
SIZE="${2:-64M}"

truncate -s "$SIZE" "$OUT"
mkfs.ext4 -q -F "$OUT"

echo "Created real, pre-formatted ext4 data disk: $OUT ($SIZE)"
