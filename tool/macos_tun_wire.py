#!/usr/bin/env python3
"""Disposable Darwin feth receive-side probes, followed by the independent Linux HTTPS canary."""

from __future__ import annotations

import argparse
import errno
import json
import os
import re
import secrets
import signal
import socket
import struct
import subprocess
import sys
import time
from pathlib import Path

PHASES = ("before", "active", "retained", "after")
PROTOCOLS = ("tcp-syn", "udp", "dns")
FAMILIES = (
    (socket.AF_INET, "198.19.249.1", "198.19.249.2"),
    (socket.AF_INET6, "fd6e:7573:7175:ed::1", "fd6e:7573:7175:ed::2"),
)
IFCONFIG = "/sbin/ifconfig"
TCPDUMP = "/usr/sbin/tcpdump"
SCOPE_OPTIONS = {
    socket.AF_INET: (socket.IPPROTO_IP, 25),
    socket.AF_INET6: (socket.IPPROTO_IPV6, 125),
}
BLOCKED = {errno.EHOSTUNREACH, errno.ENETUNREACH, errno.EACCES, errno.EPERM}


def command(*args: str) -> str:
    return subprocess.check_output(args, text=True, timeout=8)  # noqa: S603 - fixed executables and test arguments.


def numbers(phase: str, base: int, family: int, kind: int) -> tuple[int, int]:
    source = base + PHASES.index(phase) * 6 + family * 3 + kind
    return source, 53 if kind == 2 else source + 500


def dns_query(phase: str, port: int, family: int) -> bytes:
    labels = (phase.encode("ascii"), b"usque", b"invalid")
    qname = b"".join(bytes([len(label)]) + label for label in labels) + b"\0"
    return (
        struct.pack("!6H", port, 256, 1, 0, 0, 0)
        + qname
        + struct.pack("!HH", 1 if family == 0 else 28, 1)
    )


def save(path: Path, state: dict) -> None:
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state), encoding="ascii")
    tmp.chmod(0o644)  # The unprivileged sender/uploader reads only synthetic fixture metadata.
    tmp.replace(path)


def cleanup(directory: Path) -> None:
    path = directory / "links.json"
    if not path.is_file():
        return
    links = json.loads(path.read_text())["links"]
    current = dict((name, index) for index, name in socket.if_nameindex())
    for name, index in reversed(links):
        if name not in current:
            continue
        if not re.fullmatch("feth[0-9]+", name) or current[name] != index:
            raise RuntimeError("refusing to remove a replaced test interface")
        command(IFCONFIG, name, "destroy")
    path.unlink()


def setup(directory: Path) -> None:
    directory.mkdir(mode=0o755, exist_ok=False)
    directory.chmod(0o755)
    state = {"links": [], "base": 31000 + secrets.randbelow(8000), "phases": []}
    path = directory / "links.json"
    try:
        for _ in range(2):
            name = command(IFCONFIG, "feth", "create").strip()
            if not re.fullmatch("feth[0-9]+", name):
                raise RuntimeError(f"unexpected allocated feth name: {name!r}")
            state["links"].append([name, socket.if_nametoindex(name)])
            save(path, state)
        sender, peer = (link[0] for link in state["links"])
        command(IFCONFIG, sender, "peer", peer)
        command(IFCONFIG, peer, "up")
        command(IFCONFIG, sender, "up")
        # Only the sender owns IP addresses; assigning the destination IP to
        # the peer would let the local kernel shortcut the virtual wire on lo0.
        command(IFCONFIG, sender, "inet", FAMILIES[0][1], "netmask", "255.255.255.252", "alias")
        command(IFCONFIG, sender, "inet6", FAMILIES[1][1], "prefixlen", "64", "-dad", "alias")
        peer_mac = mac(peer)
        for family, _, dest in FAMILIES:
            route = command(
                "/sbin/route", "-n", "get", "-inet" if family == socket.AF_INET else "-inet6", dest
            )
            if not re.search(rf"^\s*interface:\s*{sender}\s*$", route, re.M):
                raise RuntimeError(f"synthetic probe route is not on {sender}: {route}")
        command("/usr/sbin/arp", "-s", FAMILIES[0][2], peer_mac, "ifscope", sender)
        command("/usr/sbin/ndp", "-s", FAMILIES[1][2], peer_mac)
        save(path, state)
    except BaseException:
        cleanup(directory)
        raise


