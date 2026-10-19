# Proton VPN native validation / 原生验证记录

Date: 2026-10-19. Status: **workstation validation completed; live acceptance not_run**.
This is the native implementation record;
it does not replace the [historical Python PoC record](PROTON_VPN_POC_VALIDATION.md).
The [product guide](PROTON_VPN.md) describes current behavior and limitations.

The first candidate below predates the Android request alignment. The final
section records that follow-up separately; its source fingerprint and results
must not be confused with the earlier candidate. The earlier unknown-8004
classification is superseded by the official `SessionNotAllowed` definition
documented in the [protocol reference](PROTON_VPN_PROTOCOL.md).

| Candidate identity | Value |
| --- | --- |
| Baseline commit | `cf4c4c517649ed2e8f194c8217e0fb7f8714d5a2` |
| Tested source commit / complete working-tree manifest | Uncommitted working tree; 5,504 entries; manifest SHA-256 `2cb1f3ee595ac05d50e8fac88a8e901e1b71ac829ea908a3fcc1056d6a9ac012` |
| OS, architecture and toolchain versions | WSL2 Linux `6.18.33.2`, x86_64; Rust/Cargo `1.97.1`; Flutter `3.44.7` / `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`; Dart `3.12.2`; Java `17.0.20.1`; Android NDK `29.0.14206865`, SDK CMake `3.22.1`; Python `3.14.4`, Ruff `0.16.0`, Buf `1.72.0`, Node `22.22.1` |
| Final command log and sanitized evidence identity | Local `/tmp/usque-proton-*` logs; ordinary tests use synthetic credentials only, no raw live account or challenge evidence |

The baseline alone does not identify this candidate. The source-manifest digest
below includes the uncommitted implementation and fixtures. A partial targeted
run, mock or syntax check cannot establish a complete platform gate.

## Workstation checks / 开发机检查

The applicable commands follow [CONTRIBUTING](../CONTRIBUTING.md). Passed means
the command completed successfully on this host; missing infrastructure remains
`not_run`. No package installation, TUN, route/DNS/firewall/system-proxy change,
emulator, real browser challenge or live VPN session was used for these checks.

| Scope / command group | Final result | Evidence and limits |
| --- | --- | --- |
| `cargo fmt --all --check` | passed | Final source |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed | Includes production and test paths; existing generated BoringSSL binding warnings remain dependency warnings |
| `cargo test --workspace --all-targets --locked` | passed | 1,752 passed, 0 failed, 3 intentionally ignored; final repeated run recorded below |
| `cargo check -p usque-transport --locked --no-default-features` | passed | Default/WireGuard paths also compiled by workspace checks |
| `cargo check -p usque-android --locked --no-default-features` | passed | No new disabled-feature warnings |
| Proton SRP/key/certificate tests | passed | Workspace suite: MIT vectors, malformed signature/parameters, chain/key/time checks |
| Proton API/workflow tests | passed | Synthetic HTTP only; exact HV retry, cleanup, cancellation before commit, offline local forget and polling after catalogue errors |
| Proton runtime/local-agent/renewal tests | passed | Memory TCP/mTLS, 1,216-byte MTU and segmented certificate chain, framing/status, fragments, rotation and cancellation |
| Proton core encrypted-store and lifecycle tests | passed | AEAD/AAD, bounds, links, leases, journal, atomic failure, real child-process lock exclusion and account-snapshot races; no power-loss proof |
| Locked Flutter resolution, Dart format/analyze | passed | 265 Dart files, zero format changes; no analyzer findings |
| Flutter widgets | passed | 1,132 passed, 1 pre-existing skip; fake Engine |
| Linux platform goldens | passed | 19 passed; three new Proton images and one updated picker image visually reviewed; fixed CJK fixture extended by five characters |
| Linux debug preview and production release compile | passed | Preview cannot launch native verification; real Rust sidecars also compile in release mode |
| Linux native socket and Proton policy executables | passed | Socket fixture needs allowed loopback binding; sandbox-denied attempt was retried successfully; no TUN |
| Linux package contract fixtures | passed | 19 passed, 1 skipped because desktop-file-utils is unavailable; no package installed |
| Windows helper Rust and complete Flutter/runner/golden gates | not_run | Linux host; Windows-target syntax checks are supplemental only. New Proton Windows golden and changed picker baseline still require a Windows host |
| Android pinned-NDK Rust Clippy | passed | `RUST_ANDROID_CLIPPY_OK=arm64-v8a`; does not establish three-ABI final coverage |
| Android configuration, Kotlin lint/unit/Android lint | passed | 474 unit tests, zero failures/errors/skips; no emulator or device installation |
| Python/Ruff | passed | 268 passed, 1 pre-existing skip; 55 files formatted; includes four generated native-bridge VM tests |
| Buf lint and format | passed | Append-only protobuf additions |
| Aggregate source checks | passed | `All source quality checks passed.`; PSScriptAnalyzer 1.25.0 reported no Error/Warning or casing findings |
| `python tool/check_repository_policy.py` | passed | Links/UTF-8/repository rules only |
| `git diff --check` | passed | Whitespace only |

