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

## Android verification completion follow-up — 2026-10-19

Baseline: `9dd4c5d94be6ad7c200f250d2d51444b054527a9`. The complete edited
source is identified by the canonical version-1 manifest SHA-256
`08b2dc05d9b6a46e9afbee4357cc49d7bd2d96dccc68ad221a8fad016bde1938`
(5,509 entries; 1,086,115 bytes). It covers tracked and nonignored untracked
files with regular-file hashes, lengths and executable flags, plus symlink
targets; only this validation document is excluded to avoid recursion. The
local manifest is `/tmp/usque-proton-captcha-cancel-source-manifest.json`.

The user reported that Android stayed in the foreground, the official puzzle
completed, and the window immediately disappeared with `Setup cancelled`.
The tested APK/source identity and a native event trace were not supplied, so
this observation does not identify the exact native callback that closed it.
The public verification bundle was fetched again and matched the SHA-256
already recorded in the [protocol reference](PROTON_VPN_PROTOCOL.md).

Android now accepts the challenge-bound official `pm_captcha` frame result
directly, alongside the top-level success message, using one shared one-use
state. It blocks stray top-level navigation without cancelling the bound
puzzle, and outside-dialog touches no longer cancel a drag. Completion keeps
an owned result copy while WebView destruction is deferred beyond the message
callback; the reply is delivered and the copy cleared after cleanup. Stale
callbacks cannot act on another window. No JavaScript interface was added.

Native expiry, navigation, TLS, bridge, load, download and lifecycle errors
are distinct from explicit Cancel/Back. Flutter retains a job-bound failure
through cleanup polling, displays only allowlisted verification error codes,
and independently preserves remote-revocation uncertainty. Synthetic tests
first reproduced the old `Setup cancelled` misreport and a lost top-level
success delivery; they pass with this change. They do not reproduce the
user's actual WebView callback sequence or establish that its cause is fixed.

The final commands completed with the repository's pinned toolchains. Flutter
commands run in `apps/usque_gui`, Gradle in its `android` directory, and all
other commands at the repository root. The existing optional PoC virtual
environment supplies HTTPX for the complete Python suite. Local logs use the
`/tmp/usque-proton-captcha-cancel-` prefix.

| Exact check | Result |
| --- | --- |
| `flutter pub get --enforce-lockfile` | passed |
| `dart format --output=none --set-exit-if-changed lib test` | passed: 265 files, no changes |
| `flutter analyze --no-pub` | passed: no findings |
| `flutter test --no-pub --exclude-tags golden` | passed: 1,143 tests; 1 existing skip |
| `flutter test --no-pub --tags linux-golden` | passed: 19 exact comparisons |
| `flutter build linux --debug --no-pub -t lib/main_preview.dart` | passed; binary not executed |
| `pwsh -NoProfile -File tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter build apk --debug --config-only --no-pub` | passed |
| `./gradlew --no-daemon :app:ktlintCheck` | passed |
| `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed: 476 tests, no skips; lint passed |
| `ruff check tool` and `ruff format --check tool` | passed |
| `python -m unittest discover -s tool -p "test_*.py" -v` | passed: 270 tests; 1 skip because desktop-file-utils is unavailable; includes 6 native bridge tests |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed |
| `python3 tool/check_repository_policy.py` and `git diff --check` | passed |
| `actionlint -no-color` | passed |
| Windows Rust, Flutter, runner and exact Windows golden gates | not_run: Linux workstation |
| Real Android WebView, touch/lifecycle behavior and Proton challenge resumption | not_run: no device/emulator used |

The first complete Python run could not bind its loopback handoff fixtures
inside the sandbox; the same suite passed with approved loopback access.
These fixtures do not contact the real authentication service. Challenge
origin, frame source/path/query, current token digest, allowed method, expiry
and single consumption remain enforced. No challenge or result token, raw
exception text, or unknown exception code enters ordinary status or logs.
The WebView keeps its default browser User-Agent and system network path;
its egress agreement with the WARP API remains unverified. No installation,
emulator, live authentication, VPN start, host-network mutation or release
packaging was performed.

## Account sign-in HTTP 401 recovery — 2026-10-19

Baseline: `34d219606c153045f53a29f543d74e2efd8ad180`. The complete edited
source is identified by canonical version-1 manifest SHA-256
`fc3e68885c8a772766518fc22ceb7613956d75967a0f3a540f633662fa847355`
(5,509 entries; 1,086,115 bytes), using the same tracked/nonignored source,
symlink and executable-bit coverage above. Only this validation document is
excluded. The local manifest is
`/tmp/usque-proton-login-401-source-manifest.json`.

The user's Android screenshot showed `Start sign-in` / HTTP 401 / API 401
about 1–2 seconds after submitting credentials, before any human-verification
window. This is the SRP-info step, before the password proof is submitted.
The APK/source identity for that observation was not supplied. The official
outer token handler and startup session fetcher were rechecked against their
pinned public sources; the [protocol reference](PROTON_VPN_PROTOCOL.md) now
describes that outer layer as well as the direct `PerformLogin` method.

On HTTP 401, native SRP-info and proof requests now create a missing anonymous
session or refresh the current setup job's anonymous session, then retry the
same body once. Android identity, frame data and WARP binding stay with the
same client. Recovery does not call credentialless. Credential errors,
repeated 401, failed recovery, uncertain mutations, cancellation and changed
underlay stop explicitly. Saved authenticated sessions are excluded from this
path, preserving their existing durable rotation contract. Failed anonymous
refresh stops rather than recursively replacing sessions or replaying tokens.

The new regression first reproduced the previous immediate `api_rejected`
failure. Eight added synthetic tests cover recovery at both SRP calls, reuse
and refresh, bounded retries, real 9001 response handling, request-local human
verification headers, wrong credentials, malformed/rejected/timed-out recovery,
unconfirmed mutation, cancellation, underlay changes, and cleanup ownership.
An invalid synthetic ServerProof still prevents every protected request;
these tests do not authenticate an account.

### Live synthetic AuthInfo comparison

One standalone HTTPX probe ran at `2026-10-19T09:34:21Z` through workstation
HTTPS with certificate verification and no explicit WARP binding. It used one
random synthetic username and the pinned Android `5.20.74.0+apk` API profile
with synthetic Pixel 8 / Android 15 facts. The AuthInfo body was identical
before and after session creation; cookies were cleared before each request.

| Operation | HTTP | API code |
| --- | --- | --- |
| `POST /auth/v4/info`, no session | 401 | 401 |
| `POST /auth/v4/sessions`, synthetic device frame | 200 | 1000 |
| `POST /auth/v4/info`, owned anonymous UID/Bearer | 200 | 1000 |
| `DELETE /auth/v4`, owned anonymous session | 200 | 1000 |

The second response contained SRP v4 fields; only their response shape was
checked. No password or SRP proof was submitted, no credentialless identity
was requested, and no account was authenticated. The temporary anonymous
session was revoked. Response bodies, UID and tokens were never persisted or
printed. This confirms the first-hop session-context issue from this
workstation; it does not validate Android JNI, its WARP route, or full sign-in.
The local probe script SHA-256 is
`7baae1e4a85ffe8172ef458e69cc61b7e154c1c176653549e5629b5c4b32cd2a`;
its sanitized result SHA-256 is
`0b5692bedd8e80a6e34276bdf3cf980152fb6377aa9ae891f11b97a00c1a7f2e`.

### Final workstation checks

Commands run from the repository root except Flutter commands in
`apps/usque_gui` and Gradle commands in its `android` directory. Logs use the
`/tmp/usque-proton-login-401-` prefix. No Dart or Kotlin source changed.

| Exact check | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed |
| `cargo test --workspace --all-targets --locked` | passed: 1,771 tests; 3 existing ignores |
| `cargo test -p usque-transport --features wireguard --locked proton_vpn:: -- --nocapture` | passed: 65 Proton tests |
| `pwsh -NoProfile -File tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter pub get --enforce-lockfile` | passed |
| `flutter build apk --debug --config-only --no-pub` | passed |
| `./gradlew --no-daemon :app:ktlintCheck` | passed |
| `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed: 476 tests, no failures/skips; lint passed |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed |
| `python3 tool/check_repository_policy.py` and `git diff --check` | passed |
| `actionlint -no-color` | passed |
| Windows Rust/compile gates | not_run: Linux workstation |
| Real account sign-in, Android WebView/JNI/WARP resumption and full tunnel | not_run |

Cleanup and privacy: only the current setup job's anonymous tokens are eligible
for automatic recovery. All known handles remain owned for revocation, and
unknown remote mutation remains an error. Passwords, SRP material and human
verification tokens keep their private cleared channels; recovery APIs receive
no password, username or proof. Existing authenticated renewal journaling,
ServerProof validation, protected-user/VPN permission checks and fail-closed
runtime gates remain enforced. No package was installed, no device/emulator
was used, and no host networking or release/signing state was changed.

