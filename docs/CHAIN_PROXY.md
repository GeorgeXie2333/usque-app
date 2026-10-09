# Chain proxy / 链式代理

Open **Proxy → Chain proxy**. The page switch comes first, then the source
selector: six choices that always use this order:

1. **OpenVPN**
2. **WireGuard**
3. **WARP® via WireGuard**
4. **VPN Gate**
5. **HTTP**
6. **SOCKS5**

The source names remain English in every locale. On phones and other layouts
with less than 600 logical pixels of content width, the source heading stays
above a compact selector showing the current source. Tap it to open a bottom
sheet, then choose a source to close the sheet and show its configuration. The
selector keeps the existing selected choice's size. Wider layouts keep all six
choices visible, wrapping normally and stacking at large text. A source the
running engine cannot provide is shown disabled with its reason. One exit
is enabled at a time:
`Application → WARP → selected chain exit → Internet`. System VPN, SOCKS5 and
HTTP share the final exit while retaining their own protocol capabilities;
HTTP CONNECT does not gain UDP support. Explicit direct rules still apply.

打开 **代理 → 链式代理**，总开关在最上方，其下的来源名称和顺序固定为
**OpenVPN**、**WireGuard**、**WARP via WireGuard**、**VPN Gate**、**HTTP**、**SOCKS5**。手机和内容宽度小于 600 逻辑像素的
窄屏上，“出口来源”标题独占一行，下方选择框显示当前来源，大小与原来的选中
标签一致。点击选择框打开底部列表，选中来源后列表自动收起并显示对应配置。
宽屏仍显示六个选项，通常横向排列并自动换行，大字号时竖排；不可用的来源会
禁用并说明原因。每次启用一个出口，系统 VPN、SOCKS5 和 HTTP 共用最终出口，
各入口保留自身协议能力，显式直连规则继续生效。

All sources share the page heading, enable switch, **Current connection** section
and apply bar. The current connection always describes the running exit, even
while browsing a different source. Below it, custom sources provide import and
paste actions with saved configurations; VPN Gate provides refresh, country and
favorite filters with public nodes. Selection rows use the same radio controls
and saved/current markers. VPN Gate observations remain labeled as remote data.

六种来源共用页头、总开关、“当前连接”和底部应用栏。切换来源浏览时，“当前连接”
仍显示正在使用的出口。下方内容随来源变化：OpenVPN／WireGuard 提供导入、粘贴
和已保存配置，VPN Gate 提供刷新、国家／地区筛选、收藏和公共节点。两类列表
使用相同的单选控件及“已保存的选择”“当前连接”标记；VPN Gate 的远端观测说明
保持可见。

The chain editor's current-connection section shows factual HTTP/SOCKS scope
hints from the runtime snapshot and applied session profile. Proxy-only
connections describe their application-only coverage, including failed sessions
whose scope is confirmed; tunnel connections keep explicit direct and per-app
rules. A failed confirmed VPN session warns that
ordinary networking may return only when the runtime Kill Switch is explicitly
inactive. Saved changes and editor drafts do not imply that the running session
changed. Unknown scope remains unknown; no new protection status is inferred.
Android system-blocking advice is conditional because an absent Lockdown report
does not prove it is disabled. A SOCKS5 UDP association accepted by the server is
not evidence of working end-to-end UDP forwarding. Home omits these explanatory
paragraphs. Its phone layout retains the compact chain summary and WARP status
row. Refreshes without a separate WARP observation use known connection stages;
unknown states show a dash, and reported reconnects or failures remain visible.

链式代理编辑页的“当前连接”显示 HTTP/SOCKS 范围提示，依据实际运行状态和已应用的
会话配置。已确认范围的仅代理会话即使失败，也说明只接管应用交给 Usque 的连接；
VPN 模式说明显式直连与分应用规则继续生效。已确认的 VPN 会话失败且运行状态明确
显示 Kill Switch 未启用时，
才提示设备可能恢复普通网络。已保存修改和编辑草稿不会被当成当前会话；范围未知时
不猜测，也不新增“已保护”状态。Android 系统阻断说明使用条件式文案，未上报
Lockdown 不等于已关闭。SOCKS5 UDP 关联被服务器接受不等于端到端 UDP 转发已验证。
首页不显示这些长篇说明，手机仍保留简短的链式摘要及 WARP 状态行。刷新缺少独立
WARP 上报时使用已确认的连接阶段；无法确认时显示横线，明确上报的重连或失败仍会显示。

WARP via WireGuard supports generated/imported configurations and editable
endpoints. See [WARP via WireGuard](WARP_WIREGUARD.md).