### Source and evidence identity

The local manifest is `/tmp/usque-proton-native-source-manifest.json` (1,079,565
bytes). It covers the sorted union of `git ls-files -z` and
`git ls-files --others --exclude-standard -z`, preserving each relative path,
regular-file SHA-256/length/executable flag, symlink target, or tracked deletion.
Only this validation record is excluded to avoid a recursive digest. The
version-1 manifest includes the baseline commit and exclusion list, encoded as
UTF-8 JSON with sorted keys, compact separators, ASCII escapes and one final
newline. Ignored local SDKs, credentials, JNI libraries and build outputs are
outside this source identity. The manifest was retained locally, not uploaded.

Final local logs include `/tmp/usque-proton-rust-tests-final-exact.log`,
`/tmp/usque-proton-clippy-final.log`,
`/tmp/usque-proton-linux-rust-release-final.log`,
`/tmp/usque-proton-android-rust-final.log`,
`/tmp/usque-proton-android-tests-exact.log`,
`/tmp/usque-proton-python-tests-final.log`,
`/tmp/usque-proton-flutter-widgets-final.log`,
`/tmp/usque-proton-flutter-goldens-compare-final.log`, and
`/tmp/usque-proton-aggregate-final.log`. These are local validation output,
not release or protected-lab evidence. Windows compilation/goldens and live
acceptance remain unverified regardless of these deterministic results.

The following commands were run from the repository root, using the pinned
toolchains and the temporary locked Python environment:

```shell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo check -p usque-transport --locked --no-default-features
cargo check -p usque-android --locked --no-default-features
cargo build --release --locked -p usque-engine -p usque-agent -p usque-update
pwsh -NoProfile -File tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy
pip install ruff==0.16.0
ruff check tool
ruff format --check tool
python -m unittest discover -s tool -p "test_*.py" -v
python -m unittest discover -s tool -p "test_linux_package.py" -v
buf lint
buf format --exit-code --diff
pwsh -NoProfile -File tool/check_source.ps1
python tool/check_repository_policy.py
git diff --check
```

From `apps/usque_gui`:

```shell
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub --exclude-tags golden
flutter test --no-pub --tags linux-golden
flutter build linux --debug --no-pub -t lib/main_preview.dart
flutter build linux --release --no-pub -t lib/main.dart
flutter build apk --debug --config-only --no-pub
```

From `apps/usque_gui/android`, with `USQUE_ANDROID_ABI=arm64-v8a` for the final
JNI compile-only dependency of the Kotlin gates:

```shell
./gradlew --no-daemon :app:ktlintCheck
./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug
```

The final Kotlin gate uses the matching dedicated JNI signature, including the
expected WARP profile ID; it does not exercise JNI on a device. Earlier KTX and
spacing failures were corrected and affected checks rerun. The initial Rust
regression run exposed a stream/DNS generation mismatch, which was fixed by
keeping stream-pool generations separate from the Proton control-job epoch;
the complete workspace suite then passed. No check was weakened to accept a
failure.

### Cleanup, fail-closed and privacy review

Configuration writes and provider jobs share a stable cross-process lease.
Mutations either wait for owned cleanup or fail busy; secret/runtime account
snapshots must match before a borrowed underlay is used. Token rotation persists
its pending journal before the only refresh request; uncertain results require
reauthentication. Runtime expiry and agent revocation close business admission.
Failed cleanup preserves an explicit unknown-remote-state result, including
after cancellation. Local forget explicitly does not promise remote revocation.
The existing privileged apply/disconnect lifecycle owns platform changes;
workstation fixtures cannot prove its restoration or leak properties.

Passwords, TOTP and verification handoffs use separate bounded byte channels
and are cleared after use; persistent tokens/keys/certificates use the platform
cipher. Ordinary status, diagnostics and error strings expose only allowlisted
metadata and numeric API/HTTP failures. No real credentials were supplied to
this test run. Android's default WebView profile and the verification page's
system-network path remain the documented limitations below.

Additional pure native policy targets are available after configuring the runner:

```shell
cmake --build apps/usque_gui/build/linux/x64/release --target usque_proton_verification_test
apps/usque_gui/build/linux/x64/release/runner/usque_proton_verification_test
```

Use `arm64` on a native Linux arm64 host. After the complete Windows runner
gates, run the matching pure policy fixture from the repository root:

```powershell
cmake --build apps/usque_gui/build/windows/x64 --config Release --target usque_proton_verification_test
& ./apps/usque_gui/build/windows/x64/tests/Release/usque_proton_verification_test.exe
```

Use the `arm64` directory for Windows arm64. These targets supplement the
complete platform gates in CONTRIBUTING; they do not replace them.
Offline generated-JavaScript VM checks validate top/current challenge, origin,
source, expiry and one-use behavior; they do not validate real WebView rendering,
page CSP or server acceptance. Record their harness/source identity if used.

## SDK and source provenance / SDK 与来源

| Item | Fixed source and boundary |
| --- | --- |
| Native SRP algorithm/vectors | ProtonMail/go-srp `d323688777afbb6265d8bc4a51419991a24630ca`, MIT; notice in Rust source, fixture and app license asset |
| Android/Core protocol fields | Pinned references in [protocol contract](PROTON_VPN_PROTOCOL.md); independent implementation, no official app SDK bundled |
| Windows WebView2 SDK | Microsoft.Web.WebView2 `1.0.4258.31`; headers/static loader only, installed runtime required |
| SDK package SHA-256 | `56f7f4b8bf9aee4b8efefbbdd4f67d5f74ebd1b100ed0806da71bf76af481aa9` |
| SDK digest provenance | Fixed bytes retrieved from the official HTTPS NuGet package; no independent official SHA-512 verification was obtained |
| SDK build policy | Hash-checked CMake extraction, optional local `USQUE_WEBVIEW2_NUPKG` still requires the same hash; no browser runtime installation |
| SDK license | [Microsoft notice](../apps/usque_gui/windows/runner/WebView2-LICENSE.txt), installed with the Windows payload |
| Linux WebView | Dynamic system WebKitGTK 4.1/4.0; mandatory ephemeral context and required API symbols, otherwise unavailable |
| Android WebView | App-default system WebView profile, normal open/close cookie and WebStorage cleanup; no ephemeral/process-death cleanup claim; per-job one-use message port |

## Official page bridge compatibility / 官方页面桥接兼容