## Android verification buffers and v2 directory entries — 2026-10-19

Baseline: `0c8151b567d4041e76a3d924e54104ca13b37e2e`. The final edited
source has 5,510 entries in the canonical version-1 manifest (1,086,302 bytes),
SHA-256 `0e83ed8a5b4b0e327ccc5abf6a54484ec6cb67f40e5bb6c530be6d44f03392d8`.
Only this validation document is excluded. The manifest remains local at
`/tmp/usque-proton-directory-buffer-source-manifest.json`; its contents were
verified after the checks. No Kotlin, native window, request-identity or
authentication protocol code changed in this candidate.

The user confirmed that the new build reached `Load servers` after account
sign-in, while completing the guest puzzle immediately produced the generic
verification-failed message without a native verification error code. Two
independent defects were reproduced:

- Flutter's pinned engine returns read-only platform reply buffers, including
  both small copied and large external responses. StandardMessageCodec keeps
  typed-data views into those buffers. Clearing a borrowed handoff after the
  puzzle, or clearing its returned success bytes, threw `UnsupportedError`.
  Dart now owns mutable copies before submission/cleanup. Writable transferred
  sources are cleared; read-only framework sources can only be released, not
  claimed as explicitly erased. Owned copies are cleared on normal and stale
  completion. A real codec/read-only envelope fixture and two Android widget
  regressions cover the handoff/result cases; late cancellation also uses a
  read-only reply. The final complete Flutter suite passed below.
- The v2 directory returned `EntryIP` for every physical server and no
  `EntryPerProtocol`. The previous collector skipped every such server. The
  collector now follows official `ConnectingDomain.getEntryIp`: use a valid
  WireGuardUDP override when present, otherwise the same server's `EntryIP`.
  Explicit malformed overrides are rejected; absent/null ports use the API's
  clientconfig defaults. Public IPv4, online physical status, canonical nonzero
  peer key, tier and feature filters remain required. Four new protocol tests
  cover fallback, precedence, nullable ports and rejection boundaries; the
  provisioning fixture now uses the observed v2 shape.

The [protocol reference](PROTON_VPN_PROTOCOL.md) links the pinned Flutter engine
reply path, typed-data decoder and Proton model. The cached official
`ConnectingDomain.kt` and `DefaultPorts.kt` were verified against GitHub's
pinned content and Git blob hashes. Directory failures now have specific
allowlisted English/Chinese messages rather than a repeated generic failure.

### Bounded directory probes and offline comparison

The workstation probes used verified HTTPS, the pinned Android API identity,
synthetic device facts, no account password, no redirects and no explicit WARP
binding. No real puzzle was solved or automated. Only numeric status/shape
summaries were logged. All captured temporary sessions were revoked with
HTTP 200 / API 1000; no token or UID was printed or persisted.

| Probe | Result |
| --- | --- |
| No session, config and directory GETs | HTTP 401 / API 401 |
| Synthetic anonymous session at `2026-10-19T10:09:34Z` | Session/config: 200/1000; directory: 403/9106; session revoked |
| Synthetic guest at `2026-10-19T10:11:39Z` | Anonymous/credentialless/config/directory: 200/1000; session revoked; no HV required |
| Guest directory shape | 1,403,270 bytes; 1,618 logicals, including 119 free; 2,523 physicals, all with EntryIP and none with EntryPerProtocol; 2,458 online physical status flags |
| Same saved directory through old production collector | `no_free_wireguard_nodes` |
| Same saved directory through edited production collector | 200 compatible free candidates (the existing output cap) |

The offline comparison compiled each collector with the real `usque-core`
validation and local locked dependency artifacts. Only the unused configuration
function's crypto argument was stubbed; collection performed no network or
connection. The local directory SHA-256 is
`c2b0c46ac661d2e20062def760124541e46ac7c995e7bbecea736792fb6777ff`;
clientconfig SHA-256 is
`3fb51eac1a6a4b69da624c5a4e2272bd56198ed40dbf9f5a91334703f8e588ed`.
The sanitized guest result SHA-256 is
`a9bb50366be2f60bff5c807f1ae1f14762badb1a7518ee57100632905d2e36b0`.
Raw directory snapshots and temporary probe/harness files remain local under
`/tmp/usque-proton-`; they are not publication evidence or tracked artifacts.
The collector comparison proves compatibility with this response, not binary
logical status/load processing, Android routing, or WireGuard/agent acceptance.

### Final workstation checks

Commands use the pinned toolchains in CONTRIBUTING. Flutter commands ran in
`apps/usque_gui`, Gradle in its `android` subdirectory, and other commands at
the repository root. Logs have the `/tmp/usque-proton-directory-buffer-` prefix.
An initial analyzer run rejected an unnecessary test import; the import was
restricted and the complete Flutter matrix rerun. Initial Linux compile
invocations from the repository root were rejected for missing pubspec; both
were rerun successfully from the GUI directory.

| Exact command | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed |
| `cargo test --workspace --all-targets --locked` | passed: 1,775 tests; 3 existing ignores |
| `cargo test -p usque-transport --features wireguard --locked proton_vpn:: -- --nocapture` | passed: 69 tests |
| `flutter pub get --enforce-lockfile` | passed |
| `dart format --output=none --set-exit-if-changed lib test` | passed: 265 files unchanged |
| `flutter analyze --no-pub` | passed: no issues |
| `flutter test --no-pub --exclude-tags golden` | passed: 1,146 tests; 1 existing skip |
| `flutter test --no-pub --tags linux-golden` | passed: 19 tests; no baseline changes |
| `flutter build linux --debug --no-pub -t lib/main_preview.dart` | passed |
| `flutter build linux --release --no-pub -t lib/main.dart` | passed |
| `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` | passed |
| `cmake --build apps/usque_gui/build/linux/x64/release --target usque_linux_socket_test` and `apps/usque_gui/build/linux/x64/release/runner/usque_linux_socket_test` | passed |
| `cmake --build apps/usque_gui/build/linux/x64/release --target usque_proton_verification_test` and `apps/usque_gui/build/linux/x64/release/runner/usque_proton_verification_test` | passed |
| `python3 -m unittest discover -s tool -p test_linux_package.py -v` | passed: 19 tests; 1 existing skip |
| `pwsh -NoProfile -File tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter build apk --debug --config-only --no-pub` | passed |
| `./gradlew --no-daemon :app:ktlintCheck` | passed |
| `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed: 476 tests; no failures/errors/skips; lint passed |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed |
| `python3 tool/check_repository_policy.py`, `actionlint -no-color`, `git diff --check` | passed |
| Windows Rust, Flutter/runner compile and golden gates | not_run: Linux workstation |
| Real account full setup, Android WebView/JNI/WARP resumption and tunnel/agent | not_run |

No package was installed, no device/emulator or real account was used, and no
host networking or signing/release state changed. The corrections do not alter
API headers, challenge tokens, server proof validation, session rotation,
revocation ownership or runtime authorization gates. Workstation HTTPS and
offline fixtures cannot establish successful verification or connectivity on
the user's Android device.

## Account catalogue and connection lifecycle follow-up / 账号目录与连接生命周期修正

Date: 2026-10-19. This candidate changes account/node lifecycle and the explicit
12087 recovery described in the current [protocol reference](PROTON_VPN_PROTOCOL.md).
It does not change the historical candidates above or establish live native
acceptance.

| Candidate identity | Value |
| --- | --- |
| Baseline | `717b83f913351d750d66081a3d569afa6504cf44` |
| Complete source identity | Uncommitted working tree, 5,511 entries; version-1 manifest `/tmp/usque-proton-lifecycle-source-manifest.json`, 1,086,486 bytes, SHA-256 `765730c3012a1b9a1cff5d394846719ec977e3a66ec706b14af1a110f5735067` |
| Manifest scope | Sorted tracked/unignored source union, regular-file hashes/size/executable bit, symlink targets and deletions; only this validation record excluded to avoid recursion; ignored SDKs, JNI libraries and build output excluded |
| Toolchains | Same pinned Linux x86_64 Rust 1.97.1, Flutter 3.44.7 / `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, NDK 29.0.14206865 and SDK CMake 3.22.1 as above |

The official pinned `CertificateRepository`, `RefreshSessionRequest` and
`HumanVerificationInvalidHandler` were fetched through GitHub's Contents API;
Git blob hashes and cached bytes matched. The invalid-HV handler also matched
Core 36.8.0's published source artifact. Source links and the differences between
that artifact and the public Core snapshot remain in the protocol reference.

