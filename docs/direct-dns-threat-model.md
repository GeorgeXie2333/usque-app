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
System, including migrated/missing configuration. Code references name stable
symbols rather than line numbers. See `SharedNetworkSettings` in
`crates/usque-core/src/config/network.rs`; `DirectDnsSettings`, its `Default`
and `DirectDnsSettings::validate` in `crates/usque-core/src/config/mod.rs`;
`migrate_app_config` in `crates/usque-core/src/storage.rs`; and
`message DirectDnsSettings` plus `Profile.direct_dns = 17` in
`proto/usque/v1/control.proto`.

| Component | Responsibility / source |
| --- | --- |
| Core and editor | Canonical name/path/bootstrap/port; explicit user selection; `crates/usque-core/src/config/mod.rs` `DirectDnsSettings::validate`, `DirectDnsSettings::canonicalize` |
| Transport resolver | Strict TLS, protocol validation, bounded generation-scoped pool; `crates/usque-transport/src/encrypted_dns.rs` `DirectDnsResolver::new`, `EncryptedResolver` |
| Split DNS and Geo callers | Classify before lookup, validate responses, maintain generation-scoped route hints, preserve encrypted answers across data-path fallback; `crates/usque-transport/src/split_dns.rs` `SplitDnsResolver::handle`, `DnsRouteCache`; `crates/usque-transport/src/geo_direct.rs` `connect_with_geo_fallback` |
| Platform protection | Windows authenticated Agent and Android exact Network binding; `crates/usque-agent/src/windows/server.rs` `AgentService::acquire_direct_egress`, `DirectEgressRegistry`; `crates/usque-android/src/lib.rs` `AndroidSocketProtector::bind_socket_for_generation` |
| Diagnostics/export | Read-only Standard, bounded Deep and allowlisted local exports; `crates/usque-engine/src/diagnostics/runner.rs` `run`; `crates/usque-engine/src/lib.rs` `ControlService::diagnostic_context`; `crates/usque-engine/src/maintenance.rs` `write_diagnostic_bundle`, `configuration_summary` |

The following table retains each distinct deployment/workflow resource. No
configured resolver, account identifier, real endpoint or secret is reproduced.
“Encrypted bounds” means 1–8 numeric bootstrap IPs, four live socket permits
(including connecting/retiring/Happy-Eyeballs losers), 64 admitted queries,
two bootstrap visits, one retry, four seconds total, strict public-root/name/
validity TLS, no early data, DoH h2 POST/200, and DoT length framing. DoH has
16 streams per connection; DoT has one query per connection. Sources in
`crates/usque-transport/src/encrypted_dns.rs`: the `MAX_CONNECTIONS`/`MAX_IN_FLIGHT` constants,
`EncryptedResolver::connect_one`, `bootstrap_candidates`, `query_with_retry` and
`encrypted_tls_config`.