WARP via WireGuard 支持生成／导入配置和编辑端点，详见
[使用指南](WARP_WIREGUARD.md)。

## Import, select and apply / 导入、选用与应用

Choose a custom source, then **Import file** or **Paste configuration**. Use
UTF-8 text up to 128 KiB. Windows opens its native multi-file picker; Android uses the
system multi-document provider. Select up to 128 files from the current source
at once; the library still holds at most 128 configurations. Android TV devices without a document provider can
use pasted text. Usque reads the document once and does not retain a dependency
on its path or document-provider permissions. An imported file is checked as
soon as the dialog opens; pasted text is checked with **Check configuration**.
For multiple files, a batch list automatically checks every file offline and
shows ready, incomplete, failed and saved counts. Expand an entry to review its
details, change its name or supply its individual credentials. File imports default
to the filename without its final extension. Incorrect encoding, empty
or oversized files, unsupported configurations and source mismatches are reported
per file; validation does not test connectivity. **Import valid items** saves
only entries that passed validation and have the required name and credentials.
You can complete remaining entries and save them afterward. Already imported
entries are not submitted again. Canceling the initial check saves nothing.

Saving keeps successful entries even if another fails. If communication is
interrupted, further saves stop: close the dialog and inspect the library before
importing again, because the interrupted entry may already have been saved.
Files with identical names or content create separate entries; they do not
replace existing configurations.
Text that structurally belongs to the other source is reported before the
engine runs.

The check displays the endpoint, transport, IP version, addresses, DNS,
AllowedIPs and MTU where available. OpenVPN addresses and DNS may be negotiated
by the server. Single-file imports also default to the filename without its final
extension; pasted configurations default to the endpoint host. Supply the requested
username, password or encrypted private-key password and **Save configuration**.
Errors identify a field and line without reproducing configuration values.
Saving to the library neither selects the configuration nor starts a connection.

Enable the page switch, select a saved configuration, then apply from the
action bar. The list marks the saved selection, the configuration used by the
current connection, and configurations marked **Not available with L4**. The selected
configuration's **Technical details** expand on request under its selected row.
The action bar names the pending
selection and states why a draft cannot be applied yet; while connected, its
button reads **Apply and reconnect**. A disconnected connection stays
disconnected; an active connection applies the new exit using the existing
connection workflow. Switching sources is browsing, not editing: it never asks
to discard anything, and a selection made under one source is still there when
you return to it until settings are applied. Navigating away asks before
discarding an unapplied draft. **Current connection** shows the live state,
the endpoint being tried and the connected endpoint, and explains failures and
missing DNS inline.

Use the configuration's menu to rename it or update OpenVPN credentials. Import
again to replace configuration content. Credential changes are used on the next
connection; they do not silently reconnect the current one. A configuration that
is selected, saved, or used by the current connection cannot be deleted; the
page says so instead of hiding the action. Imports are device-wide, independent
of the selected WARP account.

选择自定义来源后，可导入文件或粘贴配置。两种入口共用 Rust 校验流程，限制为
每个文件 128 KiB UTF-8 文本。Windows 和 Android 均支持多选，每次最多选择
128 个当前来源的文件，配置库总量仍限制为 128 条。TV 没有系统文件选择器时请粘贴文本。
单文件沿用原有对话框；多文件自动逐项离线预检，显示可导入、待补充、失败和已保存数量。
展开项目可检查详情、修改名称并逐项填写凭据，默认名称为去掉最后一个后缀的文件名。
空文件、超限、编码错误、来源不符或不受支持的配置逐项报错，不进行连通性探测。
点击**导入合格项**仅保存校验通过且名称与凭据齐全的项目；其余项目可补充后继续保存，
已导入项目不会再次提交。预检期间取消不会保存配置。
保存失败不回滚其他成功项；通信中断则停止后续提交，请关闭并核对配置库后再导入，
因为中断的项目可能已经保存。同名或相同内容仍作为独立新配置导入，不覆盖已有配置。
导入文件后立即检查；
粘贴文本需点击**检查配置**。粘贴到错误来源的配置会在调用引擎前得到提示。
检查后补充认证信息、命名（单文件导入默认使用不含后缀的文件名，粘贴导入默认使用
服务器主机名）并保存；保存不会选用配置
或自动连接。打开总开关、选择配置，再在底栏应用。列表会标记已保存的选择、
当前连接使用的配置以及标记为**不支持 L4** 的配置；点击所选配置下的“技术详情”
可展开详细信息。
底栏说明待应用的选择及暂时不能应用的原因；已连接时按钮为**应用并重新连接**。
切换出口来源只是浏览，不会弹出“放弃未应用的修改”提示；在某个来源下做出的
选择会保留到切回时，直到应用为止。只有离开页面才会询问是否放弃未应用的修改。
重命名和更新认证信息使用配置菜单；内容变化请重新导入。被选中、已保存或
当前连接使用的配置不能删除，页面会直接说明原因。切换 WARP 账号不会丢失
导入配置库。