| Safe check | Result |
| --- | --- |
| `bash tool/dev.sh check rust` | passed; exact format, workspace Clippy and locked workspace/all-target tests; 1,792 passed, 3 intentionally ignored |
| Proton tests within the workspace | passed; 83 transport tests, including lazy certificates, fresh peer resolution, 401-only recovery, known/unknown refresh results, key/UID binding, cancellation, grouped accounts and retained cleanup ownership |
| Core encrypted-store tests within the workspace | passed; v1 compatibility, v2 deferred certificates, metadata/size bounds, complete encrypted catalogues and cleanup-only capabilities |
| `cargo check -p usque-transport --locked --no-default-features` and `cargo check -p usque-android --locked --no-default-features` | passed |
| `bash tool/dev.sh check flutter` | passed; locked pub resolution, Dart format/analyze, 1,153 widgets passed with 1 existing skip, 19 Linux goldens passed |
| Final Linux node-picker fixture | regenerated with the documented Linux-only command, visually reviewed with the two other changed Proton images, then `flutter test --no-pub --tags linux-golden`: 19 passed; final Dart format and analysis passed |
| `bash tool/dev.sh check android` | passed; fixed-NDK arm64 Clippy, Flutter debug configuration, Kotlin lint/unit and Android lint; 477 unit tests, no failures/errors/skips |
| Final Android Rust check | `pwsh -NoProfile -File tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy`: passed on final production source |
| Linux GUI debug preview and production release | both documented `flutter build linux ... --no-pub` commands passed; no application started |
| Linux production sidecars | `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update`: passed |
| Linux native fixtures | both documented CMake targets built and both executables passed: authenticated fake socket and Proton verification policy |
| Linux package contract | `python -m unittest discover -s tool -p "test_linux_package.py" -v`: 19 passed, 1 skipped for unavailable desktop-file-utils; no installation |
| Aggregate source checks | `bash tool/dev.sh check static`: passed, including exact `pwsh -NoProfile -File tool/check_source.ps1`, repository policy, actionlint and whitespace checks |
| Windows Rust/Flutter/runner/goldens, native Android VPN/device/TV, protected leak/lifecycle checks | **not_run**; host/infrastructure unavailable; no emulator or release APK installed |

The Flutter wrapper ran the full widget suite before the final screenshot fixture
was changed from the retired setup-node prompt to the new local server picker.
Only that tagged fixture changed afterward; all 19 Linux goldens, Dart format and
analysis were rerun. Windows screenshots were not updated on Linux.
Local logs are `/tmp/usque-proton-rust-complete.log`,
`/tmp/usque-proton-flutter-gates.log`, `/tmp/usque-proton-golden-final.log`,
`/tmp/usque-proton-android-gates.log`,
`/tmp/usque-proton-android-clippy-exact-final.log`,
`/tmp/usque-proton-static-final.log`, `/tmp/usque-proton-linux-sidecars-final.log`,
`/tmp/usque-proton-linux-preview.log`, `/tmp/usque-proton-linux-release.log`,
`/tmp/usque-proton-linux-native.log` and `/tmp/usque-proton-linux-package.log`.
These remain local workstation evidence, not protected-runner or release evidence.

### Bounded HTTPS experiment and offline directory replay

The earlier explicit authorization to submit synthetic guest requests was used
for one owned-session experiment. A memory-only Ed25519 key and synthetic Android
request context were used through verified workstation HTTPS, **without WARP**.
Anonymous creation, credentialless creation, scopes, VPN permission, certificate
issuance, token refresh, repeated scopes/permission/certificate issuance and
owned-session deletion each returned HTTP 200 / API 1000. Both certificates
matched the generated key. Cleanup was confirmed; no tokens, UID, private key or
certificate were recorded. The sanitized numeric report is
`/tmp/usque-proton-lifecycle-probe-20261019-result.json`.

This probe did not require a puzzle and cannot reproduce or resolve the user's
12087, validate a real account login, or establish native WireGuard/agent
interoperability. No additional guest request was made for the parser test.
Offline replay of the previously obtained public v2 directory found 119 free
logical servers, of which 116 had an online entry and 3 were offline. The current
parser returned all 116 compatible online logical IDs exactly once. It no longer
creates multiple rows for physical entries or truncates at 200 candidates.
Synthetic fixtures separately cover 600 logical IDs, duplicate physical entries,
paid-tier filtering, revision-consistent pagination and selection past item 300.

### Cleanup, fail-closed and privacy analysis

Accounts keep their immutable local profile/key binding while each connection
resolves the current logical server's entrance, peer key and agent hostname.
Missing/due certificates are acquired on that connection; healthy access tokens
and certificates are reused. A selected unavailable server fails explicitly.
The existing in-tunnel mTLS/status/features authorization gate remains required;
there is no WARP-only or host-network fallback.

Token recovery writes a pending journal before its sole refresh call and saves a
validated replacement before any later protected request. A known response is
saved under the held account lease even if cancellation/underlay change arrives
with it; the following checkpoint prevents further network use. Unknown outcomes
remain pending. Malformed success handles are persisted solely for revocation,
with their original API identity, and cannot validate as an account session.
Cleanup failures retain ownership; they do not revoke a newly stored account.

One public row groups the same user privately, preferring the actual saved
selection so a healthy duplicate cannot hide its pending session. Related active
or saved references are checked natively. Removal first confirms clearing the
saved selection; failed confirmation prevents deletion. Remote sign-out and
local-only forgetting remain separate explicit choices. A partial remote failure
preserves records and reports uncertainty. Unrelated sources and other devices'
sessions are outside the removal scope.

The encrypted catalogue is bounded to 20,000 logical nodes, with 8 MiB plaintext /
12 MiB ciphertext records and 128 accounts. Public pages contain at most 128
metadata entries; existing IPC/Android response limits were not raised. Passwords,
challenge data, session UserID, tokens, keys and certificates stay out of account
summaries, node pages and ordinary error text. Logs use synthetic fixtures or
numeric live results. Real Android acceptance, WebView/API egress consistency,
certificate expiry/revocation on a live tunnel and platform restoration remain
**not_run**.

## Follow-up: agent recovery and Proton error provenance (2026-10-19)

This follow-up starts from `31f1165b0dc80449d4071bf1fff3883adc056d13` and
addresses the Android connection failure reported as a WARP account problem.
Official Android master was verified through GitHub as
`316a3d499e120bd2cab63c2cd8ac396e3ab3b24a`; VpnBackend and CertificateRepository
matched the cached source byte for byte. The pinned go-vpn-lib client/message
sources were also reverified. This audit made no new guest or account API request.

### Tested source identity and scope

The complete tracked/unignored source inventory, excluding only this validation
record to avoid self-reference, is
`/tmp/usque-proton-connection-source-manifest.json`: version 1, 5,513 entries,
1,086,896 bytes, SHA-256
`fdbdb7208c4740ed36f68d3d4f13646fe39c245e667389f9b4540b622f8ae0df`.
Entries record path, content length/hash and executable bit, or symlink target
and deletion. Ignored SDKs, build/JNI output, logs and private configuration are
excluded. The source inventory is checked again before committing.

Physical Label now reaches the agent's bouncing feature, including `"0"`, and
matching features are required for business admission. Restricted/nonfinal
statuses keep admission closed while waiting; their watchdog retains the
numeric reason. Expired client certificates renew on the same key. Revoked or
bad-signature certificates create a new tunnel key, durably save it with an empty
certificate in one atomic encrypted v3 record, then request its certificate and
restart WireGuard after the old workers exit. The original saved profile/key
binding and revision remain unchanged; v1/v2 records remain readable. Automatic
certificate recovery is bounded per agent, and automatic key replacement is
bounded across reconnects by a five-minute cooldown.

The credential worker starts before initial agent admission. Temporary scheduled
renewal failures retry with bounded backoff while a validated unexpired
certificate remains usable. Initial due-refresh failures have the same cache
rule. UID/plan changes invalidate old certificates. Unknown token rotation,
expiry, external sidecar mutation and lost agent authorization remain terminal
or block admission as appropriate. Android accepts recovery only for a matching
retryable provider code, typed Proton reason and current Proton profile.

Append-only PROTON failure codes preserve provider provenance and numeric API /
HTTP or agent context. Home and the chain editor distinguish session limits,
reused keys, policy restrictions, certificate, API and transport failures. Android
status, UI and diagnostic bridges allowlist numeric context and drop remote
descriptions; the typed reason survives both bridge directions. Proton failures
cannot select WARP identity or endpoint-pin remediation.

### Workstation checks

