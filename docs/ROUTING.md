# Routing rules and Ads / 分流规则与 Ads

## Configure rules

Open **Settings → Bypass settings**. **Custom routing rules** supports individual
entries and **Paste rules**, with one domain, IP address or CIDR per line. Select
the action for the entry or pasted batch — **Direct** (DIRECT), **Block**
(REJECT) or **Through Usque** (PROXY) — then **Apply changes**. The list shows
each target with its action and no numbering, because order does not affect
matching.
Settings are shared across accounts. During a stable connection, custom-rule
edits apply to new flows without reconnecting. Check the apply result: a saved
change can remain pending when disconnected, busy or recovering, or when the
current platform cannot safely prepare DIRECT resources. Country, LAN, DNS and
other transport changes retain their reconnect requirements. Combining a rule
edit with a TUN output change also requires reconnecting.

An established TCP connection or CONNECT stream keeps its selected path until
it closes. HTTP forwarding chooses the current rule revision for each new
request and does not reuse an idle upstream connection from an older revision.
UDP keeps each association target's selected path until its idle expiry or a
physical-network change. Adding REJECT does not retrospectively terminate those
existing flows. Reconnect explicitly to retire them immediately.

DNS route hints are scoped to both the physical network and the rule revision.
An outstanding lookup from a retired revision returns SERVFAIL and cannot
publish hints for the replacement. Applications may still retain their own DNS
answers. Preparation and publication validate resources and network generation;
failure preserves the previous live rules and reports that application failed.

