# WARP via WireGuard / WARP WireGuard 出口

Open **Proxy → Chain proxy → WARP via WireGuard**. This source uses
`Application → MASQUE WARP → WireGuard WARP → Internet` on Windows, Android
and Android TV. It requires CONNECT-IP. Selecting the source does not change
the current connection or automatically switch the data-plane mode. Existing
explicit direct rules still apply to application traffic; registration uses its
dedicated tunnel path.

打开**代理 → 链式代理 → WARP via WireGuard**。此来源在 Windows、Android 和
Android TV 上使用 MASQUE 外层承载 WARP WireGuard 出口，需要 CONNECT-IP。
浏览此来源不会改变当前连接或自动切换数据平面模式。应用流量仍遵守已有的
显式直连规则；注册使用专用隧道路径。

## Configuration and endpoint / 配置与端点

Use **Generate WARP configuration** to register a separate free WireGuard
identity, or import/paste an existing WARP WireGuard configuration. Generation
requires an existing MASQUE account and connection settings; registration uses
the MASQUE tunnel. Saving a configuration neither selects it nor connects it.
The private key is encrypted using the same platform storage as imported
configurations. The outer MASQUE identity is not converted or overwritten.

Enable the chain and select a saved configuration. Edit **Endpoint IP** and
**Port**. An override accepts an IPv4 or IPv6 literal
and a port from 1 through 65535. **Reset** restores the configuration's original
endpoint. Edits remain drafts until **Apply changes** or **Apply and reconnect**.
A disconnected application remains disconnected after saving.

点击**生成 WARP 配置**可注册独立的免费 WireGuard 身份，也可导入或粘贴已有
WARP WireGuard 配置。生成需要已有 MASQUE 账号及连接设置，注册请求经过
MASQUE 隧道。保存配置不会自动选用或连接，私钥使用现有平台加密存储，
不会转换或覆盖外层 MASQUE 身份。

启用链式代理并选择配置后，可编辑 **Endpoint IP** 和**端口**。IP 支持 IPv4／IPv6
字面地址，端口范围为 1–65535。**重置**恢复原始
配置端点。修改进入草稿，通过**应用更改**或**应用并重新连接**生效；未连接时
应用只保存设置。原始私钥配置不会因修改端点而被重写。

## Connection retries / 连接重试

After the MASQUE tunnel is established, **WARP via WireGuard** waits up to
3 seconds for the first WireGuard connection attempt. If it fails, Usque retries
up to five times, with respective limits of 4, 5, 5, 5 and 5 seconds. Each failed
session is cleaned up before the next attempt; success stops the retries.
Reconnection after a disconnect starts the same sequence again. Custom
WireGuard exits keep their existing behavior. Cancellation and the overall
connection deadline still apply. Exhausting the attempts stops the chain;
there is no automatic MASQUE-only fallback.

MASQUE 隧道建立后，**WARP via WireGuard** 首次等待 WireGuard 连接最长
3 秒；失败后最多重试 5 次，等待上限依次为 4、5、5、5、5 秒。每次失败的
会话清理完成后才开始下一次，连接成功即停止重试。断线后的重新建连也从
3 秒开始采用同一序列。自定义 WireGuard 出口保留原有行为。主动取消与
整条连接的总截止时间仍然有效；尝试耗尽后停止链式连接，不会自动退化为
仅使用 MASQUE。

## Generation status / 生成状态

Generation reuses the connected MASQUE outer network. When disconnected, it
creates a temporary outer session without a TUN, proxy listener or system-proxy
change, then closes that session. A failed outer connection never falls back
to physical-network registration. The panel shows progress and errors; **Cancel**
stops the operation. Leaving the page does not cancel generation. Status exists
only in the current engine process; restarting clears it. Successfully saved
configurations remain in the encrypted configuration library.