Pinned toolchains and exact CONTRIBUTING commands were used. Flutter commands
ran in `apps/usque_gui`, Gradle commands in its `android` subdirectory. Wrappers
`bash tool/dev.sh check rust`, `flutter`, `android` and `static` execute their
respective exact commands below. The final Kotlin checks were repeated after the
status-bridge change. Intermediate runs found strict Clippy/style findings and a
local Kotlin dynamic-value expression rejected by the compiler; those were
corrected without suppressing checks. An initial targeted Flutter invocation
included Windows goldens on Linux and was rejected by the existing host guard;
no Windows baseline was changed. Final widget and Linux golden commands used the
required host selection. An overlapping intermediate Rust log was discarded;
final Rust evidence is the separate `rust-confirmed` log.

| Exact command | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed |
| `cargo test --workspace --all-targets --locked` | passed: 1,806 tests; 3 existing ignores |
| `cargo check -p usque-transport --locked --no-default-features` | passed |
| `cargo check -p usque-android --locked --no-default-features` | passed |
| `flutter pub get --enforce-lockfile` | passed |
| `dart format --output=none --set-exit-if-changed lib test` | passed |
| `flutter analyze --no-pub` | passed |
| `flutter test --no-pub --exclude-tags golden` | passed: 1,155 tests; 1 existing skip |
| `flutter test --no-pub --tags linux-golden` | passed: 19 tests; baselines unchanged |
| `flutter build linux --debug --no-pub -t lib/main_preview.dart` | passed |
| `flutter build linux --release --no-pub -t lib/main.dart` | passed |
| `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` | passed |
| `cmake --build apps/usque_gui/build/linux/x64/release --target usque_linux_socket_test` and `apps/usque_gui/build/linux/x64/release/runner/usque_linux_socket_test` | passed |
| `cmake --build apps/usque_gui/build/linux/x64/release --target usque_proton_verification_test` and `apps/usque_gui/build/linux/x64/release/runner/usque_proton_verification_test` | passed |
| `python -m unittest discover -s tool -p "test_linux_package.py" -v` | passed: 19 tests; 1 existing skip (desktop-file-utils unavailable) |
| `pwsh -NoProfile -File tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter build apk --debug --config-only --no-pub` | passed |
| `./gradlew --no-daemon :app:ktlintCheck` | passed |
| `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed: 480 tests; no failures/errors/skips; lint passed |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed |
| `python tool/check_repository_policy.py`, `actionlint -no-color`, `git diff --check` | passed |
| Windows Rust/Flutter/runner/golden gates | **not_run**: Linux workstation |
| Real Android device/TV, Proton/WARP/WireGuard/agent tunnel, expiry/revocation and protected leak/lifecycle checks | **not_run**: no isolated device or protected environment; no emulator installed |

Final logs use the `/tmp/usque-proton-connection-` and
`/tmp/usque-proton-kotlin-` prefixes. They are local workstation evidence, not
protected-runner or release evidence. Memory peers verify initial recovery,
closed admission through restriction/recovery, matching bouncing, server trust
failure separation, same-key expiry renewal, new-key persistence/restart,
recovery cooldown, cancellation, transient renewal/cache reuse and precise
provider failure export. Android fixtures verify recovery through the actual
allowlisted status bridge, and Flutter fixtures verify provider-specific copy
without backend text.

### Cleanup, fail-closed and privacy

The outer WARP handle, generation checks, dedicated agent control tuple,
certificate/SNI verification and explicit feature confirmation remain required.
No failed Proton setup or recovery can select a WARP-only or host-network exit.
A server-certificate trust failure cannot invoke client-certificate recovery.
Business admission closes before recovery requests and stays closed until a new
validated connection authorizes it. Startup retries await credential and native
worker cleanup, including failure to start the credential worker. Cancellation
is propagated and owned work is awaited; no detached refresh or key rotation is
allowed to outlive its owner.

Recovered runtime keys and their cooldown live in the same atomic encrypted
record; interrupted certificate issuance cannot restore a revoked key. The
existing token-rotation journal and known-response persistence remain in force.
Public metadata and diagnostics expose no passwords, session identifiers,
tokens, key material, certificates or remote descriptions. Tests use synthetic
memory peers only. No VPN/TUN, firewall, routes, DNS, system proxy, installation,
signing or publication state was exercised. Live Android acceptance and native
platform restoration remain **not_run**.

## Follow-up: system-network public API routing (2026-10-19)

Baseline: `48a73e17ee6d6f88c785ace58974ca80bb9b762e`. The user requested that
Proton public APIs follow the current system network and applicable VPN instead
of an explicitly owned WARP underlay. The user also explicitly chose strict
system routing when Usque itself owns an unavailable VPN: API requests may fail
in that situation; no self-VPN exception or physical-network bypass is added.
This replaces the earlier control-plane routing design, not the WireGuard or
local-agent data-plane contract.

### Source identity and behavior

The complete tracked/unignored inventory is
`/tmp/usque-proton-system-network-source-manifest.json`: version 1, 5,515 entries,
1,087,275 bytes, SHA-256
`8aaab799ceaf9f5ebb2c8deccee6736769528a371e6d5c7f9e30c5708d78ed93`.
It excludes only this validation record to avoid self-reference, plus ignored
SDKs, build/JNI output, logs and local private configuration. Entries bind file
content, executable bits, symlinks and deletions. It is verified again before
committing.

The shared [system HTTPS client](../crates/usque-transport/src/proton_vpn/system_http.rs)
uses ordinary OS sockets, system DNS and supported environment proxy settings.
Guest/account setup, permissions, directories, certificates, token recovery,
renewal and session revocation all use it. Manager setup no longer starts or
borrows WARP; connection preparation no longer passes a WARP network to the API
lease. Android removes WARP-secret/profile snapshots and socket-protector setup
from the Proton JNI call. Account lifecycle leases, active/selected-account
protection, encrypted persistence and the token-rotation journal remain in force.
API app identity, User-Agent and locale are preserved.

TLS trust and hostname checks, request cancellation, bounded timeouts, numeric
Retry-After and error bodies remain enforced. Redirects, automatic library
retries and response decompression are disabled; plaintext responses remain
bounded and zeroized. No interface binding, VPN protection or alternate-network
fallback is available on this API path. WARP generation no longer fences an API
job or its puzzle. Job/session/challenge ownership, expiry and cancellation
still apply. The verification window continues using the normal platform path.

The actual exit is still WARP → WireGuard → Proton; local-agent mTLS and business
admission remain inside that tunnel. No routing, firewall, DNS, system proxy or
platform protection policy is changed. If the OS routes an API call into an
unavailable Usque VPN, it can time out. The UI now asks the user to check the
current network/VPN instead of a WARP account. This accepted limitation is
recorded in the [product guide](PROTON_VPN.md).

### Workstation checks

The pinned CONTRIBUTING commands ran through `bash tool/dev.sh check rust`,
`flutter`, `android` and `static`; the additional compile/fixture commands below
also completed. Flutter commands ran in `apps/usque_gui`, Gradle commands in
its `android` directory. The initial loopback test run was blocked by sandbox
socket restrictions and rerun with loopback access. Test findings corrected TLS
error classification through an io::Error wrapper and removed Android fields
used only by the deleted WARP snapshot check. No check was suppressed.

| Exact command | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed |
| `cargo test --workspace --all-targets --locked` | passed: 1,811 tests; 3 existing ignores |
| `cargo check -p usque-transport --locked --no-default-features` | passed |
| `cargo check -p usque-android --locked --no-default-features` | passed |
| `flutter pub get --enforce-lockfile` | passed |
| `dart format --output=none --set-exit-if-changed lib test` | passed |
| `flutter analyze --no-pub` | passed |
| `flutter test --no-pub --exclude-tags golden` | passed: 1,158 tests; 1 existing skip |
| `flutter test --no-pub --tags linux-golden` | passed: 19 tests; baselines unchanged |
| `flutter build linux --debug --no-pub -t lib/main_preview.dart` | passed |
| `flutter build linux --release --no-pub -t lib/main.dart` | passed |
| `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` | passed |
| `cmake --build apps/usque_gui/build/linux/x64/release --target usque_linux_socket_test` and `apps/usque_gui/build/linux/x64/release/runner/usque_linux_socket_test` | passed |
| `cmake --build apps/usque_gui/build/linux/x64/release --target usque_proton_verification_test` and `apps/usque_gui/build/linux/x64/release/runner/usque_proton_verification_test` | passed |
| `python -m unittest discover -s tool -p "test_linux_package.py" -v` | passed: 19 tests; 1 existing skip (desktop-file-utils unavailable) |
| `pwsh -NoProfile -File tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter build apk --debug --config-only --no-pub` | passed |
| `./gradlew --no-daemon :app:ktlintCheck` | passed |
| `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed: 480 tests; no failures/errors/skips; lint passed |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed |
| `python tool/check_repository_policy.py`, `actionlint -no-color`, `git diff --check` | passed |
| Windows Rust/Flutter/runner/golden gates | **not_run**: Linux workstation |
| Real Android device/TV, active-VPN routing, live guest/account verification, complete Proton tunnel, expiry/recovery and protected lifecycle/leak checks | **not_run**: no isolated device or protected environment; no emulator installed |

