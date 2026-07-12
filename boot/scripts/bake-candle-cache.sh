#!/usr/bin/env bash
# Bakes a real, pinned-commit Candle model + tokenizer (see
# crates/hyperion-ai-runtime/src/candle_backend.rs's own TINYLLAMAS_REVISION/
# LLAMA_TOKENIZER_REVISION doc comments) plus a real CA certificate bundle into the x86_64
# Buildroot rootfs overlay, so a `--features candle` image can run real inference with zero
# network access at boot.
#
# Both pieces are needed, not just the model: `hf-hub`'s own HTTP client
# (`rustls-platform-verifier`) builds a real TLS trust store *unconditionally at client
# construction time*, before any on-disk cache is ever consulted -- an empty trust store (this
# rootfs ships no `ca-certificates` package) makes client construction itself fail, independent of
# whether the model is already cached. See crates/hyperion-init/src/linux.rs's own HF_CACHE_DIR/
# CA_BUNDLE_PATH doc comments for the runtime side of this (which env vars point the real console
# at these baked files).
#
# Usage: bake-candle-cache.sh
#   Run after build-image.sh's own Buildroot fetch (so $BUILDROOT_DIR exists), before building
#   hyperion-console with --features candle and copying it into the same rootfs overlay. Safe to
#   re-run.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# shellcheck source=./fetch-buildroot.sh
source "$SCRIPT_DIR/fetch-buildroot.sh"

OVERLAY_DIR="$BUILDROOT_DIR/board/hyperion-x86_64/rootfs-overlay"
HF_CACHE_DIR="$OVERLAY_DIR/usr/share/hyperion/hf-cache/hub"
TLS_DIR="$OVERLAY_DIR/usr/share/hyperion/tls"

TINYLLAMAS_REVISION="0bd21da7698eaf29a0d7de3992de8a46ef624add"
LLAMA_TOKENIZER_REVISION="d02ad6cb9dd2c2296a6332199fa2fdca5938fef0"

# The real hf-hub on-disk cache convention (models--<owner>--<name>/snapshots/<revision>/<file>)
# would normally also need a content-addressed blobs/ dir with the snapshot as a symlink into it --
# but the one thing that actually matters here is hf-hub's own cache fast path
# (`download_file_to_cache`), which only ever checks whether a plain file already exists at the
# snapshot path. A real regular file there is exactly as good as the fuller structure a live
# download would produce, and much simpler to construct directly.
bake_pinned_file() {
    local repo="$1" revision="$2" filename="$3"
    local repo_folder="models--${repo//\//--}"
    local dest_dir="$HF_CACHE_DIR/$repo_folder/snapshots/$revision"
    local host_cache="${HF_HUB_CACHE:-$HOME/.cache/huggingface/hub}"
    local host_snapshot="$host_cache/$repo_folder/snapshots/$revision/$filename"

    mkdir -p "$dest_dir"
    if [[ -e "$host_snapshot" ]]; then
        echo "Reusing already-downloaded $repo@$revision/$filename from $host_cache"
        cp -L "$host_snapshot" "$dest_dir/$filename"
    else
        echo "Downloading $repo@$revision/$filename from the real Hugging Face Hub..."
        curl -fSL -o "$dest_dir/$filename" \
            "https://huggingface.co/$repo/resolve/$revision/$filename"
    fi
}

bake_pinned_file "karpathy/tinyllamas" "$TINYLLAMAS_REVISION" "stories15M.bin"
bake_pinned_file "hf-internal-testing/llama-tokenizer" "$LLAMA_TOKENIZER_REVISION" "tokenizer.json"

mkdir -p "$TLS_DIR"
if [[ -f /etc/ssl/certs/ca-certificates.crt ]]; then
    cp /etc/ssl/certs/ca-certificates.crt "$TLS_DIR/ca-certificates.crt"
    echo "Baked the real host CA bundle (/etc/ssl/certs/ca-certificates.crt) into $TLS_DIR"
else
    echo "warning: no /etc/ssl/certs/ca-certificates.crt found on this host -- a candle-enabled" >&2
    echo "image built without it will fail to construct a real Candle inference backend at all" >&2
    echo "(its HTTP client's TLS setup needs some real CA bundle just to succeed, even though" >&2
    echo "the pinned model above means it never actually uses one for a real connection)." >&2
    exit 1
fi

echo
echo "Baked into $OVERLAY_DIR. To build a candle-enabled image from here:"
echo "  1. Cross-compile with a working musl C compiler (needed by hf-hub's own TLS/HTTP"
echo "     dependencies): set CC_x86_64_unknown_linux_musl, then"
echo "     cargo build -p hyperion-init -p hyperion-console --release \\"
echo "       --target x86_64-unknown-linux-musl --features hyperion-console/candle"
echo "  2. Copy the two resulting binaries into $OVERLAY_DIR the same way build-image.sh does."
echo "  3. Re-run 'make' in $BUILDROOT_DIR (or build-image.sh's own make invocation)."
