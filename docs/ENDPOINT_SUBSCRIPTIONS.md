# Endpoint subscriptions / 端点订阅

Import HTTPS plain-text MASQUE endpoint lists in **Profiles → Subscriptions**,
match them strictly to a WARP account tier (**WARP Free**, **WARP+**, or
**Zero Trust**), and switch endpoints in a single action without overwriting
other account tiers or manual drafts.

在 **配置 → 端点订阅** 中导入 HTTPS 纯文本 MASQUE 端点列表，按 WARP 账号类型
（**WARP Free**、**WARP+** 或 **Zero Trust**）严格一对一匹配并单独保存，可在
同一订阅内单步快速切换端点，而不覆盖其他类型账号的设置或手动草稿。

## Supported formats and limits / 支持的格式与限制

Subscriptions use plain-text UTF-8 bodies served strictly over `https://`
(URL up to 2,048 bytes, decompressed body up to 512 KiB, maximum 32
subscriptions and 512 unique endpoints per subscription):

- `IPv4:Port@Note` (for example `162.159.193.10:2408@Edge 01`)
- `[IPv6]:Port@Note` (for example `[2606:4700:103::10]:443@v6 Edge`)
- `Domain:Port@Note` (public FQDN, lowercased without trailing dot)
- `#Note` end-of-line notes, bare addresses without port (inheriting the
  profile port, default `443`), bare addresses without notes, empty lines, and
  `#` / `//` whole-line comments are accepted.

The parser rejects unspecified, loopback, RFC 1918 private, CGNAT (`100.64/10`),
link-local, multicast, IPv6 ULA (`fc00::/7`), IPv4-mapped IPv6 addresses,
documentation and special-purpose IPv6 ranges (`2001:db8::/32`, `3fff::/20`,
Teredo `2001::/32`, benchmarking `2001:2::/48`, ORCHIDv2 `2001:20::/28`),
`localhost`, `.local`/`.internal` hostnames, and non-`https://` URLs. The URL
bound applies to the normalized, percent-encoded form. On
refresh, existing pinned stars are preserved by canonical endpoint ID, and if an
upstream update removes the currently active endpoint, Usque retains that active
snapshot with a **Removed upstream** badge until you switch away.

Subscription URLs, query parameters, notes, and endpoint lists are never written
to logs, error strings, or exported diagnostics.

订阅正文支持标准 UTF-8 纯文本列表，URL 仅允许 `https://`（上限 2,048 字节），
解压后正文上限 512 KiB，本地最多保存 32 组订阅、每组最多 512 个去重端点：

- `IPv4:端口@备注`、`[IPv6]:端口@备注`、`域名:端口@备注`（支持 `#备注`、省略端口默认继承当前端口 443、省略备注，以及 `#` / `//` 注释行）；
- 解析器过滤环回、私网、CGNAT、链路本地、组播、ULA、文档与特殊用途 IPv6 段等非公网地址与内网域名；
- 未连接时才直连物理网络下载订阅；连接中、重连中、错误等过渡状态一律拒绝（`SUBSCRIPTION_PROTECTED_PATH_UNAVAILABLE`），不会绕过 WARP 或 Kill Switch；
- 刷新时自动保留既有星标收藏；若上游移除了当前正在使用的端点，Usque 会以“已不在最新订阅中”警示行保留当前运行快照直到用户主动切换；
- 订阅 URL、备注和端点列表永不写入日志、报错文本或诊断导出。

## Account isolation, scenes, and Home quick-switch / 账号隔离、场景与首页联动

1. **1:1 tier matching (Schema 30)**: Consumer Free, Consumer WARP+, and each
   Zero Trust account store their active subscription reference separately
   (`network.consumer_tier_subscriptions.{free,plus}` and
   `account.zero_trust_subscription_id`). Switching accounts preserves each
   tier's binding, and Consumer accounts without a verified entitlement cannot
   activate subscriptions (`fail-closed`).
2. **Coexistence with Scenes**: **Profiles** switches between **Scenes** and
   **Subscriptions** with a segmented control. Selecting another endpoint inside the
   already active subscription triggers no scene drift prompt and leaves the
   selected scene unmodified; switching to a different subscription or deactivating
   the active subscription asks before marking an unmodified scene as modified.
3. **Scene dynamic binding**: In the Scene editor and Advanced network settings,
   selecting **Subscription** stores `endpoint.subscription_id` without pinning a
   single endpoint ID, so the scene dynamically tracks whichever endpoint is
   selected inside that subscription. Deleting a referenced subscription or
   mismatched account tier prevents scene activation with
   `SCENARIO_DEPENDENCY_UNAVAILABLE`.
