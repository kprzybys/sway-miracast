"""Unit tests for wfd_sink's pure logic (no network, no wpa_supplicant). Run: python3 -m unittest receiver/test_wfd_sink.py"""

import os
import socket
import sys
import unittest

sys.path.insert(0, os.path.dirname(__file__))
import wfd_sink as w  # noqa: E402


class Pure(unittest.TestCase):
    def test_device_info_subelement(self):
        # length 6, primary sink + session available, port 7236, 50 Mb/s
        self.assertEqual(w.device_info_subelement(True), "000600111c440032")
        self.assertEqual(w.device_info_subelement(False), "000600011c440032")

    def test_m3_reply_answers_every_name(self):
        body = w.m3_reply(["wfd_video_formats", "wfd_client_rtp_ports", "wfd_foo"], 19000)
        p = w.parse_params(body)
        self.assertEqual(p["wfd_client_rtp_ports"], "RTP/AVP/UDP;unicast 19000 0 mode=play")
        self.assertEqual(p["wfd_foo"], "none")
        self.assertTrue(body.endswith("\r\n"))

    def test_video_formats_offer_1080p30_not_p60(self):
        f = w.VIDEO_FORMATS.split()
        cea = int(f[4], 16)
        self.assertTrue(cea & (1 << 7), "1920x1080p30")
        self.assertFalse(cea & (1 << 8), "1920x1080p60")
        self.assertEqual(int(f[0], 16) >> 3, 7, "native = CEA index 7")

    def test_lease_ip(self):
        self.assertEqual(w.lease_ip("1700000000 aa:bb:cc:dd:ee:ff 192.168.173.12 * 01:aa\n"), "192.168.173.12")
        self.assertIsNone(w.lease_ip(""))

    def test_dhcp_server_from_nmcli(self):
        out = "broadcast_address = 192.168.49.255 | dhcp_server_identifier = 192.168.49.1 | ip_address = 192.168.49.23"
        self.assertEqual(w.dhcp_server(out), "192.168.49.1")
        self.assertEqual(w.dhcp_server("dhcp_server_identifier = 10.42.0.1\nip_address = 10.42.0.5\n"), "10.42.0.1")
        self.assertIsNone(w.dhcp_server(""))

    def test_rtsp_reads_split_messages(self):
        a, b = socket.socketpair()
        r = w.Rtsp(a)
        b.sendall(b"SET_PARAMETER rtsp://x/wfd1.0 RTSP/1.0\r\nCSeq: 4\r\nContent-Length: 27\r\n\r\nwfd_trigger_")
        r.feed()
        self.assertIsNone(r.read())
        b.sendall(b"method: SETUP\r\nRTSP/1.0 200 OK\r\nCSeq: 2\r\n\r\n")
        r.feed()
        m = r.read()
        self.assertEqual((m.first.split()[0], m.cseq), ("SET_PARAMETER", 4))
        self.assertEqual(w.parse_params(m.body)["wfd_trigger_method"], "SETUP")
        m2 = r.read()
        self.assertTrue(m2.is_response)
        self.assertEqual(m2.cseq, 2)
        a.close()
        b.close()

    def test_pipeline_drops_audio_when_asked(self):
        cmd = " ".join(w.pipeline_cmd(19000, 100, audio=False))
        self.assertIn("v4l2h264dec", cmd)
        self.assertNotIn("alsasink", cmd)


if __name__ == "__main__":
    unittest.main()
