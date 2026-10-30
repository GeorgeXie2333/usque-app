"""Browser-free contracts by default; browser tests require --allow-interactive.

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
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
POSIX_PROCESS_RUNNER_SUPPORTED = os.name == "posix"
ERROR_CODES = {
    "PROTON_VERIFICATION_BRIDGE",
    "PROTON_VERIFICATION_DOWNLOAD",
    "PROTON_VERIFICATION_EXPIRED",
    "PROTON_VERIFICATION_HOST_STOPPED",
    "PROTON_VERIFICATION_INTERACTIVE_DISABLED",
    "PROTON_VERIFICATION_INVALID",
    "PROTON_VERIFICATION_LOAD",
    "PROTON_VERIFICATION_NAVIGATION",
    "PROTON_VERIFICATION_TLS",
    "PROTON_VERIFICATION_UNAVAILABLE",
}


def isolated_directory(*, platform: str | None = None):
    """Use short macOS socket paths; other platforms use their standard temp."""
    selected = sys.platform if platform is None else platform
    return tempfile.TemporaryDirectory(
        prefix="usque-cef-", dir="/private/tmp" if selected == "darwin" else None
    )


def outcome(result: dict) -> dict:
    """Expose only fixed protocol labels, never messages, URLs or credentials."""
    reply = result.get("reply", {})
    if not isinstance(reply, dict):
        reply = {}
    status, code = reply.get("status"), reply.get("code")
    return {
        "exit": result.get("exit"),
        "status": status if status in ("success", "cancelled", "error") else "unrecognized",
        "code": code
        if isinstance(code, str) and code in ERROR_CODES
        else None
        if code is None
        else "unrecognized",
        "stderr_bytes": result.get("stderr_bytes"),
    }


class ValidationFailure(ValueError):
    def __init__(self, case: str, reason: str, result: dict, **observations):
        self.details = {"case": case, "reason": reason, **outcome(result), **observations}
        super().__init__(json.dumps(self.details, sort_keys=True))


def expect_error(result: dict, case: str, **observations) -> None:
    if result["exit"] != 0:
        raise ValidationFailure(case, "child_exit", result, **observations)
    if result["reply"] != {"status": "error", "code": "PROTON_VERIFICATION_LOAD"}:
        raise ValidationFailure(case, "unexpected_reply", result, **observations)


def terminate_group(process: subprocess.Popen, *, grace_seconds=3.0) -> None:
    """Reap this fixture's session, even if its leader has already exited."""
    if not POSIX_PROCESS_RUNNER_SUPPORTED:
        raise ValueError("The CEF subprocess runner requires POSIX process groups and pipes")
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        process.wait(timeout=3)
        return
    try:
        try:
            process.wait(timeout=grace_seconds)
        except subprocess.TimeoutExpired:
            pass
        # Always target the owned session again, even if wait() already reaped
        # its leader. A descendant can ignore SIGTERM after that leader exits.
        # Avoid signal-0 group probes, which restricted macOS runners can deny.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    finally:
        process.wait(timeout=3)


def execute(
    executable: Path,
    payload: bytes | None,
    *,
    smoke=False,
    keep_stdin=False,
    allow_interactive=False,
    timeout_seconds=40.0,
) -> dict:
    if smoke and not allow_interactive:
        raise ValueError("Browser smoke requires explicit --allow-interactive")
    if not 0 < timeout_seconds <= 40:
        raise ValueError("CEF fixture timeout must be within 0 and 40 seconds")
    if payload is not None and len(payload) > 65540:
        raise ValueError("CEF fixture input exceeds its frame bound")
    if not POSIX_PROCESS_RUNNER_SUPPORTED:
        raise ValueError("The CEF subprocess runner requires POSIX process groups and pipes")
    arguments = [str(executable), "--smoke-test" if smoke else "--stdio"]
    if smoke:
        arguments.append("--allow-interactive")
    elif not allow_interactive:
        # The executable must independently refuse even a valid proxy handoff
        # before loading CEF. Older executables reject this extra flag safely.
        arguments.append("--no-interactive")
    with isolated_directory() as directory:
        home = Path(directory) / "home"
        temporary = Path(directory) / "tmp"
        home.mkdir(mode=0o700)
        temporary.mkdir(mode=0o700)
        env = os.environ.copy()
        env.update(HOME=str(home), CFFIXED_USER_HOME=str(home), TMPDIR=str(temporary) + "/")
        process = subprocess.Popen(  # noqa: S603 - explicit local test executable.
            arguments,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            start_new_session=True,
        )
        selector = selectors.DefaultSelector()
        output, errors = bytearray(), bytearray()
        try:
            pending = memoryview(payload or b"")
            input_offset = 0
            if pending:
                os.set_blocking(process.stdin.fileno(), False)
                selector.register(process.stdin, selectors.EVENT_WRITE, None)
            elif not keep_stdin:
                process.stdin.close()
            for handle, buffer in ((process.stdout, output), (process.stderr, errors)):
                os.set_blocking(handle.fileno(), False)
                selector.register(handle, selectors.EVENT_READ, buffer)
            deadline = time.monotonic() + timeout_seconds
            while selector.get_map() or process.poll() is None:
                if time.monotonic() >= deadline:
                    raise TimeoutError("CEF fixture exceeded its bounded input/output deadline")
                for key, _ in selector.select(timeout=0.1):
                    if key.fileobj is process.stdin:
                        try:
                            input_offset += os.write(key.fd, pending[input_offset:])
                        except BlockingIOError:
                            continue
                        except BrokenPipeError:
                            selector.unregister(process.stdin)
                            process.stdin.close()
                            continue
                        if input_offset == len(pending):
                            selector.unregister(process.stdin)
                            if not keep_stdin:
                                process.stdin.close()
                        continue
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
            if input_offset != len(pending):
                raise ValueError("CEF closed its input before receiving the bounded frame")
            return {"exit": process.returncode, "reply": result, "stderr_bytes": len(errors)}
        finally:
            # Always reap only this fixture's process group, including a helper
            # left behind by a failed browser shutdown. No host-wide kill.
            try:
                terminate_group(process)
            finally:
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
        self.requests = 0
        self.challenges = 0
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
                    while (
                        len(data) < 16384
                        and not data.endswith(b"\r\n\r\n")
                        and not self.stopped.is_set()
                    ):
                        chunk = stream.recv(1)
                        if not chunk:
                            break
                        data.extend(chunk)
                    if not data.endswith(b"\r\n\r\n") or self.stopped.is_set():
                        continue
                    self.requests += 1
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
                        self.challenges += 1
                        stream.sendall(
                            (
                                "HTTP/1.1 407 Proxy Authentication Required\r\n"
                                f'Proxy-Authenticate: Basic realm="{self.realm}"\r\n'
                                "Content-Length: 0\r\nConnection: close\r\n\r\n"
                            ).encode()
                        )
                except OSError:
                    continue


