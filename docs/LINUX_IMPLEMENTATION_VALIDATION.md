# Linux implementation validation — 2026-10-18

This record covers the Linux desktop implementation in the working tree based
on `403a739f19dd4494ce8bcff2f4d861e4c3cc7f9d` (`dev`, version `0.3.2`). The
previous working tree was backed up before fetching and fast-forwarding to this
base, then its Linux changes were reapplied. The upstream transport, endpoint
and Chrome 155 changes were retained. No commit, release tag, installation or
publication was performed.

The implementation is uncommitted. The base commit alone does not identify the
complete tested source; this is a local engineering record, not exact-candidate
release evidence. A staged release must run its own required build and package
checks. Existing Windows/Android historical records do not certify this Linux
extension.

## Environment and scope

The workstation is Ubuntu 26.04.1, amd64. Rust is `1.97.1`; Flutter is `3.44.7`
at `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, resolved from the ignored local
SDK configuration. Android checks use the repository's pinned NDK and CMake.
This host is neither the Ubuntu 22.04 package baseline nor an isolated network
test environment.

[Linux desktop](LINUX_DESKTOP.md) maps each Windows feature to its Linux
implementation and the native acceptance it still needs. Source changes cover
the real desktop entry point, authenticated Unix IPC, Secret Service, native
Agent backend, shared transports and chains, desktop integration, package and
update helpers, and the two-architecture release contract. The independent
memory-only preview remains a separate debug application.

Only safe compile, static and deterministic checks were run. Tests use fake
network backends, temporary files/sockets and loopback listeners. No real TUN,
route, firewall, system DNS, session proxy or keyring state was changed. No
package was installed and no Android emulator was provisioned.

## Completed checks

Commands below are the change-scoped commands from
[CONTRIBUTING.md](../CONTRIBUTING.md), with working directories noted. The
aggregate source check supplements the tests and builds; it does not replace
them. Final check counts are recorded after the last source changes.

| Area | Exact command | Result |
| --- | --- | --- |
| Source aggregation | `pwsh -NoProfile -File tool/check_source.ps1` | Passed; Rust/Dart/Kotlin/Python/PowerShell/Buf static checks |
| Rust formatting | `cargo fmt --all --check` | Passed |
| Rust static checks | `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| Rust deterministic tests | `cargo test --workspace --all-targets --locked` | Passed: 1616 tests, 3 ignored, 24 suites |
| Linux sidecars | `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` | Passed on the amd64 workstation |
| Python tool pin | `pip install ruff==0.16.0` | Passed |
| Python lint | `ruff check tool` | Passed |
| Python format | `ruff format --check tool` | Passed |
| Python tests | `python -m unittest discover -s tool -p "test_*.py" -v` | Passed: 136 tests |
| Linux package fixtures | `python -m unittest discover -s tool -p "test_linux_package.py" -v` | Passed: 18 tests |
| Release-note renderer | `python -m unittest discover -s tool -p "test_release_contract.py" -v` | Passed: 33 tests |
| Protobuf lint | `buf lint` | Passed |
| Protobuf format | `buf format --exit-code --diff` | Passed |
| Protobuf compatibility | `buf breaking --against '.git#branch=origin/dev'` | Passed, including appended Linux diagnostics fields |
| Actionlint pin | `go install github.com/rhysd/actionlint/cmd/actionlint@914e7df21a07ef503a81201c76d2b11c789d3fca` | Passed |
| Workflow validation | `actionlint -no-color` | Passed |
| Repository policy | `python tool/check_repository_policy.py` | Passed |
| Whitespace | `git diff --check` | Passed |

Flutter commands run in `apps/usque_gui` with the pinned SDK:

| Exact command | Result |
| --- | --- |
| `flutter pub get --enforce-lockfile` | Passed |
| `dart format --output=none --set-exit-if-changed lib test` | Passed |
| `flutter analyze --no-pub` | Passed |
| `flutter test --no-pub --exclude-tags golden` | Passed: 1078 tests, 1 skipped |
| `flutter build linux --release --no-pub -t lib/main.dart` | Passed: production amd64 release |
| `flutter build linux --debug --no-pub -t lib/main_preview.dart` | Passed: independent preview debug build |

