"""Packet-filter contract tests using synthetic offline captures; never open a device."""

import socket
import struct
import sys
import tempfile
import unittest
from ipaddress import ip_address, ip_network
from pathlib import Path
from unittest.mock import Mock, patch

import macos_tun_wire as wire


def ethernet_frame(f: int, k: int, phase: str) -> bytes:
    family, source, destination = wire.FAMILIES[f]
    sp, dp = wire.numbers(phase, 32000, f, k)
    if k == 0:
        network_protocol = 6
        segment = struct.pack("!HHIIBBHHH", sp, dp, 1, 0, 5 << 4, 2, 1024, 0, 0)
    else:
        network_protocol = 17
        payload = wire.dns_query(phase, sp, f) if k == 2 else b"usque/offline"
        segment = struct.pack("!HHHH", sp, dp, len(payload) + 8, 0) + payload
    addresses = socket.inet_pton(family, source) + socket.inet_pton(family, destination)
    if f == 0:
        header = struct.pack("!BBHHHBBH", 0x45, 0, len(segment) + 20, 1, 0, 64, network_protocol, 0)
    else:
        header = struct.pack("!IHBB", 6 << 28, len(segment), network_protocol, 64)
    return bytes(12) + struct.pack("!H", 0x86DD if f else 0x0800) + header + addresses + segment


class WireTests(unittest.TestCase):
    def test_every_probe_uses_an_off_link_destination_via_the_controlled_gateway(self):
        output = "inet6 fe80::6465:74ff:fe68:1%feth8 prefixlen 64 scopeid 0xe"
        gateway6 = wire.peer_link_local(output + "\n", "feth8")
        self.assertEqual(gateway6, "fe80::6465:74ff:fe68:1")
        self.assertIsNone(wire.peer_link_local(output + " tentative\n", "feth8"))
        for family, source, destination in wire.FAMILIES:
            gateway = wire.IPV4_GATEWAY if family == socket.AF_INET else gateway6
            local = ip_network(f"{source}/{30 if family == socket.AF_INET else 64}", strict=False)
            if family == socket.AF_INET:
                self.assertIn(ip_address(gateway), local)
            else:
                self.assertTrue(ip_address(gateway).is_link_local)
            self.assertNotIn(ip_address(destination), local)
            route = f"  gateway: {gateway}\ninterface: feth7\n"
            wire.verify_gateway(route, "feth7", gateway)
            for incorrect in (route.replace("feth7", "en0"), route.replace(gateway, destination)):
                with self.assertRaises(RuntimeError):
                    wire.verify_gateway(incorrect, "feth7", gateway)

    def test_independent_https_is_attempted_when_capture_fails_and_both_errors_survive(self):
        directory = Path("/unused")
        config = directory / "observer.json"
        with (
            patch.object(wire, "probe", side_effect=RuntimeError("capture unavailable")),
            patch.object(wire.subprocess, "run", side_effect=OSError("HTTPS unavailable")) as https,
            self.assertRaisesRegex(
                RuntimeError, r"active.*wire: capture unavailable.*HTTPS: HTTPS unavailable"
            ),
        ):
            wire.probe_both(directory, config, "active")
        self.assertEqual(https.call_args.args[0][-2:], ["probe", "active"])

    def test_failed_wire_attempt_does_not_close_later_phases_to_measurement(self):
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            path = directory / "links.json"
            wire.save(path, {"phases": [], "links": [["feth0", 20], ["feth1", 21]]})
            with patch.object(socket, "if_nametoindex", return_value=22):
                for phase in ("before", "active"):
                    with self.assertRaisesRegex(RuntimeError, "capture peer was replaced"):
                        wire.probe(directory, directory / "observer.json", phase)

    def test_tentative_darwin_source_is_retried_before_emitting_any_probe(self):
        sock = Mock()
        sock.bind.side_effect = [OSError(wire.errno.EADDRNOTAVAIL, "tentative"), None]
        with patch.object(wire.time, "sleep") as delay:
            wire.bind_source(sock, "fd6e:7573:7175:ed::1", 31001, "feth0")
        self.assertEqual(sock.bind.call_count, 2)
        self.assertEqual(sock.bind.call_args.args, (("fd6e:7573:7175:ed::1", 31001),))
        delay.assert_called_once()

    @unittest.skipUnless(sys.platform == "darwin", "Apple tcpdump offline filter contract")
    def test_distinguishes_all_six_frame_types_and_ignores_other_phases(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "synthetic.pcap"
            with path.open("wb") as output:
                output.write(struct.pack("<IHHIIII", 0xA1B2C3D4, 2, 4, 0, 0, 65535, 1))
                for f in range(2):
                    for k in range(3):
                        frame = ethernet_frame(f, k, "before")
                        output.write(struct.pack("<4I", 1, 0, len(frame), len(frame)) + frame)
            for phase in ("before", "active"):
                filters = wire.capture_filters({"base": 32000}, phase)
                counts = {
                    key: len(
                        wire.command(wire.TCPDUMP, "-nn", "-q", "-r", str(path), expr).splitlines()
                    )
                    for key, expr in filters.items()
                }
                self.assertEqual(set(counts.values()), {1} if phase == "before" else {0})
                wire.verify_counts(phase, counts)

    def test_a_missing_positive_and_any_observed_protected_frame_fail(self):
        for phase, count in (("after", 0), ("retained", 1)):
            with self.subTest(phase=phase), self.assertRaises(RuntimeError):
                wire.verify_counts(phase, dict.fromkeys(range(6), count))


if __name__ == "__main__":
    unittest.main()