def mac(interface: str) -> str:
    match = re.search(r"\bether ([0-9a-f]{2}(?::[0-9a-f]{2}){5})", command(IFCONFIG, interface))
    if match is None:
        raise RuntimeError(f"missing Ethernet address for {interface}")
    return match.group(1)


def bind_source(sock: socket.socket, address: str, port: int, interface: str) -> None:
    deadline = time.monotonic() + 5
    while True:
        try:
            sock.bind((address, port))
            return
        except OSError as error:
            if error.errno != errno.EADDRNOTAVAIL:
                raise
            if time.monotonic() >= deadline:
                raise RuntimeError(
                    f"probe source {address} still unavailable on {interface}: {command(IFCONFIG, interface)}"
                ) from error
            time.sleep(0.1)


def transmit(state: dict, phase: str) -> None:
    if os.geteuid() == 0:
        raise RuntimeError("packet probes must run unprivileged")
    interface, index = state["links"][0]
    for f, (family, source, dest) in enumerate(FAMILIES):
        for k, kind in enumerate(PROTOCOLS):
            sp, dp = numbers(phase, state["base"], f, k)
            with socket.socket(family, socket.SOCK_STREAM if k == 0 else socket.SOCK_DGRAM) as sock:
                sock.setsockopt(*SCOPE_OPTIONS[family], index)
                bind_source(sock, source, sp, interface)
                sock.settimeout(0.4)
                if k == 0:
                    value = sock.connect_ex((dest, dp))
                    print(f"{phase} {interface} {source}:{sp} tcp connect_ex={value}", flush=True)
                    if value not in BLOCKED | {
                        0,
                        errno.ECONNREFUSED,
                        errno.ETIMEDOUT,
                        errno.EAGAIN,
                        errno.EINPROGRESS,
                    }:
                        raise OSError(value, os.strerror(value))
                else:
                    payload = (
                        dns_query(phase, sp, f)
                        if kind == "dns"
                        else f"usque/{phase}/udp".encode("ascii")
                    )
                    try:
                        length = sock.sendto(payload, (dest, dp))
                        print(
                            f"{phase} {interface} {source}:{sp} {kind} wrote {length} bytes",
                            flush=True,
                        )
                    except OSError as error:
                        if error.errno not in BLOCKED:
                            raise
                        print(
                            f"{phase} {interface} {source}:{sp} {kind} send errno={error.errno}",
                            flush=True,
                        )


def capture_filters(state: dict, phase: str) -> dict[str, str]:
    tests = {}
    for f, (_, source, dest) in enumerate(FAMILIES):
        for k, kind in enumerate(PROTOCOLS):
            sp, dp = numbers(phase, state["base"], f, k)
            proto = "tcp" if k == 0 else "udp"
            # Fixed-header SYN from this test's ordinary TCP socket.
            syn = (" and tcp[13] & 18 = 2" if f == 0 else " and ip6[53] & 18 = 2") if k == 0 else ""
            family = "ip6" if f else "ip"
            tests[f"{family}/{kind}"] = (
                f"{family} and src host {source} and dst host {dest} and "
                f"{proto} src port {sp} and dst port {dp}{syn}"
            )
    return tests


def verify_counts(phase: str, counts: dict[str, int]) -> None:
    permitted = phase in ("before", "after")
    if len(counts) != 6 or any((n > 0) != permitted for n in counts.values()):
        raise RuntimeError(f"unexpected link-peer observations during {phase}: {counts}")


