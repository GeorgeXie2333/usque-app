#!/usr/bin/env python3
"""External HTTPS canary for disposable macOS runners. Only synthetic test markers travel."""

from __future__ import annotations

import argparse
import contextlib
import errno
import hashlib
import http.client
import http.server
import ipaddress
import json
import os
import platform
import re
import secrets
import shutil
import socket
import ssl
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

ARCHES = ("arm64", "x86_64")
PHASES = ("before", "active", "retained", "after")
CONTROL = ("before", "after")
TUNNEL_HOST = r"[a-z0-9]+(?:-[a-z0-9]+)*\.trycloudflare\.com"
RELEASE = (
    "https://github.com/cloudflare/cloudflared/releases/download/2026.11.0/cloudflared-linux-amd64"
)
SHA256 = "51f7c68b57ed7269164075d7e789efce5b59355982f06392deeb037d982a51fd"


def write_json(path: Path, data: dict) -> None:
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(data), encoding="ascii")
    temporary.replace(path)


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="ascii"))


def canary_passed(events: dict) -> bool:
    return all(events.get(p, 0) > 0 for p in CONTROL) and all(
        events.get(p, 0) == 0 for p in ("active", "retained")
    )


class Receiver(http.server.BaseHTTPRequestHandler):
    """Single-threaded loopback origin, reached through a fresh outbound TLS tunnel."""

    timeout = 4

    def log_message(self, *args) -> None:
        pass  # Do not put challenge values or incoming headers in public job logs.

    def do_POST(self) -> None:
        parts = self.path.strip("/").split("/")
        if len(parts) != 4:
            self.send_error(404)
            return
        target, arch, session, phase = parts
        server = self.server
        if not (
            secrets.compare_digest(target, server.target)
            and arch in ARCHES
            and re.fullmatch("[0-9a-f]{32}", session)
            and phase in (*PHASES, "ready", "finish")
        ):
            self.send_error(404)
            return
        if phase == "ready":
            self.respond(200, session)
            return
        if phase == "before" and arch not in server.sessions:
            server.sessions[arch] = session
        if server.sessions.get(arch) != session:
            self.send_error(409)
            return
        events = server.observations[arch]
        events[phase] = events.get(phase, 0) + 1
        write_json(server.report, server.observations)
        print(f"External Linux receiver: {arch} {phase}", flush=True)
        self.respond(200 if phase != "finish" or canary_passed(events) else 409, session)

    def respond(self, status: int, body: str) -> None:
        self.send_response(status)
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body.encode("ascii"))


def serve(directory: Path) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    binary = directory / "cloudflared"
    with urllib.request.urlopen(RELEASE, timeout=60) as response, binary.open("wb") as output:  # noqa: S310 - fixed HTTPS release URL and digest.
        shutil.copyfileobj(response, output)
    if hashlib.sha256(binary.read_bytes()).hexdigest() != SHA256:
        raise RuntimeError("unexpected Cloudflare release digest")
    binary.chmod(0o700)
    # The tunnel forwards only to this ephemeral loopback listener. No runner
    # token, workspace files, command handler or upstream proxy is exposed.
    with http.server.HTTPServer(("127.0.0.1", 0), Receiver) as server:
        server.target = secrets.token_hex(24)
        server.report = directory / "report.json"
        server.sessions = {}
        server.observations = {arch: {} for arch in ARCHES}
        write_json(server.report, server.observations)
        log = directory / "connector.log"
        with (
            log.open("w", encoding="utf-8") as out,
            subprocess.Popen(  # noqa: S603 - fixed verified executable, loopback destination.
                [
                    str(binary),
                    "tunnel",
                    "--no-autoupdate",
                    "--protocol",
                    "http2",
                    "--url",
                    f"http://127.0.0.1:{server.server_port}",
                ],
                stdin=subprocess.DEVNULL,
                stdout=out,
                stderr=subprocess.STDOUT,
                env={"HOME": str(directory), "PATH": os.defpath},
            ) as tunnel,
        ):
            try:
                deadline = time.monotonic() + 90
                while tunnel.poll() is None and time.monotonic() < deadline:
                    match = re.search(rf"https://({TUNNEL_HOST})(?![a-z0-9.-])", log.read_text())
                    if match:
                        # Resolve the fresh hostname on Linux. The Mac pins these
                        # physical probes without warming its resolver before recovery.
                        try:
                            answers = socket.getaddrinfo(
                                match[1], 443, socket.AF_INET, socket.SOCK_STREAM
                            )
                        except socket.gaierror:
                            time.sleep(1)
                            continue
                        write_json(
                            directory / "endpoint.json",
                            {
                                "host": match[1],
                                "target": server.target,
                                "addresses": sorted({entry[4][0] for entry in answers}),
                            },
                        )
                        server.timeout = 2
                        while tunnel.poll() is None:
                            server.handle_request()
                        break
                    time.sleep(0.5)
                raise RuntimeError("external observer tunnel failed, see connector.log")
            finally:
                tunnel.terminate()


