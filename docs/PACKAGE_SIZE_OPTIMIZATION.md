# Package size optimization / 安装包体积优化验收

Historical record dated 2026-09-25. Fresh baseline: `e8a19bf6e7f151d7985327a46a8f10f272caa169`.
Final runtime and packaging source: `3ee995d908749b80b7023524775eb64accab434a` (version `0.2.7+21`).
The final documentation commit changes no package inputs. Work was isolated on
`codex/package-size`; the original checkout's source was preserved. The old
20.17 MiB MSI / 35.31 MiB APK examples were not used as this baseline.

## Outcome / 结果

Windows x64-v2 MSI decreased from **20.38 to 18.14 MiB (11.02%)**.
Android arm64 APK decreased from **35.35 to 15.91 MiB (55.00%)**.
These are observed local validation packages, not release-size promises.
Self-signed identities and MSI metadata change between runs, so signed package
hashes are not expected to be reproducible byte-for-byte.

The accepted changes remove six unused Lucide fonts, repair Windows icon
subsetting, separate and retain Dart symbols, and compress native libraries in
release APKs. Native compiler settings remain unchanged after performance
screening. No public API, IPC field, user configuration, protocol, TLS provider,
CPU baseline, feature set or panic policy changes. No CI size monitor, automatic
size report or size threshold was introduced.

## Source and measurement identity

The [measurement manifest](PACKAGE_SIZE_MEASUREMENTS.json) records full commits,
Git trees, SHA-256 of binary diffs from the baseline, lock/toolchain hashes,
package SHA-256, raw file inventories, native-library hashes, signing identities,
and symbol-to-AOT mappings. It contains sanitized measurements, not raw logs.
Intermediate builds were measured before their corresponding commit; those
commits identify the associated build-input changes. Final MSI/APKs were rebuilt
from clean committed source `3ee995d` and their symbols are archived under that
full commit. Native experiment records have their own exact source identity.

| Batch | Commit | Result |
| --- | --- | --- |
| 1 | `51ada8897716be8221ca3dbbf342a6acf3b4edef` | Local Lucide dependency and locked resolution |
| 2 | `5e453792ccb9bbf5786bcd983fb23741a3ddc797` | Attributed Windows backend adapter and regression tests |
| 3 | `8de7b46596fe7cbebaa06ec6867cf915fad0bb4e` | Separate symbols, validated archives, build/release workflow parity |
| 4 | `1d7128ea7b03458b1693cb1aa333f1e45fe80ee6` | Native experiment findings; no compiler flags adopted |
| 5 | `3ee995d908749b80b7023524775eb64accab434a` | Release-only Android native-library compression |
| 6 | This record | Final evidence and documentation index |

Rust 1.97.1, Flutter 3.44.7 at `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`,
Dart 3.12.2, NDK 29.0.14206865, SDK CMake 3.22.1, Android build-tools 36.0.0,
Temurin 17.0.20+8 and manifest-restored WiX 5.0.2 were used. The Windows helper
selected the supported Visual Studio/Ninja environment. Rust dependencies stayed
locked; only the intended Lucide entry changed in the Dart lockfile.
The workstation was Windows 11 build 10.0.26200, Intel Core i7-13620H
(10 cores / 16 logical processors), with 34,124,718,080 physical-memory bytes
reported by the OS. It was not a controlled performance laboratory.
The retained release profile uses `opt-level=3`, thin LTO, one codegen unit,
`panic=abort` and `strip=symbols`; pre-existing stripping is not counted as new
savings.

## Package bytes, payload bytes, installed bytes

MiB means 1,048,576 bytes. MSI payload is the sum of its File table sizes; APK
payload is the sum of ZIP entry uncompressed sizes. These are **not actual
installed disk usage**, which is `not_run` for every artifact. Installer EXE/Burn
bundle size is `not_run`: no bundle was produced or available for this candidate.

