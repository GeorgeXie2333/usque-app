# Chain proxy / 链式代理

Open **Proxy → Chain proxy**. The source selector always uses this order:

1. **OpenVPN (Custom)**
2. **WireGuard (Custom)**
3. **VPN Gate**

The source names remain English in every locale. One exit is enabled at a time:
`Application → WARP → selected chain exit → Internet`. System VPN, SOCKS5 and
HTTP share the final exit while retaining their own protocol capabilities;
HTTP CONNECT does not gain UDP support. Explicit direct rules still apply.

打开 **代理 → 链式代理**，来源名称和顺序固定为 **OpenVPN (Custom)**、
**WireGuard (Custom)**、**VPN Gate**。每次启用一个出口，系统 VPN、SOCKS5 和
HTTP 共用最终出口，各入口保留自身协议能力，显式直连规则继续生效。

## Import, select and apply / 导入、选用与应用

Choose a custom source, then **Import file** or **Paste configuration**. Use
UTF-8 text up to 128 KiB. Windows opens its native picker; Android uses the
system document provider. Android TV devices without a document provider can
use pasted text. Usque reads the document once and does not retain a dependency
on its path or document-provider permissions.

**Check configuration** displays the endpoint, protocol, address family,
addresses, DNS, AllowedIPs and MTU where available. OpenVPN addresses and DNS
may be negotiated by the server. Supply the requested username, password or
encrypted private-key password, choose a name, and **Save configuration**.
Errors identify a field and line without reproducing configuration values.
Saving to the library neither selects the configuration nor starts a connection.

Enable the page switch, select a saved configuration, review **Current
connection**, **Saved selection** and **Pending selection**, then **Apply changes**.
A disconnected connection stays disconnected; an active connection applies the
new exit using the existing connection workflow. Navigating away or switching
sources asks before discarding an unapplied draft.

Use the configuration's menu to rename it or update OpenVPN credentials. Import
again to replace configuration content. Credential changes are used on the next
connection; they do not silently reconnect the current one. To delete a selected
configuration, clear or change the selection and apply it first. A configuration
still used by the current connection cannot be deleted. Imports are device-wide,
independent of the selected WARP account.

选择自定义来源后，可导入文件或粘贴配置。两种入口共用 Rust 校验流程，限制为
128 KiB UTF-8 文本。TV 没有系统文件选择器时请粘贴文本。预览后补充认证信息、
命名并保存；保存不会选用配置或自动连接。打开总开关、选择配置，再点击
**应用更改**。页面分别展示当前连接、已保存选择和待应用草稿。重命名和更新
认证信息使用配置菜单；内容变化请重新导入。删除前必须解除已保存选择及当前
连接的引用。切换 WARP 账号不会丢失导入配置库。

## Compatibility / 兼容范围

| Source and transport | CONNECT-IP H3/H2 | L4 |
| --- | --- | --- |
| OpenVPN (Custom), TCP | Supported | Supported |
| OpenVPN (Custom), UDP | Supported | Cannot enable |
| WireGuard (Custom), UDP | Supported | Cannot enable |
| VPN Gate, directory TCP | Supported | Supported |

L4 can store UDP and WireGuard imports. Enabling them requires the explicit
**Switch to CONNECT-IP and apply** action. Capability discovery prevents enabling
WireGuard when the native binary was compiled without it. Such a binary rejects
an existing enabled WireGuard selection; it never ignores that selection.

OpenVPN supports one `remote`, a domain or IP, TCP or UDP, inline CA/client
certificate/key, `tls-auth`/`tls-crypt`, username/password and encrypted key
passwords. Family qualifiers such as `tcp4` and `udp6` are enforced. It requires
TUN and TLS 1.2 or newer with server-certificate verification. TAP, multiple
remotes, external certificate/key files, scripts, plugins, compression,
verification bypasses and interactive MFA/SSO are rejected. This is an allowlist,
not a general OpenVPN desktop client. Unsupported directives are reported during
preview. A failed custom authentication does not automatically repeat passwords.
VPN Gate retains its stricter directory/IP validation and existing retry policy.

WireGuard accepts a standard single `[Interface]` and single `[Peer]`:
`PrivateKey`, `Address`, `DNS`, `MTU`, `PublicKey`, `PresharedKey`, `Endpoint`,
`AllowedIPs`, `PersistentKeepalive`. Keys must be nonzero 32-byte Base64 values;
DNS entries must be IP addresses. One address per family is supported. Hooks,
multiple peers and platform-specific `wg-quick` routing directives are rejected.

Partial `AllowedIPs` is supported. Outbound destinations and authenticated inbound
sources are both checked. Uncovered proxy traffic is refused; explicit direct
rules retain their existing behavior. External exit-IP probes can be unavailable
for a valid private-network tunnel without failing the connection. Idle
WireGuard key expiry alone does not disconnect a healthy idle session.

L4 可以保存 OpenVPN UDP 和 WireGuard 配置，但不能直接启用。请使用页面的
**切换为 CONNECT-IP 并应用**。OpenVPN 首版只支持单 remote、TUN、内嵌证书/密钥
及非交互式认证；不支持 TAP、外部文件、脚本、插件、多服务器或 MFA/SSO。
WireGuard 首版支持标准单 Peer 和部分 AllowedIPs；范围之外的代理流量被拒绝，
显式直连规则仍生效。局部网络配置不能访问公网探测服务时，出口信息可能不可用，
这不等同于连接失败。

## DNS, MTU and failures / DNS、MTU 与故障

Endpoint names resolve inside the current WARP session. Protocol UDP uses the
private WARP network stack, bypassing the business-traffic “disable QUIC” filter.
Neither protocol opens a physical socket to its VPN server.