VPN Gate uses the same **Apply changes** and **Apply and reconnect** actions.
Preparing a node configuration shows progress and **Cancel** in the apply bar;
failed preparation or application keeps the requested selection available for
review and retry. Refresh controls stay beside the public-node list.

VPN Gate 同样使用“应用更改”和“应用并重新连接”。准备节点配置时，应用栏显示
进度及“取消”；准备或应用失败后，保留待应用的选择供检查和重试。刷新操作位于
公共节点列表区域。

## HTTP and SOCKS5 exits / HTTP 与 SOCKS5 出口

Choose **HTTP** or **SOCKS5**, then **Add proxy**. Enter a name, server hostname
or IPv4/IPv6 address and port (HTTP defaults to 8080, SOCKS5 to 1080). Enable
username/password authentication when required. HTTP uses Basic authentication;
SOCKS5 uses RFC 1929. The server field accepts an address, not a URL or embedded
credentials. Expand **DNS** to choose its transport. Optional TCP DNS entries are
numeric IP addresses, separated by spaces, commas or newlines. Blank entries use
the automatic or inherited resolver policy described below.

Save the configuration, enable chain proxy, select it and apply. Saving alone
does not select or connect. Rename and credential changes retain the existing
selection; credentials take effect on the next connection. Add a replacement
configuration to change the server, DNS or authentication mode.

HTTP means HTTP/1.1 CONNECT, not a TLS connection to the proxy. HTTPS application
traffic retains its own end-to-end TLS. HTTP Basic and SOCKS5 passwords are not
encrypted by those proxy protocols on the WARP-to-proxy leg. Credentials remain
encrypted at rest and are excluded from settings, summaries and diagnostics.

Applications using the local HTTP proxy can also target IPv6 literals. Keep
the brackets in CONNECT authorities and URLs, for example `[2001:db8::1]:443`
and `http://[2001:db8::1]/`.

HTTP and SOCKS5 exits automatically prefer TCP for web traffic by blocking
application UDP/443 on the proxy path. The existing QUIC control shows that it
is managed by the current connection; the saved manual preference is retained
and takes effect again after leaving these exits. Direct traffic, DNS conversion,
other UDP ports and Usque's own HTTP/3 transport keep their existing behavior.
TUN clients receive rate-limited ICMP errors for rejected datagrams and definite
relay failures. Local SOCKS5 clients retain DNS-only associations and use their
own fallback behavior; SOCKS5 has no per-datagram error reply.

Both exits carry TCP with H3, H2 and L4. SOCKS5 additionally uses UDP ASSOCIATE
with H3/H2 when the server accepts it. L4 remains TCP-only for these exits.
HTTP CONNECT does not carry ordinary UDP. Unsupported proxied UDP is rejected;
there is no WARP-only or physical-network fallback. Existing explicit direct
rules remain applicable. SOCKS fragmentation is unsupported; relay packets,
including the SOCKS header, are limited to 16336 bytes. TUN replies must fit its
MTU; IP options, IP fragments, IPv6 extension headers and remote ICMP Echo retain
the stream bridge's existing restrictions.

**Ready** confirms proxy endpoint reachability (and SOCKS authentication).
**TCP forwarding verified** requires a successful real CONNECT. UDP availability
confirms the final SOCKS5 server's acceptance of UDP ASSOCIATE, not end-to-end
delivery. A local DNS-only association does not change that status. Individual
target failures affect that flow; authentication failure closes the chain.

An ASSOCIATE timeout leaves UDP capability unknown. A server that accepts the
association can still silently discard datagrams; neither acceptance nor an
absence of replies is a privacy or reachability guarantee. These outcomes do not
enable direct or WARP-only fallback. Explicit GEO, LAN/CIDR and Android app
bypasses remain intentional exceptions. In proxy-only mode, only traffic sent
to Usque's local listeners is covered; applications that bypass those listeners
are outside the proxy's protection.

VPN protection covers captured traffic from the moment native blocking is
successfully installed, throughout the HTTP/SOCKS session and its protected
handoffs. UDP capability does not authorize another exit. Explicit direct and
app-exclusion rules remain outside that scope. This feature does not change
network-interface address candidates that WebRTC or other browser APIs can expose to a page;
it is not a guarantee that every browser-reported address is the proxy address.

