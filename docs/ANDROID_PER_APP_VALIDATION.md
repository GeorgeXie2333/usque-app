# Android per-app routing validation / Android 分应用路由验证

This record covers the Android per-app modes: **All apps use the VPN**,
**Proxy only selected apps** and **Bypass selected apps**. Workstation checks
use host tests and a fake engine; the emulator acceptance below runs the debug
app on disposable GitHub-hosted runners.

本记录覆盖 Android 分应用的三种模式。开发机只运行主机测试与模拟引擎；下列模拟器验收在
GitHub 托管的一次性 runner 上运行 debug 应用。

## Emulator acceptance / 模拟器验收

Dispatch the Build workflow for the branch under test:

```sh
gh workflow run build.yml --ref <branch> -f android_per_app_e2e=true
```

The run builds one x86_64 debug APK, then boots two disposable emulators:
`android-34;google_apis;x86_64` (phone) and `android-36;android-tv;x86_64`
(TV). [`tool/android_per_app_e2e.py`](../tool/android_per_app_e2e.py) drives
the app through adb only:

1. Grants VPN consent with the `ACTIVATE_VPN` appop, completes first-run setup
   through the interface and registers one live WARP Free test device.
2. Installs two code-free debuggable probe apps that only request INTERNET
   ([`tool/build_android_e2e_probes.sh`](../tool/build_android_e2e_probes.sh)),
   so each has its own UID. `run-as` acts as each probe, which also works on
   the TV image, a user build without `adb root`.
3. Connects, then saves Off, Bypass (A), Proxy only selected (A) and Off again
   through the real per-app page. On TV the mode is chosen with the D-pad.
4. For each phase, checks the VPN network's UID set, the kernel route for each
   probe UID and Cloudflare's trace fetched as that probe: `warp=on` inside the
   VPN and `warp=off` outside it. A disconnected baseline must read `warp=off`.
   Reopening the page must restore the saved mode.

Reports contain booleans, interface classes (`tun`/`other`) and probe package
names. Addresses, trace bodies and unredacted screen captures stay on the
emulator; a failure keeps only a redacted copy of the last screen.

The emulator does not cover Always-on VPN, Block connections without VPN,
reboot, Doze, upgrades or physical TV remotes. Those remain `not_run` until
the `usque-android-device` environment runs them.

模拟器不覆盖始终开启的 VPN、屏蔽未使用 VPN 的连接、重启、Doze、升级和实体 TV 遥控器；
在 `usque-android-device` 环境运行前，这些项目记为 `not_run`。

## Results / 结果

No emulator run is recorded yet. Record each run with its commit, run link and
per-device result; keep failed and `not_run` results.

尚无模拟器运行记录。每次运行需记录提交、运行链接和各设备结果，失败与 `not_run` 结果同样保留。
