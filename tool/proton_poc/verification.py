"""Official HV3 handoff encoding; challenge and success tokens remain private."""

import json
import time
from dataclasses import dataclass, field
from urllib.parse import urlencode

from .transport import PocError


@dataclass(repr=False)
class VerificationChallenge:
    token: str
    methods: tuple[str, ...]
    created_at: float = field(default_factory=time.monotonic)

    @classmethod
    def from_details(cls, details):
        if not isinstance(details, dict):
            raise PocError("invalid_verification_challenge")
        token = details.get("HumanVerificationToken")
        methods = details.get("HumanVerificationMethods")
        if (
            not isinstance(token, str)
            or not 1 <= len(token) <= 8192
            or not isinstance(methods, list)
            or not 1 <= len(methods) <= 16
            or not all(
                isinstance(x, str) and x.isascii() and x.isalnum() and len(x) <= 32 for x in methods
            )
        ):
            raise PocError("invalid_verification_challenge")
        if not {"captcha", "sms", "email"}.intersection(methods):
            raise PocError("unsupported_human_verification_method")
        return cls(token, tuple(methods))

    def url(self):
        # Fixed official host: never follow a server-provided arbitrary URL.
        return "https://verify.proton.me?" + urlencode(
            {
                "embed": "true",
                "token": self.token,
                "methods": ",".join(self.methods),
                "theme": "2",
                "vpn": "true",
            }
        )

    def success_headers(self, message):
        if time.monotonic() - self.created_at > 300:
            raise PocError("human_verification_expired")
        if not isinstance(message, str) or len(message) > 32768:
            raise PocError("invalid_verification_result")
        try:
            data = json.loads(message)
            if data["type"] != "HUMAN_VERIFICATION_SUCCESS":
                raise ValueError
            token = data["payload"]["token"]
            method = data["payload"]["type"]
        except (ValueError, KeyError, TypeError, RecursionError):
            raise PocError("invalid_verification_result") from None
        if method not in self.methods or method not in {"captcha", "sms", "email"}:
            raise PocError("unsupported_human_verification_result")
        if (
            not isinstance(token, str)
            or not 1 <= len(token) <= 8192
            or not token.isascii()
            or any(ord(c) < 33 or ord(c) > 126 for c in token)
        ):
            raise PocError("invalid_verification_result")
        return {
            "x-pm-human-verification-token": token,
            "x-pm-human-verification-token-type": method,
        }
