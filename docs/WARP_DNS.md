# WARP® exit DNS / WARP 出口 DNS

## Configure / 配置

Open **Settings → Advanced → IP & DNS** and choose the WARP DNS type.
**Plain DNS** keeps the existing IPv4 and IPv6 resolver addresses. For
**DNS over HTTPS** (DoH) or **DNS over TLS** (DoT), enter the provider's server
name and numeric bootstrap IP addresses.
The server name must match its TLS certificate. DoH also needs a request path;
the defaults are `/dns-query` and port `443`. DoT defaults to port `853`.
Use the provider's published values; bootstrap addresses are not discovered
through another DNS service. Select **Apply changes** to save and apply the
configuration. Changing a connected session's DNS reconnects that session.

打开**设置 → 高级设置 → IP 与 DNS**，选择 WARP DNS 类型。**普通 DNS**
保留原 IPv4、IPv6 服务器地址；**DNS over HTTPS**、**DNS over TLS** 填写服务商的服务器域名和引导
IP 地址。域名必须与 TLS 证书匹配。DoH 还需填写路径，默认 `/dns-query`、端口
`443`；DoT 默认端口 `853`。按服务商公布的信息填写，程序不会通过其他 DNS
服务查找引导地址。点击**应用更改**保存并生效；已连接时更改 DNS 会重新连接。

Switching types retains the draft fields while this page is open. Only the
selected type is applied. Resetting Advanced settings selects Plain DNS in the
draft and clears retained encrypted fields; it takes effect only after applying.

页面内切换类型会保留草稿，只应用当前选中的类型。高级设置恢复默认后，草稿
改回普通 DNS，并清空保留的加密配置，点击应用后才生效。

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