The captured public bundle identifies WebView2 by `window.chrome.webview`, but
its `webview` result branch emits nothing. A generic top-level `message`
listener is therefore insufficient on Windows. The native implementation keeps
that platform detection unchanged, disables unsupported WebMessage transport,
and restricts the page to CAPTCHA with `embed=true`. It accepts only the
same-origin `/core/v4/captcha` iframe's `pm_captcha` message with matching unique
`Token`, `ForceWebMessaging=1`, exact `contentWindow`, current top challenge hash
and deadline. The raw iframe result maps to the embedded success token unchanged;
see the [source call-chain record](PROTON_VPN_PROTOCOL.md#native-captcha-bridge--原生拼图桥接).
A nonce envelope crosses the suppressed `ScriptDialogOpening` alert callback;
closing the WebView is queued after the synchronous event returns. Windows does
not fabricate an Android/iOS identity or modify the official bundle.

Linux and Android expose neither `AndroidInterface` nor an iOS handler, and use
the page's existing `web` branch. Android transfers a private message port into
the top-level page and retains only its app-owned endpoint. Native wall and
elapsed deadlines both gate acceptance; reloads/cancellation close owned ports.

[Permanent offline bridge tests](../tool/test_proton_native_bridge.py) execute
the generated C++ and actual Kotlin script template with Node WebCrypto. Their
independent official-interface fixture includes platform selection, iframe
submission, unchanged token mapping and the empty Windows `webview` branch.
This is a regression model derived from the pinned page, not execution of a
vendored official bundle, a real WebView or a successful server challenge.
Final test result: **pending**. No plaintext challenge or account is used.

## Live acceptance / 实网验收

| Scenario | Native result | Required evidence |
| --- | --- | --- |
| Guest authentication | not_run | Earlier Python HTTP 422 / API 8004 is separate historical evidence |
| Account SRP/TOTP authentication | not_run | No usable account supplied; no credentials in reports |
| Official puzzle rendering and manual completion | not_run | User completes the puzzle; no automation |
| Native successful-message delivery | not_run | Real top-frame challenge, correct job/session, one use, expiry |
| Verification WebView egress equals WARP API egress | not_run | WebView currently uses system routing; consistency is unproven |
| Server accepts verification and resumes the original request | not_run | One exact retry, no secret screenshots or raw HTTP trace |
| Certificate issue/refresh and remote session revocation | not_run | Actual server acceptance and bounded cleanup outcome |
| WireGuard plus local-agent final authorization | not_run | Configuration/handshake alone is insufficient |
| End-to-end TCP, UDP, DNS and observed final exit | not_run | Dedicated permitted network-observer evidence |
| Cancel, expiry, network-generation change, process death | not_run | Real platform lifecycle and remote cleanup outcome |
| Power loss during token-rotation persistence | not_run | Isolated fault injection; ordinary atomic-save tests are insufficient |
| Platform leak prevention and state restoration | not_run | Correct isolated environment and exact candidate |

A source-controlled implementation or synthetic success must not change these
live rows to passed. Treat API 8004 as an unknown rejection, distinct from an
actual 9001 human-verification challenge. No native login success, guest
eligibility, third-party identity acceptance or end-to-end tunnel has yet been
established by this record. Protected-runner work remains optional and must obey
the repository's isolation rules; its absence is never a pass.

## Android request alignment follow-up / Android 请求对齐补充

Date: 2026-10-19. Baseline: `8f23987f9699cb20be1bf3c7355d4a4c45c52ff2` plus
the uncommitted changes. The complete local source manifest is
`/tmp/usque-proton-android-alignment-source-manifest.json`: 5,509 entries,
1,086,111 bytes, SHA-256
`b0193fed9a6751e5b5e5053ea014f6c4234f9dbb5252b64198be6ba08ba32692`.
It covers the sorted union of tracked and nonignored untracked files, including
deletions, symlink targets, and regular-file SHA-256/length/executable flags.
Only this validation document is excluded to avoid a recursive fingerprint;
the canonical version-1 JSON has sorted keys, compact separators, ASCII escapes
and one final newline. SDKs, signing material, generated JNI and build output
remain ignored. This is local workstation evidence, not a staged release or
protected-runner result.

The Android profile follows the pinned Android `5.20.74.0` direct-APK identity
and the actual published Core `36.8.0` authentication/challenge sources, as
identified in the protocol reference. AppVersion and API User-Agent share the
same compatibility version. Language, timezone, model hash, font, storage,
theme, keyboards and root observations come from Android through private JNI.
The Kotlin encoder and Rust consumer use the same synthetic device fixture.
Sessions use the concrete Device payload; credentialless/SRP Device payloads
include the sealed-frame discriminator. TV client identity and UTF-16-to-ASCII
User-Agent replacement have deterministic tests.

The guest workflow verifies the credentialless response's UserID, refreshes
scopes, and checks the protected VPN permissions without requesting an ordinary
Core user. New logins no longer rotate tokens unnecessarily. Renewal preserves
the authenticated UserID when SessionResponse omits it and rejects a changed
UserID. Each API request can present its own manual challenge, with one exact
retry. Android automatically opens each job/challenge once; polling does not
reopen it, and cancellation invalidates late answers. API 8004 and 10200 remain
distinct from a 9001 challenge; errors show an allowlisted failed step and
numeric codes without raw response bodies.

The commands below completed with the repository's pinned SDKs. Flutter commands
run in `apps/usque_gui`; Gradle commands run in its `android` directory; other
commands run at the repository root. Local logs use the
`/tmp/usque-proton-align-` prefix.

| Exact check | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed |
| `cargo test --workspace --all-targets --locked` | passed: 1,763 tests; 3 existing ignores |
| `flutter pub get --enforce-lockfile` | passed |
| `dart format --output=none --set-exit-if-changed lib test` | passed: 265 files, no changes |
| `flutter analyze --no-pub` | passed: no findings |
| `flutter test --no-pub --exclude-tags golden` | passed: 1,140 tests; 1 existing skip |
| `flutter test --no-pub --tags linux-golden` | passed: 19 exact comparisons |
| `pwsh -NoProfile -File tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter build apk --debug --config-only --no-pub` | passed |
| `./gradlew --no-daemon :app:ktlintCheck` | passed |
| `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed: 476 tests, lint passed |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed |
| `python3 tool/check_repository_policy.py` and `git diff --check` | passed |
| `actionlint -no-color` | passed |
| Windows Rust, Flutter, runner and exact Windows golden gates | not_run: Linux workstation |
| Real Android authentication, puzzle, JNI lifecycle and final VPN exit | not_run: no device/emulator or live authentication used |

Cleanup and privacy review: the existing WARP binding, cross-process lifecycle
lease, token-rotation journal, session revocation and fail-closed tunnel gates
remain enforced. Guest permission checks still require valid authenticated
session fields and successful protected VPN authorization. API header identity
is stored only with the encrypted owned session for renewal/logout; the full
device frame is neither persisted nor exposed in status/diagnostics. Passwords,
TOTP and verification tokens retain their private, cleared channels. The
verification browser retains its normal platform User-Agent and system network
path; matching its egress to the WARP API remains unverified. Device verification
9002 and other unsupported authentication modes still stop explicitly. No
installation, signing, host-network mutation or protected-runner execution was
performed. These offline passes do not establish real Proton acceptance.
