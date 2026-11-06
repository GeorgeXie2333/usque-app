# Profiles and scenes / 配置与场景

Open **Profiles** to save and reuse a local network setup. The page opens on the
selected-scene status card above the scene list; **Manage accounts** opens its
**Accounts** subpage. Home's account
entry and the account launcher open that subpage directly. Home's scene entry,
ordinary navigation and Ctrl+2 open the Profiles root. Back from Accounts returns
to Profiles, retaining the scene list's scroll position. Desktop navigation stays
visible; phones hide bottom navigation on subpages. Leaving an editor first checks
its unsaved changes, including when opening Accounts from another entry.
The page heading holds **Manage accounts** and **New scene**. A status card at
the top shows the selected scene with its state (for example **Modified** or
**Applied**), the selected account and, while a session exists, the confirmed
current connection, with a refresh button. When the current settings differ
from the selected scene, the card offers **Update from current settings** and
**Save as a new scene**; after a switch it also offers **Restore previous
settings**. While a scene is selected, **Switch to Custom** leaves it. Each scene row lists what sets it apart: the exit, the endpoint,
a non-default transport or IP version, and a bound account. The selected row
offers **Use scene** again only when using it would change something. An empty
library shows **New scene** in its empty state. Engines without scenes continue
to show account management at the root.

在 **配置** 首页保存和使用本机网络场景，通过 **账号管理** 打开 **账号** 二级页。
首页账号入口和账号启动入口直达账号页；首页场景入口、普通导航和 Ctrl+2 均打开
配置首页。账号页返回配置首页并保留场景列表的滚动位置。桌面保留侧栏，手机在
子页面隐藏底部导航。离开编辑器时先检查未保存修改，从其他入口打开账号页也遵循
同一保护。页头提供 **账号管理** 和 **新建场景**。顶部状态卡显示已选场景及其状态
（如 **已修改**、**已应用**）、已选账号；存在会话时还显示已确认的当前连接，旁边
提供刷新按钮。当前设置与所选场景不同时，状态卡提供 **用当前设置更新** 和
**另存为新场景**；切换后还提供 **恢复切换前配置**。已选场景时可用 **切换为自定义**
取消选择。每个场景行列出其区别：出口、
端点、非默认的传输协议或 IP 偏好，以及绑定账号。已选场景只有在再次使用会产生
变化时才显示 **使用场景**。场景库为空时，**新建场景** 位于空状态中。不支持场景的
引擎继续在配置首页提供账号管理。

The Accounts page supports account creation, identity setup, renaming, licenses
and deletion. Each row shows the account's kind (WARP+, WARP Free, or Zero Trust
with its organization) and at most one inline action: **Set active** on another
account, which confirms reconnection while connected or connecting. Identity and
license, rename and delete are in the row menu. The selected account carries
**Selected**; the account a confirmed connection uses carries **Connected**.
Failures and unconfirmed results stay visible on this page, and an uncertain or
failed scene result offers a refresh there.

账号页支持新建、身份设置、重命名、许可证和删除。每行显示账号类型（WARP+、
WARP Free，或带组织名的 Zero Trust），最多一个行内操作：其他账号上的
**设为当前**，连接中或正在连接时会先确认重连。身份与许可证、重命名和删除位于
行菜单。已选账号标有 **已选择**，已确认连接所用的账号标有 **已连接**。失败及
结果未确认提示直接显示在账号页，场景结果未确认或失败时可在提示中刷新。

## Create and edit / 创建与编辑

1. Choose **New scene** to capture the current confirmed saved network settings.
2. Name the scene. Open **WARP account** and pick **Use the current account**
   (which names the account it follows) or a specific account; the pick returns
   to the editor at once.
3. Enable **Chain proxy** if needed, then open its subpage to choose or prepare
   an exit. Edit endpoints, IP policy and transport preferences in the scene.
4. While the scene has unsaved edits, the bottom bar offers **Save scene** and
   **Save and use**. **Save scene** updates the template without changing the
   running connection. **Save and use** saves it and opens a separate change
   preview; cancelling that preview keeps the saved template.

选择 **新建场景**，填写名称；进入 **WARP 账号** 子页面，选择 **沿用当前账号**
（会注明当前跟随的账号）或指定账号，选择后立即返回编辑器。按需开启 **链式代理**
并进入子页面选择或准备出口。
编辑端点、IP 策略及传输协议后，底部栏提供
**保存场景** 与 **保存并使用**：**保存场景** 只保存模板；
**保存并使用** 先保存，再显示切换预览。取消预览不会撤销模板保存。
离开尚未保存的编辑器时，可以继续编辑或放弃修改。