Logs use `/tmp/usque-proton-system-network-`. Loopback TLS tests cover normal
OS connections without a WARP identity, preserved identity headers and error
bodies, redirects, declared/streamed size limits, compression rejection,
cancellation/timeout socket cleanup, untrusted certificates and wrong hostnames.
A real Manager worker starts without WARP configuration and stops before HTTP
on malformed credentials. Flutter fixtures cover network error copy. These
checks do not prove an actual VPN's routing or successful live Proton setup.

No new Proton registration, login or other account API request was sent. No
VPN/TUN, interface, route/DNS/firewall/system-proxy mutation, installation,
signing or release publication was exercised. Secrets and raw remote error text remain
outside ordinary status and diagnostics. Unknown session mutation stays pending
and is never replayed merely because the system network changes.

## Follow-up: connection APIs over established WARP (2026-10-19)

Baseline: `63b38b6ff35dc139ba6c9b7ed1d977ebcdb26053`. The user reports that
guest setup now succeeds but connection fails with a generic Proton network
error. They clarified that connection-control requests should use the WARP
tunnel once it is established. This supersedes strict system routing for the
connection lifecycle only; interactive account management retains that policy.

### Source identity and routing boundary

The complete tracked/unignored source inventory is
`/tmp/usque-proton-warp-runtime-source-manifest.json`: version 1, 5,517 entries,
1,087,651 bytes, SHA-256
`a9078f6f3c665c20cdefee81daca7c3f0b4c0232d9d885fb33894636a1446b14`.
It excludes only this validation record to avoid self-reference, plus ignored
SDK/build/JNI output, logs and local private configuration. Entries bind file
content, executable bits, symlinks and deletions; verification is repeated
before committing.

Source inspection found that connection preparation constructed `SystemHttp`
after the final traffic gate had closed. On Android, ordinary API sockets can
then be captured by Usque's own unavailable VPN, including when WARP itself is
already healthy. A generic transport error alone does not establish this as
the user's only connection failure.

Connection preparation now receives `warp_internal_network()` explicitly and
constructs the private [WARP HTTP adapter](../crates/usque-transport/src/proton_vpn/warp_http.rs).
Session/permission checks, fresh directory lookup, certificate issuance and
the entire runtime lease's renewal, token recovery, retired-session cleanup
and agent authorization recovery use that WARP handle. Its DNS and TCP remain
inside the established stack. No system socket, HTTP proxy, physical DNS or
alternate-network fallback is available to this runtime client.

The lease is bound to WARP health, cancellation and generation, including
packet reconnect counts when the stream dialer has no epoch tag. Old leases
cannot continue authorizing traffic after reconnection. Existing persistence
still saves a fully validated, known token replacement before reporting a
concurrent generation change; unknown mutations remain journaled and are not
replayed. HTTPS trust, hostname validation, bounded bodies/timeouts, numeric
error export and cancellation cleanup reuse the existing private HTTPS path.

Guest/account setup, manual directory refresh, sign-out and the verification
WebView still follow the system network and applicable VPN. They do not start
or borrow WARP. API identity, User-Agent and locale are preserved. Ordinary
application traffic still waits for Proton authorization; the private control
handle does not expose a WARP-only business exit. Local-agent mTLS stays inside
the Proton WireGuard tunnel. No platform routes, socket protection, firewall,
DNS settings, system proxy or native bridge contracts are changed.

### Workstation checks

The final source passed `bash tool/dev.sh check rust`, `android` and `static`,
plus the feature-disabled compilation checks below. The wrappers executed the
exact CONTRIBUTING commands with the pinned toolchains. The added tests use
memory networks/private dialers; existing HTTPS tests use loopback only.

| Exact command | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed |
| `cargo test --workspace --all-targets --locked` | passed: 1,816 tests; 3 existing ignores |
| `cargo check -p usque-transport --locked --no-default-features` | passed |
| `cargo check -p usque-android --locked --no-default-features` | passed |
| `pwsh -NoProfile -File tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter pub get --enforce-lockfile` | passed |
| `flutter build apk --debug --config-only --no-pub` | passed |
| `./gradlew --no-daemon :app:ktlintCheck` | passed |
| `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed; unchanged Kotlin/lint outputs reused; 480 test cases in XML, no failures/errors/skips |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed |
| `python tool/check_repository_policy.py`, `actionlint -no-color`, `git diff --check` | passed |
| Windows Rust/platform gates | **not_run**: Linux workstation |
| Real Android device/TV, live Proton connection, active-VPN routing, expiry/recovery and external leak/lifecycle checks | **not_run**: no isolated device or protected environment |

Five new regression tests cover the real runtime preparation entry point's
private dialer, no system fallback on failure, stream/packet generation changes,
unavailable/cancelled WARP and requests, and the distinction between blocked
application traffic and the surviving explicit WARP control handle. Existing
session rotation, certificate renewal and agent recovery tests still pass.
Logs use `/tmp/usque-proton-warp-runtime-`, with `-final.log` for the final Rust,
Android and aggregate checks. No Dart, GUI runner or packaging source changed;
their separate widget/golden/build matrices were not rerun.

No new live registration, login or Proton API request was sent. No VPN/TUN,
platform-network mutation, emulator installation, device/APK installation,
signing or release publication was exercised. Secrets and raw remote error text
remain outside ordinary status and diagnostics. These workstation checks fix
and verify the control-path dependency; they do not establish a successful
real Proton WireGuard/local-agent connection.

## Agent preference admission and live userspace connection (2026-10-20)

Baseline: `730824993ebac65e22fb9f1302f2feb59e894bdf`, plus the changes in
this record. The complete tracked/unignored source manifest is
`/tmp/usque-proton-feature-source-manifest.json`: version 1, 5,519 entries,
1,009,511 bytes, SHA-256
`c7148a27e7c9799191533ecdb38570f1c1b99b4bb9561054812fe81625ff8d4d`.
It excludes only this validation document to avoid self-reference; ignored
SDKs, generated JNI/build output and private configuration are not source inputs.

The preceding authorized Linux userspace run registered a synthetic Android
guest, completed connection APIs/certificate issuance and the WireGuard
handshake, then reached the 35-second startup timeout. That run returned
`PROTON_TRANSPORT_FAILED`; its trace did not distinguish agent TLS from
feature-confirmation waiting.

Comparison used current public Android `316a3d499e120bd2cab63c2cd8ac396e3ab3b24a`
and go-vpn-lib `9440653831b00dbc4711f59e8a23a06af1999405`. The agent now separates
ordinary preferences from authorization: connected does not require complete
NetShield/NAT/split-TCP echoes, and requests no longer include `jail=false`.
The selected bouncing label must still match. Certificate/SNI validation,
expiry, explicit jail, restrictions, revocation and malformed authorization
fields remain fail-closed. Reconnection and timeout policies are unchanged.
New debug markers contain only fixed stage names and connected/authorized
booleans; they do not include agent messages or identity material.

### Authorized live result

One new guest used the existing synthetic Android context fixture. Setup used
system HTTPS; connection APIs and the inner tunnel used an ephemeral WARP
registration and the production Rust userspace chain. No Android device or
APK was exercised. The disposable probe's SHA-256 is
`5d81566e1f68350ac1e7bce0d7a8c0afeae1fb5088016d1c6c49bbab8ee67bb7`.

Guest registration, WARP/MASQUE, connection APIs/certificate issuance, WireGuard,
agent TCP, mutual TLS and agent authorization all succeeded. Authorization took
11,842 ms from connection start. A subsequent HTTPS request through the final
network succeeded with 577 response bytes. The trace observed connected but
not yet authorized states before the selected-label authorization succeeded;
this is not evidence identifying which earlier preference echo caused failure.

The probe closed its loopback listener, stopped its workers and successfully
revoked both the Proton session and temporary WARP registration. Its encrypted
temporary credential directory was removed. Interface, IPv4/IPv6 route/rule and
resolver comparisons were unchanged. No TUN, host DNS, route, firewall or
system-proxy mutation was performed. The sanitized local log is
`/tmp/usque-proton-feature-live-result.jsonl`, SHA-256
`ac51af904bc9d034d6e83b7ea59e6d7c01f7e9a3ca8a8e36ee0c7301a7f92634`.

### Checks

Commands start at the repository root, except Flutter commands in
`apps/usque_gui` and Gradle commands in its `android` directory. The Linux
`rust` and `android` wrappers ran the corresponding exact CONTRIBUTING commands.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed |
| `cargo test --workspace --all-targets --locked` | passed: 1,820 tests; 3 existing ignores |
| `cargo test -p usque-transport --features wireguard --locked --offline proton_vpn::local_agent::tests -- --nocapture` | passed: 20 tests |
| `cargo build -p usque-engine --example proton_live_probe --locked --offline` | passed; disposable source removed from worktree afterward |
| `target/debug/examples/proton_live_probe` | passed: live userspace connection and final HTTPS |
| `pwsh -NoProfile -File tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter pub get --enforce-lockfile` | passed |
| `flutter build apk --debug --config-only --no-pub` | passed |
| `./gradlew --no-daemon :app:ktlintCheck` | passed |
| `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed; unchanged Kotlin tests reused; 487 XML cases, no failures/errors/skips |
| `python3 tool/check_repository_policy.py`, `git diff --check` | passed |
| Windows Rust/platform gates | **not_run**: Linux workstation |
| Android device/TV and APK lifecycle; independent leak/performance checks | **not_run**: no isolated device or protected environment |

