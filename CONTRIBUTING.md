# Contributing to Usque

Thank you for helping improve Usque. This project controls privileged network state, so a small change can affect DNS, routing, credentials, or leak prevention. Keep changes narrow, explain their security impact, and test the code paths you touch.

Participation is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md); never place exploit details or credentials in a public Issue.

## Before writing code

- Search existing Issues and Pull Requests before opening a duplicate.
- Open a Bug Issue for reproducible defects and a Feature Issue for product proposals.
- Discuss large protocol, privilege, storage, installer, release, or UX changes before investing in an implementation.
- Do not use a public Issue for traffic leaks, endpoint-pin bypasses, credential exposure, privilege-boundary failures, or release-chain vulnerabilities.
- Preserve the archived Go oracle and upstream attribution. It is a frozen behavior reference, not a maintained shipping client.

## Development safety boundary

On an ordinary development or Codex host, never:

- install a generated MSI;
- start Windows VPN mode or create a TUN/Wintun session;
- run commands that apply WFP filters, routes, interface DNS, or system-proxy changes;
- use `usque-agent --recover-state`, `--emergency-remove-kill-switch`, or the Engine's `--purge-user-data` merely to test a build.

Windows VPN, recovery, upgrade, and uninstall tests require a snapshot-enabled isolated VM with out-of-band access. Android VPN lifecycle tests require a dedicated device or isolated emulator. SOCKS5 and HTTP loopback tests, compile-only builds, MSI table/ICE validation, and `usque-agent --validate-only` are safe on the development host.

## Toolchains

Use the versions pinned by the repository:

- Rust `1.97.1`, always with `--locked`;
- Flutter commit `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`;
- Android NDK `29.0.14206865` and SDK CMake `3.22.1`;
- Ruff `0.16.0`, PSScriptAnalyzer `1.25.0`, Buf `1.72.0`, and actionlint `1.7.12`;
- WiX `5.0.2` through the checked-in .NET tool manifest.

Flutter and Android SDK paths come from `apps/usque_gui/android/local.properties`. Do not commit that file, signing material, generated JNI libraries, build directories, logs, diagnostics, or release artifacts.

## Branches, commits, and Pull Requests

1. Branch from an up-to-date `main`.
2. Keep unrelated formatting and generated-file churn out of the change.
3. Add or update tests before opening the Pull Request.
4. Complete the Pull Request template and identify tests that were not run.
5. Use a Conventional Commit-style PR title, for example `fix(android): reconnect HTTP proxy after network change`.
6. Resolve review conversations and rerun required checks after the final change.

Accepted PR title types are `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, and `revert`. The project uses squash merging. A CLA, DCO, and `Signed-off-by` trailer are not required.

## Required checks by change scope

Run the checks that cover the code you touched. Prefer the aggregate host script when several languages change:

```shell
# Check-only multi-language gates; never rewrites files.
pwsh -NoProfile -File tool/check_source.ps1
```

### Rust

```shell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

Every `unsafe` block needs a `// SAFETY:` comment that states its invariants. A public unsafe API must document a rustdoc `# Safety` section.

### Flutter and Dart

```shell
cd apps/usque_gui
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub
```

Strict casts, inference, raw types, and directive ordering are enabled.

### Android and Kotlin

```shell
cd apps/usque_gui
flutter pub get --enforce-lockfile
flutter build apk --debug --config-only --no-pub

cd android
./gradlew --no-daemon :app:ktlintCheck
./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug
```

Kotlin compiler warnings and Android lint warnings are errors. The project pins ktlint through `org.jlleitschuh.gradle.ktlint` `14.2.0` and ktlint `1.8.0`.

### Python tooling

```shell
pip install ruff==0.16.0
ruff check tool
ruff format --check tool
python -m unittest discover -s tool -p "test_*.py" -v
```

Security-rule suppressions such as `S603` or `S607` must be per-line and include a short justification. Do not add a global Ruff or Bandit suppression.