| Windows x64-v2 metric | Baseline bytes | Final bytes | Reduction |
| --- | ---: | ---: | ---: |
| Signed local MSI | 21,372,928 | 19,017,728 | 2,355,200 (11.02%) |
| MSI uncompressed File payload | 60,205,339 | 53,246,581 | 6,958,758 |
| Files in MSI | 296 | 290 | 6 |

Only arm64 received a full fresh pre-optimization Android baseline. For the other
ABIs and universal package, the available control is **after fonts and symbols**,
before compression. The following table isolates compression; it must not be
presented as the full six-batch saving for every ABI.

| Android package | Uncompressed-library control bytes | Final APK bytes | Compression-only reduction | Uncompressed ZIP payload bytes (unchanged) |
| --- | ---: | ---: | ---: | ---: |
| armeabi-v7a | 27,991,673 | 15,483,773 (14.77 MiB) | 44.68% | 29,230,009 |
| arm64-v8a | 34,590,785 | 16,678,537 (15.91 MiB) | 51.78% | 35,830,857 |
| x86_64 | 37,842,733 | 17,196,105 (16.40 MiB) | 54.56% | 39,081,585 |
| universal | 96,711,071 | 45,630,255 (43.52 MiB) | 52.82% | 97,869,937 |

All 24 native entries across the four packages retain their exact uncompressed
SHA-256. Final `.so` entries are DEFLATED; controls are STORED. The final manifest
sets `extractNativeLibs=true`; controls set it false. Release variants set both
`useLegacyPackaging` and `useLegacyPackagingFromBundle`; debug retains its prior
behavior. Increased installed storage is the accepted tradeoff, but its amount
has not been measured.

## Contributions and incremental measurements

The Windows GUI inventory excludes the four Rust executables, Wintun and PDBs.
Its final value is byte-for-byte identical to the stage-three inventory.

| Stage | GUI payload bytes | Incremental bytes removed |
| --- | ---: | ---: |
| Fresh baseline | 39,677,667 | — |
| 1: Lucide variants removed | 36,870,381 | 2,807,286 |
| 2: Windows font subsetting | 34,390,077 | 2,480,304 |
| 3: Dart symbols separated | 32,718,909 | 1,671,168 |

MSIs were made only for the baseline and final candidate. Per-batch compressed
MSI savings were not separately measured and cannot be inferred by applying a
fixed compression ratio to this table.

Android arm64 APK: 37,062,947 → 35,770,433 after font cleanup (−1,292,514);
then 34,590,785 after symbol separation (−1,179,648); then 16,678,537 after
native compression (−17,912,248). The Windows backend fix has no Android input.
Final versus baseline APK payload: 39,817,791 → 35,830,857 uncompressed bytes.

- Six Lucide font files account for 2,805,384 raw bytes; associated asset/font
  metadata accounts for the rest of the 2,807,286-byte GUI reduction. All six
  filenames and font declarations are absent from the final asset manifests.
  Ordinary codepoints and direction-sensitive constants are preserved. Upstream
  license, archive/file hashes and changes are in the
  [dependency provenance](../third_party/lucide_icons_flutter-3.1.19/PROVENANCE.md).
- Windows MaterialIcons: 1,645,184 → 1,768 bytes; Lucide: 877,160 → 40,272.
  The pinned desktop backend forwarded literal quotes in the icon flag; the
  repository adapter supplies exact `-dTreeShakeIcons=true` through the process
  boundary without changing the global SDK. A one-off fake-engine rendering test
  compared 118 constants discovered in the actual release kernel using full and
  subset fonts, with exact RGBA equality in LTR and RTL. Existing goldens were
  not updated.
- Windows Dart AOT `app.so`: 10,601,360 → 8,930,192 bytes. Android arm64
  `libapp.so`: 9,110,416 → 7,930,768 bytes. Independent `--analyze-size`
  builds completed first; their JSON hashes and root summaries are in the
  measurement manifest. The analysis outputs stay local. No obfuscation is used.
