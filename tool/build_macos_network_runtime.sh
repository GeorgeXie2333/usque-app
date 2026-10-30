#!/usr/bin/env bash
set -euo pipefail
TASK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
case "${1:-}" in
  arm64) TASK_RUST_TARGET=aarch64-apple-darwin ;;
  x86_64) TASK_RUST_TARGET=x86_64-apple-darwin ;;
  *) printf '%s\n' 'Usage: build_macos_network_runtime.sh arm64|x86_64' >&2; exit 2 ;;
esac
cd "$TASK_ROOT"
export MACOSX_DEPLOYMENT_TARGET=13.0
# Explicit C/C++ flags also invalidate cached CMake dependencies compiled for
# the build host's newer macOS. Rust's deployment setting alone is insufficient.
export CFLAGS="${CFLAGS:-} -mmacosx-version-min=13.0"
export CXXFLAGS="${CXXFLAGS:-} -mmacosx-version-min=13.0"
export CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_STRIP=none
cargo build --locked --offline --target "$TASK_RUST_TARGET" -p usque-macos-runtime
