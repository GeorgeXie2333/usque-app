# Contributing to Usque

Thanks for helping. This project changes DNS, routes, credentials, and leak prevention, so keep diffs small, say what the security impact is, and test the paths you touch.

[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) applies. Report vulnerabilities privately as in [SECURITY.md](SECURITY.md). Do not put exploit details or credentials in a public Issue.

## Before writing code

- Search existing Issues and Pull Requests first.
- Use a Bug Issue for a reproducible defect and a Feature Issue for a product proposal.
- Talk through large protocol, privilege, storage, installer, release, or UX changes before building them.
- Do not use a public Issue for traffic leaks, pin bypasses, credential exposure, privilege bugs, or release-chain problems.
- Leave the Go oracle snapshot in `oracle/go` and its attribution alone. It is a frozen local reference for interoperability, not a shipping client.

## Development machines

On a normal development machine, do not:

- install a generated MSI;
- start Windows VPN mode or create a TUN/Wintun session;
- apply WFP filters, routes, interface DNS, or system-proxy changes;
- run `usque-agent --recover-state`, `--emergency-remove-kill-switch`, or the engine `--purge-user-data` just to test a build.

Windows VPN, recovery, upgrade, and uninstall tests need a snapshot VM with another way in. Android VPN lifecycle tests need a dedicated device or isolated emulator.

These are safe on a development machine: SOCKS5 and HTTP loopback tests, compile-only builds, MSI table/ICE checks, `usque-agent --validate-only`, and `usque-uninstall --dry-run` without a live ProductCode.

How the Windows package uninstalls and when it deletes user data is in [docs/INSTALLATION.md](docs/INSTALLATION.md). Local Windows build traps (MSVC, CMake, Ninja, libclang) are in [AGENTS.md](AGENTS.md).

If you change privileged networking or the installer and cannot run the isolated tests, say so in the pull request. Do not pretend they passed.

## Toolchains

- Rust `1.97.1`, always with `--locked`
- Flutter `3.44.7` (commit `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`)
- Android NDK `29.0.14206865` and SDK CMake `3.22.1`
- Ruff `0.16.0`, PSScriptAnalyzer `1.25.0`, Buf `1.72.0`, actionlint `1.7.12`
- WiX `5.0.2` via the checked-in .NET tool manifest

Flutter and Android SDK paths come from `apps/usque_gui/android/local.properties`
(`flutter.sdk` and `sdk.dir`). Use that Flutter SDK's `bin` directory for the
`flutter` and `dart` commands below; do not assume a global installation is
correct. Verify `flutter --version` against the version and full commit pinned
in [CI](.github/workflows/ci.yml) before resolving packages. On a new machine,
install the pinned SDKs and create this local path file first. Use PowerShell 7
for the PowerShell helpers. The Windows helper additionally needs Visual Studio
C++ Build Tools and the Windows SDK; it selects the native build environment.

Toolchain manifests, Gradle configuration, and build helpers are authoritative
for versions and executable behavior. Do not commit `local.properties`, signing
material, generated JNI libraries, build directories, logs, diagnostics, or
release artifacts. Official signing rules are in
[docs/CODE_SIGNING.md](docs/CODE_SIGNING.md).

## Branches, commits, and pull requests

1. Branch from an up-to-date `main`. Long-lived local branches are fine; the pull request still targets `main`.
2. Leave unrelated formatting and generated-file noise out of the change.
3. Add or update tests before opening the pull request.
4. Fill in the pull request template and list tests you did not run.
5. Use a Conventional Commit-style title, for example `fix(android): reconnect HTTP proxy after network change`.
6. Resolve review threads and rerun required checks after the last change.

Accepted types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `revert`. The project squash-merges. Commits do not need `Signed-off-by`.

## Required checks by change scope

Run every applicable section, starting each code block at the repository root
unless it specifies another directory. Record exact commands, results, and
anything not run. The [CI](.github/workflows/ci.yml) and compile-only
[Build](.github/workflows/build.yml) workflows define the hosted gates.

### Markdown-only changes

```shell
python tool/check_repository_policy.py
git diff --check
```

