#!/usr/bin/env bash
set -euo pipefail
TASK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTENTS="$TASK_ROOT/build/macos-desktop-tests/DesktopContracts.app/Contents"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources" "$TASK_ROOT/.toolchains/cache/clang"
# AppKit resolves named images through the main bundle's compiled asset catalog.
cat > "$CONTENTS/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>io.github.georgexie2333.usque.desktop-contracts</string>
  <key>CFBundleExecutable</key><string>desktop-contracts</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST
xcrun actool "$TASK_ROOT/apps/usque_gui/macos/Runner/Assets.xcassets" \
  --compile "$CONTENTS/Resources" --platform macosx --minimum-deployment-target 13.0 \
  --target-device mac --output-format human-readable-text
for strings_file in "$TASK_ROOT"/apps/usque_gui/macos/Runner/*.lproj/UsqueNative.strings; do
  locale_dir="$(basename "$(dirname "$strings_file")")"
  mkdir -p "$CONTENTS/Resources/$locale_dir"
  cp "$strings_file" "$CONTENTS/Resources/$locale_dir/UsqueNative.strings"
done
xcrun swiftc -swift-version 5 -parse-as-library -target "$(uname -m)-apple-macos13.0" \
  -module-cache-path "$TASK_ROOT/.toolchains/cache/clang" \
  "$TASK_ROOT/apps/usque_gui/macos/Runner/NativeLocalization.swift" \
  "$TASK_ROOT/apps/usque_gui/macos/Runner/DesktopContracts.swift" \
  "$TASK_ROOT/apps/usque_gui/macos/RunnerTests/DesktopContractMain.swift" \
  -o "$CONTENTS/MacOS/desktop-contracts"
"$CONTENTS/MacOS/desktop-contracts" "$TASK_ROOT/apps/usque_gui/macos/Runner/Base.lproj/MainMenu.xib"
