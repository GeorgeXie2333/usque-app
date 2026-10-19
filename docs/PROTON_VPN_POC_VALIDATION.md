# Proton VPN 控制面 PoC 实施与验证记录

日期：2026-10-19。基线为 `cf4c4c517649ed2e8f194c8217e0fb7f8714d5a2`，
另加当前未提交的 Python PoC 与文档；没有修改产品 Rust、Flutter、IPC 或平台代码。
上游源码快照、许可、字段来源及未知项见[协议合同](PROTON_VPN_PROTOCOL.md)，
操作方式见[CLI 使用说明](../tool/proton_poc/README.md)。

本轮交付了独立实现和离线验证能力，**没有完成计划最低要求中的实网授权与配置
验收**。游客请求被服务端拒绝；用户明确本轮没有可测试账号。两者都不能记为
`wg_authorized` 或 `config_ready` 实网成功。

## 实现范围

| 能力 | 实现与边界 |
| --- | --- |
| 独立 CLI | `python -m tool.proton_poc guest/login`；显式选择普通 HTTPS 直连或已有 HTTP CONNECT/远端 DNS SOCKS5 代理 |
| 游客 | 未认证 session → credentialless → 受保护用户/资格检查；第三方 Linux 身份和 frame 接受性未确认 |
| 账号 | 独立 SRP info/login，不依赖游客 bootstrap；校验固定公钥签名模数、SRP 参数和服务器证明；仅 v4、1–72 UTF-8 字节密码 |
| 第二因素 | 本地不回显 TOTP；仅覆盖 `2FA.Enabled=1`，其他模式明确停止 |
| 刷新与清理 | 一次 refresh，原子替换 token；成功、失败及 Ctrl-C 尝试撤销本次所有已知 session；响应损坏或轮转结果未知单独报告 |
| 人工验证 | 当前挑战绑定原请求与会话，最多一次手动续接；本人在官方页启用监听并拖动拼图，临时 loopback 收到合法成功消息后续接；隐藏粘贴为备用，真实浏览器及实网均未验证 |
| 密钥与证书 | PyNaCl 转换 Ed25519/X25519；校验 TLS 返回的 leaf 与本地公钥及 API/X.509 时间；不独立验证 issuer chain 或 local-agent 接受情况 |
| 免费配置 | 选择 Tier 0 的协议目录候选，端口来自目录或当前 clientconfig；固定上游地址/DNS；候选可见不证明在线或允许连接 |
| 私有导出 | 仅显式指定路径时写出；仓库外、私有父目录、0600、拒绝覆盖与符号链接；会话退出撤销，配置仅作结构检查 |

账号、密码、第二因素、验证 token、UID、session token、设备 frame、私钥、完整
证书及原始 HTTP 正文不进入 stdout、stderr、报告或普通异常文本。CLI 输入会在
`getpass` 无法保持不回显时立即失败。挑战 URL 只在显式交互时写到本机 `/dev/tty`，
不经过报告通道；浏览器自身网络由用户管理。没有明文 session 持久化或保留会话模式。

## 实网记录

游客仅执行一次受控请求：

```shell
/tmp/usque-proton-poc-venv/bin/python -m tool.proton_poc guest --direct
```

| 字段 | 实际观测 |
| --- | --- |
| 请求开始时间 | `2026-10-19T05:33:45.074170+00:00` |
| 网络 | `direct_https`，普通宿主 HTTPS；未使用 WARP 或本地代理 |
| API | `https://vpn-api.proton.me`，`POST /auth/v4/sessions` |
| 客户端 | `external-vpn@0.1.0` / `Usque-Proton-PoC/0.1`；没有改头冒充官方客户端 |
| 阶段 | `anonymous_session` |
| HTTP / API | `422` / `8004` |
| 错误分类 | `api_rejected`；固定上游 ResponseCodes 未定义 8004，具体拒绝原因未确认 |
| 人工验证 | `not_triggered`；没有将 8004 当成 9001 |
| 清理 | `not_needed`：明确拒绝，未取得 session handle；没有把实际撤销记为通过 |
| 进程退出 | `1` |
| 当次 implementation SHA-256 | `e4a93febb920a2381b9f048fe8cc9c0f581257c53da285e1b843377c87ab67ae` |

该 SHA-256 是当次 CLI 报告计算的模块及 lockfile 摘要。之后修正了异常输入、
清理未知状态、账号 bootstrap 和退出码等实现；这次实网记录属于上述历史候选，
不表示后来改动后的实现重新完成了认证。没有重复创建游客身份、重放未知旋转结果，
或尝试通过更换官方身份绕过此次拒绝。未保存原始认证响应、设备 frame 或凭据。

| 验收状态 | 游客 | 账号 |
| --- | --- | --- |
| `auth_ok` | `failed`，未通过匿名 session 创建 | `not_run`，用户明确本轮无可测试账号 |
| `wg_authorized` | `not_run`，未到证书阶段 | `not_run` |
| `config_ready` | `not_run`，没有实网授权参数 | `not_run` |
| refresh 实网 | `not_run` | `not_run` |
| HV 实网续接 | `not_triggered` | `not_run` |
| 证书实际到期/续期 | `not_run` | `not_run` |
| `tunnel_connected` | `not_run` | `not_run` |

