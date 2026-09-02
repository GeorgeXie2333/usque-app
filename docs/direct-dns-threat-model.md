# Direct DNS threat model

## 1. Overview and effective resources

Scope: direct DNS and its actual callers, protection and diagnostic/export
boundaries; this is not a repository-wide vulnerability scan. The independent
architecture review used `1558ebfebe1ed38e969578628ffd8138166519e9`; the final
source check includes PR-12's flags, Android startup correction and native
timeline bridge. The stable target is the sanitized repository identity
`https://github.com/GeorgeXie2333/usque-app`, SHA-256
`a09f265cd93232d0857d07e515244e870406de2d40dffdfda6132591abbaa587`.
This scoped document does not replace a shared whole-repository model.
Root `SECURITY.md` is the applicable policy; no nested policy was found.

Usque has one MASQUE runtime shared by VPN and proxy frontends. Geo-selected
direct traffic alone consumes `DirectDnsSettings`: System, DoH or DoT.
`SharedNetworkSettings` is hydrated into an account's runtime Profile;
managed-account endpoint overlays do not override DNS. Core validation is
authoritative after Flutter/protobuf or Android JSON decoding. The default is
System, including migrated/missing configuration. See
`crates/usque-core/src/config/network.rs:39`,
`crates/usque-core/src/config/mod.rs:742`, and
`proto/usque/v1/control.proto:153`.

| Component | Responsibility / source |
| --- | --- |
| Core and editor | Canonical name/path/bootstrap/port; explicit user selection; `crates/usque-core/src/config/mod.rs:742` |
| Transport resolver | Strict TLS, protocol validation, bounded generation-scoped pool; `crates/usque-transport/src/encrypted_dns.rs:142` |
| Split DNS and Geo callers | Classify before lookup, validate responses, maintain generation-scoped route hints, preserve encrypted answers across data-path fallback; `crates/usque-transport/src/split_dns.rs:302`, `crates/usque-transport/src/geo_direct.rs:319` |
| Platform protection | Windows authenticated Agent and Android exact Network binding; `crates/usque-agent/src/windows/server.rs:308`, `crates/usque-android/src/lib.rs:1737` |
| Diagnostics/export | Read-only Standard, bounded Deep and allowlisted local exports; `crates/usque-engine/src/diagnostics/runner.rs:16`, `crates/usque-engine/src/maintenance.rs:140` |

The following table retains each distinct deployment/workflow resource. No
configured resolver, account identifier, real endpoint or secret is reproduced.
“Encrypted bounds” means 1–8 numeric bootstrap IPs, four live socket permits
(including connecting/retiring/Happy-Eyeballs losers), 64 admitted queries,
two bootstrap visits, one retry, four seconds total, strict public-root/name/
validity TLS, no early data, DoH h2 POST/200, and DoT length framing. DoH has
16 streams per connection; DoT has one query per connection. Sources:
`crates/usque-transport/src/encrypted_dns.rs:38`,
`crates/usque-transport/src/encrypted_dns.rs:516`,
`crates/usque-transport/src/encrypted_dns.rs:1090`.

