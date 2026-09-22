# H3 UDP chain latency / H3 UDP 链式首次访问延迟

Date: 2026-09-22. Baseline: `b2d42f0`. Exact final source and sanitized measurements
are recorded in [the candidate record](CHAIN_UDP_LATENCY_MEASUREMENTS.json).
The earlier [DNS report](CHAIN_DNS_VALIDATION.md) remains a historical record;
its DNS and CONNECT-only checks did not cover TLS handshakes.

## Reproduction and interpretation

The user reproduced slow first visits on Android system VPN with `b2d42f0`,
using both OpenVPN (Custom) UDP and WireGuard (Custom). OpenVPN TCP was unaffected.
Disabling application QUIC did not help. Changing the outer WARP transport from
H3 to CONNECT-IP H2 restored normal first visits on the user's Android device.

The authorized Windows loopback test, using the previously supplied temporary
SG WireGuard profile, reproduced the delay without TUN. DNS queries and TCP
CONNECT were fast, while nine complete HTTPS requests consistently took about
3.93–4.03 seconds. Stage timings placed about 3.87 seconds in TLS. A numeric
HTTPS destination had the same delay, so these measurements do not support a
DNS-only explanation. The same configuration over H2 completed HTTPS in about
0.19–0.25 seconds. An experimental TCP MSS ceiling of 1000 over H3 reduced the
same requests to about 0.18–0.27 seconds.

This is evidence of a packet-size-dependent failure on the nested H3 UDP path,
consistent with large TCP flights being lost and retransmitted. It does not
identify which remote hop drops the packet or prove that every external path has
the same limit. The experiment's fixed MSS is not the shipped policy.

本次已在无 TUN 的本地测试中复现 TLS 握手阶段的四秒等待，直接访问 IP 也出现；
改为 H2 或缩小 TCP MSS 后恢复。用户也确认 Android 切换 H2 后恢复。本次修复
针对 H3 嵌套 UDP 时的 TCP 包尺寸；不把此前 DNS 修复等同于解决此次症状。

## Change and boundaries

The shared chain packet path now limits both SYN and SYN-ACK MSS only for a
current H3 underlay with OpenVPN UDP or WireGuard. The complete inner TCP/IP
packet budget derives from WARP's existing 1280-byte private stack, the exit
endpoint's IPv4/IPv6 UDP headers, and protocol overhead:

- WireGuard reserves 32 bytes and rounds the plaintext budget down to a 16-byte
  boundary for peer-side padding: 1216-byte inner IP for IPv4 endpoints, 1200 for
  IPv6 endpoints. TCP/IP headers are subtracted to obtain MSS.
- OpenVPN reserves 128 bytes for its supported data-channel crypto/framing,
  including CBC/SHA512, without relying on a particular negotiated cipher.
- The user explicitly authorized this H3 chain ceiling even for `mssfix 0`.
  That directive still disables the native OpenVPN Core's own rewriting.
  Smaller explicit MSS values remain smaller; stored profiles are not rewritten.

The interface MTU stays at least 1280 and explicit WireGuard MTU is retained.
H2, OpenVPN TCP and direct-rule traffic are unchanged. Existing TCP connections
retain their negotiated MSS if the outer transport changes; new connections use
the current transport. UDP business datagrams retain the bounded fragment path;
this is not a claim that all UDP/MTU or throughput problems are solved.

The parser validates lengths, bounds extension traversal, rejects duplicate or
malformed MSS options, and avoids fragmented SYN, IP authentication headers,
TCP MD5 and TCP AO. Missing IPv6 MSS is added when header space permits, so the
implicit 1220-byte MSS does not defeat the ceiling. Unsupported/authenticated
forms are left unchanged and may still require H2. Incremental checksum updates
support odd option alignment and preserve an invalid original checksum rather
than repairing untrusted traffic. Only changed SYN headers cause a packet copy.

Admission, session generations, cancellation, bounded queues and fail-closed
shutdown are unchanged. Temporary packet-size traces and the experimental
environment override were removed. Retained debug timing logs contain only
stage, duration and success; the harness never prints configuration, keys,
addresses from the imported profile, DNS bodies or HTTPS response bodies.

## Validation