Rust and Android logs use `/tmp/usque-proton-feature-`. Memory regressions cover
omitted/default/different preference echoes, selected-label binding, explicit
jail, certificate errors and revocation. The live result establishes one native
userspace connection, not Android VPN lifecycle, observed exit IP, leak safety
or sustained performance. No emulator, APK installation, release package,
signing or publication was performed.

## Bounded connection cache and warm reconnect latency (2026-10-20)

This local Linux/WSL userspace check covers the v4 encrypted connection cache,
account/WARP-generation scoped HTTP/1 reuse and admitted Lease metadata refresh.
It includes the preceding agent preference admission fix. It is a workstation
functional benchmark, not protected performance-lab or Android device evidence.

Candidate identification:

- Base HEAD: `730824993ebac65e22fb9f1302f2feb59e894bdf` plus the working-tree changes.
- Sorted SHA-256 manifest of 5,522 tracked and new source files:
  `1d0b0f1ea59bb53eb08d921a70f6bb88d54001e54f8e1b420c05128b00209247`.
  Only this validation document is excluded to avoid a self-referential hash.
  The local manifest includes each path and content digest; HEAD alone does not
  identify the tested changes. Ignored toolchains, generated output and private
  test data are outside the source manifest.
- Temporary probe source SHA-256:
  `81f9876ca0e089aec87e773e271dfcc70307049de263d0ab7e945d80da934218`.
- Probe binary SHA-256:
  `da4633fdb350b980192ad730abce3449820fb06d596cb2efd83baf3b970d89c8`.
  Built with `cargo build -p usque-engine --example proton_live_probe --locked
  --offline`; the disposable example source was removed before workspace checks.

The probe registered one new synthetic Android-context guest and one temporary
WARP device, selected one logical node, and rebuilt the WARP/Proton userspace
runtime for each connection. Six rounds covered the first connection and five
warm reconnects. A seventh round cleared only the local optional metadata cache,
retaining the valid certificate, to exercise online directory preparation and
HTTPS reuse separately. No extra guest or WARP identity was created for it.

The time origin below is WARP's `TunnelReady` telemetry timestamp; the endpoint
is the first successful Proton final activation. Guest setup and WARP registration
are excluded. The API count covers preparation responses before activation.

| Round | Preparation API requests | Preparation | WARP ready to first authorization | WARP ready to completed final HTTPS |
| --- | --- | --- | --- | --- |
| First, deferred certificate | 1 | 1,149.682 ms | 2,412 ms | 3,002 ms |
| Warm 1 | 0 | 3.540 ms | 805 ms | 2,399 ms |
| Warm 2 | 0 | 3.785 ms | 809 ms | 2,400 ms |
| Warm 3 | 0 | 3.637 ms | 824 ms | 2,432 ms |
| Warm 4 | 0 | 3.693 ms | 806 ms | 2,396 ms |
| Warm 5 | 0 | 3.776 ms | 821 ms | 2,424 ms |
| Cold metadata, valid certificate | 4 | 4,685.539 ms | 5,507 ms | 7,103 ms |

Warm authorization median was **809 ms**, below the planned 3-second acceptance
threshold; warm completed-HTTPS median was **2,400 ms**. The first certificate
connection was below its 5-second target. Warm WireGuard startup was 147–149 ms;
agent TCP, mTLS and first authorized status together took approximately 0.65–0.67
seconds. Cold preparation used one TCP/TLS establishment for all four API
responses, confirming reuse against the real API. All seven final HTTPS requests
returned 577 bytes successfully through the admitted Proton exit.

These are first-authorization timings, not a claim of uninterrupted admission
from that instant. The warm agent exchanges also reported a brief subsequent
non-connected state while preferences/status settled; the gate closed and
reopened with agent authorization. Completed-HTTPS timings include that period
and the destination's TCP/TLS/request time. Agent protocol and admission checks
were not changed by the latency optimizations. The official Android app was not
benchmarked alongside this run, and this small sample does not establish a
cross-platform or controlled before/after latency distribution.

Cleanup and privacy:

- Every runtime closed its loopback SOCKS listener and awaited shutdown. No TUN,
  Kill Switch or system proxy was enabled.
- Both the Proton session and WARP registration were confirmed revoked; the
  account worker stopped. The private encrypted temporary directory was removed.
- Before/after interface, IPv4/IPv6 route and rule snapshots, resolver digest and
  TUN device-presence comparison all matched. This does not establish external
  leak, Android lifecycle or Windows platform-restoration behavior.
- Only allowlisted stages, booleans, durations and counts were retained. No raw
  API responses, UID, tokens, certificates, keys, node identities or endpoint
  addresses enter this public record. Cache peer data stays in the encrypted
  account sidecar; public IPC and generic/system HTTPS routing are unchanged.
- Hard TTLs, clock rollback and missing-cache checks require online preparation;
  every connection still performs certificate validation, WireGuard and agent
  authentication. Permission denial closes admission; a tier/UID change requires
  reconnect. Directory refresh never changes the active peer. Failed endpoints
  invalidate directory hints, and storage/cipher failures remain failures.

Validation commands and results for this candidate:

- `bash tool/dev.sh check rust`: **passed**, executing the exact Rust matrix:
  `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`, and
  `cargo test --workspace --all-targets --locked`.
  **1,836 passed, 3 existing ignored tests**.
- Focused offline regressions cover v1/v2/v3 cache misses, TTL/clock rollback,
  bounded selected peers, optional-cache overflow without catalogue truncation,
  zero-request warm preparation, first-certificate preparation, missing selected
  nodes, failed-peer invalidation, permission/tier invalidation, admission delay,
  refresh backoff and recovery/shutdown preemption. Pool fixtures cover complete
  response reuse, close/partial/encoded/oversized responses, origin rejection,
  generation and idle expiry, cancellation, driver shutdown and no submitted-POST
  replay. They use memory peers or loopback sockets, not platform VPN APIs.
- `bash tool/dev.sh check android`: **passed**. It verified the pinned Flutter
  SDK, ran `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy`,
  `flutter pub get --enforce-lockfile`,
  `flutter build apk --debug --config-only --no-pub`,
  `./gradlew --no-daemon :app:ktlintCheck`, and
  `./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` from the directories
  required by `CONTRIBUTING.md`. Unchanged Kotlin unit/static outputs were reused
  where Gradle reported `UP-TO-DATE`; this was not an Android device test.
- `cargo check -p usque-transport --no-default-features --locked` and
  `cargo check -p usque-android --no-default-features --locked`: **passed**.
- `python3 tool/check_repository_policy.py` and `git diff --check`: **passed**.
- Windows, physical Android/emulator, protected runners, external leak observation,
  controlled performance sampling, packaging and installation: **not_run**.


## Follow-up: account-task WARP fallback (2026-10-24)

Status: **implementation and workstation checks passed; live acceptance not_run**.
This supersedes the earlier system-only account-management routing description.
It does not change the separate WARP-only runtime authorization client.

Baseline: `188dfdc2f1416cc3aa80426d0f9514e8cb5c780c` plus the uncommitted
implementation. The sorted SHA-256 manifest of the 39 changed implementation,
dependency and test files, excluding documentation, is
`ad9a0f70b17a5b90c211da43485b49f8ced513b85cb8a9c499902e5d579a6494`.
The local manifest and sanitized command logs are under `target/proton-*` and
are not committed or published. Linux checks used a separate source copy with
the same relevant files; the final runner-only check followed the WebKit error-domain
API correction. Formatting-only test changes do not alter the tested behavior.

### Implemented behavior / 已实现行为

- Guest setup, login, manual directory refresh and remote logout start on the
  system path. A credential-free five-second ping precedes mutations. Only
  transport failures permit one task-wide switch to WARP; business responses
  and an ambiguous mutation do not trigger blind replay.