4. **Home quick-switch**: When the active account uses an endpoint subscription,
   Home shows a compact endpoint row below the scene row to switch endpoints
   within the active subscription in one menu (otherwise hidden).
5. **Zero Trust warning**: Activating a Zero Trust subscription requires the
   fullscreen Zero Trust endpoint risk confirmation once per activation and keeps
   the non-dismissible Home Zero Trust endpoint banner visible while active.

## Refresh and real MASQUE probing / 拉取更新与真实握手探测

- **Fetch path & auto-refresh**: Disconnected refreshes use a direct
  `no_proxy` HTTPS client validating public unicast targets on every redirect
  hop, only while no data plane or retained Agent protection exists;
  connected refreshes tunnel HTTPS through the active WARP connection so
  Kill Switch is preserved. Every other phase (preparing, connecting,
  reconnecting, disconnecting, error, or a connected session without an
  internal WARP network such as macOS system TUN) fails closed with
  `SUBSCRIPTION_PROTECTED_PATH_UNAVAILABLE` instead of using the physical
  network. Manual, 24-hour (`daily`), and 7-day (`weekly`) schedules check
  due subscriptions while the Engine runs, starting with the Engine process
  (1-hour failure backoff). A fail-closed or cancelled attempt records no
  failure and is retried on a later tick; refreshes never trigger background
  reconnects. Clear All Data cancels every in-flight download, and a download
  that finishes afterwards cannot recreate the cleared catalogue.
- **Numeric IP vs domain dialing**: Selecting a numeric IPv4 or IPv6 entry
  dials only that address and port without silently falling back to the legacy
  pair. Selecting a domain entry resolves A/AAAA candidates on the physical
  network per `ip_policy` (4 s budget, 16 per family) without persisting
  resolved IPs.
- **MASQUE handshakes & volatile leases**: **Test latency** runs real MASQUE
  handshakes (`probe_h3_handshake` / `probe_h2_handshake`, verifying TLS
  fingerprints and Cloudflare P-256 SPKI pins without opening `CONNECT-IP`
  tunnels) up to 4 concurrent probes and streams progress via `EventEnvelope`
  tag `26`. While desktop system TUN is connected, the Engine requests
  short-lived, in-memory, process-scoped 5-tuple permits
  (`AcquireEndpointProbeLeaseRequest` / `ReleaseEndpointProbeLeaseRequest`)
  from the privileged Agent and closes sockets before releasing leases. The
  Agent admits one lease at a time: a replacement probe waits for the previous
  lease to be released, domain candidates under one lease stay within its
  15-second TTL, and every Agent reaps an expired lease on its periodic tick.
  A refused probe lease or socket never retires the owning session. A target
  rejected by the Agent (for example a tunnel DNS server) fails only its own
  entry. Android uses `VpnService.protect(fd)` while a TUN session runs and the
  proxy route policy for proxy-only sessions. Switching accounts or physical
  network changes clears stale probe results immediately.

## Protocol compatibility and isolated validation / 协议兼容与验证边界

- **Schema 30 & append-only Protobuf**: Older versions cannot read Schema 30
  configurations; `EndpointSettings.subscription_id` (tag `7`),
  `Capabilities` tags `60..63` (`endpoint_subscriptions`,
  `endpoint_subscription_domain_endpoints`, `endpoint_subscription_probe`,
  `endpoint_subscription_connected_tun_probe`), `ControlRequest` tag `54`,
  `ControlResponse` tag `30`, `EventEnvelope` tag `26`, `AgentCapabilities`
  tags `23..24`, `TunnelPlan.hostname_endpoint_policy` (tag `17`), Agent
  requests `37..39` / response `20`, macOS Agent ops `24..26`, and
  `MacosFailure` value `15` (`PROBE_REJECTED`, a non-fatal probe refusal) are
  strictly append-only and capability-gated.
- **Isolated checks**: Deterministic parser, store, Schema 30, mock H2/H3 probe,
  Agent lease policy, Android unit/Kotlin, and Flutter UI/golden checks run on
  development hosts. Real desktop WFP/nftables/PF lease leak checks, Android
  device TUN `protect(fd)` lifecycle, and live over-tunnel captures require the
  isolated runners documented in [Contributing](../CONTRIBUTING.md) and are
  recorded as `not_run` when unavailable.