Final-candidate measurements (three fixed public HTTPS targets, three fresh
connections each; no claim that recursive DNS/server caches were cold):

| Path | Complete HTTPS median | TLS median | HTTPS successes |
| --- | ---: | ---: | ---: |
| Before fix, H3 | 3999.5 ms | 3873.6 ms | 9/9, delayed |
| Final candidate, H3 | 218.7 ms | 88.4 ms | 9/9 |
| Final candidate, H2 | 232.1 ms | 93.7 ms | 9/9 |

The final runs additionally passed 20 explicit UDP/TCP DNS exchanges and 10 SOCKS
domain CONNECT operations in total. HTTPS itself uses the same final private
packet stack through `InternalNetwork`, not a browser or OS VPN. The scope does
not establish Android VpnService lifecycle behavior or OpenVPN live-server results.

| Check / exact invocation | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed. |
| `tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed; helper uses workspace/all-targets, `--locked`, warnings denied. |
| Same helper with `-CargoAction test` | Passed: 1208 tests, eight explicitly ignored. |
| Same helper with `-Variant x64-v2` | Windows release build passed. |
| `python tool/check_openvpn_sources.py` | Five source/notice locks verified. |
| `cargo clippy -p usque-openvpn --all-targets --features interop-test --locked -- -D warnings` | Passed in initialized Windows native environment. |
| `cargo test -p usque-openvpn --features interop-test --locked` | 14 memory-only protocol tests passed. |
| `tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed with pinned NDK/CMake. |
| `tool/build_android_rust.ps1 -CargoAction build -AbiFilter all` | arm64-v8a, armeabi-v7a and x86_64 JNI debug libraries compiled. |
| Pinned Flutter `pub get --enforce-lockfile`; `build apk --debug --config-only --no-pub` | Passed; configuration only, no APK produced. |
| `gradlew.bat --no-daemon :app:ktlintCheck` | Passed. |
| `gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed: 218 tests across 31 suites; lint passed. |
| `pwsh -NoProfile -File tool/check_source.ps1` after Windows helper initialization, with pinned SDK on PATH | Rust format/Clippy, Dart format/analysis, ktlint and Ruff passed. Aggregate did not pass: PSScriptAnalyzer 1.25.0 format-file import blocked by host software-restriction policy. |
| `python tool/check_repository_policy.py`; `git diff --check` | Passed. |

New regressions cover SYN/SYN-ACK, IPv4/IPv6, odd MSS alignment, missing IPv6
MSS, unchanged payload, checksum validity, invalid-checksum preservation,
H2/TCP/smaller-MSS passthrough, duplicate/malformed options, IP fragments and
authenticated headers. Property tests also exercise arbitrary packets and valid
IP/TCP envelopes with arbitrary TCP options. Temporary experimental packet traces
and fixed-MSS overrides are absent from final production code.

The opt-in engine test remains
`chain_dns_live_tests::live_wireguard_dns_through_loopback_socks_without_tun`.
Set `USQUE_LIVE_HTTPS=1` in addition to its explicit configuration paths and H3/H2
selector to require all nine HTTPS transfers, with a bounded overall timeout.
The runtime disables TUN, system proxy and Kill Switch, binds only a high
127.0.0.1 SOCKS port, keeps imported secrets in a temporary encrypted record,
and verifies listener closure after shutdown. It makes no route, interface-DNS
or WFP changes. No temporary keys or raw logs are part of this change.

An intermediate fixed-code live run had variable DNS/TCP/HTTP delays and one TLS
failure; it is retained as a failed/noisy run, not erased or presented as a pass.
The older live harness only required one HTTPS success and therefore returned
success for that run. The final harness requires all nine. The next run with the
same runtime policy completed all nine in 0.17–0.23 seconds. Final-candidate runs
are identified separately in the candidate record.

Real Android testing of this patch, Windows VPN, external leak observation and
controlled throughput/CPU sampling are `not_run`: there is no matching isolated
environment in this task. The Android user's successful H2 workaround is not
verification of the new binary. Live OpenVPN UDP with user credentials and a
Windows ARM64 release build were also not run. No MSI/release APK, installation, signature,
push or publication is included.
