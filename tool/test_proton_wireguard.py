"""Offline keys/certificates/config and private-export fixtures; no VPN or API calls."""

from __future__ import annotations

import base64
import contextlib
import dataclasses
import importlib.util
import io
import os
import stat
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path

from proton_poc.wireguard import (
    Authorization,
    KeyMaterial,
    WireguardError,
    build_config,
    certificate_request,
    export_private_config,
    select_free_server,
    validate_certificate,
)

HAS_CRYPTO = bool(importlib.util.find_spec("cryptography") and importlib.util.find_spec("nacl"))
NOW = datetime(2026, 10, 19, tzinfo=timezone.utc)
SEED = bytes.fromhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
PEER_KEY = base64.b64encode(bytes(range(1, 33))).decode("ascii")


def directory():
    return {
        "LogicalServers": [
            {
                "ID": "synthetic-logical",
                "Tier": 0,
                "Features": 0,
                "Servers": [
                    {
                        "ID": "synthetic-physical",
                        "Status": 1,
                        "X25519PublicKey": PEER_KEY,
                        "EntryPerProtocol": {"WireGuardUDP": {"IPv4": "8.8.8.8", "Ports": [51820]}},
                    }
                ],
            }
        ]
    }


def certificate_response(
    seed=SEED, *, start=NOW - timedelta(minutes=1), end=NOW + timedelta(days=1)
):
    from cryptography import x509
    from cryptography.hazmat.primitives import serialization
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.x509.oid import NameOID

    private = Ed25519PrivateKey.from_private_bytes(seed)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "synthetic-offline-only")])
    certificate = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(private.public_key())
        .serial_number(1)
        .not_valid_before(start)
        .not_valid_after(end)
        .sign(private, algorithm=None)
    )
    return {
        "Certificate": certificate.public_bytes(serialization.Encoding.PEM).decode("ascii"),
        "ExpirationTime": int(end.timestamp()),
        "RefreshTime": int((end - timedelta(hours=1)).timestamp()),
    }


@unittest.skipUnless(HAS_CRYPTO, "install the pinned PoC dependencies for cryptographic fixtures")
class WireguardCryptoTests(unittest.TestCase):
    def setUp(self):
        self.keys = KeyMaterial.from_seed(SEED)
        self.response = certificate_response()

    def test_rfc8032_seed_and_independent_x25519_public_key(self):
        from cryptography.hazmat.primitives import serialization
        from cryptography.hazmat.primitives.asymmetric.x25519 import X25519PrivateKey

        # Ed25519 RFC8032 test 1's public key; seed and scalar must not be reused.
        self.assertEqual(
            self.keys.ed25519_public_key.hex(),
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        )
        independent = X25519PrivateKey.from_private_bytes(self.keys.x25519_private_key)
        self.assertEqual(
            independent.public_key().public_bytes(
                serialization.Encoding.Raw, serialization.PublicFormat.Raw
            ),
            self.keys.x25519_public_key,
        )
        self.assertNotEqual(self.keys.x25519_private_key, SEED)
        self.assertNotEqual(KeyMaterial.generate().ed25519_public_key, self.keys.ed25519_public_key)
        self.assertNotIn(SEED.hex(), repr(self.keys))
        with self.assertRaises(WireguardError):
            KeyMaterial.from_seed(bytes(31))

    def test_certificate_request_has_ed25519_spki_and_session_mode(self):
        request = certificate_request(self.keys)
        self.assertEqual(request["ClientPublicKey"], self.keys.public_key_pem)
        self.assertTrue(request["ClientPublicKey"].startswith("-----BEGIN PUBLIC KEY-----"))
        self.assertEqual(request["ClientPublicKeyMode"], "EC")
        self.assertEqual(request["Mode"], "session")
        self.assertEqual(request["Features"], [])
        with self.assertRaises(WireguardError):
            certificate_request(self.keys, "unsafe\nname")

    def test_certificate_key_binding_and_revalidation_before_export(self):
        authorization = validate_certificate(self.response, self.keys, NOW)
        self.assertIsInstance(authorization, Authorization)
        node = select_free_server(directory())
        config = build_config(self.keys, authorization, node, now=NOW)
        self.assertIn("Address = 10.2.0.2/32\nDNS = 10.2.0.1", config)
        self.assertIn("AllowedIPs = 0.0.0.0/0, ::/0", config)
        self.assertIn("Endpoint = 8.8.8.8:51820", config)
        self.assertIn("PersistentKeepalive = 60", config)
        self.assertNotIn("MTU", config)
        self.assertNotIn("CERTIFICATE", config)
        self.assertFalse(node.availability_verified)
        with self.assertRaisesRegex(WireguardError, "certificate_key_mismatch"):
            validate_certificate(self.response, KeyMaterial.from_seed(bytes(32)), NOW)
        with self.assertRaisesRegex(WireguardError, "certificate_key_mismatch"):
            build_config(KeyMaterial.from_seed(bytes(32)), authorization, node, now=NOW)
        with self.assertRaisesRegex(WireguardError, "certificate_expired_or_not_yet_valid"):
            build_config(self.keys, authorization, node, now=NOW + timedelta(days=2))

    def test_certificate_times_malformed_leaf_and_refresh(self):
        cases = [
            ({"Certificate": "sensitive-invalid-pem"}, "invalid_certificate"),
            ({"ExpirationTime": True}, "invalid_certificate_response"),
            (
                {"ExpirationTime": int((NOW + timedelta(days=2)).timestamp())},
                "certificate_time_mismatch",
            ),
            ({"RefreshTime": int(NOW.timestamp())}, "certificate_refresh_due"),
            ({"RefreshTime": 1}, "certificate_time_mismatch"),
            ({"Certificate": self.response["Certificate"] * 2}, "invalid_certificate"),
            (
                {"Certificate": self.response["Certificate"] + "-----END CERTIFICATE-----\n"},
                "invalid_certificate",
            ),
        ]
        for changes, category in cases:
            with self.subTest(category=category), self.assertRaisesRegex(WireguardError, category):
                validate_certificate(self.response | changes, self.keys, NOW)
        for instant in (NOW - timedelta(days=2), NOW + timedelta(days=2)):
            with self.assertRaisesRegex(WireguardError, "certificate_expired_or_not_yet_valid"):
                validate_certificate(self.response, self.keys, instant)

    def test_malformed_node_and_secret_free_exceptions(self):
        authorization = validate_certificate(self.response, self.keys, NOW)
        node = select_free_server(directory())
        output = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            for changes in (
                {"endpoint": "secret\nPrivateKey=secret"},
                {"public_key": "secret"},
                {"port": True},
            ):
                with self.assertRaises(WireguardError) as failure:
                    build_config(
                        self.keys, authorization, dataclasses.replace(node, **changes), now=NOW
                    )
                self.assertNotIn("secret", str(failure.exception))
        self.assertEqual(output.getvalue(), "")
        self.assertNotIn(self.response["Certificate"], repr(authorization))
        self.assertNotIn(PEER_KEY, repr(node))