- Recovery borrows a healthy underlay or uses the selected enrolled WARP
  account for a headless connection. Cold sockets retain OS routing and do
  not use VPN protection/interface binding to bypass another VPN. Retained
  platform guards remain authoritative; no identity is created automatically.
- API traffic, verification CONNECT traffic and verification DNS share the
  WARP handle. A replacement WARP generation invalidates the task and proxy.
  The proxy checks the generation again after DNS/dialing before acknowledging
  CONNECT. An old verification answer is never replayed across migration.
- The browser must prove HTTP and CONNECT proxy use before loading a challenge.
  Its ready marker includes SHA-256 of the random proxy password, so knowing
  the public `.invalid` nonce alone cannot forge a successful preflight.
  Origin/challenge binding and ordinary TLS verification remain intact.
- Cancellation and completion close task-owned proxy connections and headless
  resources. Borrowed WARP runtimes remain owned by their original connection.
  Android holds its process lease through the asynchronous proxy-clear
  acknowledgement; a missing acknowledgement returns an error within five
  seconds and keeps the lease closed until removal is confirmed.
- Commands opt in with `network_policy_version: 1` only after capability
  negotiation. Legacy commands retain the system path. Proxy credentials and
  URLs remain in the bounded sensitive handoff channel; public job state
  contains only the route and phase. Existing encrypted account storage and
  token-rotation uncertainty handling are retained.

### Executed checks / 已执行检查

Windows used Rust 1.97.1, Flutter 3.44.7 at
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, Visual Studio 18 Build Tools,
and the supported helper-selected native environment. Native runner CMake was
the Visual Studio-bundled version capable of its Visual Studio 18 generator.
WebView2 SDK was the pinned `1.0.4258.31`; installed Evergreen runtime was
`155.0.4283.62`. Linux checks ran on WSL Ubuntu 26.04 amd64 with the same Rust
and Flutter pins. These are workstation checks, not protected-runner evidence.

| Command / scope | Result | Evidence and limits |
| --- | --- | --- |
| `cargo fmt --all --check` | passed | Final Rust formatting |
| `tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed | Locked complete workspace/all-targets gate |
| `tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | passed | 2,062 passed, 13 ignored; includes new recovery, delivery-certainty, stale-challenge and loopback-proxy tests |
| `tool/build_windows_rust_release.ps1 -Variant x64-v2` | passed | Compile-only release sidecars; no installation |
| `tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed | Pinned Android target, NDK and CMake |
| `flutter pub get --enforce-lockfile`; `dart format --output=none --set-exit-if-changed lib test`; `flutter analyze --no-pub` | passed | Verified SDK; no dependency drift |
| Windows `flutter test --no-pub` | passed | 1,293 passed, 19 platform skips; fake Engine |
| `tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .`; `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | passed | Final native runner compiled |
| Windows CMake `usque_proton_verification_test` build and executable | passed | Handoff versions, proxy bounds and existing challenge/result policy |
| Windows CMake `usque_proton_webview_test` build and executable | passed | Seven real WebView2 scenarios: HTTP/CONNECT, wrong proxy realm, forged preflight proof, and four existing bridge/close/origin/challenge cases |
| `flutter build apk --debug --config-only --no-pub` | passed | Configuration only; no release APK or device installation |
| `gradlew --no-daemon :app:ktlintCheck` | passed | Pinned Kotlin formatter |
| `gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed | 491 unit tests, zero failures/errors/skips; debug JNI prerequisites compiled |
| Linux `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed | Generated dependency warnings remained non-first-party; no source check was weakened |
| Linux `cargo test --workspace --all-targets --locked` | passed | 1,912 passed, 4 ignored |
| Linux `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` | passed | Compile-only sidecars |
| Linux `flutter test --no-pub --exclude-tags golden` | passed | 1,201 passed, 1 platform skip |
| Linux `flutter test --no-pub --tags linux-golden` | passed | 19 exact bitmap comparisons; no baselines changed |
| Linux `flutter build linux --debug --no-pub -t lib/main_preview.dart`; `flutter build linux --release --no-pub -t lib/main.dart` | passed | Both preview and production runner; final WebKit symbol correction rebuilt |
| Linux CMake socket, Proton policy, preferences, startup-I/O and NetworkManager fixture targets | passed | Corresponding inert executables completed; no system networking changed |
| `USQUE_NM_DNS_FIXTURE=... cargo test --locked -p usque-agent --lib private_network_manager_bus -- --ignored` | passed | One explicitly selected private D-Bus test; synthetic NetworkManager only |
| `python -m unittest discover -s tool -p "test_linux_package.py" -v` | passed | 20 tests, one skip; no package installation |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed | Complete aggregate gate, after supported Windows native initialization |
| `buf lint`; `buf format --exit-code --diff` | passed | Append-only capability field 52 |
| `python tool/check_repository_policy.py`; `git diff --check` | passed | Documentation/local-link/UTF-8 policy and whitespace |

Initial attempts exposed a WebView2 authentication-navigation ordering bug,
which was fixed before the successful browser runs. Sandbox-only native-tool
startup failures were retried through the same supported helper outside the
sandbox. Linux dependency downloads timed out; checks then used a private copy
of the already downloaded locked crate cache with Cargo offline mode. An initial
Linux invocation included Windows-only golden tests; the documented Linux
selectors above were subsequently run successfully. None of those unsuccessful
attempts was counted as a pass.

### Remaining acceptance / 尚未执行的验收

| Scope | Status | Limit |
| --- | --- | --- |
| Real Proton registration/login and completed human puzzle over WARP | not_run | All new HTTP peers and credentials were synthetic |
| Dedicated Android device/emulator WebView routing, process death and cleanup | not_run | No isolated Android runtime was provisioned or exercised |
| Real Linux WebKitGTK browser proxy/authentication behavior | not_run | WebKitGTK runtime was absent; dynamic-loading source compiled, policy fixtures passed |
| External IPv4/IPv6/DNS, third-party VPN, retained Kill Switch and leak observation | not_run | Requires the designated isolated network observer and platform environments |
| Windows installed VPN/TUN/WFP/route/DNS/system-proxy lifecycle | not_run | No privileged platform mutation or installer was run |
| Linux arm64/Ubuntu 22.04 package compatibility and protected performance runs | not_run | Local amd64/Ubuntu 26.04 compilation does not establish these |

The loopback WebView2 test proves scoped HTTP/CONNECT delivery and the preflight
proof checks. Its upstream intentionally rejects CONNECT; the separate puzzle
bridge tests use intercepted synthetic HTTPS documents. Neither establishes
real WARP egress, Proton acceptance or production TLS through the entire chain.
Windows proxy flags remain an Evergreen compatibility dependency and require
ongoing runtime testing. Unsupported runtimes stop verification explicitly.
Protected-runner absence remains optional, non-blocking and never a pass.

