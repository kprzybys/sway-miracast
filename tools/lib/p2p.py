"""NetworkManager D-Bus helpers for Wi-Fi P2P (Wi-Fi Direct) discovery and WFD IE decoding.

Shared by tools/miracast-list.py, tools/tvwindow.py and tools/p2p_discover.py. Needs dbus-python and no root
(NetworkManager polkit). Scripts started with `python3 -I` must add this directory to sys.path themselves.
"""

import dbus

NM = "org.freedesktop.NetworkManager"
PROPS = dbus.PROPERTIES_IFACE
NM_DEVICE_TYPE_WIFI_P2P = 30

DEVINFO_TYPES = {0: "source", 1: "primary sink", 2: "secondary sink", 3: "dual-role"}
SUBELEM = {
    0: "device_info",
    1: "associated_bssid",
    2: "audio_formats",
    3: "video_formats",
    4: "3d_video_formats",
    5: "content_protection",
    6: "coupled_sink",
    7: "ext_capability",
    8: "local_ip",
    9: "session_info",
    10: "alt_mac",
    11: "r2_device_info",
}


def find_p2p_device(bus):
    """D-Bus object of the first NetworkManager Wi-Fi P2P device, or None."""
    nm = dbus.Interface(bus.get_object(NM, "/org/freedesktop/NetworkManager"), NM)
    for path in nm.GetDevices():
        obj = bus.get_object(NM, path)
        try:
            kind = int(obj.Get(NM + ".Device", "DeviceType", dbus_interface=PROPS))
        except dbus.DBusException:
            continue
        if kind == NM_DEVICE_TYPE_WIFI_P2P:
            return obj
    return None


def start_find(p2p_obj, timeout_s):
    """Start a P2P find; True if started, False if one was already running (its results land in Peers too)."""
    dev = dbus.Interface(p2p_obj, NM + ".Device.WifiP2P")
    try:
        dev.StartFind({"timeout": dbus.Int32(timeout_s)})
        return True
    except dbus.DBusException:
        return False


def stop_find(p2p_obj):
    """Stop the find; tolerant of "not running"."""
    dev = dbus.Interface(p2p_obj, NM + ".Device.WifiP2P")
    try:
        dev.StopFind()
    except dbus.DBusException:
        pass


def iter_peers(bus, p2p_obj):
    """Yield (path, properties dict) for each known peer; peers that vanished meanwhile are skipped."""
    for path in p2p_obj.Get(NM + ".Device.WifiP2P", "Peers", dbus_interface=PROPS):
        try:
            props = bus.get_object(NM, path).GetAll(NM + ".WifiP2PPeer", dbus_interface=PROPS)
        except dbus.DBusException:
            continue
        yield path, props


def device_info(ies):
    """WFD device-info bitmap (subelement 0) as an int, or None when the IEs carry none."""
    ies = bytes(ies)
    i = 0
    while i + 3 <= len(ies):
        sid, ln = ies[i], int.from_bytes(ies[i + 1 : i + 3], "big")
        body = ies[i + 3 : i + 3 + ln]
        if sid == 0 and len(body) >= 2:
            return int.from_bytes(body[:2], "big")
        i += 3 + ln
    return None


def sink_info(ies):
    """(device type 0..3, session available) from the WFD IEs, or None."""
    bm = device_info(ies)
    if bm is None:
        return None
    return bm & 3, (bm >> 4) & 3 == 1


def decode_devinfo(b):
    bm = int.from_bytes(b[0:2], "big")
    return {
        "raw": b.hex(),
        "device_type": DEVINFO_TYPES[bm & 3],
        "coupled_sink_by_source": bool(bm & 1 << 2),
        "coupled_sink_by_sink": bool(bm & 1 << 3),
        "session_available": (bm >> 4) & 3 == 1,
        "wfd_service_discovery": bool(bm & 1 << 6),
        "preferred_connectivity": "TDLS" if bm & 1 << 7 else "P2P",
        "content_protection_hdcp2": bool(bm & 1 << 8),
        "time_sync": bool(bm & 1 << 9),
        "audio_unsupported_at_primary_sink": bool(bm & 1 << 10),
        "audio_only_at_source": bool(bm & 1 << 11),
        "tdls_persistent_group": bool(bm & 1 << 12),
        "tdls_persistent_reinvoke": bool(bm & 1 << 13),
        "rtsp_port": int.from_bytes(b[2:4], "big"),
        "max_throughput_mbps": int.from_bytes(b[4:6], "big"),
    }


def decode_wfd_ies(raw):
    """All WFD subelements as dicts (id, name, len, hex, and decoded where known)."""
    out, i = [], 0
    while i + 3 <= len(raw):
        sid, ln = raw[i], int.from_bytes(raw[i + 1 : i + 3], "big")
        body = raw[i + 3 : i + 3 + ln]
        e = {"id": sid, "name": SUBELEM.get(sid, f"unknown_{sid}"), "len": ln, "hex": body.hex()}
        if sid == 0 and ln >= 6:
            e["decoded"] = decode_devinfo(body)
        elif sid == 1 and ln == 6:
            e["decoded"] = ":".join(f"{x:02x}" for x in body)
        elif sid == 7 and ln >= 2:
            bm = int.from_bytes(body[:2], "big")
            e["decoded"] = {
                "uibc": bool(bm & 1),
                "i2c_rw": bool(bm & 2),
                "preferred_display_mode": bool(bm & 4),
                "standby_resume": bool(bm & 8),
                "tdls_persistent": bool(bm & 16),
                "tdls_persistent_bssid": bool(bm & 32),
                "raw": f"0x{bm:04x}",
            }
        elif sid == 6 and ln >= 1:
            e["decoded"] = {"status": body[0] & 3, "coupled_mac": body[1:7].hex(":") if ln >= 7 else None}
        out.append(e)
        i += 3 + ln
    return out
