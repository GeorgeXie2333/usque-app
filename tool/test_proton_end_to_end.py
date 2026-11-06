"""Real offline guest flow through the HTTPS adapter, keys, cert and exporter.

Only HTTP is replaced with httpx.MockTransport. No account is created, socket
is opened, tunnel is started, or guest/authorization method is substituted.
"""

from __future__ import annotations

import base64
import contextlib
import importlib.util
import io
import json
import os
import stat
import tempfile
import unittest
from dataclasses import asdict
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import Mock

HAS_DEPENDENCIES = all(
    importlib.util.find_spec(name) is not None
    for name in ("httpx", "cryptography", "nacl", "bcrypt")
)
HV_FIXTURE = {
    "challenge": "synthetic-private-captcha-challenge",
    "result": "synthetic-private-captcha-result",
}


class GuestApi:
    """A synthetic issuer and stateful API, driven by genuine client requests."""

    def __init__(self, test_case, verification_index=None):
        self.test_case = test_case
        self.requests = []
        self.certificates = []
        self.verification_index = verification_index
        self.bootstrap = self.session("synthetic-bootstrap")
        self.guest = self.session("synthetic-guest")
        self.refreshed = self.session("synthetic-refreshed")
        self.user_id = "synthetic-private-user-identity"
        self.peer_key = base64.b64encode(bytes(range(1, 33))).decode("ascii")
        self.expected = [
            ("POST", "/auth/v4/sessions"),
            ("POST", "/auth/v4/credentialless"),
            ("GET", "/core/v4/users"),
            ("GET", "/vpn/v2"),
            ("POST", "/auth/v4/refresh"),
            ("GET", "/vpn/v2"),
            ("POST", "/vpn/v1/certificate"),
            ("GET", "/vpn/v2/clientconfig"),
            ("GET", "/vpn/v2/logicals"),
            ("DELETE", "/auth/v4"),
            ("DELETE", "/auth/v4"),
            ("DELETE", "/auth/v4"),
        ]

    @staticmethod
    def session(prefix):
        return {
            "UID": prefix + "-private-uid",
            "AccessToken": prefix + "-private-access",
            "RefreshToken": prefix + "-private-refresh",
        }

    def assert_session(self, request, session):
        self.test_case.assertEqual(request.headers["x-pm-uid"], session["UID"])
        self.test_case.assertEqual(
            request.headers["Authorization"], "Bearer " + session["AccessToken"]
        )

    def issue_certificate(self, payload):
        from cryptography import x509
        from cryptography.hazmat.primitives import serialization
        from cryptography.hazmat.primitives.asymmetric.ed25519 import (
            Ed25519PrivateKey,
            Ed25519PublicKey,
        )
        from cryptography.x509.oid import NameOID

        self.test_case.assertEqual(
            set(payload),
            {"ClientPublicKey", "ClientPublicKeyMode", "DeviceName", "Mode", "Features"},
        )
        self.test_case.assertEqual(payload["ClientPublicKeyMode"], "EC")
        self.test_case.assertEqual(payload["DeviceName"], "Usque PoC")
        self.test_case.assertEqual(payload["Mode"], "session")
        self.test_case.assertEqual(payload["Features"], [])
        public_key = serialization.load_pem_public_key(payload["ClientPublicKey"].encode("ascii"))
        self.test_case.assertIsInstance(public_key, Ed25519PublicKey)
        # This issuer has no relationship to Proton's PKI: the fixture proves
        # actual key association and time checks, never independent issuer trust.
        issuer = Ed25519PrivateKey.from_private_bytes(bytes(range(32)))
        issuer_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "offline-test-issuer")])
        subject = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, self.user_id)])
        now = datetime.now(timezone.utc).replace(microsecond=0)
        end = now + timedelta(days=1)
        certificate = (
            x509.CertificateBuilder()
            .subject_name(subject)
            .issuer_name(issuer_name)
            .public_key(public_key)
            .serial_number(42)
            .not_valid_before(now - timedelta(minutes=1))
            .not_valid_after(end)
            .sign(issuer, algorithm=None)
        )
        pem = certificate.public_bytes(serialization.Encoding.PEM).decode("ascii")
        self.certificates.append(pem)
        return {
            "Certificate": pem,
            "ExpirationTime": int(end.timestamp()),
            "RefreshTime": int((now + timedelta(hours=12)).timestamp()),
        }

    def __call__(self, request):
        import httpx

        index = len(self.requests)
        self.requests.append(request)
        self.test_case.assertLess(index, len(self.expected))
        self.test_case.assertEqual((request.method, request.url.path), self.expected[index])
        self.test_case.assertEqual(request.url.scheme, "https")
        self.test_case.assertEqual(request.url.host, "vpn-api.proton.me")
        self.test_case.assertEqual(request.headers["x-pm-appversion"], "external-vpn@0.1.0")
        self.test_case.assertEqual(request.headers["Accept"], "application/vnd.protonmail.v1+json")
        self.test_case.assertEqual(request.headers["Accept-Encoding"], "identity")
        for header, expected in (
            ("x-pm-human-verification-token", HV_FIXTURE["result"]),
            ("x-pm-human-verification-token-type", "captcha"),
        ):
            if index == self.verification_index:
                self.test_case.assertEqual(request.headers.get(header), expected)
            else:
                self.test_case.assertNotIn(header, request.headers)
        payload = json.loads(request.content) if request.content else None
        if index == 0:
            self.test_case.assertNotIn("x-pm-uid", request.headers)
            self.test_case.assertNotIn("Authorization", request.headers)
        elif index == 1:
            self.assert_session(request, self.bootstrap)
        elif index <= 4:
            self.assert_session(request, self.guest)
        elif index <= 9:
            self.assert_session(request, self.refreshed)
        elif index == 10:
            self.assert_session(request, self.guest)
        else:
            self.assert_session(request, self.bootstrap)
        response = {}
        if index in {0, 1}:
            self.test_case.assertEqual(set(payload), {"Payload"})
            frame = payload["Payload"]["usque-linux-v4-challenge-0"]
            self.test_case.assertEqual(frame["v"], "2.0.7")
            self.test_case.assertIsInstance(frame["deviceName"], int)
            self.test_case.assertNotIn("Password", payload)
            response = self.bootstrap if index == 0 else self.guest | {"UserID": self.user_id}
        elif index == 2:
            response = {"User": {"ID": self.user_id}}
        elif index in {3, 5}:
            response = {"VPN": {"Status": 1, "MaxTier": 0, "MaxConnect": 1}, "Delinquent": 0}
        elif index == 4:
            self.test_case.assertEqual(
                payload,
                {
                    "UID": self.guest["UID"],
                    "RefreshToken": self.guest["RefreshToken"],
                    "ResponseType": "token",
                    "GrantType": "refresh_token",
                    "RedirectURI": "http://protonmail.ch",
                },
            )
            response = self.refreshed
        elif index == 6:
            response = self.issue_certificate(payload)
        elif index == 7:
            response = {"DefaultPorts": {"WireGuard": {"UDP": [41723]}}}
        elif index == 8:
            self.test_case.assertEqual(
                dict(request.url.params),
                {"WithEntriesForProtocols": "WireGuardUDP", "WithState": "true"},
            )
            response = {
                "LogicalServers": [
                    {
                        "ID": "synthetic-private-logical-id",
                        "Tier": 0,
                        "Features": 0,
                        "Servers": [
                            {
                                "ID": "synthetic-private-physical-id",
                                "Status": 1,
                                "X25519PublicKey": self.peer_key,
                                # Missing Ports must use this API's clientconfig,
                                # never an invented or unrelated fixed endpoint.
                                "EntryPerProtocol": {"WireGuardUDP": {"IPv4": "8.8.4.4"}},
                            }
                        ],
                    }
                ]
            }
        return httpx.Response(200, json={"Code": 1000, **response})


