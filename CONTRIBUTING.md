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

Flutter and Android SDK paths come from `apps/usque_gui/android/local.properties`. Do not commit that file, signing material, generated JNI libraries, build directories, logs, diagnostics, or release artifacts. Official signing rules are in [docs/CODE_SIGNING.md](docs/CODE_SIGNING.md).

## Branches, commits, and pull requests

1. Branch from an up-to-date `main`. Long-lived local branches are fine; the pull request still targets `main`.
2. Leave unrelated formatting and generated-file noise out of the change.
3. Add or update tests before opening the pull request.
4. Fill in the pull request template and list tests you did not run.
5. Use a Conventional Commit-style title, for example `fix(android): reconnect HTTP proxy after network change`.
6. Resolve review threads and rerun required checks after the last change.

Accepted types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `revert`. The project squash-merges. Commits do not need `Signed-off-by`.

## Required checks by change scope

If several languages change, prefer the aggregate script. It only checks; it does not rewrite files:

```shell
pwsh -NoProfile -File tool/check_source.ps1
```

### Rust

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

### Android and Kotlin

```shell
cd apps/usque_gui
flutter pub get --enforce-lockfile
flutter build apk --debug --config-only --no-pub

cd android
./gradlew --no-daemon :app:ktlintCheck
./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug
```

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
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
```

Why the helper exists is in [AGENTS.md](AGENTS.md). For MSI work, restore the pinned .NET tool and follow the CI fixture build. Table and ICE validation are safe; installing the MSI is not.

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
