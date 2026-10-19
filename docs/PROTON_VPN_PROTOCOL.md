# Proton VPN 协议参考

审查日期：2026-10-19。本合同记录固定官方源码中的网络格式及本实验的
实现边界；源码审查不代表第三方客户端已通过服务端认证。参见
[PoC 使用说明](../tool/proton_poc/README.md)和
[验证结果](PROTON_VPN_POC_VALIDATION.md)。下文明确标为 PoC 的行为属于独立
Python 实验；当前 Rust/Flutter 产品实现另见本文末尾的原生实现章节、
[使用指南](PROTON_VPN.md)与[原生验证记录](PROTON_VPN_NATIVE_VALIDATION.md)。

## 固定上游与许可

| 上游 | 固定提交 | 用途 |
| --- | --- | --- |
| Proton VPN Android | [`316a3d499e120bd2cab63c2cd8ac396e3ab3b24a`](https://github.com/ProtonVPN/android-app/commit/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a) | VPN 端点、密钥表示、目录、连接参数及 local-agent |
| Proton Core Android | [`1b87f94ebfdfaf5e67145e8668efc52dbb931e0b`](https://github.com/ProtonMail/protoncore_android/commit/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b) | 游客、SRP 认证请求、会话、设备 frame、人机验证 |
| Proton go-srp | [`d323688777afbb6265d8bc4a51419991a24630ca`](https://github.com/ProtonMail/go-srp/commit/d323688777afbb6265d8bc4a51419991a24630ca) | SRP 数学、签名模数与兼容性向量 |

上述 Android/Core 被读取的文件声明 GPL-3.0-or-later。这里记录互操作字段和
行为，没有移植其 Kotlin 实现或捆绑官方 SDK。go-srp 的 MIT 声明与适用的
来源说明保留在 PoC SRP 文件和测试向量旁。独立 Python 实现所依赖 wheel 的
版本、哈希、许可和体积见
[依赖记录](../tool/proton_poc/DEPENDENCIES.md)。MIT 项目许可不会覆盖上游许可。
这三个固定版本是分别审查的源码快照，不声称 Android 构建实际链接了所列
Core 或 go-srp 提交。

## 主机、身份与 HTTP 合同

生产 API 基址为 `https://vpn-api.proton.me`。Android 的 production flavor
设置 `apiPrefix = vpn-api`，Core 的默认 host 为 `proton.me`；由此组合基址。
请求相对路径从 `/auth/`、`/core/`、`/vpn/` 开始，无额外 `/api` 前缀。
此结论还核对了实际注入链：Android 的 `@BaseProtonApiUrl` 直接使用
`environmentConfiguration.baseUrl.toHttpUrl()`，CoreNetworkModule 与
ApiManagerFactory 原样传递，ProtonApiBackend 仅补基址末尾的 `/`，没有追加 `/api`。
依据：[Android 环境配置](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/build.gradle)、
[Core 环境默认值](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/plugins/environment-configuration/src/main/kotlin/configuration/EnvironmentConfigSettings.kt)、
[Android API URL 注入](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/di/AppModule.kt)、
[Core API 工厂](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/network/data/src/main/kotlin/me/proton/core/network/data/ApiManagerFactory.kt)。

Core 的请求头合同如下：

| 头 | 值与约束 |
| --- | --- |
| `Accept` | `application/vnd.protonmail.v1+json` |
| `Content-Type` | 有 JSON 正文时 `application/json` |
| `x-pm-appversion` | 客户端 ID 与自身版本；PoC 使用独立的 Usque ID，不复制官方 App ID/版本 |
| `User-Agent` | 真实第三方客户端名称及版本，不声称运行 Android 官方 App |
| `x-pm-locale` | 本地应用语言 |
| `x-pm-uid` | 当前本次实验创建的会话 UID，仅保留内存 |
| `Authorization` | 当前会话 `Bearer <AccessToken>` |
| `x-pm-human-verification-token-type` | 当前挑战成功返回的验证类型 |
| `x-pm-human-verification-token` | 同一次挑战成功返回的 token |

依据：[Core 请求头构造](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/network/data/src/main/kotlin/me/proton/core/network/data/ProtonApiBackend.kt)、
[Android 客户端身份构造](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/api/VpnApiClient.kt)。

上表区分官方合同与 PoC：当前 PoC 使用独立客户端身份，发送 JSON Content-Type
及 `Accept-Encoding: identity`，尚未发送官方 Core 的 `x-pm-locale`。不能把表格理解为
所有官方头或平台行为都已由实验实现或验证。

源码没有证明服务端允许任意第三方 App ID 或平台 frame。若返回版本或平台拒绝，
实验应记录拒绝，不改头冒充官方客户端。TLS 和主机名必须校验；显式代理失败
不能自动切换直连。非 2xx 错误 JSON 和 `Retry-After` 仅在受大小限制的解析器中
处理，不输出原正文。`Code=1000` 是普通成功；`5003/5005` 是版本错误；
`9001` 是人工验证；`9002` 是独立的设备验证流程，不能假称普通 captcha 可解决。
依据：[响应码](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/network/domain/src/main/kotlin/me/proton/core/network/domain/ResponseCodes.kt)。

当前固定 Core 的响应码表没有 `8004` 定义。若实网返回 HTTP 422 / Code 8004，
只能保留该阶段、状态和未知拒绝分类；不能仅凭这个数字归因为密码、游客资格、
设备 frame、版本身份或 URL 路径问题。

## 会话、游客与账号

[AuthenticationApi](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/data/src/main/kotlin/me/proton/core/auth/data/api/AuthenticationApi.kt)
定义以下方法；字段来自同目录 request/response 文件及
[AuthRepositoryImpl](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/data/src/main/kotlin/me/proton/core/auth/data/repository/AuthRepositoryImpl.kt)。

| 请求 | JSON 正文 | 关键响应/后续 |
| --- | --- | --- |
| `POST /auth/v4/sessions` | `Payload` 设备 frame map | `UID, AccessToken, RefreshToken, TokenType, Scopes`；这是未认证会话 |
| `POST /auth/v4/credentialless` | `Payload` frame map | 登录响应增加 `UserID, EventID, LocalID` 等；随后读取用户与 VPN 权限 |
| `POST /auth/v4/info` | `Username, Intent`；强制 SRP 可取 `Intent: Proton` | `Version, Modulus, ServerEphemeral, Salt, SRPSession` 等 |
| `POST /auth/v4` | `Username, ClientEphemeral, ClientProof, SRPSession, Payload` | 登录会话及 `ServerProof`；验证服务器证明后才信任登录 |
| `POST /auth/v4/2fa` | TOTP 使用 `TwoFactorCode` | 已登录会话的权限升级，响应含 `Scopes` |
| `POST /auth/v4/refresh` | 见下文 | 替换整个当前 token 组合，然后请求受保护接口 |
| `DELETE /auth/v4` | 无正文 | 撤销当前会话；不能影响用户其他会话 |

游客先建立未认证会话，再在此上下文调用 credentialless；仅取得匿名 access token
不能报告 `auth_ok`。Core 的
[CreateLoginLessSession](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/domain/src/main/kotlin/me/proton/core/auth/domain/usecase/CreateLoginLessSession.kt)
把 credentialless 登录响应建立为带 UserID 的认证会话，并注明后续还有账号初始化。
PoC 用受保护的用户和 `/vpn/v2` 响应检查用户及权限，不能把服务器目录可见当作授权。

账号路径应保持独立：官方 `PerformLogin` 直接以 `sessionId=null` 获取 SRP info
并提交登录，不显式创建匿名会话；`SessionProviderImpl.getSession(null)` 只尝试
读取已有未认证会话，没有自动创建操作。创建未认证会话是 SessionManager 的
独立 `requestSession()` 方法。因此没有源码依据要求每次密码登录先成功调用
`/auth/v4/sessions`，游客该步骤的未知拒绝不能代替账号登录结果。即使直接
SRP 登录也仍须使用真实第三方身份和设备 frame，并由服务端实际判定是否接受。
依据：[PerformLogin](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/domain/src/main/kotlin/me/proton/core/auth/domain/usecase/PerformLogin.kt)、
[SessionProviderImpl](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/account-manager/data/src/main/kotlin/me/proton/core/accountmanager/data/SessionProviderImpl.kt)、
[SessionManagerImpl](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/account-manager/data/src/main/kotlin/me/proton/core/accountmanager/data/SessionManagerImpl.kt)。

刷新正文固定为 `UID`、`RefreshToken`、`ResponseType: token`、
`GrantType: refresh_token`、`RedirectURI: http://protonmail.ch`。该 URI 是协议中的
请求字段，PoC 不向它导航，也不通过它发送凭据。依据：
[RefreshSessionRequest](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/data/src/main/kotlin/me/proton/core/auth/data/api/request/RefreshSessionRequest.kt)。
刷新成功后保留新 UID/token，旧组合不再用于认证。网络超时后刷新是否生效可能
未知，不能用无限重试掩盖旋转语义；清理失败必须独立报告。

Android 的 logout 可以不带 `AuthDevice` 参数；Core 另提供 `AuthDevice=0/1`
选择是否撤销认证设备。PoC 使用前者，不删除其他设备。Core 的撤销实现会忽略错误，
PoC 必须更严格地保留真实清理结果。

`2FA.Enabled` 是位标志，低位表示 TOTP，第二位表示安全密钥/认证器。当前只实现
`Enabled=1` 的 TOTP；`Enabled=3` 即使同时具备 TOTP，也尚未覆盖。其他模式
明确受限，不能绕过。密码模式、SSO、恢复状态、设备迁移、
FIDO2 或 `9002` 也不能由普通 SRP 成功推断完成。依据：
[SecondFactorInfoResponse](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/data/src/main/kotlin/me/proton/core/auth/data/api/response/SecondFactorInfoResponse.kt)。

SRP 必须验证签名模数、合法参数及服务器证明，使用 Proton 的版本化密码派生和
小端整数格式。本 PoC 的 v4 支持与测试范围见使用说明；通用 SRP 的默认参数
不能替代该合同。未覆盖的版本应停止并分类报告。

## 设备 challenge 的实际格式

设备 frame 是普通 JSON 对象，不是额外 Base64 blob，也不是 Google Play
attestation。官方 VPN 的 map key 为 `vpn-android-v4-challenge-0`，`v` 为
`2.0.7`。字段如下：

| 字段 | 官方 Android 意义 |
| --- | --- |
| `appLang`、`regionCode` | 应用语言、系统 locale 的国家代码 |
| `timezone` | 系统时区 ID |
| `timezoneOffset` | UTC offset 与 DST 的合计分钟数取负，东京为 `-540` |
| `deviceName` | 设备描述的整数 rolling hash，见下文 |
| `isJailbreak` | 官方 Android root 探测结果 |
| `preferredContentSize` | 字符串形式字体缩放值 |
| `storageCapacity` | 设备存储容量的十进制 GB 数值 |
| `isDarkmodeOn` | 是否使用深色主题 |
| `keyboards` | 已启用输入法 package 名称列表 |

官方设备描述是 `model/brand device`，以 UTF-8 编码。每个字节按有符号 8-bit
解释；权重从 1 开始，每步乘 7 后模 100000007；累加有符号字节乘权重，最终
累加和不再取模。不能把所有字节解释为无符号，也不能对最终和取模。
账号交互若捕获了 Username frame，还会包含 `frame.name`、`timeUsername`、
`clickUsername`、`copyUsername`、`pasteUsername`、`keydownUsername`；空 frames
时官方使用 Device frame。PoC 不编造输入活动或复制用户输入内容。
依据：[frame 类型](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/challenge/data/src/main/kotlin/me/proton/core/challenge/data/frame/ChallengeFrame.kt)、
[frame 版本和前缀](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/challenge/domain/src/main/kotlin/me/proton/core/challenge/domain/ChallengeUtils.kt)、
[设备字段编码](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/util/android/device/src/main/kotlin/me/proton/core/util/android/device/DeviceUtils.kt)。

本实验运行于 Linux，明确采用 `usque-linux-v4-challenge-0`，不冒充官方 Android
frame。`deviceName` 仅从真实系统类型和架构的 `Linux/usque <machine>` 描述生成，
不读取 hostname、序列号或硬件 ID。语言、时区、时区 offset、根文件系统容量及
当前有效 UID 是否为 root 来自本机；CLI 没有 Android 的字体、输入法和主题，
对应发送空字体值、空输入法数组及 false 主题默认值。这些属于明确的 Linux
适配选择，服务端接受性未知，不应描述为官方支持。frame 不进入日志或报告。
实现与合成编码测试分别为
[challenge.py](../tool/proton_poc/challenge.py)、
[test_proton_challenge.py](../tool/test_proton_challenge.py)。

另一个尚未覆盖的编码差异来自对 Kotlin 类型和默认 Json 行为的推断，未捕获
官方 App 的 HTTP 请求，也没有查到对应的官方序列化 fixture，不能称为实测
wire 合同：`RequestSessionRequest`
的 map 值类型为具体 `ChallengeFrame.Device`，而 `LoginLessRequest` 和
`LoginRequest` 为 sealed `ChallengeFrame`。Core 的默认 Json 没有重设
`classDiscriminator`，因此按该默认序列化器，后两者还会带类的 `type` 标记
（Device 默认 serial name 为 `me.proton.core.challenge.data.frame.ChallengeFrame.Device`）。
该推断还未排除应用初始化后覆盖可变 `defaultJsonStringFormat` 的可能。
当前第三方 Linux frame 没有复制官方 Android 类名，也不生成虚构的第三方类标记。
不能据此宣称 credentialless/login 的 frame 完整复现官方 App 编码，亦不能把
这个差异认定为某个实网错误的原因；服务端对该适配的支持仍未知。依据：
[RequestSessionRequest](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/data/src/main/kotlin/me/proton/core/auth/data/api/request/RequestSessionRequest.kt)、
[LoginLessRequest](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/data/src/main/kotlin/me/proton/core/auth/data/api/request/LoginLessRequest.kt)、
[ProtonCoreConfig](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/util/kotlin/src/main/kotlin/me/proton/core/util/kotlin/ProtonCoreConfig.kt)。

## 人工验证交接

游客模式也可能出现人机验证。用户已观察到 Android 手机/平板官方 App 的游客
登录展示拼图拖动验证，并明确要求本人接管；这属于用户提供的行为观察，未记录
App 版本、HTTP trace 或具体触发请求。
固定源码的游客认证没有独立的“先获取 captcha”调用，`requestSession` 和
`performLoginLess` 均经过统一 API manager 的人机验证 handler。挑战属于实际返回
`9001` 的调用；仅从客户端源码不能确定服务端一定在哪一步触发。官方 App 的
成功验证不能代替本 PoC 自身会话的验证，也不能将另一次 `8004` 解释为 `9001`。

`9001` 响应中的 methods 与 start token 必须关联原请求及原 client/session。
Core 保留这一上下文，等待 success、failed 或 cancelled 状态，成功后重新执行
原请求。完成验证只代表可以尝试恢复该请求，不能代替服务器的最终响应。
依据：[HumanVerificationNeededHandler](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/network/domain/src/main/kotlin/me/proton/core/network/domain/handlers/HumanVerificationNeededHandler.kt)。

生产 HV3 页面为 `https://verify.proton.me`。官方构造的查询参数是
`embed=true`、`token=<start token>`、`methods=<逗号连接的服务端允许方法>`、
`theme=1`（深色）或 `theme=2`（浅色），VPN 主题另加 `vpn=true`。可选
`locale/defaultCountry/defaultEmail/defaultPhone` 无需为实验提供。字段均作
UTF-8 URL 编码。该 start token 是服务器提供的挑战上下文，不是最终成功 token；
二者均不得出现在 stdout、通用日志、报告或 shell 命令中。
依据：[HV3 URL 与 JS 回调](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/human-verification/presentation/src/main/kotlin/me/proton/core/humanverification/presentation/ui/hv3/HV3DialogFragment.kt)。

官方 Android 页面的成功消息通过 `AndroidInterface.dispatch` 回到应用，格式为
`type: HUMAN_VERIFICATION_SUCCESS` 与 `payload.token`、`payload.type`。
独立外部浏览器没有这一 Android JS interface。源码没有证明该页面提供通用的
复制 token 按钮、任意 redirect URI 或浏览器登录后的跨会话验证功能，不能臆造。
依据：[HV3ResponseMessage](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/human-verification/presentation/src/main/kotlin/me/proton/core/humanverification/presentation/ui/hv3/HV3ResponseMessage.kt)。

后续还只读检查了官方公开网页，而非真实挑战：2026-10-19 采集的
[页面脚本](https://verify.proton.me/assets/static/index.99687ed4.js) SHA-256 为
`6984f7c6b16509a36440fe70433783cc637fcdd1a6a25fe1f9c5a9e9eb53c67e`，
下载字节的 SHA-384 与当时 HTML 的 integrity 属性一致。bundle 自述版本
`5.0.11.0`、commit `8ebb265df466d2e14ee880a173c85857a9aa947a`，
这不是 Android App 的版本，也不是独立签名证明。其普通网页分支把 JSON 字符串
消息发给 `window.parent`；顶层标签页中 parent 为自身。因此可以由用户在该页手动
启用一次监听，在本人完成拼图后取得 success 消息，监听不操作或解答拼图。
未复制该 bundle 实现到仓库；临时桥接监听为独立实现。

当时公开页面的 CSP 不允许 loopback 父页面嵌入，`connect-src` 也不允许向本地
fetch，因此 PoC 使用官方页顶层标签和原生 form POST 的临时回调。接收端只绑定
`127.0.0.1`，严格检查 Host、Origin、一次性 state、路径和有界正文；成功 token
仅进 POST body，不进 callback URL、页面回显或日志。监听脚本先比较当前 URL 的
challenge token 哈希，并校验消息 origin/source，再等待官方 success；不自动解题。
异步哈希完成后与提交前再次确认当前页仍属于同一挑战，页面切换时停止。
CLI 取消只关闭本地端点，不能主动移除另一进程的页面监听；用户同时关闭验证
标签页，否则监听到脚本截止时间才失效。

网页默认 referrer policy 可能在 HTTPS→HTTP 时把 Origin 变成 null。监听脚本在
提交前显式设置 `meta referrer=origin`，只保留站点 origin，不带验证查询；接收端仍
拒绝 null Origin。依据：[Fetch Origin 算法](https://fetch.spec.whatwg.org/#append-a-request-origin-header)、
[HTML referrer meta](https://html.spec.whatwg.org/multipage/semantics.html#meta-referrer)。
这不表示实际浏览器已验证。CSP 是否允许 form 导航还不能代替 mixed-form、
本地网络策略或 WSL 回环可达性测试；浏览器可能提示或阻断。依据：
[CSP form-action](https://www.w3.org/TR/CSP3/#directive-form-action)、
[Mixed Content form submission](https://www.w3.org/TR/mixed-content/#form-submission)。

PoC 的本地手动交接必须保持 token 不回显、绑定当前挑战，只接受服务端允许的
方法，并有限次重试。若采用浏览器桥接，必须仅绑定 loopback、校验 origin、state
及消息来源，不能公开中转；若该桥接没有实测，则报告未验证。浏览器网络路径需
单独说明。无法安全交接时准确结束于人工验证阶段，不重复创建身份或绕过验证。
支付验证不在实验范围内。验证过期、取消和失败均进入适用的清理。

## VPN 授权和节点参数

[ProtonVPNRetrofit](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/api/ProtonVPNRetrofit.kt)
提供 `/vpn/v2`、`/vpn/v2/clientconfig`、`/vpn/v2/logicals` 和
`POST /vpn/v1/certificate`。VPNInfo 的 `Status`、`MaxTier`、`PlanName`、
`MaxConnect` 等反映当前身份资格；目录可见不代表可连接。目录可带
`WithEntriesForProtocols=WireGuardUDP` 和 `WithState=true`。

证书请求为 Ed25519 SPKI PEM 格式 `ClientPublicKey`，
`ClientPublicKeyMode: EC`、诚实的第三方 `DeviceName`、`Mode: session`、
`Features: []`；不需要 Duration 时省略。官方密钥库对同一 Ed25519 密钥作
经过验证的 X25519 转换，用其结果作为 WireGuard 私钥，不能截断或复用原始字节。
响应含 `Certificate` PEM、`ExpirationTime` 和 `RefreshTime`（Unix 秒）。依据：
[CertificateRepository](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/vpn/CertificateRepository.kt)、
[请求模型](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/models/vpn/CertificateRequestBody.kt)、
[响应模型](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/models/vpn/CertificateResponse.kt)。

PoC 检查证书内公钥、证书与 API 时间的一致性以及有效期；授权证据来自经 TLS
校验的当前会话接口响应。当前没有独立验证叶证书的 issuer chain，响应也没有
session ID 字段可供本地证明绑定。不能将公钥一致或 X.509 可解析描述为独立的
完整证书链认证。session 模式的授权可能随注销失效。

节点 peer 公钥来自物理服务器 `X25519PublicKey`。官方地址选择可回退到 `EntryIP`，
当前 PoC 更严格地只接受 `EntryPerProtocol.WireGuardUDP.IPv4`，不实现 `EntryIP`
回退，也不从无协议信息的地址推断 UDP 支持。端口优先读取该 entry 的 `Ports`，
缺省时才采用服务端 clientconfig 的 `DefaultPorts.WireGuard.UDP`，缺失时拒绝
猜测端口。
免费候选使用 Tier 0、排除 Secure Core/受限节点、要求物理节点在线标志和有效
peer 公钥。新版 `StatusReference`/二进制状态未实现时不能称节点已实测在线。
依据：[ConnectingDomain](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/servers/api/ConnectingDomain.kt)、
[LogicalServer](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/servers/api/LogicalServer.kt)。

固定官方连接合同提供 IPv4 隧道地址 `10.2.0.2/32`、DNS `10.2.0.1`；
这些有源码出处，非账号配置拼接。官方还定义 IPv6 地址及 DNS，本 PoC 默认不
启用 IPv6。全流量 AllowedIPs 为 `0.0.0.0/0, ::/0`，将 IPv6 默认前缀也包含在
peer 选择中；这不证明宿主路由已经配置或系统防泄漏成立。keepalive 为 60 秒。
连接参数未指定 MTU 时省略，不猜固定 MTU。
依据：[Constants](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/utils/Constants.kt)、
[ConnectionParamsWireguard](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/models/vpn/ConnectionParamsWireguard.kt)。

官方正常会话在 WireGuard 连接后还有隧道内 local-agent 交换：证书、Ed25519
私钥、VPN root certificates、local-agent 地址和节点 hostname 参与建立 agent
连接，正常最终 Connected 状态依赖 agent 状态。证书不是标准 WireGuard 握手中
直接传输的 X.509。PoC 未执行 local-agent、真实握手或数据收发，`config_ready`
仅表示结构参数齐全并通过离线校验，不表示导出文件实际可用。依据：
[VpnBackend](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/vpn/VpnBackend.kt)。

## 验证边界

编码、SRP、密钥转换、证书解析、配置及错误恢复可以离线验证。游客实网、账号
实网、真实 HV 续接、旧 token 失效、证书实际到期及 local-agent 必须分别记录。
本合同本身没有这些实网成功证据。`auth_ok`、`wg_authorized`、`config_ready`
与 `tunnel_connected` 不互相替代；会话撤销、代理出口及未执行的隔离验证都必须
如实保留在结果中。后续 Rust 控制面应显式绑定既有 WARP underlay，不得由
普通 HTTPS 直连实验推断产品链式出口或系统防泄漏已通过。

## 原生实现、local-agent 与持续授权

当前产品增加 `proton_vpn` 来源，排列在 WARP 后面。控制面使用 Rust；Flutter
只展示任务、节点和允许公开的状态，通过独立敏感字节字段传递短期凭据与验证码
交接。Windows、Linux、Android 的原生窗口仅用于官方人工验证；用户完成拼图，
程序不解题。验证码 WebView 使用系统网络，不继承 Rust WARP 的控制面句柄；
两者出口一致性与服务器是否接受这一组合均尚未实测。

原生认证 API、目录、证书请求只通过明确绑定的 WARP underlay。使用独立
`external-vpn@0.1.0` 身份和 `usque-<实际系统>-v4-challenge-0` frame，设备
描述只含实际系统和架构；不复制 Android 应用身份或编造交互、硬件及 attestation。
服务端对这个第三方适配的接受性没有得到证明。支持 SRP v4、单独 TOTP、真实
9001 触发的一次人工验证重试；9002 和未知 8004 不进入普通拼图流程。

| 原生状态边界 | 实现约束 |
| --- | --- |
| 配置准备 | 验证用户/VPN 权限，执行一次会话刷新再复核身份，选择免费候选并校验返回证书；保存会话后不撤销该会话，否则 session 模式出口会失效 |
| 持久化 | 账号/seed/证书放入平台加密 sidecar；密码不落盘；轮转请求前持久化 pending 标记，超时或身份未知不重放旧刷新令牌 |
| 开始连接 | 在同一 WARP underlay 上检查并按需刷新账号及证书，再启动内存 WireGuard/agent 栈；没有宿主 TCP 或 DNS 回退 |
| 最终授权 | WireGuard 握手后通过隧道内 TCP `10.2.0.1:65432` 建立 mTLS；校验节点 SNI、VPN 专用 CA、客户端 Ed25519 密钥/证书和 agent 状态及功能确认，之后才放行业务 |
| 持续授权 | 定期检查 agent 状态，在到期前刷新同一账号/密钥证书；过期、撤权、侧车变更或无法确认的授权关闭业务；取消时等待所持有任务完成 |
| 退出与本机遗忘 | 联网退出只尝试撤销本机拥有的会话；显式“仅忘记本机”只删除本地记录，不宣称远端已撤销；两者都保护正在使用或已保存选择的配置 |

额外只读协议参考固定如下。实现为独立 Rust 内存 TCP/TLS/framing/state
代码，没有移植或链接这两个 GPL 项目的实现。

| 上游 | 固定提交 | 读取内容 |
| --- | --- | --- |
| Proton local-agent-rs | [`74ec6f4f093805d766f11f9ef522bf1120591058`](https://github.com/ProtonVPN/local-agent-rs/commit/74ec6f4f093805d766f11f9ef522bf1120591058) | 隧道内 TCP、双向 TLS、4 字节大端长度与 JSON 控制消息、状态/功能语义 |
| Proton go-vpn-lib | [`9440653831b00dbc4711f59e8a23a06af1999405`](https://github.com/ProtonVPN/go-vpn-lib/commit/9440653831b00dbc4711f59e8a23a06af1999405) | local-agent 信任材料与证书连接合同 |

原生模块内的公开 VPN CA 只用于 agent 服务器认证，不加入操作系统信任库。
Root DER SHA-256 为
`474792c86ca0e72f6c04ea87cbfd499da38c58f4466e7b258cdf67da34656e31`，
兼容中间证书 DER SHA-256 为
`a3705cd0790c8a665ae7e8e4269974c2ee318aed03cf0c7e860c2228cf17e4ab`。
控制帧最大 32 KiB，业务准入还预留 agent 地址，防止普通流量及 IPv4 分片借用
这个授权前通道。合成双端内存栈/mTLS 测试不证明真实服务端互操作。

原生证书解析接受最多 48 KiB、1–8 张不同 DER 的 PEM 链，验证叶公钥与时间。
签发来源依赖经过验证的 API TLS，未独立建立客户端证书 issuer 链认证。
原生 SRP 的 MIT 来源声明随 Rust 源码、测试向量和应用许可资产保留。
所有真实认证、人工拼图、WireGuard/agent 互操作和最终出口验收结果，必须在
原生记录中单独列出；源码接线或离线成功不能替代这些实网结果。


## Native captcha bridge / 原生拼图桥接

补充审查日期：2026-10-19，依据仍为上文已标识 SHA-256 的公开
`index.99687ed4.js` 本地快照，未执行认证或真实拼图。此节描述后续原生桥接，
不改变历史 Python PoC 的实网结果。

该快照存在一个影响 Windows 的平台分发差异：`rQ`（字节偏移约 729171）在
存在 `window.chrome.webview` 时选择 `webview`；`a1`（约 803549）的该分支直接
结束，不调用 `window.parent.postMessage`。仅等待顶层 success event 因而不能
接通此 WebView2 页面。禁用 WebMessage 通信也不能推导出能力对象已消失；
[微软 IsWebMessageEnabled 合同](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2settings?view=webview2-1.0.4258.31#get_iswebmessageenabled)
仅承诺禁止通信，未承诺移除对象。

拼图本身已有独立的官方 frame 消息合同，其调用链为：

1. `g`（约 514437）构造同源 `/core/v4/captcha`，设置 `Token` 为当前 start
   token，并设置 `ForceWebMessaging=1`。
2. frame listener（约 514952）校验 origin 及该 iframe 的 `contentWindow`，
   收到 `pm_captcha` 后将 `event.data.token` 原样传给 `onSubmit`。
3. captcha method（约 550680）把该值原样传为 `onSubmit(value, "captcha")`。
4. 顶层 submit（约 809921）在 `embed=true` 时跳过非嵌入模式的 `ak.op`
   challenge 绑定 API，直接生成 `HUMAN_VERIFICATION_SUCCESS`，其中
   `payload.token` 等于上述值、`payload.type` 等于 `captcha`。此链路没有拼接
   start token 或变换结果 token；两类 token 必须继续分开。

Windows 原生实现据此限定为 CAPTCHA，强制官方页面的允许方法子集及
`embed=true`，不改变官方 bundle、平台检测或认证身份。fresh 顶层监听独立校验
frame 的严格来源、路径、唯一 `Token`、`ForceWebMessaging=1`、消息 source、
当前挑战 SHA-256、期限及一次性状态后，将该值映射为同一嵌入 success schema。
不解决或操作拼图。没有可用 CAPTCHA 方法时直接报告不可用。

结果通过绑定随机 nonce 的 suppressed alert 交给
[ScriptDialogOpening](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2settings?view=webview2-1.0.4258.31#get_aredefaultscriptdialogsenabled)
窄桥；默认对话框关闭，不向用户显示或记录正文。原生再次校验 alert 来源 URL、
当前顶层挑战与 nonce，并在同步回调返回后关闭窗口。Linux 和 Android 仍使用
官方 `web` 分支的顶层 postMessage；Android 只持有 native message-port 端，
不再次操作已转移给浏览器的端点。

[离线永久回归](../tool/test_proton_native_bridge.py)使用独立接口 fixture 表达
上述平台选择和 frame 结果路由，执行实际生成的监听代码，保留 Windows 空分支
并验证结果 token 未拼接。未把官方 JavaScript 源码复制进仓库，也没有据此宣称
真实 WebView 页面、服务端接受性或 WebView/API 出口一致性已经通过。
