#!/usr/bin/env bash
set -euo pipefail
TASK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TASK_NATIVE="$TASK_ROOT/apps/usque_gui/macos/NetworkExtensions"
TASK_OUTPUT="$TASK_ROOT/build/macos-network-prototype/tests"
mkdir -p "$TASK_OUTPUT" "$TASK_ROOT/.toolchains/cache/clang"
xcrun swiftc -swift-version 5 -parse-as-library -target "$(uname -m)-apple-macos13.0" \
  -module-cache-path "$TASK_ROOT/.toolchains/cache/clang" \
  "$TASK_NATIVE/Shared/Protocol.swift" "$TASK_NATIVE/ContentFilter/GuardPacket.swift" \
  "$TASK_NATIVE/Tests/ContractMain.swift" -o "$TASK_OUTPUT/native-contracts"
"$TASK_OUTPUT/native-contracts"
