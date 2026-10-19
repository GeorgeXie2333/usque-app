# Proton VPN 控制面 PoC 与链式代理接入计划

日期：2026-10-19。状态：交接计划，尚未实现或进行 Proton 实网认证。
仓库审查基线：`cf4c4c517649ed2e8f194c8217e0fb7f8714d5a2`。

本文保留最初计划。后续原生实现已按用户选择将公共 API 改为系统路由及适用的
已连接 VPN，不再要求 WARP 控制面；当前合同见[协议参考](PROTON_VPN_PROTOCOL.md)
和[使用指南](PROTON_VPN.md)。

本文供 WSL 侧开发使用。本次只交付计划；不运行联网 PoC，不安装依赖，
不触发 Windows Rust、Flutter、Android 或安装包构建。后续实施前重新检查工作区、
当前源码和上游版本，保留无关改动。

## 1. 目标与判断边界

实现一个独立、轻量、可人工操作的控制面 PoC，回答以下问题：

1. 第三方客户端能否创建 Proton 游客身份并保持可刷新的匿名认证会话？
2. 能否使用用户本地输入的 Proton 账号密码完成 SRP 登录及必要的第二因素验证？
3. 两种身份能否分别登记客户端公钥，取得有效的 WireGuard 相关授权，
   并获得可构造配置的免费节点参数？
4. 如何将已验证的控制流程接入现有 `WARP / MASQUE → WireGuard` 链式出口？

人机验证由用户手动完成，PoC 负责挑战交接、结果回传和恢复原请求。
账号、密码、第二因素验证码和验证 token 均在用户本地交互输入，不经聊天传递。

必须分别报告以下结果，不能相互替代：

| 结果 | 足够的证据 | 不代表什么 |
| --- | --- | --- |
| `auth_ok` | 游客或账号认证完成，取得有效会话并可请求受保护接口 | 尚未取得 VPN 授权 |
| `wg_authorized` | 当前身份和本地密钥完成授权，返回有效证书及其时间字段 | 尚未证明 WireGuard 握手或数据通路可用 |
| `config_ready` | 授权及节点参数足以生成通过本地校验的标准配置 | 尚未连接真实出口 |
| `tunnel_connected` | 独立数据通路实验观察到真实握手和收发 | 本 PoC 默认不执行，也不证明系统 VPN、防泄漏或平台恢复 |

本轮最低交付为游客和账号两条路径的 `wg_authorized` / `config_ready` 实测结果、
未完成项及接入设计。若服务端或验证流程阻塞，保留准确的失败阶段，不能改用
用户手动下载的配置来宣称游客或密码认证已成功。

## 2. WSL 环境与存储约束

- 使用已有的 Linux 原生 checkout；需要新 checkout 时放在 WSL Linux 文件系统。
  不在 `/mnt/c/...` 的 Windows checkout 中编译，不调用 Windows SDK 或可执行文件。
- 优先用独立 Python 3.10+ CLI，避免为控制面实验构建整个 Rust workspace、
  BoringSSL、Flutter 或 Android。Python 不是后续产品必须保留的运行时。
- 只创建一个专用小型 venv，放在 checkout 外或已有忽略目录中。
  依赖固定版本并记录哈希，优先使用已发布 wheel；不要默认全局安装、升级 SDK、
  拉取多架构工具链或启用源码编译依赖。
- SRP/密钥库确定后再安装最小依赖；不为 PoC 引入浏览器自动化、桌面 SDK 或
  官方完整 App 构建环境。需要重量级依赖时先说明原因和替代方案。
- 不运行 `cargo build`、Flutter build、`tool/dev.sh check all` 或聚合构建来验证
  一个 Python 控制面 PoC。若后续确实改动产品代码，仍须完成
  [Contributing](../CONTRIBUTING.md) 对应范围的检查，不能用本计划豁免。
- WSL 虚拟磁盘同样会占用宿主存储。记录新增环境的大致体积，复用已有缓存，
  不复制现有 `target`、Gradle 或 Flutter build 目录，也不自动清理用户缓存。

详细 Linux 工具边界参考 [Linux 与 WSL 开发](LINUX_DEVELOPMENT.md)。
PoC 阶段无需安装该文档列出的完整 GUI/Android 工具链。

## 3. 已确认事实与必须复核的上游协议