HTTP/SOCKS5 exits resolve names through the final proxy. Choose the method in
**Add proxy → DNS**:

| Choice | What it does |
| --- | --- |
| **Automatic (DoH by default)** | Uses Cloudflare® DoH at `cloudflare-dns.com/dns-query` through the final proxy, with verified TLS and fixed bootstrap IPs. Custom chain DNS or non-default inherited DNS makes it use TCP DNS instead. |
| **Encrypted DNS · Cloudflare** | Always uses that Cloudflare DoH resolver. |
| **DNS over TCP** | Sends DNS over TCP to the numeric servers you enter, or to the inherited servers when the list is empty. |

DoH failure never switches to plaintext DNS or another exit. Switching choices
keeps your custom server list as a draft; saving DoH does not submit it. Typing
the exact built-in addresses counts as inheriting them, so pick **DNS over TCP**
explicitly if you want TCP. Other exits keep their own DNS settings; older
Engines offer TCP only. The default DoH resolver also answers Android's VPN DNS
address.

Apps that pick their own DNS server keep it: valid UDP/53 queries from TUN or
the local SOCKS5 listener are sent as TCP DNS to that server through the final
proxy, with no DoH or physical DNS substitute. This works even when the exit
cannot carry ordinary UDP; other proxied UDP stays unavailable. If the proxy
refuses connections to port 53, those queries fail. Direct DNS settings and
direct routes are unchanged.

The Proxy page no longer offers a DNS mode. Older configurations that already
send hostnames to the final proxy keep doing so for local HTTP/SOCKS clients;
TUN traffic arrives as IP packets, so it cannot use that method. If a later exit
or connection-mode change no longer supports it, applying that change switches
proxy DNS to remote resolution through the current exit and keeps the other
saved DNS values.

Timing details: configured TCP DNS and DoH race at most two complete queries,
starting the backup after 250 ms, within a four-second total deadline. Each DoH
attempt has two seconds including TLS/HTTP setup. SERVFAIL and REFUSED try the
backup; a valid NXDOMAIN/NODATA answer is final. An expired TCP query counts as
a timeout even if the race cancels it first; a loser cancelled before its
deadline does not. Session or network changes cancel outstanding queries and
clear pooled connections. Upstream UDP association starts only when ordinary
UDP arrives, so its refusal or timeout never blocks the DNS relay.

选择 **HTTP** 或 **SOCKS5**，点击**添加代理**，填写名称、服务器域名或 IPv4/IPv6
地址和端口；默认端口分别为 8080、1080。按需启用用户名／密码认证。地址栏不接受
URL 或嵌入凭据。展开 **DNS** 可选择传输方式；可选的 TCP DNS 填写数值 IP，以空格、
逗号或换行分隔，留空时按下述自动或继承策略解析。保存后仍需启用链式代理、选用配置并应用。修改凭据在下次连接
生效；修改服务器、DNS 或认证模式时新增替代配置。

使用本地 HTTP 代理的应用也可访问 IPv6 地址；CONNECT 目标及网址保留方括号，
例如 `[2001:db8::1]:443` 和 `http://[2001:db8::1]/`。

HTTP 与 SOCKS5 出口会自动让网页优先使用 TCP，拦截代理路径上的应用 UDP/443。
现有开关显示“由当前连接自动管理”，保留原手动设置，退出这两类出口后恢复原设置。
直连流量、DNS 转换、其他 UDP 端口及 Usque 自身的 HTTP/3 传输保持原有行为。
TUN 会对拒绝的数据报和明确的转发失败返回限速的 ICMP 错误；本地 SOCKS5 仍保留
仅供 DNS 的关联，由客户端自行决定回退方式，SOCKS5 无法逐数据报回复错误。

两种出口在 H3/H2/L4 下均支持 TCP；SOCKS5 在 H3/H2 下可按需使用服务器提供的 UDP
关联。HTTP 及 L4 下的普通代理 UDP 不可用，失败不会退回仅 WARP 或物理直连。
“已就绪”不代表目标转发已验证；实际 CONNECT 成功后才显示“TCP 转发已验证”。
最终 SOCKS5 服务器接受 UDP 关联不等于已验证端到端数据可达；本地仅供 DNS 的关联
不会改变出口 UDP 状态。仅支持 FRAG=0，含 SOCKS 头的 relay 包
上限为 16336 字节；TUN 回包须符合 MTU，原有 IP 包限制继续适用。