The native fake-socket fixture runs from the repository root:

| Exact command | Result |
| --- | --- |
| `cmake --build apps/usque_gui/build/linux/x64/release --target usque_linux_socket_test` | Passed |
| `apps/usque_gui/build/linux/x64/release/runner/usque_linux_socket_test` | Passed |

Android checks retained from the cross-platform matrix:

| Working directory | Exact command | Result |
| --- | --- | --- |
| Repository root, PowerShell | `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed |
| `apps/usque_gui` | `flutter build apk --debug --config-only --no-pub` | Passed; configuration only |
| `apps/usque_gui/android` | `./gradlew --no-daemon :app:ktlintCheck` | Passed |
| `apps/usque_gui/android` | `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed |

## Deterministic coverage and security review

- IPC fixtures cover peer UID/PID, private directories and socket modes,
  bounded frames, descriptor shape/ownership, connection expiry and total
  request deadlines. Agent authorization also verifies the root-owned installed
  Engine and rejects unsafe executable mappings or injected loaders.
- Agent tests cover stale generations, conflicting users, exact socket leases,
  deferred chain setup, protected replacement, orphan reattachment, bounded
  recovery retries, corrupt journals and failures between resource steps.
  Restores compare resource ownership and retain recovery intent when cleanup
  cannot be confirmed. There is no global firewall flush.
- DNS and session-proxy fixtures preserve unrelated later changes. Keyring
  errors remain retryable without plaintext fallback. Startup recovery does not
  prevent the GUI from opening its authenticated Engine channel.
- Desktop fixtures cover production/preview separation, capability-driven
  controls, tray loss, maintenance errors and translations. Secrets copied to
  the clipboard use an authenticated response and expiration; diagnostic and
  debug representations do not serialize their bytes.
- Signed-update tests reject altered bytes, wrong keys, architecture errors,
  downgrade attempts and mixed installation formats. Root maintenance copies
  and independently verifies inputs before use. File-transaction fixtures
  exercise interrupted replacement/removal, corrupt backups, external edits,
  restoration retries and committed-journal cleanup.
- Package fixtures inspect ELF identity/baseline rules, payload permissions,
  symlinks, preview exclusion, maintainer hooks and an inert deb archive
  round-trip. RPM fixtures verify both architecture headers, all script bodies
  and interpreters, and rejection of extra transaction/trigger scripts. The
  release contract retains schema-1 compatibility for existing
  Windows/Android updaters while adding six Linux packages and signed metadata.
- AppImage fixtures cover isolated GTK schema compilation, static resource
  paths, escaping links, distribution package ownership and required copyright
  texts. An additional temporary-directory check copied and compiled the host's
  GTK data without launching a desktop. This does not replace baseline package
  or real-session acceptance.

The repository contains the Linux public verification key only. Tests use
ephemeral identities; official private signing material was not used for local
builds or tests. The protected release workflow remains the only source of
official packages. Logs, generated binaries and local backup material remain
outside tracked source.

## Checks not run

| Check | Status | Reason / required environment |
| --- | --- | --- |
| Native Ubuntu 22.04 amd64 build and all three complete packages | `not_run` | Workstation is Ubuntu 26.04.1; hosted baseline job has not run |
| Native Ubuntu 22.04 arm64 build and all three complete packages | `not_run` | No native arm64 host |
| Real GNOME/KDE sessions on X11/Wayland, locked/unavailable Secret Service, tray host disappearance and login autostart | `not_run` | Deterministic fixtures do not establish desktop-session integration |
| Debian/Ubuntu/Fedora/AppImage installation, upgrade and uninstall | `not_run` | Requires the distinct Linux snapshot VM; no workstation installation |
| Native TUN, nftables, resolved/NetworkManager, crash, reboot, sleep, multi-user ownership and network switching | `not_run` | No isolated Linux snapshot plus independent management channel |
| Externally observed IPv4/IPv6, DNS, Kill Switch, DIRECT and all chain exits | `not_run` | No independent network observer or connected isolated candidate |
| Windows helper-based Rust Clippy/tests/release | `not_run` | No Windows MSVC host |
| Complete Windows Flutter/widget/exact golden/runner release sequence | `not_run` | No Windows host; Linux widget results do not substitute |
| Windows installer/native lifecycle checks | `not_run` | No matching Windows host or isolated snapshot VM |
| Android real-device VPN lifecycle and release APK signing/content checks | `not_run` | No dedicated device; release APK build not requested |
| Hosted CI, protected signing, release staging and provenance publication | `not_run` | No push or release workflow invocation |

