#!/bin/sh
# Run cargo inside the builder image. Usage: packaging/cargo.sh <cargo arguments>
# Source is mounted read-write; the target directory and the registry live in Docker volumes,
# which is much faster than the Windows bind mount.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd -W 2>/dev/null || pwd)
MSYS_NO_PATHCONV=1 exec docker run --rm \
  -v "$here:/src" \
  -v smhub-cargo-registry:/usr/local/cargo/registry \
  -v smhub-cargo-git:/usr/local/cargo/git \
  -v smhub-target:/target \
  -e CARGO_BUILD_JOBS=24 \
  -w /src \
  smhub-services-build:2 "$@"