class HumanVerificationApi:
    """Inject one synthetic 9001; retain the actual headers on every request."""

    def __init__(self, test_case, api, stage):
        self.test_case = test_case
        self.api = api
        self.stage = stage
        self.requests = []
        self.challenged = None
        self.retried = False

    def __call__(self, request):
        import httpx

        self.requests.append(request)
        if self.challenged is None and len(self.api.requests) == self.stage:
            self.challenged = request
            self.test_case.assertEqual(
                (request.method, request.url.path), self.api.expected[self.stage]
            )
            for name in ("x-pm-human-verification-token", "x-pm-human-verification-token-type"):
                self.test_case.assertNotIn(name, request.headers)
            if self.stage == 0:
                self.test_case.assertNotIn("x-pm-uid", request.headers)
                self.test_case.assertNotIn("Authorization", request.headers)
            else:
                self.api.assert_session(request, self.api.bootstrap)
            return httpx.Response(
                422,
                json={
                    "Code": 9001,
                    "Details": {
                        "HumanVerificationToken": HV_FIXTURE["challenge"],
                        "HumanVerificationMethods": ["captcha"],
                    },
                },
            )
        if self.challenged is not None and not self.retried:
            if request.method == "DELETE":
                # With no interaction, or after cancellation, only the anonymous
                # session already acquired before credentialless is ours to revoke.
                self.test_case.assertEqual(self.stage, 1)
                self.test_case.assertEqual(request.url.path, "/auth/v4")
                self.api.assert_session(request, self.api.bootstrap)
                for name in (
                    "x-pm-human-verification-token",
                    "x-pm-human-verification-token-type",
                ):
                    self.test_case.assertNotIn(name, request.headers)
                return httpx.Response(200, json={"Code": 1000})
            self.test_case.assertEqual(request.method, self.challenged.method)
            self.test_case.assertEqual(request.url, self.challenged.url)
            self.test_case.assertEqual(request.content, self.challenged.content)
            for name in ("x-pm-uid", "Authorization"):
                self.test_case.assertEqual(
                    request.headers.get(name), self.challenged.headers.get(name)
                )
            self.retried = True
        # GuestApi checks both verification headers on this one retry and
        # requires their absence on every later API and DELETE request.
        return self.api(request)


