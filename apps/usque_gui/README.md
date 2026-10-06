# Usque GUI

Flutter UI for Windows, Android and Android TV. macOS source is retained for
future work but is not built, tested or released for the current product.
The UI uses native Flutter widgets without a WebView.

Desktop builds start the Rust sidecar and talk to it over current-user IPC. Android talks to the Rust library inside the isolated `:vpn` process.

Build and test commands are in [CONTRIBUTING.md](../../CONTRIBUTING.md). Feature status is in [docs/IMPLEMENTATION.md](../../docs/IMPLEMENTATION.md).

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
decoration follows the existing theme's primary colour. Native setup and
uninstall controls use the same primary/on-primary pairs, retaining system
colours in high-contrast mode.

Android launcher artwork fits the centred 66dp safe area of a 108dp adaptive
layer. The Android 13+ monochrome layer and notification icon contain the U/star
lines on transparency, rather than the solid circular background. Notifications
retain the `ic_stat_usque` resource name. The retained macOS icons are refreshed
by the generator, without establishing macOS product support.

品牌母版是唯一可编辑图形。应用图标固定使用浅色配色；页面 Logo 随主题切换，
内部线条和透明边缘保持一致。Android 单色图标保留 U 形与星形线条，
安装器和卸载器的高对比度模式继续使用系统颜色。

## Linux development preview

Linux/WSL developers can run `bash tool/dev.sh preview` from the repository root
to open the debug-only UI preview with hot reload, simulated connection states,
sample traffic and an onboarding reset. Accounts, preferences and settings stay
in memory; no engine process, VPN or update operation starts. Linux is a
development preview, not a supported VPN product or release platform. See the
[Linux development guide](../../docs/LINUX_DEVELOPMENT.md) for native editing,
checks, limitations and ignored local files. Windows golden validation remains
required.

Linux/WSL 可运行上述命令预览界面并热重载，切换模拟连接状态和重新开始首次引导。
数据仅在内存中保存，Windows golden 基线仍须在 Windows 验证。

## Editing and navigation

- First launch keeps four steps: welcome, system permissions, Cloudflare, Inc. terms and account setup. Android requires VPN authorization in the permissions step; notification authorization is optional and is requested afterward. Authorizing VPN may disconnect another VPN app, but onboarding does not start a connection. Previously completed installations enter Home normally, and connection startup rechecks revoked VPN authorization.
- The welcome copy identifies Usque as an unofficial client compatible with Cloudflare® WARP® services, with no affiliation, sponsorship or endorsement. All 21 language catalogs include a localized trademark attribution after the navigation actions on every setup step; it stays reachable by scrolling on small screens and with large text.
- The terms step opens Cloudflare's application terms and the distinct personal WARP® and Zero Trust privacy policies in the system browser. Confirmation and step progress are saved locally without credentials. Account setup uses the native initial-identity operation, preserves an existing ready account, and checks an uncertain result before allowing a new attempt. Interrupted attempts require fresh user input; License Keys and login callbacks are never replayed from a saved draft.
- 首次引导仍为欢迎、系统权限、Cloudflare 条款、账号设置四步。Android 必须先授权 VPN，通知权限可拒绝；授权本身不启动连接，且可能断开其他 VPN。已注册账号会被保留；通信超时后先检查原操作结果，确定中断后才由用户重新输入并重试。