UDP 关联超时后，能力状态仍为未知；服务器接受关联后也可能静默丢包。接受关联或
没有收到回包，都不能证明端到端可达或隐私安全，也不会触发直连或仅 WARP 回退。
显式 GEO、LAN/CIDR 及 Android 应用旁路仍是用户指定的例外。仅代理模式只覆盖
发送到 Usque 本地监听器的流量，未使用这些监听器的应用流量不在代理保护范围内。

VPN 防护从原生阻断成功安装开始，覆盖 HTTP/SOCKS 会话及其受保护交接期间接管的
流量；UDP 能力不构成改走其他出口的授权。显式直连和应用排除仍不属于这一范围。
此功能不会修改浏览器通过 WebRTC 等接口向网页提供的网卡候选地址，因此不承诺
浏览器报告的每个地址都等于代理出口地址。

HTTP/SOCKS5 出口通过最终代理解析域名，在**添加代理 → DNS** 中选择方式：

| 选项 | 作用 |
| --- | --- |
| **自动（默认 DoH）** | 经最终代理使用 Cloudflare 的 `cloudflare-dns.com/dns-query`，校验 TLS 证书并使用固定引导 IP。设置了链专属 DNS 或非默认继承 DNS 时改用 TCP DNS。 |
| **加密 DNS · Cloudflare** | 始终使用上述 Cloudflare DoH。 |
| **TCP DNS** | 通过 TCP 查询填写的数字地址；留空时使用继承的服务器。 |

DoH 失败不会改用明文 DNS 或其他出口。切换选项时保留自定义服务器草稿，保存 DoH
时不提交该列表。手填与内置默认值完全相同的地址等同于继承，如需 TCP 请明确选择
**TCP DNS**。其他出口保持自身 DNS 设置；旧引擎仅提供 TCP。默认 DoH 同时应答
Android VPN 的 DNS 地址。

应用自己指定的 DNS 服务器保持不变：TUN 和本地 SOCKS5 的有效 UDP/53 查询会经最终
代理以 TCP 发往该服务器，不会换成 DoH 或本机 DNS。即使出口不支持普通 UDP 也可
使用，其他代理 UDP 仍不可用。代理拒绝连接 53 端口时，这些查询失败。直连 DNS
设置和直连规则不受影响。

代理页不再提供 DNS 方式选择。旧配置若已设为把域名交给最终代理解析，本地 HTTP/SOCKS
客户端会继续这样做；TUN 流量只有 IP 包，无法使用该方式。之后若更换出口或连接模式
导致该方式不可用，应用修改时会改为经当前出口远程解析，其他已保存的 DNS 设置不变。

时间细节：配置的 TCP DNS 和 DoH 最多并发两个完整查询，250 ms 后启动备用，总期限
4 秒；每次 DoH 尝试最多 2 秒（含 TLS/HTTP 建连）。SERVFAIL/REFUSED 尝试备用，
有效的 NXDOMAIN/NODATA 为最终结果。已到期的 TCP 查询即使先被竞速取消仍计为超时，
截止前被取消的落败查询不计。会话或网络变化会取消未完成查询并清理连接池。上游 UDP
关联在普通 UDP 数据到来时才建立，拒绝或超时不会阻塞 DNS 中继。

HTTP 出口未使用到代理服务器的 TLS；HTTP Basic 和 SOCKS5 认证在 WARP 到代理
这一段不提供额外加密，应用自身的 HTTPS 加密继续有效。凭据仅在设备加密库保存，
不会进入设置、摘要或诊断。

## Compatibility / 兼容范围

| Source and transport | CONNECT-IP H3/H2 | L4 |
| --- | --- | --- |
| OpenVPN, TCP | Supported | Supported |
| OpenVPN, UDP | Supported | Cannot enable |
| WireGuard, UDP | Supported | Cannot enable |
| WARP via WireGuard, UDP | Supported | Cannot enable |
| VPN Gate, directory TCP | Supported | Supported |
| HTTP CONNECT | TCP | TCP |
| SOCKS5 | TCP; UDP when accepted by server, except proxied UDP/443 | TCP |

L4 can store UDP and WireGuard imports. Enabling them requires the explicit
**Turn off L4 and apply** action, which replaces the action bar's
button while the conflict exists. Capability discovery prevents enabling
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

L4 可以保存 OpenVPN UDP 和 WireGuard 配置，但不能直接启用。存在冲突时底栏
按钮直接变为**关闭 L4 并应用**。OpenVPN 支持最多 16 个同为 TCP 或同为 UDP 的
remote 候选；保留各端点的地址族限制。默认按文件顺序尝试，`remote-random`
为每次连接生成一次随机顺序。只在建立连接时切换候选；认证、证书、配置及未知
致命协议错误立即停止，错误密码不会在备用端点重复尝试。候选阶段总计最多
120 秒，每个候选最多 35 秒且受剩余预算分摊限制；连接及平台应用共用 180 秒
绝对截止时间。已经连接后的终止性故障会停止整条链的数据通道；Android 保留
用于阻断流量的 VPN 接口，直到用户主动断开。

