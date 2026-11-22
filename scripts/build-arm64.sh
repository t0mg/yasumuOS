#!/usr/bin/env bash
# Cross-compiles `yasumu` for aarch64-unknown-linux-gnu (Allwinner H700 / muOS) using Podman or Docker
# and packages dist/yasumuOS.muxapp and dist/yasumuOS-installer.tar.gz.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if command -v podman >/dev/null 2>&1; then
    CONTAINER_CLI="podman"
    VOL_OPTS=":Z"
elif command -v docker >/dev/null 2>&1; then
    CONTAINER_CLI="docker"
    VOL_OPTS=""
else
    echo "Error: neither podman nor docker found on PATH." >&2
    exit 1
fi

if ! "$CONTAINER_CLI" image exists quos-arm64-builder 2>/dev/null; then
    "$CONTAINER_CLI" build -f Dockerfile.aarch64 -t quos-arm64-builder .
fi

"$CONTAINER_CLI" run --rm \
    -v "${ROOT_DIR}:/workspace${VOL_OPTS}" \
    -v "${HOME}/.cargo/registry:/usr/local/cargo/registry${VOL_OPTS}" \
    quos-arm64-builder \
    bash -c "cargo build --release --offline --target aarch64-unknown-linux-gnu && ./scripts/package-release.sh target/aarch64-unknown-linux-gnu/release/yasumu"