Custom exits use the final tunnel for remote DNS. WireGuard prefers its configured
DNS IPs, otherwise the existing tunnel DNS; AllowedIPs applies to DNS too. A
configuration whose DNS is entirely outside AllowedIPs shows a DNS warning. Use
reachable DNS or IP destinations. DNS errors do not fall back to physical DNS.

WireGuard defaults to inner MTU 1280, with explicit MTU in the project's 1280–9000
range. Its imported MTU controls the final interface; it is not capped by the WARP
interface's MTU. The WARP stack for a chain uses MTU 1280 separately from the final interface.
Protocol UDP larger than this uses IPv4 fragmentation or the private IPv6 UDP
fragment/reassembly path; its buffers and queues are bounded. An existing WARP
session with another MTU is reconnected when first enabling a chain.

All sources use WARP connection, authenticated protocol negotiation, final
platform-network configuration, then traffic admission. Applying another exit
closes old final traffic and destroys its protocol session first. Terminal
failures stop the entire chain and retain the requested selection and error.
There is no automatic WARP-only fallback. Windows keeps the existing single
Agent-owned Wintun; Android keeps VpnService and its interface handoff. Android
process termination still requires system Always-on/Lockdown for system-level
blocking; an app-level blocker is not a system guarantee.

Home identifies `WARP → exit name`; source details remain visible in the chain
page. Traffic counters and exit IP describe the final exit. WARP RTT continues
to describe only the WARP leg.

服务器域名解析及协议 UDP 均通过当前 WARP 会话。远程 DNS 必须通过最终出口，
不会回退到物理 DNS；DNS 地址也受 AllowedIPs 限制。默认 WireGuard 内层 MTU 为
1280。切换出口先停止旧出口流量并清理旧协议会话；终止性错误会断开整条链，
保留配置与错误，不会自动退化为仅 WARP。Windows/Android 复用现有平台接口和
清理机制。Android 应用进程结束后的系统级阻断仍依赖系统 Always-on/Lockdown。

## Configuration examples / 配置结构示例

These placeholders are not usable credentials. Obtain matching keys/certificates
and an endpoint from the server administrator. Keep CA verification enabled.

```ini
# OpenVPN (Custom): replace the inline PEM with the administrator's CA.
client
dev tun
proto tcp-client
remote vpn.example.org 443
auth-user-pass
remote-cert-tls server
tls-version-min 1.2
<ca>
... administrator-provided PEM certificate ...
</ca>
```

```ini
# WireGuard (Custom): replace every key placeholder with the real Base64 key.
[Interface]
PrivateKey = <client-private-key>
Address = 10.8.0.2/32
DNS = 10.8.0.1
MTU = 1280
[Peer]
PublicKey = <server-public-key>
Endpoint = vpn.example.org:51820
AllowedIPs = 10.8.0.0/24
PersistentKeepalive = 25
```

示例只展示结构。请替换为管理员提供的服务器地址、证书和密钥，保留证书验证。
请勿将实际配置、私钥、密码或原始诊断抓包提交到仓库。

## Storage, compatibility and dependencies

Schema 17 migrates the previous VPN Gate switch and reference without changing
favorites, cached configurations or pinned snapshots. Failed validation keeps the
original settings file. Shared settings contain only the source and immutable
configuration ID/revision. Older clients cannot replace a selected imported exit.
IPC fields are appended; none of the old field numbers is reordered or reused.

Each imported record, including credentials and metadata, is separately encrypted
in `chain-profiles`: current-user DPAPI on Windows, AES-256-GCM with Android
Keystore on Android. Configuration ID is authenticated as encryption context.
Records are atomically replaced under a file lock. An edit revision protects
rename, delete and credential updates from stale concurrent writes. Temporary
objects are encrypted; orphan temporary files are collected under the same lock.
Public status/IPC/diagnostics contain only references and allowlisted metadata.
The explicit clear-all-data workflow removes these objects after disconnecting.

| Component | Pin / purpose | License |
| --- | --- | --- |
| [BoringTun](https://docs.rs/crate/boringtun/0.7.1) | `=0.7.1`, Rust protocol API, default features disabled | BSD-3-Clause |
| OpenVPN 3 Core + Mbed TLS | Existing embedded native bridge; TCP and UDP | Existing [native source notices](VPN_GATE.md#sources-licenses-and-validation) |
| [flutter_svg](https://pub.dev/packages/flutter_svg/versions/2.3.0) | `2.3.0`, local SVG assets | MIT |
| smoltcp | `=0.13.1`, existing stack with 16 KiB fragmentation buffer | 0BSD |

Cargo and Flutter lockfiles contain transitive versions and checksums. BoringTun's
CLI, OS tunnel/device layer, JNI and C FFI features are not enabled. The
`wireguard` feature defaults on in both desktop and Android crates and can be
disabled for native size comparison. The user-provided editable monochrome SVGs are
`assets/icons/openvpn.svg` and `assets/icons/wireguard.svg`, with a 24×24 viewBox;
VPN Gate retains its globe. OpenVPN's 32×32 path is scaled proportionally by 0.75;
both assets use the current theme color without altering their silhouettes.
BoringTun attribution is bundled in
the app's license registry; Flutter handles its Dart-package notices.

Read [validation and size evidence](CHAIN_PROXY_VALIDATION.md) for actual build
results and unavailable checks. Compile-only evidence does not establish real
VPN lifecycle or external leak behavior. See [Contributing](../CONTRIBUTING.md)
for the workstation and isolated-runner boundaries.
