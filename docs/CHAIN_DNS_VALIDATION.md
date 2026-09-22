# Chain DNS follow-up / 链式 DNS 后续验证

Date: 2026-09-22. Baseline: `3f7e678`.
The tested source is identified by the changed-file digests in
[the candidate and measurements record](CHAIN_DNS_MEASUREMENTS.json).
Earlier [chain validation](CHAIN_PROXY_FIX_VALIDATION.md) is historical and is
not rewritten by this follow-up.

## Confirmed defect and changes

The system-VPN DNS server previously selected between a socket receive RPC and
its reply queue. If a reply won after the other RPC had removed a query from the
stack, dropping that RPC discarded the query. The upstream RPC has no socket-level
buffer for that consumed result. An in-memory regression against the original
code returned **64 of 128 queries** and timed out; the fixed independent receiver
and writer return **128 of 128**. This reproduces a code defect, not the exact
Android wall-clock symptom.

The receive operation now persists while replies are sent. Input and output
remain bounded, session cancellation stops both directions and their children,
and the UDP socket uses the reliable full-command-queue cleanup owner.

Final-exit DNS gains automatic TCP alternatives in both the system-VPN DNS
forwarder and the SOCKS/HTTP resolver. All configured servers get a UDP first
attempt before TCP alternatives. A single server gets TCP after 250 ms; multiple
servers keep the 250 ms UDP backup and start TCP when capacity becomes available.
Both protocols share two active slots, at most one second per attempt and a
four-second overall deadline. With many candidates, shorter attempt budgets
reserve opportunities for the later servers. Truncation starts TCP immediately.

The existing bounded TCP pool is reused and bound to the actual stack's session
cancellation token. Queries validate their question, transaction ID and response.
Valid negative answers are terminal; truncated TCP answers are rejected and their
connection discarded. A cancelled partial TCP exchange is not returned to the
pool. No physical DNS fallback or endpoint-resolution policy change is introduced.

Independent `gpt-6-astra` source review caught an intermediate fairness regression:
retrying UDP/TCP in pairs could starve the fifth DNS server. The final policy gives
each server its first opportunity and reserves time; a specific fifth-server
regression and an eight-server concurrency/deadline test cover it. The final delta
review found no further definite defect in those changes or the no-TUN harness.

## Authorized live test boundary

The user explicitly authorized a temporary local high-port SOCKS5 test and
provided a temporary WireGuard configuration, while forbidding TUN. The harness
copies the existing nonsecret app settings into a test-owned temporary directory,
reads the existing WARP identity, and writes pin-refresh results only to a memory
vault. The supplied WireGuard configuration is imported into a temporary encrypted
DPAPI record; its plaintext, keys and original file are not copied into the repo.

Every run forcibly disables `frontends.tunnel`, system proxy and Kill Switch,
clears direct rules, and binds only `127.0.0.1` on a dynamically assigned port above
1024. It calls the transport data plane directly, without an Agent or OS TUN
interface. Successful measurement runs cancel and shut down the runtime and assert
the loopback listener no longer accepts connections. Failed startup performs the
normal chain cleanup. No routes, interface DNS, WFP or system proxy are changed.

The user confirmed this temporary configuration works in another WireGuard
client. On this machine, the nested WARP-to-WireGuard path did not finish its
handshake with either H3 or H2; each attempt failed at approximately 35 seconds.
The instrumented H3 attempt bound its UDP transport and submitted **7 packets to
the private WARP stack**, with **0 packets delivered to its receive interface**.
This does not prove that packets left WARP, identify where they were lost, or
establish a bad configuration or remote blocking policy. UDP connection-close
diagnostics log only counts and family, never keys, endpoints or payloads.

WARP-only SOCKS testing succeeded for 40 explicit DNS transactions: four configured
IPv4/IPv6 DNS addresses, UDP/TCP, and five A/AAAA queries per combination. TCP's
first connection and subsequent query time are measured separately. Five SOCKS
domain CONNECT requests also succeeded. These are fresh client queries, with no
claim that the recursive servers' own caches were cold. No DNS response body is
included in the report.

Measured values and candidate hashes are in the linked JSON record. The repeated
WARP-only run recorded UDP median **4.067 ms** and TCP median **3.962 ms**, with an
additional **6.157 ms** median TCP connection establishment. This illustrates that
TCP is not inherently faster; it is an alternate transport when UDP fails.
The dataset identifies the exact live-test source separately: the later runtime
change caps retained oversized rejected UDP requests to 4096 bytes, and a
test-only fixture now retries a bounded number of shared TCP/UDP port choices.
The normal-sized live queries did not exercise that rejection path; no rerun of
live tests after the final memory-bound adjustment is claimed.

For that first temporary WARP WireGuard profile, final-exit DNS measurements were
not reached because its handshake did not complete. The successful follow-up
with another user-supplied profile is recorded below. The Android four-second
symptom is not marked fixed by these SOCKS-only tests: the system-VPN DNS listener
is covered by the in-memory regression, not by these live SOCKS requests. No
Android device lifecycle, OS VPN, external leak or isolated performance-lab result
is claimed.

