#!/usr/bin/env python3
"""Find which SysEx-dump bytes a physical control changes.

Connects to the FLOW 8 over BLE, authenticates, then every --interval seconds
sends the 0x4B dump trigger and reads the SysEx dump from USB MIDI. Prints the
bytes that changed since the previous dump; bytes that keep changing on their
own (meters etc.) are muted after they've moved in --noisy dumps.
Turn the control between dumps (min, wait, max, wait...).

    python3 tools/dump_watch.py [--mac F2:1A:..] [--interval 4] [--count 15]
Dumps are saved as dump-NN.bin in the current directory.
"""
import argparse
import glob
import os
import select
import struct
import sys
import time
from collections import Counter

import dbus
import dbus.mainloop.glib
from gi.repository import GLib

sys.path.insert(0, os.path.dirname(__file__))
from ble_proxy import BUS, GATT_CHRC, PROPS, connect_mixer, find_adapter, find_mixer  # noqa: E402

AUTH = bytes.fromhex(os.environ.get("FLOW8_AUTH", "3901fd062b0639f17fe7b7278b8f355a495c2a"))  # default = src/service/ble.rs key
SESSION_START = bytes.fromhex("370138")
DUMP_TRIGGER = bytes.fromhex("4b014c")
DUMP_LEN = 3068  # full state dump, F0..F7


class Quiet:
    def note(self, text):
        print("###", text)


def flow8_rawmidi():
    for midi0 in glob.glob("/proc/asound/card*/midi0"):
        if "FLOW 8" in open(midi0).readline():
            return f"/dev/snd/midiC{midi0.split('/')[3][4:]}D0"
    sys.exit("FLOW 8 USB MIDI device not found")


def read_sysex(fd, timeout=5.0):
    buf, deadline = bytearray(), time.time() + timeout
    while time.time() < deadline:
        r, _, _ = select.select([fd], [], [], 0.2)
        if r:
            buf += os.read(fd, 4096)
            start = buf.rfind(0xF0)
            end = buf.find(0xF7, start) if start >= 0 else -1
            if end > 0:
                msg = bytes(buf[start:end + 1])
                if len(msg) == DUMP_LEN:
                    return msg
                # Short/odd message: another reader (e.g. the desktop app) is sharing the
                # USB MIDI input and stealing bytes. Never hand back a corrupt dump.
                print(f"  discarded {len(msg)}-byte SysEx (expected {DUMP_LEN}) — is another app reading USB MIDI?")
                del buf[:end + 1]
    return None


def pump(seconds):
    ctx, end = GLib.MainContext.default(), time.time() + seconds
    while time.time() < end:
        ctx.iteration(False)
        time.sleep(0.01)


def unpack(dump, start=3):
    """Undo MIDI 7-in-8 packing (header byte holds bit 7 of the next 7 bytes)."""
    out = bytearray()
    for i in range(start, len(dump) - 8, 8):
        out += bytes(dump[i + 1 + k] | (((dump[i] >> k) & 1) << 7) for k in range(7))
    return bytes(out)


def float_at(dump, idx):
    return struct.unpack("<f", unpack(dump)[idx:idx + 4])[0]


class Session:
    """Authenticated BLE session; dumps come back over USB MIDI."""

    def __init__(self, mac=None):
        self.midi_fd = os.open(flow8_rawmidi(), os.O_RDONLY | os.O_NONBLOCK)
        dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
        bus = dbus.SystemBus()
        dev_path, props = find_mixer(bus, find_adapter(bus), mac)
        # Already connected (e.g. the desktop app owns the link): share it — BlueZ multiplexes
        # GATT clients — and leave auth and the connection itself alone.
        self.shared = bool(props.get("Connected"))
        self.dev, chr_path, _ = connect_mixer(bus, dev_path, Quiet())
        self.chrc = dbus.Interface(bus.get_object(BUS, chr_path), GATT_CHRC)
        self.seen = []
        bus.add_signal_receiver(
            lambda i, c, _inv: self.seen.append(bytes(c["Value"])) if "Value" in c else None,
            "PropertiesChanged", PROPS, BUS, chr_path)
        self.chrc.StartNotify()
        if self.shared:
            print("sharing existing BLE link (already authenticated)")
            return
        pump(1.0)  # writing straight after connect fails with ATT 0x0e; src/service/ble.rs waits 1 s too
        self.write(AUTH)
        pump(1.0)
        if not any(p[:1] == b"\x36" for p in self.seen):
            self.close()
            sys.exit(f"no auth ack — set FLOW8_AUTH to this unit's key. got: {[p.hex() for p in self.seen[-3:]]}")
        print("authenticated")
        self.write(SESSION_START)
        pump(2.0)  # let the 0x38 state chunks drain

    def write(self, b):
        self.chrc.WriteValue(dbus.Array(b, "y"), {"type": "request"})

    def dump(self):
        if select.select([self.midi_fd], [], [], 0)[0]:
            os.read(self.midi_fd, 65536)
        self.write(DUMP_TRIGGER)
        return read_sysex(self.midi_fd)

    def close(self):
        if self.shared:
            return
        try:
            self.chrc.StopNotify()
            self.dev.Disconnect()
        except dbus.DBusException:
            pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--mac")
    ap.add_argument("--interval", type=float, default=4)
    ap.add_argument("--count", type=int, default=15)
    ap.add_argument("--noisy", type=int, default=3)
    args = ap.parse_args()

    s = Session(args.mac)
    try:
        prev, moved = None, Counter()
        for n in range(args.count):
            dump = s.dump()
            if not dump:
                print(f"[{n:02}] no SysEx arrived over USB")
            else:
                open(f"dump-{n:02}.bin", "wb").write(dump)
                if prev and len(prev) == len(dump):
                    diff = [i for i in range(len(dump)) if dump[i] != prev[i]]
                    moved.update(diff)
                    shown = [f"0x{i:04x}:{prev[i]:02x}>{dump[i]:02x}" for i in diff if moved[i] <= args.noisy]
                    print(f"[{n:02}] {len(dump)} bytes, {len(diff)} changed: {' '.join(shown) or '-'}")
                else:
                    print(f"[{n:02}] {len(dump)} bytes (baseline)")
                prev = dump
            pump(args.interval)
    finally:
        s.close()


if __name__ == "__main__":
    main()