纯用户名/密码配置可以不带客户端证书；支持精确的 `setenv CLIENT_CERT 0/1`，
与证书矛盾时拒绝。`mssfix 0` 关闭 OpenVPN Core 自身的 MSS 修改；正数范围为
576–65535，支持可选的 `mtu` 或 `fixed` 修饰符。H3 搭配 UDP 链式出口时，Usque
另按嵌套封装开销限制 TCP MSS，此上限也适用于 `mssfix 0`。仍不支持 TAP、外部
文件、脚本、插件或 MFA/SSO。
WireGuard 首版支持标准单 Peer 和部分 AllowedIPs；范围之外的代理流量被拒绝，
显式直连规则仍生效。局部网络配置不能访问公网探测服务时，出口信息可能不可用，
这不等同于连接失败。

## DNS, MTU and failures / DNS、MTU 与故障

On Android, a physical-network change during an established chain connection
rebuilds the whole chain after native cleanup is confirmed. The existing VPN
interface remains blocking until the replacement final network is attached.
Recovery waits while no usable physical network is selected and coalesces rapid
changes. A retryable chain transport error uses the same recovery even when its
error snapshot arrives before the physical-network callback. Replacement network
failures use 1/2/4/8/15/30-second backoff. The profile, blocking TUN and armed Kill
Switch remain in place throughout recovery. Disconnect cancels recovery.
Authentication, certificate, configuration and unconfirmed-cleanup failures stop
automatic attempts and preserve the error evidence. For HTTP/SOCKS chains,
terminal failure retains the blocking interface with Kill Switch on; with it off,
the interface is released only after native cleanup is confirmed and only by its
current owner. Unconfirmed cleanup always keeps it blocking. Other chain sources
retain their existing interface behavior. Retry or disconnect remains explicit.
Rebuilding an applied HTTP/SOCKS VPN session for a configuration change or
activating another account starts a protected replacement, including when
Kill Switch is off. For account changes, Android rereads the latest saved
account in the VPN process. It retains the old interface until the
replacement is running, and ignores superseded callbacks. Terminal failure
retains the interface when the inherited applied source or the incoming target
has Kill Switch on; when both are off, confirmed native cleanup releases it.
An unknown inherited preference is retained conservatively. The selected new
account remains available for Retry; explicit Disconnect still releases it.
Qualification follows the profile actually established on the owned TUN,
including initial startup or failure before a first
successful connection; saved pending settings cannot supply that proof. Other
account selections retain their prior behavior.
HTTP/SOCKS cold startup establishes the VPN interface before waiting for a physical
network; explicit LAN, CIDR, GEO, domain and per-app bypasses remain unchanged.
Underlay security failures keep
their original classification. Ordinary WARP mode keeps its existing native
migration/reconnect behavior.

Android 上已建立的链式连接遇到物理网络变化时，会在确认原生实例清理完成后
重建整条链。新出口网络接管前保留用于阻断流量的 VPN 接口；没有可用物理网络
时等待恢复，连续变化会合并处理。可重试的链传输错误也进入同一恢复流程，
包括错误快照先于网络回调到达的情况；重建中的网络失败按 1/2/4/8/15/30 秒
退避重试。恢复期间保留配置、阻断用 TUN 和已开启的 Kill Switch。主动断开会
取消恢复。认证、证书、配置错误或清理未确认会停止自动尝试并保留错误证据。
HTTP/SOCKS 链式终止失败时，开启 Kill Switch 会保留阻断接口；关闭时仅在确认
原生实例停止后，由当前代次释放仍属自己的接口。清理未确认时始终保持阻断。
其他链式来源保留原有接口行为，重试和主动断开仍由用户决定。已应用的 HTTP/SOCKS
VPN 会话因配置变化重建或切换账号时，会自动交接，包括关闭 Kill Switch 时。
切换账号会由 VPN 进程重新读取最新选择。新会话成功运行前保留旧接口和保护，
忽略过期回调；终止失败时，原来
已应用的来源或新目标任一开启 Kill Switch 都会保留接口，二者均关闭则在确认原生
停止后释放。来源偏好不明时保守保留，新选择仍供重试，不能按尚未生效的新偏好
提前解除已开启的阻断。是否接管以当前接口实际建立时应用的配置为依据，包含
首次连接成功前的启动和失败阶段，不能借用尚未应用的保存值。
主动断开仍会释放接口，其他模式的账号选择行为不变。HTTP/SOCKS 冷启动
先建立 VPN 接口再等待物理网络；显式 LAN、CIDR、GEO、域名及分应用绕过规则不变。
底层安全错误保留原始分类；普通 WARP
模式保留原有原生迁移和重连行为。

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