## Follow-up with the temporary SG configuration

The user subsequently supplied `wg-SG-FREE-1.conf` and authorized testing before
revoking its temporary key pair. The final test executable and unchanged candidate
source successfully established **SOCKS5 → WARP → WireGuard** with both outer H3
and H2. This profile has one configured IPv4 DNS server. Each run completed five
UDP queries, five TCP queries (reusing the TCP connection), and five SOCKS domain
CONNECT operations. Every operation succeeded; no four-second delay occurred.

| Outer transport | UDP query median | TCP query median | TCP initial connection | SOCKS domain CONNECT median |
| --- | ---: | ---: | ---: | ---: |
| H3 | 46.211 ms | 114.227 ms | 47.277 ms | 93.726 ms |
| H2 | 46.793 ms | 91.800 ms | 52.772 ms | 94.924 ms |

TCP query times exclude the separately reported initial connection. This is a
small functional comparison using the current implementation; it does not prove
an intrinsic TCP/UDP performance difference or control recursive-server caching.
In this observed chain, forcing TCP would not improve latency. The UDP-first
policy with bounded TCP alternatives remains appropriate.

The authenticated handshake and returned business packets distinguish this result
from the earlier WARP-to-WARP handshake timeout. The cause of that earlier timeout
remains undetermined. Both SG runs disabled TUN, system proxy and Kill Switch,
used only temporary high loopback SOCKS ports, and passed the listener-closed
assertion after shutdown. No supplied keys, profile contents or ordinary account
settings were modified. The dataset's `nested_wireguard_sg` section contains
sanitized timings, counts, the candidate digest and test-executable hash.

## Reproduction and checks

The ignored live entry point is
`chain_dns_live_tests::live_wireguard_dns_through_loopback_socks_without_tun`.
It requires explicit `USQUE_LIVE_CONFIG` (enrolled app settings) and
`USQUE_LIVE_WIREGUARD` (temporary configuration) environment variables.
`USQUE_LIVE_WARP_ONLY=1` enables the separate WARP-only control;
`USQUE_LIVE_TRANSPORT=h3` or `h2` selects the outer transport.
Neither variable permits TUN. Do not enable the unrelated live tests as a group.

In a fresh PowerShell process, initialize MSVC/Ninja/libclang with the Windows
helper, set `RUSTFLAGS=-C target-cpu=x86-64-v2` for subsequent Cargo commands,
then compile with:

```powershell
cargo test -p usque-engine live_wireguard_dns_through_loopback_socks_without_tun --locked --no-run
```

Run only the resulting engine **library test executable**, with the exact test
name plus `--exact --ignored --nocapture --test-threads=1`. Logs are restricted to
test stages and timing/counter metadata. Local logs are ignored under
`target/chain-dns-validation`; they are not committed.

| Check | Result |
| --- | --- |
| Original DNS receive regression | Failed: 64/128 replies, two-second test deadline. |
| Fixed receive regression | Passed: 128/128 replies. |
| Windows helper workspace tests, locked | 1,200 passed; eight explicitly ignored, including the opt-in live harness. |
| DNS regressions | TCP hedge, reuse, negative answers, truncated TCP rejection, UDP backup precedence, fifth-server fairness, shared deadlines and cancellation passed. |
| Android arm64 Clippy, pinned NDK/CMake | Passed. |
| Android JNI debug build, all three ABIs | Passed. |
| Windows helper workspace Clippy, locked | Passed. |
| Windows x64 release | Passed with the final runtime changes. |
| Rust format and repository policy | Passed. |
| Android configuration-only, ktlint, unit tests and lint | Passed; 218 Kotlin tests across 31 suites. No APK produced. |
| Aggregate `check_source.ps1` | Rust format/Clippy, Dart format/analysis, ktlint and Ruff passed. PSScriptAnalyzer 1.25.0 could not import because host software-restriction policy blocks its format file. The aggregate did not pass; policy was not bypassed. |
| Live nested WireGuard DNS, first WARP profile | Not reached: H3/H2 chain startup failed; not a DNS-test pass. |
| Live nested WireGuard DNS, subsequent SG profile | Passed under H3 and H2: 20 DNS queries and 10 domain CONNECT operations, with loopback cleanup verified. |
| Android device and OS VPN/leak checks | `not_run`; user forbids TUN and no corresponding isolated environment was used. |

One intermediate workspace rerun failed before protocol execution when Windows
rejected a test's randomly selected matching UDP port (`WSAEACCES 10013`). The
fixture now retains a TCP/UDP pair only after both binds succeed, retries at most
32 port-conflict/permission failures, and still fails if no pair is available.
The complete final rerun passed; no firewall, reservation or security policy was
changed. The aggregate's PSScriptAnalyzer blocker remains unresolved and is not
counted as a pass.

The required commands are the Windows helper Clippy/test/release commands,
`cargo fmt --all --check`, pinned Android arm64 Clippy and all-ABI JNI build,
Android debug configuration-only, ktlint/unit tests/lint, repository policy and
`git diff --check`, as specified in [Contributing](../CONTRIBUTING.md).
No MSI/release APK, installation, official signing or publication is included.