| Deployment / consumer | Configuration chain and effective resource | Readers / writers / recipients | Enforcing control and evidence / unknowns |
| --- | --- | --- | --- |
| Windows VPN / System Split DNS | Shared Profile → physical snapshot → internal `198.18.0.1` / `fd00::1` → physical DNS endpoints | User config; Agent snapshot; physical provider receives direct QNAME; other names use WARP | Target-aware leases, generation checks, WFP permits when Kill Switch is on; `crates/usque-engine/src/windows_agent.rs:665`, `crates/usque-transport/src/split_dns.rs:903`. External observer `not_run`. |
| Android VPN / System Split DNS | Profile → internal listeners → selected LinkProperties DNS, preserving IPv6 scope | User config; selected non-VPN Network DNS provider | VpnService protect + Network bind, stale-response SERVFAIL; `apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/PhysicalNetworkMonitor.kt:100`, `crates/usque-transport/src/split_dns.rs:903`. Device/observer `not_run`. |
| Windows VPN / DoH or DoT | Profile → PacketStack → ConfiguredDnsProtector → explicit numeric bootstrap; encrypted bounds | User-selected encrypted provider sees direct QNAME; physical DNS metadata is not consumed by resolver | Agent exact-generation target TCP lease/interface/WFP; `crates/usque-agent/src/windows/server.rs:308`, `crates/usque-transport/src/encrypted_dns.rs:697`. Startup still reads physical metadata for network state, not fallback. Observer `not_run`. |
| Android VPN / DoH or DoT | Android JSON → core validation → exact-Network resolver; encrypted bounds | User-selected encrypted provider; no system bootstrap lookup | Protect before exact Network bind; `crates/usque-android/src/lib.rs:1737`, `crates/usque-transport/src/encrypted_dns.rs:697`. PR-12 removes unnecessary physical-DNS-list startup dependency; a usable non-VPN network is still required. Device `not_run`. |
| Windows proxy / System direct hostnames | Profile → NoopSocketProtector → OS resolver → direct target; existing tunnel fallback | OS-selected DNS provider and direct target | Geo defaults Tunnel; proxy is not VPN egress authorization; `crates/usque-engine/src/lib.rs:1259`, `crates/usque-transport/src/socket.rs:231`. System-proxy lease only configures loopback proxy use. |
| Android proxy / System direct hostnames | Proxy route policy → selected Network.getAllByName → bound target socket; no TUN DNS listener | Underlying Network DNS and direct target | Network binding without VpnService.protect; `crates/usque-android/src/lib.rs:1737`, `apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/UsqueVpnService.kt:1642`. Device `not_run`. |
| Windows proxy / DoH or DoT | Noop protector → configured resolver → HTTP/SOCKS Geo caller; encrypted bounds, logical generation 0 | Configured encrypted provider and direct target | Strict TLS/no downgrade, but no Agent/WFP lease; `crates/usque-engine/src/lib.rs:1259`, `crates/usque-transport/src/encrypted_dns.rs:1226`. No VPN Kill Switch promise. |
| Android proxy / DoH or DoT | Proxy policy → exact-Network resolver → HTTP/SOCKS; encrypted bounds | Configured encrypted provider and direct target | Generation binding without VpnService.protect; `crates/usque-android/src/lib.rs:1737`, `crates/usque-transport/src/geo_direct.rs:319`. Device `not_run`. |
| Both platforms / independent proxy DNS modes | ProxyDnsMode selects tunneled WARP, configured UDP/53, or system resolution for non-direct traffic | Mode-selected resolver | Separate from direct_dns. Encrypted direct DNS failure is terminal; data fallback after a successful answer reuses those IPs; `crates/usque-transport/src/dns.rs:55`, `crates/usque-transport/src/geo_direct.rs:319`. DoH direct selection is not a claim about unrelated port-53 traffic. |
| Windows active runtime / Deep DNS probe | Active runtime protector → separate short-lived pool → fixed reserved `example.invalid` | Explicit Deep caller; configured encrypted provider | 3.8 s I/O + cleanup within check/session limits; no business-pool mutation; `crates/usque-engine/src/lib.rs:1013`, `crates/usque-transport/src/diagnostic_probe.rs:27`. Not an observer. |
| Windows disconnected / Deep DNS probe | Lifecycle exclusion → Noop protector → encrypted resolver, generation 0 | Explicit Deep caller; configured provider over normal host networking | TLS and bounded ownership only; no Agent physical snapshot/WFP proof; `crates/usque-engine/src/lib.rs:1031`. Reachability is transport-only. |
| Android active/disconnected / Deep DNS probe | ServiceDiagnosticProbes → JNI → ProbeProtector → reserved query | One explicit pending probe; configured provider on current Network | Exact generation before/after bind, native cancellation slot and serialized worker; `crates/usque-android/src/diagnostic_probe.rs:194`, `apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/ServiceDiagnosticProbes.kt:11`. Device `not_run`. |
| All deployments / quality telemetry | Resolver enum/counter/RTT → 1 Hz snapshot → local UI memory | Runtime writes; local Engine/UI reads | No name/address/query payload fields; `crates/usque-transport/src/network_quality.rs:300`, `apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/NetworkQualityFields.kt:5`. No automatic upload/history persistence. |
| Windows/Android / diagnostic ZIP | Snapshot/session/timeline/logs → allowlist → user-selected local file | Explicit exporting user | Custom DNS, endpoints, QNAME, secrets, paths, SSIDs and package lists excluded; `crates/usque-engine/src/maintenance.rs:140`, `apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/AndroidMaintenance.kt:51`. Native timeline is separately bounded. |
| Explicit configuration serialization | Validated Profile → protobuf/Android JSON | Configuration caller receives chosen name/path/bootstrap/port | Intentionally preserves settings, unlike diagnostic export; `crates/usque-android/src/lib.rs:1553`, `crates/usque-engine/src/lib.rs:3361`. Do not conflate these workflows. |
| Windows VPN / Geo direct TUN TCP/UDP | Numeric packet → DNS route hint/GeoIP → bounded generation-scoped gateway mapping | TUN client and explicitly direct target | No hostname re-resolution for numeric flow; exact target/protocol/interface Agent lease; `crates/usque-transport/src/direct_gateway.rs:243`, `crates/usque-agent/src/windows/server.rs:308`. Observer `not_run`. |
| Android VPN / Geo direct TUN TCP/UDP | Same numeric gateway classification → protected exact Network socket | TUN client and explicitly direct target | Requires TUN-direct protector capability, bounded mapping and generation; `crates/usque-transport/src/direct_gateway.rs:243`, `crates/usque-android/src/lib.rs:1784`. Device/observer `not_run`. |

