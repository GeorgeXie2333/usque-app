"""Offline tests for the Android per-app emulator harness parsers."""

import unittest

import android_per_app_e2e as harness

DUMPSYS = (
    "Current Networks:\n"
    "  NetworkAgentInfo{network{100}  handle{1}  ni{MOBILE CONNECTED extra: epc}"
    "  nc{[ Transports: CELLULAR Capabilities: INTERNET]}}\n"
    "  NetworkAgentInfo{network{104}  handle{2}  ni{VPN CONNECTED extra: }"
    "  nc{[ Transports: VPN Capabilities: INTERNET&NOT_RESTRICTED"
    " Uids: <{0-10149, 10151-99999}> OwnerUid: 10160 AdminUids: [10160]]}}\n"
)
# Android 12+ merges the underlying transports into the VPN network.
UNDERLYING = DUMPSYS.replace("Transports: VPN", "Transports: WIFI|VPN")


class ParsingTests(unittest.TestCase):
    def test_vpn_uid_ranges_belong_to_the_owner(self):
        ranges = harness.parse_uid_ranges(DUMPSYS, 10160)
        self.assertEqual(ranges, [(0, 10149), (10151, 99999)])
        self.assertTrue(harness.uid_in(ranges, 10149))
        self.assertFalse(harness.uid_in(ranges, 10150))
        self.assertIsNone(harness.parse_uid_ranges(DUMPSYS, 10161))

    def test_vpn_with_underlying_transports(self):
        self.assertEqual(harness.parse_uid_ranges(UNDERLYING, 10160), [(0, 10149), (10151, 99999)])

    def test_single_uid_range_and_disconnected_vpn(self):
        allow = UNDERLYING.replace("0-10149, 10151-99999", "10150-10150")
        self.assertEqual(harness.parse_uid_ranges(allow, 10160), [(10150, 10150)])
        single = UNDERLYING.replace(" Uids: <{0-10149, 10151-99999}>", " Uid: 10150")
        self.assertEqual(harness.parse_uid_ranges(single, 10160), [(10150, 10150)])
        disconnected = UNDERLYING.replace("VPN CONNECTED", "VPN DISCONNECTED")
        self.assertIsNone(harness.parse_uid_ranges(disconnected, 10160))

    def test_network_summary_has_no_addresses(self):
        text = UNDERLYING.replace("nc{", "lp{LinkAddresses: [ 10.0.0.2/32 ]}  nc{")
        summary = harness.network_summary(text, 10160)
        self.assertEqual(
            summary,
            [
                {
                    "type": "MOBILE",
                    "state": "CONNECTED",
                    "transports": ["CELLULAR"],
                    "owned_by_usque": False,
                    "uid_field": "absent",
                },
                {
                    "type": "VPN",
                    "state": "CONNECTED",
                    "transports": ["VPN", "WIFI"],
                    "owned_by_usque": True,
                    "uid_field": "ranges",
                },
            ],
        )
        self.assertNotIn("10.0.0.2", repr(summary))

    def test_route_device_class(self):
        self.assertEqual(
            harness.parse_route_device("1.1.1.1 dev tun0 table 1012 src 10.0.0.2 uid 10150"),
            "tun",
        )
        self.assertEqual(
            harness.parse_route_device("1.1.1.1 via 10.0.2.2 dev wlan0 table 1003 uid 10150"),
            "other",
        )
        self.assertEqual(harness.parse_route_device("RTNETLINK answers: error"), "unknown")

    def test_trace_warp_field(self):
        body = "HTTP/1.1 200 OK\r\n\r\nh=www.cloudflare.com\nip=192.0.2.1\nwarp=on\n"
        self.assertEqual(harness.parse_warp(body), "on")
        self.assertEqual(harness.parse_warp(body.replace("warp=on", "warp=off")), "off")
        self.assertIsNone(harness.parse_warp("HTTP/1.1 301 Moved\r\n"))

    def test_package_listing(self):
        listing = "package:com.example.a uid:10150\npackage:com.example.b uid:10151,1000\n"
        self.assertEqual(
            harness.parse_packages(listing),
            [("com.example.a", 10150), ("com.example.b", 10151)],
        )

    def test_redaction_removes_addresses(self):
        text = harness.redact("exit 192.0.2.10 and 2001:db8:1::5 via tun0, fe80::1%wlan0")
        self.assertNotIn("192.0.2.10", text)
        self.assertNotIn("2001:db8", text)
        self.assertNotIn("fe80", text)
        self.assertIn("tun0", text)
        self.assertIn("%wlan0", text)

    def test_redaction_keeps_durations_and_xml_well_formed(self):
        xml_text = (
            '<hierarchy><node content-desc="Duration&#10;00:00:12" bounds="[0,0][1,1]" />'
            '<node content-desc="Exit 192.0.2.10 2001:db8::5" bounds="[0,0][1,1]" />'
            "</hierarchy>"
        )
        nodes = harness.parse_nodes(harness.redact(xml_text))
        self.assertTrue(nodes[0].has("00:00:12"))
        self.assertTrue(nodes[1].has("Exit [ipv4] [ipv6]"))


class UiTests(unittest.TestCase):
    XML = (
        '<?xml version="1.0" encoding="UTF-8"?><hierarchy rotation="0">'
        '<node class="android.view.View" content-desc="Per-app proxy&#10;All apps use the VPN"'
        ' clickable="true" enabled="true" checked="false" selected="false" focused="false"'
        ' bounds="[0,100][1080,220]" />'
        '<node class="android.widget.Button" content-desc="Bypass selected apps"'
        ' clickable="true" enabled="true" selected="true" bounds="[720,300][1060,380]" />'
        '<node class="android.widget.EditText" text="com.example.a" hint="Search apps"'
        ' clickable="true" bounds="[0,400][1080,480]" />'
        '<node class="android.view.View" text="" bounds="invalid" />'
        "</hierarchy>"
    )

    def test_nodes_keep_label_lines_states_and_centres(self):
        nodes = harness.parse_nodes(self.XML)
        self.assertEqual(len(nodes), 3)
        row, segment, field = nodes
        self.assertTrue(row.has("Per-app proxy"))
        self.assertTrue(row.has("All apps use the VPN"))
        self.assertEqual(row.center, (540, 160))
        self.assertTrue(segment.selected)
        self.assertTrue(field.mentions("com.example.a"))
        self.assertTrue(field.has("Search apps"))
        self.assertIs(harness.Device.find(nodes, "Bypass selected apps"), segment)
        self.assertIsNone(harness.Device.find(nodes, "Missing"))

    def test_swipes_stay_inside_the_largest_scrollable_area(self):
        xml_text = (
            "<hierarchy>"
            '<node class="android.widget.ScrollView" scrollable="true" bounds="[0,128][1080,1517]" />'
            '<node class="android.widget.ScrollView" scrollable="true" bounds="[0,0][100,100]" />'
            '<node class="android.view.View" bounds="[0,0][1080,2400]" />'
            "</hierarchy>"
        )
        self.assertEqual(harness.scroll_area(harness.parse_nodes(xml_text)), (0, 128, 1080, 1517))
        self.assertIsNone(harness.scroll_area(harness.parse_nodes(self.XML)))


if __name__ == "__main__":
    unittest.main()
