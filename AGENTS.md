# Usque agent contract

## Scope and authoritative sources

This root file keeps repository-wide safety and operating rules. Do not move
critical safety guidance into a nested file.

Use the checked-in sources instead of duplicating mutable procedures:

- `CONTRIBUTING.md` is the required check and acceptance matrix.
- For Flutter layout or interaction changes, use `apps/usque_gui/README.md`
  and the existing theme and components. Select other technical documents
  by task using `docs/README.md`.
- Toolchain manifests, lockfiles, Gradle configuration, and build helpers define
  versions and local build behavior.
- `.github/workflows/ci.yml` and `.github/workflows/build.yml` define CI and
  compile-only gates.
- `.github/workflows/release.yml`, `tool/release_contract.py`, and
  `docs/CODE_SIGNING.md` define release and signing contracts.
- `tool/reliability_gate.py` and `docs/RELIABILITY_TESTING.md` define optional
  protected-runner validation and evidence contracts. Protected runners are
  supplemental and are not a publication prerequisite.

If sources conflict, apply the most restrictive safety rule. For executable
behavior, follow the helper or workflow and update stale documentation in the
same change. Never weaken a fail-closed check to make a local run pass.

## Development-machine safety boundary

On a normal development machine, never:

- install a generated MSI or use `usque-update.exe` to start an upgrade;
- run `usque-uninstall.exe` against a live ProductCode;
- start Windows VPN mode or create a TUN/Wintun session;
- apply WFP, route, interface-DNS, or system-proxy mutations;
- run `usque-agent --recover-state`, `--emergency-remove-kill-switch`, or the
  Engine's `--purge-user-data` merely to exercise a build;
- install or exercise a release APK on a personal or shared Android device;
- invoke `usque-reliability-runner`, provision its protected labels, or set
  `USQUE_ISOLATED_SNAPSHOT_VM=1` as a workstation workaround.

Keep the protected environments distinct:

- `usque-snapshot-vm`: Windows install, upgrade, connected uninstall, crash
  recovery, platform-state restoration, and Wintun lifecycle.
- `usque-android-device`: Android device, Doze, process, Always-on, Lockdown,
  reboot, upgrade, and TV lifecycle.
- `usque-network-observer`: externally observed IPv4, IPv6, DNS, Kill Switch,
  route, endpoint, and direct-rule leak behavior.
- `usque-performance-lab`: controlled repeated resource and performance
  sampling.

A label or environment variable alone is not proof of isolation. Windows
destructive tests additionally require a snapshot and independent management
channel; Android tests require a dedicated device or isolated emulator.

Safe workstation operations include source inspection, locked compile-only
builds, deterministic unit/static tests, SOCKS5 and HTTP loopback tests,
`usque-agent --validate-only`, MSI table/ICE verification, and
`usque-uninstall --dry-run` without a live ProductCode.

If an isolated test cannot run, report it as not run. Missing infrastructure
must never become a pass.

## Worktree and validation discipline

- Inspect the worktree and preserve unrelated user changes.
- Do not commit `apps/usque_gui/android/local.properties`, signing material,
  generated JNI libraries, build directories, logs, diagnostics, packet
  captures, or release artifacts.
- Treat `oracle/go` as a frozen interoperability reference. Do not update or
  reformat it during routine work. Do not reformat `third_party` or generated
  sources in unrelated changes.
- Reliability invariant identifiers and protobuf field numbers are append-only.
- Privileged networking, update, installer, uninstall, diagnostics, and release
  changes require cleanup, fail-closed, and privacy analysis.

Run every applicable check using the exact change-scoped commands in
`CONTRIBUTING.md`, including its Markdown-only section. The aggregate
`tool/check_source.ps1` is check-only and does not replace tests or platform
builds. Report commands, results, and anything not run. Do not claim a gate
passed unless its exact command completed successfully.

Within the user's requested scope, continue source edits and applicable
workstation-safe checks, fix failures introduced by the change, and rerun
affected checks without asking for confirmation at each step. The Flutter
widget and golden suites use a fake engine; they do not prove native VPN or
platform-state restoration behavior. Complete all remaining safe work when
isolated validation is unavailable, and record it as `not_run`. All isolation,
packaging, signing, and publication restrictions in this file still apply.