def acknowledge(conn: http.client.HTTPConnection, config: dict, phase: str) -> None:
    path = "/".join((config["target"], config["arch"], config["session"], phase))
    conn.request("POST", "/" + path, body=b"")
    response = conn.getresponse()
    if response.status != 200 or response.read(128).decode() != config["session"]:
        raise RuntimeError(f"external receiver did not acknowledge {phase}")


def bound_request(config: dict, phase: str, *, forbid_connect: bool = False) -> None:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as raw:
        raw.settimeout(4)
        # Darwin netinet/in.h IP_BOUND_IF. Bind raw IPv4 before TLS; no system
        # proxy, DNS lookup, tunnel routing fallback, or HTTP redirect.
        raw.setsockopt(socket.IPPROTO_IP, 25, config["ifindex"])
        try:
            raw.connect((config["address"], 443))
        except OSError as error:
            if forbid_connect and (
                isinstance(error, TimeoutError)
                or error.errno in (errno.EHOSTUNREACH, errno.ENETUNREACH, errno.EACCES, errno.EPERM)
            ):
                print(f"{phase}: physical-bound TCP connect blocked; external audit pending")
                return
            raise
        with ssl.create_default_context().wrap_socket(raw, server_hostname=config["host"]) as tls:
            with contextlib.closing(http.client.HTTPConnection(config["host"])) as conn:
                conn.sock = tls
                acknowledge(conn, config, phase)
    if forbid_connect:
        raise RuntimeError(f"physical-bound HTTPS probe escaped the Kill Switch during {phase}")


def prepare(endpoint: Path, path: Path) -> None:
    if sys.platform != "darwin" or os.getuid() == 0 or platform.machine() not in ARCHES:
        raise RuntimeError("prepare requires the unprivileged native macOS runner")
    config = read_json(endpoint)
    host = config["host"]
    if not re.fullmatch(TUNNEL_HOST, host) or not re.fullmatch(r"[0-9a-f]{48}", config["target"]):
        raise ValueError("invalid external observer endpoint")
    addresses = config.pop("addresses")
    if (
        not addresses
        or len(addresses) > 16
        or not all(ipaddress.IPv4Address(address).is_global for address in addresses)
    ):
        raise ValueError("invalid physical observer addresses")
    config.update(arch=platform.machine(), session=secrets.token_hex(16), phases=[])
    last_error = None
    for _ in range(12):
        try:
            for address in addresses:
                route = subprocess.check_output(  # noqa: S603 - OS command and validated numeric IP.
                    ["/sbin/route", "-n", "get", "-inet", address], text=True
                )
                match = re.search(r"^\s*interface:\s*(\S+)\s*$", route, flags=re.MULTILINE)
                if not match or match[1].startswith(("lo", "utun")):
                    raise RuntimeError("could not identify physical uplink for external observer")
                config.update(address=address, ifindex=socket.if_nametoindex(match[1]))
                try:
                    bound_request(config, "ready")
                except (OSError, RuntimeError, http.client.HTTPException) as error:
                    last_error = error
                    continue
                write_json(path, config)
                return
        except OSError as error:
            last_error = error
        time.sleep(2)
    raise RuntimeError(f"external observer preflight did not pass: {last_error}")


