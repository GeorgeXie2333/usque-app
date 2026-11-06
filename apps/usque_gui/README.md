# Usque GUI

Flutter UI for Windows, Linux, Android and Android TV. macOS source is retained for
future work but is not built, tested or released for the current product.
The UI uses native Flutter widgets. An isolated native WebView is reserved for
the official Proton human-verification page when the user completes its puzzle;
account passwords and all other application UI stay in native widgets.

Desktop builds start the Rust sidecar and talk to it over current-user IPC. Android talks to the Rust library inside the isolated `:vpn` process.

Build and test commands are in [CONTRIBUTING.md](../../CONTRIBUTING.md). Feature status is in [docs/IMPLEMENTATION.md](../../docs/IMPLEMENTATION.md).

## Localization

Keep all 21 catalogs complete when adding interface copy. Proton uses its own
catalog per locale, including separate Hong Kong and Taiwan Traditional Chinese
copy. Country and region display names cover the existing 249 ISO codes in every
locale; bypass search accepts localized names, English names and codes. VPN Gate
country filters use the same names while retaining ISO codes as their values.
Server names and city names supplied by a provider remain source data.

`test/localization_coverage_test.dart` checks locale and key coverage, nonempty
values, placeholder multiplicity, Proton lookups, country names and Android
translatable resources. The Proton widget tests exercise all 21 locales at 200%
text size, including RTL. Product names, protocol names and words spelled the
same in the target language are not missing translations.

Terminology rules for every locale:

- Use one term per concept within a catalog: account, server (never also
  "node"), the bypass/direct root shared by the Bypass page and the **Direct**
  action, diagnostic bundle, License Key (translated), WARP login information,
  and IP version (never "address family"; Simplified Chinese uses **IP 类型**).
- Don't show internal names: Engine, outputs, identity, draft, association,
  challenge. Point to updates as "this version of Usque" instead of the Engine.
- Instructions that name Android switches quote the exact AOSP Settings names
  for that locale ("Always-on VPN", "Block connections without VPN").
- Screen references match the labels in the same catalog. The short navigation
  labels (`nav_*`) are deliberately compact for the phone bar and can differ
  from the page titles.
- Each catalog uses one form of address, the form used by the core screens.

新增界面文案时需同时维护全部 21 个语言目录。Proton 分别使用各语言词表，香港和
台湾繁体使用对应措辞。国家／地区名称完整覆盖现有的 249 个 ISO 代码；分流搜索
支持本地化名称、英文名和代码，VPN Gate 筛选复用同一名称表。服务商提供的节点名
和城市名保留原数据。翻译覆盖测试检查目录、键、占位符和实际取词结果，Proton
界面测试覆盖全部语言的 200% 字号及从右到左布局。

各语言统一术语：同一目录内每个概念只用一个词，包括账号、服务器（不混用“节点”）、
与“直连”动作同根的分流用词、诊断包、许可证密钥（需翻译）、WARP 登录信息和 IP 类型
（不用“地址族”）。界面不出现引擎、输出、身份、草稿等内部名称，提示更新时写
“此版本的 Usque”。指引用户打开 Android 系统开关时，引用该语言 AOSP 设置的原文名称。
引用的页面名须与同一目录中的标签一致；导航短标签（`nav_*`）为手机底栏刻意从简，
可与页面标题不同。每个目录只用一种称呼方式，以核心页面为准。

## Profiles and scene drafts

The scene endpoint draft shares `EndpointFields` and preset-policy selection with
Advanced. It never calls global save operations. Scene settings require endpoint
selection, IPv4/IPv6, port and SNI, with an optional preset. Native previews and
restore use effective account endpoints, and Zero Trust overrides follow
Advanced's account-scoped merge.

The second destination is **Profiles** (**配置** in Simplified Chinese, **プロファイル** in Japanese), opening
on a selected-scene status card above the scene list, with **Manage accounts**
and **New scene** in the page heading. Accounts opens as
a standard subpage with its own create action, live account list and operation
feedback. Its nested section navigator preserves desktop navigation and uses the
normal leave guard; phones hide bottom navigation on subpages. Home account and
existing account-launch targets open Accounts directly; Back returns to Profiles.
The scene entry, ordinary navigation and Ctrl+2 open the Profiles root. Navigation
requests wait for the section to mount and cannot bypass unsaved-change guards.
Home shows the selected scene directly above Kill Switch at the same visual level:
in the desktop session card and the phone status card. The row opens Profiles.

Scene forms keep local drafts. Saving a template never calls the live network
settings save path. Schema 27 scenes own account binding, chain-proxy selection,
endpoints, IP policy and transport. Outputs, LAN, Ads, routing and all device DNS
remain global. The nested chain page reuses Proxy's full resource-management UI
in draft-selection mode. The nested account page is a single-choice picker: it
can add an account, but switching, licenses, renaming and deletion stay on
Accounts, and a pick binds the account to the draft and returns. Explicit
resource saves persist independently; returning preserves the parent draft. The native scene command
owns preview, optional account binding, activation and recovery. Current and
applied combinations are distinct, including uncertain replies and unavailable
dependencies. See [Profiles and scenes](../../docs/SCENARIOS.md).

Scene runtime confirmations are tied to the native owner epoch, session and
account observations. A failed refresh or a newer connection invalidates cached
runtime claims while preserving saved template and selection information.
Every catalogue carries its settings observation identity separately from the
public session ID. Android retains the observation generation after disconnecting,
so a disconnected catalogue can be confirmed without weakening stale session
checks. Grouped navigation cancellation revokes all provisional discard
permissions. Scene VPN Gate leases are tied to the native owner epoch; stale
leases are prepared again before selection or save, and cleanup requests cannot
release a newer owner's reference.

Android passive observation binds without creating the VPN service. When no live
control endpoint is available, saved network settings and scene templates can be
read in the UI process. These replies carry `runtime_observed: false` and contain
no native owner epoch, applied-session claims or operation receipts. They cannot
settle an uncertain write or authorize a scene resource lease. Explicit refresh,
connection recovery, scene preview/confirmation and draft-resource preparation
obtain a control owner for the work that needs one; ordinary observation never
does. Active connections retain their own service lifetime independently of UI
bindings.

When Android lists no running VPN process and no creating binding is pending,
passive settings and scene reads use saved configuration at once. A running VPN
process proves nothing by itself because the Quick Settings tile shares it.
If Android accepts a passive binding but never supplies a control endpoint, an
unsent settings or scene read falls back to saved configuration at its deadline.
When that deadline passes while the UI is visible, it also settles other unsent
passive reads, and later passive reads use saved configuration immediately until
an endpoint arrives, a creating binding starts or the UI stops. Sent requests retain their uncertain result. A
failed initial scene read stops the loading indicator and offers **Retry**,
without enabling scene creation until current settings have been read. A saved
scene catalogue remains visible while settings are also unobserved. Only a live
settings observation retires a pending saved-configuration scene read, after
which the visible page requests a fresh catalogue.

