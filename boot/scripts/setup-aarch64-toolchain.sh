#!/usr/bin/env bash
# Provisions an aarch64-linux-gnu gcc/binutils cross-toolchain without root.
#
# PRODUCTION_BOOT_PROMPT.md M11 (second reference platform, aarch64) needs this purely as a
# *linker driver*: this workspace cross-compiles hyperion-init/hyperion-console for
# aarch64-unknown-linux-musl, and the host's own native `cc` can't link aarch64 ELF objects
# ("Relocations in generic ELF (EM: 183)"). None of the linked object files need glibc itself --
# both the Rust code and the CRT objects come from rustup's own musl sysroot -- so a glibc-targeted
# cross-gcc is fine here even though the *target* is musl: `aarch64-linux-gnu-gcc` just needs to
# correctly invoke its bundled `aarch64-linux-gnu-ld`, which is architecture-specific but
# libc-agnostic. See .cargo/config.toml's own comment for the full reasoning.
#
# Same rootless technique as setup-qemu-toolchain.sh: this dev environment has no passwordless
# sudo, so `apt-get download` (fetches the .deb into a scratch dir, no privilege needed) +
# `dpkg -x` (extracts a .deb's contents anywhere, doesn't touch the system package database)
# stand in for `apt-get install`. Safe to re-run: skips work if already provisioned.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
PREFIX="$BOOT_DIR/.tools/aarch64-cross-root"
STAMP="$PREFIX/.provisioned"

if [[ -f "$STAMP" ]]; then
    echo "aarch64 cross-toolchain already provisioned at $PREFIX"
    exit 0
fi

# The full real dependency closure of gcc-aarch64-linux-gnu on this host's exact Debian release
# (confirmed via `apt-get install --no-install-recommends -s gcc-aarch64-linux-gnu`), not guessed:
# apt's own dependency resolution, just executed without ever touching the system dpkg database.
PACKAGES=(
    binutils-aarch64-linux-gnu
    gcc-12-aarch64-linux-gnu-base
    cpp-12-aarch64-linux-gnu
    cpp-aarch64-linux-gnu
    gcc-12-cross-base
    libc6-arm64-cross
    libgcc-s1-arm64-cross
    libgomp1-arm64-cross
    libitm1-arm64-cross
    libatomic1-arm64-cross
    libasan8-arm64-cross
    liblsan0-arm64-cross
    libtsan2-arm64-cross
    libstdc++6-arm64-cross
    libubsan1-arm64-cross
    libhwasan0-arm64-cross
    libgcc-12-dev-arm64-cross
    gcc-12-aarch64-linux-gnu
    gcc-aarch64-linux-gnu
)

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "Downloading ${#PACKAGES[@]} packages via apt-get download (no root required)..."
(cd "$WORK" && apt-get download "${PACKAGES[@]}")

mkdir -p "$PREFIX"
for deb in "$WORK"/*.deb; do
    dpkg -x "$deb" "$PREFIX"
done

# `dpkg -x` extracts a .deb's own tarball contents only -- it never runs a package's maintainer
# scripts (preinst/postinst/etc). On Ubuntu 24.04's real GitHub Actions runners, the package
# itself ships `usr/bin/aarch64-linux-gnu-gcc` as a symlink pointing at
# `/etc/alternatives/aarch64-linux-gnu-gcc` -- a path `update-alternatives` (run from postinst,
# which `dpkg -x` never executes) would normally populate. That leaves a real but *dangling*
# symlink at the unversioned path: `[[ -e ]]` correctly reports it as broken (confirmed directly
# against a real failed CI run: `-e` was false, so the check below did fire, but a plain `ln -s`
# then refused to overwrite the dangling symlink already occupying that path, failing with "File
# exists"). Debian 12 bookworm's own package doesn't have this problem (this same rootless
# extraction already works there unmodified) -- exact cause unconfirmed there, but irrelevant:
# `ln -sf` (removes whatever's at the destination first) handles a missing name, an already-correct
# symlink, and a dangling one identically, so it's the right fix regardless of which case a given
# distro's packaging produces.
if [[ ! -e "$PREFIX/usr/bin/aarch64-linux-gnu-gcc" ]]; then
    # The `[0-9]*` suffix (not a bare `*`) deliberately excludes sibling tools that share the
    # `aarch64-linux-gnu-gcc-` prefix but aren't version-numbered gcc itself, e.g.
    # `aarch64-linux-gnu-gcc-ar-12`/`-nm-12`/`-ranlib-12`.
    versioned_gcc="$(find "$PREFIX/usr/bin" -maxdepth 1 -name 'aarch64-linux-gnu-gcc-[0-9]*' | sort -V | tail -1)"
    if [[ -z "$versioned_gcc" ]]; then
        echo "no aarch64-linux-gnu-gcc (versioned or not) found under $PREFIX/usr/bin after extraction" >&2
        exit 1
    fi
    echo "no working unversioned aarch64-linux-gnu-gcc shipped by this distro's packages -- linking $(basename "$versioned_gcc") to it"
    ln -sf "$(basename "$versioned_gcc")" "$PREFIX/usr/bin/aarch64-linux-gnu-gcc"
fi

touch "$STAMP"
echo "Provisioned aarch64 cross-toolchain at $PREFIX"

export PATH="$PREFIX/usr/bin:$PATH"
aarch64-linux-gnu-gcc --version