这仅说明本实验的具体第三方请求被拒绝，不能推广为游客功能不存在、所有第三方
客户端都不被允许或账号认证失败。独立账号路径仍有待用户在本机终端验证。
手动下载/导入其他配置没有用来替代任何认证结果。

## 离线检查与环境

环境为原生 WSL/Linux x86-64 checkout、CPython 3.14.4。唯一新增 venv 位于
`/tmp/usque-proton-poc-venv`，下载的 wheel 位于 `/tmp/usque-proton-poc-wheels`；
初始占用分别约 65 MiB 和 18 MiB，此外固定源码参考和索引约数 MiB。
没有复制现有构建目录、安装完整官方 App SDK、浏览器自动化或 Android 模拟器。
依赖的精确版本、所有已发布 wheel 哈希和许可见
[依赖记录](../tool/proton_poc/DEPENDENCIES.md)。

下载 artifact 的 SHA-256 已逐一与 PyPI 对应发布元数据核对，并完成了这条
强制重装命令，退出码为 0：

```shell
/tmp/usque-proton-poc-venv/bin/python -m pip install --force-reinstall --require-hashes --only-binary=:all: --no-index --find-links /tmp/usque-proton-poc-wheels -r tool/proton_poc/requirements.lock
```

认证与授权测试使用官方 MIT SRP 向量、合成 session、合成证书和 HTTP mock，
不使用真实账号或 Proton 实网。覆盖证明/签名篡改、frame 编码、限流、9001/9002、响应上限/慢速正文、
禁止压缩膨胀、取消、旋转未知状态、清理失败、输入不回显、敏感数据不进入报告、
目录/证书畸形输入和私有导出。完整游客端到端测试真实执行 Flow、HTTPS adapter、
密码库和导出器，仅替换 HTTP。验证成功消息解析还包含 149 个确定性恶意 JSON 输入。

首轮离线候选的模块及 lockfile SHA-256 为
`a7cb6bb925e405cbdb03e2f8883b04b1fde530dd4f44fc83c800752cb968d31c`，
由 `tool.proton_poc.__main__.implementation_digest()` 计算；它不包含测试文件或文档，
也不是已发布的 Git 提交。该轮 PoC 专项 102 项全部通过，无跳过。

该轮在激活上述专用环境后，完成了贡献指南规定的精确命令：

| 命令 | 结果 |
| --- | --- |
| `pip install ruff==0.16.0` | 退出码 0；已安装精确版本 |
| `ruff check tool` | 退出码 0，`All checks passed!` |
| `ruff format --check tool` | 退出码 0，51 个 Python 文件已符合格式 |
| `python -m unittest discover -s tool -p "test_*.py" -v` | 退出码 0；245 项，244 通过、1 跳过 |
| `python tool/check_repository_policy.py` | 退出码 0，`REPOSITORY_POLICY_OK` |
| `git diff --check` | 退出码 0，无输出 |

唯一跳过为现有 `test_desktop_database_routes_the_zero_trust_callback`，原因是本机
缺少 `desktop-file-utils`。它属于 Linux 包 MIME fixture，未为本轮 PoC 安装额外
系统依赖；该项没有记为通过。完整单元测试内的 Python 包装/发布 fixture 也不代表
执行了 Windows、Android 或真实 Linux 安装与网络生命周期验证。

Rust/Windows/Flutter/Android 编译不适用于本轮 Python 工具范围，未运行。真实
WireGuard 握手、local-agent、系统 VPN、防泄漏、平台恢复及保护 runner 全部
`not_run`；没有运行 TUN/Wintun、`wg-quick` 或修改路由/DNS/防火墙/系统代理。

## 后续：人工拼图接管

用户随后确认，观察对象为 Android 手机/平板官方 App 的游客登录，验证形式是
必须由本人完成的拖动拼图；未提供 App 版本或 HTTP trace。该观察没有改变此前
PoC 收到 `8004` 的实网记录，也不能确定官方在哪个 API 请求上触发挑战。

据此补齐了 `--interactive` 默认的人工浏览器交接：仅当实际收到 `9001` 时暂停
原请求，把官方挑战 URL 和独立监听脚本写到本机私有终端。用户在官方顶层页面
手动启用监听并拖动拼图；脚本只接收成功消息，不操作或解答拼图。临时 IPv4
loopback 端点检查来源、Host、一次性 state 和有界正文，完成后仅续接绑定的原
请求一次。另保留显式 `--verification-handoff paste` 的隐藏粘贴模式。

脚本校验当前挑战、消息 origin/source 和绝对到期时间；监听端具有连接数量、
单连接与总等待时间限制。成功、过期或 Ctrl-C 均关闭本地端口；取消后用户还需
关闭浏览器验证页，以移除仍在另一进程内等待的页面监听。成功消息只在 POST
正文内回传，不进入 callback URL、响应回显或日志。