def probe(phase: str, path: Path) -> None:
    # The Agent harness runs as root. Probe as the identity that prepared the
    # CI config, like a regular application on the Mac. Never send credentials.
    if os.getuid() == 0:
        owner = path.stat()
        if owner.st_uid == 0:
            raise RuntimeError("expected the runner's unprivileged observer configuration")
        os.setgroups([])
        os.setgid(owner.st_gid)
        os.setuid(owner.st_uid)
    config = read_json(path)
    order = (*PHASES, "finish")
    if config["phases"] != list(order[: order.index(phase)]):
        raise RuntimeError(f"expected preceding probe phases before {phase}")
    if phase in ("active", "retained"):
        bound_request(config, phase, forbid_connect=True)
    elif phase == "finish":
        # HTTPSConnection resolves this run's hostname through macOS and uses
        # the default route. Success requires a matching receipt from Linux.
        with contextlib.closing(http.client.HTTPSConnection(config["host"], timeout=12)) as conn:
            acknowledge(conn, config, phase)
        print("finish: external HTTPS acknowledged after ordinary DNS and unbound connect")
    else:
        bound_request(config, phase)
        print(f"{phase}: Linux receiver acknowledged physical-bound probe")
    config["phases"].append(phase)
    write_json(path, config)


def wait(directory: Path) -> None:
    deadline = time.monotonic() + 70 * 60
    report = directory / "report.json"
    while time.monotonic() < deadline:
        observations = read_json(report)
        reported = {a for a in ARCHES if observations[a].get("finish")}
        terminal = set()
        # Keep observing the other Mac after a leak or job failure. A failed
        # harness may be unable to reach this listener to send its final marker.
        if len(reported) < len(ARCHES) and os.environ.get("GH_TOKEN"):
            request = urllib.request.Request(
                f"https://api.github.com/repos/{os.environ['GITHUB_REPOSITORY']}/actions/runs/"
                f"{os.environ['GITHUB_RUN_ID']}/attempts/{os.environ['GITHUB_RUN_ATTEMPT']}/jobs",
                headers={"Authorization": f"Bearer {os.environ['GH_TOKEN']}"},
            )
            try:
                with urllib.request.urlopen(request, timeout=10) as response:  # noqa: S310 - GitHub API only.
                    jobs = json.load(response)["jobs"]
                terminal = {
                    a
                    for a in ARCHES
                    for job in jobs
                    if f"macOS TUN E2E ({a})" in job["name"] and job["status"] == "completed"
                }
            except OSError as error:
                print(f"GitHub job-status lookup unavailable: {type(error).__name__}", flush=True)
        if set(ARCHES) <= reported | terminal:
            if not all(a in reported and canary_passed(observations[a]) for a in ARCHES):
                raise RuntimeError(
                    f"missing controls or protected traffic received: {observations}"
                )
            print(
                "External Linux HTTPS observations (finish uses normal DNS and routing):",
                json.dumps(observations),
            )
            return
        time.sleep(10)
    raise RuntimeError(f"timed out waiting for both architectures: {read_json(report)}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for cmd in ("serve", "wait", "prepare"):
        sub.add_parser(cmd).add_argument("path", type=Path)
    sub.add_parser("probe").add_argument("phase", choices=PHASES)
    sub.add_parser("finish")
    args = parser.parse_args()
    if args.command == "serve":
        serve(args.path)
    elif args.command == "wait":
        wait(args.path)
    else:
        config = Path(os.environ["USQUE_TUN_OBSERVER_CONFIG"])
        if args.command == "prepare":
            prepare(args.path, config)
        else:
            probe("finish" if args.command == "finish" else args.phase, config)


if __name__ == "__main__":
    main()
