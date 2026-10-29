"""Isolated CEF executable contracts; --smoke additionally requires WindowServer.

Only synthetic credentials, the fixed local data page and a closed loopback
proxy are used. No official challenge or user browser profile is opened.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import selectors
import signal
import socket
import struct
import subprocess
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def execute(executable: Path, payload: bytes | None, *, smoke=False, keep_stdin=False) -> dict:
    with tempfile.TemporaryDirectory(prefix="usque-cef-", dir="/private/tmp") as directory:
        home = Path(directory) / "home"
        temporary = Path(directory) / "tmp"
        home.mkdir(mode=0o700)
        temporary.mkdir(mode=0o700)
        env = os.environ.copy()
        env.update(HOME=str(home), CFFIXED_USER_HOME=str(home), TMPDIR=str(temporary) + "/")
        process = subprocess.Popen(  # noqa: S603 - explicit local test executable.
            [str(executable), "--smoke-test" if smoke else "--stdio"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            start_new_session=True,
        )
        selector = selectors.DefaultSelector()
        output, errors = bytearray(), bytearray()
        try:
            if payload is not None:
                process.stdin.write(payload)
                process.stdin.flush()
            if not keep_stdin:
                process.stdin.close()
            for handle, buffer in ((process.stdout, output), (process.stderr, errors)):
                os.set_blocking(handle.fileno(), False)
                selector.register(handle, selectors.EVENT_READ, buffer)
            deadline = time.monotonic() + 40
            while selector.get_map() or process.poll() is None:
                if time.monotonic() >= deadline:
                    raise TimeoutError("CEF did not terminate cleanly within 40 seconds")
                for key, _ in selector.select(timeout=0.1):
                    chunk = os.read(key.fd, 65536)
                    if not chunk:
                        selector.unregister(key.fileobj)
                    else:
                        key.data.extend(chunk)
                    if len(output) > 131076 or len(errors) > 8192:
                        raise ValueError("CEF exceeded its output bound")
            if len(output) < 4 or struct.unpack(">I", output[:4])[0] != len(output) - 4:
                raise ValueError("CEF output is not one bounded frame")
            result = json.loads(output[4:])
            if not isinstance(result, dict):
                raise ValueError("CEF response is not an object")
            return {"exit": process.returncode, "reply": result, "stderr_bytes": len(errors)}
        finally:
            # Always reap only this fixture's process group, including a helper
            # left behind by a failed browser shutdown. No host-wide kill.
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=3)
            selector.close()
            for handle in (process.stdin, process.stdout, process.stderr):
                handle.close()


def handoff(*, proxy_port=0) -> bytes:
    value = {
        "version": 2,
        "route_epoch": int(bool(proxy_port)),
        "job_id": "isolated-job",
        "challenge_id": "isolated-challenge",
        "url": "https://verify.proton.me/?token=isolated-challenge&embed=true&methods=captcha",
        "challenge_sha256": hashlib.sha256(b"isolated-challenge").hexdigest(),
        "expires_at": int(time.time() * 1000) + 120000,
        "methods": ["captcha"],
    }
    if proxy_port:
        value["proxy"] = {
            "port": proxy_port,
            "username": "a" * 32,
            "password": "b" * 64,
            "nonce": "c" * 32,
        }
    payload = json.dumps(value).encode()
    return struct.pack(">I", len(payload)) + payload


class AuthenticationProxy:
    """A synthetic loopback proxy that never opens an upstream connection."""

    def __init__(self, *, wrong_realm=False):
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(8)
        self.listener.settimeout(0.2)
        self.port = self.listener.getsockname()[1]
        self.realm = "d" * 32 if wrong_realm else "c" * 32
        self.authorized = False
        self.received_credentials = False
        self.stopped = threading.Event()
        self.thread = threading.Thread(target=self.serve, daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.stopped.set()
        self.listener.close()
        self.thread.join(timeout=3)

    def serve(self):
        expected = b"Basic " + base64.b64encode(("a" * 32 + ":" + "b" * 64).encode())
        while not self.stopped.is_set():
            try:
                stream, _ = self.listener.accept()
            except TimeoutError:
                continue
            except OSError:
                return
            with stream:
                stream.settimeout(1)
                try:
                    data = bytearray()
                    while len(data) < 16384 and not data.endswith(b"\r\n\r\n"):
                        chunk = stream.recv(1)
                        if not chunk:
                            break
                        data.extend(chunk)
                    authorization = next(
                        (
                            line.split(b":", 1)[1].strip()
                            for line in data.split(b"\r\n")
                            if line.lower().startswith(b"proxy-authorization:")
                        ),
                        None,
                    )
                    self.received_credentials |= authorization is not None
                    self.authorized |= authorization == expected
                    if authorization == expected:
                        stream.sendall(
                            b"HTTP/1.1 502 Fixture Stops Here\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                    else:
                        stream.sendall(
                            (
                                "HTTP/1.1 407 Proxy Authentication Required\r\n"
                                f'Proxy-Authenticate: Basic realm="{self.realm}"\r\n'
                                "Content-Length: 0\r\nConnection: close\r\n\r\n"
                            ).encode()
                        )
                except OSError:
                    continue


def check(executable: Path, *, smoke=False, repeat_smoke=0) -> dict:
    cases = []
    for name, payload in (
        ("empty", b""),
        ("oversized", struct.pack(">I", 65537)),
        ("truncated", struct.pack(">I", 100) + b"{}"),
        ("invalid_handoff", struct.pack(">I", 2) + b"{}"),
    ):
        result = execute(executable, payload)
        if result["exit"] == 0 or result["reply"] != {
            "status": "error",
            "code": "PROTON_VERIFICATION_INVALID",
        }:
            raise ValueError(f"CEF accepted {name}")
        cases.append(name)
    result = execute(executable, handoff())
    message = json.loads(result["reply"].get("message", "{}"))
    if result["exit"] != 0 or message != {
        "type": "USQUE_VERIFICATION_NETWORK_FAILED",
        "route_epoch": 0,
    }:
        raise ValueError("Physical-route handoff did not request the authenticated proxy")
    cases.append("physical_handoff_requests_proxy")
    if smoke:
        result = execute(executable, None, smoke=True)
        if (
            result["exit"] != 0
            or result["reply"].get("status") != "success"
            or json.loads(result["reply"].get("message", "{}")) != {"sandbox_renderer": "ready"}
        ):
            raise ValueError(
                f"Sandbox renderer did not exit cleanly (exit={result['exit']}, "
                f"status={result['reply'].get('status')}, stderr_bytes={result['stderr_bytes']})"
            )
        cases.append("sandbox_renderer_and_clean_shutdown")
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        result = execute(executable, handoff(proxy_port=port), keep_stdin=True)
        if result["exit"] != 0 or result["reply"] != {
            "status": "error",
            "code": "PROTON_VERIFICATION_LOAD",
        }:
            raise ValueError("Unavailable authenticated proxy did not stop verification")
        cases.append("proxy_failure_stops_browser")
        for wrong_realm in (False, True):
            with AuthenticationProxy(wrong_realm=wrong_realm) as proxy:
                result = execute(executable, handoff(proxy_port=proxy.port), keep_stdin=True)
                if result["exit"] != 0 or result["reply"] != {
                    "status": "error",
                    "code": "PROTON_VERIFICATION_LOAD",
                }:
                    raise ValueError("Authentication proxy failure did not end cleanly")
                if wrong_realm and proxy.received_credentials:
                    raise ValueError("Credentials were supplied to the wrong proxy realm")
                if not wrong_realm and not proxy.authorized:
                    raise ValueError("Bound authenticated proxy did not receive its credentials")
            cases.append(
                "wrong_proxy_realm_rejected" if wrong_realm else "bound_proxy_authentication"
            )
        for index in range(repeat_smoke):
            result = execute(executable, None, smoke=True)
            if result["exit"] != 0 or result["reply"].get("status") != "success":
                raise ValueError(
                    f"Renderer repeat {index + 1} did not exit cleanly: {result['exit']}"
                )
    return {
        "passed": len(cases),
        "cases": cases,
        "official_challenge": "not_run",
        "intel_native_execution": "not_run",
        "repeated_renderer_smokes": repeat_smoke,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--smoke", action="store_true")
    parser.add_argument("--repeat-smoke", type=int, default=0)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 0 <= args.repeat_smoke <= 100 or (args.repeat_smoke and not args.smoke):
        parser.error("--repeat-smoke requires --smoke and a count from 0 to 100")
    try:
        executable = args.app.resolve() / "Contents/MacOS/UsqueProtonVerification"
        result = check(executable, smoke=args.smoke, repeat_smoke=args.repeat_smoke)
        text = json.dumps(result, indent=2) + "\n"
        if args.output:
            args.output.write_text(text, encoding="utf-8")
        print(text, end="")
    except (OSError, ValueError, TimeoutError) as error:
        if args.output:
            args.output.write_text(
                json.dumps(
                    {
                        "passed": False,
                        "error": str(error),
                        "official_challenge": "not_run",
                        "intel_native_execution": "not_run",
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )
        parser.exit(1, f"CEF isolated validation failed: {error}\n")


if __name__ == "__main__":
    main()