These results establish local source/build and deterministic behavior only.
They do not establish six-package portability, native restoration or leak
freedom. Protected-environment reports remain supplemental and non-blocking;
their absence does not create a new release prerequisite or waive ordinary
build, package and approval gates.

## Review fixes and revalidation — 2026-10-18

The preceding results record the initial implementation. A subsequent review
confirmed seven defects and this addendum records their fixes. The source is
still an uncommitted working tree on the same base, so this remains local
engineering evidence rather than an exact-candidate release report.

- nftables inspection now compares equivalent typed protocol predicates while
  retaining address-family, UID, mark, address, port and rule-order checks.
  Tests include normalized readback and deleted or altered guard conditions.
- IPv6 route inspection normalizes only the kernel's implicit loopback OIF on
  throw/unreachable routes. Other interfaces, route types, selectors and
  duplicate attributes remain significant.
- Disconnect reconciles the Agent's actual source/target operation, including
  a rejected replacement and a completed rollback whose acknowledgement was
  lost. Failed IPC streams are closed; recovery reconnects with the original
  root ownership and peer checks, then queries state without replaying a
  mutation. Tests use real temporary Unix socket pairs for EOF, late replies,
  mismatched request IDs and stale operation generations.
- System-proxy startup failures stop the runtime and enter a retryable error
  state. The installed Engine retains the user's `XDG_DATA_HOME` alongside
  the other XDG paths without inheriting loader/plugin injection variables.
- The packaged desktop entry declares `com.cloudflare.warp`, matching the
  Zero Trust callback. deb/rpm declare `desktop-file-utils` and refresh the
  desktop MIME cache on installation and removal. The MIME fixture updates
  only a temporary applications directory, never the host association cache.
- AppImage updates distinguish a committed service transaction, a confirmed
  rollback and unresolved recovery. Cleanup after a durable commit cannot
  restore only the old GUI. Unknown helper outcomes retain both AppImages;
  confirmed rollback or authorization cancellation restores the old image.
  Recovery retries sync the restored directory before recording rollback,
  including already-restored names and absent original parent directories.
  Untrusted directories and external changes remain rejected.

An independent follow-up review checked the networking normalization and
recovery changes. It found additional lost-acknowledgement, disconnected-IPC
and directory-persistence cases; those were fixed before the final Rust run.
These tests do not establish native firewall, routing or desktop behavior.

The final command results for this fix pass are:

