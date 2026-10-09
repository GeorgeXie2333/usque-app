# Routing rules and Ads / 分流规则与 Ads

## Configure rules

Open **Settings → Bypass settings**. **Custom routing rules** supports individual
entries and **Paste rules**, with one domain, IP address or CIDR per line. Select
DIRECT, REJECT or PROXY for the entry or pasted batch, then **Apply changes**.
Settings are shared across accounts. Changes use the existing reconnect and
pending-settings workflow; a saved draft does not establish runtime behavior.

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
Direct failures retain the existing tunnel fallback and socket-protection rules.
VPN platform exclusions and Windows' fixed loopback/simple-hostname proxy
exceptions retain their existing boundaries.

| Action | Application traffic |
| --- | --- |
| DIRECT | Uses the existing protected TCP/UDP direct path and its tunnel fallback on connection failure. |
| REJECT | Refuses the matching target. It never falls back to another route. |
| PROXY | Uses the current tunnel and selected chain exit, overriding country direct rules. |

Domains include themselves and label-boundary subdomains. More specific domains
win: `REJECT ads.example.com` overrides `DIRECT example.com`. IPs become /32 or
/128 host networks; the longest matching prefix wins. `DIRECT 192.0.2.7` can
therefore override `REJECT 192.0.2.0/24`. List order does not affect matching.
Case, trailing dots, IDNA and network addresses are normalized by the Engine.
URLs, paths, ports, wildcards and user-supplied regular expressions are invalid.
There are at most 256 domain rules and 256 address rules.

Identical targets with different actions prevent saving and identify both rows.
Parent/child and nested-network exceptions are valid; the editor explains the
override. Identical rules are merged. Invalid input and failed saves retain the
draft. An older Engine displays the new controls read-only.

A hostname first matches custom domains, Ads, country domains, then the default
PROXY route. A rejected hostname is not resolved. For an allowed hostname, every
resolved connection address is checked against the custom address rules. An IP
whose longest-prefix result is REJECT cannot be used, even by a DIRECT or PROXY
domain exception. Other candidates remain eligible. Explicit domain actions
otherwise retain priority; explicit address rules can override preset/default
routing when there is no explicit domain match. To exempt a name from Ads, add
a domain DIRECT or PROXY rule.

## Ads

**Ads · advertising and tracking** is off by default. It uses
`category-ads-all` from the existing checksummed V2Fly GeoSite catalog. Use the
page's update action to download/update that catalog. Arbitrary subscription
URLs are not supported. Exact, suffix, keyword and bounded regular-expression
entries are loaded as one complete category; an unsupported or invalid entry
does not silently become a partial blocklist.
The category is maintained in the
[upstream definition](https://github.com/v2fly/domain-list-community/blob/master/data/category-ads-all).

Custom domains override Ads, which takes priority over country direct presets.
Failed updates retain the last complete Ads catalog. If no valid catalog exists,
connections are still allowed: the page reports Ads unavailable, while custom
rules continue to apply. Invalid custom rules prevent applying/starting the
configuration. Country data keeps its existing validation requirements.

An active session retains its loaded catalog. Downloads affect the next
connection. The page distinguishes available data from the current connection
and indicates when its revision differs. Reconnect to use the downloaded rules.

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
Conflicting domain observations sharing an IP conservatively use PROXY; explicit
IP rejection still applies. Rejecting a name does not blacklist a shared CDN IP.
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

Explicit configuration export includes these settings. Logs and diagnostic
exports do not gain target lists, DNS names or packet contents. Rule validation
returns fixed codes and rule IDs so the editor can identify its own rows.

Workstation checks use fake engines, in-memory packet paths and loopback peers.
They do not establish native VPN cleanup or external leak behavior. Follow
[Contributing](../CONTRIBUTING.md) for the exact platform gates and isolated
validation; unavailable isolated checks are `not_run`.

## 使用说明

打开 **设置 → 分流设置 → 自定义分流规则**，添加单条规则，或按指定动作批量粘贴。
每行填写域名、IP 或 CIDR，然后点击 **应用修改**。DIRECT 为直连，REJECT 为拒绝，
PROXY 使用当前隧道及链式出口。设置跨账号共享；查看应用结果，必要时重新连接。

**高级网络设置 → 路由与保护 → 允许访问局域网** 对所有输出生效：VPN/TUN、HTTP 转发及 CONNECT
（包括 Windows 系统代理）、SOCKS5 TCP/UDP；UDP 仍受所选数据平面能力限制。
保存并应用后，按提示重新连接。开启时，上述 IPv4 私有地址、链路本地地址以及 IPv6
ULA/链路本地网段自动走受保护的直连路径；关闭后取消自动直连，显式 DIRECT 规则仍可生效。
这个开关控制目标局域网访问，不控制其他设备能否使用本机代理。

进入引擎的流量仍优先遵循自定义域名/IP 规则和 Ads 拒绝。域名沿用当前代理 DNS 设置，
解析得到的内网地址可直连，不额外查询物理网络 DNS；旧配置若设为由代理服务器解析，开启此选项时
改为通过当前出口 DNS 获取并检查 IP。直连失败仍按现有规则回退到隧道，系统 VPN 排除规则和 Windows
固定的回环地址/简单主机名例外保持原有边界。

域名包含自身及子域名，更具体的域名优先；IP 使用最长前缀匹配。列表顺序不影响结果。
父子域名和包含网段可作为例外，只显示覆盖提示；同一规范化目标配置不同动作时阻止保存，
并定位冲突条目。相同规则会合并，错误或保存失败会保留草稿。

自定义域名优先于 Ads，Ads 优先于国家直连；实际连接 IP 命中 REJECT 时仍会被拒绝。
Ads 误拦截可通过自定义域名 DIRECT／PROXY 放行。Ads 默认关闭，复用现有 GeoSite
广告与跟踪分类；更新失败保留旧库，完全无有效库时警告但允许连接，自定义规则继续生效。
下载的数据在重连后使用，页面区分可用数据与当前连接状态。

旧 CIDR 和域名会迁移为 DIRECT；CIDR 不再通过系统路由提前绕过引擎。
直连支持 TCP/UDP，其他协议沿用隧道能力。应用自行使用加密 DNS、系统局域网绕过及
Android 排除应用仍有上述可见性边界，不能将此功能视为全系统防火墙。
