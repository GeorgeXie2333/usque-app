# Contributing to Usque

Usque is still establishing its security-critical foundations. Discuss large protocol, privilege, storage, or UX changes before investing in a long implementation.

## Required checks by change scope

Run the checks that cover the code you touched. Prefer the aggregate host script when several languages change:

```shell
# Check-only multi-language gates (never rewrites files).
# Requires pinned tool versions: Ruff 0.16.0, PSScriptAnalyzer 1.25.0, Buf 1.72.0.
pwsh -NoProfile -File tool/check_source.ps1
```

### Rust

```shell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

### Flutter / Dart (`apps/usque_gui`)

```shell
cd apps/usque_gui
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub
```

Strict analysis is enabled (`strict-casts`, `strict-inference`, `strict-raw-types`, `directives_ordering`).

### Android / Kotlin (`apps/usque_gui/android`)

```shell
cd apps/usque_gui
flutter pub get --enforce-lockfile
flutter build apk --debug --config-only --no-pub

cd android
./gradlew --no-daemon :app:ktlintCheck
./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug
```

- ktlint is pinned via `org.jlleitschuh.gradle.ktlint` **14.2.0** and ktlint **1.8.0**.
- Kotlin compiler warnings are errors; Android lint treats warnings as errors.
- After changing Gradle dependencies or plugins, update lock files and verification metadata:

```shell
cd apps/usque_gui/android
./gradlew --no-daemon :app:dependencies --write-locks
./gradlew --no-daemon --write-verification-metadata sha256 help
```

### Python (`tool/*.py`)

```shell
# Ruff 0.16.0 is required (see pyproject.toml required-version).
pip install ruff==0.16.0
ruff check tool
python -m unittest discover -s tool -p "test_*.py" -v
```

`subprocess` security rules (`S603`/`S607`/etc.) may only be suppressed with a **per-line** `# noqa: Sxxx` that includes a short reason. Do not disable bandit/security rules globally.

### PowerShell (`tool/*.ps1`)

Every script under `tool/` must start with `[CmdletBinding()]`, call `Set-StrictMode -Version Latest`, and set `$ErrorActionPreference = 'Stop'`.

```shell
Install-Module PSScriptAnalyzer -RequiredVersion 1.25.0 -Scope CurrentUser -Force
# Warning and Error both block (see tool/PSScriptAnalyzerSettings.psd1).
Invoke-ScriptAnalyzer -Path tool -Recurse -Settings tool/PSScriptAnalyzerSettings.psd1
```

### Protocol Buffers (`proto/`)

```shell
# Buf 1.72.0 is required (see buf.yaml).
buf lint
buf format --exit-code --diff   # check-only; use `buf format -w` to apply pure format
```

On pull requests, CI also runs a **breaking** check (`FILE` policy) against the target branch. Pure formatting is allowed; do **not** change field numbers or wire shape without an intentional, reviewed protocol change.

### GitHub Actions workflows

```shell
# actionlint v1.7.12 (same pin as CI workflow-lint job)
actionlint -no-color
```

### Windows MSI authoring

```shell
dotnet tool restore
# See .github/workflows/ci.yml windows-installer-authoring job for the full
# fixture compile + tool/verify_windows_msi.ps1 + ICE validation flow.
```

### Archived Go oracle

The archived Go project under `oracle/go` is a behavioral oracle. It must remain buildable and its attribution must remain intact:

```shell
cd oracle/go
go test ./...
python tool/verify_oracle_archive.py
```

## Quality policy

- **No blanket lint baselines.** Do not add global ignore files, repo-wide `# noqa`, baseline XML, or CI auto-fix jobs that hide new findings. Fix issues or use a narrowly scoped, justified suppression at the finding site.
- Do not introduce Detekt, mypy, clang-format, or additional task runners for these gates.
- Do not reformat `oracle/`, `third_party/`, or generated sources as part of routine quality work.
- **Rust `unsafe`:** every `unsafe` block needs a `// SAFETY:` comment explaining why the invariants hold. Public APIs that are `unsafe` must document `# Safety` requirements in their rustdoc.

## Change requirements

- Protocol changes need unit/property tests and a Go-oracle interoperability fixture.
- Parser and frame changes need malformed-input tests; externally reachable parsers need fuzz coverage.
- TUN, route, DNS, firewall, system-proxy, sleep/wake, update, and uninstall changes need cleanup and leak-prevention tests.
- New logs must be reviewed for secrets, tokens, keys, licenses, pins, and sensitive addresses.
- UI changes must work in English and Simplified Chinese, light and dark themes, keyboard focus, 200% scaling, and Android TV D-pad navigation.
- Use Lucide icons. Do not add emoji as interface icons.
- Do not introduce WebView UI or an insecure TLS toggle.

Keep dependency additions narrow, pinned through the relevant lockfile, and compatible with every declared target.