class ServerSelectionTests(unittest.TestCase):
    def test_requires_free_advertised_udp_and_nonrestricted_candidate(self):
        for field, value in (
            ("Tier", 2),
            ("Status", 0),
            ("Status", True),
            ("Features", 1),
            ("Features", 32),
        ):
            listing = directory()
            listing["LogicalServers"][0][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(WireguardError):
                select_free_server(listing)
        for field, value in (
            ("Status", 0),
            ("Status", True),
            ("X25519PublicKey", base64.b64encode(bytes(32)).decode("ascii")),
            ("EntryPerProtocol", {"WireGuardTCP": {"IPv4": "8.8.8.8", "Ports": [443]}}),
        ):
            listing = directory()
            listing["LogicalServers"][0]["Servers"][0][field] = value
            with self.subTest(field=field), self.assertRaises(WireguardError):
                select_free_server(listing)

    def test_missing_port_requires_current_client_config_not_guessed_default(self):
        listing = directory()
        entry = listing["LogicalServers"][0]["Servers"][0]["EntryPerProtocol"]["WireGuardUDP"]
        entry.pop("Ports")
        with self.assertRaises(WireguardError):
            select_free_server(listing)
        node = select_free_server(listing, {"DefaultPorts": {"WireGuard": {"UDP": [12345]}}})
        self.assertEqual(node.port, 12345)
        with self.assertRaises(WireguardError):
            select_free_server(listing, {"DefaultPorts": {"WireGuard": {"UDP": [True]}}})

    def test_endpoint_injection_and_private_addresses_rejected(self):
        for value in ("8.8.8.8\nsecret", "127.0.0.1", "10.0.0.1", "example.com", "::1"):
            listing = directory()
            listing["LogicalServers"][0]["Servers"][0]["EntryPerProtocol"]["WireGuardUDP"][
                "IPv4"
            ] = value
            with self.subTest(value=value), self.assertRaises(WireguardError):
                select_free_server(listing)


@unittest.skipUnless(os.name == "posix", "private export is a Linux-only PoC operation")
class PrivateExportTests(unittest.TestCase):
    def test_private_file_and_overwrite_refusal(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "private.conf"
            export_private_config(path, "synthetic-secret\n")
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            self.assertEqual(path.read_text(), "synthetic-secret\n")
            with self.assertRaises(WireguardError):
                export_private_config(path, "replacement")
            self.assertEqual(path.read_text(), "synthetic-secret\n")

    def test_public_parent_and_symlink_ancestors_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            parent = Path(temporary)
            public = parent / "public"
            public.mkdir(mode=0o755)
            with self.assertRaisesRegex(WireguardError, "private_export_parent_requires_0700"):
                export_private_config(public / "private.conf", "synthetic-secret")
            private = parent / "private"
            private.mkdir(mode=0o700)
            alias = parent / "alias"
            alias.symlink_to(private, target_is_directory=True)
            with self.assertRaises(WireguardError):
                export_private_config(alias / "private.conf", "synthetic-secret")
            self.assertFalse((private / "private.conf").exists())

    def test_final_symlink_and_traversal_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            parent = Path(temporary)
            target = parent / "target"
            target.write_text("unchanged")
            alias = parent / "alias"
            alias.symlink_to(target)
            with self.assertRaises(WireguardError):
                export_private_config(alias, "synthetic-secret")
            self.assertEqual(target.read_text(), "unchanged")
            with self.assertRaises(WireguardError):
                export_private_config(parent / ".." / "private.conf", "synthetic-secret")

    def test_invalid_content_cleans_partially_created_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "private.conf"
            with self.assertRaises(WireguardError) as failure:
                export_private_config(path, "synthetic-secret-\N{SNOWMAN}")
            self.assertEqual(str(failure.exception), "private_export_failed")
            self.assertFalse(path.exists())

    def test_checkout_export_rejected(self):
        with self.assertRaisesRegex(WireguardError, "invalid_private_export_path_or_content"):
            export_private_config(Path(__file__).parent / "synthetic-secret.conf", "secret")


if __name__ == "__main__":
    unittest.main()