Scenes contain optional account binding, the chain-proxy switch and exit
reference, Proton server preferences, transport and IP-family preferences, and
endpoint selection, presets, addresses, port and SNI. VPN/TUN, SOCKS5, HTTP and
system-proxy switches, LAN access, ad blocking, custom routing, country direct
choices, and WARP, direct and local-proxy DNS remain device settings. Listener
addresses and credentials, Kill Switch, auto-connect, Android per-app selection
and advanced transport tuning also remain device settings.
Zero Trust registered endpoints remain account-owned; applying
a manual scene updates only the target account's override, just like Advanced.
The selected chain exit retains its own final DNS configuration.

场景保存可选账号绑定、链式代理开关及出口引用、Proton 节点偏好、传输协议、IP 策略
和端点。VPN/TUN、SOCKS5、HTTP、系统代理开关，局域网访问、广告拦截、自定义分流、
国家／地区直连，以及 WARP、直连、本地代理 DNS 全部沿用全局设置。监听地址与认证、
Kill Switch、自动连接、Android 按应用选择及高级调优也保持全局。
Zero Trust 注册端点由账号管理，手动场景只更新目标
账号的覆盖地址，与高级设置一致。链式出口的最终
DNS 沿用所选出口配置；保存场景本身不会修改出口库中的凭据或配置。

The **Endpoint** group uses the same controls as **Advanced network settings**:
Automatic or Custom, manual IPv4/IPv6 or a built-in IPv6 preset, port, SNI and
endpoint IP policy. Automatic and presets retain the previously saved manual
address pair. Presets require IPv6-only or prefer-IPv6; L4 shows its
identity-derived SNI read-only. Editing manual Zero Trust addresses requires
the existing fullscreen risk acknowledgement. Using a scene with a manual
Zero Trust override also requires acknowledgement before the change preview.
Automatic scenes inherit the target Zero Trust account's addresses; preset
scenes retain that account's IPv4 fallback. Neither changes Consumer addresses.
The preview includes endpoint differences and reconnect requirements, and
Restore covers endpoints without overwriting later endpoint edits.

**端点** 分组与 **高级网络设置** 共用控件：自动或自定义、手动 IPv4/IPv6 或内置
IPv6 预设、端口、SNI 与端点 IP 策略。自动与预设保留此前保存的手动地址；预设要求
仅 IPv6 或优先 IPv6，L4 的账号派生 SNI 只读。编辑 Zero Trust 手动地址仍需全屏风险
确认；使用包含手动 Zero Trust 覆盖地址的场景时，在切换预览前再次确认。自动场景
沿用目标 Zero Trust 账号的地址，预设沿用该账号的 IPv4 回退地址，均不改动 Consumer
地址。预览列出端点差异及重连要求，恢复也包括端点，但不会覆盖后续端点修改。

The chain-proxy subpage uses the same source, node library, import, generation
and sign-in controls as **Proxy → Chain proxy**. Choose **Use this selection**
to return to the scene draft. It does not apply the selected exit to the current
connection. The account subpage is a single-choice list: it can add an
account, but renaming, licenses, switching and deletion stay on **Accounts**.
Picking changes only the draft; arrow keys and the D-pad move between options
without picking, and Enter or select picks the focused one.
Going back preserves the parent scene draft, and unsaved child selections use
the normal leave guard. Cancelling a departure from the whole Profiles section
keeps the leave protection on every editor, including subsequent edits.

Creating, importing, editing or deleting a library resource or account is an
explicit management operation and persists independently of the scene. Discarding
the scene does not undo those operations. A selected prepared VPN Gate node is
retained while editing and becomes a scene reference when saved. After an engine
restart, the app prepares a temporary node again before accepting or saving it;
preparation failure leaves the scene draft available to retry.
Required global routing data must already be available before using a scene.
A referenced configuration is retained by
normal library cleanup. A missing dependency makes that scene unavailable,
without preventing other scenes or accounts from loading. The same applies to
saved scene settings that a newer release no longer accepts: the scene shows
that its settings are no longer valid and **Use scene** is unavailable until
you edit and save it again. A new binding must name an existing account.
链式代理子页面复用 **代理 → 链式代理** 的来源选择、节点库、导入、生成与登录功能，
选择 **使用此选择** 后返回场景草稿，不会应用到当前连接。账号子页面是单选列表，
可以添加账号，重命名、许可证、切换和删除仍在 **账号** 页完成。选择只修改草稿；
方向键和遥控器只移动焦点，不会直接选中，按确认键才选中当前项。返回会保留上层场景
草稿，子页面尚未确认的选择遵循离开保护。取消离开整个配置页面时，各层编辑器
仍保留离开保护，之后的新修改也会得到确认。