VPN-protocol final-exit queries start with UDP and add an alternative after 250 ms. Configured
servers receive their first UDP attempt before TCP alternatives; a single DNS
server gets its TCP alternative after 250 ms. Servers and protocols share at most
two concurrent attempts and one four-second question deadline. Each attempt has
at most one second, shortened when necessary to reserve time for later candidates.
Truncated UDP responses retry TCP immediately. TCP connections are reused within
the same final session; cancelled or invalid exchanges are never returned to the
pool. Valid NXDOMAIN/NODATA answers are terminal. Direct-rule DNS retains its
separate policy. Endpoint resolution through the WARP tunnel is also separate.

WireGuard defaults to inner MTU 1280, with explicit MTU in the project's 1280–9000
range. Its imported MTU controls the final interface; it is not capped by the WARP
interface's MTU. The WARP stack for a chain uses MTU 1280 separately from the final interface.
For OpenVPN UDP and WireGuard over H3, TCP SYN/SYN-ACK MSS
is capped in both directions so ordinary TCP data fits the 1280-byte WARP
packet budget after outer IP/UDP headers and protocol overhead. WireGuard
includes the peer's 16-byte padding; OpenVPN reserves 128 bytes for its supported
data-channel crypto modes. Smaller MSS values are preserved. Interface MTU,
stored configuration, direct rules, H2 and TCP exits are unchanged. New TCP
connections use the current outer transport; existing connections retain their
negotiated MSS after a transport change. Authenticated TCP options and fragmented
SYN packets are not rewritten. This TCP mitigation does not eliminate the need
for UDP fragmentation or prove every external path's MTU.

H3 搭配 OpenVPN UDP 或 WireGuard 时，Usque 会在两个方向上限制
TCP SYN/SYN-ACK 的 MSS，为外层 IP/UDP、协议加密及 WireGuard 填充预留空间。
较小的 MSS 保持不变；不降低接口 MTU，不改写保存的配置，也不影响直连规则、H2
或 TCP 出口。外层传输切换后，现有 TCP 连接仍使用建连时的 MSS；新连接使用当前
传输的策略。带 TCP 认证选项或分片的 SYN 不改写。此修复针对 TCP 首次访问延迟，
不表示所有 UDP 大包问题均已解决。遇到旧版本的 H3 链式首次访问延迟，可将 WARP
外层切到 CONNECT-IP H2 并应用后重试。

Protocol UDP larger than this uses IPv4 fragmentation or the private IPv6 UDP
fragment/reassembly path; its buffers and queues are bounded. An existing WARP
session with another MTU is reconnected when first enabling a chain.

VPN protocol sources use WARP connection, authenticated protocol negotiation, final
platform-network configuration, then traffic admission. Applying another exit
closes old final traffic and destroys its protocol session first. Terminal
failures stop the entire chain and retain the requested selection and error.
There is no automatic WARP-only fallback. Windows keeps the existing single
Agent-owned Wintun; Android keeps VpnService and its interface handoff. Android
process termination still requires system Always-on/Lockdown for system-level
blocking; an app-level blocker is not a system guarantee.
For HTTP/SOCKS terminal failures, retaining the blocking interface follows the
applied Kill Switch and handoff rules described above, rather than unconditional
retention for every failure.

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
1280。切换出口先停止旧出口流量并清理旧协议会话；终止性错误会停止整条链的数据
通道，保留配置与错误，不会自动退化为仅 WARP。Android 的 OpenVPN/WireGuard
来源保留原有阻断接口行为；HTTP/SOCKS 终止失败按上文的 Kill Switch 和交接策略
决定是否继续保留接口。Windows/Android 复用现有平台接口和
清理机制。Android 应用进程结束后的系统级阻断仍依赖系统 Always-on/Lockdown。

## Configuration examples / 配置结构示例

These placeholders are not usable credentials. Obtain matching keys/certificates
and an endpoint from the server administrator. Keep CA verification enabled.