- Native compiler contribution to the shipped result: **zero**. B (fat LTO),
  C (`s`) and E (Windows C/C++ `/Gy /Gw`) were screened against A (3/thin).
  Forty-two memory benchmark runs provided seven alternating pairs per candidate.
  Noise exceeded the 2% tolerance and no candidate established non-regression;
  D was not pursued. See [native experiments](NATIVE_SIZE_EXPERIMENTS.md) for
  byte counts, throughput, latency, CPU, memory, exclusions and exact commands.

The separate analysis runs attributed the following bytes to representative Dart
libraries. These pre-split attribution totals describe code composition, not
independent removable bytes or compressed package savings; metadata and shared
runtime categories prevent treating this selection as a complete AOT sum.

| Dart attribution | Windows x64 bytes | Android arm64 bytes |
| --- | ---: | ---: |
| `package:flutter` | 3,830,186 | 3,399,033 |
| `package:usque` | 1,134,928 | 934,922 |
| `dart:mixin_deduplication` | 382,108 | 349,841 |
| `package:flutter_localizations` | 291,257 | 235,162 |
| `package:vector_graphics_compiler` | 189,390 | 166,383 |

## Symbols, signing and validation limits

Matching Dart symbols are outside all MSI/APK payloads. Final local archives are
under `dist/flutter-symbols/0.2.7+21/3ee995d908749b80b7023524775eb64accab434a/`, with a separate ZIP in
`dist/final-validation`. Archive manifests map each architecture, AOT build ID,
artifact SHA-256 and symbol SHA-256. Split and universal outputs never overwrite
one another. Build/Release CI preserves separate 90-day symbol artifacts;
maintainers retain a longer-lived copy as described in
[Flutter release symbols](FLUTTER_SYMBOLS.md). Symbols are not public Release files.

The committed standalone exception probe restored `sizeSymbolProbe` at
`tool/fixtures/flutter_symbol_probe.dart:3:27` and `main` at line 5 on Windows.
Nine archive-specific tests cover architecture/build-ID mismatch, missing/empty
data and overwrite rejection; the complete Python suite passed. Android symbols
were verified against each packaged AOT ELF; Android runtime stack restoration
remains `not_run` without a dedicated device or isolated emulator.

MSI table/signature/ICE checks completed successfully. The unchanged authoring
suppresses ICE61 for its equal-version upgrade contract; final ICE output still
has 11 ICE60 language warnings (baseline: 17). They are recorded, not suppressed
or claimed absent. The local signing helper removed its private key, certificate
and temporary trust entries; the final thumbprint was absent from all three
stores and staging was removed. After cleanup the self-signed artifact reports
`UnknownError` from Authenticode, as expected for the removed local trust; it is
not an officially trusted release signature.

All four APKs passed exact ABI-set inspection, signature verification against the
independently recorded temporary keytool fingerprint, ELF machine inspection,
16 KiB LOAD alignment for 64-bit libraries and ZIP alignment. No `kernel_blob.bin`,
Vulkan validation layer, `.symbols` file or nonempty `.debug_info` section was
found in the packages. Temporary Android keystores were deleted. Compression
happened before signing, with no post-signature APK rewriting.

Not run: installation or disk occupancy, Windows ARM64 compilation (missing C++
ARM64 tools), installer EXE, actual Android cold start/JNI or `:vpn` loading,
upgrade, Android TV, Doze, Always-on/Lockdown, reboot, native VPN/TUN, WFP, routes,
DNS/proxy changes, protected Windows/Android/network/performance-lab runs.
Widget tests use a fake engine. Memory/loopback tests do not establish WAN,
device lifecycle, cleanup or leak behavior. Optional protected-runner evidence
is supplemental and is not a publication prerequisite; no publication occurred.

## Commands and results