Use a verified Python 3.10+ executable if `python` is not on PATH. The policy
check covers first-party local link targets, UTF-8, and repository rules; it
does not validate external URLs, heading anchors, or the truth of documentation.
Review those separately. Check release-note template edits with the renderer's
tests as well:

```shell
python -m unittest discover -s tool -p "test_release_contract.py" -v
```

### Aggregate source checks

For multi-language changes, the aggregate script collects format and static
checks without rewriting files:

```shell
pwsh -NoProfile -File tool/check_source.ps1
```

It does **not** replace Rust, Flutter, Kotlin, Python, or Go tests, Android Rust
Clippy, or platform builds. Run those separately when applicable. On Windows,
initialize the supported native environment with the Windows Rust helper in
the same PowerShell session before running the aggregate script.

### Rust

On Windows, use the helper-based Clippy and test commands in **Windows Rust
and MSI authoring** below instead of plain Cargo in a fresh shell. The format
check applies on every host.

```shell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

Every `unsafe` block needs a `// SAFETY:` comment that states the invariants. A public unsafe API needs a rustdoc `# Safety` section.

### Flutter and Dart

Analyzer settings live in `apps/usque_gui/analysis_options.yaml`.

```shell
cd apps/usque_gui
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub
```

Bitmap tests are tagged `golden`. Their checked-in baselines use Windows x64
and the pinned Flutter SDK: [custom-font rendering varies by host
platform](https://api.flutter.dev/flutter/flutter_test/matchesGoldenFile.html).
The command above runs all tests on Windows. On other hosts, run
`flutter test --no-pub --exclude-tags golden`, then validate the bitmap suite
on Windows with `flutter test --no-pub --tags golden`. CI requires both the
Ubuntu widget suite and the Windows golden suite in `CI / gate`; neither is
optional. Keep exact pixel comparison. Regenerate baselines only on Windows
with the pinned SDK, review every visual diff, and never update them in CI.

### Android Rust and Kotlin

First run the Android-target Rust check from the repository root. The helper
requires the pinned NDK and SDK CMake installation, uses locked dependencies,
and checks the arm64-v8a library without copying JNI output:

```powershell
& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy
```

Then run the Flutter configuration and Kotlin checks from the repository root:

```shell
cd apps/usque_gui
flutter pub get --enforce-lockfile
flutter build apk --debug --config-only --no-pub

cd android
./gradlew --no-daemon :app:ktlintCheck
./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug
```

On Windows, use `.\gradlew.bat` in place of `./gradlew`. The arm64 Rust check
does not establish three-ABI build coverage. Full JNI builds use the same
helper with `-CargoAction build -AbiFilter all`; generated `jniLibs` must not
be committed. Release APK builds require an explicit request and the ephemeral
build-only signing procedure in [Build](.github/workflows/build.yml), never
official signing material on a development host. Do not install a release APK
on a personal or shared device to validate it.

Kotlin compiler warnings and Android lint warnings are errors. ktlint is pinned through `org.jlleitschuh.gradle.ktlint` `14.2.0` and ktlint `1.8.0`.

### Python tooling

```shell
pip install ruff==0.16.0
ruff check tool
ruff format --check tool
python -m unittest discover -s tool -p "test_*.py" -v
```

Security-rule suppressions such as `S603` or `S607` must be per-line and include a short reason. Do not add a global Ruff or Bandit suppression.

### PowerShell tooling

Every script in `tool/` must declare `[CmdletBinding()]`, call `Set-StrictMode -Version Latest`, and set `$ErrorActionPreference = 'Stop'`.

```shell
Install-Module PSScriptAnalyzer -RequiredVersion 1.25.0 -Scope CurrentUser -Force
Invoke-ScriptAnalyzer -Path tool -Recurse -Settings tool/PSScriptAnalyzerSettings.psd1
Invoke-ScriptAnalyzer -Path tool -Recurse -IncludeRule PSUseCorrectCasing
```

Use `tool/check_source.ps1` or the CI tooling job for the real result. `Invoke-ScriptAnalyzer` does not always exit non-zero on findings.

### Protocol Buffers

```shell
buf lint
buf format --exit-code --diff
```

CI runs Buf's `FILE` breaking check against the PR target. Do not reuse field numbers or change the wire shape without a reviewed protocol migration and wire snapshot tests.

### GitHub Actions

```shell
go install github.com/rhysd/actionlint/cmd/actionlint@914e7df21a07ef503a81201c76d2b11c789d3fca
actionlint -no-color
```

Pin external Actions to a full commit SHA and put the human release in a trailing comment. PR workflows stay read-only and must not expose secrets to untrusted code.

### Windows Rust and MSI authoring

Do not run a plain `cargo build --release` in a fresh Windows shell. Use the helper so MSVC, Ninja, CMake, and libclang are set up:

```powershell
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2
```

Why the helper exists is in [AGENTS.md](AGENTS.md). For MSI or installer-bundle work, restore the pinned .NET tool and follow the multilingual CI fixture build. Table, transform, bundle extraction, detach/reattach, and ICE validation are safe; running the bundle or installing the MSI is not.

For quiet-uninstall changes, run `pwsh -NoProfile -File
tool/test_windows_quiet_uninstall.ps1` and the same test script with Windows
PowerShell 5.1. These use inert process/registry doubles and a harmless child
process to check completion, exit codes, copy locking, and launcher encoding.
They do not run an MSI or uninstall a product.

### Windows Flutter and runner

For Flutter or Windows-runner changes, run the Windows Rust gates above and
the complete sequence below on Windows with the pinned Flutter SDK. Do not
substitute a build-only command for format, analysis, and tests:

```powershell
Set-Location apps/usque_gui
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub
& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .
flutter build windows --release --no-pub
```

The plugin-junction helper is part of the checked-in Windows build sequence.
Application assembly and binary inspection are defined in
[Build](.github/workflows/build.yml). This is compile-only validation: it does
not install an MSI, launch VPN mode, or demonstrate cleanup or leak behavior.
Create a local validation MSI only when explicitly requested and only after
fresh Rust and Flutter artifacts pass their applicable checks. Follow the
packaging safety boundary in [AGENTS.md](AGENTS.md).

### Go oracle snapshot

```shell
cd oracle/go
go mod verify
go test ./...
cd ../..
python tool/verify_oracle_archive.py
```

Do not bump `oracle/go/go.mod`, `go.sum`, or archived source in a routine dependency PR. Oracle-only vulnerabilities are reported separately and are not a reason to edit the freeze.

## Dependency changes

- Keep new dependencies small and compatible with every declared target.
- Commit the matching lockfiles.
- For Gradle, also update verification metadata on purpose:

```shell
cd apps/usque_gui/android
./gradlew --no-daemon :app:dependencies --write-locks
./gradlew --no-daemon --write-verification-metadata sha256 help
```

- Review every new artifact and checksum. CI and release jobs must not generate lock or verification metadata.
- Dependabot PRs get the same review and checks as any other PR.
- A temporary vulnerability exception must name the advisory, say why it is not exploitable right now, and include an expiry date.

## Change-specific acceptance

- Protocol changes need unit/property tests and a Go-oracle fixture.
- Parsers and frame codecs need malformed-input tests. Externally reachable parsers need fuzz coverage.
- TUN, route, DNS, WFP/firewall, system-proxy, sleep/wake, update, installer, and uninstall changes need cleanup and leak-prevention tests in an isolated environment.
- New logs and diagnostics must be checked for secrets, tokens, keys, licenses, pins, device identifiers, and sensitive addresses.
- UI changes should keep English and Simplified Chinese, light and dark themes, and keyboard focus working. Screen readers, 200% scaling, and Android TV D-pad apply when the change can affect those paths.
- Use Lucide icons, not emoji, as interface icons.
- Do not add a WebView UI, an insecure TLS toggle, automatic telemetry, or automatic diagnostic upload.

## Quality policy

- Do not add blanket lint baselines, repo-wide suppressions, auto-fix CI jobs, or generated snapshots that hide new findings.
- Do not reformat `oracle/`, `third_party/`, or generated sources as part of unrelated work.
- Do not introduce Detekt, mypy, clang-format, or another runner just to duplicate existing checks.