官方 Android 支持游客使用免费服务；官方 WireGuard 配置支持标准第三方客户端。
这些事实不能证明任意第三方客户端都能成功调用游客认证接口。

实施时先固定所读上游的 commit 和关键文件路径，记录 API base URL、版本头、
会话头、请求/响应字段、设备 challenge 编码和授权行为。下面的路径来自源码审查，
是复核起点，不能替代当前服务端实测。

| 部分 | 已查到的官方实现 | 实施时重点复核 |
| --- | --- | --- |
| 游客 | `auth/v4/sessions`、`auth/v4/credentialless`；包含 Challenge Payload | 会话创建顺序、设备 frame、游客用户初始化、所需 App/API 版本头 |
| 账号 | `auth/v4/info`、`auth/v4`；SRP 客户端证明及服务器证明校验 | Proton 的密码派生、参数签名/模数检查、编码、支持的 SRP 版本 |
| 第二因素与续期 | `auth/v4/2fa`、`auth/v4/refresh` | TOTP 与其他验证方式、刷新令牌替换、过期与注销 |
| WireGuard 授权 | `vpn/v1/certificate`；Android 使用 `Mode: session` | 密钥格式、会话/计划权限、证书到期与刷新、是否还需其他连接授权步骤 |
| 节点 | 官方服务器目录和连接参数构造 | 免费资格、节点 X25519 公钥、端点、端口、隧道地址、DNS、IPv6 |
| 人机验证 | API 可能返回 `9001`，随后用验证 token/type 重试 | 页面参数、挑战上下文、token 绑定、允许的验证方式、失效和取消 |

关键参考：