Run commands from the repository root unless `Set-Location` says otherwise.
Resolve Flutter and the Android SDK from local properties, verify the pinned
version, and use the initialized Windows helper environment as required by
[Contributing](../CONTRIBUTING.md). The command matrix remains authoritative.
Previously passed checks were reused only where subsequent changes did not
affect their inputs. Raw per-command logs and scratch scripts remain locally in
`tmp/size-work`; build and signing outputs are ignored, never committed.

```powershell
python tool/check_repository_policy.py
git diff --check
cargo fmt --all --check
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& ./tool/build_windows_rust_release.ps1 -Variant x64-v2
```

All passed; Windows workspace tests: 1,303 passed, 8 intentional ignores in 22
suites. Default-profile release binaries were rebuilt after native experiments
and again for the final MSI. No experimental flags remain.

```powershell
Set-Location apps/usque_gui
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub
& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .
flutter build windows --release --no-pub
# Independent diagnostic build, before symbol-separated normal builds:
flutter build windows --release --no-pub --analyze-size
flutter build windows --release --no-pub --split-debug-info=../../tmp/size-work/symbols/final-windows
flutter build windows --debug --no-pub
Set-Location ../..
```

Baseline and batch one: 659 Flutter tests each. Batches two and three: 663 tests,
including four adapter regression tests and all Windows pixel goldens; format,
analysis, locked resolution and release builds passed. Real debug build passed.
Adapter tests exercise Debug/Profile/Release, both architecture arguments,
spaces, Dart defines, symbols/analysis forwarding, output draining and failure
exit propagation. The full suite does not run a native VPN.

```powershell
& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy
& ./tool/build_android_rust.ps1 -BuildProfile release -AbiFilter all -CargoAction build
Set-Location apps/usque_gui
flutter pub get --enforce-lockfile
flutter build apk --debug --config-only --no-pub
Set-Location android
./gradlew.bat --no-daemon :app:ktlintCheck
./gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug
Set-Location ../../..
```

Passed after the final Gradle formatting fix: 224 Kotlin tests, zero failures,
errors or skips; lint and ktlint passed. Android Rust Clippy and all three ABI
release JNI builds passed. Separate native correctness/interop results are in
the linked experiment record.

For the explicitly authorized APK builds, the
[Build workflow](../.github/workflows/build.yml) temporary signing setup was
mirrored with a fresh RSA 3072-bit, two-day local identity. The password was
disposable, never official. `keytool -list -v -keystore <temporary.jks>` recorded
the independent SHA-256 fingerprint before cleanup. From the GUI directory:

```powershell
# Independent arm64 analysis: USQUE_ANDROID_ABI=arm64-v8a, temporary signer set.
flutter build apk --release --split-per-abi --target-platform android-arm64 --no-pub --analyze-size
# Final builds: USQUE_ANDROID_ABI=all; separate split and universal symbol dirs.
flutter build apk --release --split-per-abi --target-platform android-arm,android-arm64,android-x64 --no-pub --split-debug-info=../../tmp/size-work/symbols/final-android/split
flutter build apk --release --target-platform android-arm,android-arm64,android-x64 --no-pub --split-debug-info=../../tmp/size-work/symbols/final-android/universal
```

Both final commands passed. Preserve each control's APK before rebuilding.
Use build-tools 36.0.0 for each final/control APK:

```powershell
apksigner.bat verify --verbose --print-certs <apk>
zipalign.exe -c -P 16 4 <apk>
aapt2.exe dump xmltree <apk> --file AndroidManifest.xml
```

Compare the signer digest with the independent keytool record. A local Python
`zipfile` inventory measured `file_size`, `compress_size`, `compress_type` and
SHA-256 of each extracted `.so`, checked the exact ABI sets and banned debug
assets, and parsed ELF headers/sections for architecture, LOAD alignment and
debug data. Its sanitized results are in the manifest. Native hashes and total
raw ZIP sizes were compared directly to the stage-three control for all four
APKs. ZIP overhead/manifest changes are included in package bytes.

