# WARP® exit DNS / WARP 出口 DNS

## Configure / 配置

Open **Settings → Advanced network settings → IP & DNS** and choose the WARP DNS type.
**Plain DNS** keeps the existing IPv4 and IPv6 resolver addresses.
**DNS over HTTPS** (DoH) uses one **DoH URL** field, including the HTTPS scheme,
server name, optional port and path. New DoH drafts use
`https://cloudflare-dns.com/dns-query`; new **DNS over TLS** (DoT) drafts use
`one.one.one.one` on port `853`. Both prefill Cloudflare's server IPs:
`1.1.1.1`, `1.0.0.1`, `2606:4700:4700::1111` and `2606:4700:4700::1001`.
These values follow Cloudflare's [DoH](https://developers.cloudflare.com/1.1.1.1/encryption/dns-over-https/make-api-requests/)
and [DoT](https://developers.cloudflare.com/1.1.1.1/encryption/dns-over-tls/) documentation.
All fields remain editable. WARP bootstrap IPs remain optional; clearing them
resolves the server name using the configured Plain DNS servers inside WARP.
Select **Apply changes** to save and apply the configuration. Changing a
connected session's DNS reconnects that session.

打开**设置 → 高级网络设置 → IP 与 DNS**，选择 WARP DNS 类型。**普通 DNS**
保留原 IPv4、IPv6 服务器地址。**DNS over HTTPS** 使用单个 **DoH 地址** 输入框，
填写完整 HTTPS 链接，包括域名、可选端口和路径。新 DoH 草稿默认填入
`https://cloudflare-dns.com/dns-query`；新 **DNS over TLS** 草稿默认填入
`one.one.one.one`，端口 `853`。两者均预填 Cloudflare 的 IP：`1.1.1.1`、
`1.0.0.1`、`2606:4700:4700::1111`、`2606:4700:4700::1001`，所有字段仍可修改。
WARP 引导 IP 仍可选，清空后通过 WARP 内配置的普通 DNS 解析服务器域名。
点击**应用修改**保存并生效；已连接时更改 DNS 会重新连接。

Local HTTP/SOCKS5 proxies use remote resolution through the current exit by
default: ordinary connections use WARP DNS, while chains use their final-exit
DNS. The Proxy page does not offer a DNS mode picker or DNS address fields.
Existing non-default settings remain active internally, without a migration
panel or restore action. Editing proxy listeners does not change DNS mode,
DNS addresses or credentials. Existing local-DNS risk warnings remain when the
saved configuration uses local or system resolution.

本地 HTTP/SOCKS5 代理默认通过当前出口解析：普通连接使用 WARP DNS，链式连接
使用最终出口的 DNS。代理页不再提供 DNS 方式选择器或地址输入框。已有非默认配置
在底层继续生效，前端不显示迁移面板或恢复操作。修改代理监听地址不会改变 DNS
方式、DNS 地址或认证；已有配置使用本机或系统解析时，仍保留本机 DNS 风险提示。

Existing DoH settings automatically display as a complete URL, preserving the
saved name, custom port, path and bootstrap IPs. Opening the editor does not
rewrite settings or replace them with Cloudflare. The stored configuration and
IPC fields stay compatible with previous versions. URLs must use HTTPS, a DNS
name and a valid port; credentials, query strings and fragments are not supported.
Omitting a URL port uses `443`; omitting the path uses `/dns-query`.

Switching types retains separate drafts while this page is open. Only the
selected type is applied. Resetting Advanced settings selects Plain DNS and
clears retained drafts; selecting an encrypted type again prefills Cloudflare.
The reset takes effect only after applying.

旧版 DoH 配置会自动组合为完整链接，保留原域名、自定义端口、路径与引导 IP。
打开页面不会重写配置或替换成 Cloudflare，保存格式与 IPC 字段继续兼容旧版。
地址必须使用 HTTPS、服务器域名和有效端口，不支持用户名密码、查询参数或片段。
省略端口时使用 `443`，省略路径时使用 `/dns-query`。
页面内切换类型会分别保留草稿，只应用当前选中的类型。高级设置恢复默认后，草稿
改回普通 DNS 并清空保留的加密草稿；再次选择加密类型时预填 Cloudflare，点击应用后才生效。

## Behavior and failures / 行为与失败

WARP DoH and DoT run inside the selected WARP session, in CONNECT-IP and L4
modes. They serve ordinary VPN DNS and local HTTP/SOCKS5 remote hostname
resolution. Explicit local/System/EdgeResolved proxy DNS choices retain their
own behavior. [Direct DNS](encrypted-direct-dns.md) still controls names matched
by direct bypass rules. Apps using their own DNS retain their own resolver.

When a chain is enabled, its WARP underlay uses these settings for underlay
hostname queries. The final chain exit retains its own DNS policy; this selector
does not customize WARP via WireGuard or another final chain exit.

An unreachable server, rejected TLS certificate or invalid response fails the
query. VPN clients receive SERVFAIL and proxy hostname requests fail; there is
no automatic retry through Plain DNS, physical DNS or another exit. Check the
configured server name, path, port and bootstrap addresses. An incompatible
Engine cannot use saved encrypted settings and will not downgrade them.

WARP DoH、DoT 在当前 WARP 会话内运行，支持 CONNECT-IP 和 L4，处理普通 VPN
DNS 与本地 HTTP/SOCKS5 的远程域名解析。显式本地、System、EdgeResolved 代理
DNS 仍按自身设置工作；绕过规则命中的域名继续使用[直连 DNS](encrypted-direct-dns.md)。
应用自带的 DNS 不受此选择控制。启用链式出口时，此配置用于 WARP 底层的域名
查询，最终链出口仍使用自己的 DNS 策略。

服务器不可达、证书验证失败或响应无效时，VPN 查询返回 SERVFAIL，代理域名
请求失败，不会自动改用普通 DNS、本机 DNS 或其他出口。请检查服务器域名、
路径、端口和引导地址。不兼容的 Engine 不能使用已保存的加密配置，也不会自动降级。

## Validation limits / 验证范围

Unit and loopback tests verify configuration, encrypted exchanges, DNS
interception decisions and resource cleanup without starting a native VPN.
Windows platform-state restoration, dedicated Android lifecycle behavior and
externally observed DNS leaks require the isolated environments specified in
[Contributing](../CONTRIBUTING.md#development-machines). Missing isolated
validation is recorded as `not_run`, never as a pass. Diagnostic exports omit
custom server names, paths, bootstrap addresses and DNS query contents.

---

WARP is a trademark and/or registered trademark of Cloudflare, Inc. in the United States and other jurisdictions.
