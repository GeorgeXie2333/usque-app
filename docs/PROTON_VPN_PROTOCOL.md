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

Android 的上述快照实际声明 Core `36.8.0`，公开 Core 仓库快照则为 `36.3.1`。
此次另外读取 Maven Central 发布的
[auth-data 源码包](https://repo.maven.apache.org/maven2/me/proton/core/auth-data/36.8.0/auth-data-36.8.0-sources.jar)、
[auth-domain 源码包](https://repo.maven.apache.org/maven2/me/proton/core/auth-domain/36.8.0/auth-domain-36.8.0-sources.jar)、
[challenge-data 源码包](https://repo.maven.apache.org/maven2/me/proton/core/challenge-data/36.8.0/challenge-data-36.8.0-sources.jar)
和 network-domain 源码包，逐字节确认本文引用的 `AuthRepositoryImpl`、
`PostLoginLessAccountSetup`、`ChallengeFrame` 和 `HumanVerificationNeededHandler`
与公开快照相同；这不声称整个 Core 版本相同。原生 Android 兼容版本固定为同一
Android 快照的
[版本元数据](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/metadata/version_fdroid.txt)
所列 `5.20.74.0`，采用直接 APK flavor
的 `+apk` 后缀，版本更新须同时核对两类请求头。

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

源码没有证明服务端允许任意第三方 App ID 或平台 frame。Python 实验保留其独立
身份；后续原生 Android 的兼容身份见本文原生章节。TLS 和主机名必须校验；显式代理失败
不能自动切换直连。非 2xx 错误 JSON 和 `Retry-After` 仅在受大小限制的解析器中
处理，不输出原正文。`Code=1000` 是普通成功；`5003/5005` 是版本错误；
`9001` 是人工验证；`9002` 是独立的设备验证流程，不能假称普通 captcha 可解决。
依据：[响应码](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/network/domain/src/main/kotlin/me/proton/core/network/domain/ResponseCodes.kt)。

Core 的响应码表没有 `8004`，但补充核对的
[Proton 官方客户端常量](https://github.com/ProtonMail/clients/blob/2ecb794dbc221384dc6d88840965ac144db301ad/project/mail/rust/api/mail-api-session/src/consts.rs#L108-L121)
将它定义为 `SessionNotAllowed`，注释为客户端不被允许创建会话。原生实现保留
失败阶段和 HTTP/API 数字并单独显示此分类，不触发人机验证；不能仅凭此码确定
具体是哪个请求字段或服务端资格条件被拒绝。`10200` 对应游客账号无效/不可用，
也不触发拼图。

## 会话、游客与账号

[AuthenticationApi](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/data/src/main/kotlin/me/proton/core/auth/data/api/AuthenticationApi.kt)
定义以下方法；字段来自同目录 request/response 文件及
[AuthRepositoryImpl](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/data/src/main/kotlin/me/proton/core/auth/data/repository/AuthRepositoryImpl.kt)。

| 请求 | JSON 正文 | 关键响应/后续 |
| --- | --- | --- |
| `POST /auth/v4/sessions` | `Payload` 设备 frame map | `UID, AccessToken, RefreshToken, TokenType, Scopes`；这是未认证会话 |
| `POST /auth/v4/credentialless` | `Payload` frame map | 登录响应增加 `UserID, EventID, LocalID` 等；随后刷新 scopes，本地建立游客用户并初始化 VPN |
| `GET /auth/v4/scopes` | 无正文 | `Scopes` 数组；游客初始化不要求普通 `/core/v4/users` 资源 |
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
官方的
[PostLoginLessAccountSetup](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/domain/src/main/kotlin/me/proton/core/auth/domain/usecase/PostLoginLessAccountSetup.kt)
刷新 scopes，并在本地建立 `Type.CredentialLess` 用户。原生游客实现按此区分，
随后仍必须成功验证受保护的 `/vpn/v2`。仅普通账号使用 Core 用户资源核验身份。

账号路径应保持独立：官方 `PerformLogin` 直接以 `sessionId=null` 获取 SRP info
并提交登录，`SessionProviderImpl.getSession(null)` 读取已有未认证会话。
但这不是完整调用链：启动时 `UnAuthSessionFetcher` 在没有会话时请求匿名会话，
统一 `TokenErrorHandler` 也会在 HTTP 401 时调用 SessionManager；没有会话时
创建匿名会话，已有会话时刷新，然后重新执行原请求。不能仅从 `PerformLogin`
方法体推导出密码登录不需要匿名会话上下文。账号登录不调用 credentialless，
匿名会话也不代表已取得游客或普通账号身份。
依据：[PerformLogin](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/domain/src/main/kotlin/me/proton/core/auth/domain/usecase/PerformLogin.kt)、
[SessionProviderImpl](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/account-manager/data/src/main/kotlin/me/proton/core/accountmanager/data/SessionProviderImpl.kt)、
[UnAuthSessionFetcher](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/network/presentation/src/main/kotlin/me/proton/core/network/presentation/UnAuthSessionFetcher.kt)、
[TokenErrorHandler](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/network/domain/src/main/kotlin/me/proton/core/network/domain/handlers/TokenErrorHandler.kt)、
[SessionManagerImpl](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/account-manager/data/src/main/kotlin/me/proton/core/accountmanager/data/SessionManagerImpl.kt)。

原生账号登录在 SRP info 或提交证明收到 HTTP 401 时，为当前任务创建或刷新
未认证会话，并按同一正文重试原调用一次。Android API/User-Agent/locale 和
设备 frame 保持同一个客户端配置；创建或刷新匿名会话的请求不携带用户名、
密码或 SRP 证明。
只有实际的 9001 才进入人工验证，每个请求的验证码头均保持局部，不跨恢复
会话复用。重复 401、明确的错误凭据 8002、恢复失败、超时、未知变更、取消或
请求所属任务失效均停止；不会递归恢复匿名创建或刷新接口，也不会无限重试。
恢复后的失败步骤仍关联原 SRP 调用，恢复本身失败则显示匿名会话或刷新步骤。
该路径不接受已认证会话；已保存账号继续使用原有持久化轮换日志。匿名刷新失败
时保守停止并执行适用清理，不自动遗忘旧令牌并另建身份；这比官方对失效匿名
会话的强制退出后重建更严格。此修改没有补齐 FIDO2、其他二次验证或全部账号
初始化分支，不能据此宣称账号登录已完全兼容。

刷新正文固定为 `UID`、`RefreshToken`、`ResponseType: token`、
`GrantType: refresh_token`、`RedirectURI: http://protonmail.ch`。该 URI 是协议中的
请求字段，PoC 不向它导航，也不通过它发送凭据。依据：
[RefreshSessionRequest](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/data/src/main/kotlin/me/proton/core/auth/data/api/request/RefreshSessionRequest.kt)。
刷新成功后保留新 UID/token，旧组合不再用于认证。网络超时后刷新是否生效可能
未知，不能用无限重试掩盖旋转语义；清理失败必须独立报告。
刷新响应的 `SessionResponse` 不保证含 UserID：必须保留原来已认证的 UserID；
若响应显式返回另一个 UserID，则拒绝继续，不能在刷新过程中切换账号。

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
| `storageCapacity` | 总字节先作整数除法换算十进制 GB，再转换为 Double |
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
后续原生 Android 明确区分这两种 map：sessions 不带 `type`；credentialless 和
SRP 登录的 Device frame 带上述 discriminator。此编码来自实际发布的 Core 源码，
由共享 Kotlin/Rust 合成 fixture 检查；仍不是官方 App 的实网抓包证据。

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
原生 Rust 实现现按官方 `ConnectingDomain.getEntryIp` 规则，在没有
`WireGuardUDP` override 时读取同一物理节点的 `EntryIP`。2026-10-19 的合成
游客目录响应正使用这一结构：2,523 个物理节点均没有 `EntryPerProtocol`。
显式 override 存在但畸形时拒绝该节点，不改选默认地址；`Ports` 缺省或为官方
可空模型允许的 null 时才使用 clientconfig 端口。公网 IPv4、物理在线标志、
有效非零 X25519 公钥、免费层级及非受限节点检查仍适用，不能由 `EntryIP`
单独认定 WireGuard 连接或最终 agent 授权成功。
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
如实保留在结果中。普通 HTTPS 实验不能证明产品链式出口或系统防泄漏已通过；
当前原生 API 按账号管理与连接生命周期区分网络路径，见下文；早期控制面计划不能
直接作为当前实现合同。

## 原生实现、local-agent 与持续授权

当前产品增加 `proton_vpn` 来源，排列在 WARP 后面。控制面使用 Rust；Flutter
只展示任务、节点和允许公开的状态，通过独立敏感字节字段传递短期凭据与验证码
交接。Windows、Linux、Android 的原生窗口仅用于官方人工验证；用户完成拼图，
程序不解题。验证码 WebView 与交互账号管理 API 默认遵循系统网络路由；任务可在网络失败后
单向切换一次到 WARP，包含验证码和其 DNS 请求。业务拒绝不切换；不确定是否已提交的
会话变更不重放。原操作已明确被 9001 拒绝且会话状态确定时，验证码网络失败可触发
一次新挑战，旧窗口／挑战失效。

能力 `proton_account_warp_fallback` 使用 protobuf 字段 52。新 GUI 仅在能力存在时，
对 guest/login/refresh/logout 设置 `network_policy_version: 1`；缺省 0 保留旧行为。
任务状态仅增加 `account_network` 与 `network_phase`，不包含凭据或 URL。

连接中切换节点使用能力 `proton_server_switching`（字段 53）。
`ChainExitSettings.proton_server_id`（可选字符串，字段 7）缺省时沿用账号偏好，
显式空串表示“自动”，非空为最多 256 字节的 ASCII 逻辑节点 ID；仅允许 Proton 来源。
GUI 将账号及节点暂存为同一设置草稿，用户应用后走现有 `HotVpnGate`／平台保护替换流程。
切换账号时清除原账号的节点覆盖值。Android 连接身份比较包含该字段，并区分缺省与空串。
准备的出口必须与设置中的节点覆盖一致；节点经 WARP 内部控制通道解析，复用有效证书，
不更改本地账号密钥或绕过最终授权门。找不到手动节点时失败，不自动改用其他节点。
用户点击应用后，先持久化目标设置再替换运行时，失败或进程恢复后仍可重试；旧引擎不展示连接中的节点编辑。
敏感 handoff 的 version 2 包含 route_epoch（system=0，WARP=1），WARP 时附带随机
loopback 端口、用户名、密码、nonce。`verification_failed` 绑定 job/challenge/route_epoch。
旧原生窗口无法解析新 handoff 时停止，不降级系统路径。

WARP 优先借用现有健康 underlay，否则使用已选账户身份临时启动 headless 数据面。
API 不 protect／绑定物理网卡，不改系统 VPN、代理、路由或 DNS。保留的网络保护仍约束
临时连接。验证码代理仅 CONNECT 三个固定 Proton 验证域名的 443 端口，DNS 使用同一
WARP；HTTP + CONNECT 随机 `<nonce>.usque-proton-proxy.invalid` 预检先于含令牌页面，完成标记包含随机代理
密码的 SHA-256，不能仅凭公开 nonce 伪造通过。无 MITM 或证书绕过。
Android 网络安全配置默认禁止明文，仅为该保留域及子域开放预检；Proton 请求仍须 HTTPS。
桌面资源级失败检测覆盖脚本、图片、fetch/XHR；Windows 也订阅跨域 iframe 和 worker 的
网络失败。取消、CSP/CORS 阻止及正常 HTTP 错误响应不作为网络切换依据。
任务回复可短暂保留旧 challenge；GUI 记录手动及自动打开过的 challenge，仅自动打开新挑战。
预检 5 秒，页面加载 15 秒，人工输入最多 300 秒，代理至多 32 连接、16 KiB 头、64 个头字段，30 秒空闲超时、每方向 64 KiB 缓冲。
任务网络预算为 120 秒（不计人工输入）；每次验证窗口预留其 15/20 秒加载上限，
避免信任前端上报的计时。初始无凭据 ping 5 秒、WARP 获取 30 秒；
API 原有 5/15 秒限制及清理 25 秒、停止等待 45 秒继续生效。

连接准备则显式接收已建立的 WARP 内部网络句柄：账号／权限检查、目录刷新、
证书申请及其后整个 lease 的续期、令牌恢复、agent 授权恢复都使用该句柄，DNS 也在
WARP 内解析，不依赖被最终授权门阻断的系统 VPN 路径。结果绑定 WARP 会话代次；
重连、取消或关闭后不接受旧结果，不回退系统／物理网络，也不向应用暴露 WARP-only 出口。
两种客户端均禁止重定向、隐藏重试及自动解压，保留响应体预算、超时、取消、
TLS 证书/主机名校验和数值错误导出。实际 WireGuard 与 local-agent 继续绑定
WARP 链式出口。Android 使用
固定 APK 兼容标识 `android-vpn@5.20.74.0+apk`，电视使用 `android_tv-vpn` 前缀。
User-Agent 同步为 `ProtonVPN/5.20.74.0 (Android <实际系统版本>; <实际品牌> <实际型号>)`，
按官方规则把非 ASCII 的 UTF-16 单元替换为 `?`；`x-pm-locale` 取实际默认语言。
设备 frame 通过私有 JNI 从真实 Android 语言、时区、设备描述、字体、容量、
主题、输入法及 root 检查结果生成。
完整设备 frame 仅在内存中用于当前任务；续期所需的请求身份只随加密会话保存，
不出现在普通状态/诊断中。旧版会话缺少身份字段时保持原有默认头。
非 Android 的新游客在首次请求前自动选用独立的固定合成 Android 配置，复用
同一版本、请求头和 frame 编码校验：Android `15`、品牌 `google`、型号
`Pixel 8`，设备描述 `Pixel 8/google shiba` 的有符号 UTF-8 rolling hash 为
`73362205250`。语言为 `en`、区域为 `US`、时区为 `UTC`、offset 为 `0`；
字体缩放为字符串 `1.0`、容量为十进制 `128.0` GB、root 和深色主题均为
false、输入法列表为空。模板由正式 Rust 代码定义，不引用测试资源、不读取
本机硬件标识，也不生成每安装或每账号的设备追踪 ID。
选择仅发生在新游客分支，已有 Android 上下文优先；普通非 Android 账号登录
继续使用独立 `external-vpn@0.1.0` 身份和系统/架构 frame。保存后的游客请求、
刷新、证书申请与撤销均沿用现有加密记录的 API 身份，不重新按主机或新模板
推导；旧账号保留原身份，无新增 IPC 字段或会话存储版本。完整设备帧仍只在
当前任务内存使用，不保存、导出或记录日志。不构造输入事件或 attestation。
这些请求格式调整不证明真实服务端已接受此客户端。支持 SRP v4、单独 TOTP、
每个实际返回 9001 的请求分别人工验证；明确 12087 时可清除结果后额外获取一次新挑战；9002 和 8004 不进入普通拼图。
验证码 WebView 保留平台浏览器默认 User-Agent，与官方 HV3 页面设置一致，
不会把 API User-Agent 强加给网页资源。
Android 收到带有效挑战的 9001 后自动打开原生验证窗口，同一 job/challenge 只自动
打开一次；轮询不会重复弹出。关闭窗口或取消任务仍使迟到结果失效。

| 原生状态边界 | 实现约束 |
| --- | --- |
| 配置准备 | 游客以匿名会话申请 credentialless，检查 scopes 和 VPN 权限；普通账号验证用户资源和 VPN 权限。新登录不额外旋转 token；按账号合并保存本地密钥、逻辑节点偏好及完整可用目录，证书延迟到连接时申请 |
| 持久化 | 账号/seed/证书放入平台加密 sidecar；密码不落盘；轮转请求前持久化 pending 标记，超时或身份未知不重放旧刷新令牌 |
| 开始连接 | 通过已建立的 WARP 内部网络及 DNS 检查账号、刷新目录并解析逻辑节点当前入口；复用有效证书，仅缺失或到期时申请，再启动绑定 WARP 的内存 WireGuard/agent 栈；API 失败不回退系统网络，业务仍等待最终授权 |
| 最终授权 | WireGuard 握手后通过隧道内 TCP `10.2.0.1:65432` 建立 mTLS；校验节点 SNI、VPN 专用 CA、客户端 Ed25519 密钥/证书、agent connected 状态及选定节点标签，之后才放行业务；普通功能设置单独同步，不要求完整回显 |
| 持续授权 | 定期检查 agent 状态，通过原 WARP 控制句柄在到期前刷新同一账号/密钥证书及恢复授权；过期、撤权、侧车变更或无法确认的授权关闭业务；取消时等待所持有任务完成 |
| 退出与本机遗忘 | 联网退出只尝试撤销本机拥有的会话；显式“仅忘记本机”只删除本地记录，不宣称远端已撤销；两者保护正在使用的账号；界面先清除已保存选择，保存失败则停止删除 |

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

1. `g`（约 514437）调用 `Uq` 构造 `/core/v4/captcha`，设置 `Token` 为当前
   start token，并设置 `ForceWebMessaging=1`。`Uq`（模块 46083）在生产域名上
   将主机改为 `verify-api.proton.me`，因此 iframe 与顶层 `verify.proton.me`
   跨来源；只有 localhost、IP 及指定开发域名才走同源 `/api` 前缀。
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

2026-10-22 复核纠正了此前的“同源”假设。直接接收 `pm_captcha` 时必须校验
`https://verify-api.proton.me`，顶层成功消息及原生消息来源仍必须为
`https://verify.proton.me`，两者不能混用。Linux 导航策略只为当前挑战、指定路径、
唯一 `Token` 和 `ForceWebMessaging=1` 的验证码 iframe 增加来源许可；顶层响应
和原生回调继续要求原挑战根 URL。Android 保留正常的顶层转发路径，并修正备用
直接接收路径的验证码来源；不会向任意子 frame 暴露 native interface。

2026-10-20 的回调修正将 Windows 结果传递改为绑定随机 nonce 的
[WebMessageReceived](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2#add_webmessagereceived)
通道，并显式启用 `IsWebMessageEnabled`。不再依赖 suppressed alert 或
`ScriptDialogOpening`；默认脚本对话框仍关闭。原生再次校验消息来源 URL、
当前顶层挑战与 nonce，并在消息回调返回后关闭窗口。已校验并排队的结果在
窗口关闭时再次检查挑战和期限后回传，避免关闭消息把成功覆盖为取消；尚未收到
有效结果时关闭仍为取消。该修改不改变官方页面的 WebView2 平台选择。

Linux 同时接收官方 `web` 分支的顶层 postMessage 和原始 `pm_captcha`
结果，不再只依赖二次转发。Windows 和 Linux 共用的顶层监听在 capture 阶段
校验结果，先于页面 submit 监听移除 iframe；直接路径仅适用于唯一的
`embed=true`。Android 同时接收该顶层消息和上文所述的原始
`pm_captcha` frame 结果；直接路径也要求唯一 iframe、严格来源、路径、
challenge token、`ForceWebMessaging=1`、消息 source、当前顶层挑战 SHA-256
和期限均匹配，且服务端确实允许 CAPTCHA。两条路径共享一次性状态，不拼接
或变换结果 token。Android 只持有 native message-port 端，不再次操作已转移
给浏览器的端点，也不增加暴露给子 frame 的 JavaScript interface。

Android 先锁定有效结果的私有副本，离开 WebMessage 回调后再销毁 WebView、
回传该结果并清零副本；清理不能把成功改成取消，旧窗口回调不能关闭新窗口。
Flutter 固定版本会将平台消息包装为只读字节，StandardMessageCodec 的 typed
data 解码仍借用该 buffer。Dart 必须先为 handoff 和成功结果建立自己持有的
可写副本，再提交结果和清零，不能直接对平台借用视图执行 `fillRange`。
转移的可写源立即清零；只读平台源只能释放引用，不能宣称已将框架持有的内存
清零。应用持有的副本在成功、取消及迟到结果路径均清零。这一修正不改变验证码
token、原生窗口桥接或 API 请求身份。
依据：[Flutter 平台回复的只读包装](https://github.com/flutter/flutter/blob/84fc5cbb223bc12f83d65b647ff8a56caf779ffd/engine/src/flutter/lib/ui/window/platform_message_response_dart.cc)、
[typed data 解码](https://github.com/flutter/flutter/blob/84fc5cbb223bc12f83d65b647ff8a56caf779ffd/packages/flutter/lib/src/foundation/serialization.dart)。
弹窗外部触摸不取消拼图，意外的顶层导航仍被阻止。
显式取消按钮或返回键产生取消结果；过期、TLS、桥接及生命周期失败使用固定
错误代码，界面在后续清理轮询中保留该失败和允许列表内的错误代码，并独立
展示撤销结果未知的状态。异常原文及未知错误代码不会显示。
这些回归检查不证明用户设备上此次关闭的具体触发原因。

[离线永久回归](../tool/test_proton_native_bridge.py)使用独立接口 fixture 表达
上述平台选择和 frame 结果路由，执行实际生成的监听代码，保留 Windows 空分支，
使用生产环境的跨来源 iframe 地址，并按该 iframe 的 origin 模拟官方父页面的
转发。覆盖脚本对话框无回调、缺少顶层转发、submit 移除 iframe、重复结果及错误来源，
并验证结果 token 未拼接。未把官方 JavaScript 源码复制进仓库，也没有据此宣称
真实 WebView 页面、服务端接受性或 WebView/API 出口一致性已经通过。

[Windows WebView2 回归](../apps/usque_gui/windows/runner/proton_verification_webview_test.cpp)
另以真实浏览器内核加载两个不同 HTTPS origin 的合成文档，执行正式 Session 的
注入、原生回调和关闭流程；所有请求均被拦截为本地合成响应。它验证跨来源结果
会自动关闭窗口及错误来源、错误挑战被拒绝，不等价于真实 Proton 拼图或服务端验收。


## 逻辑节点、会话与证书生命周期补充

2026-10-19 再次通过官方 GitHub Contents API 校验了固定提交的
[CertificateRepository](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/vpn/CertificateRepository.kt)、
[RefreshSessionRequest](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/auth/data/src/main/kotlin/me/proton/core/auth/data/api/request/RefreshSessionRequest.kt)
与 [HumanVerificationInvalidHandler](https://github.com/ProtonMail/protoncore_android/blob/1b87f94ebfdfaf5e67145e8668efc52dbb931e0b/network/domain/src/main/kotlin/me/proton/core/network/domain/handlers/HumanVerificationInvalidHandler.kt)。
Git blob 与本地缓存逐字节一致；后者还与 Android 实际声明的 Core 36.8.0
network-domain sources 包对应文件一致。未复制 Kotlin 实现。

官方证书仓库按 sessionId 维护密钥与证书，复用未到期证书，按期限刷新；不要求
每次连接都先旋转 OAuth token。一个 LogicalServer 可以包含多个 ConnectingDomain，
节点选择和物理入口选择属于不同层次。原生实现据此按逻辑 ID 去重，保留账号
MaxTier 内、非 Secure Core／受限类型且兼容在线 WireGuard UDP 的全部候选，
取消旧的 200 项截断；API 与加密目录仍限制 20,000 个逻辑节点。公开 IPC 每页
128 个节点，仅含名称、国家、城市、负载及逻辑 ID；页数、总量及目录 revision
必须一致。Proton 账号摘要和节点分页不包含入口、公钥、会话 UserID、token 或证书。

v2 加密记录允许尚未签发证书，兼容读取 v1，并将旧的 logical:physical 选择迁移为
逻辑 ID。账号重新登录复用原本地密钥和配置 ID；同一 UserID 的旧记录合并展示。
连接时从有效的加密连接缓存或在线目录解析入口、公钥及 agent SNI，保留本地
静态配置作为完整性绑定。手动选择的逻辑 ID 不可用即报错，Automatic 才自动选择。
v3 记录在同一个原子加密文件中增加当前隧道密钥和最近自动换钥时间；原静态配置
的绑定密钥和 revision 保持不变。读取仍兼容 v1/v2。重新登录、刷新列表和更换
逻辑节点不会丢弃已恢复的隧道密钥。

v5 记录兼容读取 v1–v4，扩展可选 `connection_cache`，独立保存权限核验、客户端
配置和目录时间，以及已校验的默认 UDP 端口和连接入口。入口缓存不再只保留
前 32 项，而是在 20,000 项、约 4 MiB 的上限内保存候选；完整公开逻辑目录仍
保留最多 20,000 项。缓存优先保留现有排序的候选及手动选择的逻辑节点。
指定节点未命中缓存时在线刷新，不以其他逻辑节点代替。注册、登录和手动刷新
复用已经取得的权限、配置和目录结果。旧 v4 缓存可以读取，但不能作为完整
条件请求快照；其首次目录刷新取得默认端口和完整目录后才启用条件请求。

| 数据 | 已连接后的刷新年龄 | 新连接允许的最大年龄 |
| --- | --- | --- |
| 权限 | 12 小时 | 12 小时 |
| 入口目录 | 12 小时 | 12 小时 |
| 客户端配置 | 12 小时 | 目录获取时最多复用 12 小时 |

这些周期采用官方 Windows
[DefaultConfiguration](https://github.com/ProtonVPN/win-app/blob/d2a4f8bc92a0fd296943a7cdd15f4f870c8a87f9/src/Configurations/ProtonVPN.Configurations/Defaults/DefaultConfiguration.cs)
中的 `VpnPlanRequestInterval`、`ServerUpdateInterval` 和
`ClientConfigUpdateInterval`。这是本项目共享核心采用的刷新策略，并非宣称官方
所有平台都有同一强制缓存期限；Windows 的 5 分钟 plan 最小请求间隔是节流值。

固定认证 API 返回的有界 `Last-Modified` 随完整入口快照加密保存。目录到期后
发送 `If-Modified-Since`；只有该目录 GET 的空正文 `304` 可以延长原快照时间，
保持逻辑目录 revision 和手动选择。旧格式、不完整或缩减的缓存、默认端口变化、
时钟回拨和失败入口均禁用条件复用；会话恢复后的重试必须重新获取完整目录。
配置仍在有效期时，常规目录刷新不重复请求 clientconfig；显式手动刷新始终
重新获取客户端配置，避免旧默认端口使手动恢复无效。

达到最大年龄、缺失或时钟回拨时，新连接必须在线获取对应数据。缓存不跨账号
复用；UID 变化、重新登录、API 身份变化、权限等级变化和隧道换钥会清除相应
旧数据。失败入口会使目录和配置缓存失效，下次连接重新查询。缓存不代表授权：每次
连接仍检查证书、完成 WireGuard 握手、mTLS 和 agent 状态／节点标签确认。
加密记录仍限制明文 8 MiB、密文 12 MiB；可选缓存超限时先缩减入口并清除条件
复用标记，仍超限才省略缓存。完整目录
或基础记录自身超限、加密失败与写入失败仍报错。公开 IPC 不增加入口或密钥。

自动选点统一按 `ExitCountry + State + City` 分组，与节点总数和 Free／Paid
身份无关。州／省和城市先规范空白与大小写；缺失城市的节点按逻辑 ID 单独
保留。每城先测一个代表，优先采用当前网络近期的实测低延迟节点，其次按目录
负载、名称与 ID 排序。代表失败后最多再试两个同城节点，其他城市的首轮探测
优先于补测。完整节点列表仍保留供手动选择。旧入口快照缺少州／省分类，或入口
缓存被缩减时，自动模式强制取得完整目录；不使用不完整快照宣称城市全覆盖。
`SelectedNode.state` 是可选加密字段，缺失代表旧快照，空串代表 API 未提供州／省；
不新增公开 IPC 字段，也不改变账号手动偏好。

探测只使用已建立 WARP 内部 UDP，独立实现官方
[ServerPing](https://github.com/ProtonVPN/android-app/blob/deb11fef4092a1ecfcf51a7d40bdece9eb4c3e5e/app/src/main/java/com/protonvpn/android/vpn/ServerPing.kt)
的可用性报文：`fe 01`、小端秒级时间戳，以及公开协议常量对服务器公钥与时间戳
计算的 HMAC-SHA256；响应必须来自目标端点且恰为 `fe 01 01`。响应不是密码学
服务器认证，只用于选点，后续 WireGuard 和 mTLS／agent 授权保持不变。
默认并发 8、内部上限 16，发送至少间隔 10 毫秒，单次探测最多 1 秒；每次采样
使用独立源端口，避免固定响应被误计入下一轮。覆盖各城市代表后，对最快的至多
5 个候选各追加两次采样，以中位数排序；复测超时按超时上界计入，两次复测均
失败的候选不作为有效测速结果。没有候选通过复测时，使用原目录顺序
尝试连接，不伪造低延迟值。已有连接总期限和取消仍约束整个扫描。

选点结果只在当前 WARP 句柄内存中保留 30 秒；同城代表的历史测量最多保留
5 分钟。缓存绑定账号、目录 revision、WARP 会话以及实际承载路径的内存 epoch。
路径 epoch 在启用新隧道及收到 H3 迁移成功确认后递增；系统报告的新物理网络
尚未成为承载路径时，禁止启动测速或复用测速缓存。迁移期间的扫描会取消并
等待探测任务退出，丢弃部分结果；迁移完成后的下次选点重新测量，不继承旧
路径的优胜节点或同城历史。入口变更不继承旧 RTT。启动连接失败会清除
选点缓存。取消或网络变化停止并等待探测任务，部分扫描不发布结果。自动模式选出的节点不会
写成手动偏好，也不会因后台目录更新而主动切换已连接的节点。

同一次用户连接另外在内存中固定选中的逻辑节点 ID，不受上述测速缓存有效期
或网络 generation 变化影响。自动重连、完整运行时重建、证书／密钥恢复均
沿用此 ID，不重新测速；必要时仍刷新目录、权限和原节点的入口参数。原节点
不可用则报错，不自动换城或换节点。用户主动发起新连接／重试会重置此固定值，
自动模式可再次选点；手动节点偏好仍优先。此状态不写磁盘，进程重启后重新选点。
Android 同一进程内的网络恢复保留固定值，桌面连续连接内部重试同样保留。
目录、配置和权限的 12 小时策略不变。内存栈和模拟时钟测试不证明真实 Proton 探测可达性，
实际延迟、限流行为和手机资源消耗仍需隔离环境验证。

证书仍在连接时经 WARP 首次签发。已有证书通过密钥、签名和有效期校验且剩余
超过 180 秒时，建议续签时间 `RefreshTime` 不阻塞连接；最终业务准入后再续签。
缺失、已过期或临近到期的证书在准备阶段请求，瞬时失败只允许继续使用经校验
且尚未过期的证书，真实到期仍关闭门禁。该复用方式参考官方 Android
[CertificateRepository](https://github.com/ProtonVPN/android-app/blob/deb11fef4092a1ecfcf51a7d40bdece9eb4c3e5e/app/src/main/java/com/protonvpn/android/vpn/CertificateRepository.kt)，
180 秒启动余量为本项目的保守限制。

后台刷新复用当前账号 Lease，在最终业务准入后启动，单次执行并受断开取消与
等待约束。证书续期和 agent 恢复优先，可取消并等待正在执行的元数据请求。
瞬时网络／限流／服务端错误按 30–300 秒退避，仍有效的 agent 授权连接继续运行；
明确权限失效关闭业务准入，等级或 UID 变化使旧证书失效并要求现有重连流程
重新建立隧道。目录刷新不会热切换当前物理入口。账号文件锁与已知令牌替换的
持久化检查点继续保护并发变更，后台任务不会覆盖后来的节点选择或会话。

连接专用 API HTTPS 在同一账号 Lease、固定 `vpn-api.proton.me` 源和同一 WARP
generation 内使用私有 HTTP/1.1 连接，最多保留一个闲置连接，复用窗口 30 秒。
只有有界、无压缩且完整读取的 HTTP 响应允许复用；连接关闭、协议错误、部分
响应、取消和 generation 改变都会丢弃连接。提交前发现旧连接失效可以新建一次，
提交请求后不自动重放任何方法，尤其不重放证书／令牌 POST。正常停止取消并
等待连接驱动，底层无系统 TCP、DNS 或代理回退；交互式系统 HTTPS 和其他
InternalNetwork HTTPS 调用不参与此池。
此连接专用 API 的 DNS 任一地址族先返回即可开始首个 TCP 尝试；通过同一 WARP
dialer 复用有界候选竞速，最多两次连接并发，可用候选通常按 250 毫秒错峰启动，
明确失败后可立即尝试后继。另一地址族 DNS 未返回时，共享调度器为其保留一个
槽位；若首个 TCP 卡住，同族备用地址仍可能等待另一地址族的 4 秒 DNS 超时。
成功或取消后撤销其余尝试，保留网络健康与 generation 检查，不引入系统网络
回退。调试计时仅记录固定阶段名、耗时、响应字节数、城市／响应数量和缓存命中标志，
不包含令牌、证书、节点地址或 API 正文；真实连接耗时仍需隔离环境测量。
下载的普通 WireGuard 配置与原生 session-mode agent 认证不能互相替代；握手后
仍须通过 mTLS、agent connected 状态和选定节点标签确认。普通功能选项的缺失或
不同默认回显不作为额外授权条件；明确 jail、限制、撤权和证书错误仍阻断业务。

受保护请求首次 401 才启动一次 token 恢复：先持久化 pending，发出唯一刷新请求，
校验并立即保存已知替换令牌，再做后续请求。已知响应与取消或外层变化同时到达时，
仍在独占 lease 内完成本机保存，但检查点阻止后续网络或业务授权。刷新结果未知
保持 pending，不重放旧 refresh token；畸形成功里可用的 UID/access 只作加密撤销
能力保存，不能作为认证账号。UID 改变会废弃旧会话证书，保留本地密钥重新签发。
撤销句柄保留各自 API 身份，失败不宣称远端已撤销。

Core 将 12087 定义为 USER_CREATE_TOKEN_INVALID；官方 handler 清除当前 HV 结果
后重试原请求。原生仅在验证后的明确 4xx/12087 时清除该结果，以同一会话、正文
和身份重发原请求一次；若收到新 9001，仍由用户完成新挑战。最多两次人工挑战，
第二次失败或任何未知变更终止。不复用旧结果，不拼接 start token，不规避拼图；
WebView 与 API 出口差异仍可能导致拒绝，未宣称重试必定解决用户设备上的 12087。

### 节点标签、agent 原因与证书恢复

再次通过官方 GitHub API 校验 Android 当前 master 仍为 `316a3d499e120bd2cab63c2cd8ac396e3ab3b24a`。
官方 [ConnectionParams.bouncing](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/models/vpn/ConnectionParams.kt#L61)
取 ConnectingDomain.Label 非空值；[VpnBackend](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/vpn/VpnBackend.kt#L436)
将它加入 agent features。原生保留当前物理入口的标签，包括字符串 `"0"`，发送
`bouncing` 并要求相同值回显后才开放业务流量。标签只允许有界 ASCII 标识符，
畸形目录标签不能变成任意控制消息。

按官方 [错误分派与恢复](https://github.com/ProtonVPN/android-app/blob/316a3d499e120bd2cab63c2cd8ac396e3ab3b24a/app/src/main/java/com/protonvpn/android/vpn/VpnBackend.kt#L151)
区分 `86104` 暂时受限、`86101` 证书过期、`86102` 吊销、`86105` 签名错误、
`86103` 密钥重复、`86110..86115` 会话上限和 `86151..86154` 政策限制。
未知 nonfinal 原因只等待，不授权；未知 final 原因停止。受限状态在绝对连接
预算内等待，已连接会话重新受限则立即关闭业务门禁，最多等待 35 秒。
只有恢复后的 connected 与选定节点标签确认才能重新授权。

普通功能同步遵循官方 Android 的连接状态与偏好分离方式：发送 `netshield-level`、
`randomized-nat`、`split-tcp` 和可选 `bouncing`，不主动设置 `jail=false`。
经 mTLS 验证的 connected 状态不必等待普通偏好完整回显；选定节点的 `bouncing`
仍须匹配，明确 `jail=true` 或畸形授权字段不会开放业务。固定调试阶段标记只包含
栈、TCP、TLS 就绪及 connected/authorized 布尔值，不加入消息正文或身份材料。

续证 worker 在首次 agent 握手前启动。过期或远端 TLS 客户端证书警报触发同钥
续签与 agent 重连；远端证书信任或 hostname 校验失败不触发此恢复。吊销／签名
错误先持久化新隧道密钥及空证书，再签发并结束旧 WireGuard。首次连接在旧 worker
全部退出后最多重试一次；已连接会话经既有平台恢复流程重建完整链。每个 agent
最多一次证书恢复，跨重连五分钟内最多一次自动换钥。操作由同一账号 lease 串行化，
取消和外层 generation 检查不变。

定时续证的网络、429 或 5xx 暂时失败保留仍有效的证书，并从 30 秒退避到最多
五分钟。权限等级变化或 UID 变化会先废弃旧证书。已知 token 替换仍先落盘，
未知替换不能继续使用。连接时的 due 刷新暂时失败也只允许复用校验通过、尚未
过期的缓存证书。加密 sidecar 的独立变更、证书过期和终止授权仍关闭门禁。

连接错误增加 append-only `PROTON_*` 诊断分类，保留失败阶段和数字 API/HTTP
或 agent 上下文；不再压为 WARP 的 `AUTHENTICATION_FAILED` 或 pin 失败。
Android 恢复只接受当前 Proton profile、对应的结构化原因和匹配的 retryable
代码，不能用普通认证／证书失败开启重试。公开错误不含远端 description、URL、
会话标识、token、密钥或证书。