- Accounts select WARP identities; network settings are shared across accounts.
- Advanced → IP & DNS offers WARP DNS types Plain DNS, DNS over HTTPS and DNS over TLS, sharing the encrypted type names with Direct DNS. Encrypted modes use server name, port and bootstrap IP fields, plus a path for HTTPS. The editor uses concise labels and validation errors without explanatory paragraphs. It retains drafts when switching types and applies only the selected type through the existing save bar. See [WARP exit DNS](../../docs/WARP_DNS.md).
- Proxy addresses, ports, and DNS are drafts until **Apply changes** succeeds. Credentials use their separate **Save username and password** action and never enter the network draft.
- Advanced settings have a persistent apply bar and a back-navigation guard for unapplied edits. Reset loads defaults into the draft; it does not apply them immediately.
- Endpoint selection defaults to **Automatic** for new installations and staged resets. **Custom** retains manually saved IPv4/IPv6 addresses; switching the picker keeps address drafts. Automatic saves retain the confirmed custom pair, while port and SNI remain editable (L4 keeps its identity-derived SNI). Existing configurations migrate to Custom; Zero Trust keeps account-specific manual addresses: use **Edit Zero Trust endpoints**, read the red fullscreen warning and acknowledge the risks and access authorization before editing. Each page visit requires confirmation. Reset stages registered addresses and sign-in restores them, clearing the override. See [automatic endpoints](../../docs/NETWORK_SETTINGS.md#automatic-endpoints--自动选择端点).
- **Disable QUIC** under Advanced network settings → Routing & protection blocks application UDP/443 only on proxy/tunnel paths, with GEO direct traffic exempt. It defaults off and applies without reconnecting an established session. **禁用 QUIC** 默认关闭，应用后无需重连，所有 GEO 直连连接保持可用。 See [network settings](../../docs/NETWORK_SETTINGS.md#disable-application-quic--禁用应用-quic) for scope and deferred application.
- HTTP/SOCKS5 chain exits automatically prefer TCP for web traffic. The existing QUIC control displays its effective enabled state and is read-only with a short managed-by-connection hint; saved manual preferences and drafts remain intact and return after leaving these exits. Other UDP, direct routes and the WARP underlay retain their existing behavior.
- Inline output statuses distinguish enabled configuration from observed runtime state, including unavailable, limited, and failed states. Labels and icons carry the meaning without decorative badge surfaces.
- Chain proxy shares its heading, enable switch, live connection section and apply bar across OpenVPN, WireGuard, WARP via WireGuard, VPN Gate, HTTP and SOCKS5. HTTP/SOCKS5 use a structured add-proxy form with optional credentials and DNS. Below 600 logical pixels, the source heading stays on its own line above a compact picker that matches the selected choice's size; its trailing chevron opens a bottom sheet with all six choices. Wider layouts keep the wrapping choices, stacking them at large text; the selected choice carries its check after its name, never over its icon. Source content contains VPN import/paste actions or a proxy form with saved configurations; VPN Gate provides refresh, filters and public nodes. Both lists share single-selection rows and saved/current markers; configuration and node details expand on request. Switching sources never prompts: drafts are retained until settings are applied, and only leaving the page asks about unapplied edits. The apply bar names the pending selection, explains blocked drafts, and keeps VPN Gate preparation cancellable. Connection failures and missing DNS remain visible independently of the source being browsed. Chain copy is localized in every catalog. WARP via WireGuard adds configuration generation and endpoint override drafts; see [the guide](../../docs/WARP_WIREGUARD.md).
- HTTP/SOCKS Add proxy → DNS offers Automatic (DoH by default), Encrypted DNS · Cloudflare, and DNS over TCP. Automatic retains explicit DNS; DoH uses verified TLS/HTTP/2 through the final exit and does not switch exits on failure. Custom DNS drafts survive picker changes; saving DoH omits them. Engines without encrypted chain DNS expose TCP only. HTTP/SOCKS 的 DNS 默认经最终出口使用 DoH，显式自定义 DNS 保留 TCP；失败不绕过链出口。
- HTTP/SOCKS scope explanations stay in the chain editor's current-connection section, including proxy-only limits, retained explicit bypasses and terminal failure with an explicitly inactive Kill Switch. Home omits these paragraphs. The phone's compact chain summary keeps its WARP status row present across sparse refreshes, using observed WARP state or known connection stages and a dash when unknown. Android system-blocking advice in the editor is conditional; successful UDP association remains distinct from verified forwarding. Saved drafts never establish current scope. 首页不再显示长篇范围提示；手机链式状态栏保留 WARP 状态，缺少独立上报时按已确认的连接阶段显示，未知时显示横线。
- File imports support native multi-selection on Windows and Android: up to 128 files, each at most 128 KiB. Batches use offline per-file checks, editable names and individual credentials before explicitly saving valid entries. Partial successes remain saved; interrupted writes require inspecting the library before reimporting. Single-file and pasted-text workflows remain available. Windows 和 Android 支持批量导入：逐项离线检查，补充名称与凭据后统一保存合格项；通信中断时先核对配置库再重试。See [chain proxy](../../docs/CHAIN_PROXY.md).
- Home uses open sections: the connection ring and Kill Switch state, shared upload/download traces, and connection details. Desktop gives the enlarged connection control even spacing beside the status/location readouts and keeps TUN, system-proxy and chain-proxy switches in a wider right-hand column; narrow content or text above 150% stacks the sections. The TUN and system-proxy switches carry a plain-text hint describing what they do; while the HTTP local proxy is off, the system-proxy hint states that it must be enabled first. Desktop traffic traces are 96 logical pixels tall and keep that height while disconnected, when the rates show a dash. The chain-proxy heading and **Settings** action open the existing chain-proxy editor. These shortcuts save shared network settings through the existing apply lifecycle; enabling system proxy requires the HTTP local proxy. **Local proxies** keeps HTTP/SOCKS5 listener addresses and **Manage proxies** opens the existing proxy page. Connected sessions show exit region, duration, protocol and address family; the phone's idle/error overview explains the configured outputs. On desktop and phones, **Connection details** contains only selectable IPv4/IPv6 addresses and a plain **Enabled:** list of configured interfaces, without interface icons, location or runtime status labels. Every interface label and related settings heading uses the same platform-aware name: desktop English uses **TUN**, Chinese uses **虚拟网卡** (Traditional Chinese **虛擬網卡**), and other locales use their translated adapter name; Android keeps **VPN**. Unavailable or disconnected IPs show a dash. Chain progress continues to drive the main connection status and ring. Desktop adds no extra block above the main connection area when a chain is enabled. Phones retain the chain summary and its settings shortcut above the connection control. Desktop and mobile Home, including the ordinary connection overview, omit network-quality and diagnostic shortcuts; their existing settings and navigation entries remain available.
- Desktop and mobile traces share the timestamped 60-second quality history. New samples with unchanged values are retained; repaint timers do not generate observations. Source-aligned time slots tolerate scheduling jitter, and averages use actual elapsed time. Missing samples stay gaps; delayed, paused, unavailable, and disconnected readings are identified explicitly. The view does not start additional probes or persist traffic history.
- Settings groups connection/protection (auto-connect, a read-only Kill Switch row, Android Always-on, Advanced), proxy/routing (bypass, Android per-app proxy), tools (network quality, diagnostics), and application preferences. The Kill Switch row shows the configured state and opens Advanced with that draft switch in view; it still applies only through the Advanced apply bar.
- The Proxy section's output group holds every immediate output switch: TUN (VPN on Android), SOCKS5, HTTP and, on Windows, system proxy. Settings does not repeat them.
- Proxy and Settings subpages open inside their section. Desktop keeps the navigation rail visible; phones hide the bottom bar until the subpage closes. Choosing another section, or the current one again, returns to the section root and first asks about unapplied edits; declining keeps the subpage open.

- **Bypass settings** combines country rules and a line-based editor for custom CIDRs, IPs and domains. Domains include subdomains; custom rules work without GEO downloads. The editor uses the shared apply bar and leave guard. Advanced settings no longer edits bypass CIDRs. See [direct DNS and bypass targets](../../docs/encrypted-direct-dns.md#custom-bypass-targets--自定义绕过目标).

## Native UI composition

- `ContentSection` and `ContentHeading` group ordinary content with typography and spacing, not a card background. `ContentList` separates adjacent entries; `ActionRow` provides native ink, keyboard/D-pad activation and a visible, layout-stable focus outline. `InlineStatus` pairs a readable state label with a supplementary icon.
- Accounts are continuous rows. The active identity has a light selection wash, marker and explicit status; clicking the row itself never switches accounts. Identity configuration, activation, rename and delete remain separate controls.
- Settings and proxy editors use open groups with a maximum form width of 880 logical pixels. General page content remains capped at 1120. Navigation retains its existing 760/1050 breakpoints and phone/rail behavior.
- Network quality uses continuous metric sections rather than a card grid. Diagnostics keeps its checks, timeline, export and destructive actions distinct. Dialog field groups and onboarding forms do not nest card surfaces.
- Background fills are reserved for controls, selection, warnings, dialogs and save bars. Use `Panel` for explicitly isolated danger areas; do not make every panel transparent. Preserve the connection ring's drawing, proportions, phase mapping and motion when editing surrounding layouts.
- Use the existing brand colors, bundled fonts and locale fallbacks. Primary controls have a minimum 48-pixel target; status indicators do not rely on color alone. Layout code must use the existing network observations without creating probes, sampling timers or history persistence.

The workflow and native-layout widget tests use a fake engine and cover connected detail expansion, repeated collapse/restore, selectable-value copying, action-row focus and keyboard/D-pad activation, status-label contrast, narrow/landscape layouts, 200% text, and reduced motion. The `golden` suite additionally checks real-font layouts and exact Windows-pinned screenshots, including accounts, settings, proxy editors, onboarding, diagnostics, dialogs and expanded home details; neither suite starts a VPN or proves native networking behavior.

---

Cloudflare and WARP are trademarks and/or registered trademarks of Cloudflare, Inc. in the United States and other jurisdictions.