新建、导入、编辑或删除节点与账号是独立的资源管理操作，保存后立即进入对应库；
放弃场景不会撤销这些操作。已选且已准备的 VPN Gate 节点在编辑期间保留，保存场景
后转为场景引用。引擎重启后，临时节点会在确认选择或保存前重新准备；准备失败会
保留场景草稿，供稍后重试。使用场景前须准备全局分流所需数据。正常清理会保留场景引用；依赖失效只影响对应场景。
新版本不再接受的已保存场景设置同样只使该场景不可用，编辑并重新保存后即可使用。
新绑定必须指向已有账号。

## Use a scene / 使用场景

Choose **Use scene**, review the account and settings changes, then confirm.
The preview states first whether the connection stays disconnected, applies to
new connections or may reconnect, and shows any Kill Switch warning before the
changed fields. When the current settings differ from the selected scene, the
status card offers **Update from current settings**, which previews the changed
fields before saving them into the scene, and **Save as a new scene**.
The native owner submits the account and network settings together. When
disconnected, selecting a scene leaves the connection disconnected. While
connected, the preview identifies whether reconnection is needed. Hot routing
updates apply to new connections; existing streams can retain their old paths.
Reconnect when all connections need to adopt the new settings.

选择 **使用场景**，检查账号和设置变更后确认。预览先说明保持断开、对新连接生效
还是可能重连，并在变更字段前显示 Kill Switch 提示。当前设置与所选场景不同时，状态卡提供
**用当前设置更新**（保存前列出变更字段）和 **另存为新场景**。原生层一次提交账号和网络设置。
未连接时选择场景保持断开；连接中会按差异应用或重连。规则热更新对新连接生效，
已有连接可能保持原路径；需要统一切换时应重连。

On Android, a proxy-only session cannot silently obtain the system's first VPN
permission. If permission is required, disconnect, select the scene, and connect
using the normal system authorization flow.

Android 首次启用 VPN 需要系统授权。如果当前代理连接缺少该授权，请先断开、
选择场景，再通过正常连接流程授权。

The preview also lists saved device settings that differ from the running
connection and will take effect during the switch. If those saved settings turn
off VPN mode while Kill Switch is on, it warns that system-wide protection stops.

**Selected** and **Saved; waiting to apply** are not runtime success. The page
distinguishes confirmed application, failure and an unconfirmed result.
While disconnected, the card shows only the selected scene and any template
drift: the selection takes effect on the next connection, so there is no result
to wait for. A failed status read, an unresolved switch, or a recorded failed
or unconfirmed outcome still shows.
Refresh an uncertain result before deciding whether to retry; the app does not
automatically replay a lost activation request. A failed selected exit never
authorizes switching to direct access or another exit.

After a lost reply, Connect waits until the native owner settles the switch
from saved configuration. The page then reports that the scene was not
applied, or keeps **Result not confirmed** for a saved selection without a
receipt. Either way the app reads the saved account and settings back before
connecting. If the owner cannot read its storage, Connect stays unavailable and
asks you to refresh Scenes.

预览还列出已保存但尚未运行、会随本次切换生效的全局设置。若这些全局设置关闭 VPN
模式且 Kill Switch 开启，预览会提示切换后不再保护系统全部流量。

**已选择**、**已保存，待应用** 不代表当前连接已经采用设置。页面区分已确认应用、
失败与结果待确认。未连接时，状态卡只显示已选场景及模板差异：所选场景在下次连接时
生效，没有需要等待的结果。状态读取失败、切换尚未确定，或已记录的失败或待确认
结果仍会显示。结果不确定时先刷新，应用不会自动重放丢失回复的切换请求；
所选出口失败不会擅自改为直连或其他出口。

回复丢失后，连接会等待原生层根据已保存配置确定切换结果：页面显示场景未应用，
或对没有回执的已保存选择保持 **结果待确认**。两种情况都会先读回账号和设置再连接；
原生层无法读取存储时，连接保持不可用并提示刷新场景。

## Changes, binding and recovery / 修改、绑定与恢复

