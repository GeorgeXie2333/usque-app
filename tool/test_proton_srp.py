"""MIT upstream vectors and fail-closed coverage for Proton SRP v4."""

from __future__ import annotations

import base64
import importlib.util
import json
import unittest
from pathlib import Path
from unittest import mock

from proton_poc import srp

_HAVE_CRYPTO = all(
    importlib.util.find_spec(name) is not None for name in ("bcrypt", "cryptography")
)
_FIXTURE = Path(__file__).parent / "proton_poc" / "testdata" / "srp_v4.json"


@unittest.skipUnless(_HAVE_CRYPTO, "Proton PoC optional crypto dependencies are not installed")
class ProtonSrpTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.vector = json.loads(_FIXTURE.read_text(encoding="utf-8"))
        cls.modulus = base64.b64decode(cls.vector["modulus"])
        cls.number = srp._validated_modulus(cls.modulus)

    def info(self):
        return {
            "Version": 4,
            "Salt": self.vector["salt"],
            "Modulus": self.vector["signed_modulus"],
            "ServerEphemeral": self.vector["server_ephemeral"],
        }

    def official_client(self):
        secret = int(self.vector["client_secret_hex"], 16)
        with mock.patch.object(srp.secrets, "randbelow", return_value=secret - 4097):
            return srp.SrpClient.from_info(self.vector["password"], self.info())

    def test_mit_official_signed_modulus_and_crlf(self):
        for value in (
            self.vector["signed_modulus"],
            self.vector["signed_modulus"].replace("\n", "\r\n"),
        ):
            self.assertEqual(srp.verify_signed_modulus(value), self.modulus)

    def test_mit_official_bcrypt_vector(self):
        import bcrypt

        result = bcrypt.hashpw(
            self.vector["bcrypt_password"].encode(),
            ("$2y$10$" + self.vector["bcrypt_salt"]).encode(),
        )
        self.assertEqual(result.decode(), self.vector["bcrypt_hash"])

    def test_mit_official_full_srp_proof_vector(self):
        client = self.official_client()
        self.assertEqual(client.proofs().client_proof, self.vector["client_proof"])
        client.verify_server_proof(self.vector["server_proof"])
        self.assertEqual(len(base64.b64decode(client.proofs().client_ephemeral)), 256)

    def test_server_proof_uses_constant_time_comparison(self):
        client = self.official_client()
        with mock.patch.object(
            srp.hmac, "compare_digest", wraps=srp.hmac.compare_digest
        ) as compare:
            client.verify_server_proof(self.vector["server_proof"])
        compare.assert_called_once()
        self.assertEqual(len(compare.call_args.args[0]), 256)
        self.assertEqual(len(compare.call_args.args[1]), 256)

    def test_wrong_server_proof_and_invalid_encodings_fail(self):
        client = self.official_client()
        for value in ("AA==", "!" * 344, None, base64.b64encode(bytes(256)).decode()):
            with self.subTest(value_type=type(value).__name__), self.assertRaises(srp.SrpError):
                client.verify_server_proof(value)

    def test_only_version_four_supported(self):
        for version in (0, 1, 2, 3, 5, "4", True, None):
            info = self.info()
            info["Version"] = version
            with self.subTest(version=version), self.assertRaises(srp.SrpError):
                srp.SrpClient.from_info("sensitive-password", info)

    def test_signed_modulus_rejects_tampering_and_extra_data(self):
        valid = self.vector["signed_modulus"]
        for value in (
            valid + "trailing data",
            "leading data" + valid,
            valid.replace("W2z5", "X2z5", 1),
            valid.replace("SHA256", "SHA1"),
            valid.replace("wl4EA", "xl4EA"),
            valid.replace("=q6vu", "=q6vv"),
            valid + valid,
            "x" * 8193,
            None,
        ):
            with self.subTest(value_type=type(value).__name__), self.assertRaises(srp.SrpError):
                srp.verify_signed_modulus(value)

    def test_signature_packet_rejects_bad_lengths_algorithms_and_mpis(self):
        valid = self.vector["signed_modulus"]
        prefix, signature = valid.split("-----BEGIN PGP SIGNATURE-----\n", 1)
        headers, armored = signature.split("\n\n", 1)
        packet = base64.b64decode("".join(armored.split("\n=")[0].splitlines()))
        # Recompute the armor CRC so these reach packet/crypto validation.
        altered = []
        for offset, replacement in (
            (0, 0xC3),
            (1, 0x5D),
            (2, 5),
            (4, 1),
            (5, 2),
            (28, 0),
            (40, packet[40] ^ 1),
        ):
            changed = bytearray(packet)
            changed[offset] = replacement
            altered.append(bytes(changed))
        altered += [packet + b"\0", packet[:-1]]
        for changed in altered:
            value = (
                prefix
                + "-----BEGIN PGP SIGNATURE-----\n"
                + headers
                + "\n\n"
                + base64.b64encode(changed).decode()
                + "\n="
                + base64.b64encode(srp._crc24(changed)).decode()
                + "\n-----END PGP SIGNATURE-----"
            )
            with self.subTest(packet_size=len(changed)), self.assertRaises(srp.SrpError):
                srp.verify_signed_modulus(value)

    def test_public_group_parameters_reject_malformed_or_composite(self):
        values = (0, 23, (1 << 2047) + 1, (1 << 2047) + 11)
        for number in values:
            with self.subTest(number_bits=number.bit_length()), self.assertRaises(srp.SrpError):
                srp._validated_modulus(number.to_bytes(256, "little"))
        with self.assertRaises(srp.SrpError):
            srp._validated_modulus(self.modulus + b"\0")

    def test_out_of_bounds_server_ephemeral_rejected_before_password_work(self):
        for number in (0, 1, self.number - 1, self.number, (1 << 2048) - 1):
            info = self.info()
            info["ServerEphemeral"] = base64.b64encode(number.to_bytes(256, "little")).decode()
            with mock.patch.object(srp, "hash_password_v4") as derive:
                with self.assertRaises(srp.SrpError):
                    srp.SrpClient.from_info("sensitive-password", info)
                derive.assert_not_called()

    def test_missing_or_malformed_auth_fields(self):
        for field in ("Salt", "Modulus", "ServerEphemeral"):
            info = self.info()
            del info[field]
            with self.subTest(field=field), self.assertRaises(srp.SrpError):
                srp.SrpClient.from_info("sensitive-password", info)
        info = self.info()
        info["Salt"] = "AA=="
        with self.assertRaises(srp.SrpError):
            srp.SrpClient.from_info("sensitive-password", info)

    def test_degenerate_shared_base_rejected(self):
        hashed = srp.hash_password_v4(b"example", bytes(10), self.modulus)
        multiplier = (
            int.from_bytes(srp.expand_hash((2).to_bytes(256, "little") + self.modulus), "little")
            % self.number
        )
        verifier = pow(2, int.from_bytes(hashed, "little"), self.number)
        for base in (0, 1, self.number - 1):
            remote = (multiplier * verifier + base) % self.number
            with self.subTest(base_case=base in (0, 1)), self.assertRaises(srp.SrpError):
                srp._generate_proofs(self.modulus, remote.to_bytes(256, "little"), hashed)

    def test_unsupported_passwords_fail_without_echo(self):
        for password in ("", "private-secret-" * 8, "\ud800"):
            with self.subTest(length=len(password)), self.assertRaises(srp.SrpError) as raised:
                srp.hash_password_v4(password, bytes(10), self.modulus)
            self.assertNotIn(password or "private-secret-", str(raised.exception))

    def test_repr_does_not_contain_proofs_or_password(self):
        client = self.official_client()
        displayed = repr(client) + repr(client.proofs())
        for secret in (
            self.vector["password"],
            self.vector["client_proof"],
            self.vector["server_proof"],
            client.proofs().client_ephemeral,
        ):
            self.assertNotIn(secret, displayed)


if __name__ == "__main__":
    unittest.main()