def check(executable: Path, *, smoke=False, repeat_smoke=0, allow_interactive=False) -> dict:
    if smoke and not allow_interactive:
        raise ValueError("Browser smoke requires explicit --allow-interactive")
    if not 0 <= repeat_smoke <= 100 or (repeat_smoke and not smoke):
        raise ValueError("Renderer repeats require browser smoke and a count from 0 to 100")
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
    result = execute(executable, handoff(proxy_port=9))
    if result["exit"] == 0 or result["reply"] != {
        "status": "error",
        "code": "PROTON_VERIFICATION_INTERACTIVE_DISABLED",
    }:
        raise ValidationFailure("no_interactive_proxy_handoff", "browser_guard", result)
    cases.append("no_interactive_proxy_handoff")
    if smoke:
        result = execute(executable, None, smoke=True, allow_interactive=True)
        if (
            result["exit"] != 0
            or result["reply"].get("status") != "success"
            or json.loads(result["reply"].get("message", "{}")) != {"sandbox_renderer": "ready"}
        ):
            raise ValidationFailure(
                "sandbox_renderer_and_clean_shutdown", "renderer_result", result
            )
        cases.append("sandbox_renderer_and_clean_shutdown")
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
            # Keep the non-listening socket bound until the request finishes;
            # another process cannot reuse the supposedly unavailable port.
            result = execute(
                executable, handoff(proxy_port=port), keep_stdin=True, allow_interactive=True
            )
        expect_error(result, "proxy_failure_stops_browser")
        cases.append("proxy_failure_stops_browser")
        for wrong_realm in (False, True):
            with AuthenticationProxy(wrong_realm=wrong_realm) as proxy:
                case = "wrong_proxy_realm_rejected" if wrong_realm else "bound_proxy_authentication"
                result = execute(
                    executable,
                    handoff(proxy_port=proxy.port),
                    keep_stdin=True,
                    allow_interactive=True,
                )
                expect_error(
                    result,
                    case,
                    proxy_requests=proxy.requests,
                    proxy_challenges=proxy.challenges,
                    proxy_authorized=proxy.authorized,
                    proxy_received_credentials=proxy.received_credentials,
                )
                if wrong_realm and proxy.received_credentials:
                    raise ValueError("Credentials were supplied to the wrong proxy realm")
                if not wrong_realm and not proxy.authorized:
                    raise ValueError("Bound authenticated proxy did not receive its credentials")
            cases.append(
                "wrong_proxy_realm_rejected" if wrong_realm else "bound_proxy_authentication"
            )
        for index in range(repeat_smoke):
            result = execute(executable, None, smoke=True, allow_interactive=True)
            if result["exit"] != 0 or result["reply"].get("status") != "success":
                raise ValidationFailure(f"renderer_repeat_{index + 1}", "renderer_result", result)
    return {
        "passed": len(cases),
        "cases": cases,
        "official_challenge": "not_run",
        "intel_native_execution": "not_run",
        "repeated_renderer_smokes": repeat_smoke,
    }


def main(argv=None) -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--smoke", action="store_true")
    interactive = parser.add_mutually_exclusive_group()
    interactive.add_argument("--allow-interactive", action="store_true")
    interactive.add_argument("--no-interactive", dest="allow_interactive", action="store_false")
    parser.set_defaults(allow_interactive=False)
    parser.add_argument("--repeat-smoke", type=int, default=0)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    if not 0 <= args.repeat_smoke <= 100 or (args.repeat_smoke and not args.smoke):
        parser.error("--repeat-smoke requires --smoke and a count from 0 to 100")
    if args.smoke and not args.allow_interactive:
        parser.error("--smoke can open windows and requires explicit --allow-interactive")
    try:
        executable = args.app.resolve() / "Contents/MacOS/UsqueProtonVerification"
        result = check(
            executable,
            smoke=args.smoke,
            repeat_smoke=args.repeat_smoke,
            allow_interactive=args.allow_interactive,
        )
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
                        **(
                            {"failure": error.details}
                            if isinstance(error, ValidationFailure)
                            else {}
                        ),
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
