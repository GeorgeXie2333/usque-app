# Flutter release symbols

Release builds separate Dart debugging information with `--split-debug-info`.
They do not enable obfuscation. Symbols are maintenance artifacts and must stay
outside the Windows application directory and Android APKs.

## Build and archive locally

Use the pinned Flutter SDK and the complete change-scoped checks in
[Contributing](../CONTRIBUTING.md). Run size-analysis builds separately:
the pinned SDK rejects combining `--analyze-size` and `--split-debug-info`.
Analysis builds are diagnostic outputs, not the final package candidates.

From the Flutter application directory, the normal Windows release command is:

```powershell
flutter build windows --release --no-pub --split-debug-info=build/symbols/windows
```

After committing the exact tested source, run from the repository root:

```powershell
$sourceCommit = (git rev-parse HEAD).Trim()
$version = ((Get-Content apps/usque_gui/pubspec.yaml | Where-Object { $_ -match '^version: ' }) -replace '^version:\s*', '').Trim()
python tool/archive_flutter_symbols.py archive `
  --artifact apps/usque_gui/build/windows/x64/runner/Release/data/app.so `
  --symbols apps/usque_gui/build/symbols/windows `
  --output dist/flutter-symbols --version $version --source-commit $sourceCommit `
  --platform windows --kind windows --architecture windows-x64
```

For Windows ARM64, use the ARM64 build output and `windows-arm64` architecture.
Do not label a dirty-source build with only the parent commit: commit the tested
changes first or retain its full source-difference evidence outside this clean
release archive workflow. Rebuild after any subsequent code change.

For authorized Android release builds, use the existing temporary build-only
signing procedure locally. Give split and universal builds separate symbol
directories, for example `build/symbols/android/split` and
`build/symbols/android/universal`. Archive each finished APK with
`--platform android`, its matching `--kind split` or `--kind universal`, and
the corresponding symbol directory. A split package takes one ABI in
`--architecture`; universal takes `armeabi-v7a,arm64-v8a,x86_64`.

The archive tool checks every AOT library against its symbols using ELF
architecture, GNU build ID and nonempty DWARF information. Missing, empty,
wrong-architecture or mismatched symbols fail before creating an archive.
It records AOT, symbol and input artifact SHA-256 values. For Windows, the input
artifact is the installed `data/app.so`, not an MSI signature or wrapper.

The output layout is:

```text
<output>/<version>/<full-commit>/<platform>/<kind>/<artifact-sha256>/
  manifest.json
  <architecture>/app.<target>.symbols
```

Existing destinations are never overwritten. Keep local archive copies when
removing intermediate build directories; symbol files are not recoverable from
the stripped binary.

## Workflow retention

The Build and Release workflows generate and validate the same separate
symbols. Each successful job uploads a `usque-flutter-symbols-*` artifact,
retained for 90 days. Maintainers must download and retain the matching archive
for the supported lifetime of a released version; the workflow copy is not a
permanent backup. Artifacts follow the repository's existing access rules.

Symbol artifacts deliberately do not match the release candidate's
`usque-release-*` download pattern. They are not copied into the candidate,
installed application, or public Release asset list. This adds no size report,
comparison job, or package-size threshold to CI.

## Restore a Dart stack trace

Select the archive for the actual binary's version, source and architecture.
Verify the trace build ID before passing it to Flutter:

```shell
python tool/archive_flutter_symbols.py verify-stack --symbols "path/to/app.target.symbols" --stack "path/to/stack.txt"
flutter symbolize --debug-info="path/to/app.target.symbols" --input="path/to/stack.txt"
```

Both commands must succeed. A trace without an exact build ID match is rejected
by the first command; do not guess using a nearby version's symbols. The helper
never uploads the trace. Keep user logs and credentials out of symbol archives.

The standalone [symbol probe](../tool/fixtures/flutter_symbol_probe.dart) throws
a fixed exception without starting Usque. With the pinned Dart SDK, compile it
using `dart compile aot-snapshot --save-debugging-info=<symbols> -o <snapshot>`
and run the snapshot with `dartaotruntime`. Its nonzero exit is intentional.
Save its output, run the two commands above, and confirm the restored
`sizeSymbolProbe` frame and source line. Its snapshot and symbols are temporary
validation outputs, never application assets.

These symbols restore Dart frames. Rust/native symbol handling and platform
crash dumps remain separate from this workflow.
