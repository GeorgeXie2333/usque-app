#!/usr/bin/env bash
set -euo pipefail
TASK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TASK_OUTPUT="$TASK_ROOT/build/macos-maintenance/tests"
mkdir -p "$TASK_OUTPUT" "$TASK_ROOT/.toolchains/cache/clang"
xcrun swiftc -swift-version 5 -warnings-as-errors -parse-as-library -target "$(uname -m)-apple-macos13.0" \
  -module-cache-path "$TASK_ROOT/.toolchains/cache/clang" \
  "$TASK_ROOT/tool/macos-maintenance/Maintenance.swift" "$TASK_ROOT/tool/macos-maintenance/Tests.swift" \
  -o "$TASK_OUTPUT/maintenance-contracts"
"$TASK_OUTPUT/maintenance-contracts"
