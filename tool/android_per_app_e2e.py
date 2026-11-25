#!/usr/bin/env python3
"""Android per-app routing acceptance on a disposable emulator.

Drives the debug app through adb only: first-run setup with one live WARP Free
registration, then the Off, Bypass and Proxy-only per-app modes through the
real settings page. Two code-free debuggable probe apps provide the UIDs;
`run-as` fetches Cloudflare's trace (`warp=on` or `off`) as each of them, so
user builds without `adb root` (Android TV images) work too. Each phase also
checks the VPN network's UID set and the kernel route chosen for each probe.

Two routing phases follow. The app's configuration is seeded with the largest
custom rule set (4096 rules per kind, ending in `DIRECT www.cloudflare.com`),
so connecting hands a profile of about 1.1 MB to the VPN process; the probe
must then see `warp=off` for that host and `warp=on` for 1.1.1.1. The rule is
then deleted through the editor's search and saved, which sends the settings
request and result through the same file handoff; the probe must see
`warp=on` again and no handoff file may remain.

Reports carry booleans, counts, interface classes and package names only;
addresses, trace bodies and UI text with addresses never leave the emulator.
"""

from __future__ import annotations

import argparse
import hashlib
import ipaddress
import json
import re
import shlex
import subprocess
import sys
import time
import uuid
import xml.etree.ElementTree as ElementTree
from dataclasses import dataclass, field
from pathlib import Path

PACKAGE = "io.github.georgexie2333.usque"
ACTIVITY = f"{PACKAGE}/.MainActivity"
PROBE_PACKAGES = (
    "io.github.georgexie2333.usque.e2e.probea",
    "io.github.georgexie2333.usque.e2e.probeb",
)
TRACE_HOST = "www.cloudflare.com"
# Serves the trace over plain HTTP like TRACE_HOST, but is its parent domain,
# so a rule for TRACE_HOST does not match it. (1.1.1.1 redirects to HTTPS.)
TUNNEL_HOST = "cloudflare.com"
APP_LOG = "no_backup/logs/android-engine.jsonl"
UUID_PATTERN = re.compile(
    r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"
)
ROUTE_PROBE = "1.1.1.1"

# English labels of the current interface (the emulator runs in en-US).
CONNECT = "Connect"
SETTINGS = "Settings"
PER_APP = "Per-app proxy"
MODE_LABELS = {
    "off": "All apps use the VPN",
    "include": "Proxy only selected apps",
    "exclude": "Bypass selected apps",
}
SAVE = "Save"
SEARCH = "Search apps"
BYPASS_SETTINGS = "Bypass settings"
RULE_SEARCH = "Search rules"
EDIT = "Edit"
DELETE = "Delete"
# Core limits: rules per kind and UTF-8 bytes of all targets together.
RULES_PER_KIND = 4096
TARGET_BUDGET = 384 * 1024
CONFIG = "no_backup/usque_config/profiles-v2.json"
HANDOFF_DIRECTORY = "no_backup/ipc-handoff"
BACK = "Back"
RETRY = "Retry"
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
    scrollable: bool = False

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
                scrollable=element.get("scrollable") == "true",
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


def is_search_field(node: Node, hint: str = SEARCH) -> bool:
    """A search field: an EditText, or a TV field still being browsed."""

    return node.class_name.endswith("EditText") or node.has(hint)