| Working directory | Exact command | Result |
| --- | --- | --- |
| Repository root | `bash tool/dev.sh doctor` | Passed; verified the pinned Flutter SDK from local properties |
| Repository root | `cargo fmt --all --check` | Passed |
| Repository root | `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| Repository root | `cargo test --workspace --all-targets --locked` | Passed: 1641 tests, 3 ignored, 24 suites |
| Repository root | `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` | Passed on the amd64 workstation |
| Repository root | `pwsh -NoProfile -File tool/check_source.ps1` | Passed |
| Repository root | `pip install ruff==0.16.0` | Passed; pinned version already installed |
| Repository root | `ruff check tool` | Passed |
| Repository root | `ruff format --check tool` | Passed |
| Repository root | `python -m unittest discover -s tool -p "test_*.py" -v` | Passed: 137 tests |
| Repository root | `python -m unittest discover -s tool -p "test_linux_package.py" -v` | Passed: 19 tests |
| Repository root | `desktop-file-validate packaging/linux/io.github.georgexie2333.usque.desktop` | Passed |
| `apps/usque_gui` | `flutter pub get --enforce-lockfile` | Passed |
| `apps/usque_gui` | `dart format --output=none --set-exit-if-changed lib test` | Passed |
| `apps/usque_gui` | `flutter analyze --no-pub` | Passed |
| `apps/usque_gui` | `flutter test --no-pub --exclude-tags golden` | Passed: 1078 tests, 1 skipped |
| `apps/usque_gui` | `flutter build linux --debug --no-pub -t lib/main_preview.dart` | Passed |
| `apps/usque_gui` | `flutter build linux --release --no-pub -t lib/main.dart` | Passed |
| Repository root | `cmake --build apps/usque_gui/build/linux/x64/release --target usque_linux_socket_test` | Passed |
| Repository root | `apps/usque_gui/build/linux/x64/release/runner/usque_linux_socket_test` | Passed |
| Repository root, PowerShell | `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed |
| `apps/usque_gui` | `flutter build apk --debug --config-only --no-pub` | Passed; configuration only |
| `apps/usque_gui/android` | `./gradlew --no-daemon :app:ktlintCheck` | Passed |
| `apps/usque_gui/android` | `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed |
| Repository root | `python tool/check_repository_policy.py` | Passed |
| Repository root | `git diff --check` | Passed |

The desktop-file utilities were downloaded from the configured Ubuntu package
source, checked against its package SHA-256 and extracted under `/tmp`; they
were not installed. Their extracted `usr/bin` directory was added to the test
process PATH, so the final MIME and Python runs had no skipped dependency
fixtures. The native IPC fixture was run immediately after the production
build because Flutter shares generated target configuration with the preview.

The unavailable checks listed above remain `not_run`: native Ubuntu 22.04
amd64/arm64 packaging, real GNOME/KDE sessions, Windows native/exact-golden
gates, and isolated installation, VPN, leak and recovery tests. No package was
installed and no service or VPN session was started. Tests did not mutate host
routes, firewall, DNS, system proxy or keyring state.
No official signing material was accessed, and no commit or publication was
performed.

## Independent Linux golden coverage — 2026-10-18

The Linux screenshot suite now has 16 reviewed baselines in
`apps/usque_gui/test/goldens/linux/`, exercised by
[linux_desktop_golden_test.dart](../apps/usque_gui/test/linux_desktop_golden_test.dart).
They were captured and compared locally on Ubuntu 26.04.1 amd64 with the pinned
Flutter 3.44.7 SDK. This is still the uncommitted working tree on the base
identified above, not evidence for a staged release candidate.

The scenes cover idle/connected/error home states, expanded IPv4/IPv6 details,
200% text, service installation/repair/version mismatch/cancelled authorization,
an unavailable tray, onboarding permissions and keyring cancellation, Persian
Zero Trust with RTL, Chinese routing rules with Ads, a selected WireGuard chain,
and multiple accounts. They use the Linux platform branch, fixed clocks and
simulated Engine/service responses. State and viewport assertions ensure the
controls and states under test are actually visible in the captured image.

Visual review found and corrected two fixture defects before accepting these
baselines: asynchronous settings readback replaced the routing/chain fixtures
with defaults, and Flutter's built-in test font intercepted some Chinese
characters before the intended Noto fallback. The latter is handled only in the
test font loader; production styles remain unchanged. Font fixtures have fixed
versions, SHA-256 checks, licenses and reproduction scripts documented in
[the font fixture reference](../apps/usque_gui/test/fonts/README.md). CJK subsets
and the empty missing-family font were regenerated with FontTools 4.60.1 and
verified byte-for-byte reproducible. All 16 screenshots were inspected, and
the affected Chinese and routing/chain scenes were inspected again after fixes.

The global golden comparator preserves Flutter's exact pixel comparison and
failure images. It rejects cross-platform reads and updates, malformed paths,
and unsupported host architectures. Five deterministic tests cover that guard.
All 111 existing Windows PNGs retained their SHA-256 values. The Windows golden
job and its command remain in place. The Ubuntu CI job now checks the expected
amd64 architecture, compares Linux goldens and uploads failure images; it never
updates baselines. `bash tool/dev.sh check flutter` includes Linux goldens on
amd64 and explicitly reports them as `not_run` on other architectures.

| Working directory | Exact command | Result |
| --- | --- | --- |
| Repository root | `bash tool/dev.sh doctor` | Passed; pinned Flutter version and commit verified |
| `apps/usque_gui` | `flutter pub get --enforce-lockfile` | Passed |
| `apps/usque_gui` | `dart format --output=none --set-exit-if-changed lib test` | Passed; 258 files unchanged |
| `apps/usque_gui` | `flutter analyze --no-pub` | Passed |
| `apps/usque_gui` | `flutter test --no-pub test/platform_golden_comparator_test.dart` | Passed: 5 tests |
| `apps/usque_gui` | `flutter test --no-pub --exclude-tags golden` | Passed: 1083 tests, 1 skipped |
| `apps/usque_gui` | `flutter test --no-pub test/linux_desktop_golden_test.dart --update-goldens --reporter expanded` | Passed: generated 16 Linux baselines, subsequently visually reviewed |
| `apps/usque_gui` | `flutter test --no-pub --tags linux-golden --reporter expanded` | Passed: 16 exact comparisons after the final fixture fixes |
| `apps/usque_gui` | `flutter test --no-pub --tags linux-golden` | Passed: 16 exact comparisons in a second process |
| Repository root | `pip install ruff==0.16.0` | Passed; pinned version already installed |
| Repository root | `ruff check tool` | Passed |
| Repository root | `ruff format --check tool` | Passed |
| Repository root | `python -m unittest discover -s tool -p "test_*.py" -v` | Passed: 139 tests, no skips |
| Repository root | `go install github.com/rhysd/actionlint/cmd/actionlint@914e7df21a07ef503a81201c76d2b11c789d3fca` | Passed |
| Repository root | `actionlint -no-color` | Passed |
| Repository root | `pwsh -NoProfile -File tool/check_source.ps1` | Passed, including Rust Clippy, Flutter analysis, Kotlin lint, PowerShell and Buf checks |
| Repository root | `ruff check apps/usque_gui/test/fonts` | Passed |
| Repository root | `ruff format --check apps/usque_gui/test/fonts` | Passed |
| Repository root | `python tool/check_repository_policy.py` | Passed |
| Repository root | `git diff --check` | Passed |

The Python run used the previously extracted temporary desktop-file utilities,
as described above. This addition changes test infrastructure, fixtures and
documentation; production source and package contents are unchanged by the
golden work. The preceding Rust/platform build results remain separate records.

Hosted Ubuntu 24.04 CI execution, Linux arm64 rendering, Windows golden execution
and native GNOME/KDE/X11/Wayland checks remain `not_run` locally. The screenshots
do not capture native window decorations or establish service, keyring, tray,
installation or VPN behavior. No package was installed and no system setting or
credential store was changed by these tests.

## Pre-commit review and revalidation — 2026-10-18

This review covers the same base and the complete working tree, including the
Linux golden additions above. The checks were run before the requested commit
and push; these local results remain separate from hosted CI and release
candidate evidence. Earlier sections retain their original counts and scope.

Two additional issues were corrected during review:

- Missing Linux VPN capability disabled the TUN switch even when TUN was
  already enabled by the default profile. An AppImage without the installed
  service therefore could not switch to local-proxy-only operation. Home,
  Proxy and tray controls now permit disabling an enabled output after its
  capability becomes unavailable, while preventing re-enabling it and keeping
  the existing operation locks. The same rule covers system-proxy capability
  loss. Three regression tests fail with the original gates and pass with the
  correction; they also verify that HTTP and SOCKS5 remain enabled.
- An empty line split the Linux download rows from the release-note table.
  The rows now belong to the same table as the Windows and Android downloads.

The final staged diff check also caught whitespace in two newly added files.
An extra C++ trailing blank line and one trailing space in the copied font
license were removed. The license wording is unchanged; its recorded SHA-256
and reproduction note were updated. Native socket tests and documentation
checks were repeated after this cleanup.

Review also checked the Agent authorization and ownership boundaries, recovery
journals, signed maintenance inputs, cleanup behavior and diagnostic privacy.
The output-control correction only changes saved preferences through the
existing settings lifecycle; it adds no privileged operation. The tests use
fake engines and native socket fixtures. All 111 existing Windows golden PNGs
were compared with the base and remain byte-for-byte unchanged.

The following commands completed successfully in this review. Flutter checks
and both Linux GUI builds were repeated after the output-control correction.
Rust and Android source did not change after their checks. The SDK was resolved
from the ignored local configuration and verified against the CI version and
commit with `bash tool/dev.sh doctor`.

| Working directory | Exact command | Result |
| --- | --- | --- |
| Repository root | `cargo fmt --all --check` | Passed |
| Repository root | `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| Repository root | `cargo test --workspace --all-targets --locked` | Passed: 1641 tests, 3 ignored, 24 suites |
| Repository root | `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` | Passed |
| `apps/usque_gui` | `flutter pub get --enforce-lockfile` | Passed |
| `apps/usque_gui` | `dart format --output=none --set-exit-if-changed lib test` | Passed |
| `apps/usque_gui` | `flutter analyze --no-pub` | Passed |
| `apps/usque_gui` | `flutter test --no-pub test/linux_desktop_test.dart --plain-name 'unavailable Linux' --reporter expanded` | Passed: 3 regression tests |
| `apps/usque_gui` | `flutter test --no-pub --exclude-tags golden` | Passed: 1086 tests, 1 Windows-only test skipped |
| `apps/usque_gui` | `flutter test --no-pub --tags linux-golden` | Passed: 16 exact comparisons; no baselines regenerated |
| `apps/usque_gui` | `flutter build linux --debug --no-pub -t lib/main_preview.dart` | Passed |
| `apps/usque_gui` | `flutter build linux --release --no-pub -t lib/main.dart` | Passed |
| Repository root | `cmake --build apps/usque_gui/build/linux/x64/release --target usque_linux_socket_test` | Passed |
| Repository root | `apps/usque_gui/build/linux/x64/release/runner/usque_linux_socket_test` | Passed |
| Repository root, PowerShell | `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed |
| `apps/usque_gui` | `flutter build apk --debug --config-only --no-pub` | Passed; configuration only |
| `apps/usque_gui/android` | `./gradlew --no-daemon :app:ktlintCheck` | Passed |
| `apps/usque_gui/android` | `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed |
| Repository root | `pip install ruff==0.16.0` | Passed; pinned version already installed |
| Repository root | `ruff check tool` | Passed |
| Repository root | `ruff format --check tool` | Passed |
| Repository root | `python -m unittest discover -s tool -p "test_*.py" -v` | Passed: 139 tests, no skips |
| Repository root | `python -m unittest discover -s tool -p "test_linux_package.py" -v` | Passed: 19 tests, no skips |
| Repository root | `python -m unittest discover -s tool -p "test_release_contract.py" -v` | Passed: 33 tests after the template correction |
| Repository root | `desktop-file-validate packaging/linux/io.github.georgexie2333.usque.desktop` | Passed |
| Repository root | `buf lint` | Passed |
| Repository root | `buf format --exit-code --diff` | Passed |
| Repository root | `go install github.com/rhysd/actionlint/cmd/actionlint@914e7df21a07ef503a81201c76d2b11c789d3fca` | Passed |
| Repository root | `actionlint -no-color` | Passed |
| Repository root | `ruff check apps/usque_gui/test/fonts` | Passed |
| Repository root | `ruff format --check apps/usque_gui/test/fonts` | Passed |
| Repository root | `pwsh -NoProfile -File tool/check_source.ps1` | Passed again after the correction |
| Repository root | `python tool/check_repository_policy.py` | Passed |
| Repository root | `git diff --check` | Passed |

The Python and desktop-file checks used the verified temporary extraction of
desktop-file-utils described above. No package was installed, no service or
VPN was started, and no host network or credential state was changed.

The unavailable checks in the earlier table remain `not_run` locally, including
Ubuntu 22.04 amd64/arm64 package builds, Windows native and exact-golden gates,
native desktop sessions and isolated install/network/recovery validation.
Hosted CI results must be checked separately after pushing. Linux widget and
golden passes do not substitute for those environments.
