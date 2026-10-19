"""Bounded HTTPS transport. Sensitive responses never become diagnostic messages."""

from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from urllib.parse import urlsplit

import httpx

from . import ANDROID_SOURCE, CORE_SOURCE, SRP_SOURCE

API_BASE = "https://vpn-api.proton.me"
MAX_BODY = 8 * 1024 * 1024


class PocError(Exception):
    """Only fixed categories and numeric metadata may cross the reporting boundary."""

    def __init__(self, category: str, *, status: int = 0, code: int = 0):
        super().__init__(category)
        self.category = category
        self.status = status
        self.code = code


class ApiError(PocError):
    def __init__(self, status: int, code: int, details: dict):
        category = {
            9001: "human_verification_required",
            9002: "device_verification_required",
            8002: "wrong_password",
            5003: "unsupported_client_version",
            5005: "unsupported_api_version",
            5099: "external_account_not_supported_by_client",
            8100: "sso_required",
        }.get(code, "api_rejected")
        if status == 429:
            category = "rate_limited"
        if status == 401:
            category = "unauthorized"
        super().__init__(category, status=status, code=code)
        self.details = details


@dataclass(repr=False)
class Session:
    uid: str = ""
    access_token: str = ""
    refresh_token: str = ""

    def replace(self, response: dict) -> None:
        values = [response.get(k) for k in ("UID", "AccessToken", "RefreshToken")]
        if not all(isinstance(x, str) and x and len(x) <= 16384 for x in values):
            raise PocError("invalid_session_response")
        # Validate every field before replacing a rotating refresh token.
        self.uid, self.access_token, self.refresh_token = values

    def clear(self) -> None:
        self.uid = self.access_token = self.refresh_token = ""


@dataclass
class Result:
    identity: str
    network: str
    app_version: str
    upstream_android: str = ANDROID_SOURCE
    upstream_core: str = CORE_SOURCE
    upstream_srp: str = SRP_SOURCE
    phase: str = "start"
    auth_ok: str = "not_run"
    wg_authorized: str = "not_run"
    config_ready: str = "not_run"
    tunnel_connected: str = "not_run"
    human_verification: str = "not_triggered"
    cleanup: str = "not_needed"
    error: str = "none"
    http_status: int = 0
    api_code: int = 0
    refresh: str = "not_run"
    timestamp: str = ""
    # The report deliberately has no arbitrary dictionary/string from the server.
    checks: list[str] = field(default_factory=list)


class Transport:
    def __init__(self, *, app_version: str, proxy: str | None, direct: bool, client=None):
        if direct == bool(proxy):
            raise PocError("select_exactly_one_network_path")
        if not app_version or len(app_version) > 128 or not app_version.isascii():
            raise PocError("invalid_app_version")
        if any(c.isspace() or ord(c) < 33 for c in app_version):
            raise PocError("invalid_app_version")
        if proxy:
            try:
                parsed = urlsplit(proxy)
                port = parsed.port
            except ValueError:
                raise PocError("invalid_proxy") from None
            if (
                parsed.scheme not in {"http", "https", "socks5h"}
                or not parsed.hostname
                or parsed.username is not None
                or parsed.password is not None
                or parsed.path not in {"", "/"}
                or parsed.query
                or parsed.fragment
                or port is None
            ):
                raise PocError("invalid_proxy_use_http_connect_or_socks5h_without_credentials")
        self.session = Session()
        self.direct = direct
        self.headers = {
            "Accept": "application/vnd.protonmail.v1+json",
            "Accept-Encoding": "identity",
            "Content-Type": "application/json",
            "x-pm-appversion": app_version,
            "User-Agent": "Usque-Proton-PoC/0.1 (independent control-plane experiment)",
        }
        try:
            self.client = client or httpx.Client(
                proxy=proxy,
                trust_env=False,
                follow_redirects=False,
                verify=True,
                timeout=httpx.Timeout(20.0, connect=10.0),
            )
        except (ValueError, ImportError, OSError, httpx.InvalidURL):
            raise PocError("transport_setup_failed") from None

    def request(
        self, method: str, path: str, payload=None, *, extra_headers=None, params=None
    ) -> dict:
        if not path.startswith("/") or path.startswith("//") or "?" in path or "#" in path:
            raise PocError("invalid_api_path")
        headers = self.headers.copy()
        if self.session.uid:
            headers["x-pm-uid"] = self.session.uid
            headers["Authorization"] = "Bearer " + self.session.access_token
        if extra_headers:
            headers.update(extra_headers)
        for attempt in range(2):
            try:
                started = time.monotonic()
                with self.client.stream(
                    method, API_BASE + path, headers=headers, json=payload, params=params
                ) as response:
                    if response.headers.get("Content-Encoding", "identity").lower() != "identity":
                        raise PocError("unsupported_content_encoding")
                    data = bytearray()
                    # No chunk aggregation: examine each received block even for trickle responses.
                    # Only identity encoding is accepted, avoiding unbounded decompression.
                    for chunk in response.iter_bytes():
                        if len(data) + len(chunk) > MAX_BODY:
                            raise PocError("response_too_large")
                        if time.monotonic() - started > 45:
                            raise PocError("response_deadline")
                        data.extend(chunk)
                    status = response.status_code
                    retry_after = response.headers.get("Retry-After", "")
                if status == 429 and method == "GET" and attempt == 0 and not extra_headers:
                    if retry_after.isdecimal() and 0 < int(retry_after) <= 30:
                        time.sleep(int(retry_after))
                        continue
                try:
                    body = json.loads(data)
                except (ValueError, UnicodeError, RecursionError):
                    raise PocError("invalid_json_response", status=status) from None
                if not isinstance(body, dict):
                    raise PocError("invalid_json_response", status=status)
                code = body.get("Code", 0)
                if type(code) is not int:
                    raise PocError("invalid_api_code", status=status)
                if not 200 <= status < 300 or code not in {1000, 1001}:
                    raise ApiError(status, code, body.get("Details", {}))
                return body
            except httpx.TimeoutException:
                raise PocError("network_timeout") from None
            except (httpx.HTTPError, httpx.InvalidURL, OSError, ValueError):
                raise PocError("transport_failed") from None
        raise PocError("rate_limited")

    def close(self) -> None:
        self.client.close()
