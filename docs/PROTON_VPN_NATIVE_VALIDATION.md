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