| Deployment / consumer | Configuration chain and effective resource | Readers / writers / recipients | Enforcing control and evidence / unknowns |
| --- | --- | --- | --- |
| Windows VPN / System Split DNS | Shared Profile → physical snapshot → internal `198.18.0.1` / `fd00::1` → physical DNS endpoints | User config; Agent snapshot; physical provider receives direct QNAME; other names use the final exit's DNS: WARP, or the active chain exit's (custom OpenVPN/WireGuard, WARP via WireGuard, VPN Gate) | Target-aware leases, generation checks, WFP permits when Kill Switch is on; `crates/usque-engine/src/windows_agent.rs` `WindowsVpnSocketProtector::protect_target_generation`; `crates/usque-transport/src/split_dns.rs` `SplitDnsResolver::query_direct`, `SplitDnsResolver::query_tunnel`. External observer `not_run`. |
| Android VPN / System Split DNS | Profile → internal listeners → selected LinkProperties DNS, preserving IPv6 scope | User config; selected non-VPN Network DNS provider | VpnService protect + Network bind, stale-response SERVFAIL; `apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/PhysicalNetworkMonitor.kt` `selectUnderlyingNetwork`; `crates/usque-transport/src/split_dns.rs` `SplitDnsResolver::handle`, `reply_for_generation`. Device/observer `not_run`. |
| Windows VPN / DoH or DoT | Profile → PacketStack → ConfiguredDnsProtector → explicit numeric bootstrap; encrypted bounds | User-selected encrypted provider sees direct QNAME; physical DNS metadata is not consumed by resolver | Agent exact-generation target TCP lease/interface/WFP; `crates/usque-agent/src/windows/server.rs` `AgentService::acquire_direct_egress`; `crates/usque-transport/src/encrypted_dns.rs` `EncryptedResolver::connect_one`. Startup still reads physical metadata for network state, not fallback. Observer `not_run`. |
| Android VPN / DoH or DoT | Android JSON → core validation → exact-Network resolver; encrypted bounds | User-selected encrypted provider; no system bootstrap lookup | Protect before exact Network bind; `crates/usque-android/src/lib.rs` `AndroidSocketProtector::bind_socket_for_generation`; `crates/usque-transport/src/encrypted_dns.rs` `EncryptedResolver::connect_one`. PR-12 removes unnecessary physical-DNS-list startup dependency; a usable non-VPN network is still required. Device `not_run`. |
| Windows proxy / System direct hostnames | Profile → NoopSocketProtector → OS resolver → direct target; existing tunnel fallback | OS-selected DNS provider and direct target | Geo defaults Tunnel; proxy is not VPN egress authorization; `crates/usque-engine/src/lib.rs` `ControlService::connect_with_cancellation_locked`, `load_geo_direct_policy`; `crates/usque-transport/src/socket.rs` `NoopSocketProtector`. System-proxy lease only configures loopback proxy use. |
| Android proxy / System direct hostnames | Proxy route policy → selected Network.getAllByName → bound target socket; no TUN DNS listener | Underlying Network DNS and direct target | Network binding without VpnService.protect; `crates/usque-android/src/lib.rs` `AndroidSocketRoutePolicy`, `AndroidSocketProtector::resolve`; `apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/UsqueVpnService.kt` `resolveUnderlyingHost`, `bindSocketToUnderlyingGeneration`. The hostname lookup uses the current Network; only the socket bind is generation-checked. Device `not_run`. |
| Windows proxy / DoH or DoT | Noop protector → configured resolver → HTTP/SOCKS Geo caller; encrypted bounds, logical generation 0 | Configured encrypted provider and direct target | Strict TLS/no downgrade, but no Agent/WFP lease; `crates/usque-engine/src/lib.rs` `ControlService::connect_with_cancellation_locked`; `crates/usque-transport/src/encrypted_dns.rs` `configure_direct_dns`, `encrypted_tls_config`. No VPN Kill Switch promise. |
| Android proxy / DoH or DoT | Proxy policy → exact-Network resolver → HTTP/SOCKS; encrypted bounds | Configured encrypted provider and direct target | Generation binding without VpnService.protect; `crates/usque-android/src/lib.rs` `AndroidSocketProtector::bind_socket_for_generation`; `crates/usque-transport/src/geo_direct.rs` `connect_with_geo_fallback`. Device `not_run`. |
| Both platforms / independent proxy DNS modes | ProxyDnsMode for non-direct traffic: Remote queries through the tunnel (the final exit's DNS when a chain is active); LocalConfigured sends plaintext UDP/53 to configured servers; System uses the OS resolver; EdgeResolved (L4 proxy only) leaves tunnel-routed hostnames unresolved in the L4 CONNECT authority for the edge to resolve, and an active chain turns it into Remote | Mode-selected resolver, or the L4 edge for EdgeResolved | Separate from direct_dns. Encrypted direct DNS failure is terminal; data fallback after a successful answer reuses those IPs; `crates/usque-core/src/config/mod.rs` `ProxyDnsMode`; `crates/usque-transport/src/dns.rs` `Resolver::resolve_candidates`; `crates/usque-transport/src/data_plane.rs` `final_profile`; `crates/usque-transport/src/geo_direct.rs` `connect_with_geo_fallback`. DoH direct selection is not a claim about unrelated port-53 traffic. |
| Windows active runtime / Deep DNS probe | Active runtime protector → separate short-lived pool → fixed reserved `example.invalid` | Explicit Deep caller; configured encrypted provider | 3.8 s I/O + cleanup within check/session limits; no business-pool mutation; `crates/usque-engine/src/lib.rs` `ControlService::diagnostic_probe_context`; `crates/usque-transport/src/diagnostic_probe.rs` `PROBE_IO_TIMEOUT`, `run_dns_probe`. Not an observer. |
| Windows disconnected / Deep DNS probe | Lifecycle exclusion → Noop protector → encrypted resolver, generation 0 | Explicit Deep caller; configured provider over normal host networking | TLS and bounded ownership only; no Agent physical snapshot/WFP proof; `crates/usque-engine/src/lib.rs` `ControlService::diagnostic_probe_context` (disconnected branch). Reachability is transport-only. |
| Android active/disconnected / Deep DNS probe | ServiceDiagnosticProbes → JNI → ProbeProtector → reserved query | One explicit pending probe; configured provider on current Network | Exact generation before/after bind, native cancellation slot and serialized worker; `crates/usque-android/src/diagnostic_probe.rs` `ProbeProtector`, `run_probe`; `apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/ServiceDiagnosticProbes.kt` `ServiceDiagnosticProbes`. Device `not_run`. |
| All deployments / quality telemetry | Resolver enum/counter/RTT → 1 Hz snapshot → local UI memory | Runtime writes; local Engine/UI reads | No name/address/query payload fields; `crates/usque-transport/src/network_quality.rs` `DirectDnsQuality`; `apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/NetworkQualityFields.kt` `NetworkQualityFields`. No automatic upload/history persistence. |
| Windows/Android / diagnostic ZIP | Snapshot/session/timeline/logs → allowlist → user-selected local file | Explicit exporting user | Custom DNS, endpoints, QNAME, secrets, paths, SSIDs and package lists excluded; `crates/usque-engine/src/maintenance.rs` `write_diagnostic_bundle`, `configuration_summary`; `apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/AndroidMaintenance.kt` `writeDiagnostics`. Native timeline is separately bounded. |
| Explicit configuration serialization | Validated Profile → protobuf/Android JSON | Configuration caller receives chosen name/path/bootstrap/port | Intentionally preserves settings, unlike diagnostic export; `crates/usque-android/src/lib.rs` `android_profile_value`; `crates/usque-engine/src/lib.rs` `profile_to_proto`, `profile_from_proto`. Do not conflate these workflows. |
| Windows VPN / Geo direct TUN TCP/UDP | Numeric packet → DNS route hint/GeoIP → bounded generation-scoped gateway mapping | TUN client and explicitly direct target | No hostname re-resolution for numeric flow; exact target/protocol/interface Agent lease; `crates/usque-transport/src/direct_gateway.rs` `DirectGatewayRouter::route_outgoing`; `crates/usque-agent/src/windows/server.rs` `AgentService::acquire_direct_egress`. Observer `not_run`. |
| Android VPN / Geo direct TUN TCP/UDP | Same numeric gateway classification → protected exact Network socket | TUN client and explicitly direct target | Requires TUN-direct protector capability, bounded mapping and generation; `crates/usque-transport/src/direct_gateway.rs` `DirectGatewayRouter::route_outgoing`; `crates/usque-android/src/lib.rs` `AndroidSocketProtector::tun_direct_available`. Device/observer `not_run`. |

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
(`crates/usque-agent/src/windows/auth.rs` `authenticate_named_pipe`;
`crates/usque-agent/src/windows/server.rs` `AgentService::acquire_direct_egress`). Android separates VPN protect
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
  Sources: `crates/usque-transport/src/split_dns.rs` `direct_udp`, `direct_tcp` and
  `resolve_physical_host`.
- Agent chooses a physical interface by the verified endpoint-path family,
  not a fresh route lookup for each custom resolver. Multi-homed reachability
  needs protected testing; failure is not permission to broaden routing.
  Source: `crates/usque-agent/src/windows/server.rs`
  `AgentService::physical_network_info`, `AgentService::acquire_direct_egress`.
- Actual zero port-53/direct-rule/candidate-path packet observations and
  platform cleanup remain unknown until exact-candidate protected evidence.

## 3. Prioritized attack stories (hypotheses, not findings)

| Priority | Scenario and attacker gain | Prerequisite / impact | Existing controls, mitigation and evidence |
| --- | --- | --- | --- |
| High | On-path actor forces encrypted TLS/DNS failure hoping to obtain plaintext QNAME | Victim chose DoH/DoT and issues a direct query; confidentiality loss if a downgrade exists | Encrypted enum dispatch, numeric bootstrap, terminal errors/SERVFAIL and IP-reusing data fallback. Loopback/fault/proxy tests cover negative paths; independently observe port 53 before claiming packet proof. `crates/usque-transport/src/encrypted_dns.rs` `DirectDnsResolver::query`; `crates/usque-transport/src/split_dns.rs` `SplitDnsResolver::query_direct`; `crates/usque-transport/src/geo_direct.rs` `connect_with_geo_fallback`. |
| High | Network churn races setup/response delivery and sends data on the wrong network | VPN direct traffic, an actual generation race and failed ownership check | Exact-generation binding, I/O-owned leases, 100 ms pool observer, synchronous boundaries and queued-reply check; actor/generation tests. Verify real OS transitions on dedicated runners. `crates/usque-transport/src/encrypted_dns.rs` `EncryptedResolver::connect_one`, `DirectDnsResolver::with_tls_config`; `crates/usque-transport/src/split_dns.rs` `reply_for_generation`. |
| High | Unprivileged local process asks Agent for an unauthorized direct exception | Must cross pipe authentication and operation ownership; possible Kill Switch bypass | PID/SID/path/signer, exact target/protocol/interface, generation and registry caps; no generic route-bypass API. Agent unit tests and isolated WFP evidence, not public exploit claims. `crates/usque-agent/src/windows/auth.rs` `authenticate_named_pipe`; `crates/usque-agent/src/windows/server.rs` `AgentService::acquire_direct_egress`, `validate_direct_context`. |
| Medium | Malicious resolver returns cross-query, oversized or truncated data to poison a route hint or exhaust memory | Legitimate configured resolver connection; wrong destination or service degradation | ID/question/record/TTL/CNAME validation, capped response/body/query/task/pool sizes, one retry and deadlines; property tests and malformed TLS-server fixtures. Resolver returning a valid unwanted answer is a trust assumption, not TLS bypass. `crates/usque-transport/src/split_dns.rs` `validate_response`; `crates/usque-transport/src/encrypted_dns.rs` `EncryptedResolver::query_doh`, `validate_doh_headers`. |
| Medium | Proxy client floods requests or drops a partial exchange to retain sockets/tasks | Authorized local proxy access or explicitly exposed listener; availability impact | Four actual I/O permits, 64 queries, Split DNS 512-task cap, bounded waits/cancellation; cancellation/pool-saturation tests. Preserve these limits and run thermal/performance lab separately. `crates/usque-transport/src/encrypted_dns.rs` `EncryptedResolver::query`, `EncryptedResolver::connect_one`; `crates/usque-transport/src/split_dns.rs` `SplitDnsResolver`. |
| Medium | Private query/endpoint leaks through a failure, timeline or diagnostic bundle | User exports a bundle or an ordinary log captures raw data | Fixed enums/numbers, explicit export allowlist and bounded native bridge; adversarial export fixtures. Configuration export intentionally contains chosen DNS settings and is a different workflow. Desktop Debug-level Engine logs can contain sanitized TUN direct-flow error text; see [encrypted direct DNS](encrypted-direct-dns.md#privacy-and-validation-limits). `crates/usque-engine/src/maintenance.rs` `write_diagnostic_bundle`; `crates/usque-engine/src/logging.rs` `sanitize_log_bytes`; `crates/usque-android/src/connection_timeline.rs` `json_snapshot`. |
| Low | Doctor reachability result is interpreted as proof of VPN/packet-observer protection | User/operator misreads result; false assurance rather than demonstrated exploitation | Explicit transport-only/protection distinctions, N/A states, no local “zero leaks” claim, exact-candidate `not_run` matrix. [Network Doctor](network-doctor.md). |

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
