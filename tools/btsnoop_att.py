#!/usr/bin/env python3
"""Print FLOW 8 BLE traffic (ATT writes/notifications) from an Android btsnoop_hci.log.

Get the log: Developer options -> "Enable Bluetooth HCI snoop log" = Enabled,
toggle Bluetooth, use the app, then `adb bugreport x.zip` and unzip
FS/data/misc/bluetooth/logs/btsnoop_hci.log. Timestamps are UTC.

    python3 tools/btsnoop_att.py btsnoop_hci.log [--all]   (--all keeps 0x22 metering + echoes)
"""
import datetime
import struct
import sys

EPOCH = datetime.datetime(1, 1, 1)  # btsnoop timestamps: microseconds since year 0 (AD 1 in Python)


def att_values(path):
    d = open(path, "rb").read()
    assert d[:8] == b"btsnoop\0", "not a btsnoop file"
    off = 16
    while off + 24 <= len(d):
        orig, incl, flags, _drops, ts = struct.unpack(">IIIIq", d[off:off + 24])
        p = d[off + 24:off + 24 + incl]
        off += 24 + incl
        # H4 ACL -> L2CAP CID 4 (ATT); Write Req 0x12 / Write Cmd 0x52 / Notify 0x1b
        if len(p) < 12 or p[0] != 2 or struct.unpack("<H", p[7:9])[0] != 4 or p[9] not in (0x12, 0x52, 0x1b):
            continue
        t = EPOCH + datetime.timedelta(microseconds=ts) - datetime.timedelta(days=366)
        yield t, "MIX>PH" if flags & 1 else "PH>MIX", p[12:], incl < orig


def main():
    show_all = "--all" in sys.argv
    last = None
    for t, direction, v, truncated in att_values(sys.argv[1]):
        if not show_all and (direction == "MIX>PH" and v[:1] == b"\x22" or (direction, v) == last):
            continue
        last = (direction, v)
        print(f"{t:%H:%M:%S.%f}"[:-3], direction, v.hex(" "), "(TRUNCATED)" if truncated else "")


if __name__ == "__main__":
    main()
