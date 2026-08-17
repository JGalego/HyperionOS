#!/usr/bin/env bash
# Builds the hyperion-node container image.
#
# The binary is cross-compiled on the host rather than inside a builder stage: `hyperion-console`
# already targets static musl for the boot image, so the same target gives a runtime image with no
# distro in it at all -- `FROM scratch`, ~14 MB, nothing to patch. A Rust builder stage would add
# a couple of gigabytes of toolchain to download and cache for no gain.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOCKER_DIR="$REPO_ROOT/docker"
IMAGE="${HYPERION_IMAGE:-hyperion-node:dev}"
TARGET=x86_64-unknown-linux-musl

if ! rustup target list --installed | grep -q "$TARGET"; then
    echo "Installing the $TARGET Rust target..."
    rustup target add "$TARGET"
fi

# `ring` (via rustls, which the cloud backends need) compiles C, so a musl C compiler has to exist.
# Without this the build fails deep inside a build script with `ToolNotFound`, which doesn't say
# what to install.
if ! command -v musl-gcc >/dev/null 2>&1; then
    echo "error: musl-gcc not found -- the TLS stack compiles C against musl." >&2
    echo "  Debian/Ubuntu:  sudo apt-get install musl-tools" >&2
    echo "  Fedora:         sudo dnf install musl-gcc" >&2
    echo "  macOS:          brew install FiloSottile/musl-cross/musl-cross" >&2
    exit 1
fi

echo "Cross-compiling hyperion-console ($TARGET, static)..."
CC_x86_64_unknown_linux_musl=musl-gcc cargo build --release --locked \
    --manifest-path "$REPO_ROOT/Cargo.toml" \
    --target "$TARGET" -p hyperion-console --bin hyperion-console \
    --features real-http,openai-compat,anthropic,gemini,mdns

cp "$REPO_ROOT/target/$TARGET/release/hyperion-console" "$DOCKER_DIR/hyperion-console"

# Only used when a node talks to a cloud provider; the mesh itself is plain HTTP between peers.
for bundle in /etc/ssl/certs/ca-certificates.crt /etc/pki/tls/certs/ca-bundle.crt \
              /etc/ssl/cert.pem; do
    if [[ -f "$bundle" ]]; then cp "$bundle" "$DOCKER_DIR/ca-certificates.crt"; break; fi
done
if [[ ! -f "$DOCKER_DIR/ca-certificates.crt" ]]; then
    echo "warning: no system CA bundle found; cloud backends won't work in the image." >&2
    : > "$DOCKER_DIR/ca-certificates.crt"
fi

docker build -t "$IMAGE" "$DOCKER_DIR"
echo
docker images "$IMAGE" --format 'built {{.Repository}}:{{.Tag}}  {{.Size}}'