实现与离线检查已完成；真实受限网络、Android 真机、Linux WebKit 运行时和外部出口观测
仍为 `not_run`。没有创建真实 Proton 账号、安装应用、启动系统 VPN，或修改系统路由、
DNS、WFP、代理。功能与协议边界见 [使用说明](PROTON_VPN.md#verification-window-and-network-path--验证窗口与网络路径)
和 [协议说明](PROTON_VPN_PROTOCOL.md)。

## 2026-10-25 review fixes / 审查问题修复

The five review findings were addressed without changing the single-fallback
or mutation-delivery rules:

- Desktop capability decoding now reads protobuf field 52. A framed wire test
  covers both an enabled modern engine and a legacy response with no field.
- The probe namespace is now `<nonce>.usque-proton-proxy.invalid` on Rust,
  Android and desktop. Android's network security configuration denies cleartext
  by default and allows only that reserved namespace. The host test checks the
  manifest/configuration linkage and generated probe hosts; it is not a device
  WebView test.
- Manual verification openings are recorded alongside automatic openings. A
  stale `verification_failed` reply cannot consume the old handoff again. The
  widget regression exercises old waiting state, running state, then a new
  challenge, and checks that the job is never cancelled.
- HTTP errors inspect their underlying Hyper/I/O causes. TLS loopback peers
  cover EOF before response headers and a truncated response body, preserving
  `not_submitted=false`. Malformed HTTP stays a protocol error. Safe reads can
  migrate, while uncertain mutations remain non-replayable.
- Windows observes browser-owned resource failure events, including attached
  cross-origin frame/worker targets before their scripts run. Linux observes
  resource failure and the separate TLS-failure signal, with a bounded set of
  session-owned signal connections. Cancellation, policy rejection, unknown
  errors and HTTP status responses do not trigger migration. No page messages,
  request bodies or headers are used to classify failures.

Three independent review agents rechecked the fixes. That second pass identified
the separate Linux TLS signal and CORS error-domain distinction; both were
corrected and covered by the GObject signal fixture before final validation.

### Workstation checks

Commands run from the locations specified in `CONTRIBUTING.md`, using the same
pinned Rust, Flutter, Android NDK and SDK CMake as the preceding entry. Linux
checks used the disposable Ubuntu 26.04 amd64 copy, with the unchanged locked
Cargo cache. Logs and generated artifacts remain ignored local files.

| Command / scope | Result |
| --- | --- |
| `build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed |
| `build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | passed; 2,063 tests, 13 ignored |
| `build_windows_rust_release.ps1 -Variant x64-v2` | passed |
| `build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter pub get --enforce-lockfile`; `flutter build apk --debug --config-only --no-pub` | passed |
| `gradlew.bat --no-daemon :app:ktlintCheck`; `gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed; 492 JUnit tests, no failures |
| Windows `dart format --output=none --set-exit-if-changed lib test`; `flutter analyze --no-pub`; `flutter test --no-pub` | passed; 1,295 tests, 19 skipped |
| `prepare_windows_plugin_junctions.ps1 -FlutterProject .`; `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | passed |
| Windows `cmake --build apps/usque_gui/build/windows/x64 --config Release --target usque_proton_webview_test usque_proton_verification_test`, followed by both executables | passed; policy fixture and 14 WebView2 cases |
| Linux `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo test --workspace --all-targets --locked` | passed; 1,913 tests, four ignored |
| Linux `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` | passed |
| Linux locked pub resolution, format and analyzer checks; `flutter test --no-pub --exclude-tags golden`; `flutter test --no-pub --tags linux-golden` | passed; 1,203 widget tests, one skip; 19 goldens |
| Linux `flutter build linux --debug --no-pub -t lib/main_preview.dart`; `flutter build linux --release --no-pub -t lib/main.dart` | passed |
| Linux CMake socket, Proton policy/resource, preferences, startup-I/O and NetworkManager fixtures | passed; resource fixture covers network/TLS errors, policy exclusions and stale callbacks |
| `USQUE_NM_DNS_FIXTURE=... cargo test --locked -p usque-agent --lib private_network_manager_bus -- --ignored`; `python -m unittest discover -s tool -p "test_linux_package.py" -v` | passed; one private D-Bus test; 20 package tests, one skip |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed |
| `python tool/check_repository_policy.py`; `git diff --check` | passed |

The real WebView2 fixture serves synthetic documents and uses a loopback proxy
that never opens an upstream connection. It covers a successful HTML load
followed by script, image, fetch, XHR, cross-origin iframe-fetch and worker-fetch
network failures, plus cancellation/CORS/CSP exclusions, existing bridge cases
and authenticated proxy preflight checks. The Linux resource fixture invokes
production callbacks using local GObject signals, not a real WebKit browser.

Initial Windows attempts hit native DLL loading and temporary-file permissions;
the supported helper and standard user temporary directory were used for the
successful checks. The first stale-snapshot widget fixture incorrectly waited
for an indefinite progress animation to settle; it now advances the running
state explicitly. Failed attempts are not counted as passes.

All live/isolated acceptance rows in the preceding entry remain `not_run`,
including Android WebView execution, real Linux WebKit proxy routing, a real
Proton puzzle over WARP, external leak observation and privileged VPN lifecycle.
No installer, release APK, system VPN or system-network mutation was exercised.

## 2026-10-25 connected server selection / 连接中切换节点

The connected account's server picker now stages an explicit logical-server
choice with the account. **Apply and reconnect** uses the existing shared
settings transaction and protected exit replacement; choosing or cancelling in
the picker does not itself mutate a live account or reconnect. Existing TCP
connections may be interrupted during application. Account removal, refresh and
sign-in remain protected from use against the running account.

Capability 53 advertises support. Optional `ChainExitSettings` field 7 preserves
three states: absent inherits the account preference, empty explicitly selects
Automatic, and a bounded ASCII ID selects that logical server. The prepared
exit must match the requested override. Proton resolves its peer over the WARP
control channel and retains the account's keys and usable certificate. A missing
manual server fails instead of silently selecting another server. Selection
changes use the existing cancellation, final-traffic admission and cleanup
paths; no privileged platform mutation implementation was added.

New regressions cover Windows/Android/Linux widget interactions, cancel without
saving, failed save/retry, explicit Automatic, capability and protobuf presence,
Android selection identity, persisted profile roundtrip, hot-exit classification,
fresh peer resolution, immutable account keys and unavailable manual selection.
The server-name cache is bounded and contains only display metadata.

| Workstation check | Result |
| --- | --- |
| `build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed |
| `build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | passed; 2,066 tests, 13 ignored |
| `build_windows_rust_release.ps1 -Variant x64-v2` | passed |
| Windows locked pub resolution, Dart format, `flutter analyze --no-pub`, `flutter test --no-pub` | passed; 1,299 tests, 19 skipped |
| `prepare_windows_plugin_junctions.ps1 -FlutterProject .`; `flutter build windows --release --no-pub --split-debug-info=build/symbols/windows` | passed |
| `build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `flutter build apk --debug --config-only --no-pub`; `gradlew.bat --no-daemon :app:ktlintCheck`; `gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | passed; 493 JUnit tests, no failures |
| Linux `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo test --workspace --all-targets --locked` | passed; 1,916 tests, four ignored |
| Linux `cargo build --release --locked -p usque-engine -p usque-agent -p usque-update` | passed |
| Linux locked pub resolution, Dart format, `flutter analyze --no-pub`, `flutter test --no-pub --exclude-tags golden`; `flutter test --no-pub --tags linux-golden` | passed; 1,207 widget tests, one skip; 19 goldens |
| `flutter build linux --debug --no-pub -t lib/main_preview.dart`; `flutter build linux --release --no-pub -t lib/main.dart` | passed |
| Linux CMake socket, Proton policy/resource, preferences and startup-I/O fixtures from `CONTRIBUTING.md` | passed |
| `USQUE_NM_DNS_FIXTURE=... cargo test --locked -p usque-agent --lib private_network_manager_bus -- --ignored`; `python -m unittest discover -s tool -p "test_linux_package.py" -v` | passed; one private D-Bus test; 20 package tests, one skip |
| `pwsh -NoProfile -File tool/check_source.ps1` | passed |
| `python tool/check_repository_policy.py`; `git diff --check` | passed |

The first Linux Rust check encountered stale generated protocol bindings in the
disposable copy; regeneration and the complete Rust gates subsequently passed.
Windows native DLL loading and linker temporary-file restrictions required
retrying through the same supported helper. Failed attempts are not passes.

Real connected Proton node switching, failure/cancellation under network loss,
external leak observation, Windows VPN/TUN/WFP lifecycle, Android device VPN
lifecycle and Linux privileged network restoration remain `not_run`. These need
the corresponding isolated environments. Widget and memory-peer tests do not
establish live platform protection. No real Proton request, system VPN, installer
or release APK was exercised for this feature.

### 2026-10-25 dependency audit follow-up

[CI run 40197648510](https://github.com/GeorgeXie2333/usque-app/actions/runs/40197648510)
failed the shipping dependency audit on `crypto-bigint@0.7.5` because of
[RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html).
The workspace pin and lockfile now use the patched `0.7.6`; no other locked
package, audit exception or frozen Go-oracle source changed.

The following audit commands passed with the checksum-verified Windows build of
OSV Scanner 2.3.8 and the same inputs and policy as CI:

```powershell
python tool/export_gradle_runtime_sbom.py apps/usque_gui/android/app/gradle.lockfile target/proton-ci-android-runtime.cdx.json
target/osv-scanner-windows-amd64-2.3.8.exe scan --format json --output-file target/proton-ci-osv-shipping.json --config osv-scanner.toml -L Cargo.lock -L apps/usque_gui/pubspec.lock -L target/proton-ci-android-runtime.cdx.json
target/osv-scanner-windows-amd64-2.3.8.exe scan --format json --output-file target/proton-ci-osv-oracle.json --config osv-scanner.toml -L oracle/go/go.mod
python tool/check_osv_results.py --shipping target/proton-ci-osv-shipping.json --oracle target/proton-ci-osv-oracle.json --config osv-scanner.toml
```

The shipping scan found no vulnerabilities. The oracle scan completed with its
expected findings exit code; the policy check passed and kept those findings
informational. The Linux scanner attempt timed out during the OSV TLS handshake
and is not counted as a pass.

With `0.7.6`, the Windows and Linux Rust Clippy and test commands in the table
above were repeated successfully, including the Proton SRP proof vectors:
2,066 Windows tests (13 ignored) and 1,916 Linux tests (four ignored). The Windows
and Linux Rust release builds, Android arm64 Rust Clippy, and Android debug unit-test/lint
commands also passed again (493 JUnit tests, no failures). The unchanged Flutter
checks retain the results above; these deterministic checks do not change any
live/isolated `not_run` result.