@unittest.skipUnless(
    HAS_DEPENDENCIES and os.name == "posix",
    "install the pinned Linux PoC dependencies for the full offline integration fixture",
)
class ProtonEndToEndTests(unittest.TestCase):
    def execute_guest(self, api, handler=None, *, interaction=None, export_path=None):
        import httpx

        from proton_poc.flow import run
        from proton_poc.transport import Transport

        client = httpx.Client(transport=httpx.MockTransport(handler or api), trust_env=False)
        transport = Transport(
            app_version="external-vpn@0.1.0", direct=True, proxy=None, client=client
        )
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            result = run(
                transport, identity="guest", interaction=interaction, export_path=export_path
            )
        self.assertEqual(stdout.getvalue(), "")
        self.assertEqual(stderr.getvalue(), "")
        self.assertTrue(client.is_closed)
        self.assertEqual(transport.session.uid, "")
        self.assertEqual(transport.session.access_token, "")
        self.assertEqual(transport.session.refresh_token, "")
        report = json.dumps(asdict(result))
        secrets = [
            api.user_id,
            api.peer_key,
            *api.bootstrap.values(),
            *api.guest.values(),
            *api.refreshed.values(),
            *api.certificates,
            # JSON escapes PEM newlines; checking the full PEM alone would not
            # detect a certificate leaked inside a serialized report string.
            *(line for certificate in api.certificates for line in certificate.splitlines()),
            "synthetic-private-logical-id",
            "synthetic-private-physical-id",
            *HV_FIXTURE.values(),
        ]
        for secret in secrets:
            self.assertNotIn(secret, report)
            self.assertNotIn(secret, stdout.getvalue())
            self.assertNotIn(secret, stderr.getvalue())
        return result, report

    def verification_interaction(self, *, cancelled=False):
        from proton_poc.flow import LocalInteraction

        interaction = Mock(spec=LocalInteraction)

        def verify(challenge):
            self.assertEqual(challenge.token, HV_FIXTURE["challenge"])
            self.assertEqual(challenge.methods, ("captcha",))
            if cancelled:
                raise KeyboardInterrupt
            # This is a local synthetic success event, not a solution of any
            # real captcha. Exercise the production event validator and encoder.
            return challenge.success_headers(
                json.dumps(
                    {
                        "type": "HUMAN_VERIFICATION_SUCCESS",
                        "payload": {"token": HV_FIXTURE["result"], "type": "captcha"},
                    }
                )
            )

        interaction.verify.side_effect = verify
        return interaction

    def run_guest(self, export_path=None, *, verification_stage=None):
        api = GuestApi(self, verification_index=verification_stage)
        handler = None
        interaction = None
        if verification_stage is not None:
            handler = HumanVerificationApi(self, api, verification_stage)
            interaction = self.verification_interaction()
        result, report = self.execute_guest(
            api, handler, interaction=interaction, export_path=export_path
        )
        self.assertEqual(
            [(request.method, request.url.path) for request in api.requests], api.expected
        )
        self.assertEqual(result.identity, "guest")
        self.assertEqual(result.network, "direct_https")
        self.assertEqual(result.auth_ok, "passed")
        self.assertEqual(result.refresh, "passed")
        self.assertEqual(result.wg_authorized, "passed")
        self.assertEqual(result.config_ready, "passed")
        self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(result.tunnel_connected, "not_run")
        self.assertEqual(
            result.human_verification, "passed" if handler is not None else "not_triggered"
        )
        self.assertEqual(result.http_status, 0)
        self.assertEqual(result.api_code, 0)
        self.assertTrue(result.timestamp)
        self.assertEqual(
            set(result.checks)
            - {
                "export_for_structure_only_session_will_be_revoked",
                "browser_network_user_managed",
            },
            {
                "certificate_issuer_chain_not_verified",
                "certificate_session_binding_via_authenticated_request",
                "local_agent_not_implemented",
                "node_availability_not_verified",
            },
        )
        # All changed UIDs belong to this experiment. Revoke the active handle
        # then each retired handle, using its own token rather than the last one.
        revoked = [r.headers["x-pm-uid"] for r in api.requests if r.method == "DELETE"]
        self.assertEqual(revoked, [api.refreshed["UID"], api.guest["UID"], api.bootstrap["UID"]])
        if handler is not None:
            interaction.verify.assert_called_once()
            self.assertTrue(handler.retried)
            self.assertEqual(len(handler.requests), len(api.expected) + 1)
            with_headers = [
                request
                for request in handler.requests
                if "x-pm-human-verification-token" in request.headers
            ]
            self.assertEqual(with_headers, [handler.requests[verification_stage + 1]])
            self.assertIn("browser_network_user_managed", result.checks)
        return result, report

    def test_guest_to_config_and_revoke_without_export(self):
        result, _ = self.run_guest()
        self.assertEqual(result.error, "none")
        self.assertEqual(result.phase, "complete")
        self.assertNotIn("export_for_structure_only_session_will_be_revoked", result.checks)

    def test_explicit_private_export_has_valid_generated_key_and_0600(self):
        from cryptography.hazmat.primitives.asymmetric.x25519 import X25519PrivateKey

        with tempfile.TemporaryDirectory() as temporary:
            # macOS exposes its temporary directory through a /var alias. The
            # exporter intentionally refuses every symlink ancestor.
            path = Path(temporary).resolve() / "synthetic-proton.conf"
            result, report = self.run_guest(path)
            self.assertEqual(result.error, "none")
            self.assertEqual(result.phase, "complete")
            self.assertIn("export_for_structure_only_session_will_be_revoked", result.checks)
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            config = path.read_text(encoding="ascii")
            private_key = next(
                line.removeprefix("PrivateKey = ")
                for line in config.splitlines()
                if line.startswith("PrivateKey = ")
            )
            raw = base64.b64decode(private_key, validate=True)
            self.assertEqual(len(raw), 32)
            X25519PrivateKey.from_private_bytes(raw)
            self.assertNotIn(private_key, report)
            self.assertNotIn(config, report)
            self.assertIn("Address = 10.2.0.2/32\nDNS = 10.2.0.1", config)
            self.assertIn("Endpoint = 8.8.4.4:41723", config)
            self.assertIn("AllowedIPs = 0.0.0.0/0, ::/0", config)
            self.assertNotIn("MTU", config)

    def test_existing_export_is_preserved_and_all_sessions_still_revoke(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "existing.conf"
            path.write_text("synthetic-existing-content", encoding="ascii")
            result, report = self.run_guest(path)
            self.assertEqual(result.phase, "private_export")
            self.assertEqual(result.error, "private_export_failed")
            self.assertEqual(path.read_text(encoding="ascii"), "synthetic-existing-content")
            self.assertNotIn("export_for_structure_only_session_will_be_revoked", result.checks)
            self.assertNotIn(str(path), report)

    def test_guest_human_verification_retries_only_the_bound_request(self):
        for stage in (0, 1):
            with self.subTest(stage=stage):
                result, _ = self.run_guest(verification_stage=stage)
                self.assertEqual(result.error, "none")
                self.assertEqual(result.phase, "complete")

    def assert_stopped_challenge(self, api, handler, result, stage):
        self.assertEqual(result.phase, "anonymous_session" if stage == 0 else "credentialless")
        self.assertEqual(result.auth_ok, "failed")
        self.assertEqual(result.refresh, "not_run")
        self.assertEqual(result.wg_authorized, "not_run")
        self.assertEqual(result.config_ready, "not_run")
        self.assertEqual(result.tunnel_connected, "not_run")
        self.assertEqual(result.human_verification, "required_not_completed")
        self.assertFalse(handler.retried)
        expected = api.expected[: stage + 1]
        if stage == 1:
            expected = [*expected, ("DELETE", "/auth/v4")]
            self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(
            [(request.method, request.url.path) for request in handler.requests], expected
        )
        for request in handler.requests:
            for name in ("x-pm-human-verification-token", "x-pm-human-verification-token-type"):
                self.assertNotIn(name, request.headers)

    def test_guest_human_verification_without_interaction_stops_and_cleans_up(self):
        for stage in (0, 1):
            with self.subTest(stage=stage):
                api = GuestApi(self)
                handler = HumanVerificationApi(self, api, stage)
                result, _ = self.execute_guest(api, handler, interaction=None)
                self.assert_stopped_challenge(api, handler, result, stage)
                self.assertEqual(result.error, "human_verification_required")
                self.assertEqual(result.http_status, 422)
                self.assertEqual(result.api_code, 9001)
                if stage == 0:
                    self.assertEqual(result.cleanup, "not_needed")

    def test_guest_human_verification_cancelled_locally_never_retries(self):
        for stage in (0, 1):
            with self.subTest(stage=stage):
                api = GuestApi(self)
                handler = HumanVerificationApi(self, api, stage)
                interaction = self.verification_interaction(cancelled=True)
                result, _ = self.execute_guest(api, handler, interaction=interaction)
                interaction.verify.assert_called_once()
                self.assert_stopped_challenge(api, handler, result, stage)
                self.assertEqual(result.error, "cancelled")
                if stage == 0:
                    # A definite 9001 arrived before local cancellation; no
                    # session-creating retry was sent and no handle was issued.
                    self.assertEqual(result.cleanup, "not_needed")


if __name__ == "__main__":
    unittest.main()
