#!/usr/bin/env bash
set -euo pipefail
TASK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TASK_ARCH="${1:?arm64 or x86_64 required}"
case "$TASK_ARCH" in arm64|x86_64) ;; *) exit 2 ;; esac
TASK_OUTPUT="$TASK_ROOT/build/macos-agent/$TASK_ARCH"
mkdir -p "$TASK_OUTPUT" "$TASK_ROOT/.toolchains/cache/clang"
xcrun swiftc -swift-version 5 -warnings-as-errors -O -parse-as-library -target "$TASK_ARCH-apple-macos13.0" \
  -module-cache-path "$TASK_ROOT/.toolchains/cache/clang" \
  "$TASK_ROOT/tool/macos-agent/Maintenance.swift" "$TASK_ROOT/tool/macos-agent/Main.swift" \
  -o "$TASK_OUTPUT/usque-agent-maintenance"
/usr/bin/lipo -verify_arch "$TASK_ARCH" "$TASK_OUTPUT/usque-agent-maintenance"
