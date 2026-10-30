# macOS development package size

The [compact dev preview](https://github.com/JustinL330/usque-app/releases/tag/macos-dev-v0.3.3-compact-12636756)
reduces both native DMGs while retaining the Proton CEF component, its sandbox
and all Flutter application languages, including Traditional Chinese.

| Download | Previous bytes | Compact bytes | Reduction |
| --- | ---: | ---: | ---: |
| Apple Silicon (arm64) | 235,657,163 | 125,767,519 | 46.63% |
| Intel (x86_64) | 258,917,929 | 147,135,475 | 43.17% |

These are actual DMG file sizes. Decimal MB is bytes / 1,000,000; GitHub may
display binary units instead. The app's uncompressed file payload changes from
515,676,174 to 366,028,790 bytes on arm64 and from 548,603,664 to 387,881,448 bytes
on Intel. Payload totals do not measure actual installed disk occupancy.

## Why the first packages were large

- CEF is a complete embedded browser engine needed by the planned macOS 13+
  Proton verification component. Its main native library alone is approximately
  237 MB on arm64 and 263 MB on Intel. This implementation bundles it separately
  from Flutter; it does not depend on a browser installed on the user's Mac.
- All five network/control executables retained the full statically linked Rust
  data plane. Before linker dead-code elimination, they totaled 114,948,528 bytes
  on arm64 and 129,068,544 bytes on Intel, including unused code and symbols.
- The CEF distribution supplied many UI locale bundles that this verification
  component never selects. The original DMG also used default zlib compression.

Windows package sizes are not a direct baseline for this bundle: the macOS
package contains the standalone CEF browser and four native Network Extension
prototypes in addition to Flutter and the Engine.

## Changes and retained functionality

1. Native linking uses `-dead_strip`. Only Packet Tunnel retains the data-plane
   code it references; other providers retain their needed policy/native code.
   The five executables total 21,122,512 / 24,281,504 bytes before final symbol
   cleanup. All provider class metadata and required Rust FFI entries were
   inspected in both architectures, including the filter's two provider classes.
2. Before fresh ad-hoc signing, packaging removes debug/local symbol-table
   entries with `strip -S -x` and refuses any change in global exports. It does
   not use aggressive default stripping on dynamically loaded frameworks.
3. CEF keeps `en`, `zh_CN` and `zh_TW` resources, exactly matching the existing
   verification locale selector. Missing or empty required resources fail before
   deletion. Other Flutter application languages are untouched; their verification
   window already falls back to English. ICU, snapshots, rendering libraries,
   sandbox helpers and license notices remain included.
4. DMGs use LZMA-compressed `ULMO`, within the macOS 13 minimum. The native disk
   image tool confirms the format and verifies each image without mounting it.
   Chromium's [Mac packaging tool](https://chromium.googlesource.com/chromium/src/+/refs/tags/111.0.5557.0/chrome/installer/mac/pkg-dmg)
   also documents this macOS 10.15+ format. Compression changes download size,
   not application features.

The CEF component still accounts for 299,007,767 / 313,365,561 uncompressed bytes
after trimming. Removing it would remove a planned verification dependency;
these packages retain it. The [CEF deployment documentation](https://chromiumembedded.github.io/cef/general_usage.html)
describes the browser/framework and locale resource layout.

## Source and validation

The baseline is tag `macos-dev-v0.3.3-3289e10d`. Compact packaging and native
linking source is `12636756a527267e029f34173c9e3e035ddaacfc`, tree
`0d182d6bfee114efd862dfa0118722fbe52f5b11`. Existing GUI, Engine and CEF build
outputs from `00e243f54cfe8cc6cf4665e9d30172344606c4e8` were repackaged; the five
native network/control programs were rebuilt. This is not a new full Flutter or
Rust workspace build. Later download-link documentation does not change these
binaries.

- 17 packaging fixture tests and 22 browser-free CEF harness tests passed.
- Seven native contract groups, both-architecture native typechecking and the
  source-archive repository policy passed.
- Each app contains 20 Mach-O files matching its target architecture; ad-hoc
  signature integrity and DMG integrity/ULMO format checks passed.
- All non-debug file-backed sections in the other 15 Mach-O files match the
  previous package. Only the five rebuilt network/control binaries may differ
  through dead-code elimination; their required entry points remain present.

The [machine-readable size record](MACOS_PACKAGE_SIZE_VALIDATION.json) includes
exact bytes, hashes and static native checks. No application, CEF browser,
provider or control helper was launched, no package installed and no host
network setting changed during this work. Intel execution and macOS 13 hardware
remain untested. The [existing QA limitations](MACOS_QA_CHECKLIST.md), including
the unresolved full CEF proxy-failure shutdown smoke, remain in effect. These
unnotarized development packages keep system VPN activation and production
automatic installation disabled.

繁體中文：首版主要大在內附的 CEF 瀏覽器、五份網路程式重複帶入未使用的
Rust 程式碼，以及多餘的驗證語言檔。精簡後下載量約減少 47%／43%，保留繁體中文、
原有介面語言、驗證元件與 sandbox。安裝包仍未公證；體積縮減不代表實機驗收通過。