### PowerShell tooling

Every script in `tool/` must declare `[CmdletBinding()]`, call `Set-StrictMode -Version Latest`, and set `$ErrorActionPreference = 'Stop'`.

```shell
Install-Module PSScriptAnalyzer -RequiredVersion 1.25.0 -Scope CurrentUser -Force
Invoke-ScriptAnalyzer -Path tool -Recurse -Settings tool/PSScriptAnalyzerSettings.psd1
Invoke-ScriptAnalyzer -Path tool -Recurse -IncludeRule PSUseCorrectCasing
```

Use `tool/check_source.ps1` or the CI tooling gate for the fail-closed result; `Invoke-ScriptAnalyzer` does not always exit non-zero for findings by itself.

### Protocol Buffers

```shell
buf lint
buf format --exit-code --diff
```

CI runs Buf's `FILE` breaking check against the PR target. Do not reuse field numbers or change wire shape without an intentional, reviewed protocol migration and wire snapshot tests.

### GitHub Actions

```shell
go install github.com/rhysd/actionlint/cmd/actionlint@914e7df21a07ef503a81201c76d2b11c789d3fca
actionlint -no-color
```

External Actions must be pinned to a complete commit SHA with the human-readable release in a trailing comment. PR workflows must use read-only permissions and must not expose secrets to untrusted code.

### Windows Rust and MSI authoring

Do not use a plain release Cargo command in a fresh Windows shell. Use the checked-in helper so MSVC, Ninja, CMake, and libclang are initialized consistently:

```powershell
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
```

For MSI authoring, restore the pinned .NET tool and follow the fixture build in CI. Static table and ICE validation are safe; installing the MSI is not.

### Archived Go oracle

```shell
cd oracle/go
go mod verify
go test ./...
cd ../..
python tool/verify_oracle_archive.py
```

Do not update `oracle/go/go.mod`, `go.sum`, or archived source in a routine dependency PR. Vulnerabilities in the non-shipping oracle are reported separately and do not justify silently changing the frozen reference.

## Dependency changes

- Keep dependency additions narrow and compatible with every declared target.
- Commit the relevant lockfile changes.
- For Gradle changes, also update dependency verification metadata intentionally:

```shell
cd apps/usque_gui/android
./gradlew --no-daemon :app:dependencies --write-locks
./gradlew --no-daemon --write-verification-metadata sha256 help
```

- Review every new artifact and checksum; CI and release workflows must never generate lock or verification metadata.
- Dependabot PRs receive the same review, CI, Build, and security gates as human contributions.
- A temporary vulnerability exception must name the advisory, explain why it is not currently exploitable, and include an expiry date.

## Change-specific acceptance

- Protocol changes need unit/property tests and a Go-oracle interoperability fixture.
- Parsers and frame codecs need malformed-input tests; externally reachable parsers need fuzz coverage.
- TUN, route, DNS, WFP/firewall, system-proxy, sleep/wake, update, installer, and uninstall changes need cleanup and leak-prevention tests in an isolated environment.
- New logs and diagnostics must be reviewed for Secrets, tokens, keys, licenses, pins, device identifiers, and sensitive addresses.
- UI changes must support English and Simplified Chinese, light/dark themes, keyboard focus, screen readers, 200% scaling, and Android TV D-pad navigation.
- Use Lucide icons; do not use Emoji as interface icons.
- Do not add WebView UI, an insecure TLS toggle, automatic telemetry, or automatic diagnostic upload.

## Quality policy

- Do not add blanket lint baselines, repository-wide suppressions, auto-fix CI jobs, or generated snapshots that conceal new findings.
- Do not reformat `oracle/`, `third_party/`, or generated sources as part of unrelated work.
- Do not introduce Detekt, mypy, clang-format, or another task runner solely to duplicate existing gates.
- If a required platform test cannot be run safely, state that limitation in the Pull Request instead of simulating a pass.
