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
   through the real per-app page. On TV the mode is chosen with the D-pad. A
   connection error from the live endpoint is retried at most three times and
   the count is reported as `connect_retries`.
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

### 2026-11-25: emulator acceptance passed / 模拟器验收通过

Run [45510590638](https://github.com/GeorgeXie2333/usque-app/actions/runs/45510590638)
at `34195c99` (feature `33b6123e` plus harness commits), debug x86_64 APK,
live WARP Free registration, no connection retries on either device.

| Phase | Phone (API 34, `google_apis`) | TV (API 36, `android-tv`, D-pad) |
|---|---|---|
| Disconnected baseline | A `warp=off` | A `warp=off` |
| 0 Off | A, B in VPN, `tun`, `warp=on` | A, B in VPN, `tun`, `warp=on` |
| 1 Bypass A | A outside, `other`, `warp=off`; B in VPN, `warp=on` | same as phone |
| 2 Proxy only A | A in VPN, `warp=on`; B outside, `other`, `warp=off` | same as phone |
| 3 Off | A, B in VPN, `tun`, `warp=on` | A, B in VPN, `tun`, `warp=on` |

Each reopened page restored the saved mode. Result: **passed** on both
emulators. Always-on VPN, Block connections without VPN, reboot, Doze,
upgrades and physical TV remotes remain `not_run`.

### Earlier runs / 之前的运行

The feature code was the same in every run; each failure was in the
emulator setup or the harness and was fixed before the next run.

| Run | Commit | Phone | TV | Cause |
|---|---|---|---|---|
| 45480651404 | `b8bbf90f` | failed | failed | AVD written outside the expected directory |
| 45482738746 | `2e719d23` | failed | cancelled | VPN listed as `WIFI\|VPN` not recognised; TV adb unauthorized |
| 45494098550 | `747e6cb1` | failed after 0 | failed | Soft keyboard blocked swipes; TV image has no `adb root` |
| 45497204219 | `70dfc806` | failed | failed after 0 | Live endpoint connection error (no retry yet); TV search field not recognised |
| 45499778778 | `90dc33a0` | passed | failed | adb key created after emulator boot |
| 45503641934 | `7cb4820c` | passed | failed after 0 | Swipes did not scroll the TV page |
| 45505953126 | `85e8e5fe` | passed | failed after 0 | Same; TV path moved to the D-pad |
| 45508438414 | `20a048bc` | passed | failed after 1 | TV navigation rail missing from the accessibility tree |

Run 45510493222 was cancelled: it was dispatched at a stale head after a
failed push.

2026-11-25 的运行 45510590638 中，手机与 TV 模拟器的全部阶段均通过；之前的失败均来自模拟器
环境或驱动脚本，已逐一修复。始终开启的 VPN、屏蔽未使用 VPN 的连接、重启、Doze、升级和实体
TV 遥控器仍为 `not_run`。