配置首页顶部的状态卡展示已选场景及其状态、已选账号和当前连接，当前设置偏离所选场景时
提供“用当前设置更新”和“另存为新场景”；下方场景列表只列出各场景与默认不同的设置。
页头的“账号管理”打开账号二级页，“新建场景”与之并列。首页账号和账号启动入口直达
账号页，返回回到配置首页；普通导航和 Ctrl+2 均回到配置首页，离开编辑器仍检查草稿。
配置页与首页都以引擎确认状态区分已选目标和当前连接。场景编辑保留独立草稿，
账号默认沿用当前选择，也可绑定指定账号；保存模板不会改变连接。场景只管理账号绑定、
链式代理、端点、IP 策略与传输协议，输出、局域网、广告、分流和 DNS 保持全局。
链式代理子页面支持完整资源管理；账号子页面是单选列表，可添加账号，切换、许可证、
重命名和删除仍在账号页完成。选择只更新草稿，明确保存的资源独立持久化。
首页将所选场景放在 Kill Switch 正上方，以同级状态行展示：桌面端位于会话卡内，
手机端位于状态卡内；点击该行进入场景配置。

Android 的被动观察绑定不会创建 VPN 服务。控制服务不可用时，UI 进程可读取已保存的
网络设置和场景模板，响应以 `runtime_observed: false` 标记，不包含原生所有者代次、
会话已应用状态或操作回执，因此不能确认结果未知的写入或授权场景资源租约。
用户明确刷新、恢复连接、预览并确认场景或准备草稿资源时，才为所需工作持有控制服务；
普通观察不获取这种所有权。活动连接独立于 UI 绑定维持服务生命周期。

Android 进程列表中没有 VPN 进程且没有进行中的创建型绑定时，被动的设置和场景读取
立即使用已保存配置；快捷设置磁贴与 VPN 服务共用该进程，因此进程存在本身不能证明服务在运行。
Android 被动绑定已受理但始终未提供控制端点时，尚未发送的设置或场景读取会在超时后
回退读取已保存配置；若超时发生在 UI 可见期间，还会同时结束其他尚未发送的被动读取，
此后在收到控制端点、开始创建型绑定或 UI 停止前，被动读取直接使用已保存配置。已发送请求仍保留未确认状态。
场景首次读取失败后停止加载动画并提供“重试”，读取到当前设置前不启用新建场景。
设置同样未被运行时观察时，已保存配置的场景列表保持可见；只有实时设置观察才会使等待中
的已保存配置场景读取过期，随后当前可见页面重新读取场景列表。

## Linux desktop

The production entry point is `lib/main.dart`; the Linux runner builds `usque`
with the application ID `io.github.georgexie2333.usque`. It launches the real
Engine through a private Unix socket and verifies the connected process UID and
PID. Engine configuration, cache and state use XDG directories; credentials
require an unlocked Secret Service keyring. The keyring error remains retryable.
The native runner provides single-instance activation, file pickers, browser
callbacks, notifications, login startup, and a StatusNotifierItem tray. Closing
to the tray is disabled when no host is present; host loss restores the window.
Window size and maximization are saved; Wayland positioning belongs to the
compositor. Output controls follow capabilities reported by the running Engine.
Copied License Keys use an owned clipboard selection that expires after 30
seconds without clearing a newer selection. Clipboard managers outside Usque
may retain their own copies.

**Settings → Background service** and onboarding offer explicit installation or
repair. Verify an AppImage signature before approving its initial administrator
request. Installation checks for host systemd, nftables and libsecret-1 and
does not connect a VPN. Restart Usque after service changes. An AppImage can
explicitly install its matching service through the installed helper; a
mismatched deb/rpm service requires the matching package to be reinstalled.
Linux update selection requires the current architecture and installation format
(`AppImage`, `deb`, or `rpm`) and signed metadata. The maintenance helper verifies
that metadata independently before installation. Cancellation remains retryable.

`lib/main_preview.dart` remains a separate debug-only simulated application with
its own ID and in-memory state. It cannot register production native shell or
Engine channels. Never package its `usque_ui_preview` binary. Required safe
checks and unavailable Windows/isolated checks are recorded separately in
[CONTRIBUTING.md](../../CONTRIBUTING.md).

## Brand assets

The transparent 1600-pixel [brand master](../../assets/branding/usque-app-icon.png)
is the only editable artwork. Run `python tool/generate_brand_assets.py` from the
repository root on Windows with Pillow and the Segoe UI fonts to regenerate
platform resources and the shared README banner. Recolouring and alpha-mask
extraction happen before resizing; do not edit generated variants individually.

App, tray and distribution icons use `#C2500C` with `#F5F4F1` lines.
`UsqueLogo` selects that light asset or the dark page asset (`#FFA45C` with
`#441800` lines) from the active Flutter theme. It retains each caller's size
and is decorative unless a standalone semantic label is supplied. Brand
decoration follows the existing theme's primary colour. The native setup and
uninstall windows take their colours, radii and motion timing from
`packaging/windows/setup/tokens.json`, which names the Flutter token behind each
value; system high contrast keeps system colours. After changing a referenced
theme value, update that file and run `python tool/generate_windows_setup_tokens.py`.

Android launcher artwork fits the centred 66dp safe area of a 108dp adaptive
layer. The full-colour icon uses an opaque `#C2500C` background and a transparent
foreground containing only the `#F5F4F1` U/star lines. The foreground retains
the lines' position relative to the master disk. Android composites adaptive
icons over black, so a transparent background would expose a black tile.
The launcher determines the final mask and effects; circular and rounded-square
icons both have the same full brand-colour background. Android 13+ themed icons
use the system's palette and background. The monochrome layer and notification
icon contain the U/star lines on transparency, rather than the solid circular
background. Notifications retain the `ic_stat_usque` resource name.
The retained macOS icons are refreshed
by the generator, without establishing macOS product support.

品牌母版是唯一可编辑图形。应用图标固定使用浅色配色；页面 Logo 随主题切换，
内部线条和透明边缘保持一致。Android 彩色启动图标使用不透明的品牌橙色满底，
前景仅保留米白色 U 形与星形线条，并保留母版中的位置和比例，避免透明背景露出系统黑底。
桌面决定圆形或圆角方形裁切，系统主题图标使用系统背景与配色。单色图标保留 U 形与星形线条，
安装器和卸载器的高对比度模式继续使用系统颜色。

## Linux development preview

Linux/WSL developers can run `bash tool/dev.sh preview` from the repository root
to open the debug-only UI preview with hot reload, simulated connection states,
sample traffic and an onboarding reset. Accounts, preferences and settings stay
in memory; no engine process, VPN or update operation starts. This preview
uses an independent application identity and does not access production data. See the
[Linux development guide](../../docs/LINUX_DEVELOPMENT.md) for native editing,
checks, limitations and ignored local files. Windows golden validation remains
required.

Linux/WSL 可运行上述命令预览界面并热重载，切换模拟连接状态和重新开始首次引导。
数据仅在内存中保存，Windows golden 基线仍须在 Windows 验证。