Endpoint scanning, scan history and country filters are no longer available.
Existing configurations and endpoint overrides remain usable. The application
does not load old operation history. Explicit clear-all-data also removes
retired encrypted sidecar files after workers stop.

生成时复用已连接的 MASQUE 外层；未连接时建立临时外层，不创建系统隧道、
代理监听或修改系统代理，结束后清理。外层失败不会改用物理网络注册。
面板显示生成进度和错误，可点击**取消**停止；离开页面后生成任务继续。
任务状态只保存在当前引擎进程内，重启后清空；已成功保存的配置仍保留在
加密配置库中。

端点扫描、扫描历史和国家筛选已移除。已有配置和端点覆盖仍可使用；旧扫描
历史不再读取，显式清除全部数据仍会在停止任务后删除遗留的加密附属文件。

## Registration failures / 注册失败

Generation registers a new WireGuard identity. If it fails, the message
distinguishes missing account credentials, temporary MASQUE
startup failure, and Cloudflare registration failure. A registration code such
as `registration_create_http_403`, `registration_device_timeout` or
`registration_create_tls` identifies the operation and HTTP/transport failure.
Only bounded error codes are shown; tokens, private keys and server response
bodies are excluded. Importing a configuration does not test registration.

生成配置需要注册 WireGuard 身份。失败提示会区分缺少账号凭据、
临时 MASQUE 连接失败和 Cloudflare 注册失败。注册错误码会标明创建账号或读取
设备配置的阶段，以及 HTTP 状态、超时或 TLS 等错误，可用于排查。界面不会显示
访问令牌、私钥或服务端响应正文。能够导入配置并不代表注册服务已经可用。

Technical provenance: [upstream reference](WARP_WIREGUARD_UPSTREAM.md).

## Implementation contract

- Schema 18 adds `chain_exit.endpoint_override`; encrypted configuration records
  use version 3 with an explicit `source`. Version 1/2 readers recover the source
  from the existing protocol without changing profile IDs or revisions.
- `warp_wireguard` is a source, not another transport. A validated WireGuard
  configuration is cloned with its effective endpoint at connection time. The
  original configuration and private key remain unchanged. The effective endpoint
  participates in settings comparison and the running profile reference.
- Control request field 48 / response field 25 carry bounded command/metadata
  JSON. Capability field 38 advertises support; endpoint override fields 5/6 are
  appended to `ChainExitSettings`. Android uses the existing VpnService control
  channel. Clients without the capability cannot select this source.
- Commands are `generate`, `get` and `cancel`. Generation optionally accepts
  `name`; `get` optionally takes `job_id`, and `cancel` requires it. Replies
  contain generation status and the saved profile reference. Scan commands and
  parameters are rejected. Private keys and registration tokens never enter
  these replies. Existing protobuf field numbers remain unchanged.
- The engine owns one generation worker. Settings/account mutations first
  request cancellation and confirm cleanup; no network operation holds the
  settings write lock. Only the current generation status remains in memory;
  there is no operation index, checkpoint store or history reader. Generated
  configurations use the normal encrypted profile library.
- Registration requests have a separate 15-second budget, including up to
  10 seconds for DNS/TCP/TLS setup. A temporary session has no listener credentials
  dependency because it starts no local listener.
- Registration uses a newly generated Curve25519 key and Cloudflare's service
  through the outer MASQUE network. It bypasses frontend direct rules and has
  no operating-system DNS, TCP or UDP fallback. The connected user's independent
  WireGuard session remains in place.
- The former endpoint probe and Cloudflare trace/metadata observation pipeline
  have been removed, including status payloads and UI fields. Home keeps its
  existing ip.sb geolocation.

Historical validation: [implementation validation](WARP_WIREGUARD_VALIDATION.md),
[registration fix](WARP_WIREGUARD_REGISTRATION_FIX.md), and
[former scan validation](WARP_WIREGUARD_SCAN_VALIDATION.md). These records describe
their original candidates, including the removed scanner.