**Advanced network settings → Routing & protection → Allow local network** applies to every output:
VPN/TUN, HTTP forwarding and CONNECT (including Windows system proxy), and
SOCKS5 TCP/UDP where the selected data plane supports it. Save/apply the setting
and reconnect when requested. It selects the protected direct path for
`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `169.254.0.0/16`, `fc00::/7`
and `fe80::/10`, including IPv4-mapped IPv6 destinations. Turning it off removes
this automatic selection; explicit DIRECT rules can still allow those targets.
This setting controls destination access, not exposure of proxy listeners to
other devices.

For traffic entering the engine, explicit domain/address rules and Ads rejection
retain priority over automatic LAN routing. Hostnames use the configured proxy
DNS path; returned private/link-local addresses can go direct, without adding a
physical DNS query. Older configurations that let the proxy server resolve names
instead resolve through the current exit DNS while LAN routing is enabled, so
the returned address can be checked first.
Direct failures retain the existing tunnel fallback and socket-protection rules,
except when the physical network is known to lack the destination address family.
VPN platform exclusions and Windows' fixed loopback/simple-hostname proxy
exceptions retain their existing boundaries.

| Action | Application traffic |
| --- | --- |
| DIRECT | Uses protected TCP/UDP direct egress. A known unavailable physical address family fails locally; other connection failures retain the existing tunnel fallback. |
| REJECT | Refuses the matching target. It never falls back to another route. |
| PROXY | Uses the current tunnel and selected chain exit, overriding country direct rules. |

Domains include themselves and label-boundary subdomains. More specific domains
win: `REJECT ads.example.com` overrides `DIRECT example.com`. IPs become /32 or
/128 host networks; the longest matching prefix wins. `DIRECT 192.0.2.7` can
therefore override `REJECT 192.0.2.0/24`. List order does not affect matching.
Case, trailing dots, IDNA and network addresses are normalized by the Engine.
URLs, paths, ports, wildcards and user-supplied regular expressions are invalid.
There are at most 256 domain rules and 256 address rules.

When the platform reports an IPv4-only physical network, Split DNS turns a
DIRECT domain's answer containing only unusable public IPv6 addresses into an
empty successful AAAA answer, allowing applications to use IPv4. IPv6-only
physical networks receive the corresponding A answer behavior. Local addresses,
PROXY address exceptions and upstream DNS errors retain their existing handling.
This applies to System, DoH and DoT direct DNS. An application
that still submits an address in a known unavailable DIRECT family receives a
local failure instead of using WARP; a dual-stack hostname tries its available
direct candidates. PROXY retains the tunnel's IPv4/IPv6 capabilities. Unknown
physical-family availability retains the existing protected connection attempts.
These observations describe Internet paths; loopback and local-network targets
retain protected direct attempts even without a default route for their family.

Identical targets with different actions prevent saving; both rows show the
reason, and the apply bar names them by target. Parent/child and nested-network
exceptions are valid; each overlapping row states that the more specific rule
wins. Identical rules are merged. Invalid input and failed saves retain the
draft. An older Engine displays the new controls read-only.

A hostname first matches custom domains, Ads, country domains, then the default
PROXY route. A rejected hostname is not resolved. For an allowed hostname, every
resolved connection address is checked against the custom address rules. An IP
whose longest-prefix result is REJECT cannot be used, even by a DIRECT or PROXY
domain exception. Other candidates remain eligible. Explicit domain actions
otherwise retain priority; explicit address rules can override preset/default
routing when there is no explicit domain match. To exempt a name from Ads, add
a domain DIRECT or PROXY rule.

## Country presets

In **Bypass settings → Countries**, download a country's data, enable its
switch, then **Apply changes**. A valid GeoIP cache is required; country GeoSite
categories are optional. Without a country domain category, hostname targets
have no GeoSite DIRECT match; IP-address targets still match GeoIP. Unknown
names and addresses keep the existing tunnel route. Missing or corrupt GeoIP,
and corrupt GeoSite when present, prevent applying or starting the configuration.

CN uses the upstream checksummed `cn.dat`. Other countries are extracted from
the checksummed `geoip.dat` aggregate, cached once and rechecked against its
upstream SHA-256 before reuse. Only the requested country is written into its
country cache. Downloads retain the HTTPS/source allowlist, atomic replacement
and last-good-cache behavior, with an 8 MiB country limit and a 32 MiB aggregate
limit. A failed download leaves existing country data intact.

## Ads

**Ad and tracker blocking** is off by default. It uses
`category-ads-all` from the existing checksummed V2Fly GeoSite catalog. Enable
it and choose **Apply changes**: missing rules are downloaded and validated
automatically before saving. A failed first download leaves the preference saved
but reports blocking inactive, with **Retry** beside the status. While enabled,
the running app checks on startup, successful connection and its next scheduled
check, at most once every seven days after success. Failed attempts retry after
one hour; deadlines survive app restarts. Checks do not wake a closed app.
**Rule details** contains the source, update/check times and **Update now**.
Ads maintenance fetches only GeoSite, without refreshing country GeoIP files.
Arbitrary subscription URLs are not supported. Exact, suffix, keyword and bounded regular-expression
entries are loaded as one complete category; an unsupported or invalid entry
does not silently become a partial blocklist.
The category is maintained in the
[upstream definition](https://github.com/v2fly/domain-list-community/blob/master/data/category-ads-all).

Custom domains override Ads, which takes priority over country direct presets.
Failed updates retain the last complete Ads catalog. If no valid catalog exists,
connections are still allowed: the page reports Ads unavailable, while custom
rules continue to apply. Invalid custom rules prevent applying/starting the
configuration. Country data requires valid GeoIP; GeoSite adds domain matching
when available.

Ads switches and verified catalog replacements apply to new requests in the
current session without reconnecting. A session-owned shared matcher swaps only
complete validated categories and retains its previous good revision on failure.
The native process observes atomic cache replacements once per second, including
Android downloads made by the UI process. Its watcher expires with the session.
The page reports actual runtime activation separately from rule availability.
Existing established flows and DNS answers already cached by applications are
not terminated or flushed; this is domain filtering, not retroactive inspection.
Country presets and custom routing edits retain their existing apply lifecycle.

Windows and Android prepare the internal DNS gateway when the tunnel starts,
even with Ads disabled, so enabling Ads does not replace the interface or its
DNS configuration. Queries retain the configured upstream and exit policy;
this does not add physical DNS queries or network-state mutations on a toggle.
Downloads retain the existing HTTPS, source allowlist, checksum and size checks.
No browsing history, domain lists or credentials enter new logs or telemetry.

## Runtime and compatibility

HTTP forwarding and CONNECT return 403 for rejected requests. SOCKS5 CONNECT
returns 0x02 as defined in [RFC 1928](https://www.rfc-editor.org/rfc/rfc1928.html);
rejected SOCKS5 UDP datagrams are dropped. Usque-handled DNS returns
REFUSED (RCODE 5, [RFC 1035](https://www.rfc-editor.org/rfc/rfc1035.html))
without querying an upstream for rejected names; observable CNAME chains
are checked as well. TUN rejects TCP with RST and UDP with rate-limited ICMP or
ICMPv6 replies where valid; other rejected IP packets are dropped. Refusal is
local to the request/flow and does not restart the session.

IP rules also apply to numerical candidates resolved through a proxy exit.
When address checks are required, older configurations that let the proxy
server resolve names instead resolve through the current exit DNS and submit
the checked address. It does not fall
back to physical DNS or submit an unchecked hostname when resolution fails.

DNS route hints remain bounded by TTL, capacity and network generation.
Explicit IP rejection applies first, followed by live explicit domain decisions
(PROXY wins if explicit domain decisions share an IP), then custom address rules.
Conflicting learned observations otherwise conservatively use PROXY; they do not
override an explicit DIRECT domain decision or a custom address rule. Rejecting
a name does not blacklist a shared CDN IP.
Applications using their own encrypted DNS can hide names from Usque; address
rules still apply to traffic entering its engine. LAN/system exclusions,
Android excluded applications and tunnel control traffic retain their existing
boundaries. This is not a system-wide firewall or TLS inspection feature.

Configuration schema 24 migrates legacy domain/CIDR bypass targets to DIRECT
rules and retains countries. Those CIDRs no longer install physical routes or
broad firewall permits: custom routing occurs in the application data plane.
DIRECT supports the existing TCP/UDP direct path; ICMP and other protocols retain
the selected tunnel's capabilities. Older clients cannot overwrite new routing
through legacy fields or unversioned whole-profile writes. Non-routing edits
continue through field-scoped settings updates.

TUN DIRECT targets that cannot receive a direct-egress lease (multicast,
unspecified addresses or destination port zero) use the existing tunnel fallback
without repeatedly requesting an invalid lease. This does not implement LAN
multicast forwarding. Existing imported rules remain loadable; a broad DIRECT
CIDR containing both unicast and multicast addresses still supports its unicast
targets. Agent validation remains authoritative for every requested lease.

The default log reports DIRECT fallback reasons and gateway return-queue drops
using fixed codes and numeric counts, with repeated messages limited per reason
to one every 15 seconds while events continue. It does not log destination
addresses or rule contents. Desktop diagnostic exports include the corresponding
gateway-lifetime counters; see [network-quality metrics](network-quality-metrics.md).

Explicit configuration export includes these settings. Logs and diagnostic
exports do not gain target lists, DNS names or packet contents. Rule validation
returns fixed codes and rule IDs so the editor can identify its own rows.

Workstation tests use fake engines, in-memory packet paths and loopback peers;
native VPN cleanup and external leak behavior need the isolated environments in
[Contributing](../CONTRIBUTING.md).

## 使用说明

打开 **设置 → 分流设置 → 自定义分流规则**，添加单条规则，或按指定动作批量粘贴。
每行填写域名、IP 或 CIDR，然后点击 **应用修改**。动作 **直连**（DIRECT）不经过隧道，
**拦截**（REJECT）拒绝连接，**代理**（PROXY）使用当前隧道及链式出口。列表不编号，
规则顺序不影响匹配；冲突的规则会在所在行显示原因。设置跨账号共享。连接稳定时，
仅修改自定义规则会直接用于新流，无需重连；请查看应用结果，离线、忙碌、恢复中或无法安全
准备直连资源时可能仅保存为待应用。国家、局域网、DNS 和其他传输设置仍遵循重连要求；
规则与 TUN 开关同时修改也会重连。

已有 TCP／CONNECT 连接保持原路径直到关闭；HTTP 转发的每个新请求读取当前规则版本，
不会复用旧版本的空闲上游连接。UDP 关联的同一目标保持原路径，直到空闲过期或物理网络变化。
新增 REJECT 不会追溯中断这些已有流；需要立即清退时可主动重新连接。
DNS 路由提示同时绑定网络代次与规则版本，旧查询在版本切换后返回 SERVFAIL，不能写入新版本提示；
应用自身的 DNS 缓存不受强制清除。准备和发布失败会保留旧的运行规则并报告应用失败。

在 **分流设置 → 国家/地区** 下载国家数据，打开直连开关并点击 **应用修改**。
启用需要有效 GeoIP，GeoSite 国家域名分类为可选；没有域名分类时，仍可按 IP 目标
匹配国家，域名目标及未匹配的地址沿用隧道路由。缺少或损坏的 GeoIP，以及已存在但损坏的 GeoSite
会阻止应用设置或连接。CN 使用带校验的 `cn.dat`，其他国家从带 SHA-256 校验的 `geoip.dat`
总库提取；总库共用缓存，每次复用前重新对照上游校验值。单国上限 8 MiB，总库上限 32 MiB，
保留 HTTPS、来源白名单、原子替换和下载失败保留旧数据的保护。

**高级网络设置 → 路由与保护 → 允许访问局域网** 对所有输出生效：VPN/TUN、HTTP 转发及 CONNECT
（包括 Windows 系统代理）、SOCKS5 TCP/UDP；UDP 仍受所选数据平面能力限制。
保存并应用后，按提示重新连接。开启时，上述 IPv4 私有地址、链路本地地址以及 IPv6
ULA/链路本地网段自动走受保护的直连路径；关闭后取消自动直连，显式 DIRECT 规则仍可生效。
这个开关控制目标局域网访问，不控制其他设备能否使用本机代理。

进入引擎的流量仍优先遵循自定义域名/IP 规则和 Ads 拒绝。域名沿用当前代理 DNS 设置，
解析得到的内网地址可直连，不额外查询物理网络 DNS；旧配置若设为由代理服务器解析，开启此选项时
改为通过当前出口 DNS 获取并检查 IP。除物理网络已明确不支持目标地址族外，直连失败仍按现有规则回退到隧道。系统 VPN 排除规则和 Windows
固定的回环地址/简单主机名例外保持原有边界。

本地物理网络仅支持 IPv4 时，若 DIRECT 域名的 AAAA 回答仅含无法直连的公网 IPv6 地址，
分流 DNS 返回成功但无地址的响应，使应用使用 IPv4；仅支持 IPv6 时对 A 回答作相同处理。
局域网地址、PROXY 地址例外和上游 DNS 错误沿用现有处理。System、DoH 和 DoT 直连 DNS 均适用。
若应用仍提交本地已知不支持的 DIRECT 地址族，连接在本地失败，不再转入 WARP；双栈域名会尝试
可用的直连地址。PROXY 仍可使用隧道提供的 IPv4／IPv6。平台未报告地址族能力时，沿用受保护的连接尝试。
此处地址族信息描述互联网路径；回环和局域网目标即使缺少该地址族的默认路由，也仍尝试受保护的直连。

域名包含自身及子域名，更具体的域名优先；IP 使用最长前缀匹配。列表顺序不影响结果。
父子域名和包含网段可作为例外，只显示覆盖提示；同一规范化目标配置不同动作时阻止保存，
并定位冲突条目。相同规则会合并，错误或保存失败会保留草稿。

自定义域名优先于 Ads，Ads 优先于国家直连；实际连接 IP 命中 REJECT 时仍会被拒绝。
Ads 误拦截可通过自定义域名 DIRECT／PROXY 放行。Ads 默认关闭，复用现有 GeoSite
广告与跟踪分类；更新失败保留旧库，完全无有效库时警告但允许连接，自定义规则继续生效。
开启广告拦截并点击 **应用修改** 后，缺少的规则自动下载、校验；首次下载失败会显示尚未生效并提供
**重试**。启用期间，应用在启动、连接成功及到期时检查，每次成功后间隔 7 天，失败后 1 小时重试；
关闭应用时不会唤醒检查，检查期限跨重启保存。**规则详情** 提供来源、更新／检查时间和 **立即更新**。
广告维护只更新 GeoSite，不连带更新国家 GeoIP。

广告开关和规则更新无需重连，完整校验后用于当前连接的新请求；原生进程每秒观察一次规则文件替换，
失败保留上次有效规则。页面根据运行时状态显示是否生效。已建立的流和应用缓存的 DNS 答案不会被强制
中断或清空。Windows／Android 在连接建立时准备内部 DNS 通道，因此广告开关无需修改网卡或系统 DNS；
上游解析及出口选择保持原策略。自定义分流规则按上述版本机制用于新流；国家设置仍需重连。

旧 CIDR 和域名会迁移为 DIRECT；CIDR 不再通过系统路由提前绕过引擎。
直连支持 TCP/UDP，其他协议沿用隧道能力。应用自行使用加密 DNS、系统局域网绕过及
Android 排除应用仍有上述可见性边界，不能将此功能视为全系统防火墙。
