#!/usr/bin/env bash
# Isolated contract/frontend tests only. Does not activate Network Extensions,
# alter network services, access the user's Keychain, or contact VPN servers.
set -euo pipefail
TASK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$TASK_ROOT"
if [[ "$(uname -s)" != Darwin ]]; then
  printf '%s\n' 'Run this entry point on macOS; Engine integration is macOS-specific.' >&2
  exit 2
fi
export MACOSX_DEPLOYMENT_TARGET=13.0
# Recent Xcode strip removes symbols needed when loading Rust proc macros.
export CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_STRIP=none
cargo test --locked --offline -p usque-macos-runtime
cargo test --locked --offline -p usque-transport system_proxy::tests
cargo test --locked --offline -p usque-engine macos_runtime::tests
bash tool/test_macos_native_contracts.sh
printf '%s\n' 'Isolated tests passed. Signed activation and real-network safety remain unverified.'