- [官方 Android 游客与免费节点说明](https://protonvpn.com/support/best-android-vpn-app)
- [官方 WireGuard 配置说明](https://protonvpn.com/support/wireguard-configurations)
- [AuthenticationApi](https://github.com/ProtonMail/protoncore_android/blob/main/auth/data/src/main/kotlin/me/proton/core/auth/data/api/AuthenticationApi.kt)
- [AuthRepositoryImpl](https://github.com/ProtonMail/protoncore_android/blob/main/auth/data/src/main/kotlin/me/proton/core/auth/data/repository/AuthRepositoryImpl.kt)
- [CreateLoginLessSession](https://github.com/ProtonMail/protoncore_android/blob/main/auth/domain/src/main/kotlin/me/proton/core/auth/domain/usecase/CreateLoginLessSession.kt)
- [PerformLogin](https://github.com/ProtonMail/protoncore_android/blob/main/auth/domain/src/main/kotlin/me/proton/core/auth/domain/usecase/PerformLogin.kt)
- [VpnLogin](https://github.com/ProtonVPN/android-app/blob/master/app/src/main/java/com/protonvpn/android/auth/usecase/VpnLogin.kt)
- [CertificateRepository](https://github.com/ProtonVPN/android-app/blob/master/app/src/main/java/com/protonvpn/android/vpn/CertificateRepository.kt)
- [ProtonApiRetroFit](https://github.com/ProtonVPN/android-app/blob/master/app/src/main/java/com/protonvpn/android/api/ProtonApiRetroFit.kt)
- [ConnectionParamsWireguard](https://github.com/ProtonVPN/android-app/blob/master/app/src/main/java/com/protonvpn/android/models/vpn/ConnectionParamsWireguard.kt)
- [Human verification](https://github.com/ProtonMail/protoncore_android/tree/main/human-verification)
- [官方 go-srp](https://github.com/ProtonMail/go-srp)

许可必须按实际依赖和源码逐项审查。Android/Core 所读文件采用 GPLv3 系列许可，
本项目声明 MIT；不得把参考协议等同于可以无条件复制源码。
`go-srp` 仓库标示 MIT，可作为兼容性参考；本计划不要求引入 Go 运行时。

## 4. PoC 形态与隐私边界

建议新增 `tool/proton_poc/` 独立模块、`tool/test_proton_poc.py`、最小依赖锁文件
和使用说明。具体文件名可随仓库约定调整。先完成 CLI，不改 Flutter、IPC、
服务安装或现有连接行为。

拟定的交互能力如下，当前仓库尚没有这些命令：

- `guest`：创建匿名会话，查询免费资格，登记密钥并请求授权。
- `login`：本地提示输入账号；密码和第二因素用不回显输入，完成同样的授权流程。
- 显式指定直连或代理传输；代理可使用用户已经提供的 HTTP CONNECT 或支持远端
  DNS 的 SOCKS5 入口。使用代理失败时不能静默直连，不能自动启动官方 App 或 VPN。
- 人工验证期间保持原会话；完成后只重试与该挑战绑定的请求。
- 默认仅输出脱敏阶段和结果；生成真实 `.conf` 必须显式请求，写入用户指定的
  私有路径，拒绝覆盖已有文件，不打印到 stdout。

`Mode: session` 授权可能随 PoC 会话撤销而失效。默认导出的配置仅供结构检查，
报告应区分“取得授权时有效”和“清理后仍可连接”。若后续独立隧道实验需要保留
会话，必须显式选择该模式，并先实现受保护存储、到期和最终撤销机制；
不能为保留配置而静默跳过清理。

安全要求：

- TLS 证书、主机名和 SRP 服务器证明必须校验；不通过关闭校验来跑通实验。
- 默认会话与私钥只在内存中保存。交互等待可在同一进程内完成；跨进程恢复如需
  持久化，先实现受保护存储，不能直接落明文 session JSON。
- 私有文件使用 Linux `0600` 权限和受限父目录，不进入 Git、公共报告或聊天。
  密码、token 不放进命令行、URL 查询参数、环境变量示例或 shell history。
- 日志采用允许列表，不记录原始 HTTP 请求/响应、密码、UID、访问/刷新令牌、
  验证 token、设备 challenge、私钥、完整证书或账号身份；异常堆栈也必须脱敏。
- 请求设置连接/响应超时、响应大小限制、取消、有限重试和退避；遵守限流。
  错误密码、人机验证和授权创建不能无限自动重试。
- 成功、失败和 Ctrl-C 均执行适用的清理。退出前尝试撤销本次创建的测试会话，
  撤销失败单独记录，不能伪装已清理；不影响用户的其他会话。

PoC 可显式选择普通 HTTPS 直连来验证控制协议，也可通过已有代理测试。
报告必须注明实际网络路径。普通 HTTPS 成功不证明产品中通过 WARP 的路径成功。

## 5. 实施顺序

### P0：协议合同与离线骨架

1. 固定上游源码版本，确认 API base URL、身份/版本头、设备 challenge 生成方式。
   不臆造官方客户端身份，也不把设备 challenge 自动等同于 Google attestation。
2. 选择可审查且许可合适的 HTTP、SRP、Ed25519/X25519 实现；记录依赖版本与哈希。
   SRP 必须与 Proton 的实际算法一致，不能只采用通用 SRP 默认参数。
3. 实现统一 HTTP adapter、脱敏错误模型、内存会话和可取消的状态机。
4. 使用合成数据及有明确许可的测试向量验证 SRP 证明校验、刷新、限流、
   `9001`、超时、取消、错误正文和私钥不进入日志。

交付：离线测试通过的骨架和固定的协议合同。此阶段没有实网成功结论。

### P1：游客路径

1. 按官方顺序创建匿名会话并调用 credentialless 流程，处理设备 challenge。
2. 识别游客用户及免费权限，确认受保护 API 可用；保留 access/refresh token。
3. 验证一次令牌刷新及旧/新令牌状态处理，不反复创建匿名身份或规避限流。
4. 查询官方目录，保留符合权限且支持目标 WireGuard 协议的候选节点。
   区分“目录中可见”和“当前身份获准连接”；免费节点选择和冷却按服务端规则处理。
5. 进入 P3。设备校验或后端拒绝时返回准确阶段、错误分类和待验证原因。

### P2：账号密码路径

1. 用户在本地输入账号与密码，执行 SRP 认证并校验服务器证明。
2. 需要第二因素时由用户本地完成；优先覆盖常见 TOTP。其他账号/验证模式按实际
   返回实现或明确报告未覆盖，不能跳过第二因素，也不能泛化成所有账号都失败。
3. 任一步骤出现人工验证时转入下述续接流程。
4. 检查账号权限，验证一次 refresh，进入 P3。至少覆盖用户提供的免费账号；
   付费账号属于额外样本，不是本轮必需。
5. 只做有限次数的失败路径检查；错误密码等优先使用离线 mock，避免账号锁定。

人工验证续接：保存待重试请求及其会话、展示官方允许的验证入口，用户完成后
取得 token/type，以 `x-pm-human-verification-token` 和
`x-pm-human-verification-token-type` 重试原请求。具体页面、参数、回调或本地
粘贴方式必须根据固定的官方实现确认，不假设在另一个浏览器登录即可验证原会话。
过期、取消和验证失败必须可恢复或明确结束，不循环弹窗。
若使用浏览器回调，仅绑定 loopback，并校验与当前挑战关联的 state；
不使用公共中转服务器。外部浏览器的网络路径需单独注明。

### P3：WireGuard 授权与参数校验

1. 按官方实现生成并编码客户端密钥。Android 当前使用 Ed25519 密钥及其 X25519
   表示；转换交给已验证的密码库，不简单复用/截断原始字节。
2. 复核并调用证书接口。已查到字段包括 `ClientPublicKey`、
   `ClientPublicKeyMode: EC`、设备名和 `Mode: session`，最终以固定上游合同为准。
3. 校验响应、证书与本地密钥关联、有效期及刷新时间，确认授权属于当前会话。
   证书用于控制面授权，不描述为标准 WireGuard 握手直接传送 X.509 证书。
4. 从节点及官方连接参数取得 peer 公钥、endpoint、端口、地址、DNS、AllowedIPs
   和所需 MTU。不能用猜测的固定内网地址或另一个账号的配置补齐缺失字段。
5. 离线构造并校验配置。检查官方连接流程是否还要求 local-agent 或其他授权
   交换；若尚未确认，明确记录，不能因证书已签发就宣称隧道可用。
6. 对游客和账号分别记录 `auth_ok`、`wg_authorized`、`config_ready`。
   到期/续期先用可控 mock 验证；没有实际到期实测时标为未实测。

到这里停止系统网络操作。真实隧道和最终出口验证是单独实验，优先使用现有
用户态数据通路及 loopback 代理；不运行 `wg-quick`、创建 TUN/Wintun、修改系统
路由/DNS/代理/防火墙，也不安装 VPN 或 APK。

### P4：接入设计与结论

整理两种身份的实际结果、受限原因、依赖体积、许可证和上游版本，给出继续实施、
补充验证或暂缓游客支持的建议。即使游客失败，保留账号路径的独立结论；
手动配置导入仍可作为现有能力，但不能替代本轮认证验收。

## 6. 现有链式代理接入点

以下为后续产品设计，不在控制面 PoC 中直接修改：

| 接入点 | 现有源码 | 后续工作 |
| --- | --- | --- |
| 来源/能力 | [chain_exit/mod.rs](../crates/usque-core/src/chain_exit/mod.rs) | 追加 Proton 来源及能力协商；保留 CONNECT-IP 限制 |
| WireGuard 数据面 | [wireguard.rs](../crates/usque-transport/src/wireguard.rs)、[chain_session.rs](../crates/usque-transport/src/chain_session.rs) | 复用内存会话和现有生命周期，不另建系统 VPN |
| 控制网络 | [internal_network.rs](../crates/usque-transport/src/internal_network.rs) | Proton API 与 DNS 绑定 WARP underlay；失败不回退宿主网络 |
| 后台任务 | [warp_wireguard.rs](../crates/usque-transport/src/warp_wireguard.rs) | 参考其临时/已有 underlay、取消、清理机制，新增独立 Proton manager |
| 加密配置库 | [chain_exit/store.rs](../crates/usque-core/src/chain_exit/store.rs) | 复用配置加密与导入；另建账号 session/刷新/到期模型 |
| Desktop / Android | [Engine 接线](../crates/usque-engine/src/warp_wireguard.rs)、[Android 接线](../crates/usque-android/src/warp_wireguard.rs) | 参考异步任务结构接入共享 Rust 控制面 |
| IPC | [control.proto](../proto/usque/v1/control.proto) | 追加字段和状态；保持既有字段编号，敏感值不进入状态/诊断 |
| Flutter | [来源模型](../apps/usque_gui/lib/models/chain_exit_models.dart)、[链式代理页面](../apps/usque_gui/lib/screens/chain_proxy_screen.dart)、[现有生成面板](../apps/usque_gui/lib/widgets/warp_wireguard_panel.dart) | 使用既有主题/组件增加身份、验证等待、节点与授权状态 |

一个已确认的接口缺口：`InternalNetwork::request_https` 当前返回成功正文，
非 2xx 只保留 HTTP status，丢弃错误正文且不返回响应头。
Proton 验证挑战和限流可能需要错误 JSON 与 `Retry-After` 等信息。正式集成时应增加
受大小限制的结构化响应接口，保留旧调用者合同；敏感正文只供认证逻辑使用，
不进入通用错误、日志或诊断。不能为方便解析而关闭 TLS、响应限制或网络隔离。

产品边界：

- 外层 WARP 身份与 Proton 身份分离。认证、节点和授权续期使用明确的 underlay
  handle，避免依赖尚未建立或已失效的 Proton 最终出口。
- 当前标准 WireGuard 链式出口要求 H3/H2 CONNECT-IP，L4 返回不支持；
  不把 Proton 的 TCP/Stealth 支持混入本轮范围。
- 每次仍只有一个所选链式出口，保持既有 DNS、AllowedIPs、MTU/MSS 和显式直连规则。
- 授权失效、注销、取消、切换账号和旧 generation 返回都要明确处理；
  不使用已撤销密钥，不自动降级为 WARP-only 来掩盖 Proton 失败。
- 账号密码仅用于认证；持久保存所需 token/私钥时使用平台保护，界面只接收必要
  元数据和明确的用户交互挑战。后续 UI 设计遵循
  [GUI README](../apps/usque_gui/README.md)。

## 7. 验证矩阵与交付记录

| 检查 | 本轮实施要求 |
| --- | --- |
| 离线协议测试 | 覆盖 SRP 证明、设备 challenge 构造、密钥转换、证书解析、refresh、错误及取消 |
| 日志/文件隐私 | 用合成敏感值验证 stdout、stderr、异常和报告无泄漏，私有导出权限正确 |
| 游客实网 | 单个受控匿名身份完成认证与授权，记录网络路径、时间、源码版本及分阶段结果 |
| 账号实网 | 用户本地交互完成认证与授权；没有账号或用户未完成验证时标 `not_run`/受阻 |
| 人机验证 | 离线测试始终覆盖；实网没有触发则记录 `not_triggered`，不能宣称真实续接已通过 |
| 实际 WireGuard 隧道 | 本轮默认 `not_run`；证书和配置成功不能替代 |
| 系统 VPN/泄漏/平台恢复 | 本轮 `not_run`；后续按规定隔离环境验证，不以 WSL 标签代替隔离证明 |
| Windows/Flutter/Android 构建 | 仅 Python PoC 不适用；改动对应产品代码后再按贡献指南执行 |

Python 工具若加入 `tool/`，按 [Contributing](../CONTRIBUTING.md) 的 Python tooling
范围运行精确检查；在专用 WSL 环境中安装固定 Ruff，认证依赖另行锁定：

```shell
pip install ruff==0.16.0
ruff check tool
ruff format --check tool
python -m unittest discover -s tool -p "test_*.py" -v
python tool/check_repository_policy.py
git diff --check
```

文档本身只需最后两项。不要为 Markdown 改动触发产品编译。
未来 Rust/Flutter/IPC 改动须重新匹配贡献指南完整矩阵，Linux 结果不能替代
Windows 平台检查；缺少环境的检查明确记为 `not_run`。

交付清单：

1. 独立 PoC、最小固定依赖、离线测试和本地使用说明。
2. 固定上游 commit 的协议合同，包含游客、账号、验证、授权及节点参数来源。
3. 允许列表脱敏报告：测试源码、环境、时间、网络路径、分阶段结果、错误分类、
   是否触发验证、清理结果及未验证项；不提交原始认证材料。
4. 两条身份路径的独立结论和现有链式代理接入建议。

当前交接状态：只有源码/文档调研；游客认证、密码登录、人工验证续接、
WireGuard 授权、配置生成和真实连接均未运行。后续实现者应追加实际记录，
不得把本计划中的预期流程写成已通过结果。

## 8. 后续实施记录

2026-10-19 已新增[独立 CLI](../tool/proton_poc/README.md)和
[固定版本协议合同](PROTON_VPN_PROTOCOL.md)。实现、验证命令、游客请求被拒与
账号未实测的独立结果见[实施与验证记录](PROTON_VPN_POC_VALIDATION.md)。
上文“尚未实现”和“本次只交付计划”保留原交接时点的状态；不能据此忽略后续
记录，也不能将已实现的离线能力写成实网授权成功。