def probe(directory: Path, config: Path, phase: str) -> None:
    path = directory / "links.json"
    state = json.loads(path.read_text())
    if state["phases"] != list(PHASES[: PHASES.index(phase)]):
        raise RuntimeError("wire phases not executed in order")
    # Each phase is attempted once. Its exit status records acceptance; a
    # failed capture must not prevent fresh measurements in later phases.
    state["phases"].append(phase)
    save(path, state)
    peer, index = state["links"][1]
    if socket.if_nametoindex(peer) != index:
        raise RuntimeError("capture peer was replaced")
    filters = capture_filters(state, phase)
    capture = directory / f"{phase}.pcap"
    logpath = directory / f"{phase}.log"
    # On the otherwise unconfigured peer use the same promiscuous BPF receive
    # path as a Darwin TAP. Store only the test addresses and per-phase ports.
    match = " or ".join(f"({p})" for p in filters.values())
    with (
        logpath.open("w") as log,
        subprocess.Popen(  # noqa: S603 - capture only synthetic per-phase frames on our peer.
            [
                TCPDUMP,
                "-i",
                peer,
                "-n",
                "--immediate-mode",
                "-U",
                "-s",
                "160",
                "-w",
                str(capture),
                match,
            ],
            stdout=log,
            stderr=log,
        ) as sniff,
    ):
        try:
            deadline = time.monotonic() + 5
            while "listening on" not in logpath.read_text():
                if sniff.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError(f"capture did not start: {logpath.read_text()}")
                time.sleep(0.05)
            owner = config.stat()
            if owner.st_uid == 0:
                raise RuntimeError("expected observer config owned by ordinary runner")
            subprocess.run(  # noqa: S603 - execute this script under the runner UID.
                [sys.executable, str(Path(__file__).resolve()), "send", phase],
                user=owner.st_uid,
                group=owner.st_gid,
                extra_groups=[],
                check=True,
                timeout=16,
            )
            time.sleep(0.5)
            if sniff.poll() is not None:
                raise RuntimeError("capture stopped during probe transmission")
        finally:
            sniff.send_signal(signal.SIGINT)
            sniff.wait(timeout=5)
    if sniff.returncode or "0 packets dropped by kernel" not in logpath.read_text().splitlines():
        raise RuntimeError(f"capture incomplete: {logpath.read_text()}")
    capture.chmod(0o644)  # Filtered test frames are uploaded as the unprivileged runner.
    counts = {
        key: len(command(TCPDUMP, "-nn", "-q", "-r", str(capture), expr).splitlines())
        for key, expr in filters.items()
    }
    print(f"Darwin virtual Ethernet receive-side {phase}: {json.dumps(counts)}", flush=True)
    save(directory / f"{phase}.json", counts)
    try:
        verify_counts(phase, counts)
    except RuntimeError:
        print(f"Receive-side capture: {logpath.read_text()}", flush=True)
        for name, _ in state["links"]:
            print(command(IFCONFIG, name), flush=True)
            print(command("/usr/sbin/netstat", "-ibn", "-I", name), flush=True)
        raise


def probe_both(directory: Path, config: Path, phase: str) -> None:
    errors = []
    try:
        probe(directory, config, phase)
    except Exception as error:
        errors.append(f"wire: {error}")
    try:
        subprocess.run(  # noqa: S603 - fixed sibling script.
            [
                sys.executable,
                str(Path(__file__).with_name("macos_tun_observer.py")),
                "probe",
                phase,
            ],
            check=True,
        )
    except (OSError, subprocess.SubprocessError) as error:
        errors.append(f"HTTPS: {error}")
    if errors:
        raise RuntimeError(f"{phase} observation failed: " + "; ".join(errors))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for action in ("probe", "send"):
        sub.add_parser(action).add_argument("phase", choices=PHASES)
    sub.add_parser("setup")
    sub.add_parser("cleanup")
    args = parser.parse_args()
    if (
        sys.platform != "darwin"
        or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted"
        or os.environ.get("USQUE_DISPOSABLE_MAC") != "1"
        or os.environ.get("GITHUB_ACTIONS") != "true"
    ):
        raise RuntimeError("this test requires a disposable GitHub-hosted macOS runner")
    if args.command != "send" and os.geteuid() != 0:
        raise RuntimeError("virtual wire setup/capture requires root")
    config = Path(os.environ["USQUE_TUN_OBSERVER_CONFIG"])
    directory = config.parent / "wire"
    if args.command == "setup":
        setup(directory)
    elif args.command == "cleanup":
        cleanup(directory)
    elif args.command == "send":
        transmit(json.loads((directory / "links.json").read_text()), args.phase)
    else:
        probe_both(directory, config, args.phase)


if __name__ == "__main__":
    main()
