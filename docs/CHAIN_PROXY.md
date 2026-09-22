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

OpenVPN supports up to 16 distinct `remote` endpoints, domains or IPs, with
one TCP or UDP transport shared by every candidate. Family qualifiers such as
`tcp4` and `udp6` remain specific to each endpoint. Startup tries file order,
or a fresh permutation when `remote-random` is present. The saved endpoint is
always the first file candidate; connection details separately identify the
current attempt and actual connected endpoint. Multi-endpoint imports require
the engine's advertised capability.

Only DNS, dial, transport-close and connection-timeout failures can advance to
the next candidate. Authentication, certificate, configuration and unknown fatal
protocol errors stop immediately. Failed cleanup also stops further attempts.
The candidate phase has a 120-second budget; each attempt gets at most 35 seconds
and no more than its share of the remaining budget. Connection and final platform
admission share a 180-second absolute deadline. An established connection never
switches endpoints automatically after a terminal failure.

Inline CA/client certificates/keys, `tls-auth`/`tls-crypt`, username/password and
encrypted private-key passwords are supported. A password-only profile does not
require a client certificate. `setenv CLIENT_CERT 0` explicitly selects that mode;
`CLIENT_CERT 1` requires an inline certificate and key. Contradictory modes are
rejected. Other `setenv` options are unsupported. `mssfix 0` disables OpenVPN
Core's MSS rewriting; positive values 576–65535 support an optional `mtu` or
`fixed` modifier. Zero does not accept a modifier. H3 with a UDP chain exit
also applies Usque's independent TCP MSS ceiling, including when `mssfix 0`
is present, to account for the nested transport's encapsulation overhead.

TUN, TLS 1.2 or newer and server-certificate verification are required. TAP,
external certificate/key/credential files, scripts, plugins, compression,
verification bypasses and interactive MFA/SSO are rejected during preview.
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
**切换为 CONNECT-IP 并应用**。OpenVPN 支持最多 16 个同为 TCP 或同为 UDP 的
remote 候选；保留各端点的地址族限制。默认按文件顺序尝试，`remote-random`
为每次连接生成一次随机顺序。只在建立连接时切换候选；认证、证书、配置及未知
致命协议错误立即停止，错误密码不会在备用端点重复尝试。候选阶段总计最多
120 秒，每个候选最多 35 秒且受剩余预算分摊限制；连接及平台应用共用 180 秒
绝对截止时间。已经连接后的终止性故障仍断开整条链。

纯用户名/密码配置可以不带客户端证书；支持精确的 `setenv CLIENT_CERT 0/1`，
与证书矛盾时拒绝。`mssfix 0` 关闭 OpenVPN Core 自身的 MSS 修改；正数范围为
576–65535，支持可选的 `mtu` 或 `fixed` 修饰符。H3 搭配 UDP 链式出口时，Usque
另按嵌套封装开销限制 TCP MSS，此上限也适用于 `mssfix 0`。仍不支持 TAP、外部
文件、脚本、插件或 MFA/SSO。
WireGuard 首版支持标准单 Peer 和部分 AllowedIPs；范围之外的代理流量被拒绝，
显式直连规则仍生效。局部网络配置不能访问公网探测服务时，出口信息可能不可用，
这不等同于连接失败。

## DNS, MTU and failures / DNS、MTU 与故障

Endpoint names resolve inside the current WARP session. Protocol UDP uses the
private WARP network stack, bypassing the business-traffic “disable QUIC” filter.
Neither protocol opens a physical socket to its VPN server.

Custom exits use the final tunnel for remote DNS. WireGuard prefers its configured
DNS IPs, otherwise the existing tunnel DNS; AllowedIPs applies to DNS too. A
candidate is filtered by the final address families and AllowedIPs before it is
used. Explicitly configured DNS is never replaced merely because all of it was
filtered out. With no usable DNS, the private tunnel and IP destinations remain
available; the system VPN uses the in-app synthetic DNS service to return failure.
It never leaves platform DNS unspecified to obtain physical fallback.

Final-exit queries start with UDP and add an alternative after 250 ms. Configured
servers receive their first UDP attempt before TCP alternatives; a single DNS
server gets its TCP alternative after 250 ms. Servers and protocols share at most
two concurrent attempts and one four-second question deadline. Each attempt has
at most one second, shortened when necessary to reserve time for later candidates.
Truncated UDP responses retry TCP immediately. TCP connections are reused within
the same final session; cancelled or invalid exchanges are never returned to the
pool. Valid NXDOMAIN/NODATA answers are terminal. Direct-rule DNS retains its
separate policy. Endpoint resolution through WARP is also separate.