```ini
# OpenVPN: replace the inline PEM with the administrator's CA.
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
# WireGuard: replace every key placeholder with the real Base64 key.
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
favorites, cached configurations or pinned snapshots. Schema 18 appends an
optional endpoint override, an IP address and port accepted only with a selected
**WARP via WireGuard** configuration. Failed validation keeps the original
settings file. Shared settings contain only the source, immutable configuration
ID/revision and that optional override. Older clients cannot replace a selected
imported exit. IPC fields are appended; none of the old field numbers is
reordered or reused.

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
| OpenVPN 3 Core + Mbed TLS | Existing embedded native bridge; TCP and UDP | OpenVPN 3 Core used under MPL-2.0 (offered as AGPL-3.0-only or MPL-2.0); Mbed TLS used under Apache-2.0; see [native source notices](VPN_GATE.md#sources-licenses-and-validation) |
| [flutter_svg](https://pub.dev/packages/flutter_svg/versions/2.3.0) | `2.3.0`, local SVG assets | MIT |
| smoltcp | `=0.14.0`, existing stack with 16 KiB fragmentation buffer | 0BSD |

Cargo and Flutter lockfiles contain transitive versions and checksums. BoringTun's
CLI, OS tunnel/device layer, JNI and C FFI features are not enabled. The
`wireguard` feature defaults on in both desktop and Android crates and can be
disabled for native size comparison. The editable monochrome SVGs in
`apps/usque_gui/assets/icons/` were supplied by the project maintainer:
`openvpn.svg` and `wireguard.svg` use a 24×24 viewBox, and `warp-wireguard.svg`
keeps its original `viewBox="120 80 250 330"`. OpenVPN's 32×32 path is scaled
proportionally by 0.75. All three use the current theme color without altering
their silhouettes; VPN Gate retains its globe. The repository does not record an
upstream source or license for these icon paths. The OpenVPN, WireGuard and WARP
names and marks belong to their respective owners and identify the exit source
only. BoringTun attribution is bundled in the app's license registry; Flutter
handles its Dart-package notices.

Read the original [validation and size evidence](CHAIN_PROXY_VALIDATION.md) and
the later [fix validation record](CHAIN_PROXY_FIX_VALIDATION.md) for candidate-specific
build results and unavailable checks. Compile-only evidence does not establish real
VPN lifecycle or external leak behavior. See [Contributing](../CONTRIBUTING.md)
for the workstation and isolated-runner boundaries.

## Imported record compatibility / 导入记录兼容

The [shared configuration](../crates/usque-core/src/config/mod.rs) stores chain
configuration references, plus the
optional **WARP via WireGuard** endpoint override (`ChainExitSettings` IPC fields
5/6). HTTP/SOCKS5 records use version 5, appending `dns_transport` (`auto`, `doh`, `tcp`);
version 4 reads reconstruct only the appended metadata without rewriting credentials,
IDs or revisions. Older clients reject v5. DNS-mode edits require a replacement
proxy configuration; shared DNS policy edits reconnect the session. VPN records are written as version 3, which records the exit source
explicitly, with a 192 KiB serialized plaintext limit and 256 KiB ciphertext
limit. Versions 1 and 2 are read without changing IDs, revisions or saved
selections; their source is recovered from the stored protocol, and a later edit
rewrites the record as version 3. Historical version 1 records between 192 and
256 KiB plaintext remain readable and deletable; a later edit must meet the new
write limit and otherwise leaves the original record intact. Legacy incomplete
authentication records can be listed/deleted but must be reimported with a valid
authentication mode before connecting. No automatic rewrite or batch deletion
occurs. Windows selection commits and deletion hold the configuration transaction
before the library lock, so concurrent operations cannot leave a dangling reference.

共享配置保存链式配置引用，以及 **WARP via WireGuard** 可选的端点
覆盖（IPC `ChainExitSettings` 字段 5/6）。HTTP/SOCKS5 加密对象写入版本 5，新增 `dns_transport`（`auto`、`doh`、`tcp`）；
读取版本 4 只补齐新元数据，不重写凭据、ID 或版本引用。旧客户端拒绝 v5。
修改链 DNS 模式需添加替代配置，共享 DNS 策略变更会重建会话。VPN 对象仍写入
版本 3，明确记录出口来源；
序列化明文最多 192 KiB、密文最多 256 KiB。兼容读取版本 1 和 2，按保存的协议
恢复来源，不改变 ID、版本引用或已保存选择；再次修改时改写为版本 3。
历史较大记录可读取、删除，再次修改超限时保留原对象。旧版缺少认证方式的记录
可管理，但必须重新导入有效配置后才能连接。Windows 删除与选用共用配置事务，
避免并发操作留下失效引用。

---

Cloudflare and WARP are trademarks and/or registered trademarks of Cloudflare, Inc. in the United States and other jurisdictions.
