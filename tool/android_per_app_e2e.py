#!/usr/bin/env python3
"""Android per-app routing acceptance on a disposable emulator.

Drives the debug app through adb only: first-run setup with one live WARP Free
registration, then the Off, Bypass and Proxy-only per-app modes through the
real settings page. Each phase checks the VPN network's UID set, the kernel
route chosen for each probe UID and Cloudflare's trace (`warp=on` or `off`)
fetched as that UID. Reports carry booleans, interface classes and package
names only; addresses, trace bodies and UI text with addresses never leave the
emulator.
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import re
import shlex
import subprocess
import sys
import time
import xml.etree.ElementTree as ElementTree
from dataclasses import dataclass, field
from pathlib import Path

PACKAGE = "io.github.georgexie2333.usque"
ACTIVITY = f"{PACKAGE}/.MainActivity"
TRACE_HOST = "www.cloudflare.com"
ROUTE_PROBE = "1.1.1.1"
INET_GID = 3003

# English labels of the current interface (the emulator runs in en-US).
CONNECT = "Connect"
SETTINGS = "Settings"
PER_APP = "Per-app proxy"
MODE_LABELS = {
    "off": "All apps use the VPN",
    "include": "Proxy only selected apps",
    "exclude": "Bypass selected apps",
}
SHOW_SYSTEM = "Show system apps"
SAVE = "Save"
TERMS_ACCEPT = "I understand and accept these conditions."
SETUP_ADVANCE = (
    "Continue with saved account",
    "Grant permissions and continue",
    "Finish setup",
    "Continue",
    "Allow",
    "OK",
)

IPV4 = re.compile(r"\b(?:\d{1,3}\.){3}\d{1,3}\b")
IPV6_CANDIDATE = re.compile(r"(?<![0-9A-Za-z:])[0-9A-Fa-f:]*:[0-9A-Fa-f:]*:[0-9A-Fa-f:]*")
BOUNDS = re.compile(r"\[(\d+),(\d+)\]\[(\d+),(\d+)\]")


def _redact_ipv6(match: re.Match[str]) -> str:
    token = match.group(0)
    for candidate in (token, token.rstrip(":")):
        try:
            ipaddress.IPv6Address(candidate)
        except ValueError:
            continue
        return "[ipv6]" + token[len(candidate) :]
    # Durations such as 00:00:12 and MAC-style values are not IPv6 addresses.
    return token


def redact(text: str) -> str:
    """Remove IPv4 and IPv6 literals before anything is printed or stored.

    The placeholders contain no markup characters, so a redacted UI dump stays
    well-formed XML.
    """

    return IPV6_CANDIDATE.sub(_redact_ipv6, IPV4.sub("[ipv4]", text))


@dataclass(frozen=True)
class Node:
    labels: tuple[str, ...]
    bounds: tuple[int, int, int, int]
    clickable: bool
    enabled: bool
    checked: bool
    selected: bool
    focused: bool
    class_name: str

    @property
    def center(self) -> tuple[int, int]:
        left, top, right, bottom = self.bounds
        return (left + right) // 2, (top + bottom) // 2

    def has(self, label: str) -> bool:
        return label in self.labels

    def mentions(self, value: str) -> bool:
        return any(value in line for line in self.labels)


def parse_nodes(xml_text: str) -> list[Node]:
    """Flatten a uiautomator dump into nodes with their label lines."""

    # S314: the XML comes from the disposable emulator under test, not a network peer.
    root = ElementTree.fromstring(xml_text)  # noqa: S314 - local emulator dump
    nodes: list[Node] = []
    for element in root.iter("node"):
        match = BOUNDS.fullmatch(element.get("bounds", ""))
        if match is None:
            continue
        lines: list[str] = []
        for attribute in ("text", "content-desc", "hint"):
            for line in (element.get(attribute) or "").splitlines():
                if line.strip():
                    lines.append(line.strip())
        nodes.append(
            Node(
                labels=tuple(lines),
                bounds=tuple(int(value) for value in match.groups()),
                clickable=element.get("clickable") == "true",
                enabled=element.get("enabled") != "false",
                checked=element.get("checked") == "true",
                selected=element.get("selected") == "true",
                focused=element.get("focused") == "true",
                class_name=element.get("class", ""),
            )
        )
    return nodes


NETWORK_STATE = re.compile(r"ni\{(\S+) (\S+)")
TRANSPORTS = re.compile(r"Transports: ([A-Z_|]+)")
OWNER_UID = re.compile(r"OwnerUid: (\d+)")
UID_RANGES = re.compile(r"Uids: <\{([^}]*)\}>")
SINGLE_UID = re.compile(r"(?<![A-Za-z])Uid: (\d+)")


def _network_agents(dumpsys: str):
    for line in dumpsys.splitlines():
        if "NetworkAgentInfo{" in line and "Transports:" in line:
            yield line


def _transports(line: str) -> set[str]:
    match = TRANSPORTS.search(line)
    return set(match.group(1).split("|")) if match else set()


def parse_uid_ranges(dumpsys: str, owner_uid: int) -> list[tuple[int, int]] | None:
    """Return the UID ranges of the connected VPN network owned by `owner_uid`.

    A VPN network also carries its underlying transports (`WIFI|VPN`), and a
    single UID is printed as `Uid: N` instead of a range set.
    """

    for line in _network_agents(dumpsys):
        if "VPN" not in _transports(line):
            continue
        state = NETWORK_STATE.search(line)
        if state is None or state.group(2) != "CONNECTED":
            continue
        owner = OWNER_UID.search(line)
        if owner is None or int(owner.group(1)) != owner_uid:
            continue
        uids = UID_RANGES.search(line)
        if uids is None:
            single = SINGLE_UID.search(line)
            return [(int(single.group(1)), int(single.group(1)))] if single else []
        ranges: list[tuple[int, int]] = []
        for part in uids.group(1).split(","):
            part = part.strip()
            if not part:
                continue
            low, _, high = part.partition("-")
            ranges.append((int(low), int(high or low)))
        return ranges
    return None


def network_summary(dumpsys: str, owner_uid: int) -> list[dict]:
    """Describe each network without addresses, for failure reports."""

    summary: list[dict] = []
    for line in _network_agents(dumpsys):
        state = NETWORK_STATE.search(line)
        owner = OWNER_UID.search(line)
        summary.append(
            {
                "type": state.group(1) if state else None,
                "state": state.group(2) if state else None,
                "transports": sorted(_transports(line)),
                "owned_by_usque": owner is not None and int(owner.group(1)) == owner_uid,
                "uid_field": (
                    "ranges"
                    if UID_RANGES.search(line)
                    else "single"
                    if SINGLE_UID.search(line)
                    else "absent"
                ),
            }
        )
    return summary


def uid_in(ranges: list[tuple[int, int]], uid: int) -> bool:
    return any(low <= uid <= high for low, high in ranges)


def parse_route_device(output: str) -> str:
    """Classify `ip route get` output as `tun`, `other` or `unknown`."""

    match = re.search(r"\bdev (\S+)", output)
    if match is None:
        return "unknown"
    return "tun" if match.group(1).startswith("tun") else "other"


def parse_warp(trace: str) -> str | None:
    match = re.search(r"^warp=(on|off|plus)\s*$", trace, re.MULTILINE)
    return match.group(1) if match else None


def parse_packages(listing: str) -> list[tuple[str, int]]:
    packages: list[tuple[str, int]] = []
    for line in listing.splitlines():
        match = re.fullmatch(r"package:(\S+) uid:(\d+)(?:,\d+)*", line.strip())
        if match:
            packages.append((match.group(1), int(match.group(2))))
    return packages


class HarnessError(RuntimeError):
    pass


@dataclass
class Device:
    adb: str
    serial: str
    log: list[str] = field(default_factory=list)
    last_dump: str = ""
    last_networks: list[dict] = field(default_factory=list)

    def say(self, message: str) -> None:
        line = redact(message)
        self.log.append(line)
        print(line, flush=True)

    def run(self, *args: str, timeout: float = 60, check: bool = True) -> str:
        command = [self.adb, "-s", self.serial, *args]
        result = subprocess.run(  # noqa: S603 - fixed adb executable and harness arguments
            command,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        if check and result.returncode != 0:
            raise HarnessError(
                f"adb {args[0]} failed ({result.returncode}): {redact(result.stderr.strip())}"
            )
        return result.stdout

    def shell(self, command: str, timeout: float = 60, check: bool = True) -> str:
        return self.run("shell", command, timeout=timeout, check=check)

    # Device preparation -------------------------------------------------

    def wait_boot(self, timeout: float = 600) -> None:
        self.run("wait-for-device", timeout=timeout)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.shell("getprop sys.boot_completed", check=False).strip() == "1":
                return
            time.sleep(3)
        raise HarnessError("emulator did not finish booting")

    def become_root(self) -> None:
        self.run("root", check=False)
        time.sleep(3)
        self.wait_boot()
        if self.shell("id -u").strip() != "0":
            raise HarnessError("adb root is unavailable; probe UIDs cannot be exercised")

    def prepare(self) -> None:
        for name in (
            "window_animation_scale",
            "transition_animation_scale",
            "animator_duration_scale",
        ):
            self.shell(f"settings put global {name} 0")
        self.shell("settings put system screen_off_timeout 1800000")
        self.shell("svc power stayon true", check=False)
        self.shell("input keyevent KEYCODE_WAKEUP", check=False)
        self.shell("wm dismiss-keyguard", check=False)

    def install(self, apk: Path) -> None:
        self.run("install", "-r", "-g", str(apk), timeout=300)
        # Grants VPN consent without the system dialog on this disposable emulator.
        self.shell(f"appops set {PACKAGE} ACTIVATE_VPN allow")

    def package_uid(self, package: str) -> int:
        for name, uid in parse_packages(self.shell("pm list packages -U")):
            if name == package:
                return uid
        raise HarnessError(f"{package} is not installed")

    def probe_packages(self) -> list[tuple[str, int]]:
        """Pick two non-Usque apps with INTERNET and a UID of their own."""

        by_uid: dict[int, list[str]] = {}
        for name, uid in parse_packages(self.shell("pm list packages -U")):
            if uid >= 10000 and name != PACKAGE:
                by_uid.setdefault(uid, []).append(name)
        chosen: list[tuple[str, int]] = []
        for uid, names in sorted(by_uid.items(), key=lambda item: sorted(item[1])[0]):
            if len(names) != 1:
                continue
            permissions = self.shell(f"dumpsys package {shlex.quote(names[0])}", timeout=30)
            if "android.permission.INTERNET: granted=true" not in permissions:
                continue
            chosen.append((names[0], uid))
            if len(chosen) == 2:
                for name, _ in chosen:
                    # Background restrictions must not masquerade as routing results.
                    self.shell(f"am set-standby-bucket {shlex.quote(name)} active", check=False)
                    self.shell(f"cmd deviceidle whitelist +{shlex.quote(name)}", check=False)
                return chosen
        raise HarnessError("the image does not provide two probe apps with INTERNET")

    # UI -----------------------------------------------------------------

    def dump(self) -> list[Node]:
        """Capture the screen; an empty list means this attempt was not idle."""

        for _ in range(4):
            output = self.shell(
                "uiautomator dump --compressed /data/local/tmp/usque-ui.xml",
                timeout=40,
                check=False,
            )
            if "dumped to" in output:
                xml_text = self.shell("cat /data/local/tmp/usque-ui.xml", timeout=20)
                self.last_dump = xml_text
                return parse_nodes(xml_text)
            time.sleep(1.5)
        self.say("ui: capture unavailable (screen not idle)")
        return []

    @staticmethod
    def find(nodes: list[Node], label: str, clickable: bool = True) -> Node | None:
        return next(
            (node for node in nodes if node.has(label) and (node.clickable or not clickable)),
            None,
        )

    def tap(self, node: Node) -> None:
        x, y = node.center
        self.shell(f"input tap {x} {y}")
        time.sleep(1.2)

    def key(self, *names: str) -> None:
        self.shell("input keyevent " + " ".join(names))
        time.sleep(0.6)

    def screen_size(self) -> tuple[int, int]:
        match = re.search(r"(\d+)x(\d+)", self.shell("wm size"))
        if match is None:
            raise HarnessError("screen size is unavailable")
        return int(match.group(1)), int(match.group(2))

    def swipe(self, upward: bool) -> None:
        width, height = self.screen_size()
        start, end = (int(height * 0.75), int(height * 0.3))
        if not upward:
            start, end = end, start
        self.shell(f"input swipe {width // 2} {start} {width // 2} {end} 350")
        time.sleep(1.0)

    def wait_label(self, label: str, timeout: float = 60, clickable: bool = True) -> Node:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            node = self.find(self.dump(), label, clickable)
            if node is not None and node.enabled:
                return node
            time.sleep(1.5)
        raise HarnessError(f"'{label}' did not appear")

    def scroll_to(self, predicate, attempts: int = 8) -> Node:
        for direction in (True, False):
            for _ in range(attempts):
                for node in self.dump():
                    if predicate(node):
                        return node
                self.swipe(upward=direction)
        raise HarnessError("the requested row is not on this page")

    # Flows --------------------------------------------------------------

    def launch(self) -> None:
        self.shell(f"am start -W -n {ACTIVITY}", timeout=90)
        time.sleep(3)

    def complete_setup(self, timeout: float = 420) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            nodes = self.dump()
            if self.find(nodes, CONNECT) is not None:
                self.say("setup: home reached")
                return
            terms = next(
                (node for node in nodes if node.has(TERMS_ACCEPT) and node.clickable), None
            )
            if terms is not None and not terms.checked:
                self.say("setup: accepting the Cloudflare terms")
                self.tap(terms)
                continue
            for label in SETUP_ADVANCE:
                node = self.find(nodes, label)
                if node is not None and node.enabled:
                    self.say(f"setup: {label}")
                    self.tap(node)
                    if label == "Finish setup":
                        # Live WARP registration; give the network time.
                        time.sleep(8)
                    break
            else:
                time.sleep(3)
        raise HarnessError("first-run setup did not reach Home")

    def connect(self, owner_uid: int, timeout: float = 240) -> None:
        self.tap(self.wait_label(CONNECT, timeout=60))
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.vpn_ranges(owner_uid) is not None:
                self.say("connect: VPN network is up")
                return
            # Accept a VPN consent dialog if the appop did not suppress it.
            for label in ("OK", "Allow"):
                node = self.find(self.dump(), label)
                if node is not None:
                    self.tap(node)
            time.sleep(4)
        raise HarnessError("the VPN network did not come up")

    def open_per_app(self) -> None:
        nodes = self.dump()
        if any(node.has(MODE_LABELS["off"]) and node.clickable for node in nodes):
            return
        self.tap(self.wait_label(SETTINGS))
        row = self.scroll_to(lambda node: node.has(PER_APP) and node.clickable)
        self.tap(row)
        self.wait_label(MODE_LABELS["exclude"])

    def mode_segment(self, mode: str) -> Node:
        node = self.find(self.dump(), MODE_LABELS[mode])
        if node is None:
            raise HarnessError(f"mode segment {mode} is missing")
        return node

    def current_mode(self) -> str | None:
        nodes = self.dump()
        for mode, label in MODE_LABELS.items():
            node = self.find(nodes, label)
            if node is not None and (node.selected or node.checked):
                return mode
        return None

    def choose_mode_with_dpad(self, mode: str) -> None:
        """Move focus with the D-pad onto the selector, then pick `mode`."""

        order = ["off", "include", "exclude"]
        for _ in range(24):
            focused = next(
                (
                    node
                    for node in self.dump()
                    if node.focused and any(node.has(label) for label in MODE_LABELS.values())
                ),
                None,
            )
            if focused is not None:
                current = next(m for m, label in MODE_LABELS.items() if focused.has(label))
                step = order.index(mode) - order.index(current)
                horizontal = focused.bounds[1] == self.mode_segment(mode).bounds[1]
                forward, backward = (
                    ("KEYCODE_DPAD_RIGHT", "KEYCODE_DPAD_LEFT")
                    if horizontal
                    else ("KEYCODE_DPAD_DOWN", "KEYCODE_DPAD_UP")
                )
                for _ in range(abs(step)):
                    self.key(forward if step > 0 else backward)
                self.key("KEYCODE_DPAD_CENTER")
                time.sleep(1)
                if self.current_mode() != mode:
                    raise HarnessError(f"D-pad selection did not choose {mode}")
                return
            self.key("KEYCODE_DPAD_DOWN")
        raise HarnessError("D-pad focus never reached the mode selector")

    def set_search(self, value: str) -> None:
        field_node = self.scroll_to(lambda node: node.class_name.endswith("EditText"))
        self.tap(field_node)
        self.key("KEYCODE_MOVE_END")
        self.shell("input keyevent " + " ".join(["KEYCODE_DEL"] * 80))
        if value:
            self.shell(f"input text {shlex.quote(value)}")
            time.sleep(1.5)
            current = self.scroll_to(lambda node: node.class_name.endswith("EditText"))
            if value not in current.labels:
                # TV inputs start browsing; activation enables editing.
                self.key("KEYCODE_DPAD_CENTER")
                self.shell(f"input text {shlex.quote(value)}")
                time.sleep(1.5)

    def set_app(self, package: str, wanted: bool) -> None:
        def is_row(node: Node) -> bool:
            return (
                node.mentions(package)
                and node.clickable
                and not node.class_name.endswith("EditText")
            )

        self.set_search(package)
        row = self.scroll_to(is_row)
        if row.checked != wanted:
            self.tap(row)
            row = self.scroll_to(is_row)
            if row.checked != wanted:
                raise HarnessError(f"could not change the selection of {package}")

    def apply_mode(
        self,
        mode: str,
        selected: list[str],
        unselected: list[str],
        dpad: bool,
        expect_initial: str | None,
    ) -> None:
        self.open_per_app()
        if expect_initial is not None and self.current_mode() != expect_initial:
            raise HarnessError(f"saved mode was not restored as {expect_initial}")
        if dpad:
            self.choose_mode_with_dpad(mode)
        else:
            self.tap(self.mode_segment(mode))
            if self.current_mode() != mode:
                raise HarnessError(f"mode {mode} was not selected")
        if mode != "off":
            toggle = self.scroll_to(lambda node: node.has(SHOW_SYSTEM) and node.clickable)
            if not toggle.checked:
                self.tap(toggle)
            for package in selected:
                self.set_app(package, True)
            for package in unselected:
                self.set_app(package, False)
            self.set_search("")
        save = self.scroll_to(lambda node: node.has(SAVE) and node.clickable and node.enabled)
        self.tap(save)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if not any(node.has(MODE_LABELS["exclude"]) for node in self.dump()):
                self.say(f"mode {mode}: saved")
                return
            time.sleep(1.5)
        raise HarnessError(f"saving mode {mode} did not close the page")

    # Observations -------------------------------------------------------

    def vpn_ranges(self, owner_uid: int) -> list[tuple[int, int]] | None:
        dumpsys = self.shell("dumpsys connectivity", timeout=60)
        self.last_networks = network_summary(dumpsys, owner_uid)
        return parse_uid_ranges(dumpsys, owner_uid)

    def route(self, uid: int) -> str:
        output = self.shell(f"ip -4 route get {ROUTE_PROBE} uid {uid}", check=False)
        return parse_route_device(output)

    def warp(self, uid: int) -> str | None:
        request = (
            f"GET /cdn-cgi/trace HTTP/1.1\\r\\nHost: {TRACE_HOST}\\r\\n"
            "User-Agent: usque-android-e2e\\r\\nConnection: close\\r\\n\\r\\n"
        )
        # Keep stdin open so nc reads the reply before the server closes.
        script = f"(printf '{request}'; sleep 6) | nc -w 15 {TRACE_HOST} 80"
        command = f"su {uid},{uid},{INET_GID} sh -c {shlex.quote(script)}"
        for _ in range(3):
            value = parse_warp(self.shell(command, timeout=45, check=False))
            if value is not None:
                return value
            time.sleep(3)
        return None


def observe(
    device: Device,
    owner_uid: int,
    probes: list[tuple[str, int]],
    expected_vpn: dict[str, bool],
    timeout: float = 150,
) -> dict:
    """Wait for the expected VPN UID set, then record route and trace per probe."""

    deadline = time.monotonic() + timeout
    ranges = None
    while time.monotonic() < deadline:
        ranges = device.vpn_ranges(owner_uid)
        if ranges is not None and all(
            uid_in(ranges, uid) == expected_vpn[name] for name, uid in probes
        ):
            break
        time.sleep(4)
    result: dict = {"probes": {}, "passed": True}
    for name, uid in probes:
        in_vpn = ranges is not None and uid_in(ranges, uid)
        route = device.route(uid)
        warp = None
        for _ in range(5):
            warp = device.warp(uid)
            if warp is not None and (warp != "off") == expected_vpn[name]:
                break
            time.sleep(6)
        passed = (
            ranges is not None
            and in_vpn == expected_vpn[name]
            and warp is not None
            and (warp != "off") == expected_vpn[name]
            and route != ("other" if expected_vpn[name] else "tun")
        )
        result["probes"][name] = {
            "expected_vpn": expected_vpn[name],
            "in_vpn_uid_ranges": in_vpn,
            "kernel_route": route,
            "trace_warp": warp,
            "passed": passed,
        }
        result["passed"] &= passed
    result["vpn_network_present"] = ranges is not None
    if not result["passed"]:
        result["networks"] = device.last_networks
    return result


def run(arguments: argparse.Namespace) -> dict:
    device = Device(arguments.adb, arguments.serial)
    report: dict = {"device": arguments.device, "phases": {}, "passed": False}
    try:
        device.wait_boot()
        device.become_root()
        device.prepare()
        report["sdk"] = device.shell("getprop ro.build.version.sdk").strip()
        report["characteristics"] = device.shell("getprop ro.build.characteristics").strip()
        device.install(arguments.apk)
        owner_uid = device.package_uid(PACKAGE)
        probes = device.probe_packages()
        (first, first_uid), (second, second_uid) = probes
        report["probes"] = [first, second]
        device.say(f"probes: {first} (A), {second} (B)")

        baseline = device.warp(first_uid)
        report["phases"]["disconnected"] = {
            "passed": baseline == "off",
            "trace_warp": baseline,
        }

        device.launch()
        device.complete_setup()
        device.connect(owner_uid)
        dpad = arguments.device == "tv"
        phases = (
            ("off", None, [], [], {first: True, second: True}),
            ("exclude", "off", [first], [second], {first: False, second: True}),
            ("include", "exclude", [first], [second], {first: True, second: False}),
            ("off", "include", [], [], {first: True, second: True}),
        )
        for index, (mode, initial, selected, unselected, expected) in enumerate(phases):
            name = f"{index}-{mode}"
            if index > 0:
                device.apply_mode(mode, selected, unselected, dpad, initial)
            report["phases"][name] = observe(device, owner_uid, probes, expected)
            device.say(f"phase {name}: {'pass' if report['phases'][name]['passed'] else 'FAIL'}")
        report["passed"] = all(phase["passed"] for phase in report["phases"].values())
    except (HarnessError, subprocess.TimeoutExpired) as error:
        report["error"] = redact(str(error))
        report["networks"] = device.last_networks
        device.say(f"error: {report['error']}")
        if device.last_dump and arguments.ui_dump:
            arguments.ui_dump.write_text(redact(device.last_dump), encoding="utf-8")
    report["log"] = device.log
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apk", type=Path, required=True)
    parser.add_argument("--device", choices=("phone", "tv"), required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--ui-dump", type=Path)
    parser.add_argument("--adb", default="adb")
    parser.add_argument("--serial", default="emulator-5554")
    arguments = parser.parse_args()
    report = run(arguments)
    arguments.report.write_text(json.dumps(report, indent=2, sort_keys=True), encoding="utf-8")
    print(json.dumps({k: v for k, v in report.items() if k != "log"}, indent=2, sort_keys=True))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