def largest_rule_set(direct_host: str) -> list[dict]:
    """Both kinds full and nearly the whole target budget spent.

    Every rule rejects a documentation address or a `.test` name, except the
    last: `DIRECT direct_host`. Its effect proves the complete list reached
    the VPN process, since a truncated payload would lose it.
    """

    def rule(kind: str, target: str, action: str) -> dict:
        return {"id": str(uuid.uuid4()), "kind": kind, "target": target, "action": action}

    rules = [
        rule(
            "cidr",
            f"2001:db8:aaaa:bbbb:cccc:dddd:{0x8000 | index >> 8:04x}:"
            f"{0x8000 | index & 0xFF:04x}/128",
            "reject",
        )
        for index in range(RULES_PER_KIND)
    ]
    used = sum(len(item["target"]) for item in rules) + len(direct_host)
    domains = RULES_PER_KIND - 1
    length = min((TARGET_BUDGET - used) // domains, 63 + len(".test"))
    for index in range(domains):
        label = f"r{index:04d}-"
        rules.append(
            rule("domain", label + "a" * (length - len(label) - len(".test")) + ".test", "reject")
        )
    rules.append(rule("domain", direct_host, "direct"))
    return rules


def swipe_path(
    nodes: list[Node], area: tuple[int, int, int, int], upward: bool
) -> tuple[int, int, int]:
    """Return `(x, start_y, end_y)` for a swipe that does not begin on a text field.

    A drag that starts on a text field selects text instead of scrolling.
    """

    left, top, right, bottom = area
    height = bottom - top
    x = (left + right) // 2
    fields = [node.bounds for node in nodes if is_search_field(node)]

    def clear(y: int) -> bool:
        return not any(fl <= x <= fr and ft <= y <= fb for fl, ft, fr, fb in fields)

    tenths = (8, 7, 9, 6, 5) if upward else (2, 3, 1, 4, 5)
    starts = [top + height * tenth // 10 for tenth in tenths]
    start = next((y for y in starts if clear(y)), starts[0])
    if upward:
        end = max(top + height // 10, start - height * 3 // 5)
    else:
        end = min(bottom - height // 10, start + height * 3 // 5)
    return x, start, end


def scroll_area(nodes: list[Node]) -> tuple[int, int, int, int] | None:
    """Bounds of the largest scrollable node in a dump."""

    scrollable = [node for node in nodes if node.scrollable]
    if not scrollable:
        return None
    largest = max(
        scrollable,
        key=lambda node: (node.bounds[2] - node.bounds[0]) * (node.bounds[3] - node.bounds[1]),
    )
    return largest.bounds


def parse_route_device(output: str) -> str:
    """Classify `ip route get` output as `tun`, `other` or `unknown`."""

    match = re.search(r"\bdev (\S+)", output)
    if match is None:
        return "unknown"
    return "tun" if match.group(1).startswith("tun") else "other"


def trace_failure(output: str) -> str:
    """Why a trace fetch returned no `warp` field, without any addresses."""

    status = re.search(r"HTTP/1\.[01] (\d{3})", output)
    if status is not None:
        return f"http-{status.group(1)}"
    lowered = output.lower()
    if any(word in lowered for word in ("bad address", "resolve", "getaddrinfo", "no address")):
        return "dns"
    if "refused" in lowered:
        return "refused"
    return "no-response"


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
    artifacts: Path | None = None
    last_trace_failure: str = ""

    def save_dump(self, name: str) -> None:
        if self.artifacts is not None and self.last_dump:
            (self.artifacts / f"{name}.xml").write_text(redact(self.last_dump), encoding="utf-8")

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
        # The AVD has a hardware keyboard; a soft keyboard would cover the page
        # and swallow swipes.
        self.shell("settings put secure show_ime_with_hard_keyboard 0", check=False)

    def install(self, apk: Path) -> None:
        self.run("install", "-r", "-g", str(apk), timeout=300)
        # Grants VPN consent without the system dialog on this disposable emulator.
        self.shell(f"appops set {PACKAGE} ACTIVATE_VPN allow")

    def package_uid(self, package: str) -> int:
        for name, uid in parse_packages(self.shell("pm list packages -U")):
            if name == package:
                return uid
        raise HarnessError(f"{package} is not installed")

    def install_probes(self, apks: list[Path]) -> list[tuple[str, int]]:
        """Install the code-free debuggable probe apps and check `run-as`."""

        probes: list[tuple[str, int]] = []
        for apk in apks:
            self.run("install", "-r", str(apk), timeout=180)
        for package in PROBE_PACKAGES:
            uid = self.package_uid(package)
            if self.shell(f"run-as {package} id -u", check=False).strip() != str(uid):
                raise HarnessError(f"run-as cannot act as {package}")
            # Background restrictions must not masquerade as routing results.
            self.shell(f"am set-standby-bucket {package} active", check=False)
            self.shell(f"cmd deviceidle whitelist +{package}", check=False)
            probes.append((package, uid))
        return probes

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

    def swipe(self, nodes: list[Node], upward: bool) -> None:
        """Swipe inside the largest scrollable area, or the screen if none."""

        area = scroll_area(nodes)
        if area is None:
            width, height = self.screen_size()
            area = (0, 0, width, height)
        x, start, end = swipe_path(nodes, area, upward)
        self.shell(f"input swipe {x} {start} {x} {end} 350")
        time.sleep(1.0)

    def wait_label(self, label: str, timeout: float = 60, clickable: bool = True) -> Node:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            node = self.find(self.dump(), label, clickable)
            if node is not None and node.enabled:
                return node
            time.sleep(1.5)
        raise HarnessError(f"'{label}' did not appear")

    def scroll_to(self, predicate, attempts: int = 8, towards_top: bool = False) -> Node:
        directions = (False, True) if towards_top else (True, False)
        for upward in directions:
            for _ in range(attempts):
                nodes = self.dump()
                for node in nodes:
                    if predicate(node):
                        return node
                self.swipe(nodes, upward=upward)
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

    def connect(self, owner_uid: int, timeout: float = 300, max_retries: int = 3) -> int:
        """Connect and return how many times a connection error was retried."""

        self.tap(self.wait_label(CONNECT, timeout=60))
        retries = 0
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.vpn_ranges(owner_uid) is not None:
                self.say(f"connect: VPN network is up after {retries} retries")
                return retries
            nodes = self.dump()
            retry = self.find(nodes, RETRY)
            if retry is not None and retries < max_retries:
                # The live WARP endpoint can fail transiently from hosted runners;
                # retries are reported, never hidden.
                retries += 1
                self.say(f"connect: connection error, retry {retries}")
                self.tap(retry)
            # Accept a VPN consent dialog if the appop did not suppress it.
            for label in ("OK", "Allow"):
                node = self.find(nodes, label)
                if node is not None:
                    self.tap(node)
            time.sleep(5)
        raise HarnessError(f"the VPN network did not come up after {retries} retries")

    def open_per_app(self) -> None:
        def is_row(node: Node) -> bool:
            return node.has(PER_APP) and node.clickable

        nodes = self.dump()
        if any(node.has(MODE_LABELS["off"]) and node.clickable for node in nodes):
            return
        # Saving returns to Settings; the TV navigation rail is not always in
        # the accessibility tree, so use the row when it is already on screen.
        row = next((node for node in nodes if is_row(node)), None)
        if row is None:
            self.tap(self.wait_label(SETTINGS))
            row = self.scroll_to(is_row)
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

    def set_search(self, value: str, hint: str = SEARCH) -> None:
        def field(node: Node) -> bool:
            return is_search_field(node, hint)

        def clear_and_type() -> None:
            self.key("KEYCODE_MOVE_END")
            self.shell("input keyevent " + " ".join(["KEYCODE_DEL"] * 80))
            if value:
                self.shell(f"input text {shlex.quote(value)}")
            time.sleep(1.5)

        # Tapping also starts editing a TV field, which is browsed until then.
        self.tap(self.scroll_to(field))
        clear_and_type()
        current = self.scroll_to(field)
        # Package names contain dots; the empty field shows only its hint.
        typed = value in current.labels if value else not current.mentions(".")
        if typed:
            return
        self.key("KEYCODE_DPAD_CENTER")
        clear_and_type()

    def leave_text_field(self) -> None:
        """Finish editing a TV text field so the D-pad moves focus again."""

        for key in ("KEYCODE_ENTER", "KEYCODE_ESCAPE"):
            editing = any(
                node.focused and node.class_name.endswith("EditText") for node in self.dump()
            )
            if not editing:
                return
            self.key(key)
        if any(node.focused and node.class_name.endswith("EditText") for node in self.dump()):
            raise HarnessError("the TV search field stayed in editing mode")

    def focus_onto(self, predicate, key: str, presses: int = 30) -> Node:
        """Press `key` until a focused node satisfies `predicate`."""

        for _ in range(presses):
            nodes = self.dump()
            focused = next((node for node in nodes if node.focused and predicate(node)), None)
            if focused is not None:
                return focused
            # Back sits left of Save in the page header; step right onto Save.
            on_back = any(node.focused and node.has(BACK) for node in nodes)
            self.key("KEYCODE_DPAD_RIGHT" if on_back and key == "KEYCODE_DPAD_UP" else key)
        raise HarnessError("D-pad focus did not reach the requested control")

    def set_app(self, package: str, wanted: bool, dpad: bool) -> None:
        def is_row(node: Node) -> bool:
            return node.mentions(package) and node.clickable and not is_search_field(node)

        self.set_search(package)
        if dpad:
            # Real TV path: leave the field, move focus down onto the filtered
            # row (the page scrolls it into view) and toggle it with Center.
            self.leave_text_field()
            row = self.focus_onto(is_row, "KEYCODE_DPAD_DOWN")
            if row.checked != wanted:
                self.key("KEYCODE_DPAD_CENTER")
                row = self.focus_onto(is_row, "KEYCODE_DPAD_DOWN", presses=1)
        else:
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
            for package in selected:
                self.set_app(package, True, dpad)
            for package in unselected:
                self.set_app(package, False, dpad)
            self.set_search("")

        def is_save(node: Node) -> bool:
            return node.has(SAVE) and node.clickable and node.enabled

        if dpad:
            self.leave_text_field()
            self.focus_onto(is_save, "KEYCODE_DPAD_UP", presses=40)
            self.key("KEYCODE_DPAD_CENTER")
        else:
            self.tap(self.scroll_to(is_save, towards_top=True))
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if not any(node.has(MODE_LABELS["exclude"]) for node in self.dump()):
                self.say(f"mode {mode}: saved")
                return
            time.sleep(1.5)
        raise HarnessError(f"saving mode {mode} did not close the page")

    # Routing rules ------------------------------------------------------

    def run_as(self, script: str, data: bytes | None = None, timeout: float = 120) -> str:
        """Run `script` as the debuggable app, optionally with stdin `data`."""

        command = [
            self.adb,
            "-s",
            self.serial,
            "shell",
            f"run-as {PACKAGE} sh -c {shlex.quote(script)}",
        ]
        result = subprocess.run(  # noqa: S603 - fixed adb executable and harness arguments
            command, input=data, capture_output=True, timeout=timeout, check=False
        )
        if result.returncode != 0:
            raise HarnessError(f"run-as failed: {redact(result.stderr.decode(errors='replace'))}")
        return result.stdout.decode(errors="replace")

    def seed_rules(self, rules: list[dict]) -> None:
        """Stop the app and store `rules` in its configuration file."""

        self.shell(f"am force-stop {PACKAGE}")
        time.sleep(2)
        config = json.loads(self.run_as(f"cat {CONFIG}"))
        config["network"]["routing"]["rules"] = rules
        data = json.dumps(config, separators=(",", ":")).encode()
        self.run_as(f"cat > {CONFIG}.new && mv {CONFIG}.new {CONFIG}", data=data)
        stored = self.run_as(f"sha256sum {CONFIG}").split()[0]
        if stored != hashlib.sha256(data).hexdigest():
            raise HarnessError("the seeded configuration was not stored intact")
        self.say(f"rules: stored {len(rules)} rules in a {len(data)}-byte configuration")

    def handoff_files(self) -> int:
        listing = self.run_as(f"ls {HANDOFF_DIRECTORY} 2>/dev/null; true")
        return len([line for line in listing.splitlines() if line.strip()])

    def open_bypass_settings(self) -> None:
        def is_row(node: Node) -> bool:
            return node.has(BYPASS_SETTINGS) and node.clickable

        nodes = self.dump()
        row = next((node for node in nodes if is_row(node)), None)
        if row is None:
            self.tap(self.wait_label(SETTINGS))
            row = self.scroll_to(is_row)
        self.tap(row)
        # The search field exists only for lists longer than 50 rules. Phones
        # expose it as an EditText without the label; TVs as a hinted view.
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            nodes = self.dump()
            if any(node.has(BACK) for node in nodes) and any(
                is_search_field(node, RULE_SEARCH) for node in nodes
            ):
                self.save_dump("rules-page")
                self.say("rules: the editor shows the search field for the large list")
                return
            time.sleep(2)
        raise HarnessError("the rule search field did not appear")

    def type_rule_search(self, value: str) -> None:
        """Type into the empty rule search only once it is being edited."""

        def editing() -> bool:
            return any(
                node.focused and node.class_name.endswith("EditText") for node in self.dump()
            )

        self.tap(self.scroll_to(lambda node: is_search_field(node, RULE_SEARCH)))
        if not editing():
            # A TV field is browsed until activated.
            self.key("KEYCODE_DPAD_CENTER")
        if not editing():
            raise HarnessError("the rule search field did not start editing")
        self.shell(f"input text {shlex.quote(value)}")
        time.sleep(2)
        field = self.scroll_to(lambda node: is_search_field(node, RULE_SEARCH))
        if value not in field.labels:
            raise HarnessError("the rule search text did not land in the field")
        # Finish editing so taps and the D-pad reach the filtered rows (TV);
        # harmless on phones.
        self.leave_text_field()

    def save_app_log(self, lines: int = 80) -> None:
        """Keep the app's own event log tail, without addresses or IDs."""

        if self.artifacts is None:
            return
        try:
            text = self.run_as(f"tail -n {lines} {APP_LOG} 2>/dev/null; true")
        except HarnessError:
            return
        (self.artifacts / "app-log-tail.txt").write_text(
            UUID_PATTERN.sub("[uuid]", redact(text)), encoding="utf-8"
        )

    def delete_rule(self, target: str) -> None:
        """Find `target` through the editor search, delete it and save."""

        def is_row(node: Node) -> bool:
            return (
                node.mentions(target) and node.clickable and not is_search_field(node, RULE_SEARCH)
            )

        self.type_rule_search(target)
        self.save_dump("rules-search")
        row = self.scroll_to(is_row)
        nodes = self.dump()
        top, bottom = row.bounds[1], row.bounds[3]
        menu = next(
            (
                node
                for node in nodes
                if node.has(EDIT)
                and node.clickable
                and not node.mentions(target)
                and top <= node.center[1] <= bottom
            ),
            None,
        )
        if menu is not None:
            self.tap(menu)
        else:
            # The trailing menu button sits at the row's end edge.
            self.say("rules: menu button not exposed; tapping the row's end")
            self.shell(f"input tap {row.bounds[2] - 48} {row.center[1]}")
            time.sleep(1.2)
        self.tap(self.wait_label(DELETE, timeout=20))
        if any(is_row(node) for node in self.dump()):
            raise HarnessError("the rule is still listed after Delete")

        def is_save(node: Node) -> bool:
            return node.has(SAVE) and node.clickable and node.enabled

        self.tap(self.scroll_to(is_save))
        self.say("rules: deleted the DIRECT rule and saved")

    # Observations -------------------------------------------------------

    def vpn_ranges(self, owner_uid: int) -> list[tuple[int, int]] | None:
        dumpsys = self.shell("dumpsys connectivity", timeout=60)
        self.last_networks = network_summary(dumpsys, owner_uid)
        return parse_uid_ranges(dumpsys, owner_uid)

    def route(self, uid: int) -> str:
        output = self.shell(f"ip -4 route get {ROUTE_PROBE} uid {uid}", check=False)
        return parse_route_device(output)

    def warp(self, package: str, host: str = TRACE_HOST) -> str | None:
        """Fetch Cloudflare's trace from `host` as `package`; return `warp`."""

        request = (
            f"GET /cdn-cgi/trace HTTP/1.1\\r\\nHost: {host}\\r\\n"
            "User-Agent: usque-android-e2e\\r\\nConnection: close\\r\\n\\r\\n"
        )
        # Keep stdin open so nc reads the reply before the server closes.
        script = f"(printf '{request}'; sleep 6) | nc -w 15 {host} 80"
        command = f"run-as {package} sh -c {shlex.quote(script + ' 2>&1')}"
        for _ in range(3):
            output = self.shell(command, timeout=45, check=False)
            value = parse_warp(output)
            if value is not None:
                self.last_trace_failure = ""
                return value
            self.last_trace_failure = trace_failure(output)
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
            warp = device.warp(name)
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


def wait_warp(device: Device, package: str, host: str, wanted: str, timeout: float) -> str | None:
    deadline = time.monotonic() + timeout
    value = None
    while time.monotonic() < deadline:
        value = device.warp(package, host)
        if value == wanted:
            return value
        time.sleep(6)
    return value


def rule_phases(device: Device, owner_uid: int, probe: str):
    """Connect with the largest rule set, then delete its DIRECT rule in the UI.

    The profile (about 1.1 MB) crosses into the VPN process through the file
    handoff; the save sends the settings request and receives the settings
    result the same way. The probe stays inside the VPN in both phases.
    """

    rules = largest_rule_set(TRACE_HOST)
    device.seed_rules(rules)
    device.launch()
    retries = device.connect(owner_uid)
    in_vpn = uid_in(device.vpn_ranges(owner_uid) or [], device.package_uid(probe))
    tunnel = wait_warp(device, probe, TUNNEL_HOST, "on", 150)
    tunnel_failure = device.last_trace_failure
    direct = wait_warp(device, probe, TRACE_HOST, "off", 150)
    direct_failure = device.last_trace_failure
    phase = {
        "rules": len(rules),
        "target_bytes": sum(len(rule["target"].encode()) for rule in rules),
        "connect_retries": retries,
        "in_vpn_uid_ranges": in_vpn,
        "tunnel_trace_warp": tunnel,
        "direct_rule_trace_warp": direct,
        "passed": in_vpn and direct == "off" and tunnel == "on",
    }
    if tunnel is None:
        phase["tunnel_trace_failure"] = tunnel_failure
    if direct is None:
        phase["direct_rule_trace_failure"] = direct_failure
    if not phase["passed"]:
        device.save_app_log()
    yield "5-rules-connect", phase

    device.open_bypass_settings()
    device.delete_rule(TRACE_HOST)
    after = wait_warp(device, probe, TRACE_HOST, "on", 180)
    leftovers = device.handoff_files()
    for _ in range(4):
        if leftovers == 0:
            break
        time.sleep(5)
        leftovers = device.handoff_files()
    phase = {
        "direct_rule_trace_warp": after,
        "handoff_files_left": leftovers,
        "passed": after == "on" and leftovers == 0,
    }
    if after is None:
        phase["direct_rule_trace_failure"] = device.last_trace_failure
    if not phase["passed"]:
        device.save_app_log()
    yield "6-rules-edit", phase


def run(arguments: argparse.Namespace) -> dict:
    device = Device(arguments.adb, arguments.serial, artifacts=arguments.report.parent)
    report: dict = {"device": arguments.device, "phases": {}, "passed": False}
    try:
        device.wait_boot()
        device.prepare()
        report["sdk"] = device.shell("getprop ro.build.version.sdk").strip()
        report["characteristics"] = device.shell("getprop ro.build.characteristics").strip()
        device.install(arguments.apk)
        owner_uid = device.package_uid(PACKAGE)
        probes = device.install_probes(arguments.probe_apk)
        (first, _), (second, _) = probes
        report["probes"] = [first, second]
        device.say(f"probes: {first} (A), {second} (B)")

        baseline = device.warp(first)
        report["phases"]["disconnected"] = {
            "passed": baseline == "off",
            "trace_warp": baseline,
        }

        device.launch()
        device.complete_setup()
        report["connect_retries"] = device.connect(owner_uid)
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
        for name, phase in rule_phases(device, owner_uid, first):
            report["phases"][name] = phase
            device.say(f"phase {name}: {'pass' if phase['passed'] else 'FAIL'}")
        report["passed"] = all(phase["passed"] for phase in report["phases"].values())
    except (HarnessError, subprocess.TimeoutExpired) as error:
        report["error"] = redact(str(error))
        report["networks"] = device.last_networks
        device.say(f"error: {report['error']}")
        if device.last_dump and arguments.ui_dump:
            arguments.ui_dump.write_text(redact(device.last_dump), encoding="utf-8")
        device.save_app_log()
    report["log"] = device.log
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apk", type=Path, required=True)
    parser.add_argument(
        "--probe-apk",
        type=Path,
        action="append",
        required=True,
        help="code-free debuggable probe app; pass once per probe",
    )
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
