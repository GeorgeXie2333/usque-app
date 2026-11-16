"""Packet-filter contract tests using synthetic offline captures; never open a device."""

import socket
import struct
import sys
import tempfile
import unittest
from pathlib import Path

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