## 2. Trust boundaries, assets and objectives

Assets are configuration integrity; QNAME confidentiality and answer integrity;
exact physical egress authorization and lease lifetime; separation of Split,
WARP, physical and encrypted DNS; bounded sockets/queries/tasks; local user
traffic; sanitized metrics/exports; WARP key material and Agent operation
ownership. Credentials remain secure-store references, not model contents.

Boundaries are user/editor → authoritative core; shared configuration → hydrated
Profile; DNS wire input → bounded parser/route hints; transport → resolver;
Engine → authenticated privileged Agent; JNI → VpnService/Network; runtime →
local telemetry; and explicit diagnostic/configuration export → selected file.
Windows pipe authentication checks PID/SID/image/signer and operation ownership
(`crates/usque-agent/src/windows/auth.rs:107`,
`crates/usque-agent/src/windows/server.rs:308`). Android separates VPN protect
from proxy-only Network binding. System exposes direct names to the selected
physical provider by design; encrypted mode exposes them to the chosen TLS
provider, which can still return unwanted but syntactically valid answers.

Realistic attackers include a network observer/on-path actor, malicious
resolver, malformed-input sender, and a local or explicitly exposed proxy
client. A profile author controls DNS settings but cannot disable core TLS
validation. Unprivileged processes do not gain Agent permissions by knowing
a target IP. Host administrator/LocalSystem/root, a compromised OS, and an
already-authorized operator changing their own resolver are outside this model.

Objectives:

- `INV-DIRECT-DNS-NO-PLAINTEXT-FALLBACK`: encrypted failures are terminal or
  SERVFAIL. Numeric bootstrap never causes system lookup; post-resolution
  direct data fallback reuses the encrypted answer.
- Strict chain/name/time verification, canonical names, DoH ALPN/status/media/
  body validation, DoT framing and shared DNS response correlation; no early
  data, certificate-ignore control, custom production CA or hidden downgrade.
- PhysicalSystem is explicit; tunnel DNS is not silently replaced. Generation
  changes invalidate encrypted pools, queued replies and route hints.
- Socket protection precedes I/O; lease lifetime covers I/O destruction, with
  deployment differences above. Kill Switch rules are not relaxed for probes.
- Queue, pool, retry and deadline limits survive errors and cancellation;
  Standard remains read-only; Deep cleans its temporary resources.
- Local metrics and diagnostic exports exclude private names/addresses and
  cannot claim external zero-packet proof.

Assumptions: trusted platform network snapshots/binding, secure profile storage,
Geo data and public-root store; no DNSSEC, ODoH or resolver-content trust beyond
TLS plus DNS correlation is claimed. Architecture review and loopback tests
are not protected evidence. Windows VM, dedicated Android, independent
observer and controlled performance lab are all `not_run` here.

Resolved discrepancies and residual questions:

- README/installation privacy text now describes explicit encrypted modes;
  PR-00's historical baseline remains historical, not current authority.
- Android's former custom-mode physical-DNS-list prerequisite is removed and
  tested in PR-12; the non-VPN network requirement is retained.
- Desktop proxy/disconnected Noop protection is now explicitly documented,
  not mislabeled as Agent protection.
- Legacy PhysicalSystem exchanges use target-aware protection with pre/post
  generation checks, not every encrypted-path exact-generation overload.
  `resolve_physical_host` loops bounded server/A/AAAA lists with per-exchange
  limits, but has no single encrypted-style four-second total budget. These
  unchanged legacy semantics are not evidence of an encrypted fallback.
  Sources: `crates/usque-transport/src/split_dns.rs:903` and
  `crates/usque-transport/src/split_dns.rs:1137`.
- Agent chooses a physical interface by the verified endpoint-path family,
  not a fresh route lookup for each custom resolver. Multi-homed reachability
  needs protected testing; failure is not permission to broaden routing.
  Source: `crates/usque-agent/src/windows/server.rs:308`.