## Toolchains and dependency locking

For Codex cloud setup, follow `docs/CODEX_CLOUD.md`. Source
`.toolchains/codex-cloud.env.sh` before toolchain commands. A cloud container
is not a protected VPN or device test environment; all safety boundaries above
still apply. Keep this root contract in Git so fresh cloud checkouts receive it.

- Rust versions and targets come from `rust-toolchain.toml`. Cargo build, test,
  and Clippy operations must use `--locked`.
- Resolve Flutter from `apps/usque_gui/android/local.properties`, verify it
  matches the CI pin, and use `pub get --enforce-lockfile` followed by
  `--no-pub` checks/builds. Do not assume `flutter` is globally available.
- `tool/build_android_rust.ps1` enforces the pinned NDK revision from
  `source.properties` and the exact SDK CMake version. Do not override it with
  a different NDK.
- `tool/build_windows_rust_release.ps1` selects the supported CMake/Ninja and
  Visual Studio environment through `vswhere.exe`.
- Restore WiX through the checked-in .NET tool manifest; do not substitute a
  global version.

## Windows native build trap

Do not run a plain release Cargo build in a fresh Windows shell. With Visual
Studio 18 Build Tools, CMake 3.22 cannot name the Visual Studio 18 generator and
BoringSSL fails before compiling. Use `tool/build_windows_rust_release.ps1`
for Windows Rust Clippy, tests, and release builds, following the Windows Rust
and MSI authoring section of `CONTRIBUTING.md`.

When editing the helper, preserve these invariants:

- call the selected `vcvars*.bat` without `-arch` or `-host_arch`;
- retain PATH normalization, Ninja selection, and loadable `libclang.dll`
  discovery;
- retain imported MSVC/Windows SDK include forwarding for vendored
  `boring-sys` bindgen.

A cached binding is not clean-shell evidence. After a failed configure, clean
only `boring-sys` for the affected profile/target; never delete the whole
`target` tree.

For Flutter or Windows-runner changes, follow the complete Windows Flutter
and runner section of `CONTRIBUTING.md`, including its Windows Rust gates.
A build-only subset does not establish that validation passed.

## Packaging, Android, and release boundaries

Create a local validation MSI only when explicitly requested. First produce
fresh Rust and Flutter release artifacts from the same working tree after all
applicable checks. Pass the current SemVer shared by `Cargo.toml` and
`apps/usque_gui/pubspec.yaml`; `build_windows_local_validation.ps1` otherwise
copies whatever artifacts already exist.

The local packaging script temporarily creates and trusts a self-signed
identity. It may require approved certificate-store access and removes its key
and trust entries afterward. Never install, publish, or rename its output to
look official. Keep the custom installer UI; do not replace it with stock
`WixUI_InstallDir`.

Use `tool/build_android_rust.ps1` for JNI builds and never commit its generated
`jniLibs`. Release APK builds are signing-sensitive: run them only when
explicitly requested, mirror `.github/workflows/build.yml` with an ephemeral
build-only identity, and never use official release signing material locally.
Before delivery, verify ABI contents, absence of `kernel_blob.bin` and Vulkan
validation layers, and the signing-certificate identity.

Official packages come only from the approved tag workflow and exact staged
candidate. Local artifacts cannot replace a failed job. Do not access signing
secrets, move release tags, publish releases, or upload artifacts without an
explicit user request and satisfied approval gates.

Protected-runner execution and reports are optional, non-blocking validation
and must not gate publication. Their absence, `failed` or `not_run` status, or
an unavailable runner does not block publication. Never present such a result as
a pass. Any report used or published as release evidence must match the exact
candidate and come from the required isolated environment. Forged,
wrong-candidate, or wrong-environment evidence must be rejected. Restricted
packet captures and raw lab evidence stay only in a private repository or an
equivalently access-controlled store. The public repository always skips the
protected lab jobs and records them as `not_run`. Only allowlisted sanitized
evidence may be public.
