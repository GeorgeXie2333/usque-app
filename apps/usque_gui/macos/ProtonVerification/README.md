# Standalone macOS Proton verification component

This component uses CEF on macOS 13 and newer. There is no WKWebView fallback.
The browser runs separately from Flutter and the Engine and accepts one bounded
verification handoff over stdin. The Flutter launcher verifies the bundled
component's code identity before transmitting credentials. Release launch
requires the configured Developer ID identity; the unsigned development DMG
does not enable this account flow merely because the component is present.

## Dependency and build

CEF's [official binary-distribution instructions](https://github.com/chromiumembedded/cef/blob/master/docs/general_usage.md)
link to the [CEF automated build distribution](https://cef-builds.spotifycdn.com/index.html).
The reviewed matching stable build is
`156.0.14+g4fa4ab5+chromium-156.0.8078.48` (metadata checked 2026-10-29).
Both SDKs and their framework declare macOS 13 compatibility.

| Architecture | Distribution | Official SHA-1 | Pinned SHA-256 |
| --- | --- | --- | --- |
| Apple Silicon | `macosarm64_minimal` | `7359a9d293dd5c480bec3f18b762731b483b0e13` | `ed994b907ad596ca609a0542a1e2b22aa97e93e98b8141fed4c24aef39b9ccf6` |
| Intel | `macosx64_minimal` | `bc46c83878741fa2b23486940dacebabb294bf39` | `e584ade521802f8c2779d24996b32ddc183e8502743e5e3910b7247bb08f24f1` |

Download those exact minimal SDK archives from the official distribution over
HTTPS into `.toolchains/cef/macosarm64.tar.bz2` and
`.toolchains/cef/macosx64.tar.bz2`. These local dependencies are ignored by Git.
Then run:

```sh
source .toolchains/env.sh
python3 tool/build_macos_proton_verification.py --arch arm64
python3 tool/build_macos_proton_verification.py --arch x86_64
```

The default builder verifies the archive SHA-256 and freshly extracts the
consumed SDK into a generated private staging directory with Python's safe
tar extraction filter. It does not trust a separately editable extracted SDK.
Python with `tarfile.data_filter` (Python 3.12 or newer), CMake, Ninja and the
repository's Xcode toolchain are required. The explicit `--cef-root` developer
override is recorded as a custom unverified SDK in the build report.

The builder creates `build/macos-proton-verification/<architecture>/` with the
standalone app, CEF framework, five sandbox helper app variants, licenses and a
build report. All Mach-O files must match the selected architecture and support
macOS 13. These local outputs receive ad-hoc integrity signatures; they are not
Developer ID signed or notarized.

CEF's framework is loaded dynamically in the browser and helper processes.
Each child initializes `CefScopedSandboxContext` before loading the framework;
sandbox failure terminates the child. The browser always sets
`no_sandbox = false`. The build rejects `USE_SANDBOX=OFF`.

## Network and message boundaries

- stdin: one 4-byte big-endian length followed by at most 65,536 bytes of the
  existing sensitive JSON handoff. No secrets are passed in arguments,
  environment variables, logs or handoff files.
- stdout: one bounded frame containing `status: success`, `cancelled` or
  `error`. Success contains the original validated message as a JSON string;
  errors contain a `PROTON_VERIFICATION_*` code. The parent additionally waits
  for a clean process exit and rejects trailing bytes.
- Cancellation closes stdin. The browser also enforces wall-clock and
  monotonic expiration, bounded loading and proxy readiness.
- A version-2 physical-route handoff returns the existing
  `USQUE_VERIFICATION_NETWORK_FAILED` message before starting the browser. This
  requests the Engine's existing authenticated proxy recovery path. The
  component does not fall back to the physical network.
- Proxy handoffs use an in-memory request context and a fixed loopback HTTP
  proxy. Proxy authentication requires the exact host, port, Basic scheme and
  challenge realm. A probe verifies authenticated HTTP and CONNECT handling
  before the official challenge is loaded. Proxy or resource transport failure
  ends the verification.
- CEF process metadata uses a short, atomically created private directory with
  mode `0700`; normal shutdown removes it. The handoff never enters that
  directory. The browser profile remains in memory.
- The browser and its child processes use `use-mock-keychain` so Chromium Safe
  Storage does not access the user's macOS Keychain or ask to create a keychain
  under the isolated tests' temporary HOME. This uses mock encryption and is
  suitable only for this one-shot, in-memory profile: do not enable persistent
  cookies, saved passwords or a disk cache with this setting. It does not
  disable TLS certificate validation or the CEF sandbox. A test keychain popup
  should be cancelled; resetting the user's default keychain is unnecessary.
- Network resources are restricted to the official HTTPS
  `verify.proton.me`, `verify-api.proton.me` and `account.proton.me` origins.
  Top-frame navigation must retain the handoff's challenge digest. CAPTCHA
  frames, event origin/source, expiry and returned token schema reuse
  `native/proton_verification.h`; Swift does not implement different semantics.
- The native bridge accepts only the bound main frame and one session nonce.
  Downloads, popups, permission prompts, certificate exceptions, persistent
  browser storage and console logging are disabled.

## Isolated validation

```sh
python3 tool/test_macos_proton_verification.py \
  --app build/macos-proton-verification/arm64/UsqueProtonVerification.app

python3 tool/test_macos_proton_verification.py \
  --app build/macos-proton-verification/arm64/UsqueProtonVerification.app \
  --smoke --output build/macos-proton-verification/arm64/isolated-tests.json
```

The first command exercises malformed framing and proxy handoff recovery
without opening a browser. The second additionally requires WindowServer and
starts only a fixed local data page and synthetic loopback proxy fixtures,
including bound Basic authentication and a rejected foreign realm. It
requires a clean browser exit; receiving a result followed by a forced kill or
Chromium teardown watchdog failure is a failed test. Each fixture has a bounded
timeout and cleans only its own process group.

Current validation (2026-10-29): both architecture builds pass their native
Mach-O, macOS 13 minimum-version and ad-hoc integrity checks. The five framing
and handoff-recovery checks pass on Apple Silicon. After the Keychain isolation
change, a focused local renderer smoke returns success and exits cleanly with
status `0`; its observed GPU, renderer and utility processes all carry
`--use-mock-keychain` (see the generated `keychain-focused-smoke.json`). The full
`--smoke` suite still **fails** at `Authentication proxy failure did not end
cleanly` (see `keychain-smoke-tests.json`). Earlier runs also showed intermittent
Chromium teardown watchdog failures with exit status `2`, including in a minimal
client. The focused Keychain check does not establish full lifecycle or
proxy-authentication reliability. Official account flow acceptance remains
incomplete; keep the signed-launch capability gated.

Cross compilation does not demonstrate Intel execution. These fixtures do not
prove an official Proton challenge, signed application launch, macOS 13 device
behavior or full VPN safety. Those acceptance results remain separate.