- Actual zero port-53/direct-rule/candidate-path packet observations and
  platform cleanup remain unknown until exact-candidate protected evidence.

## 3. Prioritized attack stories (hypotheses, not findings)

| Priority | Scenario and attacker gain | Prerequisite / impact | Existing controls, mitigation and evidence |
| --- | --- | --- | --- |
| High | On-path actor forces encrypted TLS/DNS failure hoping to obtain plaintext QNAME | Victim chose DoH/DoT and issues a direct query; confidentiality loss if a downgrade exists | Encrypted enum dispatch, numeric bootstrap, terminal errors/SERVFAIL and IP-reusing data fallback. Loopback/fault/proxy tests cover negative paths; independently observe port 53 before claiming packet proof. `crates/usque-transport/src/encrypted_dns.rs:565`, `crates/usque-transport/src/geo_direct.rs:319`. |
| High | Network churn races setup/response delivery and sends data on the wrong network | VPN direct traffic, an actual generation race and failed ownership check | Exact-generation binding, I/O-owned leases, 100 ms pool observer, synchronous boundaries and queued-reply check; actor/generation tests. Verify real OS transitions on dedicated runners. `crates/usque-transport/src/encrypted_dns.rs:697`, `crates/usque-transport/src/split_dns.rs:264`. |
| High | Unprivileged local process asks Agent for an unauthorized direct exception | Must cross pipe authentication and operation ownership; possible Kill Switch bypass | PID/SID/path/signer, exact target/protocol/interface, generation and registry caps; no generic route-bypass API. Agent unit tests and isolated WFP evidence, not public exploit claims. `crates/usque-agent/src/windows/auth.rs:107`, `crates/usque-agent/src/windows/server.rs:308`. |
| Medium | Malicious resolver returns cross-query, oversized or truncated data to poison a route hint or exhaust memory | Legitimate configured resolver connection; wrong destination or service degradation | ID/question/record/TTL/CNAME validation, capped response/body/query/task/pool sizes, one retry and deadlines; property tests and malformed TLS-server fixtures. Resolver returning a valid unwanted answer is a trust assumption, not TLS bypass. `crates/usque-transport/src/split_dns.rs:892`, `crates/usque-transport/src/encrypted_dns.rs:860`. |
| Medium | Proxy client floods requests or drops a partial exchange to retain sockets/tasks | Authorized local proxy access or explicitly exposed listener; availability impact | Four actual I/O permits, 64 queries, Split DNS 512-task cap, bounded waits/cancellation; cancellation/pool-saturation tests. Preserve these limits and run thermal/performance lab separately. `crates/usque-transport/src/encrypted_dns.rs:274`, `crates/usque-transport/src/encrypted_dns.rs:516`. |
| Medium | Private query/endpoint leaks through a failure, timeline or diagnostic bundle | User exports a bundle or an ordinary log captures raw data | Fixed enums/numbers, explicit export allowlist and bounded native bridge; adversarial export fixtures. Configuration export intentionally contains chosen DNS settings and is a different workflow. `crates/usque-engine/src/maintenance.rs:140`, `crates/usque-android/src/connection_timeline.rs:1`. |
| Low | Doctor reachability result is interpreted as proof of VPN/packet-observer protection | User/operator misreads result; false assurance rather than demonstrated exploitation | Explicit transport-only/protection distinctions, N/A states, no local “zero leaks” claim, exact-candidate `not_run` matrix. `docs/network-doctor.md:1`. |

## 4. Severity calibration

Critical would require a credible path from an untrusted DNS/proxy input to
privileged arbitrary code or broad key compromise, not merely an available
JNI symbol or an authorized administrator. No such finding is asserted here.
High includes a reproducible silent encrypted-to-plaintext downgrade or
unauthorized Kill Switch exception affecting victim traffic. Medium includes
cross-query response confusion or sustained remote resource exhaustion with
concrete access prerequisites. Low covers limited security-relevant disclosure
or misleading protection metadata without a stronger demonstrated impact.

Expected resolver visibility, explicit System mode, ordinary Windows proxy
networking, a rejected certificate, unavailable infrastructure, and a bounded
timeout are counterexamples: they are not by themselves vulnerabilities.
An already-authorized profile change is not an attacker privilege escalation.
Impact and confidence are separate: source mapping has high confidence about
call paths and limits, while real-device cleanup, multi-homed behavior and
external packet counts have no execution evidence in this run. Missing evidence
does not lower the impact of a real issue, and must never be turned into pass.