```powershell
python -m ruff check tool
python -m ruff format --check tool
python -m unittest discover -s tool -p "test_*.py" -v
actionlint -no-color
# In the helper-initialized Windows native environment:
pwsh -NoProfile -File tool/check_source.ps1
```

Passed: Python 86 tests, Ruff 0.16.0, pinned actionlint, and the exact aggregate
command after the final Gradle fix. Aggregate included PSScriptAnalyzer 1.25.0,
Buf 1.72.0, Rust/Dart/Kotlin checks. It was not substituted for tests or builds.

```powershell
dart compile aot-snapshot --save-debugging-info=tmp/size-work/committed-probe.symbols -o tmp/size-work/committed-probe.aot tool/fixtures/flutter_symbol_probe.dart
dartaotruntime tmp/size-work/committed-probe.aot
python tool/archive_flutter_symbols.py verify-stack --symbols tmp/size-work/committed-probe.symbols --stack tmp/size-work/committed-probe.stack
flutter symbolize --debug-info=tmp/size-work/committed-probe.symbols --input=tmp/size-work/committed-probe.stack
```

Capture the runtime's stdout/stderr into `committed-probe.stack`; its nonzero
exit is the deliberate exception. Verification and symbolization passed with
the expected source lines. This fixture is not imported into the product.

The archive invocations from the repository root were:

```powershell
$sizeSourceCommit = '3ee995d908749b80b7023524775eb64accab434a'
python tool/archive_flutter_symbols.py archive --artifact dist/final-windows/windows-gui/data/app.so --symbols tmp/size-work/symbols/final-windows --output dist/flutter-symbols --version 0.2.7+21 --source-commit $sizeSourceCommit --platform windows --kind windows --architecture windows-x64
foreach ($sizeAbi in @('armeabi-v7a', 'arm64-v8a', 'x86_64')) {
    python tool/archive_flutter_symbols.py archive --artifact "dist/final-android/app-$sizeAbi-release.apk" --symbols tmp/size-work/symbols/final-android/split --output dist/flutter-symbols --version 0.2.7+21 --source-commit $sizeSourceCommit --platform android --kind split --architecture $sizeAbi
    if ($LASTEXITCODE -ne 0) { throw 'Symbol validation failed' }
}
python tool/archive_flutter_symbols.py archive --artifact dist/final-android/app-release.apk --symbols tmp/size-work/symbols/final-android/universal --output dist/flutter-symbols --version 0.2.7+21 --source-commit $sizeSourceCommit --platform android --kind universal --architecture armeabi-v7a,arm64-v8a,x86_64
```

All five archives passed ELF/build-ID checks before local retention. Their
mappings are in the manifest. Use a fresh archive destination for reproduction;
the tool deliberately rejects overwrite. See [the symbol guide](FLUTTER_SYMBOLS.md)
for ongoing maintenance.

```powershell
dotnet tool restore
& ./tool/build_windows_local_validation.ps1 -Variant x64-v2 -Version 0.2.7 -BuildLabel local-validation-size-3ee995d -OutputDirectory dist/final-validation
```

Passed after fresh release outputs. The helper performs pinned signature, MSI
table and ICE checks; it never installs the package. A read-only WindowsInstaller
COM database query, `SELECT FileName, FileSize FROM File`, supplied the payload
inventory. The baseline used the same helper/version with label
`size-baseline-e8a19bf`. Resolved staging containment was checked before invoking
the helper's cleanup. Final validation artifacts and the symbol ZIP are in
`dist/final-validation`, named explicitly as local validation outputs.

The final documentation batch runs `python tool/check_repository_policy.py` and
`git diff --check`, then stages only this record, its sanitized JSON and the
documentation index. No package, key, raw log, symbol, build tree or generated
JNI output is committed.