WireGuard defaults to inner MTU 1280, with explicit MTU in the project's 1280–9000
range. Its imported MTU controls the final interface; it is not capped by the WARP
interface's MTU. The WARP stack for a chain uses MTU 1280 separately from the final interface.
For OpenVPN (Custom) UDP and WireGuard (Custom) over H3, TCP SYN/SYN-ACK MSS
is capped in both directions so ordinary TCP data fits the 1280-byte WARP
packet budget after outer IP/UDP headers and protocol overhead. WireGuard
includes the peer's 16-byte padding; OpenVPN reserves 128 bytes for its supported
data-channel crypto modes. Smaller MSS values are preserved. Interface MTU,
stored configuration, direct rules, H2 and TCP exits are unchanged. New TCP
connections use the current outer transport; existing connections retain their
negotiated MSS after a transport change. Authenticated TCP options and fragmented
SYN packets are not rewritten. This TCP mitigation does not eliminate the need
for UDP fragmentation or prove every external path's MTU.

H3 搭配 OpenVPN (Custom) UDP 或 WireGuard (Custom) 时，Usque 会在两个方向上限制
TCP SYN/SYN-ACK 的 MSS，为外层 IP/UDP、协议加密及 WireGuard 填充预留空间。
较小的 MSS 保持不变；不降低接口 MTU，不改写保存的配置，也不影响直连规则、H2
或 TCP 出口。外层传输切换后，现有 TCP 连接仍使用建连时的 MSS；新连接使用当前
传输的策略。带 TCP 认证选项或分片的 SYN 不改写。此修复针对 TCP 首次访问延迟，
不表示所有 UDP 大包问题均已解决。遇到旧版本的 H3 链式首次访问延迟，可将 WARP
外层切到 CONNECT-IP H2 并应用后重试。

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
不会回退到物理 DNS；DNS 地址在使用前按最终地址族和 AllowedIPs 过滤。没有可用
DNS 时仍可使用 IP 访问局部网络，系统 VPN 的应用内 DNS 返回明确失败。第一个
DNS 候选立即以 UDP 查询，250 ms 后启用备用候选；只有一个 DNS 时，该备用为 TCP。
多个 DNS 优先完成各服务器的 UDP 首试，再安排 TCP 备用。两种协议共用最多两个
并发与整轮 4 秒预算；每次最多 1 秒，候选较多时缩短，避免后面的服务器没有机会。
UDP 截断回复立即改用 TCP；TCP 连接在同一链式会话内复用。有效的 NXDOMAIN/NODATA
不会重复向其他候选查询。默认 WireGuard 内层 MTU 为
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

Read the original [validation and size evidence](CHAIN_PROXY_VALIDATION.md) and
the later [fix validation record](CHAIN_PROXY_FIX_VALIDATION.md) for candidate-specific
build results and unavailable checks. Compile-only evidence does not establish real
VPN lifecycle or external leak behavior. See [Contributing](../CONTRIBUTING.md)
for the workstation and isolated-runner boundaries.

## Imported record compatibility / 导入记录兼容

Shared settings remain schema 17 and store only configuration references.
Encrypted records are written as version 2, with a 192 KiB serialized plaintext
limit and 256 KiB ciphertext limit. Version 1 is read without changing IDs,
revisions or saved selections. Historical version 1 records between 192 and
256 KiB plaintext remain readable and deletable; a later edit must meet the new
write limit and otherwise leaves the original record intact. Legacy incomplete
authentication records can be listed/deleted but must be reimported with a valid
authentication mode before connecting. No automatic rewrite or batch deletion
occurs. Windows selection commits and deletion hold the configuration transaction
before the library lock, so concurrent operations cannot leave a dangling reference.

共享设置仍为 schema 17，仅保存配置引用。加密对象写入版本 2，序列化明文最多
192 KiB、密文最多 256 KiB。兼容读取版本 1，不改变 ID、版本引用或已保存选择；
历史较大记录可读取、删除，再次修改超限时保留原对象。旧版缺少认证方式的记录
可管理，但必须重新导入有效配置后才能连接。Windows 删除与选用共用配置事务，
避免并发操作留下失效引用。
