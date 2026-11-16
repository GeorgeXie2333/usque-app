#!/usr/bin/env bash
set -euo pipefail
TASK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TASK_OUTPUT="$TASK_ROOT/build/macos-desktop-tests"
mkdir -p "$TASK_OUTPUT" "$TASK_ROOT/.toolchains/cache/clang"
for strings_file in "$TASK_ROOT"/apps/usque_gui/macos/Runner/*.lproj/UsqueNative.strings; do
  locale_dir="$(basename "$(dirname "$strings_file")")"
  mkdir -p "$TASK_OUTPUT/$locale_dir"
  cp "$strings_file" "$TASK_OUTPUT/$locale_dir/UsqueNative.strings"
done
xcrun swiftc -swift-version 5 -parse-as-library -target "$(uname -m)-apple-macos13.0" \
  -module-cache-path "$TASK_ROOT/.toolchains/cache/clang" \
  "$TASK_ROOT/apps/usque_gui/macos/Runner/NativeLocalization.swift" \
  "$TASK_ROOT/apps/usque_gui/macos/Runner/DesktopContracts.swift" \
  "$TASK_ROOT/apps/usque_gui/macos/RunnerTests/DesktopContractMain.swift" \
  -o "$TASK_OUTPUT/desktop-contracts"
"$TASK_OUTPUT/desktop-contracts" "$TASK_ROOT/apps/usque_gui/macos/Runner/Base.lproj/MainMenu.xib"
