# WARP via WireGuard / WARP WireGuard 出口

Open **Proxy → Chain proxy → WARP via WireGuard**. This source uses
`Application → MASQUE WARP → WireGuard WARP → Internet` on Windows, Android
and Android TV. It requires CONNECT-IP. Selecting the source does not change
the current connection or automatically switch the data-plane mode. Existing
explicit direct rules still apply to application traffic; registration and
discovery use their dedicated tunnel paths.

打开**代理 → 链式代理 → WARP via WireGuard**。此来源在 Windows、Android 和
Android TV 上使用 MASQUE 外层承载 WARP WireGuard 出口，需要 CONNECT-IP。
浏览此来源不会改变当前连接或自动切换数据平面模式。应用流量仍遵守已有的
显式直连规则；注册和扫描使用专用隧道路径。

## Configuration and endpoint / 配置与端点

Use **Generate WARP configuration** to register a separate free WireGuard
identity, or import/paste an existing WARP WireGuard configuration. Generation
requires an existing MASQUE account and connection settings; registration uses
the MASQUE tunnel. Saving a configuration neither selects it nor connects it.
The private key is encrypted using the same platform storage as imported
configurations. The outer MASQUE identity is not converted or overwritten.

Enable the chain and select a saved configuration. Edit **Endpoint IP** and
**Port**, or select a scan result. An override accepts an IPv4 or IPv6 literal
and a port from 1 through 65535. **Reset** restores the configuration's original
endpoint. Edits remain drafts until **Apply changes** or **Apply and reconnect**.
A disconnected application remains disconnected after saving.

点击**生成 WARP 配置**可注册独立的免费 WireGuard 身份，也可导入或粘贴已有
WARP WireGuard 配置。生成需要已有 MASQUE 账号及连接设置，注册请求经过
MASQUE 隧道。保存配置不会自动选用或连接，私钥使用现有平台加密存储，
不会转换或覆盖外层 MASQUE 身份。

启用链式代理并选择配置后，可编辑 **Endpoint IP** 和**端口**，或选用扫描
结果。IP 支持 IPv4／IPv6 字面地址，端口范围为 1–65535。**重置**恢复原始
配置端点。修改进入草稿，通过**应用更改**或**应用并重新连接**生效；未连接时
应用只保存设置。原始私钥配置不会因修改端点而被重写。

## Scan, pause and resume / 扫描、暂停与继续

**Quick scan** samples five addresses per built-in pool on four primary ports.
**Scan one IP** tests all 54 known WARP ports for the entered address.
**Complete IPv4 pool** tests every IPv4 address/known-port combination and may
take days. IPv6 supports quick and single-address scans, not exhaustive scans.

Discovery uses a separate persistent scan identity and one candidate tunnel at
a time. It reuses the connected MASQUE outer network; when disconnected it
creates a temporary outer session without a TUN, proxy listener or system-proxy
change. It closes that session after the operation. A failed outer connection
never falls back to physical-network probing.

Leaving the page does not cancel a task. **Pause** retains progress for manual
**Resume**. **Cancel** ends the task and retains committed results. Process exit
does not cause automatic scanning on restart. Results and checkpoints are
encrypted; a crash can cause the last uncommitted batch to be tested again.
The history picker lists the latest sixteen operations. Clear-all-data removes
the scan identity, progress and historical results after workers stop.

Changes to the outer account/settings or observed outer address require a new
scan. Previous results remain historical observations. Progress counts attempts,
including failures; result pages contain endpoints with validated tunnel data.

**快速扫描**每个内置地址池抽样五个地址，测试四个常用端口；**指定 IP 深扫**
测试该地址的全部 54 个已知 WARP 端口；**完整 IPv4 地址池**遍历全部地址与
端口组合，可能耗时数天。IPv6 支持抽样及单地址深扫，不提供穷举。

扫描复用独立保存的扫描身份，串行测试候选，不主动中断当前出口。已连接时
复用 MASQUE 外层，未连接时建立临时外层，不创建系统隧道、代理监听或系统
代理设置。结束后清理临时会话；外层失败不会改用物理网络探测。

离开页面后任务继续。**暂停**保留进度，可手动**继续扫描**；**取消**终止任务，
保留已提交的结果。重启不会自动扫描。结果和进度加密保存，进程异常退出后
可能重测最后尚未提交的一批。历史选择器列出最近十六次操作；清除全部数据
会在停止任务后删除扫描身份、进度及历史结果。

外层账号、设置或已观测的外层地址改变时，需要开始新的扫描，旧结果仍作为
历史观测保留。进度包含失败尝试，结果列表仅展示已经验证隧道数据通信的端点。

## Understanding countries / 理解国家信息

Country filters use Cloudflare `/meta` observations, not the endpoint IP's
location or the edge node's country. The trace request supplies the actual exit
IP. IPv4 and IPv6 observations are separate; absent metadata stays unknown.
Response time measures an in-tunnel HTTPS trace request, not host ICMP latency.

The current connection is measured again with the selected user configuration.
Its country can differ from the scan identity's observation. Home retains its
existing ip.sb geolocation and may show a different country. A healthy tunnel
is not disconnected when a country lookup fails. No country lock, automatic
endpoint switching, or guarantee of multiple countries is provided.

国家筛选依据 Cloudflare `/meta` 的观测，不以 endpoint 的 IP 地理位置或接入
节点国家代替；trace 请求提供实际出口 IP。IPv4、IPv6 分别记录，元数据缺失
显示未知。响应耗时测量隧道内 HTTPS trace 请求，不是宿主机 ICMP 延迟。

连接后使用用户选择的配置重新测量，可能与扫描身份的结果不同。首页仍使用
现有 ip.sb 地理归属，显示国家也可能不同。查询失败不会断开健康连接；此功能
不锁定国家、不自动更换端点，也不保证可选多个国家。

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
- Commands are `generate`, `start`, `get`, `pause`, `resume` and `cancel`.
  Network operations return a job ID; `get` takes `job_id`, an attempt-index
  `cursor` and an optional two-letter observed `country`. Pages contain at most
  100 usable endpoints and return `next_cursor`. Results preserve each family
  independently. Private keys and registration tokens never enter these replies.
- The engine owns one worker and a dedicated encrypted scanner identity.
  Foreground navigation does not own that worker. Settings/account mutations
  first request cancellation and confirm cleanup; no network operation holds
  the settings write lock. Resume validates outer settings and the observed
  outer address. A changed runtime path/reconnection ends the current scan.
- Every attempted IP/port, including failures, is saved in encrypted blocks of
  at most 64 results. A partially filled block is reused. The manifest is written
  only after its result blocks and country indexes. Uncommitted rows are hidden
  by the persisted cursor, including after a crash or storage error. The UI reads
  bounded pages and uses encrypted country indexes to skip unrelated blocks.
- Handshake deadline: 3 seconds. Each HTTPS request: 5 seconds. Endpoint probe:
  20 seconds, followed by confirmed cleanup before the next candidate. A valid
  trace response over HTTPS is required for a usable endpoint; handshake success
  alone does not qualify. Metadata failure leaves the country unknown.
- Registration uses a newly generated Curve25519 key and Cloudflare's service
  through the outer MASQUE network. Candidate DNS/HTTPS use the candidate's
  private userspace stack. Both bypass frontend direct rules; neither path has
  an operating-system DNS, TCP or UDP fallback. The connected user's independent
  WireGuard session remains in place while another identity is scanned.

Validation and size measurements: [implementation validation](WARP_WIREGUARD_VALIDATION.md).