## Android TV text input

On Android TV and Fire TV, D-pad focus highlights a text field without opening
the keyboard. Press Select, Enter or gamepad A to edit; pointer and accessibility
activation also work. The first Back ends editing, keeps the draft and returns
to the same field. Directional navigation then reaches adjacent controls. A
later Back follows the existing page/dialog and unsaved-changes rules. IME
submission keeps the field's existing action; Next without a custom action
moves to the next focus target without automatically editing it.

All Material text inputs use the shared `TvTextInput` adapter. Native TV
detection and a UI-only editing-session channel handle legacy pre-IME Back and
Android 33+ system Back separately. Phone and desktop input keep their normal
behavior. The channel carries no field contents or credentials. Widget tests
and JVM policy tests use fake inputs and do not establish Fire OS IME behavior;
device and emulator validation remains separate.

Android TV／Fire TV 的方向键只移动焦点，按确认、Enter 或手柄 A 才进入编辑；
点击和无障碍激活同样可用。第一次返回结束编辑并保留草稿及当前字段焦点，之后
可以继续用方向键访问其他控件。再次返回才执行页面／弹窗返回和未保存修改保护。
手机和桌面保留原有输入交互。自动化测试使用模拟输入及 JVM 策略夹具，不等于
Fire OS 输入法实测。

## Editing and navigation

- Android **Settings → Application → Hide from recent tasks** hides Usque's
  task card, including on Android TV systems with a recent-tasks interface.
  It is off by default and takes effect immediately. Reopen Usque using its
  app icon or the existing notification/shortcut entries. Turning it off
  restores normal task visibility. The preference survives app restarts;
  **About → Clear all data** resets it. If the setting cannot be confirmed,
  use **Retry** before relying on it. The launcher icon, VPN notification and
  connection stay unchanged; this does not erase system usage records.
- Android **设置 → 应用 → 从最近任务中隐藏**会隐藏 Usque 的最近任务卡片，也适用于
  提供最近任务界面的 Android TV。默认关闭，切换后立即生效；可通过应用图标、现有
  通知或快捷入口重新打开，关闭开关后恢复正常显示。设置在重启应用后保留，
  **关于 → 清除全部数据**会将其重置。无法确认设置时，请先点击**重试**。
  启动器图标、VPN 通知和连接保持原样，此功能不会清除系统使用记录。

- Ads downloads missing verified rules when applying its enabled preference. While enabled, the running app checks every seven days, retries failed checks after one hour, and persists deadlines across restarts. The switch and downloaded rules apply to new requests without reconnecting. Status uses the native loaded revision; a first download failure remains visibly inactive with Retry. Rule details holds source, timestamps and manual update. Existing country update actions remain separate.
- 广告拦截在应用开启设置时自动准备规则，启用期间每 7 天检查、失败后 1 小时重试，期限跨重启保存。开关和规则更新直接用于当前连接的新请求，无需重连；状态按原生实际加载结果显示，首次失败提供重试，手动更新收进规则详情。

- Update checks on Windows, Linux and Android request GitHub's latest official release, compare its stable version with the installed version, and retain the 512 KiB response limit. Installable packages still require matching asset and manifest metadata (name, platform, architecture, size and SHA-256). Manual failures distinguish network/timeouts, rate limits, unavailable release information, oversized responses and invalid responses; automatic failures remain silent. Raw backend errors are never displayed.
- Windows、Linux 与 Android 的更新检查只请求 GitHub 最新正式版，比较已安装版本，并保留 512 KiB 响应上限。安装包仍须通过资源与清单的名称、平台、架构、大小及 SHA-256 校验。手动检查会区分网络／超时、限流、信息不可用、响应超限和响应无效；自动检查失败保持静默，界面不显示后端原始错误。

- First launch keeps four steps: welcome, system permissions, Cloudflare, Inc. terms and account setup. The wide layout's brand pane carries the non-affiliation notice on every step, so its welcome step explains the next steps instead; narrow layouts show the notice in the welcome step. The welcome step also outlines the remaining steps, and the progress line names the next one. Narrow layouts open every step with the progress line and show the mark only in the welcome heading. Steps are top-aligned; when the window has room and text is not enlarged, the actions stay at the bottom edge with the primary action at the end, otherwise they scroll after the step. The terms acceptance sits directly above those actions, so a short screen or large text never scrolls it out of reach. While a step continues below the pinned actions, it fades out above a hairline. Actions too long for one row stack at full width with the primary action on top and Back last. Setup lists share one row grammar: Android's notification permission is a row with its name over its state, and the account-method radios take the icon column so their titles align with the other steps' rows. The brand pane's ring shrinks on short windows to keep clear of its notices. Completing setup plays one hand-off of at most `UsqueMotion.lock`: the brand ring closes, the wide brand pane folds into the rail with its mark landing on the rail's (`ShellScreen.railBrandLogo`), and Home rises in. The outgoing setup holds its last frame and takes no input, focus or semantics; reduced motion, a hidden window and every other screen switch change directly. Android only requests optional notification permission during setup, explaining that the persistent service notification helps background operation and that no other notifications are sent. Onboarding never probes or requests VPN authorization. After the user presses Connect, Android requests VPN authorization directly if needed, then connects without an additional app confirmation. Cancelling system authorization leaves Home disconnected without restarting setup.
- The welcome copy identifies Usque as an unofficial client compatible with Cloudflare® WARP® services, with no affiliation, sponsorship or endorsement. All 21 language catalogs include a localized trademark attribution on every setup step. The wide brand pane shows it under the non-affiliation notice. Narrow layouts end the scrolling step with it: it rests just above the pinned actions while the step is short, and it follows the actions when they are not pinned. It stays reachable by scrolling on small screens and with large text.
- The terms step opens Cloudflare's application terms and the distinct personal WARP® and Zero Trust privacy policies in the system browser; like About's browser links, those rows end in an external-link glyph. Confirmation and step progress are saved locally without credentials. Account setup uses the native initial-identity operation, preserves an existing ready account, and checks an uncertain result before allowing a new attempt. Interrupted attempts require fresh user input; License Keys and login callbacks are never replayed from a saved draft.
- 首次引导仍为欢迎、系统权限、Cloudflare 条款、账号设置四步。欢迎页列出后续步骤，进度行提示下一步；窄布局各步骤以进度行开头，仅欢迎页标题显示 Logo。内容顶部对齐，空间足够且未放大字号时操作按钮固定在底部、主按钮始终靠后，否则随内容滚动；条款确认勾选框紧贴操作按钮上方，屏幕较矮或字号放大时也无需滚动即可勾选，内容未显示完时在底部渐隐并显示分隔线。按钮文字过长、一行放不下时，按钮改为整宽堆叠，主按钮在上、返回在下。引导中的列表统一行样式：Android 通知权限以“名称 + 状态”一行展示，账号方式单选项与其他步骤的列表文字对齐；窗口较矮时品牌栏圆环随之缩小，避免压住底部声明。商标声明在宽布局位于品牌栏的非关联声明下方，窄布局位于滚动内容末尾。完成设置后播放一次交接动画：品牌环闭合，宽布局品牌栏收进侧栏、Logo 落到侧栏 Logo 上，首页随后浮现；退场的引导页保持最后一帧且不接收输入、焦点和读屏。减少动态效果、窗口隐藏及其他页面切换均直接切换。Android 引导只申请可选的通知权限，说明常驻通知用于帮助后台保活、不会推送其他通知；不探测或申请 VPN 权限。用户点击连接后按需直接申请 Android 系统 VPN 授权，不再额外弹出应用确认框；取消系统授权后留在首页且保持未连接。已注册账号会被保留；通信超时后先检查原操作结果，确定中断后才由用户重新输入并重试。