- Editing current scene-managed settings does not overwrite the scene template.
  **Modified** indicates a difference; save a new scene, update the template,
  or use the saved scene again.
- Changing outputs, routing or DNS does not mark a scene modified and is not
  reverted by applying or restoring a scene.
- Manually selecting another account is allowed for a bound scene. It reports
  that the account differs from the binding. Reusing the scene restores the
  bound account. Updating a bound template from current settings previews the
  binding change before saving; an unbound scene stays unbound.
- Editing a template can show **Scene has an update** while the existing
  connection continues using its previous snapshot.
- **Switch to Custom** clears the selection and returns the endpoint,
  transport, IP preference and exit to Usque defaults for the current account.
  Its preview lists the changes first; other device settings stay as they are.
  The switch is recorded like any other, so **Restore previous settings** can
  return to the scene. Zero Trust accounts keep their account-owned addresses.
- Deleting a scene keeps current network settings and shows **Custom**.
  Deleting a bound account removes its credentials normally. The scene then
  offers **Use the current account** to repair its binding before use.
- **Restore previous settings** previews the latest switch's saved prior
  account and scene settings. Later account or scene-setting edits invalidate
  that recovery. Global settings remain current. Application success is checked
  separately from restoring the saved configuration.

Returning manually to the old target account does not reactivate an expired
restore. Restoring a Zero Trust preset also restores the recorded fallback
addresses. If a status refresh fails or belongs to an earlier connection,
Profiles keeps the saved choices visible but shows the current scene as
unconfirmed until the native owner confirms it again.

修改当前场景受管设置不会覆盖模板；修改输出、分流或 DNS 不会使场景显示已修改，
使用或恢复场景也不会覆盖这些全局值。**切换为自定义** 取消所选场景，并把当前账号的
端点、传输协议、IP 偏好和出口恢复为 Usque 默认设置；预览先列出变更，其他全局设置
保持不变。该操作与其他切换一样会被记录，可通过 **恢复切换前配置** 回到原场景；
Zero Trust 账号保留其账号地址。手动换账号可以使绑定场景显示 **账号与绑定不同**。
再次使用会恢复绑定账号。更新模板不会打断当前连接。删除模板保留当前设置并显示
**自定义**；删除账号照常移除凭据，场景卡片提供 **沿用当前账号** 以修复绑定。
**恢复切换前配置** 只针对最近切换，先显示预览；后续修改账号或场景受管设置会使
旧恢复操作失效，防止覆盖新修改。恢复保存值与确认运行生效仍是两个结果。
手动切回原目标账号不会重新启用已失效的恢复。恢复 Zero Trust 预设时也恢复当时的
回退地址。状态刷新失败或仍属于旧连接时，配置页保留已保存选择，但将当前场景标为
待确认，直到原生层重新确认。

## Compatibility and validation / 兼容与验证

Scenes are introduced in v0.3.4; the published v0.3.3 does not include them
and cannot read schema 27.
Schema 27 introduces the scene library, selection and latest-switch record.
Upgrading a configuration without scenes starts with an empty library and no
selection, preserving existing accounts and device settings. Scenes and their
snapshots always contain endpoints and the current set of managed fields.
Obsolete development scene formats have no migration or fallback.
VPN Gate scene drafts additionally require the engine's epoch-checked node
requests; unsupported engines retain other scene and account operations.
Existing account and network choices are preserved. Engines predating scenes
advertise no scene capability and retain account management. Scene references contain no
account credentials, and scene values are excluded from logs and diagnostics.
There is no scene synchronization, import/export or automatic network trigger.

场景功能在 v0.3.4 中引入；已发布的 v0.3.3 不包含该功能，也无法读取 schema 27。
Schema 27 一次引入场景库、当前选择和最近切换记录。从没有场景的配置升级时，
场景库为空且没有选择，原账号与全局设置保持不变。场景及其快照必须包含端点和
当前受管字段；开发期旧场景格式不提供迁移或回退。尚不支持场景的引擎仍可管理账号。
VPN Gate 场景草稿另需引擎支持按实例校验的节点请求；不支持时其他场景与账号操作仍可用。
场景引用不含账号凭据，场景内容不进入日志或诊断。
本功能不包含同步、导入导出或网络条件自动切换。

Deterministic core, owner and fake-engine UI tests cover configuration and
interaction contracts. They do not establish native VPN handoff or leak
prevention. Those checks require the isolated environments described in
[Contributing](../CONTRIBUTING.md); unavailable checks are recorded as `not_run`.