公开页面脚本及 CSP 的只读核查依据见[协议合同](PROTON_VPN_PROTOCOL.md)。
本地测试使用合成消息、实际 `127.0.0.1` sockets，以及现有 Node `v22.22.1`
执行生成脚本的离线 DOM fixture；没有安装浏览器、npm 包或自动化工具。
沙箱最初禁止本机 bind，相关测试如实失败；之后在获准解除此限制的进程中重跑。
这些检查证明本地协议与脚本逻辑，不证明真实页面可完成跨来源 form 导航。

没有再次请求创建游客身份，也没有取得真实挑战；真人拼图、真实浏览器回传、
服务端接受验证 token、账号登录及后续授权仍未实测。浏览器网络与 CLI 路径独立，
mixed-form、本地网络策略及 Windows/WSL 回环可达性均保留为未验证。

补充实现后的模块及 lockfile SHA-256 为
`8fce68c08386fa465b48959778bc857b51326b5673b8da6e32d1ccb79a222e56`。
11 个 `tool/test_proton_*.py` 按文件名排序、依次对 basename 与原始字节更新的
SHA-256 为 `fe50a39b38e7fc59391449d905d9610846e04ba03f74010cad30d6dfd9f8f9ea`。
这两个摘要覆盖 PoC 实现、依赖锁和专项测试，不覆盖文档及整个未提交工作树；
不是 Git 提交或实网认证候选已通过的证明。

在相同专用环境中重新完成贡献指南的精确检查：

| 命令 | 本次结果 |
| --- | --- |
| `pip install ruff==0.16.0` | 退出码 0，版本符合要求 |
| `ruff check tool` | 退出码 0，`All checks passed!` |
| `ruff format --check tool` | 退出码 0，54 个文件符合格式 |
| `python -m unittest discover -s tool -p "test_*.py" -v` | 退出码 0；265 项，264 通过、1 跳过 |
| `python tool/check_repository_policy.py` | 退出码 0，`REPOSITORY_POLICY_OK` |
| `git diff --check` | 退出码 0，无输出 |

其中 PoC 专项 122 项全部通过，包含 10 项实际合成 loopback 测试、7 项 Node
脚本语义测试，以及游客两个可能挑战阶段的成功续接、取消和非交互失败路径。
跳过项仍为上述缺少 `desktop-file-utils` 的既有 MIME fixture，未记为通过。

## 接入建议

保留控制面 PoC 为独立工具，暂不将 Proton 标记为可用产品来源。下一步先确认
第三方客户端身份、Linux challenge 的可接受合同和真实人工验证交接，再在用户
本地完成免费账号样本；游客和账号分别继续，不能相互替代。session 模式还需独立
验证隧道内 local-agent，标准配置文本齐全不足以交付最终出口。

对照当前源码核查后，后续产品集成可按以下顺序推进：

1. 为 [InternalNetwork](../crates/usque-transport/src/internal_network.rs) 增加受限的
   结构化 HTTPS 响应，保留 status、必要 headers 和有上限的成功/失败正文。
   旧 `request_https` 只返回成功字节，非 2xx 只有状态，无法承载 HV 或限流合同。
   保留 TLS、主机名、取消、大小限制和网络隔离。
2. 新增独立 Proton manager，参考
   [WARP manager](../crates/usque-transport/src/warp_wireguard.rs) 的 job、generation、
   单任务、取消和 `Outer.close`。控制面显式使用 WARP underlay handle，
   不使用尚未建立的 Proton 最终出口，失败不回退宿主网络。
3. 在 [ChainSource](../crates/usque-core/src/chain_exit/mod.rs) 追加来源，输出经校验的
   [WireGuardProfile](../crates/usque-core/src/chain_exit/wireguard.rs)，复用
   [chain_session](../crates/usque-transport/src/chain_session.rs) 数据面。
   现有 WireGuard 要求 UDP/CONNECT-IP，继续拒绝 L4Proxy，保留 DNS/AllowedIPs/
   MTU/MSS 与显式直连规则。
4. 使用 [ProfileCipher/ChainProfileStore](../crates/usque-core/src/chain_exit/store.rs)
   的平台加密和原子存储，另建 session、Ed25519 密钥、证书及到期/撤销记录。
   账号 token 不放进普通 `.conf` 或通用 metadata。认证与旧 generation 返回
   必须隔离；不使用已撤销密钥，也不通过降级 WARP-only 掩盖失败。
5. 在用户态 WireGuard 之后实现并验证 local-agent，再设计追加 IPC 字段与既有
   GUI 组件交互；敏感值不进入状态/诊断，字段编号保持 append-only。
   Desktop/Android 接线可参考既有
   [Engine](../crates/usque-engine/src/warp_wireguard.rs) 和
   [Android](../crates/usque-android/src/warp_wireguard.rs) 任务入口。

这些是后续设计，不是本轮已实现的产品接入或已完成的隧道验证。
