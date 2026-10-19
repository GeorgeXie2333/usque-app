"""One-use loopback handoff for a puzzle the user solves on the official page.

The JavaScript is an independent event listener, not a CAPTCHA solver. Actual
browser navigation, mixed-content and CSP behavior remain unverified. The small
HTTP reader accepts only the single bounded native-form callback defined here.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import math
import re
import secrets
import socket
import time
from urllib.parse import parse_qsl

from .transport import PocError
from .verification import VerificationChallenge

_HEADER_LIMIT = 16 * 1024
_BODY_LIMIT = 40 * 1024
_CONNECTION_SECONDS = 3.0
_POLL_SECONDS = 0.2
_MAX_CONNECTIONS = 8
_OFFICIAL_ORIGIN = "https://verify.proton.me"
_HEADER_NAME = re.compile(rb"[!#$%&'*+.^_`|~0-9A-Za-z-]+")
_INVALID_PERCENT = re.compile(r"%(?![0-9A-Fa-f]{2})")


def _make_snippet(
    callback_url, state, challenge_digest, methods, timeout_ms, expires_at_wall_ms=None
):
    """Build exactly the fresh listener used by BrowserHandoff; no I/O."""
    if expires_at_wall_ms is None:
        expires_at_wall_ms = math.floor(time.time() * 1000 + timeout_ms)
    values = json.dumps(
        {
            "callback": callback_url,
            "state": state,
            "digest": challenge_digest,
            "methods": list(methods),
            "timeout": timeout_ms,
            "expiresAt": expires_at_wall_ms,
        },
        ensure_ascii=True,
    )
    return (
        "(async () => {\n"
        '  "use strict";\n'
        f"  const settings = {values};\n"
        '  const official = "https://verify.proton.me";\n'
        "  if (Date.now() >= settings.expiresAt) return;\n"
        "  const page = new URL(window.location.href);\n"
        "  if (window.top !== window || page.origin !== official) {\n"
        '    throw new Error("Open the official verification page as the top-level page.");\n'
        "  }\n"
        '  const candidates = page.searchParams.getAll("token");\n'
        "  if (candidates.length !== 1 || !candidates[0]) {\n"
        '    throw new Error("The current verification challenge does not match.");\n'
        "  }\n"
        "  const samePage = () => {\n"
        "    try {\n"
        "      const current = new URL(window.location.href);\n"
        '      const currentTokens = current.searchParams.getAll("token");\n'
        "      return window.top === window && current.origin === official &&\n"
        "        currentTokens.length === 1 && currentTokens[0] === candidates[0];\n"
        "    } catch { return false; }\n"
        "  };\n"
        "  const bytes = await crypto.subtle.digest(\n"
        '    "SHA-256", new TextEncoder().encode(candidates[0])\n'
        "  );\n"
        "  const digest = Array.from(new Uint8Array(bytes), value =>\n"
        '    value.toString(16).padStart(2, "0")).join("");\n'
        "  if (digest !== settings.digest) {\n"
        '    throw new Error("The current verification challenge does not match.");\n'
        "  }\n"
        "  if (Date.now() >= settings.expiresAt || !samePage()) return;\n"
        "  let used = false;\n"
        "  const handler = event => {\n"
        "    if (Date.now() >= settings.expiresAt || !samePage()) {\n"
        '      used = true; window.removeEventListener("message", handler);\n'
        "      clearTimeout(timer); return;\n"
        "    }\n"
        "    if (used || event.origin !== official || event.source !== window ||\n"
        '        typeof event.data !== "string" || event.data.length > 32768) return;\n'
        "    let data;\n"
        "    try { data = JSON.parse(event.data); } catch { return; }\n"
        '    if (!data || data.type !== "HUMAN_VERIFICATION_SUCCESS" ||\n'
        "        !data.payload || Array.isArray(data.payload) ||\n"
        '        typeof data.payload.token !== "string" ||\n'
        "        !/^[\\x21-\\x7e]{1,8192}$/.test(data.payload.token) ||\n"
        '        !["captcha", "sms", "email"].includes(data.payload.type) ||\n'
        "        !settings.methods.includes(data.payload.type)) return;\n"
        "    used = true;\n"
        '    window.removeEventListener("message", handler);\n'
        "    clearTimeout(timer);\n"
        '    const form = document.createElement("form");\n'
        '    form.method = "POST";\n'
        "    form.action = settings.callback;\n"
        '    form.target = "_self";\n'
        '    form.enctype = "application/x-www-form-urlencoded";\n'
        "    const fields = {state: settings.state, message: event.data};\n"
        "    for (const [name, value] of Object.entries(fields)) {\n"
        '      const input = document.createElement("input");\n'
        '      input.type = "hidden"; input.name = name; input.value = value;\n'
        "      form.appendChild(input);\n"
        "    }\n"
        "    // A meta referrer policy updates the document policy. Using origin\n"
        "    // is intended to preserve Origin on HTTPS-to-loopback form POST\n"
        "    // without sending the challenge query in Referer. Browser behavior\n"
        "    // is not yet verified; the callback still rejects Origin: null.\n"
        '    const policy = document.createElement("meta");\n'
        '    policy.name = "referrer"; policy.content = "origin";\n'
        "    document.head.appendChild(policy);\n"
        "    document.body.appendChild(form);\n"
        "    HTMLFormElement.prototype.submit.call(form);\n"
        "  };\n"
        "  const timer = setTimeout(() => {\n"
        '    used = true; window.removeEventListener("message", handler);\n'
        "  }, Math.max(0, Math.min(settings.timeout, settings.expiresAt - Date.now())));\n"
        '  window.addEventListener("message", handler);\n'
        "})();"
    )


class BrowserHandoff:
    """A bounded, one-use IPv4 loopback listener. It never opens a browser."""

    def __init__(self, challenge):
        if not isinstance(challenge, VerificationChallenge):
            raise PocError("invalid_verification_challenge")
        if not isinstance(challenge.created_at, (int, float)) or not math.isfinite(
            challenge.created_at
        ):
            raise PocError("invalid_verification_challenge")
        try:
            self._digest = hashlib.sha256(challenge.token.encode("utf-8")).hexdigest()
        except (AttributeError, UnicodeError):
            raise PocError("invalid_verification_challenge") from None
        self._challenge = challenge
        self._state = secrets.token_urlsafe(32)
        self._listener = None
        self._connection = None
        self._port = 0
        self._deadline = 0.0
        self._entered = False
        self._finished = False
        self._closed = False

    def __enter__(self):
        if self._entered or self._finished or self._closed:
            raise PocError("browser_handoff_already_used")
        self._entered = True
        now = time.monotonic()
        self._deadline = min(now + 300, self._challenge.created_at + 300)
        if self._deadline <= now:
            self.close()
            raise PocError("human_verification_expired")
        try:
            self._listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            self._listener.set_inheritable(False)
            self._listener.bind(("127.0.0.1", 0))
            self._listener.listen(1)
            self._listener.settimeout(_POLL_SECONDS)
            self._port = self._listener.getsockname()[1]
        except OSError:
            self.close()
            raise PocError("browser_handoff_unavailable") from None
        return self

    def __exit__(self, *args):
        self.close()

    @property
    def snippet(self):
        if not self._entered or self._closed or self._finished:
            raise PocError("browser_handoff_not_active")
        remaining = self._deadline - time.monotonic()
        if remaining <= 0:
            raise PocError("human_verification_expired")
        return _make_snippet(
            f"http://127.0.0.1:{self._port}/complete",
            self._state,
            self._digest,
            self._challenge.methods,
            max(1, math.floor(remaining * 1000)),
            math.floor(time.time() * 1000 + remaining * 1000),
        )

    def close(self):
        self._closed = True
        for resource in (self._connection, self._listener):
            if resource is not None:
                try:
                    resource.close()
                except OSError:
                    pass
        self._connection = self._listener = None

    def _receive(self, connection, deadline):
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise PocError("browser_handoff_request_timeout")
            if self._closed:
                raise PocError("browser_handoff_cancelled")
            try:
                connection.settimeout(min(_POLL_SECONDS, remaining))
                data = connection.recv(4096)
            except TimeoutError:
                continue
            except OSError:
                raise PocError("browser_handoff_invalid_request") from None
            if not data:
                raise PocError("browser_handoff_invalid_request")
            return data

    def _read_request(self, connection, deadline):
        data = bytearray()
        while b"\r\n\r\n" not in data:
            data.extend(self._receive(connection, deadline))
            if len(data) > _HEADER_LIMIT and b"\r\n\r\n" not in data:
                raise PocError("browser_handoff_invalid_request")
        offset = data.index(b"\r\n\r\n") + 4
        if offset > _HEADER_LIMIT:
            raise PocError("browser_handoff_invalid_request")
        lines = bytes(data[: offset - 4]).split(b"\r\n")
        if not lines or lines[0] != b"POST /complete HTTP/1.1" or len(lines) > 65:
            raise PocError("browser_handoff_invalid_request")
        headers = {}
        for line in lines[1:]:
            if b":" not in line or line.startswith((b" ", b"\t")):
                raise PocError("browser_handoff_invalid_request")
            name, value = line.split(b":", 1)
            name = name.lower()
            value = value.strip(b" \t")
            if (
                _HEADER_NAME.fullmatch(name) is None
                or name in headers
                or any(byte < 32 or byte > 126 for byte in value)
            ):
                raise PocError("browser_handoff_invalid_request")
            headers[name] = value
        if (
            headers.get(b"host") != f"127.0.0.1:{self._port}".encode("ascii")
            or headers.get(b"origin") != _OFFICIAL_ORIGIN.encode("ascii")
            or b"transfer-encoding" in headers
            or b"expect" in headers
            or headers.get(b"content-type", b"").lower()
            not in {
                b"application/x-www-form-urlencoded",
                b"application/x-www-form-urlencoded; charset=utf-8",
            }
        ):
            raise PocError("browser_handoff_invalid_request")
        raw_length = headers.get(b"content-length", b"")
        if re.fullmatch(rb"[1-9][0-9]{0,4}", raw_length) is None:
            raise PocError("browser_handoff_invalid_request")
        length = int(raw_length)
        if length > _BODY_LIMIT or len(data) - offset > length:
            raise PocError("browser_handoff_invalid_request")
        body = bytearray(data[offset:])
        while len(body) < length:
            body.extend(self._receive(connection, deadline))
            if len(body) > length:
                raise PocError("browser_handoff_invalid_request")
        try:
            encoded = body.decode("ascii")
            if _INVALID_PERCENT.search(encoded):
                raise ValueError
            pairs = parse_qsl(
                encoded,
                keep_blank_values=True,
                strict_parsing=True,
                encoding="utf-8",
                errors="strict",
                max_num_fields=2,
            )
            fields = dict(pairs)
            if len(pairs) != 2 or set(fields) != {"state", "message"}:
                raise ValueError
            supplied = fields["state"].encode("ascii")
        except (ValueError, UnicodeError):
            raise PocError("browser_handoff_invalid_request") from None
        if not hmac.compare_digest(supplied, self._state.encode("ascii")):
            raise PocError("browser_handoff_invalid_request")
        return self._challenge.success_headers(fields["message"])

    @staticmethod
    def _respond(connection, status, deadline):
        if time.monotonic() >= deadline:
            return
        text = (
            b"Verification received. Return to the PoC terminal.\n"
            if status == 200
            else b"Invalid verification callback.\n"
        )
        label = b"200 OK" if status == 200 else b"400 Bad Request"
        response = (
            b"HTTP/1.1 " + label + b"\r\nConnection: close\r\n"
            b"Content-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\n"
            b"Referrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\n"
            b"Content-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\n"
            b"Content-Length: " + str(len(text)).encode("ascii") + b"\r\n\r\n" + text
        )
        try:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return
            connection.settimeout(min(_POLL_SECONDS, remaining))
            connection.sendall(response)
        except OSError:
            pass

    def wait(self):
        if not self._entered or self._closed or self._finished:
            raise PocError("browser_handoff_not_active")
        self._finished = True
        listener = self._listener
        try:
            for _ in range(_MAX_CONNECTIONS):
                while True:
                    if self._closed:
                        raise PocError("browser_handoff_cancelled")
                    if time.monotonic() >= self._deadline:
                        raise PocError("human_verification_expired")
                    try:
                        connection, peer = listener.accept()
                    except TimeoutError:
                        continue
                    except OSError:
                        raise PocError("browser_handoff_cancelled") from None
                    break
                self._connection = connection
                deadline = min(self._deadline, time.monotonic() + _CONNECTION_SECONDS)
                try:
                    if peer[0] != "127.0.0.1":
                        raise PocError("browser_handoff_invalid_request")
                    headers = self._read_request(connection, deadline)
                    if time.monotonic() >= self._deadline:
                        raise PocError("human_verification_expired")
                    self._respond(connection, 200, deadline)
                    return headers
                except PocError as error:
                    self._respond(connection, 400, deadline)
                    if error.category == "human_verification_expired":
                        raise
                finally:
                    connection.close()
                    self._connection = None
            raise PocError("browser_handoff_request_limit")
        finally:
            self.close()