- Accounts select WARP identities; network settings are shared across accounts.
- Advanced → WARP DNS offers WARP DNS types Plain DNS, DNS over HTTPS and DNS over TLS, sharing the encrypted type names with Direct DNS. Both editors use one complete HTTPS URL for DoH; DoT retains server name and port. New encrypted drafts prefill Cloudflare (`https://cloudflare-dns.com/dns-query` or `one.one.one.one:853`) and its IPv4/IPv6 bootstrap addresses. WARP bootstrap IPs remain optional; Direct DNS requires them. Existing DoH name/port/path settings display as a URL without rewriting saved fields, replacing custom values, or introducing migration UI. Protocol drafts stay separate, including invalid URL text and bootstrap lists; an explicit Advanced reset clears both editors' inactive drafts. Only the selected type is applied through the existing save bar. Labels and validation errors stay concise. See [WARP exit DNS](../../docs/WARP_DNS.md).
- WARP 与直连 DNS 的 DoH 编辑统一为完整 HTTPS 地址，旧域名、端口与路径直接组合显示并保留。新 DoH/DoT 草稿预填 Cloudflare，切换类型分别保留草稿，高级设置重置后重新使用默认值；点击应用后才生效。
- Proxy addresses and ports are drafts until **Apply changes** succeeds. The page has no DNS mode picker or DNS address editor: new configurations use remote resolution through the current exit (WARP DNS for ordinary connections, final-exit DNS for chains). Existing DNS settings remain compatible internally; the page has no DNS migration status or restore action. Listener edits preserve the latest confirmed DNS mode and addresses. Existing local-DNS risk warnings still reflect the saved configuration. Credentials use their separate tonal **Save username and password** action and never enter the network draft. The apply bar, aligned with the 880-pixel form column, appears only while there are edits, a save in progress, or a result to report.
- 代理页不再提供 DNS 方式、地址编辑、迁移提示或恢复操作。默认通过当前出口解析；旧配置由底层兼容，监听地址修改不会改写 DNS 配置。
- Advanced settings have an apply bar, aligned with the 880-pixel form column, that appears only while there are edits, a save in progress, or a result to report, and a back-navigation guard for unapplied edits. Reset loads defaults into the draft; it does not apply them immediately. Sections follow topic order: Routing & protection (local network, Disable QUIC), then an untitled caution, WARP DNS, Direct DNS, Endpoint (Automatic/Custom, addresses, port, SNI, endpoint IP version) and Transport (protocol, congestion control beside MTU). Explanations use muted hint text under their controls; the four dropdowns use the typed-value style through `FieldDropdown` so a picker beside a text field keeps the same height. Validation focuses the first invalid field in page order.
- 高级网络设置按主题排列：路由与保护、风险提示、WARP DNS、直连 DNS、端点（含端点 IP 偏好）、传输协议（拥塞控制与 MTU 并排）。说明文字统一为灰色提示，校验时按页面顺序聚焦第一个错误字段。
- Endpoint selection defaults to **Automatic** for new installations and staged resets. **Custom** retains manually saved IPv4/IPv6 addresses; switching the picker keeps address drafts. Automatic saves retain the confirmed custom pair, while port and SNI remain editable (L4 keeps its identity-derived SNI). Existing configurations migrate to Custom; Zero Trust keeps account-specific manual addresses: use **Edit Zero Trust endpoints**, read the red fullscreen warning and acknowledge the risks and access authorization before editing. Each page visit requires confirmation. Reset stages registered addresses and sign-in restores them, clearing the override. See [automatic endpoints](../../docs/NETWORK_SETTINGS.md#automatic-endpoints--自动选择端点).
- Custom → **Built-in presets** offers United States, Germany and Finland with a shared country and automatic matching to the authenticated account. The compact picker shows the country and a short IPv6-only hint; incompatible IP policies open a two-choice IPv6 dialog. Prefer IPv6 can fall back to the retained IPv4 after retryable failures, with one short status message. Presets resolve on each connection and do not show manual ZT risk warnings; manual editing and actual manually overridden ZT IPv4 fallback retain those warnings. See [built-in presets](../../docs/NETWORK_SETTINGS.md#built-in-ipv6-presets--内置-ipv6-预设).
- 自定义端点的“内置预设”提供美国、德国、芬兰，国家共享，按已验证的账号自动匹配。界面显示国家和“仅使用 IPv6 端点”简短说明；选择时按需引导 IPv6 策略，优先 IPv6 可回退原 IPv4。内置预设免手动端点风险确认，手动填写及实际手动 ZT IPv4 回退仍沿用风险提示。
- **Disable QUIC** under Advanced network settings → Routing & protection blocks application UDP/443 only on proxy/tunnel paths, with GEO direct traffic exempt. It defaults off and applies without reconnecting an established session. **禁用 QUIC** 默认关闭，应用后无需重连，所有 GEO 直连连接保持可用。 See [network settings](../../docs/NETWORK_SETTINGS.md#disable-application-quic--禁用应用-quic) for scope and deferred application.
- HTTP/SOCKS5 chain exits automatically prefer TCP for web traffic. The existing QUIC control displays its effective enabled state and is read-only with a short managed-by-connection hint; saved manual preferences and drafts remain intact and return after leaving these exits. Other UDP, direct routes and the WARP underlay retain their existing behavior.
- Inline output statuses distinguish enabled configuration from observed runtime state, including unavailable, limited, and failed states. Labels and icons carry the meaning without decorative badge surfaces.
- Unconfirmed network-settings feedback explicitly says that the app cannot confirm whether the settings have taken effect. Home and affected settings pages offer **Refresh status** without repeating the save or reconnecting. While visible, the app retries confirmation reads at most three times, after 1, 2 and 4 seconds; overlapping refreshes share one read. Continued uncertainty shows guidance to fully quit, reopen and check settings. A matching durable save reply confirms that operation even if newer events already superseded its snapshot; it never replaces the newer configuration. Recent operation receipts also recover a lost save reply after correcting an endpoint, so an acknowledged earlier save no longer leaves a warning or locks Home controls once the new settings work. 网络设置结果未确认时，首页和相关设置页提供“刷新状态”，不会重复保存或重新连接。界面可见时按 1、2、4 秒间隔最多补查三次，仍无法确认时提示退出并重新打开后核对设置；晚到的保存成功回复可确认其自身操作，但不会覆盖更新的设置。修改端点后也可通过近期保存回执补回丢失的确认，避免旧保存已确认但提示仍残留、首页开关仍被锁住。
- Chain proxy shares its heading, enable switch, live connection section and apply bar across OpenVPN, WireGuard, WARP via WireGuard, Proton VPN, VPN Gate, HTTP and SOCKS5. The live connection section follows the switch, before the source picker, so failures and missing DNS stay in view whichever source is open; its label/value facts stay within a 520-pixel reading width. Sources are grouped as **Your configurations** (OpenVPN, WireGuard, HTTP, SOCKS5) and **Services** (Proton VPN, WARP via WireGuard, VPN Gate); each source has its own icon, with the maintainer-supplied Proton mark and Lucide server/cable icons for HTTP/SOCKS5. Below 600 logical pixels, the source heading stays on its own line above a compact picker that matches the selected choice's size; its trailing chevron opens a bottom sheet with both groups. Wider layouts put each group on one row with the group names in a shared column, stacking at large text; the selected choice carries its check after its name, never over its icon. Source content contains VPN import/paste actions or a proxy form with saved configurations; VPN Gate provides refresh beside its heading, filters and public nodes. Both lists share single-selection rows and markers: the configuration used by the running session shows its stage (connected, connecting or failed, never connected after a failure) and the saved choice is marked unless that session is live. Choosing a row also turns the page switch on in the draft. Each configuration's menu holds **Technical details**, **Clear selection** for the selected row, rename, credentials and delete; delete stays visible but disabled with its reason while a configuration is selected, saved or in use. An empty library shows one empty-state block, and its first add action is tonal. Pasted text is checked after typing pauses, a check made stale by later edits is discarded, and text that belongs to the other VPN source offers **Switch to …**, which reopens it there. **Save and select** stages a new import or proxy as the pending choice without connecting. Switching sources never prompts: drafts are retained until settings are applied, and only leaving the page asks about unapplied edits. The apply bar appears with the first edit, while saving, or when a result or unconfirmed status needs reporting; it names the pending selection without a redundant unsaved-changes line, explains blocked drafts, and keeps VPN Gate preparation cancellable. Chain copy is localized in every catalog. WARP via WireGuard adds configuration generation and endpoint override drafts, shown only once a configuration is selected; see [the guide](../../docs/WARP_WIREGUARD.md).
- HTTP/SOCKS Add proxy → DNS offers Automatic (DoH by default), Encrypted DNS · Cloudflare, and DNS over TCP. Automatic retains explicit DNS; DoH uses verified TLS/HTTP/2 through the final exit and does not switch exits on failure. Custom DNS drafts survive picker changes; saving DoH omits them. Engines without encrypted chain DNS expose TCP only. HTTP/SOCKS 的 DNS 默认经最终出口使用 DoH，显式自定义 DNS 保留 TCP；失败不绕过链出口。
- HTTP/SOCKS scope explanations stay in the chain editor's current-connection section, including proxy-only limits, retained explicit bypasses and terminal failure with an explicitly inactive Kill Switch. Home omits these paragraphs. The phone's compact chain summary keeps its WARP status row present across sparse refreshes, using observed WARP state or known connection stages and a dash when unknown. Android system-blocking advice in the editor is conditional; successful UDP association remains distinct from verified forwarding. Saved drafts never establish current scope. 首页不再显示长篇范围提示；手机链式状态栏保留 WARP 状态，缺少独立上报时按已确认的连接阶段显示，未知时显示横线。
- File imports support native multi-selection on Windows and Android: up to 128 files, each at most 128 KiB. Batches use offline per-file checks, editable names and individual credentials before explicitly saving valid entries. Partial successes remain saved; interrupted writes require inspecting the library before reimporting. Single-file and pasted-text workflows remain available. Windows 和 Android 支持批量导入：逐项离线检查，补充名称与凭据后统一保存合格项；通信中断时先核对配置库再重试。See [chain proxy](../../docs/CHAIN_PROXY.md).
- Home groups the session, the outputs and the traffic in `Panel` cards on the canvas; risk and error banners stay above them. On desktop the session card holds the connection ring beside the active account, the status headline with a lamp in the phase colour, the observed exit location, protocol, IP version and duration, and the Kill Switch line, with **Connection details** folded along its bottom edge. The outputs card beside it holds the TUN, system-proxy and chain-proxy switches with **Local proxies** at its foot, and both cards share one height. Narrow content or text above 150% stacks the cards. Windows never uses the phone layout: a window narrower than 760 logical pixels stacks the same desktop cards. Each switch row has an icon tile that takes the brand tint while its output is on, its title and hint in one text column, and a standard-size switch centred on the row. The TUN and system-proxy switches carry a plain-text hint describing what they do; while the HTTP local proxy is off, the system-proxy hint states that it must be enabled first. The traffic card draws download and upload over one shared scale whose top guide names a rounded rate; curves are smoothed without overshooting a sample, and the latest reading is marked. The desktop chart is at least 96 logical pixels tall, and the card takes the window height left below the other cards, up to 420, so Home ends at the page's bottom margin; shorter windows keep the minimum and scroll. The height never depends on the connection state: without a reading, the card's note sits inside the empty chart, and disconnected rates show a dash. Rates ease between samples unless reduced motion is requested. The chain-proxy row's title and summary open the existing chain-proxy editor; the summary names the source and its live or saved choice (a VPN Gate node with its flag, or a configuration name) with the same status wording as the Proxy page, and shows guidance until an exit is chosen. These shortcuts save shared network settings through the existing apply lifecycle; enabling system proxy requires the HTTP local proxy. **Local proxies** keeps HTTP/SOCKS5 listener addresses and **Manage proxies** opens the existing proxy page. Connected sessions show exit region, duration, protocol and IP version; desktop Location shows only the observed country, with its bundled flag before the name and a separate location icon. Phones put the active account in the page heading, where it opens Accounts, centre the ring with the status, exit and session clock under it, and follow with the traffic card and a status card. The status card holds the Kill Switch, the protocol while connected or the configured outputs while idle or failed, and **Connection details**. On desktop and phones, **Connection details** groups selectable IPv4/IPv6 addresses after **Exit IP:** and shows a plain **Enabled:** list of configured interfaces, without IP/interface icons, location or runtime status labels. Missing address families take no space; a dash appears only when no exit IP is available or the session is disconnected. Every interface label and related settings heading uses the same platform-aware name: desktop English uses **TUN**, Chinese uses **虚拟网卡** (Traditional Chinese **虛擬網卡**), and other locales use their translated adapter name; Android keeps **VPN**. Chain progress continues to drive the main connection status and ring. Desktop adds no extra block above the main connection area when a chain is enabled. Phones retain the chain summary as a card above the connection control that opens its settings: **WARP → source**, the chosen exit and the WARP hop. The ring's headline is its only overall status. Desktop and mobile Home omit network-quality and diagnostic shortcuts; their existing settings and navigation entries remain available. The one exception is a failed connection: its error banner offers **Start diagnostics**, which opens Diagnostics inside Settings (so Back returns to the Settings list) and starts a Standard run; errors from other operations in the same slot get no action. While disconnected, an enabled Kill Switch reads **Engages when connected**; **Inactive** is kept for a session that is up or failed without protection. A connected session whose exit has not been detected yet reads **Not detected yet** rather than **Not available**.
- Desktop and mobile traces share the timestamped 60-second quality history. New samples with unchanged values are retained; repaint timers do not generate observations. Source-aligned time slots tolerate scheduling jitter, and averages use actual elapsed time. Missing samples stay gaps; delayed, paused, unavailable, and disconnected readings are identified explicitly. The view does not start additional probes or persist traffic history.
- Settings groups connection/protection (auto-connect, Kill Switch, Android Always-on, Advanced), proxy/routing (bypass, Android per-app proxy), tools (network quality, diagnostics), and application preferences. Application preferences are plain rows (theme, language, startup, Windows tray, update checks, **About Usque**) without nested subheadings; the update actions show the installed version. Theme is chosen only here; the navigation rail holds the destinations and, when extended, the brand. About lists the version, the non-affiliation and trademark notices, Cloudflare's application terms and privacy policies, the source code, the bundled open-source licenses, collapsed technical details (the IPC API) and the isolated **Clear all data** panel. Diagnostics is titled **Diagnostics** and does not repeat this information. The Kill Switch toggle saves the preference directly through the shared network-settings lifecycle, with platform-specific help and a disabled control while saving or while network shortcuts are locked. Advanced no longer edits or resets that preference. Settings has no apply bar of its own, so it shows only network-settings problems (failed, unconfirmed or unknown results) above the groups, with a reconnect action when available; success messages stay with the page that made the change.
- The Proxy section's output group holds every immediate output switch: TUN (VPN on Android), SOCKS5, HTTP and, on Windows, system proxy. Settings does not repeat them. TUN and system proxy reuse the Home hints and names; system proxy requires the HTTP local proxy to turn on but can always be turned off, as on Home. Listener sections keep their capability summary when switched off; the status beside the title reports that state.
- The Proxy page lists each output once. **Device traffic** holds the immediate TUN (VPN on Android) and, on desktop, system-proxy switches. The SOCKS5 and HTTP sections are headed by their own immediate switch, with the listener status below it and the address/port draft underneath. The **Chain proxy** section uses an open heading: the status at the heading's trailing edge (below the title on narrow layouts), the source and choice in the heading's text column and a **Manage** action that opens the editor. Every source, including VPN Gate, reads through one summary and the chain status wording; resolving a saved choice reads the local catalogue or library once while the page is visible.
- 首页改用卡片分组：桌面端会话卡（连接环、账号、状态、出口、协议与 Kill Switch，底部折叠连接详情）与输出卡（虚拟网卡、系统代理、链式代理开关及本地代理）等高并排，流量卡在下方填满剩余高度，上下行共用一个刻度；链式代理行显示来源和当前或已保存的出口。手机端账号移到页头，连接环下方为状态、出口与连接时长，随后是流量卡和状态卡。Windows 窄窗口改为堆叠桌面卡片。代理页的链式代理与监听分区使用同一种开放标题，所有来源共用同一摘要和状态用词。账号页与代理、设置页同宽，连接中切换账号会先确认。设置新增“关于 Usque”，汇总版本、声明、Cloudflare 条款与隐私政策、源代码和开源许可。
- Proxy and Settings subpages open inside their section. Desktop keeps the navigation rail visible; phones hide the bottom bar until the subpage closes. Choosing another section, or the current one again, returns to the section root and first asks about unapplied edits; declining keeps the subpage open.

- The Windows tray icon adds a status dot: amber while connecting or reconnecting, green when connected, red on error, none when disconnected. The tray menu has checked TUN and system-proxy items that save through the same apply lifecycle and locks as the Home switches; system proxy is greyed out while the HTTP local proxy is off.
- While the window is hidden or in the background, Windows shows a notification when a session stays in reconnecting for more than five seconds, when a connection ends in an error, and when an announced interruption recovers. A short reconnect, a user disconnect and the first status after start-up stay silent. The text is fixed copy that names an active Kill Switch; engine error details stay inside the app. Notifications respect Windows quiet hours, and clicking one opens the window.
- Windows reports initial, hidden and minimized window visibility to Flutter. Hidden windows pause quality/history reads, diagnostics refresh timers, UI selectors and animation tickers. Connection/error events, tray commands and settings confirmations remain active. Restoring the window refreshes current state without replaying a mutation. The native event reader waits for data or cancellation with overlapped I/O instead of polling an idle pipe. Fake-engine and IPC fixtures cover this lifecycle; controlled CPU measurements remain separate.
- Windows 窗口隐藏或最小化时暂停质量历史查询、诊断刷新、界面重建和动画；连接／错误事件、托盘操作与设置确认继续运行。恢复窗口会重新读取状态，不重放保存或连接操作。原生事件管道以可取消的异步读取等待数据，避免空闲轮询；实际 CPU 改善仍须独立测量。
- Windows opens centred on the monitor under the pointer the first time, then restores the last position, size and maximized state if that monitor is still attached. The frame is stored under `HKCU\Software\io.github.georgexie2333\Usque` next to the close-to-tray preference and holds no account or network data.
- Windows keyboard shortcuts: **Ctrl+1** to **Ctrl+4** select Home, Profiles, Proxy and Settings; **Esc** (outside a text field), **Alt+Left** and the mouse back button leave a subpage through its unapplied-edits prompt; **Ctrl+S** applies the visible apply bar; **F5** refreshes the VPN Gate list and the diagnostics timeline. Shortcuts are inactive while a dialog or popup is open and on Android.
- Windows 托盘图标带状态圆点，托盘菜单可直接切换虚拟网卡和系统代理；窗口在后台时，持续 5 秒以上的重连、连接错误以及之后的恢复会弹出系统通知。窗口首次居中打开，之后恢复上次的位置、大小与最大化状态。快捷键：Ctrl+1～4 切换页面，Esc／Alt+←／鼠标后退键返回，Ctrl+S 应用修改，F5 刷新。

- **Bypass settings** combines a custom DIRECT/REJECT/PROXY rule list, an independent Ads switch, and country direct presets. Rows are not numbered, because specificity rather than order decides precedence; each shows its target and a localized action status (**Direct**, **Block**, **Through Usque**). The editor supports individual edits and action-scoped bulk paste. A rejected or conflicting row shows the reason under it and the apply bar names affected rules by target; overlapping rows with different actions each state that the more specific rule wins. It uses the shared apply bar, shown only while there is something to apply or report, and leave guard. The Ads state reads as a status: active in green, not ready with a warning. Ads availability and the current session revision are distinct; complete verified rules update the running session automatically. See [routing and Ads](../../docs/ROUTING.md).
- Country presets become available with verified GeoIP data. GeoSite country categories add domain matching when present; countries without them still match IP-address targets while hostname targets retain the tunnel route. CN retains its checksummed country artifact; other countries use extracted data from the checksummed shared aggregate. See the same routing guide for bounds and cache validation.
- 国家预设下载有效 GeoIP 后即可启用；GeoSite 国家分类为可选，没有分类时仍按 IP 目标匹配，域名目标沿用隧道路由。CN 保留带校验的单国数据，其他国家从已校验的共用总库提取。

Home shows a non-dismissible risk banner above the connection controls when the
selected account has custom Zero Trust IPv4/IPv6 addresses or a still-active ZT
session uses them. It reads saved/confirmed state, not editor drafts, and remains
visible during a deferred reset until that session stops or registered addresses
are applied. Ordinary WARP and unchanged registration addresses have no banner.

## Proton accounts and server selection

Connection failures preserve Proton as the source and distinguish authorization,
certificate recovery, restricted servers, session limits, repeated keys, policy,
API and transport errors. The Home banner and chain editor share these messages;
bounded numeric API/HTTP or agent context is separate from remote descriptions.
Proton failures never direct users to recreate a WARP account. Recovery keeps
business traffic blocked until the agent authorizes the new connection.

Proton 连接错误在首页和链式代理页使用一致的来源与分类，不再引导重置 WARP；
只显示允许公开的数字上下文。恢复期间保持业务流量阻断，直到 agent 重新授权。

The Proton source shows one row per authenticated user under **Proton accounts**,
grouping legacy duplicate profiles. Select an account separately from its
searchable server preference, which is a **Server** row under the account;
With a supporting Engine, Automatic and manual logical-server choices are
staged with the account in the shared apply bar. While connected, choose a new
server and use **Apply and reconnect** to replace the Proton exit through the
existing protected connection lifecycle. Closing the picker or discarding the
draft does not change the running server. Older Engines retain next-connection
preferences and require disconnection before editing the current account.
The server picker shows bundled country flags beside country codes and keeps its
draggable scrollbar visible while the list overflows.
Refresh, sign-in renewal and removal sit in the account's menu; once an account
exists, guest setup and sign-in move under **Add account**. Setup and job errors
use the shared warning banner. The
native layer retains the complete eligible directory and exposes revision-bound
128-entry metadata pages. Physical entries and session credentials stay private.
Connection resolves a fresh peer and obtains or renews its session certificate
only when needed. Removal offers remote sign-out or explicit local-only forgetting,
clears a saved selection first, and protects the running account. See the
[Proton guide](../../docs/PROTON_VPN.md) for behavior and validation limits.

Proton 来源在“Proton 账号”下按账号显示，旧版重复记录合并为一行；点击账号下的
“节点”行可搜索并选择完整可用逻辑节点。支持的新引擎将账号和节点暂存为待应用选择；
连接中可选择其他节点或“自动”，再点击“应用并重连”，通过现有保护流程切换出口。
关闭节点列表或放弃未应用更改不会影响当前连接；旧引擎保留下次连接生效的行为。
节点国家代码前显示内置国旗；列表超出可见范围时，滚动条常驻并支持拖动。
刷新、重新登录和移除位于账号菜单；已有账号后，游客设置和登录收进“添加账号”。连接时解析当前入口并按需获取证书。移除账号明确区分联网退出
和仅本机遗忘，先清除已保存选择，并保护正在使用的账号。

## Native UI composition

- `ContentSection` and `ContentHeading` group ordinary content with typography and spacing, not a card background. `ContentList` separates adjacent entries; `ActionRow` provides native ink, keyboard/D-pad activation and a visible, layout-stable focus outline. The connection ring's power button marks keyboard/D-pad focus with a primary halo separated from its fill by a gap, plus a slight lift, so focus stays visible on a primary button on Android TV; both are paint-only. `LinkRow` builds Settings navigation rows on `ActionRow`: the chevron always stays at the trailing edge (rows that open the system browser set `external` and show an external-link glyph there instead), and below 480 logical pixels or above 150% text a row value or `status` (such as the Proxy chain row's `InlineStatus`) moves under its summary; `details` carries rich summary text. `RowTileTheme` gives Material list tiles the same 20-pixel icons, 12-pixel gap and title style, so switch and navigation rows share one text column; Settings, Proxy outputs, Advanced protection, Bypass Ads and the chain-proxy enable switch use it. `HintText` is the muted style for explanations under a control. `FieldDropdown` gives dropdowns the typed-value style and a text field's height at any text scale; Advanced, the chain-proxy DNS picker, the routing-rule dialog and the VPN Gate country filter use it. Expanders inside a section use zero tile padding so their titles share the section edge, and section-level apply bars align with the 880-pixel form column. `InlineStatus` pairs a readable state label with a supplementary icon.
- Accounts are continuous rows in the same 880-pixel column as Proxy and Settings, so section headings never move between them. Each row starts with a tile for the account's kind (WARP+, WARP Free or Zero Trust), brand-tinted while selected, then the name above one line holding the kind in words (Zero Trust adds its organization; product names keep left-to-right order and still start on the reading side in right-to-left text) and the tags: **Selected** for the saved choice and **Connected** only on the account a runtime-confirmed connection uses. The page has no separate summary; an uncertain or failed scene result offers refresh from its banner, and while an unconfirmed read may have replaced the saved choice no row claims it. The selected account also has the light selection wash and start marker, and clicking the row itself never switches accounts. A row has at most one inline action, **Set active** on an unselected account: beside the block on wide rows, and under the text starting on its edge below 620 logical pixels or above 150% text. Identity and license (identity setup while a credential is missing or damaged), rename and delete are in a labelled menu at every width, with delete last after a divider in the danger colour; the only account's delete entry is disabled and names why. Normal flows always provision an identity, so a missing or damaged one appears only as a warning in place of the kind. Delete always confirms. Setting another account active while a session is connected or connecting first confirms that the connection will reconnect with it. Account rows show the account's kind; the layers glyph marks scenes.
- The scene editor's **WARP account** row names the account that **Use the current account** follows. Its page is a single-choice list in the VPN Gate pick grammar: a dot or empty circle leads each option (**Use the current account**, then each account with its kind), with checked semantics in one mutually exclusive group. Activating an option picks it and returns; arrow keys and the D-pad only move focus, so browsing never picks, and picking the current option leaves the draft unchanged. It offers **Add account** but no other account management.
- Scene rows follow the same grammar: the selected scene has the selection wash, start marker and a **Selected** tag; its name sits above any problem tags and a quieter line of facts that sets it apart (exit, endpoint, a non-default transport or IP version, and a bound account; defaults stay unstated), and clicking the row never uses the scene. Labelled facts go through the localized `scenario_fact` template rather than a hard-coded colon, so Chinese and Japanese use a full-width colon and French a spaced one; the status card's readouts set label and value apart by style and wrap the value under a long label. The selected row offers **Use scene** again only while its template, binding or result has drifted, failed or is unconfirmed. Wide rows wrap extra actions instead of narrowing the name; the row menu (duplicate, update, delete) gives each entry an icon and puts delete last, after a divider, in the danger colour. Above the list, a `Panel` status card shows the selected scene in `headlineSmall` with its Engine state (no tag while it is only selected), template or binding drift, the selected account and the confirmed current connection, with refresh beside them. A modified selection offers **Update from current settings** (tonal) and **Save as a new scene** there, a restore point adds **Restore previous settings**, and any selection offers **Switch to Custom** last, which previews Usque's default scene settings before clearing the selection. While disconnected, the card adds no state tag for a selection that is only waiting for the next connection. The list heading shows the scene count; an empty library shows its only create action inside its empty state, with the device-wide scope note. Accounts has no summary: its rows carry the selected and the connected account. The scene editor states that scope once as its subtitle, then groups name, account binding, chain proxy (its switch on `RowTileTheme` so it shares the link rows' icon and text columns), endpoints and transport; account choice and chain management open as nested pages without discarding the draft.
- Status tones have one meaning app-wide. Brand marks a saved choice or a step in progress (selected, saved selection, connecting); neutral states a fact or a wait; success is reserved for a state the Engine confirmed (applied, connected); caution asks for attention (modified, unconfirmed, missing dependency); danger reports a failure. A saved selection never uses success or a check mark, and a pick in a list (such as a VPN Gate node) uses a radio dot, not a check. `InlineStatus` defaults to a dot for brand; pass a specific glyph when the state has a better one.
- Row actions read and focus in one order: the primary action, then other labelled actions, then contextual ones, then a menu last. Keep an editing action directly before the primary use action so D-pad focus moves between them, and keep every action reachable without a tooltip.
- `SaveChangesBar` defaults to apply wording. A form whose save does not apply anything (such as a scene template) supplies its own unsaved status, and may add one lower-emphasis `secondaryAction` (such as **Save and use**) before the primary button.
- Destructive confirmations (delete, remove, sign out and remove, clear data) set `UsqueDialog.danger` and style their confirm button with `dangerButtonStyle`.
- Change previews read top-down: what applying does (stays disconnected, new connections only, may reconnect, unconfirmed), then safety warnings such as Kill Switch, then the account change, then each field as old → new, then other saved settings that also take effect.
- `SegmentedButton` keeps its check mark as a value picker. Profiles puts **Manage accounts** and then **New scene** in its page heading, following the standard responsive page actions, as Accounts does with **Add account**.
- Accounts, Settings and proxy editors use open groups with a maximum form width of 880 logical pixels. Settings and Proxy share one row grammar: `ContentGroup` puts a quiet `titleSmall` label (and an optional hint) over a `ContentList`, and every row uses `RowInsets` (an 8-pixel inset), so icons, titles, switches and chevrons stay in the same columns when switching between the two pages. Content under a row's title, such as a listener's status, starts at `RowInsets.textIndent`. Proxy groups are Exit (the chain-proxy `LinkRow`), Device traffic, Local proxies (each listener's switch, status and addresses) and proxy authentication. Pages opened from Settings follow the same grammar: `FormGroup` is a `ContentGroup` with one inset form entry, used by Advanced network settings (Routing & protection switch rows, then WARP DNS, Direct DNS, Endpoint and Transport) and Bypass (custom routing rules as inset rows with their actions last, Routing & protection for Ads, and Countries / regions with data status and the country list as separate entries). Per-app proxy keeps its rows and app list on the same inset. `DirectDnsEditor` and `RoutingRulesEditor` take `grouped: true` there. Scenes inherit these global settings. Network quality and Diagnostics remain dashboards on the 1120-pixel column. Home remains capped at 1120. Navigation retains its existing 760/1050 breakpoints and phone/rail behavior.
- Network quality uses continuous metric sections rather than a card grid. It leads with the overall level in display size and the latest round trip and loss. Congestion, path MTU, migration, direct DNS and queue metrics are folded under **Advanced metrics**; the group opens on first view and shows a count when one of its phases is degraded, with **Start diagnostics** beside it, and phase states use `InlineStatus`. **Run Network Doctor** is the primary action only while the level is fair or poor. Each section follows only the values it shows (`ListenableSelector`), so a new sample repaints the trends rather than the whole page. Diagnostics keeps its checks, timeline and export distinct. A completed run concludes with either **No problems found** or the number of problems and the two most severe with their advice, each scrolling to its check. Results list the most severe category first; failed and warning checks open with their explanation and show a readable failure title (the code stays in the detail card), and categories whose checks all passed fold to one line. Only counts that occurred are shown. The destructive **Clear all data** panel lives in About, where a failed wipe is reported inside the panel. Dialog field groups and onboarding forms do not nest card surfaces.
- Background fills are reserved for controls, selection, warnings, dialogs and save bars. Use `Panel` for explicitly isolated danger areas; do not make every panel transparent. Home groups content in `Panel` cards (its session, outputs and traffic, and the phone chain summary), and Profiles puts its selected-scene status in one; other pages keep open sections. Home spacing uses the `UsqueSpace` 4-pixel steps. Preserve the connection ring's drawing, proportions, phase mapping and motion when editing surrounding layouts.
- Use the existing brand colors, bundled fonts and locale fallbacks. Primary controls have a minimum 48-pixel target; status indicators do not rely on color alone. Layout code must use the existing network observations without creating probes, sampling timers or history persistence.

The workflow and native-layout widget tests use a fake engine and cover connected detail expansion, repeated collapse/restore, selectable-value copying, action-row focus and keyboard/D-pad activation, status-label contrast, narrow/landscape layouts, 200% text, and reduced motion. Bitmap tests use the `golden` tag and exact comparison with the pinned Flutter SDK. Existing Windows screenshots cover accounts, settings, proxy editors, onboarding, diagnostics, dialogs and expanded home details.

The independent `linux-golden` suite exercises Linux UI branches with a fake
engine and [fixed test font fixtures](test/fonts/README.md). Its images live in `test/goldens/linux/`
and are compared on Linux amd64; it is also tagged `golden`. Run
`flutter test --no-pub --tags linux-golden`, or use `bash tool/dev.sh check flutter`
from the repository root. See [Contributing](../../CONTRIBUTING.md) for scoped
baseline regeneration. Linux images do not replace the Windows suite or establish
arm64 rendering, native GNOME/KDE, X11/Wayland, tray or networking behavior.

Linux golden 使用独立目录、固定测试字体和模拟引擎，在 amd64 上精确比较 Linux
界面分支；Windows 截图仍单独验证。两套截图均不启动 VPN，也不证明原生桌面与
网络行为正确。

`test/readme_assets_test.dart` renders the root README images from the same
fixtures and fonts. It is tagged `readme-assets`, writes files only when
`USQUE_README_ASSETS` is set, and is not a comparison. Use
`bash tool/dev.sh readme-assets`; see [Linux development](../../docs/LINUX_DEVELOPMENT.md#readme-images--readme-图片).

---

Cloudflare and WARP are trademarks and/or registered trademarks of Cloudflare, Inc. in the United States and other jurisdictions.
